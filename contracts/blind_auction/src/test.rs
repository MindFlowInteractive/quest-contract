#![cfg(test)]
extern crate std;
use super::*;
use soroban_sdk::{testutils::Address as _, testutils::Ledger, Address, Symbol};

/// Storage key for the mock NFT used to verify that the winner actually
/// receives the escrowed token.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
enum NftKey {
    Owner(BytesN<32>),
}

/// Minimal stand-in for a SEP-11 NFT collection.
#[contract]
pub struct MockNft;

#[contractimpl]
impl MockNft {
    pub fn mint(env: Env, to: Address, token_id: BytesN<32>) {
        env.storage().persistent().set(&NftKey::Owner(token_id), &to);
    }

    pub fn owner_of(env: Env, token_id: BytesN<32>) -> Address {
        env.storage()
            .persistent()
            .get(&NftKey::Owner(token_id))
            .unwrap()
    }

    pub fn transfer(env: Env, from: Address, to: Address, token_id: BytesN<32>) {
        from.require_auth();
        let current: Address = env
            .storage()
            .persistent()
            .get(&NftKey::Owner(token_id))
            .unwrap();
        if current != from {
            panic!("sender does not own the nft");
        }
        env.storage().persistent().set(&NftKey::Owner(token_id), &to);
    }
}

/// Mirrors the contract side sealed-bid preimage: `auction_id || amount || salt`.
fn commitment(env: &Env, auction_id: u64, amount: i128, salt: &BytesN<32>) -> BytesN<32> {
    let mut preimage = Bytes::new(env);
    preimage.append(&Bytes::from_array(env, &auction_id.to_be_bytes()));
    preimage.append(&Bytes::from_array(env, &amount.to_be_bytes()));
    preimage.append(&Bytes::from_array(env, &salt.to_array()));
    env.crypto().sha256(&preimage).into()
}

/// Deterministic 32 byte salt for a test bid.
fn salt(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

const START: u64 = 1_000;
const COMMIT_DEADLINE: u64 = 2_000;
const REVEAL_DEADLINE: u64 = 4_000;
const DEPOSIT: i128 = 10;
const RESERVE: i128 = 200;
const MINT: i128 = 1_000;

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let contract_id = env.register_contract(None, BlindAuction);
    let client = BlindAuctionClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    // Analytics are unavailable before initialization.
    let err = client.try_get_analytics().unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::NotInitialized));

    client.initialize(&admin);
    assert_eq!(client.get_analytics().total_auctions, 0);

    // Duplicate initialization is rejected.
    let err = client.try_initialize(&admin).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::AlreadyInitialized));
}

#[test]
fn test_create_auction() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let token_admin = Address::generate(&env);
    let payment = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    let nft_id = BytesN::from_array(&env, &[7u8; 32]);
    let nft = env.register_contract(None, MockNft);

    let contract_id = env.register_contract(None, BlindAuction);
    let client = BlindAuctionClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let seller = Address::generate(&env);
    client.initialize(&admin);

    let auction_id = client.create_auction(
        &seller,
        &nft,
        &nft_id,
        &payment,
        &RESERVE,
        &DEPOSIT,
        &COMMIT_DEADLINE,
        &REVEAL_DEADLINE,
    );
    assert_eq!(auction_id, 1);

    let auction = client.get_auction(&auction_id);
    assert_eq!(auction.auction_id, 1);
    assert_eq!(auction.seller, seller);
    assert_eq!(auction.nft_contract, nft);
    assert_eq!(auction.nft_id, nft_id);
    assert_eq!(auction.payment_token, payment);
    assert_eq!(auction.reserve_price, RESERVE);
    assert_eq!(auction.bid_deposit, DEPOSIT);
    assert_eq!(auction.commit_deadline, COMMIT_DEADLINE);
    assert_eq!(auction.reveal_deadline, REVEAL_DEADLINE);
    assert_eq!(auction.created_at, START);
    assert_eq!(auction.bid_count, 0);
    assert_eq!(auction.escrow_balance, 0);
    assert!(!auction.closed);
    assert!(!auction.cancelled);

    // Ids are handed out sequentially.
    let nft_id_2 = BytesN::from_array(&env, &[8u8; 32]);
    let second = client.create_auction(
        &seller,
        &nft,
        &nft_id_2,
        &payment,
        &RESERVE,
        &DEPOSIT,
        &COMMIT_DEADLINE,
        &REVEAL_DEADLINE,
    );
    assert_eq!(second, 2);

    // Duplicate: the same NFT cannot back two live auctions.
    let err = client
        .try_create_auction(
            &seller,
            &nft,
            &nft_id,
            &payment,
            &RESERVE,
            &DEPOSIT,
            &COMMIT_DEADLINE,
            &REVEAL_DEADLINE,
        )
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::AuctionAlreadyExists));

    // Invalid terms.
    let nft_id_3 = BytesN::from_array(&env, &[9u8; 32]);
    let err = client
        .try_create_auction(
            &seller,
            &nft,
            &nft_id_3,
            &payment,
            &0i128,
            &DEPOSIT,
            &COMMIT_DEADLINE,
            &REVEAL_DEADLINE,
        )
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::InvalidReserve));

    let err = client
        .try_create_auction(
            &seller,
            &nft,
            &nft_id_3,
            &payment,
            &RESERVE,
            &0i128,
            &COMMIT_DEADLINE,
            &REVEAL_DEADLINE,
        )
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::InvalidDeposit));

    // Commit deadline already in the past.
    let err = client
        .try_create_auction(
            &seller,
            &nft,
            &nft_id_3,
            &payment,
            &RESERVE,
            &DEPOSIT,
            &START,
            &REVEAL_DEADLINE,
        )
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::InvalidDeadlines));

    // Reveal window not after the commit window.
    let err = client
        .try_create_auction(
            &seller,
            &nft,
            &nft_id_3,
            &payment,
            &RESERVE,
            &DEPOSIT,
            &COMMIT_DEADLINE,
            &COMMIT_DEADLINE,
        )
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::InvalidDeadlines));

    // Unknown auctions.
    let err = client.try_get_auction(&999).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::AuctionNotFound));
}

