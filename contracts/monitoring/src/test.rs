#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Symbol,
};

mod access_control {
    soroban_sdk::contractimport!(file = "../access_control/target/wasm32-unknown-unknown/release/access_control.wasm");
}

fn setup() -> (Env, MonitoringContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, MonitoringContract);
    let client = MonitoringContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    (env, client, admin)
}

#[test]
fn test_diff_snapshots() {
    let (env, client, _admin) = setup();
    let access_control_contract_id = env.register_contract_wasm(None, access_control::WASM);
    let access_control_client = access_control::Client::new(&env, &access_control_contract_id);

    let snapshot_id1 = client.take_snapshot(&access_control_contract_id);
    let snapshot1 = client.get_snapshot(&snapshot_id1).unwrap();

    let user = Address::generate(&env);
    let role = Symbol::new(&env, "test_role");
    access_control_client.grant_role(&user, &role);

    let snapshot_id2 = client.take_snapshot(&access_control_contract_id);
    let snapshot2 = client.get_snapshot(&snapshot_id2).unwrap();

    let diff = client.diff_snapshots(&snapshot1, &snapshot2);

    assert_eq!(diff.changed.len(), 1);
}

#[test]
fn test_audit_trail() {
    let (env, client, _admin) = setup();
    let access_control_contract_id = env.register_contract_wasm(None, access_control::WASM);

    let snapshot_id = client.take_snapshot(&access_control_contract_id);
    let audit_trail = client.get_audit_trail();

    assert_eq!(audit_trail.len(), 1);
    let entry = audit_trail.get(0).unwrap();
    assert_eq!(entry.snapshot_id, snapshot_id);
    assert!(entry.timestamp > 0);
}

#[test]
fn test_take_snapshot() {
    let (env, client, _admin) = setup();
    let access_control_contract_id = env.register_contract_wasm(None, access_control::WASM);
    let snapshot_id = client.take_snapshot(&access_control_contract_id);
    let snapshot = client.get_snapshot(&snapshot_id);

    assert!(snapshot.is_some());
    assert_eq!(snapshot.unwrap().state.len(), 2);
}