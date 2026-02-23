#![cfg(test)]

use crate::{
    ListingStatus, PuzzleRentalContract, PuzzleRentalContractClient, RentalStatus,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

// ============================================================
// Test Helpers
// ============================================================

struct TestSetup {
    env: Env,
    contract_id: Address,
    admin: Address,
    owner: Address,
    renter: Address,
    token_id: Address,
    nft_contract: Address,
}

fn setup() -> TestSetup {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, PuzzleRentalContract);
    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let renter = Address::generate(&env);
    let nft_contract = Address::generate(&env);

    // Create a Stellar asset token for payments
    let token_admin = Address::generate(&env);
    let token_contract = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_id = token_contract.address();
    let token_sac = StellarAssetClient::new(&env, &token_id);

    // Mint initial tokens to renter
    token_sac.mint(&renter, &10_000);

    // Initialize the contract
    let client = PuzzleRentalContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    TestSetup {
        env,
        contract_id,
        admin,
        owner,
        renter,
        token_id,
        nft_contract,
    }
}

fn set_timestamp(env: &Env, ts: u64) {
    env.ledger().set(LedgerInfo {
        timestamp: ts,
        protocol_version: 21,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 1,
        min_persistent_entry_ttl: 1,
        max_entry_ttl: 6_312_000,
    });
}

// ============================================================
// Unit Tests: Initialization
// ============================================================

#[test]
fn test_initialize_succeeds() {
    // setup() already calls initialize; if it doesn't panic the contract is ready
    let t = setup();
    let _client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
}

#[test]
#[should_panic(expected = "already initialized")]
fn test_double_initialize_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    // Second initialization must fail
    client.initialize(&t.admin);
}

// ============================================================
// Unit Tests: Listing Creation
// ============================================================

#[test]
fn test_create_listing_returns_sequential_ids() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    let id1 = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &50u32,
    );
    let id2 = client.create_listing(
        &t.owner, &t.nft_contract, &2u64, &t.token_id,
        &200i128, &7200u64, &5u32, &false, &0u32,
    );

    assert_eq!(id1, 1u64);
    assert_eq!(id2, 2u64);
}

#[test]
fn test_create_listing_stores_correct_fields() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    let listing_id = client.create_listing(
        &t.owner,
        &t.nft_contract,
        &42u64,
        &t.token_id,
        &500i128,
        &3600u64,
        &10u32,
        &true,
        &75u32,
    );

    let listing = client.get_listing(&listing_id);
    assert_eq!(listing.listing_id, listing_id);
    assert_eq!(listing.owner, t.owner);
    assert_eq!(listing.nft_contract, t.nft_contract);
    assert_eq!(listing.nft_token_id, 42u64);
    assert_eq!(listing.payment_token, t.token_id);
    assert_eq!(listing.price_per_period, 500);
    assert_eq!(listing.period_duration, 3600);
    assert_eq!(listing.max_periods, 10);
    assert!(listing.allow_extensions);
    assert_eq!(listing.early_termination_refund_pct, 75);
    assert!(matches!(listing.status, ListingStatus::Active));
}

#[test]
#[should_panic(expected = "price must be positive")]
fn test_create_listing_zero_price_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &0i128, &3600u64, &10u32, &true, &50u32,
    );
}

#[test]
#[should_panic(expected = "price must be positive")]
fn test_create_listing_negative_price_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &-1i128, &3600u64, &10u32, &true, &50u32,
    );
}

#[test]
#[should_panic(expected = "period duration must be > 0")]
fn test_create_listing_zero_duration_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &0u64, &10u32, &true, &50u32,
    );
}

#[test]
#[should_panic(expected = "max periods must be > 0")]
fn test_create_listing_zero_max_periods_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &0u32, &true, &50u32,
    );
}

#[test]
#[should_panic(expected = "refund pct must be 0-100")]
fn test_create_listing_refund_pct_over_100_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &101u32,
    );
}

// ============================================================
// Unit Tests: Listing Cancellation
// ============================================================

#[test]
fn test_cancel_listing_sets_cancelled_status() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &50u32,
    );

    client.cancel_listing(&t.owner, &listing_id);

    let listing = client.get_listing(&listing_id);
    assert!(matches!(listing.status, ListingStatus::Cancelled));
}

