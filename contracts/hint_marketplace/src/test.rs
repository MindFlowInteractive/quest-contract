#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, Env, symbol_short,
};

fn create_test_content_hash(env: &Env) -> [u8; 32] {
    [0u8; 32] // Simplified for testing
}

#[test]
fn test_initialize() {
    let env = Env::default();
    let contract_id = env.register_contract(None, HintMarketplace);
    let client = HintMarketplaceClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);

    client.initialize(
        &admin,
        &fee_recipient,
        &250, // 2.5% fee
        &3600, // 1 hour min
        &86400 * 30, // 30 days max
        &500, // 5% price adjustment
        &HintQuality::Good,
    );

    let config = client.get_config();
    assert_eq!(config.admin, admin);
    assert_eq!(config.fee_recipient, fee_recipient);
    assert_eq!(config.fee_bps, 250);
    assert_eq!(config.min_quality_for_listing, HintQuality::Good);
}

#[test]
fn test_create_hint() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, HintMarketplace);
    let client = HintMarketplaceClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);

    client.initialize(
        &admin,
        &admin,
        &250,
        &3600,
        &86400 * 30,
        &500,
        &HintQuality::Good,
    );

    let content_hash = create_test_content_hash(&env);
    let hint_id = client.create_hint(&creator, &1u32, &content_hash, &HintQuality::Excellent);

    assert_eq!(hint_id, 1);

    let hint = client.get_hint(&hint_id).unwrap();
    assert_eq!(hint.creator, creator);
    assert_eq!(hint.puzzle_id, 1);
    assert_eq!(hint.quality, HintQuality::Excellent);
    assert_eq!(hint.total_sales, 0);
}

#[test]
fn test_create_listing() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup token
    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();

    // Setup marketplace
    let contract_id = env.register_contract(None, HintMarketplace);
    let client = HintMarketplaceClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &admin, &250, &3600, &86400 * 30, &500, &HintQuality::Good);

    // Create hint
    let creator = Address::generate(&env);
    let content_hash = create_test_content_hash(&env);
    let hint_id = client.create_hint(&creator, &1u32, &content_hash, &HintQuality::Excellent);

    // Create listing
    let listing_id = client.create_listing(
        &creator,
        &hint_id,
        &token_contract_id,
        &1000, // base price
        &86400, // 1 day duration
        &500, // 5% royalty
    );

    // Verify listing
    let listing = client.get_listing(&listing_id).unwrap();
    assert_eq!(listing.seller, creator);
    assert_eq!(listing.hint_id, hint_id);
    assert_eq!(listing.base_price, 1000);
    assert_eq!(listing.status, ListingStatus::Active);
    assert_eq!(listing.royalty_bps, 500);

    // Verify listing appears in indexes
    let hint_listings = client.get_listings_by_hint(&hint_id);
    assert!(hint_listings.contains(&listing_id));

    let seller_listings = client.get_listings_by_seller(&creator);
    assert!(seller_listings.contains(&listing_id));

    let puzzle_listings = client.get_listings_by_puzzle(&1u32);
    assert!(puzzle_listings.contains(&listing_id));
}

#[test]
fn test_buy_hint() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup token
    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = token::Client::new(&env, &token_contract_id);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_contract_id);

    // Setup marketplace
    let contract_id = env.register_contract(None, HintMarketplace);
    let client = HintMarketplaceClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    client.initialize(&admin, &fee_recipient, &250, &3600, &86400 * 30, &500, &HintQuality::Good);

    // Create hint and listing
    let creator = Address::generate(&env);
    let seller = Address::generate(&env);
    let buyer = Address::generate(&env);

    let content_hash = create_test_content_hash(&env);
    let hint_id = client.create_hint(&creator, &1u32, &content_hash, &HintQuality::Excellent);

    let listing_id = client.create_listing(
        &seller,
        &hint_id,
        &token_contract_id,
        &1000,
        &86400,
        &500, // 5% royalty
    );

    // Mint tokens to buyer
    token_admin_client.mint(&buyer, &10000);

    // Initial balances
    let initial_seller_balance = token_client.balance(&seller);
    let initial_fee_recipient_balance = token_client.balance(&fee_recipient);
    let initial_creator_balance = token_client.balance(&creator);

    // Buy hint
    client.buy(&buyer, &listing_id);

    // Verify listing is sold
    let listing = client.get_listing(&listing_id).unwrap();
    assert_eq!(listing.status, ListingStatus::Sold);

    // Verify balances
    // Fee: 1000 * 250 / 10000 = 25
    // Royalty: 1000 * 500 / 10000 = 50
    // Seller gets: 1000 - 25 - 50 = 925
    assert_eq!(token_client.balance(&seller), initial_seller_balance + 925);
    assert_eq!(token_client.balance(&fee_recipient), initial_fee_recipient_balance + 25);
    assert_eq!(token_client.balance(&creator), initial_creator_balance + 50);

    // Verify hint sales updated
    let hint = client.get_hint(&hint_id).unwrap();
    assert_eq!(hint.total_sales, 1);
}

