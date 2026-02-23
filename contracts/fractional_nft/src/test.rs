#![cfg(test)]

use super::*;
use soroban_sdk::{contract, contractimpl, testutils::{Address as _, Ledger}, token, Address, Env, String};

// Mock NFT contract for testing fractional_nft
#[contract]
pub struct MockNFT;

#[contractimpl]
impl MockNFT {
    pub fn owner_of(env: Env, token_id: u32) -> Address {
        env.storage()
            .persistent()
            .get(&token_id)
            .unwrap_or_else(|| panic!("token_not_found"))
    }

    pub fn transfer(env: Env, from: Address, to: Address, token_id: u32) {
        from.require_auth();
        let current_owner: Address = env.storage()
            .persistent()
            .get(&token_id)
            .unwrap_or_else(|| panic!("token_not_found"));
        if current_owner != from {
            panic!("not_owner");
        }
        env.storage().persistent().set(&token_id, &to);
    }

    pub fn set_owner(env: Env, token_id: u32, owner: Address) {
        env.storage().persistent().set(&token_id, &owner);
    }
}

fn setup_token<'a>(
    env: &'a Env,
    admin: &'a Address,
) -> (Address, token::Client<'a>, token::StellarAssetClient<'a>) {
    let token_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_client = token::Client::new(env, &token_id);
    let token_admin_client = token::StellarAssetClient::new(env, &token_id);
    (token_id, token_client, token_admin_client)
}

fn setup_mock_nft<'a>(env: &'a Env) -> (Address, MockNFTClient<'a>) {
    let nft_id = env.register_contract(None, MockNFT);
    let client = MockNFTClient::new(env, &nft_id);
    (nft_id, client)
}

#[test]
fn test_fractionalize_and_transfer_shares() {
    let env = Env::default();
    env.mock_all_auths();

    let (nft_contract_id, nft) = setup_mock_nft(&env);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let token_id = 1u32;
    nft.set_owner(&token_id, &owner);

    let contract_id = env.register_contract(None, FractionalNftContract);
    let client = FractionalNftContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let vault_id = client.fractionalize(
        &owner,
        &nft_contract_id,
        &token_id,
        &100i128,
        &0u32,
        &None,
    );

    assert_eq!(client.get_vault(&vault_id).unwrap().total_shares, 100);
    assert_eq!(client.balance_of(&vault_id, &owner), 100);

    let alice = Address::generate(&env);
    client.transfer_shares(&vault_id, &owner, &alice, &25i128);

    assert_eq!(client.balance_of(&vault_id, &owner), 75);
    assert_eq!(client.balance_of(&vault_id, &alice), 25);
}

#[test]
#[should_panic(expected = "below_min_ownership_threshold")]
fn test_minimum_ownership_threshold_enforced() {
    let env = Env::default();
    env.mock_all_auths();

    let (nft_contract_id, nft) = setup_mock_nft(&env);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let token_id = 1u32;
    nft.set_owner(&token_id, &owner);

    let contract_id = env.register_contract(None, FractionalNftContract);
    let client = FractionalNftContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    // total_shares=100, min_ownership_bps=1000 => min_shares=10
    let vault_id = client.fractionalize(
        &owner,
        &nft_contract_id,
        &token_id,
        &100i128,
        &1000u32,
        &None,
    );

    let bob = Address::generate(&env);
    // This would leave bob with 5 shares (below 10) => should panic.
    client.transfer_shares(&vault_id, &owner, &bob, &5i128);
}

#[test]
fn test_share_trading_listing_buy_and_cancel() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let seller = Address::generate(&env);
    let buyer = Address::generate(&env);

    let (payment_token_id, payment_token, payment_admin) = setup_token(&env, &admin);
    payment_admin.mint(&buyer, &1_000);

    let (nft_contract_id, nft) = setup_mock_nft(&env);
    let token_id = 1u32;
    nft.set_owner(&token_id, &seller);

    let contract_id = env.register_contract(None, FractionalNftContract);
    let client = FractionalNftContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let vault_id = client.fractionalize(
        &seller,
        &nft_contract_id,
        &token_id,
        &100i128,
        &0u32,
        &None,
    );

    let listing_id = client.create_listing(
        &seller,
        &vault_id,
        &20i128,
        &payment_token_id,
        &2i128,
        &None,
    );

    assert_eq!(client.balance_of(&vault_id, &seller), 80);
    assert_eq!(client.balance_of(&vault_id, &contract_id), 20);

    client.buy_listing(&buyer, &listing_id);

    assert_eq!(client.balance_of(&vault_id, &buyer), 20);
    assert_eq!(client.balance_of(&vault_id, &contract_id), 0);

    // Buyer paid seller 40 tokens.
    assert_eq!(payment_token.balance(&buyer), 960);
    assert_eq!(payment_token.balance(&seller), 40);

    // Create another listing and cancel it.
    let listing_id2 = client.create_listing(
        &seller,
        &vault_id,
        &10i128,
        &payment_token_id,
        &1i128,
        &None,
    );
    assert_eq!(client.balance_of(&vault_id, &contract_id), 10);

    client.cancel_listing(&seller, &listing_id2);
    assert_eq!(client.balance_of(&vault_id, &contract_id), 0);
    assert_eq!(client.balance_of(&vault_id, &seller), 80);
}

