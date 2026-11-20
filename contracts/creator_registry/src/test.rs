#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Env, String};

#[test]
fn test_init_and_admin() {
    let env = Env::default();
    let contract_id = env.register_contract(None, CreatorRegistryContract);
    let client = CreatorRegistryContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.init(&admin);

    assert_eq!(client.get_admin(), admin);

    // Double init must fail
    let res = client.try_init(&admin);
    assert!(res.is_err());
}

#[test]
fn test_register_and_lookup() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, CreatorRegistryContract);
    let client = CreatorRegistryContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    client.init(&admin);

    assert_eq!(client.is_registered(&creator), false);
    assert_eq!(client.is_verified(&creator), false);
    assert_eq!(client.get_total_creators(), 0);

    let uri = String::from_str(&env, "ipfs://bafybeicreatorprofile1");
    client.register_creator(&creator, &uri);

    assert_eq!(client.is_registered(&creator), true);
    assert_eq!(client.is_verified(&creator), false);
    assert_eq!(client.get_total_creators(), 1);

    let profile = client.get_creator(&creator).unwrap();
    assert_eq!(profile.creator, creator);
    assert_eq!(profile.profile_uri, uri);
    assert_eq!(profile.is_verified, false);
    assert_eq!(profile.verified_at, 0);

    // Duplicate registration must fail
    let res = client.try_register_creator(&creator, &uri);
    assert!(res.is_err());
}

#[test]
fn test_update_profile() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, CreatorRegistryContract);
    let client = CreatorRegistryContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    client.init(&admin);

    let uri1 = String::from_str(&env, "ipfs://profile1");
    client.register_creator(&creator, &uri1);

    let uri2 = String::from_str(&env, "ipfs://profile2");
    client.update_profile(&creator, &uri2);

    let profile = client.get_creator(&creator).unwrap();
    assert_eq!(profile.profile_uri, uri2);
}

#[test]
fn test_admin_verification() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, CreatorRegistryContract);
    let client = CreatorRegistryContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    client.init(&admin);

    let uri = String::from_str(&env, "ipfs://creator");
    client.register_creator(&creator, &uri);

    assert_eq!(client.is_verified(&creator), false);

    // Admin grants verification
    client.set_verified(&creator, &true);
    assert_eq!(client.is_verified(&creator), true);
    let profile = client.get_creator(&creator).unwrap();
    assert_eq!(profile.is_verified, true);

    // Admin revokes verification
    client.set_verified(&creator, &false);
    assert_eq!(client.is_verified(&creator), false);
    let revoked = client.get_creator(&creator).unwrap();
    assert_eq!(revoked.is_verified, false);
    assert_eq!(revoked.verified_at, 0);
}