#[test]
fn test_cancel_listing_removes_from_marketplace() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &50u32,
    );

    client.cancel_listing(&t.owner, &listing_id);

    let page = client.get_active_listings(&0u64, &10u32);
    assert_eq!(page.total, 0);
}

#[test]
#[should_panic(expected = "not the owner")]
fn test_cancel_listing_wrong_caller_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &50u32,
    );

    client.cancel_listing(&t.renter, &listing_id);
}

#[test]
#[should_panic(expected = "listing not found")]
fn test_cancel_nonexistent_listing_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    client.cancel_listing(&t.owner, &999u64);
}

// ============================================================
// Unit Tests: Renting
// ============================================================

#[test]
fn test_rent_basic_creates_agreement() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 1000);

    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &50u32,
    );

    let rental_id = client.rent(&t.renter, &listing_id, &2u32);
    let rental = client.get_rental(&rental_id);

    assert_eq!(rental.rental_id, rental_id);
    assert_eq!(rental.listing_id, listing_id);
    assert_eq!(rental.renter, t.renter);
    assert_eq!(rental.owner, t.owner);
    assert_eq!(rental.periods, 2);
    assert_eq!(rental.total_paid, 200);
    assert_eq!(rental.start_time, 1000);
    assert_eq!(rental.end_time, 1000 + 3600 * 2);
    assert!(matches!(rental.status, RentalStatus::Active));
}

#[test]
fn test_rent_escrows_tokens_in_contract() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    let token_client = TokenClient::new(&t.env, &t.token_id);

    let before_renter = token_client.balance(&t.renter);

    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &250i128, &3600u64, &10u32, &true, &0u32,
    );

    client.rent(&t.renter, &listing_id, &3u32);

    // Renter paid; owner has not received anything yet (funds escrowed in contract)
    assert_eq!(token_client.balance(&t.renter), before_renter - 750);
    assert_eq!(token_client.balance(&t.owner), 0);
    assert_eq!(token_client.balance(&t.contract_id), 750);
}

#[test]
#[should_panic(expected = "listing not active")]
fn test_rent_cancelled_listing_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &50u32,
    );
    client.cancel_listing(&t.owner, &listing_id);
    client.rent(&t.renter, &listing_id, &1u32);
}

#[test]
#[should_panic(expected = "exceeds max periods")]
fn test_rent_exceeds_max_periods_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &3u32, &true, &50u32,
    );
    client.rent(&t.renter, &listing_id, &5u32);
}

#[test]
#[should_panic(expected = "periods must be > 0")]
fn test_rent_zero_periods_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &50u32,
    );
    client.rent(&t.renter, &listing_id, &0u32);
}

#[test]
#[should_panic(expected = "owner cannot rent own listing")]
fn test_owner_cannot_rent_own_listing() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    let token_sac = StellarAssetClient::new(&t.env, &t.token_id);
    token_sac.mint(&t.owner, &10_000);

    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &50u32,
    );
    client.rent(&t.owner, &listing_id, &1u32);
}

#[test]
#[should_panic(expected = "listing not found")]
fn test_rent_nonexistent_listing_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    client.rent(&t.renter, &999u64, &1u32);
}

// ============================================================
// Unit Tests: Access Control
// ============================================================

#[test]
fn test_has_access_during_active_rental() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 1000);

    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &42u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &50u32,
    );
    client.rent(&t.renter, &listing_id, &1u32);

    set_timestamp(&t.env, 2000);
    assert!(client.has_access(&t.renter, &t.nft_contract, &42u64));
}

#[test]
fn test_no_access_without_rental() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    assert!(!client.has_access(&t.renter, &t.nft_contract, &42u64));
}

#[test]
fn test_no_access_after_expiry() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 1000);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &42u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );
    client.rent(&t.renter, &listing_id, &1u32);
    // end_time = 1000 + 3600 = 4600

    set_timestamp(&t.env, 5000);
    assert!(!client.has_access(&t.renter, &t.nft_contract, &42u64));
}

