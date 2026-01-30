#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, Env, symbol_short,
};

// Helper function to create a mock NFT contract
fn create_mock_nft(env: &Env) -> Address {
    // For testing, we'll use a simple mock that tracks ownership
    // In a real scenario, this would be the actual NFT contract
    Address::generate(env)
}

// Helper function to create a test asset
fn create_test_asset(env: &Env, nft_contract: Address, token_id: u32) -> Asset {
    Asset {
        asset_type: AssetType::NFT,
        contract: nft_contract,
        token_id,
    }
}

#[test]
fn test_initialize() {
    let env = Env::default();
    let contract_id = env.register_contract(None, MarketplaceContract);
    let client = MarketplaceContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);

    client.initialize(
        &admin,
        &fee_recipient,
        &250, // 2.5% fee
        &3600, // 1 hour min
        &86400 * 30, // 30 days max
    );

    let config = client.get_config();
    assert_eq!(config.admin, admin);
    assert_eq!(config.fee_recipient, fee_recipient);
    assert_eq!(config.fee_bps, 250);
}

#[test]
fn test_create_listing() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup token
    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = token::Client::new(&env, &token_contract_id);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_contract_id);

    // Setup marketplace
    let contract_id = env.register_contract(None, MarketplaceContract);
    let client = MarketplaceContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    client.initialize(&admin, &fee_recipient, &250, &3600, &86400 * 30);

    // Setup seller and NFT
    let seller = Address::generate(&env);
    let nft_contract = create_mock_nft(&env);
    let token_id = 1u32;

    // Create listing
    let asset = create_test_asset(&env, nft_contract.clone(), token_id);
    let creator = Some(Address::generate(&env));
    let listing_id = client.create_listing(
        &seller,
        &asset,
        &token_contract_id,
        &1000, // price
        &creator,
        &500, // 5% royalty
    );

    // Verify listing
    let listing = client.get_listing(&listing_id).unwrap();
    assert_eq!(listing.seller, seller);
    assert_eq!(listing.price, 1000);
    assert_eq!(listing.status, ListingStatus::Active);
    assert_eq!(listing.creator, creator);
    assert_eq!(listing.royalty_bps, 500);

    // Verify listing appears in seller's listings
    let seller_listings = client.get_listings_by_seller(&seller);
    assert!(seller_listings.contains(&listing_id));

    // Verify listing appears in active listings
    let active_listings = client.get_active_listings();
    assert!(active_listings.contains(&listing_id));
}

#[test]
fn test_buy_listing() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup token
    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = token::Client::new(&env, &token_contract_id);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_contract_id);

    // Setup marketplace
    let contract_id = env.register_contract(None, MarketplaceContract);
    let client = MarketplaceContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    client.initialize(&admin, &fee_recipient, &250, &3600, &86400 * 30);

    // Setup users
    let seller = Address::generate(&env);
    let buyer = Address::generate(&env);
    let creator = Address::generate(&env);

    // Mint tokens to buyer
    token_admin_client.mint(&buyer, &10000);

    // Create listing
    let nft_contract = create_mock_nft(&env);
    let asset = create_test_asset(&env, nft_contract.clone(), 1u32);
    let listing_id = client.create_listing(
        &seller,
        &asset,
        &token_contract_id,
        &1000,
        &Some(creator.clone()),
        &500, // 5% royalty
    );

    // Initial balances
    let initial_seller_balance = token_client.balance(&seller);
    let initial_fee_recipient_balance = token_client.balance(&fee_recipient);
    let initial_creator_balance = token_client.balance(&creator);

    // Buy listing
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
    assert_eq!(token_client.balance(&buyer), 10000 - 1000);

    // Verify price history
    let history = client.get_price_history(&nft_contract, &1u32);
    assert_eq!(history.len(), 1);
    assert_eq!(history.get(0).unwrap(), &1000);
}

