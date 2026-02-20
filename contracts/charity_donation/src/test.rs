#![cfg(test)]
use super::*;
use soroban_sdk::{testutils::Address as _, token, Address, Env, String};

fn create_token_contract<'a>(env: &Env, admin: &Address) -> (token::Client<'a>, token::StellarAssetClient<'a>) {
    let contract_address = env.register_stellar_asset_contract(admin.clone());
    (
        token::Client::new(env, &contract_address),
        token::StellarAssetClient::new(env, &contract_address),
    )
}

#[test]
fn test_donation_flow() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let donor = Address::generate(&env);
    let charity_wallet = Address::generate(&env);

    let (token, token_admin) = create_token_contract(&env, &admin);
    token_admin.mint(&donor, &10000);

    let contract_id = env.register_contract(None, CharityContract);
    let client = CharityContractClient::new(&env, &contract_id);

    client.initialize(&admin, &token.address);

    let charity_id = client.add_charity(&admin, &String::from_str(&env, "Test Charity"), &charity_wallet);
    assert_eq!(charity_id, 1);

    client.verify_charity(&admin, &charity_id);

    client.donate(&donor, &charity_id, &1000);

    let charity = client.get_charity(&charity_id);
    assert_eq!(charity.total_raised, 1000);
    assert_eq!(charity.contributor_count, 1);

    let donor_total = client.get_donor_total(&donor, &charity_id);
    assert_eq!(donor_total, 1000);

    assert_eq!(token.balance(&charity_wallet), 1000);
}

#[test]
fn test_quadratic_funding() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let donor1 = Address::generate(&env);
    let donor2 = Address::generate(&env);
    let funder = Address::generate(&env);
    let charity_wallet = Address::generate(&env);

    let (token, token_admin) = create_token_contract(&env, &admin);
    token_admin.mint(&donor1, &10000);
    token_admin.mint(&donor2, &10000);
    token_admin.mint(&funder, &50000);

    let contract_id = env.register_contract(None, CharityContract);
    let client = CharityContractClient::new(&env, &contract_id);

    client.initialize(&admin, &token.address);

    let charity_id = client.add_charity(&admin, &String::from_str(&env, "QF Charity"), &charity_wallet);
    client.verify_charity(&admin, &charity_id);

    client.donate(&donor1, &charity_id, &100);
    client.donate(&donor2, &charity_id, &100);

    client.fund_matching_pool(&funder, &10000);

    let matched = client.distribute_matching(&admin, &charity_id);
    assert!(matched > 0);

    let final_balance = token.balance(&charity_wallet);
    assert_eq!(final_balance, 200 + matched);
}

#[test]
fn test_receipt_issuance() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let donor = Address::generate(&env);
    let charity_wallet = Address::generate(&env);

    let (token, token_admin) = create_token_contract(&env, &admin);
    token_admin.mint(&donor, &10000);

    let contract_id = env.register_contract(None, CharityContract);
    let client = CharityContractClient::new(&env, &contract_id);

    client.initialize(&admin, &token.address);

    let charity_id = client.add_charity(&admin, &String::from_str(&env, "Receipt Charity"), &charity_wallet);
    client.verify_charity(&admin, &charity_id);

    client.donate(&donor, &charity_id, &500);

    let receipt_id = client.issue_receipt(&donor, &charity_id);
    assert_eq!(receipt_id, 1);
}

#[test]
fn test_recurring_donations() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let donor = Address::generate(&env);
    let charity_wallet = Address::generate(&env);

    let (token, _) = create_token_contract(&env, &admin);

    let contract_id = env.register_contract(None, CharityContract);
    let client = CharityContractClient::new(&env, &contract_id);

    client.initialize(&admin, &token.address);

    let charity_id = client.add_charity(&admin, &String::from_str(&env, "Recurring Charity"), &charity_wallet);
    client.verify_charity(&admin, &charity_id);

    client.set_recurring(&donor, &charity_id, &100, &true);
    client.set_recurring(&donor, &charity_id, &100, &false);
}

#[test]
fn test_leaderboard() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let donor1 = Address::generate(&env);
    let donor2 = Address::generate(&env);
    let charity_wallet = Address::generate(&env);

    let (token, token_admin) = create_token_contract(&env, &admin);
    token_admin.mint(&donor1, &10000);
    token_admin.mint(&donor2, &10000);

    let contract_id = env.register_contract(None, CharityContract);
    let client = CharityContractClient::new(&env, &contract_id);

    client.initialize(&admin, &token.address);

    let charity_id = client.add_charity(&admin, &String::from_str(&env, "Leaderboard Charity"), &charity_wallet);
    client.verify_charity(&admin, &charity_id);

    client.donate(&donor1, &charity_id, &1000);
    client.donate(&donor2, &charity_id, &500);

    let leaderboard = client.get_leaderboard();
    assert_eq!(leaderboard.len(), 2);
    assert_eq!(leaderboard.get(0).unwrap().0, donor1);
    assert_eq!(leaderboard.get(0).unwrap().1, 1000);
}

#[test]
#[should_panic(expected = "Charity not verified")]
fn test_unverified_charity_donation() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let donor = Address::generate(&env);
    let charity_wallet = Address::generate(&env);

    let (token, token_admin) = create_token_contract(&env, &admin);
    token_admin.mint(&donor, &10000);

    let contract_id = env.register_contract(None, CharityContract);
    let client = CharityContractClient::new(&env, &contract_id);

    client.initialize(&admin, &token.address);

    let charity_id = client.add_charity(&admin, &String::from_str(&env, "Unverified"), &charity_wallet);

    client.donate(&donor, &charity_id, &1000);
}