#[test]
fn test_no_access_for_wrong_nft_token() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 1000);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );
    client.rent(&t.renter, &listing_id, &1u32);

    // Renter has access to token 1, not token 2
    assert!(client.has_access(&t.renter, &t.nft_contract, &1u64));
    assert!(!client.has_access(&t.renter, &t.nft_contract, &2u64));
}

#[test]
fn test_no_access_for_different_nft_contract() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    let other_contract = Address::generate(&t.env);

    set_timestamp(&t.env, 1000);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );
    client.rent(&t.renter, &listing_id, &1u32);

    assert!(client.has_access(&t.renter, &t.nft_contract, &1u64));
    assert!(!client.has_access(&t.renter, &other_contract, &1u64));
}

// ============================================================
// Unit Tests: Rental Extension
// ============================================================

#[test]
fn test_extend_rental_updates_end_time_and_cost() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    let token_sac = StellarAssetClient::new(&t.env, &t.token_id);
    token_sac.mint(&t.renter, &10_000);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &50u32,
    );

    let rental_id = client.rent(&t.renter, &listing_id, &2u32);
    // end_time = 0 + 3600*2 = 7200, total_paid = 200

    set_timestamp(&t.env, 2000);
    client.extend_rental(&t.renter, &rental_id, &3u32);

    let rental = client.get_rental(&rental_id);
    assert_eq!(rental.periods, 5);
    assert_eq!(rental.end_time, 3600 * 5); // 0 + 3600*5
    assert_eq!(rental.total_paid, 500);
}

#[test]
fn test_extend_rental_escrows_additional_tokens() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    let token_sac = StellarAssetClient::new(&t.env, &t.token_id);
    let token_client = TokenClient::new(&t.env, &t.token_id);
    token_sac.mint(&t.renter, &10_000);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);

    let renter_after_rent = token_client.balance(&t.renter);
    let contract_after_rent = token_client.balance(&t.contract_id);

    client.extend_rental(&t.renter, &rental_id, &2u32);

    // Additional 200 tokens escrowed; owner still receives nothing until expiry
    assert_eq!(token_client.balance(&t.renter), renter_after_rent - 200);
    assert_eq!(token_client.balance(&t.contract_id), contract_after_rent + 200);
    assert_eq!(token_client.balance(&t.owner), 0);
}

#[test]
#[should_panic(expected = "extensions not allowed")]
fn test_extend_rental_not_allowed_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &false, &50u32, // no extensions
    );
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);
    client.extend_rental(&t.renter, &rental_id, &1u32);
}

#[test]
#[should_panic(expected = "exceeds max periods")]
fn test_extend_beyond_max_periods_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    let token_sac = StellarAssetClient::new(&t.env, &t.token_id);
    token_sac.mint(&t.renter, &10_000);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &3u32, &true, &50u32, // max = 3
    );
    let rental_id = client.rent(&t.renter, &listing_id, &2u32);
    // Already at 2; extending by 2 would give 4 which exceeds max of 3
    client.extend_rental(&t.renter, &rental_id, &2u32);
}

#[test]
#[should_panic(expected = "not the renter")]
fn test_extend_rental_wrong_caller_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);
    client.extend_rental(&t.owner, &rental_id, &1u32);
}

#[test]
#[should_panic(expected = "additional periods must be > 0")]
fn test_extend_rental_zero_periods_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);
    client.extend_rental(&t.renter, &rental_id, &0u32);
}

// ============================================================
// Unit Tests: Expiration
// ============================================================

#[test]
fn test_expire_rental_sets_expired_status() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 1000);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);
    // end_time = 4600

    set_timestamp(&t.env, 5000);
    client.expire_rental(&rental_id);

    let rental = client.get_rental(&rental_id);
    assert!(matches!(rental.status, RentalStatus::Expired));
}

#[test]
fn test_expire_rental_writes_history() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &200i128, &3600u64, &10u32, &true, &0u32,
    );
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);

    set_timestamp(&t.env, 4000);
    client.expire_rental(&rental_id);

    let history = client.get_rental_history(&rental_id);
    assert_eq!(history.rental_id, rental_id);
    assert_eq!(history.total_paid, 200);
    assert!(matches!(history.final_status, RentalStatus::Expired));
}

#[test]
#[should_panic(expected = "rental has not expired yet")]
fn test_expire_rental_too_early_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 1000);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);

    set_timestamp(&t.env, 2000); // end_time = 4600, not yet
    client.expire_rental(&rental_id);
}