#[test]
fn test_create_and_accept_offer() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup token
    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = token::Client::new(&env, &token_contract_id);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_contract_id);

    // Setup marketplace
    let contract_id = env.register_contract(None, MarketplaceContract);
    let client = MarketplaceContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    client.initialize(&admin, &fee_recipient, &250, &3600, &86400 * 30);

    // Setup users
    let seller = Address::generate(&env);
    let buyer = Address::generate(&env);
    let creator = Address::generate(&env);

    // Mint tokens to buyer
    token_admin_client.mint(&buyer, &10000);

    // Create listing
    let nft_contract = create_mock_nft(&env);
    let asset = create_test_asset(&env, nft_contract.clone(), 1u32);
    let listing_id = client.create_listing(
        &seller,
        &asset,
        &token_contract_id,
        &1000,
        &Some(creator.clone()),
        &500,
    );

    // Create offer (lower than listing price)
    let offer_id = client.create_offer(&buyer, &listing_id, &800, &None);

    // Verify offer
    let offer = client.get_offer(&offer_id).unwrap();
    assert_eq!(offer.buyer, buyer);
    assert_eq!(offer.price, 800);
    assert_eq!(offer.status, OfferStatus::Open);

    // Verify buyer's tokens are escrowed
    assert_eq!(token_client.balance(&buyer), 10000 - 800);
    assert_eq!(token_client.balance(&contract_id), 800);

    // Accept offer
    let initial_seller_balance = token_client.balance(&seller);
    let initial_fee_recipient_balance = token_client.balance(&fee_recipient);
    let initial_creator_balance = token_client.balance(&creator);

    client.accept_offer(&seller, &offer_id);

    // Verify offer is accepted
    let offer = client.get_offer(&offer_id).unwrap();
    assert_eq!(offer.status, OfferStatus::Accepted);

    // Verify listing is sold
    let listing = client.get_listing(&listing_id).unwrap();
    assert_eq!(listing.status, ListingStatus::Sold);

    // Verify balances
    // Fee: 800 * 250 / 10000 = 20
    // Royalty: 800 * 500 / 10000 = 40
    // Seller gets: 800 - 20 - 40 = 740
    assert_eq!(token_client.balance(&seller), initial_seller_balance + 740);
    assert_eq!(token_client.balance(&fee_recipient), initial_fee_recipient_balance + 20);
    assert_eq!(token_client.balance(&creator), initial_creator_balance + 40);
}

#[test]
fn test_counter_offer() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup token
    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = token::Client::new(&env, &token_contract_id);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_contract_id);

    // Setup marketplace
    let contract_id = env.register_contract(None, MarketplaceContract);
    let client = MarketplaceContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    client.initialize(&admin, &fee_recipient, &250, &3600, &86400 * 30);

    // Setup users
    let seller = Address::generate(&env);
    let buyer = Address::generate(&env);

    // Mint tokens
    token_admin_client.mint(&buyer, &10000);

    // Create listing
    let nft_contract = create_mock_nft(&env);
    let asset = create_test_asset(&env, nft_contract.clone(), 1u32);
    let listing_id = client.create_listing(
        &seller,
        &asset,
        &token_contract_id,
        &1000,
        &None,
        &0,
    );

    // Create offer
    let offer_id = client.create_offer(&buyer, &listing_id, &800, &None);

    // Create counter offer
    let counter_offer_id = client.create_counter_offer(&seller, &offer_id, &900, &None);

    // Verify counter offer
    let counter_offer = client.get_counter_offer(&counter_offer_id).unwrap();
    assert_eq!(counter_offer.price, 900);
    assert_eq!(counter_offer.seller, seller);

    // Verify original offer is marked as countered
    let offer = client.get_offer(&offer_id).unwrap();
    assert_eq!(offer.status, OfferStatus::Countered);

    // Accept counter offer
    client.accept_counter_offer(&buyer, &counter_offer_id);

    // Verify listing is sold
    let listing = client.get_listing(&listing_id).unwrap();
    assert_eq!(listing.status, ListingStatus::Sold);

    // Verify buyer paid the difference (900 - 800 = 100)
    assert_eq!(token_client.balance(&buyer), 10000 - 900);
}