#[test]
fn test_rate_hint() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, HintMarketplace);
    let client = HintMarketplaceClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &admin, &250, &3600, &86400 * 30, &500, &HintQuality::Good);

    // Create hint
    let creator = Address::generate(&env);
    let rater = Address::generate(&env);
    let content_hash = create_test_content_hash(&env);
    let hint_id = client.create_hint(&creator, &1u32, &content_hash, &HintQuality::Good);

    // Rate hint
    client.rate_hint(&rater, &hint_id, &5, &5);

    // Verify rating
    let rating = client.get_rating(&hint_id, &rater).unwrap();
    assert_eq!(rating.quality_rating, 5);
    assert_eq!(rating.helpfulness, 5);

    // Verify hint quality updated
    let hint = client.get_hint(&hint_id).unwrap();
    assert_eq!(hint.rating_count, 1);
    assert_eq!(hint.total_rating, 5);
    assert_eq!(hint.quality, HintQuality::Perfect); // Average rating of 5
}

#[test]
fn test_create_and_buy_pack() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup token
    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = token::Client::new(&env, &token_contract_id);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_contract_id);

    // Setup marketplace
    let contract_id = env.register_contract(None, HintMarketplace);
    let client = HintMarketplaceClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    client.initialize(&admin, &fee_recipient, &250, &3600, &86400 * 30, &500, &HintQuality::Good);

    // Create hints
    let creator = Address::generate(&env);
    let buyer = Address::generate(&env);
    let content_hash = create_test_content_hash(&env);

    let hint_id1 = client.create_hint(&creator, &1u32, &content_hash, &HintQuality::Good);
    let hint_id2 = client.create_hint(&creator, &2u32, &content_hash, &HintQuality::Excellent);

    // Create pack
    let mut hint_ids = Vec::new(&env);
    hint_ids.push_back(hint_id1);
    hint_ids.push_back(hint_id2);

    let pack_id = client.create_pack(
        &creator,
        &symbol_short!("StarterPack"),
        &hint_ids,
        &1500, // Pack price (discount from 2000)
        &2500, // 25% discount
        &None,
    );

    // Verify pack
    let pack = client.get_pack(&pack_id).unwrap();
    assert_eq!(pack.creator, creator);
    assert_eq!(pack.pack_price, 1500);

    // Mint tokens to buyer
    token_admin_client.mint(&buyer, &10000);

    // Initial balances
    let initial_creator_balance = token_client.balance(&creator);
    let initial_fee_recipient_balance = token_client.balance(&fee_recipient);

    // Buy pack
    client.buy_pack(&buyer, &pack_id, &token_contract_id);

    // Verify balances
    // Fee: 1500 * 250 / 10000 = 37.5 (rounded to 37)
    // Creator gets: 1500 - 37 = 1463
    assert_eq!(token_client.balance(&creator), initial_creator_balance + 1463);
    assert_eq!(token_client.balance(&fee_recipient), initial_fee_recipient_balance + 37);

    // Verify hint sales updated
    let hint1 = client.get_hint(&hint_id1).unwrap();
    let hint2 = client.get_hint(&hint_id2).unwrap();
    assert_eq!(hint1.total_sales, 1);
    assert_eq!(hint2.total_sales, 1);
}

#[test]
fn test_listing_expiration() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, HintMarketplace);
    let client = HintMarketplaceClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &admin, &250, &3600, &86400 * 30, &500, &HintQuality::Good);

    // Create hint and listing
    let creator = Address::generate(&env);
    let content_hash = create_test_content_hash(&env);
    let hint_id = client.create_hint(&creator, &1u32, &content_hash, &HintQuality::Good);

    let token_contract_id = env.register_stellar_asset_contract_v2(admin.clone()).address();

    // Set timestamp to a known value
    env.ledger().set_timestamp(1000);

    let listing_id = client.create_listing(
        &creator,
        &hint_id,
        &token_contract_id,
        &1000,
        &3600, // 1 hour duration
        &500,
    );

    // Verify listing is active
    let listing = client.get_listing(&listing_id).unwrap();
    assert_eq!(listing.status, ListingStatus::Active);
    assert_eq!(listing.expiration_time, 1000 + 3600);

    // Expire listing by moving time forward
    env.ledger().set_timestamp(5000);

    // Manually expire listing
    let mut listing_ids = Vec::new(&env);
    listing_ids.push_back(listing_id);
    client.expire_listings(&listing_ids);

    // Verify listing is expired
    let listing = client.get_listing(&listing_id).unwrap();
    assert_eq!(listing.status, ListingStatus::Expired);
}