#[test]
fn test_create_auction_requires_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let payment = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let nft = env.register_contract(None, MockNft);
    let nft_id = BytesN::from_array(&env, &[1u8; 32]);
    let seller = Address::generate(&env);

    let contract_id = env.register_contract(None, BlindAuction);
    let client = BlindAuctionClient::new(&env, &contract_id);

    let err = client
        .try_create_auction(
            &seller,
            &nft,
            &nft_id,
            &payment,
            &RESERVE,
            &DEPOSIT,
            &COMMIT_DEADLINE,
            &REVEAL_DEADLINE,
        )
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::NotInitialized));
}

#[test]
fn test_commit_phase() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let token_admin = Address::generate(&env);
    let payment = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let asset = token::StellarAssetClient::new(&env, &payment);
    let wallet = token::Client::new(&env, &payment);

    let nft_id = BytesN::from_array(&env, &[3u8; 32]);
    let nft = env.register_contract(None, MockNft);

    let contract_id = env.register_contract(None, BlindAuction);
    let client = BlindAuctionClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let seller = Address::generate(&env);
    let bidder = Address::generate(&env);
    client.initialize(&admin);
    asset.mint(&bidder, &MINT);

    let auction_id = client.create_auction(
        &seller,
        &nft,
        &nft_id,
        &payment,
        &RESERVE,
        &DEPOSIT,
        &COMMIT_DEADLINE,
        &REVEAL_DEADLINE,
    );

    let sealed = commitment(&env, auction_id, 300, &salt(&env, 1));

    // The deposit must match the auction terms exactly.
    let err = client
        .try_commit_bid(&auction_id, &bidder, &sealed, &1i128)
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::InvalidDeposit));

    client.commit_bid(&auction_id, &bidder, &sealed, &DEPOSIT);

    assert_eq!(client.get_bid_count(&auction_id), 1);
    assert_eq!(client.get_escrow_balance(&auction_id), DEPOSIT);
    assert_eq!(wallet.balance(&contract_id), DEPOSIT);
    assert_eq!(wallet.balance(&bidder), MINT - DEPOSIT);
    assert_eq!(client.get_bidders(&auction_id).len(), 1);

    // The commitment is public, the plaintext bid is not.
    let info = client.get_commitment(&auction_id, &bidder);
    assert_eq!(info.bidder, bidder);
    assert_eq!(info.commitment, sealed);
    assert!(!info.revealed);
    assert!(!info.refunded);

    let err = client
        .try_get_revealed_bid(&auction_id, &bidder)
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::BidNotRevealed));

    // One sealed bid per bidder.
    let err = client
        .try_commit_bid(&auction_id, &bidder, &sealed, &DEPOSIT)
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::BidAlreadyCommitted));

    // Late commits are rejected once the commit window closes.
    env.ledger().set_timestamp(COMMIT_DEADLINE + 1);
    let other = Address::generate(&env);
    asset.mint(&other, &MINT);
    let other_sealed = commitment(&env, auction_id, 500, &salt(&env, 2));
    let err = client
        .try_commit_bid(&auction_id, &other, &other_sealed, &DEPOSIT)
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::CommitPeriodClosed));

    // The winner view stays sealed until the auction is settled.
    let err = client.try_get_winner(&auction_id).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::RevealNotEnded));
}