#[test]
#[should_panic(expected = "rental already closed")]
fn test_expire_already_expired_rental_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);

    set_timestamp(&t.env, 4000);
    client.expire_rental(&rental_id);
    client.expire_rental(&rental_id); // second call should panic
}

// ============================================================
// Unit Tests: Early Termination & Refunds
// ============================================================

#[test]
fn test_terminate_rental_with_full_refund() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    let token_client = TokenClient::new(&t.env, &t.token_id);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &1000i128, &10_000u64, &10u32, &true,
        &100u32, // 100% refund
    );

    let renter_before = token_client.balance(&t.renter);
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);
    // paid = 1000, end_time = 10000; escrowed in contract

    // Terminate at 5000 → 50% unused → refund = 500, owner gets 500
    set_timestamp(&t.env, 5000);
    client.terminate_rental(&t.renter, &rental_id);

    assert_eq!(token_client.balance(&t.renter), renter_before - 500);
    assert_eq!(token_client.balance(&t.owner), 500);
    assert_eq!(token_client.balance(&t.contract_id), 0);
}

#[test]
fn test_terminate_rental_with_partial_refund() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    let token_client = TokenClient::new(&t.env, &t.token_id);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &1000i128, &10_000u64, &10u32, &true,
        &50u32, // 50% refund
    );

    let renter_before = token_client.balance(&t.renter);
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);
    // paid = 1000, end_time = 10000; escrowed in contract

    // Terminate at 5000 → 50% unused → refund = 1000 * 0.5 * 50% = 250, owner gets 750
    set_timestamp(&t.env, 5000);
    client.terminate_rental(&t.renter, &rental_id);

    assert_eq!(token_client.balance(&t.renter), renter_before - 750);
    assert_eq!(token_client.balance(&t.owner), 750);
    assert_eq!(token_client.balance(&t.contract_id), 0);
}

#[test]
fn test_terminate_rental_with_no_refund() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    let token_client = TokenClient::new(&t.env, &t.token_id);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &1000i128, &10_000u64, &10u32, &true,
        &0u32, // 0% refund
    );

    let renter_before = token_client.balance(&t.renter);
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);

    set_timestamp(&t.env, 5000);
    client.terminate_rental(&t.renter, &rental_id);

    // No refund; full amount goes to owner
    assert_eq!(token_client.balance(&t.renter), renter_before - 1000);
    assert_eq!(token_client.balance(&t.owner), 1000);
    assert_eq!(token_client.balance(&t.contract_id), 0);
}

#[test]
fn test_terminate_rental_sets_terminated_status() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);

    set_timestamp(&t.env, 1000);
    client.terminate_rental(&t.renter, &rental_id);

    let rental = client.get_rental(&rental_id);
    assert!(matches!(rental.status, RentalStatus::Terminated));
}

#[test]
fn test_terminate_rental_writes_history() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &500i128, &10_000u64, &10u32, &true, &0u32,
    );
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);

    set_timestamp(&t.env, 5000);
    client.terminate_rental(&t.renter, &rental_id);

    let history = client.get_rental_history(&rental_id);
    assert_eq!(history.rental_id, rental_id);
    assert_eq!(history.total_paid, 500);
    assert!(matches!(history.final_status, RentalStatus::Terminated));
}

#[test]
#[should_panic(expected = "not the renter")]
fn test_terminate_rental_wrong_caller_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);
    client.terminate_rental(&t.owner, &rental_id);
}

#[test]
#[should_panic(expected = "rental is not active")]
fn test_terminate_already_terminated_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);

    set_timestamp(&t.env, 1000);
    client.terminate_rental(&t.renter, &rental_id);
    client.terminate_rental(&t.renter, &rental_id); // second call should panic
}

// ============================================================
// Unit Tests: Marketplace Discovery
// ============================================================

#[test]
fn test_marketplace_pagination_first_page() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    for i in 0..5u64 {
        client.create_listing(
            &t.owner, &t.nft_contract, &i, &t.token_id,
            &100i128, &3600u64, &10u32, &true, &50u32,
        );
    }

    let page = client.get_active_listings(&0u64, &3u32);
    assert_eq!(page.listings.len(), 3);
    assert_eq!(page.total, 5);
}

