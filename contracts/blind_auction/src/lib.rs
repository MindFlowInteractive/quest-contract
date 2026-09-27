#![no_std]
use soroban_sdk::{
    contract, contractimpl, symbol_short, token, Address, Bytes, BytesN, Env, IntoVal, Symbol,
    Vec,
};

mod storage;
mod types;

use storage::Storage;
use types::{
    Auction, AuctionStats, BlindAuctionError, Commitment, CommitmentInfo, Config, GlobalStats,
    Winner,
};

/// Sealed-bid (blind) NFT auction.
///
/// Lifecycle
/// ---------
/// 1. `create_auction` - the seller (or the admin) publishes an NFT together
///    with a reserve price, a bid deposit and a two phase timeline.
/// 2. `commit_bid` - during the commit window bidders submit a commitment
///    (`sha256(auction_id || amount || salt)`) and escrow the bid deposit. The
///    plaintext amount never reaches the chain.
/// 3. `reveal_bid` - during the reveal window bidders publish `(amount, salt)`.
///    The contract recomputes the hash and rejects mismatches, then escrows the
///    remainder of the bid so the full amount is held before settlement.
/// 4. `close_auction` - after the reveal window the highest valid revealed bid
///    wins, provided it clears the reserve. Ties are broken deterministically
///    in favour of the earliest commit.
/// 5. `withdraw_proceeds` / `transfer_nft` - the seller collects the escrowed
///    winning bid and hands the NFT to the winner.
/// 6. `claim_refund` - every losing bidder (or every bidder on a cancelled
///    auction) reclaims their escrow exactly once.
#[contract]
pub struct BlindAuction;

#[contractimpl]
impl BlindAuction {
    /// One time setup. Stores the admin plus the global analytics counters.
    pub fn initialize(env: Env, admin: Address) -> Result<(), BlindAuctionError> {
        if Storage::has_config(&env) {
            return Err(BlindAuctionError::AlreadyInitialized);
        }

        Storage::set_config(&env, &Config { admin });

        if !Storage::has_stats(&env) {
            Storage::set_stats(
                &env,
                &GlobalStats {
                    total_auctions: 0,
                    total_bids: 0,
                    total_reveals: 0,
                    closed_count: 0,
                    cancelled_count: 0,
                    auctions_with_winner: 0,
                    total_volume: 0,
                    highest_bid: 0,
                    total_escrowed: 0,
                    total_refunded: 0,
                    outstanding_escrow: 0,
                },
            );
        }

        Ok(())
    }

    /// Publishes a new blind auction and returns its id.
    #[allow(clippy::too_many_arguments)]
    pub fn create_auction(
        env: Env,
        seller: Address,
        nft_contract: Address,
        nft_id: BytesN<32>,
        payment_token: Address,
        reserve_price: i128,
        bid_deposit: i128,
        commit_deadline: u64,
        reveal_deadline: u64,
    ) -> Result<u64, BlindAuctionError> {
        // Fails with `NotInitialized` when the contract has not been set up yet.
        Storage::get_config(&env)?;
        seller.require_auth();

        if reserve_price <= 0 {
            return Err(BlindAuctionError::InvalidReserve);
        }
        if bid_deposit <= 0 {
            return Err(BlindAuctionError::InvalidDeposit);
        }

        let now = env.ledger().timestamp();
        if commit_deadline <= now || reveal_deadline <= commit_deadline {
            return Err(BlindAuctionError::InvalidDeadlines);
        }
        if Storage::has_active_nft(&env, &nft_id) {
            return Err(BlindAuctionError::AuctionAlreadyExists);
        }

        let auction_id = Storage::next_auction_id(&env);

        let auction = Auction {
            auction_id,
            seller: seller.clone(),
            nft_contract,
            nft_id: nft_id.clone(),
            payment_token,
            reserve_price,
            bid_deposit,
            commit_deadline,
            reveal_deadline,
            created_at: now,
            bid_count: 0,
            reveal_count: 0,
            highest_bid: 0,
            winning_amount: 0,
            escrow_balance: 0,
            total_refunded: 0,
            winner: None,
            closed_at: None,
            closed: false,
            cancelled: false,
            proceeds_paid: false,
            nft_transferred: false,
        };

        Storage::set_auction(&env, &auction);
        Storage::init_bidders(&env, auction_id);
        Storage::set_active_nft(&env, &nft_id, &auction_id);

        let mut stats = Storage::get_stats(&env)?;
        stats.total_auctions += 1;
        Storage::set_stats(&env, &stats);

        env.events()
            .publish((symbol_short!("created"), auction_id), (seller, nft_id));

        Ok(auction_id)
    }

