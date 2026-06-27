#![cfg(test)]

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{token, Address, Env};

fn setup_test() -> (Env, TimeLockVaultClient, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, TimeLockVault);
    let client = TimeLockVaultClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    let token_admin = Address::generate(&env);
    let token_contract = env.register_stellar_asset_contract_v2(token_admin);
    let token_id = token_contract.address();

    let beneficiary = Address::generate(&env);

    (env, client, admin, token_id, beneficiary)
}

#[test]
fn test_deposit_and_withdraw() {
    let (env, client, _admin, token_id, beneficiary) = setup_test();

    let depositor = Address::generate(&env);
    let token_client = token::TokenClient::new(&env, &token_id);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
    
    token_admin_client.mint(&depositor, &1000);

    let current_time = env.ledger().timestamp();
    let unlock_time = current_time + 100;

    client.deposit(&depositor, &token_id, &1000, &beneficiary, &unlock_time);

    let status = client.query_status(&beneficiary, &token_id);
    assert_eq!(status.locked_amount, 1000);
    assert_eq!(status.unlock_time, unlock_time);
    assert_eq!(status.is_condition_met, false);

    env.ledger().with_mut(|li| {
        li.timestamp = unlock_time + 1;
    });

    client.withdraw(&beneficiary, &token_id);

    assert_eq!(token_client.balance(&beneficiary), 1000);

    let status_after = client.query_status(&beneficiary, &token_id);
    assert_eq!(status_after.locked_amount, 0);
}

#[test]
#[should_panic(expected = "funds are locked")]
fn test_withdraw_too_early() {
    let (env, client, _admin, token_id, beneficiary) = setup_test();

    let depositor = Address::generate(&env);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
    token_admin_client.mint(&depositor, &1000);

    let current_time = env.ledger().timestamp();
    let unlock_time = current_time + 100;

    client.deposit(&depositor, &token_id, &1000, &beneficiary, &unlock_time);

    client.withdraw(&beneficiary, &token_id);
}

#[test]
fn test_emergency_unlock() {
    let (env, client, _admin, token_id, beneficiary) = setup_test();

    let depositor = Address::generate(&env);
    let token_client = token::TokenClient::new(&env, &token_id);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
    token_admin_client.mint(&depositor, &1000);

    let current_time = env.ledger().timestamp();
    let unlock_time = current_time + 100;

    client.deposit(&depositor, &token_id, &1000, &beneficiary, &unlock_time);

    client.emergency_unlock(&beneficiary, &token_id);

    client.withdraw(&beneficiary, &token_id);
    assert_eq!(token_client.balance(&beneficiary), 1000);
}

#[test]
fn test_set_condition() {
    let (env, client, _admin, token_id, beneficiary) = setup_test();

    let depositor = Address::generate(&env);
    let token_client = token::TokenClient::new(&env, &token_id);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
    token_admin_client.mint(&depositor, &1000);

    let current_time = env.ledger().timestamp();
    let unlock_time = current_time + 100;

    client.deposit(&depositor, &token_id, &1000, &beneficiary, &unlock_time);

    client.set_condition(&beneficiary, &token_id, &true);

    client.withdraw(&beneficiary, &token_id);
    assert_eq!(token_client.balance(&beneficiary), 1000);
}
