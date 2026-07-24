#![cfg(test)]

use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::{Address as _, Ledger},
    token, Address, Env, IntoVal, Symbol, Val,
};

use super::*;

fn default_reserve_config() -> ReserveConfig {
    ReserveConfig {
        collateral_factor_bps: 7500,
        liquidation_threshold_bps: 8000,
        liquidation_bonus_bps: 500,
        base_rate_bps: 100,
        slope1_bps: 400,
        slope2_bps: 3000,
        optimal_utilization_bps: 8000,
        reserve_factor_bps: 1000,
        is_active: true,
        is_frozen: false,
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

fn setup_lending_env() -> (
    Env,
    Address,
    token::StellarAssetClient<'static>,
    Address,
    token::StellarAssetClient<'static>,
    CollateralLendingContractClient<'static>,
    MockOracleClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let (collateral_token, _collateral_client, collateral_admin) =
        setup_token(&env, &admin);
    let (borrow_token, _borrow_client, borrow_admin) = setup_token(&env, &admin);

    let oracle_id = env.register_contract(None, MockOracle);
    let oracle_client = MockOracleClient::new(&env, &oracle_id);

    oracle_client.set_price(&Symbol::new(&env, "USD"), &100_000_000);

    let contract_id = env.register_contract(None, CollateralLendingContract);
    let client = CollateralLendingContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    client.set_oracle(&admin, &oracle_id);

    (
        env,
        collateral_token,
        collateral_admin,
        borrow_token,
        borrow_admin,
        client,
        oracle_client,
    )
}

fn configure_reserves(
    client: &CollateralLendingContractClient<'static>,
    admin: &Address,
    collateral_token: &Address,
    borrow_token: &Address,
) {
    let mut coll_config = default_reserve_config();
    coll_config.collateral_factor_bps = 7500;
    coll_config.liquidation_threshold_bps = 8000;

    let borrow_config = default_reserve_config();

    client.configure_reserve(admin, collateral_token, &coll_config);
    client.configure_reserve(admin, borrow_token, &borrow_config);
}

#[contracttype]
enum MockOracleDataKey {
    Price(Symbol),
}

#[contract]
struct MockOracle;

#[contractimpl]
impl MockOracle {
    pub fn set_price(env: Env, asset: Symbol, price: i128) {
        env.storage()
            .persistent()
            .set(&MockOracleDataKey::Price(asset), &price);
    }

    pub fn price(env: Env, asset: Symbol) -> i128 {
        env.storage()
            .persistent()
            .get(&MockOracleDataKey::Price(asset))
            .unwrap_or(0)
    }
}

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, CollateralLendingContract);
    let client = CollateralLendingContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    assert_eq!(client.get_reserve_list().len(), 0);
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_double_initialize() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, CollateralLendingContract);
    let client = CollateralLendingContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    client.initialize(&admin);
}

#[test]
fn test_set_oracle() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);

    let contract_id = env.register_contract(None, CollateralLendingContract);
    let client = CollateralLendingContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    assert!(client.get_oracle().is_none());

    client.set_oracle(&admin, &oracle);
    assert_eq!(client.get_oracle().unwrap(), oracle);
}

#[test]
fn test_configure_reserve() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let (token, _, _) = setup_token(&env, &admin);

    let contract_id = env.register_contract(None, CollateralLendingContract);
    let client = CollateralLendingContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    client.configure_reserve(&admin, &token, &default_reserve_config());

    let config = client.get_reserve_config(&token).unwrap();
    assert_eq!(config.collateral_factor_bps, 7500);
    assert_eq!(config.is_active, true);
    assert_eq!(config.is_frozen, false);

    let data = client.get_reserve_data(&token).unwrap();
    assert_eq!(data.total_liquidity, 0);
    assert_eq!(data.total_borrows, 0);

    let list = client.get_reserve_list();
    assert_eq!(list.len(), 1);
    assert_eq!(list.get(0).unwrap(), token);
}

#[test]
#[should_panic(expected = "Reserve already configured")]
fn test_configure_reserve_twice() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let (token, _, _) = setup_token(&env, &admin);

    let contract_id = env.register_contract(None, CollateralLendingContract);
    let client = CollateralLendingContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    client.configure_reserve(&admin, &token, &default_reserve_config());
    client.configure_reserve(&admin, &token, &default_reserve_config());
}