    /// Phase 1: escrow the bid deposit and store a hash of `(amount, salt)`.
    pub fn commit_bid(
        env: Env,
        auction_id: u64,
        bidder: Address,
        commitment: BytesN<32>,
        deposit: i128,
    ) -> Result<(), BlindAuctionError> {
        bidder.require_auth();

        let mut auction = Storage::get_auction(&env, auction_id)?;
        if auction.cancelled {
            return Err(BlindAuctionError::AuctionCancelled);
        }
        if auction.closed {
            return Err(BlindAuctionError::AuctionAlreadyClosed);
        }
        if env.ledger().timestamp() > auction.commit_deadline {
            return Err(BlindAuctionError::CommitPeriodClosed);
        }
        if deposit != auction.bid_deposit {
            return Err(BlindAuctionError::InvalidDeposit);
        }
        if Storage::has_commitment(&env, auction_id, &bidder) {
            return Err(BlindAuctionError::BidAlreadyCommitted);
        }

        // Escrow the deposit: it is refundable if the bid is never revealed or
        // does not win, and it is credited towards the winning bid amount.
        let payment = token::Client::new(&env, &auction.payment_token);
        payment.transfer(
            &bidder,
            &env.current_contract_address(),
            &auction.bid_deposit,
        );

        let record = Commitment {
            bidder: bidder.clone(),
            auction_id,
            commitment,
            committed_at: env.ledger().timestamp(),
            revealed_amount: None,
            revealed_at: None,
            escrow_amount: auction.bid_deposit,
            refunded: false,
        };
        Storage::set_commitment(&env, auction_id, &bidder, &record);
        Storage::push_bidder(&env, auction_id, &bidder);

        auction.bid_count += 1;
        auction.escrow_balance += auction.bid_deposit;
        Storage::set_auction(&env, &auction);

        let mut stats = Storage::get_stats(&env)?;
        stats.total_bids += 1;
        stats.total_escrowed += auction.bid_deposit;
        stats.outstanding_escrow += auction.bid_deposit;
        Storage::set_stats(&env, &stats);

        env.events().publish(
            (symbol_short!("committed"), auction_id, bidder),
            auction.bid_deposit,
        );

        Ok(())
    }