#[test]
#[should_panic(expected = "Listing has expired")]
fn test_buy_expired_listing() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup token
    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_contract_id);

    // Setup marketplace
    let contract_id = env.register_contract(None, HintMarketplace);
    let client = HintMarketplaceClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &admin, &250, &3600, &86400 * 30, &500, &HintQuality::Good);

    // Create hint and listing
    let creator = Address::generate(&env);
    let buyer = Address::generate(&env);
    let content_hash = create_test_content_hash(&env);
    let hint_id = client.create_hint(&creator, &1u32, &content_hash, &HintQuality::Good);

    // Set timestamp
    env.ledger().set_timestamp(1000);

    let listing_id = client.create_listing(
        &creator,
        &hint_id,
        &token_contract_id,
        &1000,
        &3600, // 1 hour duration
        &500,
    );

    // Mint tokens to buyer
    token_admin_client.mint(&buyer, &10000);

    // Move time forward past expiration
    env.ledger().set_timestamp(5000);

    // Try to buy expired listing - should panic
    client.buy(&buyer, &listing_id);
}

#[test]
fn test_dynamic_pricing() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, HintMarketplace);
    let client = HintMarketplaceClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &admin, &250, &3600, &86400 * 30, &1000, &HintQuality::Good);

    // Create hints with different qualities
    let creator = Address::generate(&env);
    let content_hash = create_test_content_hash(&env);

    let poor_hint_id = client.create_hint(&creator, &1u32, &content_hash, &HintQuality::Poor);
    let excellent_hint_id = client.create_hint(&creator, &2u32, &content_hash, &HintQuality::Excellent);

    let token_contract_id = env.register_stellar_asset_contract_v2(admin.clone()).address();
    let base_price = 1000i128;

    // Create listings
    let poor_listing_id = client.create_listing(
        &creator,
        &poor_hint_id,
        &token_contract_id,
        &base_price,
        &86400,
        &500,
    );

    let excellent_listing_id = client.create_listing(
        &creator,
        &excellent_hint_id,
        &token_contract_id,
        &base_price,
        &86400,
        &500,
    );

    // Verify dynamic pricing
    let poor_listing = client.get_listing(&poor_listing_id).unwrap();
    let excellent_listing = client.get_listing(&excellent_listing_id).unwrap();

    // Excellent quality should have higher price than poor quality
    assert!(excellent_listing.current_price > poor_listing.current_price);
}

#[test]
fn test_cancel_listing() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, HintMarketplace);
    let client = HintMarketplaceClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &admin, &250, &3600, &86400 * 30, &500, &HintQuality::Good);

    // Create hint and listing
    let creator = Address::generate(&env);
    let content_hash = create_test_content_hash(&env);
    let hint_id = client.create_hint(&creator, &1u32, &content_hash, &HintQuality::Good);

    let token_contract_id = env.register_stellar_asset_contract_v2(admin.clone()).address();

    let listing_id = client.create_listing(
        &creator,
        &hint_id,
        &token_contract_id,
        &1000,
        &86400,
        &500,
    );

    // Verify listing is active
    let listing = client.get_listing(&listing_id).unwrap();
    assert_eq!(listing.status, ListingStatus::Active);

    // Cancel listing
    client.cancel_listing(&creator, &listing_id);

    // Verify listing is cancelled
    let listing = client.get_listing(&listing_id).unwrap();
    assert_eq!(listing.status, ListingStatus::Cancelled);

    // Verify removed from active listings
    let active_listings = client.get_active_listings();
    assert!(!active_listings.contains(&listing_id));
}

#[test]
fn test_multiple_ratings() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, HintMarketplace);
    let client = HintMarketplaceClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &admin, &250, &3600, &86400 * 30, &500, &HintQuality::Good);

    // Create hint
    let creator = Address::generate(&env);
    let content_hash = create_test_content_hash(&env);
    let hint_id = client.create_hint(&creator, &1u32, &content_hash, &HintQuality::Good);

    // Multiple raters
    let rater1 = Address::generate(&env);
    let rater2 = Address::generate(&env);
    let rater3 = Address::generate(&env);

    // Rate hint multiple times
    client.rate_hint(&rater1, &hint_id, &5, &5);
    client.rate_hint(&rater2, &hint_id, &4, &4);
    client.rate_hint(&rater3, &hint_id, &3, &3);

    // Verify hint statistics
    let hint = client.get_hint(&hint_id).unwrap();
    assert_eq!(hint.rating_count, 3);
    assert_eq!(hint.total_rating, 12); // 5 + 4 + 3
    // Average: 12 / 3 = 4, so quality should be Excellent
    assert_eq!(hint.quality, HintQuality::Excellent);

    // Verify all ratings exist
    let rating1 = client.get_rating(&hint_id, &rater1).unwrap();
    let rating2 = client.get_rating(&hint_id, &rater2).unwrap();
    let rating3 = client.get_rating(&hint_id, &rater3).unwrap();

    assert_eq!(rating1.quality_rating, 5);
    assert_eq!(rating2.quality_rating, 4);
    assert_eq!(rating3.quality_rating, 3);
}