#[test]
fn test_update_reserve_config() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let (token, _, _) = setup_token(&env, &admin);

    let contract_id = env.register_contract(None, CollateralLendingContract);
    let client = CollateralLendingContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    client.configure_reserve(&admin, &token, &default_reserve_config());

    let mut updated = default_reserve_config();
    updated.collateral_factor_bps = 8000;
    client.update_reserve_config(&admin, &token, &updated);

    let config = client.get_reserve_config(&token).unwrap();
    assert_eq!(config.collateral_factor_bps, 8000);
}

#[test]
fn test_deposit() {
    let (env, collateral_token, collateral_admin, _, _, client, _) = setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &10_000);
    configure_reserves(&client, &admin, &collateral_token, &collateral_token);

    client.deposit(&user, &collateral_token, &5_000);

    let deposit = client.get_user_deposit(&user, &collateral_token);
    assert_eq!(deposit.amount, 5_000);

    let data = client.get_reserve_data(&collateral_token).unwrap();
    assert_eq!(data.total_liquidity, 5_000);
    assert_eq!(data.available_liquidity, 5_000);
}

#[test]
#[should_panic(expected = "Invalid amount")]
fn test_deposit_zero() {
    let (env, collateral_token, _, _, _, client, _) = setup_lending_env();
    let user = Address::generate(&env);
    client.deposit(&user, &collateral_token, &0);
}

#[test]
fn test_withdraw() {
    let (env, collateral_token, collateral_admin, _, _, client, _) = setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &10_000);
    configure_reserves(&client, &admin, &collateral_token, &collateral_token);

    client.deposit(&user, &collateral_token, &5_000);
    client.withdraw(&user, &collateral_token, &2_000);

    let deposit = client.get_user_deposit(&user, &collateral_token);
    assert_eq!(deposit.amount, 3_000);

    let data = client.get_reserve_data(&collateral_token).unwrap();
    assert_eq!(data.total_liquidity, 3_000);
    assert_eq!(data.available_liquidity, 3_000);
}

#[test]
#[should_panic(expected = "Insufficient balance")]
fn test_withdraw_too_much() {
    let (env, collateral_token, collateral_admin, _, _, client, _) = setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &10_000);
    configure_reserves(&client, &admin, &collateral_token, &collateral_token);

    client.deposit(&user, &collateral_token, &5_000);
    client.withdraw(&user, &collateral_token, &10_000);
}

#[test]
fn test_enable_disable_collateral() {
    let (env, collateral_token, collateral_admin, _, _, client, _) = setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &10_000);
    configure_reserves(&client, &admin, &collateral_token, &collateral_token);

    client.deposit(&user, &collateral_token, &5_000);

    assert!(!client.is_asset_collateral(&user, &collateral_token));
    client.enable_collateral(&user, &collateral_token);
    assert!(client.is_asset_collateral(&user, &collateral_token));
    client.disable_collateral(&user, &collateral_token);
    assert!(!client.is_asset_collateral(&user, &collateral_token));
}

#[test]
#[should_panic(expected = "Collateral already enabled")]
fn test_enable_collateral_twice() {
    let (env, collateral_token, collateral_admin, _, _, client, _) = setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &10_000);
    configure_reserves(&client, &admin, &collateral_token, &collateral_token);

    client.deposit(&user, &collateral_token, &5_000);
    client.enable_collateral(&user, &collateral_token);
    client.enable_collateral(&user, &collateral_token);
}

#[test]
fn test_borrow_and_repay() {
    let (env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, _) =
        setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &100_000);
    borrow_admin.mint(&user, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &50_000);
    client.enable_collateral(&user, &collateral_token);

    client.borrow(&user, &borrow_token, &1_000);

    let borrow = client.get_user_borrow(&user, &borrow_token);
    assert_eq!(borrow.amount, 1_000);

    let data = client.get_reserve_data(&borrow_token).unwrap();
    assert_eq!(data.total_borrows, 1_000);
    assert!(data.total_liquidity < 10_000_000);
    assert_eq!(data.available_liquidity, 9_999_000);

    let repaid = client.repay(&user, &user, &borrow_token, &1_000);
    assert_eq!(repaid, 1_000);

    let borrow = client.get_user_borrow(&user, &borrow_token);
    assert_eq!(borrow.amount, 0);
    assert_eq!(borrow.accumulated_interest, 0);
}