#[test]
fn test_reveal_phase() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let token_admin = Address::generate(&env);
    let payment = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let asset = token::StellarAssetClient::new(&env, &payment);
    let wallet = token::Client::new(&env, &payment);

    let nft_id = BytesN::from_array(&env, &[4u8; 32]);
    let nft = env.register_contract(None, MockNft);

    let contract_id = env.register_contract(None, BlindAuction);
    let client = BlindAuctionClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let seller = Address::generate(&env);
    let bidder = Address::generate(&env);
    client.initialize(&admin);
    asset.mint(&bidder, &MINT);

    let auction_id = client.create_auction(
        &seller,
        &nft,
        &nft_id,
        &payment,
        &RESERVE,
        &DEPOSIT,
        &COMMIT_DEADLINE,
        &REVEAL_DEADLINE,
    );

    let sealed = commitment(&env, auction_id, 300, &salt(&env, 9));
    client.commit_bid(&auction_id, &bidder, &sealed, &DEPOSIT);

    // Two more sealed bids join during the commit window.
    let second = Address::generate(&env);
    asset.mint(&second, &MINT);
    let second_sealed = commitment(&env, auction_id, 0, &salt(&env, 3));
    client.commit_bid(&auction_id, &second, &second_sealed, &DEPOSIT);

    let third = Address::generate(&env);
    asset.mint(&third, &MINT);
    let third_sealed = commitment(&env, auction_id, 400, &salt(&env, 4));
    client.commit_bid(&auction_id, &third, &third_sealed, &DEPOSIT);

    // Revealing is impossible while bids are still being committed.
    let err = client
        .try_reveal_bid(&auction_id, &bidder, &300, &salt(&env, 9))
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::RevealPeriodNotStarted));

    env.ledger().set_timestamp(COMMIT_DEADLINE + 1);

    // A wrong salt does not open the envelope.
    let err = client
        .try_reveal_bid(&auction_id, &bidder, &300, &salt(&env, 8))
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::CommitmentMismatch));

    // A tampered amount does not open it either.
    let err = client
        .try_reveal_bid(&auction_id, &bidder, &900, &salt(&env, 9))
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::CommitmentMismatch));

    // Still sealed after the failed attempts.
    let err = client
        .try_get_revealed_bid(&auction_id, &bidder)
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::BidNotRevealed));

    // Bidders without a commitment cannot reveal anything.
    let stranger = Address::generate(&env);
    let err = client
        .try_reveal_bid(&auction_id, &stranger, &400, &salt(&env, 5))
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::CommitmentMissing));

    // The correct salt reveals the bid and escrows the remainder: 3 deposits
    // of 10 plus the 290 needed to cover the 300 bid.
    client.reveal_bid(&auction_id, &bidder, &300, &salt(&env, 9));
    assert_eq!(client.get_revealed_bid(&auction_id, &bidder), 300);
    assert_eq!(client.get_escrow_balance(&auction_id), 320);
    assert_eq!(wallet.balance(&contract_id), 320);
    assert_eq!(wallet.balance(&bidder), MINT - 300);
    assert!(client.get_commitment(&auction_id, &bidder).revealed);

    // Revealing twice is rejected.
    let err = client
        .try_reveal_bid(&auction_id, &bidder, &300, &salt(&env, 9))
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::BidAlreadyRevealed));

    // Non positive bids are never valid.
    let err = client
        .try_reveal_bid(&auction_id, &second, &0, &salt(&env, 3))
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::InvalidBidAmount));

    // Late reveals are rejected once the reveal window closes.
    env.ledger().set_timestamp(REVEAL_DEADLINE + 1);
    let err = client
        .try_reveal_bid(&auction_id, &third, &400, &salt(&env, 4))
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::RevealPeriodClosed));
}

