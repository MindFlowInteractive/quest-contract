#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _},
    Address, Env, symbol_short,
};

fn create_test_env() -> (Env, Address, Address, Address) {
    let env = Env::default();
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let other_user = Address::generate(&env);

    env.mock_all_auths();

    (env, admin, creator, other_user)
}

#[test]
fn test_initialize() {
    let (env, admin, _, _) = create_test_env();
    let contract_id = env.register_contract(None, ContentLicenseContract);
    let client = ContentLicenseContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    assert_eq!(client.get_admin(), Some(admin.clone()));

    // Initializing twice should fail
    let res = client.try_initialize(&admin);
    assert_eq!(res, Err(Ok(LicenseError::AlreadyInitialized)));
}

#[test]
fn test_register_initial_terms() {
    let (env, admin, creator, _) = create_test_env();
    let contract_id = env.register_contract(None, ContentLicenseContract);
    let client = ContentLicenseContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    let puzzle_id: u64 = 501;
    let uri = symbol_short!("IPFS_V1");

    let v = client.register_terms(
        &creator,
        &puzzle_id,
        &LicenseType::FreeToPlay,
        &0,
        &uri,
    );
    assert_eq!(v, 1);

    assert_eq!(client.get_creator(&puzzle_id), Some(creator.clone()));
    assert_eq!(client.get_latest_version(&puzzle_id), 1);

    let current = client.get_current_terms(&puzzle_id).unwrap();
    assert_eq!(current.creator, creator);
    assert_eq!(current.version, 1);
    assert_eq!(current.license_type, LicenseType::FreeToPlay);
    assert_eq!(current.fee_amount, 0);
    assert_eq!(current.terms_uri, uri);
}

#[test]
fn test_new_version_registration_and_immutability() {
    let (env, admin, creator, _) = create_test_env();
    let contract_id = env.register_contract(None, ContentLicenseContract);
    let client = ContentLicenseContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    let puzzle_id: u64 = 502;
    let uri1 = symbol_short!("TERMS_1");
    let uri2 = symbol_short!("TERMS_2");

    // Version 1
    let v1 = client.register_terms(
        &creator,
        &puzzle_id,
        &LicenseType::FreeToPlay,
        &0,
        &uri1,
    );
    assert_eq!(v1, 1);

    // Version 2: Switch to PaidPerPlay with 25 tokens fee
    let v2 = client.register_terms(
        &creator,
        &puzzle_id,
        &LicenseType::PaidPerPlay,
        &25,
        &uri2,
    );
    assert_eq!(v2, 2);
    assert_eq!(client.get_latest_version(&puzzle_id), 2);

    // Current terms must reflect v2
    let current = client.get_current_terms(&puzzle_id).unwrap();
    assert_eq!(current.version, 2);
    assert_eq!(current.license_type, LicenseType::PaidPerPlay);
    assert_eq!(current.fee_amount, 25);
    assert_eq!(current.terms_uri, uri2);

    // Version 1 terms must remain immutable and intact
    let terms_v1 = client.get_terms_by_version(&puzzle_id, &1).unwrap();
    assert_eq!(terms_v1.version, 1);
    assert_eq!(terms_v1.license_type, LicenseType::FreeToPlay);
    assert_eq!(terms_v1.fee_amount, 0);
    assert_eq!(terms_v1.terms_uri, uri1);
}

#[test]
fn test_non_creator_cannot_register_or_edit() {
    let (env, admin, creator, other_user) = create_test_env();
    let contract_id = env.register_contract(None, ContentLicenseContract);
    let client = ContentLicenseContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    let puzzle_id: u64 = 503;
    let uri = symbol_short!("ORIGINAL");

    // Creator registers v1
    client.register_terms(
        &creator,
        &puzzle_id,
        &LicenseType::RemixAllowed,
        &0,
        &uri,
    );

    // other_user tries to register new terms for puzzle_id
    let res = client.try_register_terms(
        &other_user,
        &puzzle_id,
        &LicenseType::PaidPerPlay,
        &100,
        &symbol_short!("HIJACK"),
    );
    assert_eq!(res, Err(Ok(LicenseError::NotAuthorized)));

    // Latest version is still 1 and creator unchanged
    assert_eq!(client.get_latest_version(&puzzle_id), 1);
    assert_eq!(client.get_creator(&puzzle_id), Some(creator));
}

#[test]
fn test_non_existent_puzzle() {
    let (env, admin, _, _) = create_test_env();
    let contract_id = env.register_contract(None, ContentLicenseContract);
    let client = ContentLicenseContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    assert_eq!(client.get_current_terms(&999), None);
    assert_eq!(client.get_terms_by_version(&999, &1), None);
    assert_eq!(client.get_creator(&999), None);
    assert_eq!(client.get_latest_version(&999), 0);
}

#[test]
fn test_transfer_admin() {
    let (env, admin, _, other_user) = create_test_env();
    let contract_id = env.register_contract(None, ContentLicenseContract);
    let client = ContentLicenseContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    client.transfer_admin(&admin, &other_user);

    assert_eq!(client.get_admin(), Some(other_user.clone()));

    // Old admin cannot transfer
    let old_res = client.try_transfer_admin(&admin, &other_user);
    assert_eq!(old_res, Err(Ok(LicenseError::NotAuthorized)));
}
