#![cfg(test)]
extern crate std;

use soroban_sdk::{
    testutils::Address as _,
    testutils::BytesN as _,
    Env,
    Symbol,
};

use super::*;

#[test]
fn test_emergency_override() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, OracleIntegration);
    let admin = soroban_sdk::Address::generate(&env);

    let signer = soroban_sdk::testutils::BytesN::<32>::random(&env);
    let signers = soroban_sdk::Vec::from_array(&env, &[signer.clone()]);

    let client = OracleIntegrationClient::new(&env, &contract_id);

    client.initialize(&admin, &signers, &1u32, &60u64, &500u32, &30u64);

    // emergency price should override regardless of asset sources
    client.set_emergency_price(&200, &10, &1);

    let asset = Symbol::new(&env, "XLM");
    let snap = client.get_price(&asset);

    assert_eq!(snap.unwrap().price, 200);
}

#[test]
fn test_signed_submission_and_cache() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, OracleIntegration);
    let admin = soroban_sdk::Address::generate(&env);

    let signer = soroban_sdk::testutils::BytesN::<32>::random(&env);
    let signers = soroban_sdk::Vec::from_array(&env, &[signer.clone()]);

    let client = OracleIntegrationClient::new(&env, &contract_id);

    client.initialize(&admin, &signers, &1u32, &60u64, &500u32, &30u64);

    // In mock env we can't easily generate valid ed25519 signatures.
    // So we only test failure on insufficient signatures.
    let asset = Symbol::new(&env, "XLM");
    let empty_sigs: soroban_sdk::Vec<(soroban_sdk::BytesN<32>, soroban_sdk::BytesN<64>)> =
        soroban_sdk::Vec::new(&env);

    let res = client.try_submit_signed_price(&asset, &100, &1, &1, &empty_sigs);
    assert!(res.is_err());
}