#[test]
#[should_panic(expected = "Already rated this hint")]
fn test_duplicate_rating() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, HintMarketplace);
    let client = HintMarketplaceClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &admin, &250, &3600, &86400 * 30, &500, &HintQuality::Good);

    // Create hint
    let creator = Address::generate(&env);
    let rater = Address::generate(&env);
    let content_hash = create_test_content_hash(&env);
    let hint_id = client.create_hint(&creator, &1u32, &content_hash, &HintQuality::Good);

    // Rate twice - should panic
    client.rate_hint(&rater, &hint_id, &5, &5);
    client.rate_hint(&rater, &hint_id, &4, &4);
}

#[test]
#[should_panic(expected = "Hint quality below minimum requirement")]
fn test_minimum_quality_requirement() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, HintMarketplace);
    let client = HintMarketplaceClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    // Set minimum quality to Good
    client.initialize(&admin, &admin, &250, &3600, &86400 * 30, &500, &HintQuality::Good);

    // Create hint with Poor quality
    let creator = Address::generate(&env);
    let content_hash = create_test_content_hash(&env);
    let hint_id = client.create_hint(&creator, &1u32, &content_hash, &HintQuality::Poor);

    let token_contract_id = env.register_stellar_asset_contract_v2(admin.clone()).address();

    // Try to list - should panic
    client.create_listing(
        &creator,
        &hint_id,
        &token_contract_id,
        &1000,
        &86400,
        &500,
    );
}

#[test]
fn test_price_history() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup token
    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_contract_id);

    // Setup marketplace
    let contract_id = env.register_contract(None, HintMarketplace);
    let client = HintMarketplaceClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &admin, &250, &3600, &86400 * 30, &500, &HintQuality::Good);

    // Create hint and multiple listings
    let creator = Address::generate(&env);
    let buyer1 = Address::generate(&env);
    let buyer2 = Address::generate(&env);
    let content_hash = create_test_content_hash(&env);
    let hint_id = client.create_hint(&creator, &1u32, &content_hash, &HintQuality::Good);

    // Mint tokens
    token_admin_client.mint(&buyer1, &10000);
    token_admin_client.mint(&buyer2, &10000);

    // Create and buy first listing
    let listing_id1 = client.create_listing(
        &creator,
        &hint_id,
        &token_contract_id,
        &1000,
        &86400,
        &500,
    );
    client.buy(&buyer1, &listing_id1);

    // Create and buy second listing
    let listing_id2 = client.create_listing(
        &creator,
        &hint_id,
        &token_contract_id,
        &1500,
        &86400,
        &500,
    );
    client.buy(&buyer2, &listing_id2);

    // Check price history
    let history = client.get_price_history(&hint_id);
    assert_eq!(history.len(), 2);
}

#[test]
fn test_demand_metrics() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup token
    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_contract_id);

    // Setup marketplace
    let contract_id = env.register_contract(None, HintMarketplace);
    let client = HintMarketplaceClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &admin, &250, &3600, &86400 * 30, &500, &HintQuality::Good);

    // Create hint
    let creator = Address::generate(&env);
    let buyer = Address::generate(&env);
    let content_hash = create_test_content_hash(&env);
    let hint_id = client.create_hint(&creator, &1u32, &content_hash, &HintQuality::Good);

    // Mint tokens
    token_admin_client.mint(&buyer, &10000);

    // Set timestamp
    env.ledger().set_timestamp(1000);

    // Create listing
    let listing_id = client.create_listing(
        &creator,
        &hint_id,
        &token_contract_id,
        &1000,
        &86400,
        &500,
    );

    // Move time forward and buy
    env.ledger().set_timestamp(2000);
    client.buy(&buyer, &listing_id);

    // Check demand metrics
    let metrics = client.get_demand_metrics(&hint_id).unwrap();
    assert_eq!(metrics.purchases, 1);
    assert_eq!(metrics.last_purchase_time, 2000);
    assert_eq!(metrics.average_time_to_sale, 1000); // 2000 - 1000
}