#[test]
fn test_winner_determination() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let token_admin = Address::generate(&env);
    let payment = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let asset = token::StellarAssetClient::new(&env, &payment);
    let wallet = token::Client::new(&env, &payment);

    let nft_id = BytesN::from_array(&env, &[5u8; 32]);
    let nft = env.register_contract(None, MockNft);
    let nft_client = MockNftClient::new(&env, &nft);

    let contract_id = env.register_contract(None, BlindAuction);
    let client = BlindAuctionClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let seller = Address::generate(&env);
    let bidder1 = Address::generate(&env);
    let bidder2 = Address::generate(&env);
    let bidder3 = Address::generate(&env);

    client.initialize(&admin);
    asset.mint(&bidder1, &MINT);
    asset.mint(&bidder2, &MINT);
    asset.mint(&bidder3, &MINT);
    nft_client.mint(&seller, &nft_id);

    let auction_id = client.create_auction(
        &seller,
        &nft,
        &nft_id,
        &payment,
        &RESERVE,
        &DEPOSIT,
        &COMMIT_DEADLINE,
        &REVEAL_DEADLINE,
    );

    client.commit_bid(
        &auction_id,
        &bidder1,
        &commitment(&env, auction_id, 100, &salt(&env, 1)),
        &DEPOSIT,
    );
    client.commit_bid(
        &auction_id,
        &bidder2,
        &commitment(&env, auction_id, 300, &salt(&env, 2)),
        &DEPOSIT,
    );
    client.commit_bid(
        &auction_id,
        &bidder3,
        &commitment(&env, auction_id, 250, &salt(&env, 3)),
        &DEPOSIT,
    );
    assert_eq!(client.get_bid_count(&auction_id), 3);

    // The auction cannot be closed while bids may still be revealed.
    env.ledger().set_timestamp(COMMIT_DEADLINE + 1);
    let err = client.try_close_auction(&auction_id).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::RevealNotEnded));
    let err = client.try_determine_winner(&auction_id).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::RevealNotEnded));

    client.reveal_bid(&auction_id, &bidder1, &100, &salt(&env, 1));
    client.reveal_bid(&auction_id, &bidder2, &300, &salt(&env, 2));
    client.reveal_bid(&auction_id, &bidder3, &250, &salt(&env, 3));

    // 3 deposits of 10 plus the top ups of 90, 290 and 240.
    assert_eq!(client.get_escrow_balance(&auction_id), 650);
    assert_eq!(wallet.balance(&contract_id), 650);

    env.ledger().set_timestamp(REVEAL_DEADLINE + 1);

    let preview = client.determine_winner(&auction_id);
    assert_eq!(preview.as_ref().unwrap().bidder, bidder2);
    assert_eq!(preview.as_ref().unwrap().amount, 300);

    let settled = client.close_auction(&auction_id);
    assert_eq!(settled.as_ref().unwrap().bidder, bidder2);
    assert_eq!(settled.as_ref().unwrap().amount, 300);

    let stored = client.get_winner(&auction_id);
    assert_eq!(stored.as_ref().unwrap().bidder, bidder2);
    assert_eq!(stored.as_ref().unwrap().amount, 300);

    let auction = client.get_auction(&auction_id);
    assert!(auction.closed);
    assert_eq!(auction.winner, Some(bidder2.clone()));
    assert_eq!(auction.winning_amount, 300);
    assert_eq!(auction.highest_bid, 300);
    assert_eq!(auction.reveal_count, 3);

    // Settling twice is rejected.
    let err = client.try_close_auction(&auction_id).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::AuctionAlreadyClosed));

    // The seller collects the escrowed winning bid, once.
    let proceeds = client.withdraw_proceeds(&auction_id, &seller);
    assert_eq!(proceeds, 300);
    assert_eq!(wallet.balance(&seller), 300);
    assert_eq!(wallet.balance(&contract_id), 350);
    let err = client
        .try_withdraw_proceeds(&auction_id, &seller)
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::ProceedsAlreadyWithdrawn));

    // And hands the NFT over, once.
    client.transfer_nft(&auction_id, &seller);
    assert_eq!(nft_client.owner_of(&nft_id), bidder2);
    let err = client.try_transfer_nft(&auction_id, &seller).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::NftAlreadyTransferred));

    // Losers get their escrows back, the winner does not.
    client.claim_refund(&auction_id, &bidder1);
    client.claim_refund(&auction_id, &bidder3);
    assert_eq!(wallet.balance(&bidder1), MINT);
    assert_eq!(wallet.balance(&bidder3), MINT);
    assert_eq!(wallet.balance(&bidder2), MINT - 300);
    assert_eq!(wallet.balance(&contract_id), 0);

    let err = client.try_claim_refund(&auction_id, &bidder2).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::NotTheWinner));
}