#[test]
fn test_marketplace_pagination_second_page() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    for i in 0..5u64 {
        client.create_listing(
            &t.owner, &t.nft_contract, &i, &t.token_id,
            &100i128, &3600u64, &10u32, &true, &50u32,
        );
    }

    let page = client.get_active_listings(&3u64, &3u32);
    assert_eq!(page.listings.len(), 2);
    assert_eq!(page.total, 5);
}

#[test]
fn test_marketplace_empty_when_no_listings() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    let page = client.get_active_listings(&0u64, &10u32);
    assert_eq!(page.listings.len(), 0);
    assert_eq!(page.total, 0);
}

#[test]
fn test_marketplace_excludes_cancelled_listings() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    let id1 = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );
    client.create_listing(
        &t.owner, &t.nft_contract, &2u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );

    client.cancel_listing(&t.owner, &id1);

    let page = client.get_active_listings(&0u64, &10u32);
    // id1 was removed from the index on cancellation
    assert_eq!(page.total, 1);
}

// ============================================================
// Unit Tests: History Tracking
// ============================================================

#[test]
fn test_rental_history_stored_after_expiry() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 1000);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &200i128, &3600u64, &10u32, &true, &0u32,
    );
    let rental_id = client.rent(&t.renter, &listing_id, &2u32);

    set_timestamp(&t.env, 10_000);
    client.expire_rental(&rental_id);

    let history = client.get_rental_history(&rental_id);
    assert_eq!(history.rental_id, rental_id);
    assert_eq!(history.listing_id, listing_id);
    assert_eq!(history.owner, t.owner);
    assert_eq!(history.renter, t.renter);
    assert_eq!(history.total_paid, 400);
    assert_eq!(history.start_time, 1000);
    assert_eq!(history.end_time, 1000 + 3600 * 2);
    assert!(matches!(history.final_status, RentalStatus::Expired));
}

#[test]
#[should_panic(expected = "rental history not found")]
fn test_get_rental_history_before_close_panics() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);

    // Rental is still active; history doesn't exist yet
    client.get_rental_history(&rental_id);
}

// ============================================================
// Unit Tests: Owner & Renter Index Queries
// ============================================================

#[test]
fn test_get_owner_listings_returns_all() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    let id1 = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );
    let id2 = client.create_listing(
        &t.owner, &t.nft_contract, &2u64, &t.token_id,
        &200i128, &7200u64, &5u32, &false, &0u32,
    );

    let listings = client.get_owner_listings(&t.owner);
    assert_eq!(listings.len(), 2);
    assert_eq!(listings.get(0).unwrap(), id1);
    assert_eq!(listings.get(1).unwrap(), id2);
}

#[test]
fn test_get_owner_listings_empty_for_unknown_owner() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    let unknown = Address::generate(&t.env);

    let listings = client.get_owner_listings(&unknown);
    assert_eq!(listings.len(), 0);
}

#[test]
fn test_get_renter_rentals_returns_all() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);

    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );

    let r1 = client.rent(&t.renter, &listing_id, &1u32);
    let r2 = client.rent(&t.renter, &listing_id, &1u32);

    let rentals = client.get_renter_rentals(&t.renter);
    assert_eq!(rentals.len(), 2);
    assert_eq!(rentals.get(0).unwrap(), r1);
    assert_eq!(rentals.get(1).unwrap(), r2);
}

#[test]
fn test_get_renter_rentals_empty_for_unknown_renter() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    let unknown = Address::generate(&t.env);

    let rentals = client.get_renter_rentals(&unknown);
    assert_eq!(rentals.len(), 0);
}

// ============================================================
// Integration Tests: Full Rental Lifecycle
// ============================================================

