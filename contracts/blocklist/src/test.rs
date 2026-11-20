#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Vec, symbol_short,
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
    let contract_id = env.register_contract(None, BlocklistContract);
    let client = BlocklistContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    assert_eq!(client.get_admin(), Some(admin.clone()));

    // Initializing twice should fail
    let res = client.try_initialize(&admin);
    assert_eq!(res, Err(Ok(BlocklistError::AlreadyInitialized)));
}

#[test]
fn test_block_address_and_is_blocked() {
    let (env, admin, user1, user2) = create_test_env();
    let contract_id = env.register_contract(None, BlocklistContract);
    let client = BlocklistContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    assert!(!client.is_blocked(&user1));
    assert!(!client.is_blocked(&user2));

    let reason = symbol_short!("CHEATING");
    client.block_address(&admin, &user1, &reason);

    assert!(client.is_blocked(&user1));
    assert!(!client.is_blocked(&user2));

    let entry = client.get_block_entry(&user1).unwrap();
    assert_eq!(entry.address, user1);
    assert_eq!(entry.reason, reason);
}

#[test]
fn test_unblock_address() {
    let (env, admin, user1, _) = create_test_env();
    let contract_id = env.register_contract(None, BlocklistContract);
    let client = BlocklistContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    let reason = symbol_short!("SANCTION");
    client.block_address(&admin, &user1, &reason);
    assert!(client.is_blocked(&user1));

    client.unblock_address(&admin, &user1);
    assert!(!client.is_blocked(&user1));
    assert_eq!(client.get_block_entry(&user1), None);
}

#[test]
fn test_already_blocked_and_not_blocked_errors() {
    let (env, admin, user1, _) = create_test_env();
    let contract_id = env.register_contract(None, BlocklistContract);
    let client = BlocklistContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    let reason = symbol_short!("BOT");
    client.block_address(&admin, &user1, &reason);

    // Blocking again should return AlreadyBlocked
    let err_block = client.try_block_address(&admin, &user1, &reason);
    assert_eq!(err_block, Err(Ok(BlocklistError::AlreadyBlocked)));

    client.unblock_address(&admin, &user1);

    // Unblocking an already unblocked address should return NotBlocked
    let err_unblock = client.try_unblock_address(&admin, &user1);
    assert_eq!(err_unblock, Err(Ok(BlocklistError::NotBlocked)));
}

#[test]
fn test_batch_block_and_unblock() {
    let (env, admin, user1, user2) = create_test_env();
    let contract_id = env.register_contract(None, BlocklistContract);
    let client = BlocklistContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    let mut addrs = Vec::new(&env);
    addrs.push_back(user1.clone());
    addrs.push_back(user2.clone());

    let reason = symbol_short!("EXPLOIT");
    client.batch_block(&admin, &addrs, &reason);

    assert!(client.is_blocked(&user1));
    assert!(client.is_blocked(&user2));

    client.batch_unblock(&admin, &addrs);

    assert!(!client.is_blocked(&user1));
    assert!(!client.is_blocked(&user2));
}

#[test]
fn test_transfer_admin() {
    let (env, admin, user1, new_admin) = create_test_env();
    let contract_id = env.register_contract(None, BlocklistContract);
    let client = BlocklistContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    client.transfer_admin(&admin, &new_admin);

    assert_eq!(client.get_admin(), Some(new_admin.clone()));

    // Old admin cannot block
    let reason = symbol_short!("SUSPICIOUS");
    let old_res = client.try_block_address(&admin, &user1, &reason);
    assert_eq!(old_res, Err(Ok(BlocklistError::NotAuthorized)));

    // New admin can block
    let new_res = client.try_block_address(&new_admin, &user1, &reason);
    assert!(new_res.is_ok());
    assert!(client.is_blocked(&user1));
}