#[test]
fn test_tie_break_prefers_earliest_commit() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let payment = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let asset = token::StellarAssetClient::new(&env, &payment);
    let nft_id = BytesN::from_array(&env, &[6u8; 32]);
    let nft = env.register_contract(None, MockNft);

    let contract_id = env.register_contract(None, BlindAuction);
    let client = BlindAuctionClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let seller = Address::generate(&env);
    let early = Address::generate(&env);
    let late = Address::generate(&env);

    client.initialize(&admin);
    asset.mint(&early, &MINT);
    asset.mint(&late, &MINT);

    let auction_id = client.create_auction(
        &seller,
        &nft,
        &nft_id,
        &payment,
        &RESERVE,
        &DEPOSIT,
        &COMMIT_DEADLINE,
        &REVEAL_DEADLINE,
    );

    env.ledger().set_timestamp(START + 100);
    client.commit_bid(
        &auction_id,
        &early,
        &commitment(&env, auction_id, 300, &salt(&env, 1)),
        &DEPOSIT,
    );
    env.ledger().set_timestamp(START + 200);
    client.commit_bid(
        &auction_id,
        &late,
        &commitment(&env, auction_id, 300, &salt(&env, 2)),
        &DEPOSIT,
    );

    env.ledger().set_timestamp(COMMIT_DEADLINE + 1);
    client.reveal_bid(&auction_id, &early, &300, &salt(&env, 1));
    client.reveal_bid(&auction_id, &late, &300, &salt(&env, 2));

    env.ledger().set_timestamp(REVEAL_DEADLINE + 1);
    let winner = client.close_auction(&auction_id);
    assert_eq!(winner.as_ref().unwrap().bidder, early);
    assert_eq!(winner.as_ref().unwrap().committed_at, START + 100);
}

#[test]
fn test_reserve_not_met_has_no_winner() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let payment = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let asset = token::StellarAssetClient::new(&env, &payment);
    let wallet = token::Client::new(&env, &payment);
    let nft_id = BytesN::from_array(&env, &[10u8; 32]);
    let nft = env.register_contract(None, MockNft);

    let contract_id = env.register_contract(None, BlindAuction);
    let client = BlindAuctionClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let seller = Address::generate(&env);
    let bidder = Address::generate(&env);

    client.initialize(&admin);
    asset.mint(&bidder, &MINT);

    let auction_id = client.create_auction(
        &seller,
        &nft,
        &nft_id,
        &payment,
        &RESERVE,
        &DEPOSIT,
        &COMMIT_DEADLINE,
        &REVEAL_DEADLINE,
    );

    client.commit_bid(
        &auction_id,
        &bidder,
        &commitment(&env, auction_id, 50, &salt(&env, 1)),
        &DEPOSIT,
    );

    env.ledger().set_timestamp(COMMIT_DEADLINE + 1);
    client.reveal_bid(&auction_id, &bidder, &50, &salt(&env, 1));

    env.ledger().set_timestamp(REVEAL_DEADLINE + 1);
    let winner = client.close_auction(&auction_id);
    assert!(winner.is_none());

    let auction = client.get_auction(&auction_id);
    assert!(auction.closed);
    assert_eq!(auction.winner, None);
    assert_eq!(auction.winning_amount, 0);
    assert_eq!(auction.highest_bid, 50);

    assert!(client.get_winner(&auction_id).is_none());

    // Nothing to withdraw without a winner.
    let err = client
        .try_withdraw_proceeds(&auction_id, &seller)
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::NoWinningBid));

    // The bidder still gets the whole escrow back.
    let refund = client.claim_refund(&auction_id, &bidder);
    assert_eq!(refund, 50);
    assert_eq!(wallet.balance(&bidder), MINT);
    assert_eq!(wallet.balance(&contract_id), 0);
}

#[test]
fn test_refund_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let payment = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let asset = token::StellarAssetClient::new(&env, &payment);
    let wallet = token::Client::new(&env, &payment);
    let nft_id = BytesN::from_array(&env, &[11u8; 32]);
    let nft = env.register_contract(None, MockNft);

    let contract_id = env.register_contract(None, BlindAuction);
    let client = BlindAuctionClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let seller = Address::generate(&env);
    let loser = Address::generate(&env);

    client.initialize(&admin);
    asset.mint(&loser, &MINT);

    let auction_id = client.create_auction(
        &seller,
        &nft,
        &nft_id,
        &payment,
        &RESERVE,
        &DEPOSIT,
        &COMMIT_DEADLINE,
        &REVEAL_DEADLINE,
    );

    // Refunds are not available while the auction is live.
    let sealed = commitment(&env, auction_id, 100, &salt(&env, 1));
    client.commit_bid(&auction_id, &loser, &sealed, &DEPOSIT);
    let err = client.try_claim_refund(&auction_id, &loser).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::RefundNotAvailable));

    env.ledger().set_timestamp(COMMIT_DEADLINE + 1);
    client.reveal_bid(&auction_id, &loser, &100, &salt(&env, 1));

    env.ledger().set_timestamp(REVEAL_DEADLINE + 1);
    client.close_auction(&auction_id);

    // The reserve was not met, so the full escrow comes back.
    let refund = client.claim_refund(&auction_id, &loser);
    assert_eq!(refund, 100);
    assert_eq!(wallet.balance(&loser), MINT);

    // A second claim must not pay twice.
    let err = client.try_claim_refund(&auction_id, &loser).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::AlreadyRefunded));
    assert_eq!(wallet.balance(&loser), MINT);

    // Bidders that never committed have nothing to reclaim.
    let stranger = Address::generate(&env);
    let err = client.try_claim_refund(&auction_id, &stranger).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::CommitmentMissing));
}