    /// Phase 2: open the sealed bid by publishing its salt.
    pub fn reveal_bid(
        env: Env,
        auction_id: u64,
        bidder: Address,
        amount: i128,
        salt: BytesN<32>,
    ) -> Result<(), BlindAuctionError> {
        bidder.require_auth();

        let mut auction = Storage::get_auction(&env, auction_id)?;
        if auction.cancelled {
            return Err(BlindAuctionError::AuctionCancelled);
        }
        if auction.closed {
            return Err(BlindAuctionError::AuctionAlreadyClosed);
        }

        let now = env.ledger().timestamp();
        if now <= auction.commit_deadline {
            return Err(BlindAuctionError::RevealPeriodNotStarted);
        }
        if now > auction.reveal_deadline {
            return Err(BlindAuctionError::RevealPeriodClosed);
        }
        if amount <= 0 {
            return Err(BlindAuctionError::InvalidBidAmount);
        }

        let mut record = Storage::get_commitment(&env, auction_id, &bidder)?;
        if record.revealed_amount.is_some() {
            return Err(BlindAuctionError::BidAlreadyRevealed);
        }

        // Recompute the commitment; this is the "bids stay secret" guarantee.
        let computed = Self::bid_hash(&env, auction_id, amount, &salt);
        if computed != record.commitment {
            return Err(BlindAuctionError::CommitmentMismatch);
        }

        // Top the escrow up to the full bid so that the contract always holds
        // the whole amount of every valid bid by the time it settles.
        let top_up = if amount > record.escrow_amount {
            amount - record.escrow_amount
        } else {
            0
        };
        if top_up > 0 {
            let payment = token::Client::new(&env, &auction.payment_token);
            payment.transfer(&bidder, &env.current_contract_address(), &top_up);
            record.escrow_amount = amount;
            auction.escrow_balance += top_up;
        }

        record.revealed_amount = Some(amount);
        record.revealed_at = Some(now);
        Storage::set_commitment(&env, auction_id, &bidder, &record);

        auction.reveal_count += 1;
        Storage::set_auction(&env, &auction);

        let mut stats = Storage::get_stats(&env)?;
        stats.total_reveals += 1;
        stats.total_escrowed += top_up;
        stats.outstanding_escrow += top_up;
        Storage::set_stats(&env, &stats);

        env.events()
            .publish((symbol_short!("revealed"), auction_id, bidder), amount);

        Ok(())
    }

    /// Read-only preview of the bid that would win, available once the reveal
    /// window has closed.
    pub fn determine_winner(env: Env, auction_id: u64) -> Result<Option<Winner>, BlindAuctionError> {
        let auction = Storage::get_auction(&env, auction_id)?;
        if auction.cancelled {
            return Err(BlindAuctionError::AuctionCancelled);
        }
        if env.ledger().timestamp() <= auction.reveal_deadline {
            return Err(BlindAuctionError::RevealNotEnded);
        }

        Ok(Self::pick_winner(&env, &auction))
    }

    /// Settles the auction and records the winner.
    ///
    /// The auction can only be closed once; the highest revealed bid wins and
    /// ties are resolved in favour of the bidder that committed first.
    pub fn close_auction(env: Env, auction_id: u64) -> Result<Option<Winner>, BlindAuctionError> {
        let mut auction = Storage::get_auction(&env, auction_id)?;
        if auction.cancelled {
            return Err(BlindAuctionError::AuctionCancelled);
        }
        if auction.closed {
            return Err(BlindAuctionError::AuctionAlreadyClosed);
        }

        let now = env.ledger().timestamp();
        if now <= auction.reveal_deadline {
            return Err(BlindAuctionError::RevealNotEnded);
        }

        let best = Self::pick_winner(&env, &auction);
        if let Some(b) = &best {
            auction.highest_bid = b.amount;
        }

        // The auction only has a winner when the reserve has been cleared.
        let winner = match best {
            Some(w) if w.amount >= auction.reserve_price => Some(w),
            _ => None,
        };

        if let Some(w) = &winner {
            auction.winner = Some(w.bidder.clone());
            auction.winning_amount = w.amount;
        }
        auction.closed = true;
        auction.closed_at = Some(now);
        Storage::set_auction(&env, &auction);
        Storage::clear_active_nft(&env, &auction.nft_id);

        let mut stats = Storage::get_stats(&env)?;
        stats.closed_count += 1;
        if let Some(w) = &winner {
            stats.auctions_with_winner += 1;
            stats.total_volume += w.amount;
            if w.amount > stats.highest_bid {
                stats.highest_bid = w.amount;
            }
        }
        Storage::set_stats(&env, &stats);

        env.events().publish(
            (symbol_short!("settled"), auction_id),
            (auction.winner.clone(), auction.winning_amount),
        );

        Ok(winner)
    }