#[test]
fn test_borrow_with_interest_accrual() {
    let (mut env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, _) =
        setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &100_000);
    borrow_admin.mint(&user, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &50_000);
    client.enable_collateral(&user, &collateral_token);
    client.borrow(&user, &borrow_token, &1_000);

    env.ledger().set_timestamp(365 * 24 * 60 * 60);

    let repaid = client.repay(&user, &user, &borrow_token, &10_000);
    assert!(repaid > 1_000);

    let borrow = client.get_user_borrow(&user, &borrow_token);
    assert_eq!(borrow.amount, 0);
    assert_eq!(borrow.accumulated_interest, 0);
}

#[test]
fn test_partial_repay() {
    let (env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, _) =
        setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &100_000);
    borrow_admin.mint(&user, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &50_000);
    client.enable_collateral(&user, &collateral_token);
    client.borrow(&user, &borrow_token, &1_000);

    let repaid = client.repay(&user, &user, &borrow_token, &400);
    assert_eq!(repaid, 400);

    let borrow = client.get_user_borrow(&user, &borrow_token);
    assert_eq!(borrow.amount, 600);
}

#[test]
#[should_panic(expected = "Health factor too low")]
fn test_borrow_beyond_health_factor() {
    let (env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, _) =
        setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &10_000);
    borrow_admin.mint(&user, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &10_000);
    client.enable_collateral(&user, &collateral_token);

    collateral_asset_value = 10_000 * 1.0 = 10_000
    weighted = 10_000 * 0.80 = 8_000
    borrow for 10_000 > 8_000 = health factor < 1.0

    client.borrow(&user, &borrow_token, &10_000);
}

#[test]
fn test_full_borrow_under_threshold() {
    let (env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, _) =
        setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &100_000);
    borrow_admin.mint(&user, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &100_000);
    client.enable_collateral(&user, &collateral_token);

    client.borrow(&user, &borrow_token, &79_999);
    let borrow = client.get_user_borrow(&user, &borrow_token);
    assert_eq!(borrow.amount, 79_999);

    let health = client.get_health_factor(&user);
    assert!(health.health_factor_bps > BASIS_POINTS);
}

#[test]
fn test_liquidation() {
    let (mut env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, oracle) =
        setup_lending_env();
    let user = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &10_000);
    borrow_admin.mint(&user, &10_000_000);
    borrow_admin.mint(&liquidator, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &10_000);
    client.enable_collateral(&user, &collateral_token);
    client.borrow(&user, &borrow_token, &7_999);

    oracle.set_price(&Symbol::new(&env, "USD"), &50_000_000);

    let health = client.get_health_factor(&user);
    assert!(health.health_factor_bps < BASIS_POINTS);

    let repaid = client.liquidate(&liquidator, &user, &borrow_token, &4_000);
    assert!(repaid > 0);

    let borrow = client.get_user_borrow(&user, &borrow_token);
    assert!(borrow.amount < 7_999);
}

#[test]
#[should_panic(expected = "Position healthy")]
fn test_liquidate_healthy_position() {
    let (env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, _) =
        setup_lending_env();
    let user = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &100_000);
    borrow_admin.mint(&user, &10_000_000);
    borrow_admin.mint(&liquidator, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &100_000);
    client.enable_collateral(&user, &collateral_token);
    client.borrow(&user, &borrow_token, &10_000);

    client.liquidate(&liquidator, &user, &borrow_token, &1_000);
}

#[test]
fn test_health_factor() {
    let (env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, _) =
        setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &100_000);
    borrow_admin.mint(&user, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    let health = client.get_health_factor(&user);
    assert_eq!(health.health_factor_bps, i128::MAX);
    assert_eq!(health.total_collateral_value, 0);
    assert_eq!(health.total_borrow_value, 0);

    client.deposit(&user, &collateral_token, &100_000);
    client.enable_collateral(&user, &collateral_token);

    let health = client.get_health_factor(&user);
    assert_eq!(health.health_factor_bps, i128::MAX);
    assert!(health.total_collateral_value > 0);
    assert_eq!(health.total_borrow_value, 0);

    client.borrow(&user, &borrow_token, &50_000);

    let health = client.get_health_factor(&user);
    assert!(health.total_borrow_value > 0);
    assert!(health.health_factor_bps > BASIS_POINTS);
}

