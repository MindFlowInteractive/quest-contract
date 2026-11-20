#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _},
    Address, Env,
};

fn create_test_env() -> (Env, Address, Address, Address) {
    let env = Env::default();
    let admin = Address::generate(&env);
    let player1 = Address::generate(&env);
    let player2 = Address::generate(&env);

    env.mock_all_auths();

    (env, admin, player1, player2)
}

#[test]
fn test_initialize() {
    let (env, admin, _, _) = create_test_env();
    let contract_id = env.register_contract(None, ItemDurabilityContract);
    let client = ItemDurabilityContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    assert_eq!(client.get_admin(), Some(admin.clone()));

    // Initializing twice should fail
    let res = client.try_initialize(&admin);
    assert_eq!(res, Err(Ok(DurabilityError::AlreadyInitialized)));
}

#[test]
fn test_register_and_get_item() {
    let (env, admin, player1, _) = create_test_env();
    let contract_id = env.register_contract(None, ItemDurabilityContract);
    let client = ItemDurabilityContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    let item_id: u64 = 101;
    client.register_item(&admin, &item_id, &player1, &5);

    let item = client.get_item(&item_id).unwrap();
    assert_eq!(item.owner, player1);
    assert_eq!(item.max_durability, 5);
    assert_eq!(item.current_durability, 5);

    assert_eq!(client.get_durability(&item_id), 5);
}

#[test]
fn test_use_item_decreases_by_one() {
    let (env, admin, player1, _) = create_test_env();
    let contract_id = env.register_contract(None, ItemDurabilityContract);
    let client = ItemDurabilityContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    let item_id: u64 = 102;
    client.register_item(&admin, &item_id, &player1, &3);

    // First use: 3 -> 2
    let d1 = client.use_item(&player1, &item_id);
    assert_eq!(d1, 2);
    assert_eq!(client.get_durability(&item_id), 2);

    // Second use: 2 -> 1
    let d2 = client.use_item(&player1, &item_id);
    assert_eq!(d2, 1);
    assert_eq!(client.get_durability(&item_id), 1);

    // Third use: 1 -> 0
    let d3 = client.use_item(&player1, &item_id);
    assert_eq!(d3, 0);
    assert_eq!(client.get_durability(&item_id), 0);
}

#[test]
fn test_zero_durability_cannot_be_used() {
    let (env, admin, player1, _) = create_test_env();
    let contract_id = env.register_contract(None, ItemDurabilityContract);
    let client = ItemDurabilityContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    let item_id: u64 = 103;
    client.register_item(&admin, &item_id, &player1, &1);

    // Use once to reach 0
    let d = client.use_item(&player1, &item_id);
    assert_eq!(d, 0);

    // Using again should fail with ZeroDurability
    let res = client.try_use_item(&player1, &item_id);
    assert_eq!(res, Err(Ok(DurabilityError::ZeroDurability)));
}

#[test]
fn test_unauthorized_use_rejected() {
    let (env, admin, player1, player2) = create_test_env();
    let contract_id = env.register_contract(None, ItemDurabilityContract);
    let client = ItemDurabilityContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    let item_id: u64 = 104;
    client.register_item(&admin, &item_id, &player1, &5);

    // player2 is not the owner
    let res = client.try_use_item(&player2, &item_id);
    assert_eq!(res, Err(Ok(DurabilityError::NotAuthorized)));

    // Durability unchanged
    assert_eq!(client.get_durability(&item_id), 5);
}

#[test]
fn test_item_not_found() {
    let (env, admin, player1, _) = create_test_env();
    let contract_id = env.register_contract(None, ItemDurabilityContract);
    let client = ItemDurabilityContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    let res = client.try_use_item(&player1, &999);
    assert_eq!(res, Err(Ok(DurabilityError::ItemNotFound)));

    assert_eq!(client.get_item(&999), None);
}

#[test]
fn test_invalid_max_durability() {
    let (env, admin, player1, _) = create_test_env();
    let contract_id = env.register_contract(None, ItemDurabilityContract);
    let client = ItemDurabilityContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    let res = client.try_register_item(&admin, &105, &player1, &0);
    assert_eq!(res, Err(Ok(DurabilityError::InvalidMaxDurability)));
}

#[test]
fn test_transfer_admin() {
    let (env, admin, player1, new_admin) = create_test_env();
    let contract_id = env.register_contract(None, ItemDurabilityContract);
    let client = ItemDurabilityContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    client.transfer_admin(&admin, &new_admin);

    assert_eq!(client.get_admin(), Some(new_admin.clone()));

    // Old admin cannot register
    let old_res = client.try_register_item(&admin, &106, &player1, &5);
    assert_eq!(old_res, Err(Ok(DurabilityError::NotAuthorized)));

    // New admin can register
    let new_res = client.try_register_item(&new_admin, &106, &player1, &5);
    assert!(new_res.is_ok());
}