#[test]
fn test_cancel_listing() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup token
    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_contract_id);

    // Setup marketplace
    let contract_id = env.register_contract(None, MarketplaceContract);
    let client = MarketplaceContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    client.initialize(&admin, &fee_recipient, &250, &3600, &86400 * 30);

    // Setup users
    let seller = Address::generate(&env);
    let buyer = Address::generate(&env);

    // Mint tokens to buyer
    token_admin_client.mint(&buyer, &10000);

    // Create listing
    let nft_contract = create_mock_nft(&env);
    let asset = create_test_asset(&env, nft_contract.clone(), 1u32);
    let listing_id = client.create_listing(
        &seller,
        &asset,
        &token_contract_id,
        &1000,
        &None,
        &0,
    );

    // Create offer
    let token_client = token::Client::new(&env, &token_contract_id);
    let offer_id = client.create_offer(&buyer, &listing_id, &800, &None);

    // Verify offer is escrowed
    assert_eq!(token_client.balance(&contract_id), 800);
    assert_eq!(token_client.balance(&buyer), 10000 - 800);

    // Cancel listing
    client.cancel_listing(&seller, &listing_id);

    // Verify listing is cancelled
    let listing = client.get_listing(&listing_id).unwrap();
    assert_eq!(listing.status, ListingStatus::Cancelled);

    // Verify offer was refunded
    assert_eq!(token_client.balance(&buyer), 10000);
    assert_eq!(token_client.balance(&contract_id), 0);

    // Verify offer status
    let offer = client.get_offer(&offer_id).unwrap();
    assert_eq!(offer.status, OfferStatus::Cancelled);
}

#[test]
fn test_cancel_offer() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup token
    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = token::Client::new(&env, &token_contract_id);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_contract_id);

    // Setup marketplace
    let contract_id = env.register_contract(None, MarketplaceContract);
    let client = MarketplaceContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    client.initialize(&admin, &fee_recipient, &250, &3600, &86400 * 30);

    // Setup users
    let seller = Address::generate(&env);
    let buyer = Address::generate(&env);

    // Mint tokens to buyer
    token_admin_client.mint(&buyer, &10000);

    // Create listing
    let nft_contract = create_mock_nft(&env);
    let asset = create_test_asset(&env, nft_contract.clone(), 1u32);
    let listing_id = client.create_listing(
        &seller,
        &asset,
        &token_contract_id,
        &1000,
        &None,
        &0,
    );

    // Create offer
    let offer_id = client.create_offer(&buyer, &listing_id, &800, &None);

    // Verify offer is escrowed
    assert_eq!(token_client.balance(&contract_id), 800);
    assert_eq!(token_client.balance(&buyer), 10000 - 800);

    // Cancel offer
    client.cancel_offer(&buyer, &offer_id);

    // Verify offer is cancelled
    let offer = client.get_offer(&offer_id).unwrap();
    assert_eq!(offer.status, OfferStatus::Cancelled);

    // Verify refund
    assert_eq!(token_client.balance(&buyer), 10000);
    assert_eq!(token_client.balance(&contract_id), 0);
}

#[test]
fn test_reject_offer() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup token
    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = token::Client::new(&env, &token_contract_id);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_contract_id);

    // Setup marketplace
    let contract_id = env.register_contract(None, MarketplaceContract);
    let client = MarketplaceContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    client.initialize(&admin, &fee_recipient, &250, &3600, &86400 * 30);

    // Setup users
    let seller = Address::generate(&env);
    let buyer = Address::generate(&env);

    // Mint tokens to buyer
    token_admin_client.mint(&buyer, &10000);

    // Create listing
    let nft_contract = create_mock_nft(&env);
    let asset = create_test_asset(&env, nft_contract.clone(), 1u32);
    let listing_id = client.create_listing(
        &seller,
        &asset,
        &token_contract_id,
        &1000,
        &None,
        &0,
    );

    // Create offer
    let offer_id = client.create_offer(&buyer, &listing_id, &800, &None);

    // Reject offer
    client.reject_offer(&seller, &offer_id);

    // Verify offer is rejected
    let offer = client.get_offer(&offer_id).unwrap();
    assert_eq!(offer.status, OfferStatus::Rejected);

    // Verify refund
    assert_eq!(token_client.balance(&buyer), 10000);
    assert_eq!(token_client.balance(&contract_id), 0);
}