#[test]
fn test_full_happy_path_lifecycle() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    let token_sac = StellarAssetClient::new(&t.env, &t.token_id);
    let token_client = TokenClient::new(&t.env, &t.token_id);

    // Fund accounts
    token_sac.mint(&t.renter, &5000);
    token_sac.mint(&t.owner, &5000);

    let initial_renter = token_client.balance(&t.renter);
    let initial_owner = token_client.balance(&t.owner);

    // 1. Owner creates listing
    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &99u64, &t.token_id,
        &1000i128, &10_000u64, &5u32, &true, &50u32,
    );

    // 2. Marketplace shows the listing
    let page = client.get_active_listings(&0u64, &10u32);
    assert_eq!(page.total, 1);

    // 3. Renter pays for 2 periods; funds escrowed in contract
    let rental_id = client.rent(&t.renter, &listing_id, &2u32);
    assert_eq!(token_client.balance(&t.renter), initial_renter - 2000);
    assert_eq!(token_client.balance(&t.owner), initial_owner); // not yet paid
    assert_eq!(token_client.balance(&t.contract_id), 2000);

    // 4. Access granted mid-rental
    set_timestamp(&t.env, 5000);
    assert!(client.has_access(&t.renter, &t.nft_contract, &99u64));

    // 5. Extend by 1 period; more tokens escrowed
    client.extend_rental(&t.renter, &rental_id, &1u32);
    assert_eq!(token_client.balance(&t.renter), initial_renter - 3000);
    assert_eq!(token_client.balance(&t.contract_id), 3000);

    let rental = client.get_rental(&rental_id);
    assert_eq!(rental.end_time, 10_000 * 3);
    assert_eq!(rental.periods, 3);

    // 6. Access granted in the extended window
    set_timestamp(&t.env, 25_000);
    assert!(client.has_access(&t.renter, &t.nft_contract, &99u64));

    // 7. Expire after end_time
    set_timestamp(&t.env, 31_000);
    client.expire_rental(&rental_id);

    // 8. Access denied
    assert!(!client.has_access(&t.renter, &t.nft_contract, &99u64));

    // 9. Owner receives full escrowed amount on expiry; contract balance zeroed
    assert_eq!(token_client.balance(&t.owner), initial_owner + 3000);
    assert_eq!(token_client.balance(&t.contract_id), 0);

    // 10. History recorded correctly
    let history = client.get_rental_history(&rental_id);
    assert_eq!(history.total_paid, 3000);
    assert!(matches!(history.final_status, RentalStatus::Expired));

    // 11. Index queries return correct data
    let owner_listings = client.get_owner_listings(&t.owner);
    assert_eq!(owner_listings.len(), 1);
    assert_eq!(owner_listings.get(0).unwrap(), listing_id);

    let renter_rentals = client.get_renter_rentals(&t.renter);
    assert_eq!(renter_rentals.len(), 1);
    assert_eq!(renter_rentals.get(0).unwrap(), rental_id);
}

#[test]
fn test_full_early_termination_lifecycle() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    let token_sac = StellarAssetClient::new(&t.env, &t.token_id);
    let token_client = TokenClient::new(&t.env, &t.token_id);

    token_sac.mint(&t.renter, &5000);
    token_sac.mint(&t.owner, &5000);

    let renter_before = token_client.balance(&t.renter);
    let owner_before = token_client.balance(&t.owner);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &1000i128, &10_000u64, &5u32, &true,
        &100u32, // 100% refund of unused time
    );

    // Renter pays for 1 period → cost = 1000, end_time = 10000; funds escrowed
    let rental_id = client.rent(&t.renter, &listing_id, &1u32);
    assert_eq!(token_client.balance(&t.renter), renter_before - 1000);
    assert_eq!(token_client.balance(&t.owner), owner_before); // not yet paid
    assert_eq!(token_client.balance(&t.contract_id), 1000);

    // Access confirmed
    set_timestamp(&t.env, 2000);
    assert!(client.has_access(&t.renter, &t.nft_contract, &1u64));

    // Terminate at t=5000 → 50% unused → refund = 500, owner gets 500
    set_timestamp(&t.env, 5000);
    client.terminate_rental(&t.renter, &rental_id);

    assert_eq!(token_client.balance(&t.renter), renter_before - 500);
    assert_eq!(token_client.balance(&t.owner), owner_before + 500);
    assert_eq!(token_client.balance(&t.contract_id), 0);

    // Access revoked (rental terminated, status check in has_access)
    assert!(!client.has_access(&t.renter, &t.nft_contract, &1u64));

    // History recorded
    let history = client.get_rental_history(&rental_id);
    assert!(matches!(history.final_status, RentalStatus::Terminated));
}

