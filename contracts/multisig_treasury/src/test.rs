#![cfg(test)]

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, String, Symbol, IntoVal, Vec};

fn setup_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

#[test]
fn test_initialization() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let config = client.get_config_info();
    assert_eq!(config.owner, owner);
    assert_eq!(config.threshold, 2);
    assert_eq!(config.total_signers, 1);
    assert_eq!(config.proposal_timeout, 86400);
    assert_eq!(config.max_pending_proposals, 10);
    assert!(config.emergency_recovery_enabled);
    
    let member = client.get_member_info(&owner).unwrap();
    assert!(matches!(member.role, Role::Owner));
    assert!(member.active);
    
    let members = client.get_all_members();
    assert_eq!(members.len(), 1);
    assert_eq!(members.get(0).unwrap(), owner);
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_double_initialization() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    client.initialize(&owner, &2, &86400, &10, &0);
}

#[test]
#[should_panic(expected = "Invalid threshold")]
fn test_initialize_with_zero_threshold() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &0, &86400, &10, &0);
}

#[test]
fn test_add_member() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);
    let admin1 = Address::generate(&env);
    
    client.add_member(&owner, &signer1, &Role::Signer);
    client.add_member(&owner, &signer2, &Role::Signer);
    client.add_member(&owner, &admin1, &Role::Admin);
    
    let config = client.get_config_info();
    assert_eq!(config.total_signers, 4);
    
    let members = client.get_all_members();
    assert_eq!(members.len(), 4);
}

#[test]
#[should_panic(expected = "Member already exists")]
fn test_add_duplicate_member() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let signer = Address::generate(&env);
    client.add_member(&owner, &signer, &Role::Signer);
    client.add_member(&owner, &signer, &Role::Signer);
}

#[test]
#[should_panic(expected = "Insufficient role to add members")]
fn test_signer_cannot_add_member() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let signer = Address::generate(&env);
    client.add_member(&owner, &signer, &Role::Signer);
    
    let new_member = Address::generate(&env);
    client.add_member(&signer, &new_member, &Role::Signer);
}

#[test]
#[should_panic(expected = "Only Owner can add Owners or Admins")]
fn test_admin_cannot_add_admin() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let admin = Address::generate(&env);
    client.add_member(&owner, &admin, &Role::Admin);
    
    let new_admin = Address::generate(&env);
    client.add_member(&admin, &new_admin, &Role::Admin);
}

#[test]
fn test_remove_member() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);
    
    client.add_member(&owner, &signer1, &Role::Signer);
    client.add_member(&owner, &signer2, &Role::Signer);
    
    client.remove_member(&owner, &signer1);
    
    assert!(client.get_member_info(&signer1).is_none());
    assert_eq!(client.get_all_members().len(), 2);
    assert_eq!(client.get_config_info().total_signers, 2);
}

#[test]
#[should_panic(expected = "Cannot remove last owner")]
fn test_cannot_remove_last_owner() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    client.remove_member(&owner, &owner);
}

#[test]
fn test_admin_can_remove_signer() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let admin = Address::generate(&env);
    let signer = Address::generate(&env);
    
    client.add_member(&owner, &admin, &Role::Admin);
    client.add_member(&owner, &signer, &Role::Signer);
    
    client.remove_member(&admin, &signer);
    assert!(client.get_member_info(&signer).is_none());
}

#[test]
#[should_panic(expected = "Admin can only remove Signers")]
fn test_admin_cannot_remove_admin() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let admin1 = Address::generate(&env);
    let admin2 = Address::generate(&env);
    
    client.add_member(&owner, &admin1, &Role::Admin);
    client.add_member(&owner, &admin2, &Role::Admin);
    
    client.remove_member(&admin1, &admin2);
}

#[test]
fn test_update_member_role() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let signer = Address::generate(&env);
    client.add_member(&owner, &signer, &Role::Signer);
    client.update_member_role(&owner, &signer, &Role::Admin);
    
    assert!(matches!(client.get_member_info(&signer).unwrap().role, Role::Admin));
}

