#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, token, Address, Env};

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

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &10);

    let config = client.get_config();
    assert_eq!(config.admin, admin);
    assert_eq!(config.fee_bps, 10);
    assert_eq!(config.max_loan_ratio, 8000);
    assert_eq!(config.paused, false);

    let analytics = client.get_analytics();
    assert_eq!(analytics.total_loans, 0);
    assert_eq!(analytics.total_volume_borrowed, 0);
    assert_eq!(analytics.total_fees_collected, 0);
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_double_initialize() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &10);
    client.initialize(&admin, &10);
}

#[test]
#[should_panic(expected = "Fee must be between 10-30 basis points")]
fn test_invalid_fee_too_low() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &5);
}

#[test]
#[should_panic(expected = "Fee must be between 10-30 basis points")]
fn test_invalid_fee_too_high() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &50);
}

#[test]
fn test_add_liquidity() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let lender = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let (token_id, token_client, token_admin_client) = setup_token(&env, &token_admin);

    token_admin_client.mint(&lender, &10_000);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &10);
    client.add_liquidity(&lender, &token_id, &5_000);

    let pool = client.get_pool(&token_id).unwrap();
    assert_eq!(pool.total_liquidity, 5_000);
    assert_eq!(pool.available_liquidity, 5_000);
    assert_eq!(pool.fees_collected, 0);

    assert_eq!(token_client.balance(&lender), 5_000);
    assert_eq!(token_client.balance(&contract_id), 5_000);
}

#[test]
fn test_add_liquidity_multiple_times() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let lender = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let (token_id, _, token_admin_client) = setup_token(&env, &token_admin);

    token_admin_client.mint(&lender, &10_000);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &10);
    client.add_liquidity(&lender, &token_id, &3_000);
    client.add_liquidity(&lender, &token_id, &2_000);

    let pool = client.get_pool(&token_id).unwrap();
    assert_eq!(pool.total_liquidity, 5_000);
    assert_eq!(pool.available_liquidity, 5_000);
}

#[test]
fn test_remove_liquidity() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let lender = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let (token_id, token_client, token_admin_client) = setup_token(&env, &token_admin);

    token_admin_client.mint(&lender, &10_000);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &10);
    client.add_liquidity(&lender, &token_id, &5_000);
    client.remove_liquidity(&lender, &token_id, &2_000);

    let pool = client.get_pool(&token_id).unwrap();
    assert_eq!(pool.total_liquidity, 3_000);
    assert_eq!(pool.available_liquidity, 3_000);
    assert_eq!(token_client.balance(&lender), 7_000);
}

#[test]
#[should_panic(expected = "Insufficient available liquidity")]
fn test_remove_liquidity_too_much() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let lender = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let (token_id, _, token_admin_client) = setup_token(&env, &token_admin);

    token_admin_client.mint(&lender, &10_000);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &10);
    client.add_liquidity(&lender, &token_id, &5_000);
    client.remove_liquidity(&lender, &token_id, &10_000);
}

#[test]
fn test_calculate_fee() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &10);

    let fee = client.calculate_fee(&10_000);
    assert_eq!(fee, 10);
}

#[test]
fn test_set_fee_bps() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &10);
    client.set_fee_bps(&admin, &20);

    let config = client.get_config();
    assert_eq!(config.fee_bps, 20);

    let fee = client.calculate_fee(&10_000);
    assert_eq!(fee, 20);
}

#[test]
fn test_set_max_loan_ratio() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &10);
    client.set_max_loan_ratio(&admin, &5000);

    let config = client.get_config();
    assert_eq!(config.max_loan_ratio, 5000);
}

#[test]
fn test_pause_unpause() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &10);
    client.pause(&admin);

    let config = client.get_config();
    assert_eq!(config.paused, true);

    client.unpause(&admin);
    let config = client.get_config();
    assert_eq!(config.paused, false);
}

#[test]
fn test_get_analytics() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &10);

    let analytics = client.get_analytics();
    assert_eq!(analytics.total_loans, 0);
    assert_eq!(analytics.total_volume_borrowed, 0);
    assert_eq!(analytics.total_fees_collected, 0);
    assert_eq!(analytics.total_repaid, 0);
    assert_eq!(analytics.defaulted_loans, 0);
}

#[test]
fn test_get_all_pools() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let lender = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let (token_id, _, token_admin_client) = setup_token(&env, &token_admin);
    token_admin_client.mint(&lender, &10_000);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &10);
    client.add_liquidity(&lender, &token_id, &5_000);

    let pools = client.get_all_pools();
    assert_eq!(pools.len(), 1);
    assert_eq!(pools.get(0).unwrap(), token_id);
}

#[test]
fn test_get_lender_position() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let lender = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let (token_id, _, token_admin_client) = setup_token(&env, &token_admin);
    token_admin_client.mint(&lender, &10_000);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &10);
    client.add_liquidity(&lender, &token_id, &5_000);

    let position = client.get_lender_position(&token_id, &lender).unwrap();
    assert_eq!(position.amount, 5_000);
    assert_eq!(position.lender, lender);
}

#[test]
#[should_panic(expected = "Admin only")]
fn test_non_admin_set_fee() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let non_admin = Address::generate(&env);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &10);
    client.set_fee_bps(&non_admin, &20);
}

#[test]
#[should_panic(expected = "Contract is paused")]
fn test_paused_add_liquidity() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let lender = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let (token_id, _, token_admin_client) = setup_token(&env, &token_admin);
    token_admin_client.mint(&lender, &10_000);

    let contract_id = env.register_contract(None, FlashLoanContract);
    let client = FlashLoanContractClient::new(&env, &contract_id);

    client.initialize(&admin, &10);
    client.pause(&admin);
    client.add_liquidity(&lender, &token_id, &5_000);
}