#[test]
fn test_multiple_renters_independent_access_windows() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    let token_sac = StellarAssetClient::new(&t.env, &t.token_id);

    let renter2 = Address::generate(&t.env);
    token_sac.mint(&renter2, &5000);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &7u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );

    // renter1 rents 1 period (ends at 3600), renter2 rents 2 periods (ends at 7200)
    let r1 = client.rent(&t.renter, &listing_id, &1u32);
    let r2 = client.rent(&renter2, &listing_id, &2u32);
    assert_ne!(r1, r2);

    // Both have access at t=1000
    set_timestamp(&t.env, 1000);
    assert!(client.has_access(&t.renter, &t.nft_contract, &7u64));
    assert!(client.has_access(&renter2, &t.nft_contract, &7u64));

    // After renter1 expires, only renter2 has access
    set_timestamp(&t.env, 4000);
    assert!(!client.has_access(&t.renter, &t.nft_contract, &7u64));
    assert!(client.has_access(&renter2, &t.nft_contract, &7u64));

    // Both expired at t=7500
    set_timestamp(&t.env, 7500);
    assert!(!client.has_access(&t.renter, &t.nft_contract, &7u64));
    assert!(!client.has_access(&renter2, &t.nft_contract, &7u64));
}

#[test]
fn test_owner_with_multiple_listings_and_renters() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    let token_sac = StellarAssetClient::new(&t.env, &t.token_id);

    let renter2 = Address::generate(&t.env);
    token_sac.mint(&renter2, &5000);

    set_timestamp(&t.env, 0);

    let listing1 = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );
    let listing2 = client.create_listing(
        &t.owner, &t.nft_contract, &2u64, &t.token_id,
        &200i128, &7200u64, &5u32, &false, &0u32,
    );

    let r1 = client.rent(&t.renter, &listing1, &1u32);
    let r2 = client.rent(&renter2, &listing2, &1u32);

    // Both renters have access to their respective NFTs
    set_timestamp(&t.env, 1000);
    assert!(client.has_access(&t.renter, &t.nft_contract, &1u64));
    assert!(client.has_access(&renter2, &t.nft_contract, &2u64));

    // Cross-access is denied
    assert!(!client.has_access(&t.renter, &t.nft_contract, &2u64));
    assert!(!client.has_access(&renter2, &t.nft_contract, &1u64));

    // Owner has 2 listings in index
    let owner_listings = client.get_owner_listings(&t.owner);
    assert_eq!(owner_listings.len(), 2);

    // Each renter has 1 rental
    assert_eq!(client.get_renter_rentals(&t.renter).len(), 1);
    assert_eq!(client.get_renter_rentals(&renter2).len(), 1);

    // Expire both
    set_timestamp(&t.env, 20_000);
    client.expire_rental(&r1);
    client.expire_rental(&r2);

    let h1 = client.get_rental_history(&r1);
    let h2 = client.get_rental_history(&r2);
    assert!(matches!(h1.final_status, RentalStatus::Expired));
    assert!(matches!(h2.final_status, RentalStatus::Expired));
}

#[test]
fn test_renter_sequential_rentals_on_same_listing() {
    let t = setup();
    let client = PuzzleRentalContractClient::new(&t.env, &t.contract_id);
    let token_sac = StellarAssetClient::new(&t.env, &t.token_id);
    token_sac.mint(&t.renter, &50_000);

    set_timestamp(&t.env, 0);
    let listing_id = client.create_listing(
        &t.owner, &t.nft_contract, &1u64, &t.token_id,
        &100i128, &3600u64, &10u32, &true, &0u32,
    );

    // First rental
    let r1 = client.rent(&t.renter, &listing_id, &1u32);
    set_timestamp(&t.env, 4000);
    client.expire_rental(&r1);

    // Second rental after first expired
    set_timestamp(&t.env, 5000);
    let r2 = client.rent(&t.renter, &listing_id, &1u32);

    assert_ne!(r1, r2);
    assert!(client.has_access(&t.renter, &t.nft_contract, &1u64));

    let rentals = client.get_renter_rentals(&t.renter);
    assert_eq!(rentals.len(), 2);
}