#[test]
fn test_propose_and_sign_transfer() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);
    let token = Address::generate(&env);
    let destination = Address::generate(&env);
    
    client.add_member(&owner, &signer1, &Role::Signer);
    client.add_member(&owner, &signer2, &Role::Signer);
    
    let tx_id = client.propose_transfer(&owner, &token, &destination, &1000, &String::from_str(&env, "Test transfer"));
    assert_eq!(tx_id, 1);
    
    let tx = client.get_transaction_info(&tx_id).unwrap();
    assert!(matches!(tx.status, TransactionStatus::Pending));
    
    client.sign_transaction(&owner, &tx_id);
    let tx = client.get_transaction_info(&tx_id).unwrap();
    assert_eq!(tx.signatures.len(), 1);
    
    client.sign_transaction(&signer1, &tx_id);
    let tx = client.get_transaction_info(&tx_id).unwrap();
    assert_eq!(tx.signatures.len(), 2);
    assert!(matches!(tx.status, TransactionStatus::Approved));
    
    assert!(client.has_signer_signed(&tx_id, &owner));
    assert!(client.has_signer_signed(&tx_id, &signer1));
}

#[test]
#[should_panic(expected = "Already signed")]
fn test_cannot_sign_twice() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let token = Address::generate(&env);
    let destination = Address::generate(&env);
    
    let tx_id = client.propose_transfer(&owner, &token, &destination, &1000, &String::from_str(&env, "Test"));
    client.sign_transaction(&owner, &tx_id);
    client.sign_transaction(&owner, &tx_id);
}

#[test]
#[should_panic(expected = "Not a member")]
fn test_non_member_cannot_sign() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let token = Address::generate(&env);
    let destination = Address::generate(&env);
    let non_member = Address::generate(&env);
    
    let tx_id = client.propose_transfer(&owner, &token, &destination, &1000, &String::from_str(&env, "Test"));
    client.sign_transaction(&non_member, &tx_id);
}

#[test]
fn test_propose_contract_call() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let target = Address::generate(&env);
    let args = Vec::from_array(&env, [100i32.into_val(&env)]);
    
    let tx_id = client.propose_contract_call(&owner, &target, &Symbol::new(&env, "test"), &args, &String::from_str(&env, "Test"));
    let tx = client.get_transaction_info(&tx_id).unwrap();
    assert!(matches!(tx.transaction_type, TransactionType::ContractCall));
}

#[test]
fn test_reject_transaction() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let token = Address::generate(&env);
    let destination = Address::generate(&env);
    
    let tx_id = client.propose_transfer(&owner, &token, &destination, &1000, &String::from_str(&env, "Test"));
    client.reject_transaction(&owner, &tx_id);
    
    let tx = client.get_transaction_info(&tx_id).unwrap();
    assert!(matches!(tx.status, TransactionStatus::Rejected));
    assert_eq!(client.get_pending_transaction_ids().len(), 0);
}

#[test]
#[should_panic(expected = "Only proposer can reject")]
fn test_non_proposer_cannot_reject() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let signer = Address::generate(&env);
    client.add_member(&owner, &signer, &Role::Signer);
    
    let token = Address::generate(&env);
    let destination = Address::generate(&env);
    
    let tx_id = client.propose_transfer(&owner, &token, &destination, &1000, &String::from_str(&env, "Test"));
    client.reject_transaction(&signer, &tx_id);
}

#[test]
fn test_update_config() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);
    client.add_member(&owner, &signer1, &Role::Signer);
    client.add_member(&owner, &signer2, &Role::Signer);
    
    client.update_config(&owner, &3, &172800, &20);
    
    let config = client.get_config_info();
    assert_eq!(config.threshold, 3);
    assert_eq!(config.proposal_timeout, 172800);
    assert_eq!(config.max_pending_proposals, 20);
}

#[test]
#[should_panic(expected = "Only Owner can update config")]
fn test_non_owner_cannot_update_config() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let signer = Address::generate(&env);
    client.add_member(&owner, &signer, &Role::Signer);
    
    client.update_config(&signer, &1, &86400, &10);
}

