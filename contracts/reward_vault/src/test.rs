#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::Client as TokenClient,
    token::StellarAssetClient,
    vec, Address, Env,
};

const WEEK: u64 = 7 * 24 * 60 * 60;
const MONTH: u64 = 30 * 24 * 60 * 60;
const QUARTER: u64 = 90 * 24 * 60 * 60;

fn create_token_contract<'a>(env: &Env, admin: &Address) -> (Address, TokenClient<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let address = sac.address();
    (address.clone(), TokenClient::new(env, &address))
}

fn setup_vault_contract(
    env: &Env,
) -> (
    RewardVaultContractClient<'_>,
    Address,
    Address,
    Address,
    Address,
    TokenClient<'_>,
    StellarAssetClient<'_>,
) {
    let admin = Address::generate(env);
    let user = Address::generate(env);
    let beneficiary = Address::generate(env);
    let relayer = Address::generate(env);
    let token_admin = Address::generate(env);

    let (token_addr, token_client) = create_token_contract(env, &token_admin);
    let token_admin_client = StellarAssetClient::new(env, &token_addr);

    let contract_id = env.register_contract(None, RewardVaultContract);
    let client = RewardVaultContractClient::new(env, &contract_id);

    client.initialize(
        &admin,
        &token_addr,
        &1000u32, // 10% early penalty
        &2500u32, // 25% emergency penalty
        &vec![env, WEEK, MONTH, QUARTER],
        &vec![env, 500u32, 1200u32, 2500u32], // 5%, 12%, 25%
    );

    (
        client,
        admin,
        user,
        beneficiary,
        relayer,
        token_client,
        token_admin_client,
    )
}

#[test]
fn test_deposit_locks_for_selected_period() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);

    let (client, _, user, _, _, token_client, token_admin_client) = setup_vault_contract(&env);

    token_admin_client.mint(&user, &10_000_000);
    client.deposit(&user, &5_000_000, &MONTH);

    let vault = client.get_vault(&user).unwrap();
    assert_eq!(vault.amount, 5_000_000);
    assert_eq!(vault.lock_period, MONTH);
    assert_eq!(vault.bonus_bps, 1200);
    assert!(client.get_time_until_maturity(&user) > 0);
    assert_eq!(token_client.balance(&user), 5_000_000);
}

#[test]
fn test_bonus_increases_with_longer_locks() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _, _, _, _, _, _) = setup_vault_contract(&env);
    let amount = 10_000_000i128;

    let week_bonus = client.quote_bonus_for_lock(&WEEK, &amount);
    let month_bonus = client.quote_bonus_for_lock(&MONTH, &amount);
    let quarter_bonus = client.quote_bonus_for_lock(&QUARTER, &amount);

    assert!(week_bonus < month_bonus);
    assert!(month_bonus < quarter_bonus);
}

#[test]
fn test_early_withdrawal_penalized() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);

    let (client, _, user, _, _, token_client, token_admin_client) = setup_vault_contract(&env);

    token_admin_client.mint(&user, &10_000_000);
    client.deposit(&user, &10_000_000, &MONTH);

    env.ledger().set_timestamp(2_000); // Still before maturity
    let payout = client.early_withdraw(&user);

    // 10% penalty => 9_000_000 returned
    assert_eq!(payout, 9_000_000);
    assert_eq!(token_client.balance(&user), 9_000_000);
    assert!(client.get_vault(&user).is_none());
}

#[test]
fn test_maturity_triggers_full_payout() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);

    let (client, admin, user, _, _, token_client, token_admin_client) = setup_vault_contract(&env);

    token_admin_client.mint(&user, &20_000_000);
    token_admin_client.mint(&admin, &20_000_000);
    client.fund_bonus_pool(&admin, &20_000_000);

    client.deposit(&user, &10_000_000, &MONTH);
    env.ledger().set_timestamp(1_000 + MONTH + 1);

    let payout = client.withdraw_mature(&user);
    // 12% bonus on 10_000_000 = 1_200_000; total 11_200_000
    assert_eq!(payout, 11_200_000);
    assert_eq!(token_client.balance(&user), 21_200_000);
    assert!(client.get_vault(&user).is_none());
}

#[test]
fn test_extensions_work_correctly() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);

    let (client, _, user, _, _, _, token_admin_client) = setup_vault_contract(&env);

    token_admin_client.mint(&user, &10_000_000);
    client.deposit(&user, &10_000_000, &WEEK);

    let before = client.get_vault(&user).unwrap();
    assert_eq!(before.bonus_bps, 500);

    // Extend from 1 week to 1 month total.
    client.extend_lock(&user, &(MONTH - WEEK));
    let after = client.get_vault(&user).unwrap();
    assert_eq!(after.lock_period, MONTH);
    assert_eq!(after.bonus_bps, 1200);
    assert!(after.maturity_at > before.maturity_at);
}

#[test]
fn test_emergency_unlock_path() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);

    let (client, admin, user, _, _, token_client, token_admin_client) = setup_vault_contract(&env);

    token_admin_client.mint(&user, &8_000_000);
    client.deposit(&user, &8_000_000, &MONTH);

    client.set_emergency_unlock(&admin, &true);
    let payout = client.emergency_withdraw(&user);

    // 25% emergency penalty => 6_000_000
    assert_eq!(payout, 6_000_000);
    assert_eq!(token_client.balance(&user), 6_000_000);
    assert!(client.get_vault(&user).is_none());
}

#[test]
fn test_inheritance_claim_for_beneficiary() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);

    let (client, admin, user, beneficiary, _, token_client, token_admin_client) =
        setup_vault_contract(&env);

    token_admin_client.mint(&user, &10_000_000);
    token_admin_client.mint(&admin, &20_000_000);
    client.fund_bonus_pool(&admin, &20_000_000);

    client.deposit(&user, &10_000_000, &WEEK);
    client.set_beneficiary(&user, &beneficiary);

    env.ledger().set_timestamp(1_000 + WEEK + 1);
    let payout = client.claim_inheritance(&beneficiary, &user);

    // 5% bonus on 10_000_000 = 500_000
    assert_eq!(payout, 10_500_000);
    assert_eq!(token_client.balance(&beneficiary), 10_500_000);
    assert!(client.get_vault(&user).is_none());
}

#[test]
fn test_auto_distribution_via_relayer() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);

    let (client, admin, user, _, relayer, token_client, token_admin_client) =
        setup_vault_contract(&env);

    token_admin_client.mint(&user, &10_000_000);
    token_admin_client.mint(&admin, &20_000_000);
    token_admin_client.mint(&relayer, &1); // keep relayer in test setup for realism
    client.fund_bonus_pool(&admin, &20_000_000);

    client.deposit(&user, &10_000_000, &WEEK);
    env.ledger().set_timestamp(1_000 + WEEK + 1);

    let payout = client.distribute_mature_payout(&user);
    assert_eq!(payout, 10_500_000);
    assert_eq!(token_client.balance(&user), 10_500_000);
}
