#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Vec, BytesN, symbol_short,
};

fn create_test_env() -> (Env, Address, Address, Address) {
    let env = Env::default();
    let admin = Address::generate(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    
    env.mock_all_auths();
    
    (env, admin, user1, user2)
}

#[test]
fn test_initialize() {
    let (env, admin, _, _) = create_test_env();
    let contract_id = env.register_contract(None, WhitelistContract);
    let client = WhitelistContractClient::new(&env, &contract_id);
    
    client.initialize(&admin);
    
    assert_eq!(client.get_admin(), Some(admin));
}

#[test]
fn test_add_to_whitelist() {
    let (env, admin, user1, _) = create_test_env();
    let contract_id = env.register_contract(None, WhitelistContract);
    let client = WhitelistContractClient::new(&env, &contract_id);
    
    client.initialize(&admin);
    
    let permissions = Vec::from_array(&env, [symbol_short!("PUZZLE"), symbol_short!("EVENT")]);
    
    client.add_to_whitelist(&admin, &user1, &2, &None, &permissions);
    
    assert!(client.is_whitelisted(&user1, &Some(1)));
    assert!(client.is_whitelisted(&user1, &Some(2)));
    assert!(!client.is_whitelisted(&user1, &Some(3)));
    
    let entry = client.get_whitelist_entry(&user1).unwrap();
    assert_eq!(entry.tier, 2);
    assert_eq!(entry.expiration, None);
    assert_eq!(entry.permissions, permissions);
}

#[test]
fn test_tier_permissions() {
    let (env, admin, user1, _) = create_test_env();
    let contract_id = env.register_contract(None, WhitelistContract);
    let client = WhitelistContractClient::new(&env, &contract_id);
    
    client.initialize(&admin);
    
    let permissions = Vec::from_array(&env, [symbol_short!("PUZZLE")]);
    client.add_to_whitelist(&admin, &user1, &1, &None, &permissions);
    
    assert!(client.has_permission(&user1, &symbol_short!("PUZZLE")));
    assert!(!client.has_permission(&user1, &symbol_short!("EVENT")));
}

#[test]
fn test_expiration() {
    let (env, admin, user1, _) = create_test_env();
    let contract_id = env.register_contract(None, WhitelistContract);
    let client = WhitelistContractClient::new(&env, &contract_id);
    
    client.initialize(&admin);
    
    // Set expiration to block 100
    let expiration = Some(100u32);
    let permissions = Vec::from_array(&env, [symbol_short!("PUZZLE")]);
    
    client.add_to_whitelist(&admin, &user1, &1, &expiration, &permissions);
    
    // Should be whitelisted at block 50
    env.ledger().with_mut(|li| {
        li.sequence_number = 50;
    });
    assert!(client.is_whitelisted(&user1, &Some(1)));
    
    // Should not be whitelisted at block 101
    env.ledger().with_mut(|li| {
        li.sequence_number = 101;
    });
    assert!(!client.is_whitelisted(&user1, &Some(1)));
}

#[test]
fn test_batch_operations() {
    let (env, admin, user1, user2) = create_test_env();
    let contract_id = env.register_contract(None, WhitelistContract);
    let client = WhitelistContractClient::new(&env, &contract_id);
    
    client.initialize(&admin);
    
    let permissions = Vec::from_array(&env, [symbol_short!("PUZZLE")]);
    
    let entries = Vec::from_array(&env, [
        WhitelistEntry {
            address: user1.clone(),
            tier: 1,
            expiration: None,
            permissions: permissions.clone(),
        },
        WhitelistEntry {
            address: user2.clone(),
            tier: 2,
            expiration: None,
            permissions: permissions.clone(),
        },
    ]);
    
    client.batch_add_to_whitelist(&admin, &entries);
    
    assert!(client.is_whitelisted(&user1, &Some(1)));
    assert!(client.is_whitelisted(&user2, &Some(2)));
    
    // Test batch removal
    let addresses = Vec::from_array(&env, [user1.clone(), user2.clone()]);
    client.batch_remove_from_whitelist(&admin, &addresses);
    
    assert!(!client.is_whitelisted(&user1, &Some(1)));
    assert!(!client.is_whitelisted(&user2, &Some(1)));
}

#[test]
fn test_remove_from_whitelist() {
    let (env, admin, user1, _) = create_test_env();
    let contract_id = env.register_contract(None, WhitelistContract);
    let client = WhitelistContractClient::new(&env, &contract_id);
    
    client.initialize(&admin);
    
    let permissions = Vec::from_array(&env, [symbol_short!("PUZZLE")]);
    client.add_to_whitelist(&admin, &user1, &1, &None, &permissions);
    
    assert!(client.is_whitelisted(&user1, &Some(1)));
    
    client.remove_from_whitelist(&admin, &user1);
    
    assert!(!client.is_whitelisted(&user1, &Some(1)));
    assert_eq!(client.get_whitelist_entry(&user1), None);
}

#[test]
fn test_tier_based_permissions() {
    let (env, admin, _user1, _) = create_test_env();
    let contract_id = env.register_contract(None, WhitelistContract);
    let client = WhitelistContractClient::new(&env, &contract_id);
    
    client.initialize(&admin);
    
    let tier1_perms = Vec::from_array(&env, [symbol_short!("BASIC")]);
    let tier2_perms = Vec::from_array(&env, [symbol_short!("BASIC"), symbol_short!("PREMIUM")]);
    
    client.set_tier_permissions(&admin, &1, &tier1_perms);
    client.set_tier_permissions(&admin, &2, &tier2_perms);
    
    assert_eq!(client.get_tier_permissions(&1), tier1_perms);
    assert_eq!(client.get_tier_permissions(&2), tier2_perms);
}

#[test]
fn test_merkle_operations() {
    let (env, admin, _user1, _) = create_test_env();
    let contract_id = env.register_contract(None, WhitelistContract);
    let client = WhitelistContractClient::new(&env, &contract_id);
    
    client.initialize(&admin);
    
    let merkle_root = BytesN::from_array(&env, &[1u8; 32]);
    client.set_merkle_root(&admin, &merkle_root);
    
    // Create snapshot
    client.create_snapshot(&admin, &merkle_root, &100);
    
    let snapshot = client.get_snapshot().unwrap();
    assert_eq!(snapshot.merkle_root, merkle_root);
    assert_eq!(snapshot.total_entries, 100);
}

#[test]
fn test_admin_controls() {
    let (env, admin, user1, user2) = create_test_env();
    let contract_id = env.register_contract(None, WhitelistContract);
    let client = WhitelistContractClient::new(&env, &contract_id);
    
    client.initialize(&admin);
    
    // Test admin transfer
    client.transfer_admin(&admin, &user1);
    assert_eq!(client.get_admin(), Some(user1.clone()));
    
    // Old admin should not be able to add to whitelist
    let permissions = Vec::from_array(&env, [symbol_short!("PUZZLE")]);
    let result = client.try_add_to_whitelist(&admin, &user2, &1, &None, &permissions);
    assert!(result.is_err());
    
    // New admin should be able to add to whitelist
    client.add_to_whitelist(&user1, &user2, &1, &None, &permissions);
    assert!(client.is_whitelisted(&user2, &Some(1)));
}

#[test]
fn test_invalid_tier() {
    let (env, admin, user1, _) = create_test_env();
    let contract_id = env.register_contract(None, WhitelistContract);
    let client = WhitelistContractClient::new(&env, &contract_id);
    
    client.initialize(&admin);
    
    let permissions = Vec::from_array(&env, [symbol_short!("PUZZLE")]);
    let result = client.try_add_to_whitelist(&admin, &user1, &0, &None, &permissions);
    assert!(result.is_err()); // Should fail with tier 0
}

#[test]
fn test_unauthorized_access() {
    let (env, admin, user1, user2) = create_test_env();
    let contract_id = env.register_contract(None, WhitelistContract);
    let client = WhitelistContractClient::new(&env, &contract_id);
    
    client.initialize(&admin);
    
    let permissions = Vec::from_array(&env, [symbol_short!("PUZZLE")]);
    let result = client.try_add_to_whitelist(&user1, &user2, &1, &None, &permissions);
    assert!(result.is_err()); // user1 is not admin, should fail
}

#[test]
fn test_merkle_proof_verification() {
    let (env, admin, user1, _) = create_test_env();
    let contract_id = env.register_contract(None, WhitelistContract);
    let client = WhitelistContractClient::new(&env, &contract_id);
    
    client.initialize(&admin);
    
    // Set a merkle root
    let merkle_root = BytesN::from_array(&env, &[1u8; 32]);
    client.set_merkle_root(&admin, &merkle_root);
    
    // Create a simple proof (in practice, this would be generated off-chain)
    let proof = Vec::from_array(&env, [BytesN::from_array(&env, &[2u8; 32])]);
    
    // This will likely return false since we're using a dummy proof, but tests the function
    let result = client.verify_merkle_proof(&user1, &1, &proof);
    // The function returns Result<bool, Error>, but the client wrapper returns bool
    // Just verify the function executes without panicking
    let _ = result;
}