#[test]
fn test_get_reserve_data() {
    let (env, collateral_token, _, _, _, client, _) = setup_lending_env();
    let admin = Address::generate(&env);

    assert!(client.get_reserve_data(&collateral_token).is_none());

    client.configure_reserve(&admin, &collateral_token, &default_reserve_config());
    let data = client.get_reserve_data(&collateral_token).unwrap();
    assert_eq!(data.total_liquidity, 0);
}

#[test]
fn test_get_reserve_config_none() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let (token, _, _) = setup_token(&env, &admin);

    let contract_id = env.register_contract(None, CollateralLendingContract);
    let client = CollateralLendingContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    assert!(client.get_reserve_config(&token).is_none());
}

#[test]
fn test_get_reserve_list() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let (token_a, _, _) = setup_token(&env, &admin);
    let (token_b, _, _) = setup_token(&env, &admin);

    let contract_id = env.register_contract(None, CollateralLendingContract);
    let client = CollateralLendingContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    assert_eq!(client.get_reserve_list().len(), 0);

    client.configure_reserve(&admin, &token_a, &default_reserve_config());
    client.configure_reserve(&admin, &token_b, &default_reserve_config());

    let list = client.get_reserve_list();
    assert_eq!(list.len(), 2);
}

#[test]
fn test_get_simulation_rate() {
    let (env, collateral_token, _, _, _, client, _) = setup_lending_env();
    let admin = Address::generate(&env);
    client.configure_reserve(&admin, &collateral_token, &default_reserve_config());

    let rate = client.get_simulation_rate(&collateral_token, &100_000, &80_000);
    assert!(rate > 0);
}

#[test]
fn test_oracle_price_change_triggers_liquidation() {
    let (mut env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, oracle) =
        setup_lending_env();
    let user = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &100_000);
    borrow_admin.mint(&user, &10_000_000);
    borrow_admin.mint(&liquidator, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &100_000);
    client.enable_collateral(&user, &collateral_token);
    client.borrow(&user, &borrow_token, &70_000);

    let health = client.get_health_factor(&user);
    assert!(health.health_factor_bps >= BASIS_POINTS);

    oracle.set_price(&Symbol::new(&env, "USD"), &60_000_000);

    let health_after = client.get_health_factor(&user);
    assert!(health_after.health_factor_bps < BASIS_POINTS);

    let repaid = client.liquidate(&liquidator, &user, &borrow_token, &35_000);
    assert!(repaid > 0);
}

#[test]
fn test_withdraw_collateral_reduces_health_factor() {
    let (env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, _) =
        setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &200_000);
    borrow_admin.mint(&user, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &200_000);
    client.enable_collateral(&user, &collateral_token);
    client.borrow(&user, &borrow_token, &50_000);

    let health_before = client.get_health_factor(&user);
    assert!(health_before.health_factor_bps > BASIS_POINTS);

    let deposit = client.get_user_deposit(&user, &collateral_token);
    client.withdraw(&user, &collateral_token, &100_000);

    let health_after = client.get_health_factor(&user);
    assert!(health_after.health_factor_bps < health_before.health_factor_bps);
    assert!(health_after.health_factor_bps > BASIS_POINTS);
}

#[test]
#[should_panic(expected = "Health factor too low")]
fn test_withdraw_makes_position_unhealthy() {
    let (env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, _) =
        setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &100_000);
    borrow_admin.mint(&user, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &100_000);
    client.enable_collateral(&user, &collateral_token);
    client.borrow(&user, &borrow_token, &75_000);

    client.withdraw(&user, &collateral_token, &30_000);
}

#[test]
fn test_deposit_multiple_users() {
    let (env, collateral_token, collateral_admin, _, _, client, _) = setup_lending_env();
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user1, &10_000);
    collateral_admin.mint(&user2, &10_000);
    configure_reserves(&client, &admin, &collateral_token, &collateral_token);

    client.deposit(&user1, &collateral_token, &4_000);
    client.deposit(&user2, &collateral_token, &6_000);

    assert_eq!(
        client.get_user_deposit(&user1, &collateral_token).amount,
        4_000
    );
    assert_eq!(
        client.get_user_deposit(&user2, &collateral_token).amount,
        6_000
    );

    let data = client.get_reserve_data(&collateral_token).unwrap();
    assert_eq!(data.total_liquidity, 10_000);
}