    /// Pays the escrowed winning bid to the seller. Callable by the seller or
    /// by the admin, and only once.
    pub fn withdraw_proceeds(
        env: Env,
        auction_id: u64,
        seller: Address,
    ) -> Result<i128, BlindAuctionError> {
        seller.require_auth();

        let config = Storage::get_config(&env)?;
        let mut auction = Storage::get_auction(&env, auction_id)?;

        if seller != auction.seller && seller != config.admin {
            return Err(BlindAuctionError::Unauthorized);
        }
        if auction.cancelled {
            return Err(BlindAuctionError::AuctionCancelled);
        }
        if !auction.closed {
            return Err(BlindAuctionError::RevealNotEnded);
        }
        if auction.proceeds_paid {
            return Err(BlindAuctionError::ProceedsAlreadyWithdrawn);
        }
        if auction.winner.is_none() {
            return Err(BlindAuctionError::NoWinningBid);
        }

        let amount = auction.winning_amount;
        if amount <= 0 || auction.escrow_balance < amount {
            return Err(BlindAuctionError::NothingToWithdraw);
        }

        let payment = token::Client::new(&env, &auction.payment_token);
        payment.transfer(
            &env.current_contract_address(),
            &seller,
            &amount,
        );

        auction.escrow_balance -= amount;
        auction.proceeds_paid = true;
        Storage::set_auction(&env, &auction);

        let mut stats = Storage::get_stats(&env)?;
        stats.outstanding_escrow -= amount;
        Storage::set_stats(&env, &stats);

        env.events()
            .publish((symbol_short!("proceeds"), auction_id, seller), amount);

        Ok(amount)
    }

    /// Hands the escrowed NFT to the winner. Callable by the seller only, and
    /// only once the auction has been settled with a winner.
    pub fn transfer_nft(
        env: Env,
        auction_id: u64,
        seller: Address,
    ) -> Result<(), BlindAuctionError> {
        seller.require_auth();

        let mut auction = Storage::get_auction(&env, auction_id)?;
        if seller != auction.seller {
            return Err(BlindAuctionError::Unauthorized);
        }
        if !auction.closed {
            return Err(BlindAuctionError::RevealNotEnded);
        }
        if auction.nft_transferred {
            return Err(BlindAuctionError::NftAlreadyTransferred);
        }

        let winner = match auction.winner.clone() {
            Some(w) => w,
            None => return Err(BlindAuctionError::NoWinningBid),
        };

        env.invoke_contract::<()>(
            &auction.nft_contract,
            &Symbol::new(&env, "transfer"),
            (
                auction.seller.clone(),
                winner.clone(),
                auction.nft_id.clone(),
            )
                .into_val(&env),
        );

        auction.nft_transferred = true;
        Storage::set_auction(&env, &auction);

        env.events().publish(
            (symbol_short!("nft_sent"), auction_id, winner),
            auction.nft_id,
        );

        Ok(())
    }

    /// Returns a bidder's escrowed funds. Available once the auction is closed
    /// or cancelled, and never for the winner. Idempotent: a second claim
    /// fails instead of paying twice.
    pub fn claim_refund(
        env: Env,
        auction_id: u64,
        bidder: Address,
    ) -> Result<i128, BlindAuctionError> {
        bidder.require_auth();

        let mut auction = Storage::get_auction(&env, auction_id)?;
        if !auction.closed && !auction.cancelled {
            return Err(BlindAuctionError::RefundNotAvailable);
        }
        if auction.winner == Some(bidder.clone()) {
            return Err(BlindAuctionError::NotTheWinner);
        }

        let mut record = Storage::get_commitment(&env, auction_id, &bidder)?;
        if record.refunded {
            return Err(BlindAuctionError::AlreadyRefunded);
        }
        if record.escrow_amount <= 0 {
            return Err(BlindAuctionError::RefundNotAvailable);
        }

        let amount = record.escrow_amount;
        let payment = token::Client::new(&env, &auction.payment_token);
        payment.transfer(
            &env.current_contract_address(),
            &bidder,
            &amount,
        );

        record.refunded = true;
        record.escrow_amount = 0;
        Storage::set_commitment(&env, auction_id, &bidder, &record);

        auction.escrow_balance -= amount;
        auction.total_refunded += amount;
        Storage::set_auction(&env, &auction);

        let mut stats = Storage::get_stats(&env)?;
        stats.total_refunded += amount;
        stats.outstanding_escrow -= amount;
        Storage::set_stats(&env, &stats);

        env.events()
            .publish((symbol_short!("refund"), auction_id, bidder), amount);

        Ok(amount)
    }