#[test]
fn test_unrevealed_bid_only_loses_the_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let payment = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let asset = token::StellarAssetClient::new(&env, &payment);
    let wallet = token::Client::new(&env, &payment);
    let nft_id = BytesN::from_array(&env, &[12u8; 32]);
    let nft = env.register_contract(None, MockNft);

    let contract_id = env.register_contract(None, BlindAuction);
    let client = BlindAuctionClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let seller = Address::generate(&env);
    let winner = Address::generate(&env);
    let quitter = Address::generate(&env);

    client.initialize(&admin);
    asset.mint(&winner, &MINT);
    asset.mint(&quitter, &MINT);

    let auction_id = client.create_auction(
        &seller,
        &nft,
        &nft_id,
        &payment,
        &RESERVE,
        &DEPOSIT,
        &COMMIT_DEADLINE,
        &REVEAL_DEADLINE,
    );

    client.commit_bid(
        &auction_id,
        &winner,
        &commitment(&env, auction_id, 400, &salt(&env, 1)),
        &DEPOSIT,
    );
    client.commit_bid(
        &auction_id,
        &quitter,
        &commitment(&env, auction_id, 900, &salt(&env, 2)),
        &DEPOSIT,
    );

    env.ledger().set_timestamp(COMMIT_DEADLINE + 1);
    client.reveal_bid(&auction_id, &winner, &400, &salt(&env, 1));
    // `quitter` never reveals, so its sealed 900 does not compete.

    env.ledger().set_timestamp(REVEAL_DEADLINE + 1);
    let settled = client.close_auction(&auction_id);
    assert_eq!(settled.as_ref().unwrap().bidder, winner);

    // A sealed bid can never be read, even after settlement.
    let err = client
        .try_get_revealed_bid(&auction_id, &quitter)
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::BidNotRevealed));

    // Only the deposit is returned.
    let refund = client.claim_refund(&auction_id, &quitter);
    assert_eq!(refund, DEPOSIT);
    assert_eq!(wallet.balance(&quitter), MINT - DEPOSIT);
    assert_eq!(wallet.balance(&contract_id), 400);
}

#[test]
fn test_cancel_auction() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let payment = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let asset = token::StellarAssetClient::new(&env, &payment);
    let wallet = token::Client::new(&env, &payment);
    let nft_id = BytesN::from_array(&env, &[13u8; 32]);
    let nft = env.register_contract(None, MockNft);

    let contract_id = env.register_contract(None, BlindAuction);
    let client = BlindAuctionClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let seller = Address::generate(&env);
    let bidder = Address::generate(&env);
    let stranger = Address::generate(&env);

    client.initialize(&admin);
    asset.mint(&bidder, &MINT);

    let auction_id = client.create_auction(
        &seller,
        &nft,
        &nft_id,
        &payment,
        &RESERVE,
        &DEPOSIT,
        &COMMIT_DEADLINE,
        &REVEAL_DEADLINE,
    );

    client.commit_bid(
        &auction_id,
        &bidder,
        &commitment(&env, auction_id, 500, &salt(&env, 1)),
        &DEPOSIT,
    );

    // Only the seller or the admin may cancel.
    let err = client.try_cancel_auction(&auction_id, &stranger).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::Unauthorized));

    // The admin can cancel.
    client.cancel_auction(&auction_id, &admin);
    assert!(client.get_auction(&auction_id).cancelled);

    // Cancelling twice is rejected.
    let err = client.try_cancel_auction(&auction_id, &seller).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::AuctionCancelled));

    // No further bids and no reveals on a cancelled auction.
    let late = Address::generate(&env);
    asset.mint(&late, &MINT);
    let err = client
        .try_commit_bid(
            &auction_id,
            &late,
            &commitment(&env, auction_id, 600, &salt(&env, 2)),
            &DEPOSIT,
        )
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::AuctionCancelled));

    env.ledger().set_timestamp(COMMIT_DEADLINE + 1);
    let err = client
        .try_reveal_bid(&auction_id, &bidder, &500, &salt(&env, 1))
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::AuctionCancelled));
    let err = client.try_close_auction(&auction_id).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::AuctionCancelled));

    // Every deposit becomes refundable.
    let refund = client.claim_refund(&auction_id, &bidder);
    assert_eq!(refund, DEPOSIT);
    assert_eq!(wallet.balance(&bidder), MINT);
    assert_eq!(wallet.balance(&contract_id), 0);
    assert_eq!(client.get_analytics().cancelled_count, 1);
}