#[test]
fn test_buyout_tender_and_reclaim() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let buyer = Address::generate(&env);
    let holder = Address::generate(&env);

    let (payment_token_id, payment_token, payment_admin) = setup_token(&env, &admin);
    payment_admin.mint(&buyer, &10_000);

    let (nft_contract_id, nft) = setup_mock_nft(&env);

    let original_owner = Address::generate(&env);
    let token_id = 1u32;
    nft.set_owner(&token_id, &original_owner);

    let contract_id = env.register_contract(None, FractionalNftContract);
    let client = FractionalNftContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let vault_id = client.fractionalize(
        &original_owner,
        &nft_contract_id,
        &token_id,
        &100i128,
        &0u32,
        &None,
    );

    // Move some shares to holder.
    client.transfer_shares(&vault_id, &original_owner, &holder, &30i128);

    env.ledger().set_timestamp(100);
    client.start_buyout(&buyer, &vault_id, &payment_token_id, &1_000i128, &200u64);

    // Holder tenders 10 shares => gets 1000 * 10 / 100 = 100.
    client.tender_shares(&holder, &vault_id, &10i128);
    assert_eq!(payment_token.balance(&holder), 100);
    assert_eq!(client.balance_of(&vault_id, &buyer), 10);

    // After end_time, buyer can reclaim remaining escrow.
    env.ledger().set_timestamp(201);
    let buyer_before = payment_token.balance(&buyer);
    client.reclaim_buyout_escrow(&buyer, &vault_id);
    let buyer_after = payment_token.balance(&buyer);
    assert!(buyer_after > buyer_before);
}

#[test]
fn test_profit_sharing_from_rentals_and_claim() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let payer = Address::generate(&env);
    let owner = Address::generate(&env);
    let other = Address::generate(&env);

    let (rental_token_id, rental_token, rental_admin) = setup_token(&env, &admin);
    rental_admin.mint(&payer, &1_000);

    let (nft_contract_id, nft) = setup_mock_nft(&env);
    let token_id = 1u32;
    nft.set_owner(&token_id, &owner);

    let contract_id = env.register_contract(None, FractionalNftContract);
    let client = FractionalNftContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let vault_id = client.fractionalize(
        &owner,
        &nft_contract_id,
        &token_id,
        &100i128,
        &0u32,
        &Some(rental_token_id.clone()),
    );

    // Transfer 40 shares to other.
    client.transfer_shares(&vault_id, &owner, &other, &40i128);

    client.deposit_rental_income(&payer, &vault_id, &500i128);

    // Owner has 60%, other has 40%.
    client.claim_rental_profit(&owner, &vault_id);
    client.claim_rental_profit(&other, &vault_id);

    assert_eq!(rental_token.balance(&owner), 300);
    assert_eq!(rental_token.balance(&other), 200);
}

#[test]
fn test_voting_set_rental_token() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let voter2 = Address::generate(&env);

    let (new_rental_token_id, _, _) = setup_token(&env, &admin);

    let (nft_contract_id, nft) = setup_mock_nft(&env);
    let token_id = 1u32;
    nft.set_owner(&token_id, &owner);

    let contract_id = env.register_contract(None, FractionalNftContract);
    let client = FractionalNftContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let vault_id = client.fractionalize(
        &owner,
        &nft_contract_id,
        &token_id,
        &100i128,
        &0u32,
        &None,
    );

    client.transfer_shares(&vault_id, &owner, &voter2, &30i128);

    env.ledger().set_timestamp(100);
    let proposal_id = client.create_proposal_set_rental_token(
        &owner,
        &vault_id,
        &new_rental_token_id,
        &50u64,
    );

    client.vote(&owner, &proposal_id, &true);
    client.vote(&voter2, &proposal_id, &true);

    env.ledger().set_timestamp(151);
    client.execute_proposal(&proposal_id);

    assert_eq!(client.get_vault(&vault_id).unwrap().rental_token, Some(new_rental_token_id));
}

#[test]
fn test_recombine_merge_fractions() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let receiver = Address::generate(&env);

    let (nft_contract_id, nft) = setup_mock_nft(&env);
    let token_id = 1u32;
    nft.set_owner(&token_id, &owner);

    let contract_id = env.register_contract(None, FractionalNftContract);
    let client = FractionalNftContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let vault_id = client.fractionalize(
        &owner,
        &nft_contract_id,
        &token_id,
        &100i128,
        &0u32,
        &None,
    );

    // Owner already holds 100 shares.
    client.recombine(&owner, &vault_id, &receiver);

    assert_eq!(nft.owner_of(&token_id), receiver);
    assert_eq!(client.get_vault(&vault_id).unwrap().active, false);
}