    /// Cancels an auction before the commit window closes, making every
    /// deposit immediately refundable.
    pub fn cancel_auction(
        env: Env,
        auction_id: u64,
        caller: Address,
    ) -> Result<(), BlindAuctionError> {
        caller.require_auth();

        let config = Storage::get_config(&env)?;
        let mut auction = Storage::get_auction(&env, auction_id)?;

        if caller != auction.seller && caller != config.admin {
            return Err(BlindAuctionError::Unauthorized);
        }
        if auction.closed {
            return Err(BlindAuctionError::AuctionAlreadyClosed);
        }
        if auction.cancelled {
            return Err(BlindAuctionError::AuctionCancelled);
        }
        if env.ledger().timestamp() > auction.commit_deadline {
            return Err(BlindAuctionError::CancelWindowClosed);
        }

        auction.cancelled = true;
        Storage::set_auction(&env, &auction);
        Storage::clear_active_nft(&env, &auction.nft_id);

        let mut stats = Storage::get_stats(&env)?;
        stats.cancelled_count += 1;
        Storage::set_stats(&env, &stats);

        env.events()
            .publish((symbol_short!("cancel"), auction_id), caller);

        Ok(())
    }

    // ----- getters ---------------------------------------------------------

    pub fn get_auction(env: Env, auction_id: u64) -> Result<Auction, BlindAuctionError> {
        Storage::get_auction(&env, auction_id)
    }

    /// Returns the stored commitment. The plaintext bid amount is never part
    /// of this projection.
    pub fn get_commitment(
        env: Env,
        auction_id: u64,
        bidder: Address,
    ) -> Result<CommitmentInfo, BlindAuctionError> {
        let record = Storage::get_commitment(&env, auction_id, &bidder)?;

        Ok(CommitmentInfo {
            bidder: record.bidder,
            auction_id: record.auction_id,
            commitment: record.commitment,
            committed_at: record.committed_at,
            revealed: record.revealed_amount.is_some(),
            refunded: record.refunded,
        })
    }

    /// Returns a revealed bid amount. Fails with `BidNotRevealed` while the bid
    /// is still sealed, which is what keeps bids secret.
    pub fn get_revealed_bid(
        env: Env,
        auction_id: u64,
        bidder: Address,
    ) -> Result<i128, BlindAuctionError> {
        let _auction = Storage::get_auction(&env, auction_id)?;
        let record = Storage::get_commitment(&env, auction_id, &bidder)?;

        match record.revealed_amount {
            Some(amount) => Ok(amount),
            None => Err(BlindAuctionError::BidNotRevealed),
        }
    }

    pub fn get_bid_count(env: Env, auction_id: u64) -> Result<u32, BlindAuctionError> {
        let auction = Storage::get_auction(&env, auction_id)?;
        Ok(auction.bid_count)
    }

    pub fn get_bidders(env: Env, auction_id: u64) -> Result<Vec<Address>, BlindAuctionError> {
        Storage::get_auction(&env, auction_id)?;
        Ok(Storage::get_bidders(&env, auction_id))
    }

