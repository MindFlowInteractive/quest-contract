#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Vec, symbol_short,
};

fn create_test_env() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    let admin = Address::generate(&env);
    let game_contract = Address::generate(&env);
    let player1 = Address::generate(&env);
    let player2 = Address::generate(&env);

    env.mock_all_auths();

    (env, admin, game_contract, player1, player2)
}

#[test]
fn test_initialize() {
    let (env, admin, _, _, _) = create_test_env();
    let contract_id = env.register_contract(None, PointsLedgerContract);
    let client = PointsLedgerContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    assert_eq!(client.get_admin(), Some(admin.clone()));

    // Initializing twice should fail
    let res = client.try_initialize(&admin);
    assert_eq!(res, Err(Ok(PointsLedgerError::AlreadyInitialized)));
}

#[test]
fn test_admin_can_earn() {
    let (env, admin, _, player1, _) = create_test_env();
    let contract_id = env.register_contract(None, PointsLedgerContract);
    let client = PointsLedgerContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    assert_eq!(client.get_balance(&player1), 0);
    assert_eq!(client.get_history(&player1).len(), 0);

    let reason = symbol_short!("WELCOME");
    client.earn(&admin, &player1, &100, &reason);

    assert_eq!(client.get_balance(&player1), 100);

    let history = client.get_history(&player1);
    assert_eq!(history.len(), 1);
    let entry = history.get(0).unwrap();
    assert_eq!(entry.player, player1);
    assert_eq!(entry.amount, 100);
    assert_eq!(entry.reason_code, reason);
    assert_eq!(entry.caller, admin);
}

#[test]
fn test_authorized_contract_can_earn() {
    let (env, admin, game_contract, player1, _) = create_test_env();
    let contract_id = env.register_contract(None, PointsLedgerContract);
    let client = PointsLedgerContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    assert!(!client.is_authorized(&game_contract));
    client.authorize_caller(&admin, &game_contract);
    assert!(client.is_authorized(&game_contract));

    let reason = symbol_short!("PUZZLE");
    client.earn(&game_contract, &player1, &50, &reason);

    assert_eq!(client.get_balance(&player1), 50);

    let history = client.get_history(&player1);
    assert_eq!(history.len(), 1);
    let entry = history.get(0).unwrap();
    assert_eq!(entry.caller, game_contract);
    assert_eq!(entry.amount, 50);
}

#[test]
fn test_unauthorized_earn_fails() {
    let (env, admin, _, player1, player2) = create_test_env();
    let contract_id = env.register_contract(None, PointsLedgerContract);
    let client = PointsLedgerContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    // player2 is not authorized to award points
    let reason = symbol_short!("CHEAT");
    let res = client.try_earn(&player2, &player1, &1000, &reason);
    assert_eq!(res, Err(Ok(PointsLedgerError::NotAuthorized)));
    assert_eq!(client.get_balance(&player1), 0);
}

#[test]
fn test_balance_equals_sum_of_history() {
    let (env, admin, game_contract, player1, _) = create_test_env();
    let contract_id = env.register_contract(None, PointsLedgerContract);
    let client = PointsLedgerContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    client.authorize_caller(&admin, &game_contract);

    client.earn(&game_contract, &player1, &50, &symbol_short!("PUZZLE1"));
    client.earn(&game_contract, &player1, &75, &symbol_short!("PUZZLE2"));
    client.earn(&admin, &player1, &25, &symbol_short!("BONUS"));

    let history = client.get_history(&player1);
    assert_eq!(history.len(), 3);

    let mut sum: u64 = 0;
    for entry in history.iter() {
        sum = sum.checked_add(entry.amount).unwrap();
    }

    assert_eq!(sum, 150);
    assert_eq!(client.get_balance(&player1), sum);
}

#[test]
fn test_deauthorize_contract() {
    let (env, admin, game_contract, player1, _) = create_test_env();
    let contract_id = env.register_contract(None, PointsLedgerContract);
    let client = PointsLedgerContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    client.authorize_caller(&admin, &game_contract);
    assert!(client.is_authorized(&game_contract));

    client.deauthorize_caller(&admin, &game_contract);
    assert!(!client.is_authorized(&game_contract));

    let res = client.try_earn(&game_contract, &player1, &50, &symbol_short!("EXPLOIT"));
    assert_eq!(res, Err(Ok(PointsLedgerError::NotAuthorized)));
}

#[test]
fn test_invalid_zero_amount() {
    let (env, admin, _, player1, _) = create_test_env();
    let contract_id = env.register_contract(None, PointsLedgerContract);
    let client = PointsLedgerContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    let res = client.try_earn(&admin, &player1, &0, &symbol_short!("ZERO"));
    assert_eq!(res, Err(Ok(PointsLedgerError::InvalidAmount)));
}

#[test]
fn test_transfer_admin() {
    let (env, admin, game_contract, player1, new_admin) = create_test_env();
    let contract_id = env.register_contract(None, PointsLedgerContract);
    let client = PointsLedgerContractClient::new(&env, &contract_id);

    client.initialize(&admin);
    client.transfer_admin(&admin, &new_admin);

    assert_eq!(client.get_admin(), Some(new_admin.clone()));

    // Old admin can no longer authorize
    let old_res = client.try_authorize_caller(&admin, &game_contract);
    assert_eq!(old_res, Err(Ok(PointsLedgerError::NotAuthorized)));

    // New admin can authorize
    let new_res = client.try_authorize_caller(&new_admin, &game_contract);
    assert!(new_res.is_ok());
    assert!(client.is_authorized(&game_contract));

    client.earn(&game_contract, &player1, &100, &symbol_short!("QUEST"));
    assert_eq!(client.get_balance(&player1), 100);
}
