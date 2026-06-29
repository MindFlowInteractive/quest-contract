#![cfg(test)]

use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::{Address, Env};

use crate::{
    ClaimStatus, FailureType, PolicyStatus, RiskLevel,
    InsurancePoolContract, InsurancePoolContractClient,
};

fn create_token_contract<'a>(env: &Env, admin: &Address) -> (Address, TokenClient<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let address = sac.address();
    (address.clone(), TokenClient::new(env, &address))
}

fn setup_insurance_pool_contract(env: &Env) -> (
    InsurancePoolContractClient,
    Address,
    Address,
    TokenClient,
    StellarAssetClient,
) {
    let admin = Address::generate(env);
    let token_admin = Address::generate(env);

    let (token_addr, token_client) = create_token_contract(env, &token_admin);
    let token_admin_client = StellarAssetClient::new(env, &token_addr);

    let contract_id = env.register_contract(None, InsurancePoolContract);
    let client = InsurancePoolContractClient::new(env, &contract_id);

    client.initialize(&admin, &token_addr, &100);

    (client, admin, token_admin, token_client, token_admin_client)
}

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, _token_admin, token_client, token_admin_client) = setup_insurance_pool_contract(&env);

    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_config().payment_token, token_client.address);
    assert_eq!(client.get_config().base_premium_rate, 100);
    assert_eq!(client.get_config().paused, false);
    assert_eq!(client.get_premium_pool(), 0);
    assert_eq!(client.get_reserve_pool(), 0);
    assert_eq!(client.get_total_policies(), 0);
    assert_eq!(client.get_total_claims(), 0);
}

#[test]
fn test_purchase_coverage() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, _token_admin, token_client, token_admin_client) = setup_insurance_pool_contract(&env);

    let payer = Address::generate(&env);
    let contract_address = Address::generate(&env);
    let coverage_amount = 1_000_000i128;
    let coverage_period = 30 * 24 * 60 * 64u64; // 30 days

    token_admin_client.mint(&admin, &10_000_000);
    token_client.transfer(&admin, &payer, &10_000_000);

    client.purchase_coverage(
        contract_address.clone(),
        coverage_amount,
        coverage_period,
        payer.clone(),
    );

    let policy = client.get_policy(&contract_address).unwrap();
    assert_eq!(policy.contract_address, contract_address);
    assert_eq!(policy.coverage_amount, coverage_amount);
    assert_eq!(policy.status, PolicyStatus::Active);
    assert!(policy.premium_paid > 0);
    assert!(client.get_premium_pool() > 0);
    assert!(client.get_reserve_pool() > 0);
    assert_eq!(client.get_total_policies(), 1);
    assert!(client.is_coverage_active(&contract_address));
}