#[test]
fn test_repay_full_debt_with_interest() {
    let (mut env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, _) =
        setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &100_000);
    borrow_admin.mint(&user, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &100_000);
    client.enable_collateral(&user, &collateral_token);
    client.borrow(&user, &borrow_token, &5_000);

    env.ledger().set_timestamp(180 * 24 * 60 * 60);

    let borrow_before = client.get_user_borrow(&user, &borrow_token);
    assert!(borrow_before.accumulated_interest > 0);

    let total_debt = borrow_before.amount + borrow_before.accumulated_interest;
    let repaid = client.repay(&user, &user, &borrow_token, &total_debt);
    assert_eq!(repaid, total_debt);

    let borrow_after = client.get_user_borrow(&user, &borrow_token);
    assert_eq!(borrow_after.amount, 0);
    assert_eq!(borrow_after.accumulated_interest, 0);
}

#[test]
#[should_panic(expected = "Invalid collateral factor")]
fn test_invalid_reserve_config_collateral_factor() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let (token, _, _) = setup_token(&env, &admin);

    let contract_id = env.register_contract(None, CollateralLendingContract);
    let client = CollateralLendingContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let mut config = default_reserve_config();
    config.collateral_factor_bps = 12_000;
    client.configure_reserve(&admin, &token, &config);
}

#[test]
#[should_panic(expected = "Invalid liquidation bonus")]
fn test_invalid_reserve_config_liquidation_bonus() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let (token, _, _) = setup_token(&env, &admin);

    let contract_id = env.register_contract(None, CollateralLendingContract);
    let client = CollateralLendingContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let mut config = default_reserve_config();
    config.liquidation_bonus_bps = 2_000;
    client.configure_reserve(&admin, &token, &config);
}

#[test]
fn test_liquidator_receives_collateral() {
    let (mut env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, oracle) =
        setup_lending_env();
    let user = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &10_000);
    borrow_admin.mint(&user, &10_000_000);
    borrow_admin.mint(&liquidator, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &10_000);
    client.enable_collateral(&user, &collateral_token);
    client.borrow(&user, &borrow_token, &7_000);

    oracle.set_price(&Symbol::new(&env, "USD"), &50_000_000);

    let collateral_before = client.get_user_deposit(&user, &collateral_token).amount;

    let repaid = client.liquidate(&liquidator, &user, &borrow_token, &3_000);
    assert!(repaid > 0);

    let collateral_after = client.get_user_deposit(&user, &collateral_token).amount;
    assert!(collateral_after < collateral_before);
}

#[test]
fn test_disable_all_collateral_when_no_borrow() {
    let (env, collateral_token, collateral_admin, _, _, client, _) = setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &10_000);
    configure_reserves(&client, &admin, &collateral_token, &collateral_token);

    client.deposit(&user, &collateral_token, &5_000);
    client.enable_collateral(&user, &collateral_token);
    assert!(client.is_asset_collateral(&user, &collateral_token));

    client.disable_collateral(&user, &collateral_token);
    assert!(!client.is_asset_collateral(&user, &collateral_token));
}

#[test]
#[should_panic(expected = "Asset not active")]
fn test_deposit_to_inactive_reserve() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let (token, _, _) = setup_token(&env, &admin);

    let contract_id = env.register_contract(None, CollateralLendingContract);
    let client = CollateralLendingContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let mut config = default_reserve_config();
    config.is_active = false;
    client.configure_reserve(&admin, &token, &config);

    let user = Address::generate(&env);
    client.deposit(&user, &token, &1_000);
}

#[test]
#[should_panic(expected = "Reserve is frozen")]
fn test_deposit_to_frozen_reserve() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let (token, _, _) = setup_token(&env, &admin);

    let contract_id = env.register_contract(None, CollateralLendingContract);
    let client = CollateralLendingContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let mut config = default_reserve_config();
    config.is_frozen = true;
    client.configure_reserve(&admin, &token, &config);

    let user = Address::generate(&env);
    client.deposit(&user, &token, &1_000);
}

#[test]
fn test_multiple_borrows_same_user() {
    let (env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, _) =
        setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &200_000);
    borrow_admin.mint(&user, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &200_000);
    client.enable_collateral(&user, &collateral_token);

    client.borrow(&user, &borrow_token, &30_000);
    client.borrow(&user, &borrow_token, &20_000);

    let borrow = client.get_user_borrow(&user, &borrow_token);
    assert_eq!(borrow.amount, 50_000);
}