#[test]
fn test_price_discovery() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup token
    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_contract_id);

    // Setup marketplace
    let contract_id = env.register_contract(None, MarketplaceContract);
    let client = MarketplaceContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    client.initialize(&admin, &fee_recipient, &250, &3600, &86400 * 30);

    // Setup users
    let seller1 = Address::generate(&env);
    let seller2 = Address::generate(&env);
    let buyer1 = Address::generate(&env);
    let buyer2 = Address::generate(&env);

    // Mint tokens
    token_admin_client.mint(&buyer1, &10000);
    token_admin_client.mint(&buyer2, &10000);

    let nft_contract = create_mock_nft(&env);
    let asset = create_test_asset(&env, nft_contract.clone(), 1u32);

    // Create and sell first listing
    let listing_id1 = client.create_listing(
        &seller1,
        &asset,
        &token_contract_id,
        &1000,
        &None,
        &0,
    );
    client.buy(&buyer1, &listing_id1);

    // Create and sell second listing (different price)
    let listing_id2 = client.create_listing(
        &seller2,
        &asset,
        &token_contract_id,
        &1500,
        &None,
        &0,
    );
    client.buy(&buyer2, &listing_id2);

    // Check price history
    let history = client.get_price_history(&nft_contract, &1u32);
    assert_eq!(history.len(), 2);
    assert_eq!(history.get(0).unwrap(), &1000);
    assert_eq!(history.get(1).unwrap(), &1500);

    // Check average price
    let avg_price = client.get_average_price(&nft_contract, &1u32).unwrap();
    assert_eq!(avg_price, 1250);

    // Check min price
    let min_price = client.get_min_price(&nft_contract, &1u32).unwrap();
    assert_eq!(min_price, 1000);

    // Check max price
    let max_price = client.get_max_price(&nft_contract, &1u32).unwrap();
    assert_eq!(max_price, 1500);
}

#[test]
fn test_multiple_offers_refund() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup token
    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_client = token::Client::new(&env, &token_contract_id);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_contract_id);

    // Setup marketplace
    let contract_id = env.register_contract(None, MarketplaceContract);
    let client = MarketplaceContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    client.initialize(&admin, &fee_recipient, &250, &3600, &86400 * 30);

    // Setup users
    let seller = Address::generate(&env);
    let buyer1 = Address::generate(&env);
    let buyer2 = Address::generate(&env);
    let buyer3 = Address::generate(&env);

    // Mint tokens
    token_admin_client.mint(&buyer1, &10000);
    token_admin_client.mint(&buyer2, &10000);
    token_admin_client.mint(&buyer3, &10000);

    // Create listing
    let nft_contract = create_mock_nft(&env);
    let asset = create_test_asset(&env, nft_contract.clone(), 1u32);
    let listing_id = client.create_listing(
        &seller,
        &asset,
        &token_contract_id,
        &1000,
        &None,
        &0,
    );

    // Create multiple offers
    let offer_id1 = client.create_offer(&buyer1, &listing_id, &800, &None);
    let offer_id2 = client.create_offer(&buyer2, &listing_id, &900, &None);
    let offer_id3 = client.create_offer(&buyer3, &listing_id, &950, &None);

    // Verify all offers are escrowed
    assert_eq!(token_client.balance(&contract_id), 800 + 900 + 950);

    // Accept one offer
    client.accept_offer(&seller, &offer_id2);

    // Verify other offers are refunded
    assert_eq!(token_client.balance(&buyer1), 10000);
    assert_eq!(token_client.balance(&buyer3), 10000);
    assert_eq!(token_client.balance(&contract_id), 0);

    // Verify offer statuses
    let offer1 = client.get_offer(&offer_id1).unwrap();
    let offer2 = client.get_offer(&offer_id2).unwrap();
    let offer3 = client.get_offer(&offer_id3).unwrap();

    assert_eq!(offer1.status, OfferStatus::Cancelled);
    assert_eq!(offer2.status, OfferStatus::Accepted);
    assert_eq!(offer3.status, OfferStatus::Cancelled);
}

#[test]
fn test_update_config() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup marketplace
    let contract_id = env.register_contract(None, MarketplaceContract);
    let client = MarketplaceContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    let new_fee_recipient = Address::generate(&env);

    client.initialize(&admin, &fee_recipient, &250, &3600, &86400 * 30);

    // Update config
    client.update_config(
        &Some(new_fee_recipient.clone()),
        &Some(300), // 3% fee
        &None,
        &None,
    );

    // Verify config updated
    let config = client.get_config();
    assert_eq!(config.fee_recipient, new_fee_recipient);
    assert_eq!(config.fee_bps, 300);
}