#[test]
fn test_emergency_recovery_activation() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let owner2 = Address::generate(&env);
    client.add_member(&owner, &owner2, &Role::Owner);
    
    client.activate_emergency_recovery(&owner, &String::from_str(&env, "Security breach"));
    
    let emergency = client.get_emergency_info().unwrap();
    assert!(!emergency.recovery_approved);
}

#[test]
#[should_panic(expected = "Only Owner can activate emergency recovery")]
fn test_non_owner_cannot_activate_emergency() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let admin = Address::generate(&env);
    client.add_member(&owner, &admin, &Role::Admin);
    
    client.activate_emergency_recovery(&admin, &String::from_str(&env, "Emergency"));
}

#[test]
fn test_emergency_recovery_execution() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let owner2 = Address::generate(&env);
    client.add_member(&owner, &owner2, &Role::Owner);
    
    let new_owner = Address::generate(&env);
    
    client.activate_emergency_recovery(&owner, &String::from_str(&env, "Compromised"));
    client.execute_emergency_recovery(&owner2, &new_owner);
    
    assert_eq!(client.get_config_info().owner, new_owner);
    assert!(matches!(client.get_member_info(&new_owner).unwrap().role, Role::Owner));
    assert!(client.get_emergency_info().unwrap().recovery_approved);
}

#[test]
fn test_cancel_emergency_recovery() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let owner2 = Address::generate(&env);
    client.add_member(&owner, &owner2, &Role::Owner);
    
    client.activate_emergency_recovery(&owner, &String::from_str(&env, "False alarm"));
    client.cancel_emergency_recovery(&owner2);
    
    assert!(client.get_emergency_info().is_none());
}

#[test]
#[should_panic(expected = "Activator cannot be the only approver")]
fn test_emergency_requires_second_owner() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let owner2 = Address::generate(&env);
    client.add_member(&owner, &owner2, &Role::Owner);
    
    let new_owner = Address::generate(&env);
    
    client.activate_emergency_recovery(&owner, &String::from_str(&env, "Emergency"));
    client.execute_emergency_recovery(&owner, &new_owner);
}

#[test]
fn test_transaction_counter() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let token = Address::generate(&env);
    let destination = Address::generate(&env);
    
    let tx1 = client.propose_transfer(&owner, &token, &destination, &100, &String::from_str(&env, "First"));
    let tx2 = client.propose_transfer(&owner, &token, &destination, &200, &String::from_str(&env, "Second"));
    let tx3 = client.propose_transfer(&owner, &token, &destination, &300, &String::from_str(&env, "Third"));
    
    assert_eq!(tx1, 1);
    assert_eq!(tx2, 2);
    assert_eq!(tx3, 3);
    assert_eq!(client.get_pending_transaction_ids().len(), 3);
}

#[test]
fn test_role_levels() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &2, &86400, &10, &0);
    
    let admin = Address::generate(&env);
    let signer = Address::generate(&env);
    
    client.add_member(&owner, &admin, &Role::Admin);
    client.add_member(&owner, &signer, &Role::Signer);
    
    assert!(matches!(client.get_member_info(&owner).unwrap().role, Role::Owner));
    assert!(matches!(client.get_member_info(&admin).unwrap().role, Role::Admin));
    assert!(matches!(client.get_member_info(&signer).unwrap().role, Role::Signer));
}

#[test]
#[should_panic(expected = "Too many pending proposals")]
fn test_pending_transaction_limit() {
    let env = setup_env();
    let owner = Address::generate(&env);
    
    let contract_id = env.register_contract(None, MultisigTreasury);
    let client = MultisigTreasuryClient::new(&env, &contract_id);
    
    client.initialize(&owner, &1, &86400, &3, &0);
    
    let token = Address::generate(&env);
    let destination = Address::generate(&env);
    
    client.propose_transfer(&owner, &token, &destination, &100, &String::from_str(&env, "1"));
    client.propose_transfer(&owner, &token, &destination, &200, &String::from_str(&env, "2"));
    client.propose_transfer(&owner, &token, &destination, &300, &String::from_str(&env, "3"));
    
    client.propose_transfer(&owner, &token, &destination, &400, &String::from_str(&env, "4"));
}