    /// The settled winner, or `None` when the reserve was not met.
    pub fn get_winner(env: Env, auction_id: u64) -> Result<Option<Winner>, BlindAuctionError> {
        let auction = Storage::get_auction(&env, auction_id)?;
        if !auction.closed {
            return Err(BlindAuctionError::RevealNotEnded);
        }

        let winner = match auction.winner.clone() {
            Some(w) => w,
            None => return Ok(None),
        };
        let record = Storage::get_commitment(&env, auction_id, &winner)?;

        Ok(Some(Winner {
            bidder: winner,
            amount: auction.winning_amount,
            committed_at: record.committed_at,
            revealed_at: record.revealed_at.unwrap_or(0),
        }))
    }

    /// Funds currently held by the contract on behalf of an auction.
    pub fn get_escrow_balance(env: Env, auction_id: u64) -> Result<i128, BlindAuctionError> {
        let auction = Storage::get_auction(&env, auction_id)?;
        Ok(auction.escrow_balance)
    }

    /// Contract wide analytics.
    pub fn get_analytics(env: Env) -> Result<GlobalStats, BlindAuctionError> {
        Storage::get_stats(&env)
    }

    /// Analytics for a single auction, derived from stored state.
    pub fn get_auction_stats(env: Env, auction_id: u64) -> Result<AuctionStats, BlindAuctionError> {
        let auction = Storage::get_auction(&env, auction_id)?;

        let commit_ratio_bps = if auction.bid_count == 0 {
            0
        } else {
            ((auction.reveal_count as u64 * 10_000) / auction.bid_count as u64) as u32
        };

        Ok(AuctionStats {
            auction_id,
            bid_count: auction.bid_count,
            reveal_count: auction.reveal_count,
            commit_ratio_bps,
            reserve_price: auction.reserve_price,
            highest_bid: auction.highest_bid,
            winning_amount: auction.winning_amount,
            escrow_balance: auction.escrow_balance,
            total_refunded: auction.total_refunded,
            winner: auction.winner,
            closed: auction.closed,
            cancelled: auction.cancelled,
            created_at: auction.created_at,
            closed_at: auction.closed_at,
            commit_window: auction.commit_deadline.saturating_sub(auction.created_at),
            reveal_window: auction.reveal_deadline.saturating_sub(auction.commit_deadline),
        })
    }

    // ----- helpers ---------------------------------------------------------

    /// The canonical sealed-bid preimage: `auction_id || amount || salt`.
    ///
    /// Both sides hash exactly these bytes, so a bidder must pick the salt
    /// before committing and keep it until the reveal window opens.
    fn bid_hash(env: &Env, auction_id: u64, amount: i128, salt: &BytesN<32>) -> BytesN<32> {
        let mut preimage = Bytes::new(env);
        preimage.append(&Bytes::from_array(env, &auction_id.to_be_bytes()));
        preimage.append(&Bytes::from_array(env, &amount.to_be_bytes()));
        preimage.append(&Bytes::from_array(env, &salt.to_array()));
        env.crypto().sha256(&preimage).into()
    }

    /// Highest valid revealed bid. Bidders are visited in commit order and the
    /// running best is only replaced on a *strictly* higher amount, so the
    /// earliest commit wins ties.
    fn pick_winner(env: &Env, auction: &Auction) -> Option<Winner> {
        let bidders = Storage::get_bidders(env, auction.auction_id);
        let mut best: Option<Winner> = None;

        for bidder in bidders.iter() {
            let record = match Storage::try_get_commitment(env, auction.auction_id, &bidder) {
                Some(r) => r,
                None => continue,
            };
            let amount = match record.revealed_amount {
                Some(a) => a,
                None => continue,
            };

            let is_better = match best {
                Some(ref current) => amount > current.amount,
                None => true,
            };

            if is_better {
                best = Some(Winner {
                    bidder,
                    amount,
                    committed_at: record.committed_at,
                    revealed_at: record.revealed_at.unwrap_or(0),
                });
            }
        }

        best
    }
}

mod test;