#[test]
fn test_multiple_deposits_same_user() {
    let (env, collateral_token, collateral_admin, _, _, client, _) = setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &10_000);
    configure_reserves(&client, &admin, &collateral_token, &collateral_token);

    client.deposit(&user, &collateral_token, &3_000);
    client.deposit(&user, &collateral_token, &4_000);

    let deposit = client.get_user_deposit(&user, &collateral_token);
    assert_eq!(deposit.amount, 7_000);
}

#[test]
fn test_reserve_factor_collects_fees() {
    let (mut env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, _) =
        setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &100_000);
    borrow_admin.mint(&user, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &100_000);
    client.enable_collateral(&user, &collateral_token);
    client.borrow(&user, &borrow_token, &50_000);

    let data_before = client.get_reserve_data(&borrow_token).unwrap();
    assert_eq!(data_before.total_borrows, 50_000);

    env.ledger().set_timestamp(365 * 24 * 60 * 60);

    client.repay(&user, &user, &borrow_token, &100_000);

    let data_after = client.get_reserve_data(&borrow_token).unwrap();
    assert!(data_after.total_liquidity > 10_000_000);
}

#[test]
fn test_deposit_into_new_reserve_after_configure() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let (token_a, token_a_admin, _) = setup_token(&env, &admin);
    let (token_b, token_b_admin, _) = setup_token(&env, &admin);

    let oracle_id = env.register_contract(None, MockOracle);
    let oracle_client = MockOracleClient::new(&env, &oracle_id);
    oracle_client.set_price(&Symbol::new(&env, "USD"), &100_000_000);

    let contract_id = env.register_contract(None, CollateralLendingContract);
    let client = CollateralLendingContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    client.set_oracle(&admin, &oracle_id);

    client.configure_reserve(&admin, &token_a, &default_reserve_config());
    client.configure_reserve(&admin, &token_b, &default_reserve_config());

    let user = Address::generate(&env);
    token_a_admin.mint(&user, &5_000);
    token_b_admin.mint(&user, &3_000);

    client.deposit(&user, &token_a, &5_000);
    client.deposit(&user, &token_b, &3_000);

    assert_eq!(
        client.get_user_deposit(&user, &token_a).amount,
        5_000
    );
    assert_eq!(
        client.get_user_deposit(&user, &token_b).amount,
        3_000
    );
}

#[test]
fn test_borrow_repay_borrow_again() {
    let (env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, _) =
        setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &200_000);
    borrow_admin.mint(&user, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &200_000);
    client.enable_collateral(&user, &collateral_token);

    client.borrow(&user, &borrow_token, &50_000);
    client.repay(&user, &user, &borrow_token, &50_000);

    let borrow = client.get_user_borrow(&user, &borrow_token);
    assert_eq!(borrow.amount, 0);

    client.borrow(&user, &borrow_token, &30_000);
    let borrow = client.get_user_borrow(&user, &borrow_token);
    assert_eq!(borrow.amount, 30_000);
}

#[test]
fn test_repay_excess_returns_actual() {
    let (env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, _) =
        setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &200_000);
    borrow_admin.mint(&user, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &200_000);
    client.enable_collateral(&user, &collateral_token);
    client.borrow(&user, &borrow_token, &10_000);

    let repaid = client.repay(&user, &user, &borrow_token, &100_000);
    assert_eq!(repaid, 10_000);
}

#[test]
#[should_panic(expected = "Insufficient liquidity")]
fn test_borrow_more_than_liquidity() {
    let (env, collateral_token, collateral_admin, borrow_token, borrow_admin, client, _) =
        setup_lending_env();
    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    collateral_admin.mint(&user, &100_000);
    borrow_admin.mint(&user, &10_000_000);
    configure_reserves(&client, &admin, &collateral_token, &borrow_token);

    client.deposit(&user, &collateral_token, &100_000);
    client.enable_collateral(&user, &collateral_token);

    client.borrow(&user, &borrow_token, &100_000_000);
}

#[test]
#[should_panic(expected = "Collateral factor must be <= liquidation threshold")]
fn test_validate_collateral_factor_gt_liquidation() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let (token, _, _) = setup_token(&env, &admin);

    let contract_id = env.register_contract(None, CollateralLendingContract);
    let client = CollateralLendingContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let mut config = default_reserve_config();
    config.collateral_factor_bps = 9000;
    config.liquidation_threshold_bps = 8500;
    client.configure_reserve(&admin, &token, &config);
}