#[test]
fn test_cancel_window_and_authorization() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let payment = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let nft_id = BytesN::from_array(&env, &[14u8; 32]);
    let nft = env.register_contract(None, MockNft);

    let contract_id = env.register_contract(None, BlindAuction);
    let client = BlindAuctionClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let seller = Address::generate(&env);
    client.initialize(&admin);

    let auction_id = client.create_auction(
        &seller,
        &nft,
        &nft_id,
        &payment,
        &RESERVE,
        &DEPOSIT,
        &COMMIT_DEADLINE,
        &REVEAL_DEADLINE,
    );

    // The seller can cancel inside the commit window.
    env.ledger().set_timestamp(COMMIT_DEADLINE);
    client.cancel_auction(&auction_id, &seller);

    // But not after it.
    let nft_id_2 = BytesN::from_array(&env, &[15u8; 32]);
    let second = client.create_auction(
        &seller,
        &nft,
        &nft_id_2,
        &payment,
        &RESERVE,
        &DEPOSIT,
        &COMMIT_DEADLINE + 1_000,
        &COMMIT_DEADLINE + 2_000,
    );
    env.ledger().set_timestamp(COMMIT_DEADLINE + 1_001);
    let err = client.try_cancel_auction(&second, &seller).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::CancelWindowClosed));

    // Neither can proceeds be pulled from a live auction.
    let err = client.try_withdraw_proceeds(&second, &seller).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::RevealNotEnded));
    let err = client.try_transfer_nft(&second, &seller).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::RevealNotEnded));
}

#[test]
fn test_only_seller_or_admin_may_collect() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let payment = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let asset = token::StellarAssetClient::new(&env, &payment);
    let nft_id = BytesN::from_array(&env, &[16u8; 32]);
    let nft = env.register_contract(None, MockNft);

    let contract_id = env.register_contract(None, BlindAuction);
    let client = BlindAuctionClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let seller = Address::generate(&env);
    let bidder = Address::generate(&env);
    let stranger = Address::generate(&env);

    client.initialize(&admin);
    asset.mint(&bidder, &MINT);

    let auction_id = client.create_auction(
        &seller,
        &nft,
        &nft_id,
        &payment,
        &RESERVE,
        &DEPOSIT,
        &COMMIT_DEADLINE,
        &REVEAL_DEADLINE,
    );
    client.commit_bid(
        &auction_id,
        &bidder,
        &commitment(&env, auction_id, 400, &salt(&env, 1)),
        &DEPOSIT,
    );

    env.ledger().set_timestamp(COMMIT_DEADLINE + 1);
    client.reveal_bid(&auction_id, &bidder, &400, &salt(&env, 1));
    env.ledger().set_timestamp(REVEAL_DEADLINE + 1);
    client.close_auction(&auction_id);

    // A random address cannot pull the proceeds.
    let err = client
        .try_withdraw_proceeds(&auction_id, &stranger)
        .unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::Unauthorized));

    // Nor hand out the NFT.
    let err = client.try_transfer_nft(&auction_id, &stranger).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::Unauthorized));

    // The admin may collect on the seller's behalf.
    let proceeds = client.withdraw_proceeds(&auction_id, &admin);
    assert_eq!(proceeds, 400);
}

