#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, IntoVal, Symbol,
};

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AuctionError {
    EndTimeMustBeAfterStartTime = 1,
    AuctionNotFound = 2,
    EnglishOnly = 3,
    AuctionAlreadySettled = 4,
    AuctionNotStarted = 5,
    AuctionEnded = 6,
    BidTooLow = 7,
    DutchOnly = 8,
    CurrentPriceHigherThanMax = 9,
    AuctionStillOngoing = 10,
    SealedBidOnly = 11,
    BiddingPeriodEnded = 12,
    RevealPeriodNotStarted = 13,
    RevealPeriodEnded = 14,
    BidAlreadyRevealed = 15,
    InvalidBidHash = 16,
    BidsNotRevealed = 17,
}

fn panic_with_error(env: &Env, err: AuctionError) -> ! {
    env.events().publish(("auction_error",), err as u32);
    panic!("auction contract error");
}

// 1. DATA STRUCTURES
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AuctionType {
    English = 1,
    Dutch = 2,
    SealedBid = 3,
}

// NEW: Grouping settings to avoid the 10-parameter limit
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuctionSettings {
    pub start_time: u64,
    pub end_time: u64,
    pub starting_price: i128,
    pub reserve_price: i128,
    pub buy_now_price: i128,
    pub min_bid_increment: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuctionInfo {
    pub auction_id: u64,
    pub seller: Address,
    pub nft_contract: Address,
    pub nft_id: u64,
    pub payment_token: Address,
    pub auction_type: AuctionType,
    // We flatten the settings into the info for easier reading later
    pub settings: AuctionSettings,
    pub highest_bidder: Option<Address>,
    pub current_bid: i128,
    pub settled: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bid {
    pub bidder: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuctionAnalytics {
    pub total_bids: u32,
    pub unique_bidders: u32,
    pub bid_count_by_bidder: soroban_sdk::Map<Address, u32>,
    pub created_at: u64,
    pub settled_at: Option<u64>,
}

#[contracttype]
pub enum DataKey {
    Auction(u64),
    AuctionCount,
    AuctionBids(u64), // Stores Vec<Bid> for each auction
    AuctionAnalytics(u64), // Stores analytics for each auction
}

// 2. CONTRACT LOGIC
#[contract]
pub struct AuctionContract;

#[contractimpl]
impl AuctionContract {
    /// Initialize the contract
    pub fn init(env: Env, _admin: Address) {
        if !env.storage().instance().has(&DataKey::AuctionCount) {
            env.storage().instance().set(&DataKey::AuctionCount, &0u64);
        }
    }

    /// Creates a new auction
    pub fn create_auction(
        env: Env,
        seller: Address,
        nft_contract: Address,
        nft_id: u64,
        payment_token: Address,
        auction_type: AuctionType,
        settings: AuctionSettings, // <--- Grouped arguments here
    ) -> u64 {
        seller.require_auth();

        if settings.end_time <= settings.start_time {
            panic_with_error(&env, AuctionError::EndTimeMustBeAfterStartTime);
        }

        // Generate ID
        let mut id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::AuctionCount)
            .unwrap_or(0);
        id += 1;
        env.storage().instance().set(&DataKey::AuctionCount, &id);

        // Create Auction Object
        let auction = AuctionInfo {
            auction_id: id,
            seller,
            nft_contract,
            nft_id,
            payment_token,
            auction_type,
            settings, // Save the grouped settings
            highest_bidder: None,
            current_bid: 0,
            settled: false,
        };

        // Save
        env.storage()
            .instance()
            .set(&DataKey::Auction(id), &auction);

        id
    }

    /// Place a bid on an English Auction
    pub fn place_bid(env: Env, bidder: Address, auction_id: u64, bid_amount: i128) {
        // 1. Auth check
        bidder.require_auth();

        // 2. Load the auction
        // We assume the auction exists (unwrap panics if it doesn't)
        let mut auction: AuctionInfo = env
            .storage()
            .instance()
            .get(&DataKey::Auction(auction_id))
            .unwrap_or_else(|| panic_with_error(&env, AuctionError::AuctionNotFound));

        // 3. Validation Checks
        if auction.auction_type != AuctionType::English {
            panic_with_error(&env, AuctionError::EnglishOnly);
        }
        if auction.settled {
            panic_with_error(&env, AuctionError::AuctionAlreadySettled);
        }

        let current_time = env.ledger().timestamp();
        if current_time < auction.settings.start_time {
            panic_with_error(&env, AuctionError::AuctionNotStarted);
        }
        if current_time > auction.settings.end_time {
            panic_with_error(&env, AuctionError::AuctionEnded);
        }

        // 4. Price Logic & Refunds
        let token_client = token::Client::new(&env, &auction.payment_token);

        if let Some(previous_bidder) = auction.highest_bidder {
            // CASE A: Outbidding someone
            // Check if bid is high enough (Current Bid + Increment)
            if bid_amount < auction.current_bid + auction.settings.min_bid_increment {
                panic_with_error(&env, AuctionError::BidTooLow);
            }

            // Refund the previous bidder!
            // We send the money held in the contract back to them.
            token_client.transfer(
                &env.current_contract_address(),
                &previous_bidder,
                &auction.current_bid,
            );
        } else {
            // CASE B: First bid of the auction
            if bid_amount < auction.settings.starting_price {
                panic_with_error(&env, AuctionError::BidTooLow);
            }
        }

        // 5. Take Payment (Escrow)
        // Pull the new bid amount from the bidder to the contract
        token_client.transfer(&bidder, &env.current_contract_address(), &bid_amount);

        // 6. Anti-Sniping (Extension)
        // If bid is placed in the last 5 minutes (300 seconds), extend end time by 5 mins.
        let time_remaining = auction.settings.end_time - current_time;
        if time_remaining < 300 {
            auction.settings.end_time = current_time + 300;
        }

        // 7. Update State & Save
        auction.highest_bidder = Some(bidder);
        auction.current_bid = bid_amount;

        env.storage()
            .instance()
            .set(&DataKey::Auction(auction_id), &auction);
    }

    /// Helper: Calculate the current price for a Dutch Auction
    /// Formula: StartPrice - (DecayPerSecond * SecondsElapsed)
    fn calculate_dutch_price(settings: &AuctionSettings, current_time: u64) -> i128 {
        if current_time <= settings.start_time {
            return settings.starting_price;
        }
        if current_time >= settings.end_time {
            return settings.reserve_price;
        }

        let total_duration = settings.end_time - settings.start_time;
        let elapsed = current_time - settings.start_time;

        // Price difference between Start and Reserve (Floor)
        let price_drop_range = settings.starting_price - settings.reserve_price;

        // Calculate how much price has dropped so far
        // Note: We cast to i128 to avoid overflow, then divide
        let drop_amount = (price_drop_range * elapsed as i128) / total_duration as i128;

        settings.starting_price - drop_amount
    }

    /// Buy immediately in a Dutch Auction
    pub fn buy_dutch(env: Env, buyer: Address, auction_id: u64, max_amount: i128) {
        buyer.require_auth();

        let mut auction: AuctionInfo = env
            .storage()
            .instance()
            .get(&DataKey::Auction(auction_id))
            .unwrap_or_else(|| panic_with_error(&env, AuctionError::AuctionNotFound));

        // 1. Validation
        if auction.auction_type != AuctionType::Dutch {
            panic_with_error(&env, AuctionError::DutchOnly);
        }
        if auction.settled {
            panic_with_error(&env, AuctionError::AuctionAlreadySettled);
        }

        // 2. Calculate Price
        let current_time = env.ledger().timestamp();
        let current_price = Self::calculate_dutch_price(&auction.settings, current_time);

        // 3. Check Price Acceptability
        // The buyer says "I will pay up to X". If current price is higher, fail.
        if current_price > max_amount {
            panic_with_error(&env, AuctionError::CurrentPriceHigherThanMax);
        }

        // 4. Process Payment
        // Buyer pays the calculated CURRENT price (not their max)
        let token_client = token::Client::new(&env, &auction.payment_token);
        token_client.transfer(&buyer, &env.current_contract_address(), &current_price);

        // 5. End the Auction Immediately
        auction.highest_bidder = Some(buyer);
        auction.current_bid = current_price;
        auction.settled = true; // Dutch auctions end instantly

        env.storage()
            .instance()
            .set(&DataKey::Auction(auction_id), &auction);
    }

    /// Finalize the auction (Send money to seller, NFT to winner)
    pub fn settle_auction(env: Env, auction_id: u64) {
        let mut auction: AuctionInfo = env
            .storage()
            .instance()
            .get(&DataKey::Auction(auction_id))
            .unwrap_or_else(|| panic_with_error(&env, AuctionError::AuctionNotFound));

        if auction.settled {
            panic_with_error(&env, AuctionError::AuctionAlreadySettled);
        }

        // For English auctions, ensure time has passed
        if auction.auction_type == AuctionType::English {
            if env.ledger().timestamp() < auction.settings.end_time {
                panic_with_error(&env, AuctionError::AuctionStillOngoing);
            }
        }

        // If there is a winner...
        if let Some(winner) = auction.highest_bidder.clone() {
            // 1. Pay the Seller
            let token_client = token::Client::new(&env, &auction.payment_token);
            token_client.transfer(
                &env.current_contract_address(),
                &auction.seller,
                &auction.current_bid,
            );

            // 2. Transfer the NFT
            // We invoke the NFT contract's "transfer" function dynamically.
            // Args: (from, to, token_id)
            let transfer_args = (auction.seller.clone(), winner, auction.nft_id);

            env.invoke_contract::<()>(
                &auction.nft_contract,
                &Symbol::new(&env, "transfer"),
                transfer_args.into_val(&env),
            );
        }

        // Mark as settled so it can't be processed again
        auction.settled = true;
        env.storage()
            .instance()
            .set(&DataKey::Auction(auction_id), &auction);
    }

    /// Helper to fetch auction data
    pub fn get_auction(env: Env, auction_id: u64) -> Option<AuctionInfo> {
        env.storage().instance().get(&DataKey::Auction(auction_id))
    }
}

#[cfg(test)]
mod test;