#[test]
fn test_analytics() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let payment = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let asset = token::StellarAssetClient::new(&env, &payment);
    let nft = env.register_contract(None, MockNft);

    let contract_id = env.register_contract(None, BlindAuction);
    let client = BlindAuctionClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let seller = Address::generate(&env);
    let bidder1 = Address::generate(&env);
    let bidder2 = Address::generate(&env);
    let bidder3 = Address::generate(&env);

    client.initialize(&admin);
    asset.mint(&bidder1, &MINT);
    asset.mint(&bidder2, &MINT);
    asset.mint(&bidder3, &MINT);

    // Auction #1: three sealed bids, two revealed, highest clears the reserve.
    let nft_id = BytesN::from_array(&env, &[17u8; 32]);
    let first = client.create_auction(
        &seller,
        &nft,
        &nft_id,
        &payment,
        &RESERVE,
        &DEPOSIT,
        &COMMIT_DEADLINE,
        &REVEAL_DEADLINE,
    );
    client.commit_bid(
        &first,
        &bidder1,
        &commitment(&env, first, 100, &salt(&env, 1)),
        &DEPOSIT,
    );
    client.commit_bid(
        &first,
        &bidder2,
        &commitment(&env, first, 300, &salt(&env, 2)),
        &DEPOSIT,
    );
    client.commit_bid(
        &first,
        &bidder3,
        &commitment(&env, first, 250, &salt(&env, 3)),
        &DEPOSIT,
    );

    env.ledger().set_timestamp(COMMIT_DEADLINE + 1);
    client.reveal_bid(&first, &bidder2, &300, &salt(&env, 2));
    client.reveal_bid(&first, &bidder3, &250, &salt(&env, 3));

    env.ledger().set_timestamp(REVEAL_DEADLINE + 1);
    client.close_auction(&first);
    client.claim_refund(&first, &bidder3);
    client.claim_refund(&first, &bidder1);
    client.withdraw_proceeds(&first, &seller);

    // Auction #2: cancelled with a single deposit.
    let nft_id_2 = BytesN::from_array(&env, &[18u8; 32]);
    let second = client.create_auction(
        &seller,
        &nft,
        &nft_id_2,
        &payment,
        &RESERVE,
        &DEPOSIT,
        &REVEAL_DEADLINE + 1_000,
        &REVEAL_DEADLINE + 2_000,
    );
    client.commit_bid(
        &second,
        &bidder1,
        &commitment(&env, second, 900, &salt(&env, 4)),
        &DEPOSIT,
    );
    client.cancel_auction(&second, &seller);
    client.claim_refund(&second, &bidder1);

    let stats = client.get_analytics();
    assert_eq!(stats.total_auctions, 2);
    assert_eq!(stats.total_bids, 4);
    assert_eq!(stats.total_reveals, 2);
    assert_eq!(stats.closed_count, 1);
    assert_eq!(stats.cancelled_count, 1);
    assert_eq!(stats.auctions_with_winner, 1);
    assert_eq!(stats.total_volume, 300);
    assert_eq!(stats.highest_bid, 300);
    // 3 x 10 deposits plus the top ups of 290 and 240, plus the 10 from #2.
    assert_eq!(stats.total_escrowed, 570);
    assert_eq!(stats.total_refunded, 270);
    // Everything came back: 300 to the seller, 250 + 10 + 10 refunded.
    assert_eq!(stats.outstanding_escrow, 0);

    let auction_stats = client.get_auction_stats(&first);
    assert_eq!(auction_stats.auction_id, first);
    assert_eq!(auction_stats.bid_count, 3);
    assert_eq!(auction_stats.reveal_count, 2);
    // Two of three sealed bids were opened.
    assert_eq!(auction_stats.commit_ratio_bps, 6_666);
    assert_eq!(auction_stats.reserve_price, RESERVE);
    assert_eq!(auction_stats.highest_bid, 300);
    assert_eq!(auction_stats.winning_amount, 300);
    assert_eq!(auction_stats.escrow_balance, 0);
    assert_eq!(auction_stats.total_refunded, 260);
    assert_eq!(auction_stats.winner, Some(bidder2.clone()));
    assert!(auction_stats.closed);
    assert!(!auction_stats.cancelled);
    assert_eq!(auction_stats.created_at, START);
    assert_eq!(auction_stats.closed_at, Some(REVEAL_DEADLINE + 1));
    assert_eq!(auction_stats.commit_window, COMMIT_DEADLINE - START);
    assert_eq!(auction_stats.reveal_window, REVEAL_DEADLINE - COMMIT_DEADLINE);

    let cancelled_stats = client.get_auction_stats(&second);
    assert_eq!(cancelled_stats.bid_count, 1);
    assert_eq!(cancelled_stats.reveal_count, 0);
    assert_eq!(cancelled_stats.commit_ratio_bps, 0);
    assert!(cancelled_stats.cancelled);
    assert!(!cancelled_stats.closed);
    assert_eq!(cancelled_stats.highest_bid, 0);
    assert_eq!(cancelled_stats.total_refunded, DEPOSIT);

    let err = client.try_get_auction_stats(&404).unwrap_err();
    assert_eq!(err, Err(BlindAuctionError::AuctionNotFound));
}
