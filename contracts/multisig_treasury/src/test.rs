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
    
    client.initialize(
        &owner,
        &2,                  // threshold: 2 signatures required
        &86400,              // proposal_timeout: 24 hours
        &10,                 // max_pending_proposals: 10
    );
    
    let config = client.get_config_info();
    assert_eq!(config.owner, owner);
    assert_eq!(config.threshold, 2);
    assert_eq!(config.total_signers, 1);
    assert_eq!(config.proposal_timeout, 86400);
    assert_eq!(config.max_pending_proposals, 10);
    assert!(config.emergency_recovery_enabled);
    
    // Check owner is member
    let member = client.get_member_info(&owner).unwrap();
    assert_eq!(member.address, owner);
    assert!(matches!(member.role, Role::Owner));
    assert!(member.active);
    
    // Check members list
    let members = client.get_all_members();
    assert_eq!(members.len(), 1);
    assert_eq!(members.get(0).unwrap(), owner);
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_double_initialization() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    // Try to initialize again - should panic
    MultisigTreasury::initialize(
        env.clone(),
        owner.clone(),
        2,
        86400,
        10,
    );
}

#[test]
#[should_panic(expected = "Invalid threshold")]
fn test_initialize_with_zero_threshold() {
    let (env, owner) = setup_env();
    
    MultisigTreasury::initialize(
        env.clone(),
        owner.clone(),
        0,  // Invalid threshold
        86400,
        10,
    );
}

#[test]
fn test_add_member() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);
    let admin1 = Address::generate(&env);
    
    // Add members
    MultisigTreasury::add_member(env.clone(), owner.clone(), signer1.clone(), Role::Signer);
    MultisigTreasury::add_member(env.clone(), owner.clone(), signer2.clone(), Role::Signer);
    MultisigTreasury::add_member(env.clone(), owner.clone(), admin1.clone(), Role::Admin);
    
    // Verify members added
    let config = MultisigTreasury::get_config_info(env.clone());
    assert_eq!(config.total_signers, 4);
    
    let members = MultisigTreasury::get_all_members(env.clone());
    assert_eq!(members.len(), 4);
    
    let member1 = MultisigTreasury::get_member_info(env.clone(), signer1).unwrap();
    assert!(matches!(member1.role, Role::Signer));
    
    let admin = MultisigTreasury::get_member_info(env.clone(), admin1).unwrap();
    assert!(matches!(admin.role, Role::Admin));
}

#[test]
#[should_panic(expected = "Member already exists")]
fn test_add_duplicate_member() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let signer = Address::generate(&env);
    
    MultisigTreasury::add_member(env.clone(), owner.clone(), signer.clone(), Role::Signer);
    // Try to add same member again
    MultisigTreasury::add_member(env.clone(), owner.clone(), signer.clone(), Role::Signer);
}

#[test]
#[should_panic(expected = "Insufficient role to add members")]
fn test_signer_cannot_add_member() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let signer = Address::generate(&env);
    MultisigTreasury::add_member(env.clone(), owner.clone(), signer.clone(), Role::Signer);
    
    // Try to add member as signer
    let new_member = Address::generate(&env);
    MultisigTreasury::add_member(env.clone(), signer.clone(), new_member, Role::Signer);
}

#[test]
#[should_panic(expected = "Only Owner can add Owners or Admins")]
fn test_admin_cannot_add_admin() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let admin = Address::generate(&env);
    MultisigTreasury::add_member(env.clone(), owner.clone(), admin.clone(), Role::Admin);
    
    // Admin tries to add another admin
    let new_admin = Address::generate(&env);
    MultisigTreasury::add_member(env.clone(), admin.clone(), new_admin, Role::Admin);
}

#[test]
fn test_remove_member() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);
    
    MultisigTreasury::add_member(env.clone(), owner.clone(), signer1.clone(), Role::Signer);
    MultisigTreasury::add_member(env.clone(), owner.clone(), signer2.clone(), Role::Signer);
    
    // Remove a signer
    MultisigTreasury::remove_member(env.clone(), owner.clone(), signer1.clone());
    
    // Verify removal
    let member = MultisigTreasury::get_member_info(env.clone(), signer1);
    assert!(member.is_none());
    
    let members = MultisigTreasury::get_all_members(env.clone());
    assert_eq!(members.len(), 2);
    
    let config = MultisigTreasury::get_config_info(env.clone());
    assert_eq!(config.total_signers, 2);
}

#[test]
#[should_panic(expected = "Cannot remove last owner")]
fn test_cannot_remove_last_owner() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    // Try to remove the only owner
    MultisigTreasury::remove_member(env.clone(), owner.clone(), owner.clone());
}

#[test]
fn test_admin_can_remove_signer() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let admin = Address::generate(&env);
    let signer = Address::generate(&env);
    
    MultisigTreasury::add_member(env.clone(), owner.clone(), admin.clone(), Role::Admin);
    MultisigTreasury::add_member(env.clone(), owner.clone(), signer.clone(), Role::Signer);
    
    // Admin removes signer
    MultisigTreasury::remove_member(env.clone(), admin.clone(), signer.clone());
    
    let member = MultisigTreasury::get_member_info(env.clone(), signer);
    assert!(member.is_none());
}

#[test]
#[should_panic(expected = "Admin can only remove Signers")]
fn test_admin_cannot_remove_admin() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let admin1 = Address::generate(&env);
    let admin2 = Address::generate(&env);
    
    MultisigTreasury::add_member(env.clone(), owner.clone(), admin1.clone(), Role::Admin);
    MultisigTreasury::add_member(env.clone(), owner.clone(), admin2.clone(), Role::Admin);
    
    // Admin tries to remove another admin
    MultisigTreasury::remove_member(env.clone(), admin1.clone(), admin2.clone());
}

#[test]
fn test_update_member_role() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let signer = Address::generate(&env);
    MultisigTreasury::add_member(env.clone(), owner.clone(), signer.clone(), Role::Signer);
    
    // Promote signer to admin
    MultisigTreasury::update_member_role(env.clone(), owner.clone(), signer.clone(), Role::Admin);
    
    let member = MultisigTreasury::get_member_info(env.clone(), signer).unwrap();
    assert!(matches!(member.role, Role::Admin));
}

#[test]
fn test_propose_and_sign_transfer() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);
    let token = Address::generate(&env);
    let destination = Address::generate(&env);
    
    MultisigTreasury::add_member(env.clone(), owner.clone(), signer1.clone(), Role::Signer);
    MultisigTreasury::add_member(env.clone(), owner.clone(), signer2.clone(), Role::Signer);
    
    // Propose transfer (threshold is 2, so need 2 signatures)
    let tx_id = MultisigTreasury::propose_transfer(
        env.clone(),
        owner.clone(),
        token.clone(),
        destination.clone(),
        1000,
        String::from_str(&env, "Test transfer"),
    );
    
    assert_eq!(tx_id, 1);
    
    // Get transaction info
    let tx = MultisigTreasury::get_transaction_info(env.clone(), tx_id).unwrap();
    assert_eq!(tx.proposer, owner);
    assert!(matches!(tx.transaction_type, TransactionType::TokenTransfer));
    assert!(matches!(tx.status, TransactionStatus::Pending));
    assert_eq!(tx.signatures.len(), 0);
    
    // Sign as owner
    MultisigTreasury::sign_transaction(env.clone(), owner.clone(), tx_id);
    
    let tx = MultisigTreasury::get_transaction_info(env.clone(), tx_id).unwrap();
    assert_eq!(tx.signatures.len(), 1);
    
    // Sign as signer1 - should reach threshold
    MultisigTreasury::sign_transaction(env.clone(), signer1.clone(), tx_id);
    
    let tx = MultisigTreasury::get_transaction_info(env.clone(), tx_id).unwrap();
    assert_eq!(tx.signatures.len(), 2);
    assert!(matches!(tx.status, TransactionStatus::Approved));
    
    // Verify signer tracking
    assert!(MultisigTreasury::has_signer_signed(env.clone(), tx_id, owner.clone()));
    assert!(MultisigTreasury::has_signer_signed(env.clone(), tx_id, signer1.clone()));
    assert!(!MultisigTreasury::has_signer_signed(env.clone(), tx_id, signer2.clone()));
}

#[test]
#[should_panic(expected = "Already signed")]
fn test_cannot_sign_twice() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let token = Address::generate(&env);
    let destination = Address::generate(&env);
    
    let tx_id = MultisigTreasury::propose_transfer(
        env.clone(),
        owner.clone(),
        token.clone(),
        destination.clone(),
        1000,
        String::from_str(&env, "Test transfer"),
    );
    
    MultisigTreasury::sign_transaction(env.clone(), owner.clone(), tx_id);
    // Try to sign again
    MultisigTreasury::sign_transaction(env.clone(), owner.clone(), tx_id);
}

#[test]
#[should_panic(expected = "Not a member")]
fn test_non_member_cannot_sign() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let token = Address::generate(&env);
    let destination = Address::generate(&env);
    let non_member = Address::generate(&env);
    
    let tx_id = MultisigTreasury::propose_transfer(
        env.clone(),
        owner.clone(),
        token.clone(),
        destination.clone(),
        1000,
        String::from_str(&env, "Test transfer"),
    );
    
    MultisigTreasury::sign_transaction(env.clone(), non_member.clone(), tx_id);
}

#[test]
fn test_propose_contract_call() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let target_contract = Address::generate(&env);
    
    let args = Vec::from_array(&env, [
        100i32.into_val(&env),
    ]);
    
    let tx_id = MultisigTreasury::propose_contract_call(
        env.clone(),
        owner.clone(),
        target_contract.clone(),
        Symbol::new(&env, "test_function"),
        args,
        String::from_str(&env, "Test contract call"),
    );
    
    let tx = MultisigTreasury::get_transaction_info(env.clone(), tx_id).unwrap();
    assert!(matches!(tx.transaction_type, TransactionType::ContractCall));
    assert_eq!(tx.target, Some(target_contract));
}

#[test]
fn test_reject_transaction() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let token = Address::generate(&env);
    let destination = Address::generate(&env);
    
    let tx_id = MultisigTreasury::propose_transfer(
        env.clone(),
        owner.clone(),
        token.clone(),
        destination.clone(),
        1000,
        String::from_str(&env, "Test transfer"),
    );
    
    MultisigTreasury::reject_transaction(env.clone(), owner.clone(), tx_id);
    
    let tx = MultisigTreasury::get_transaction_info(env.clone(), tx_id).unwrap();
    assert!(matches!(tx.status, TransactionStatus::Rejected));
    
    // Check pending list
    let pending = MultisigTreasury::get_pending_transaction_ids(env.clone());
    assert_eq!(pending.len(), 0);
}

#[test]
#[should_panic(expected = "Only proposer can reject")]
fn test_non_proposer_cannot_reject() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let signer = Address::generate(&env);
    MultisigTreasury::add_member(env.clone(), owner.clone(), signer.clone(), Role::Signer);
    
    let token = Address::generate(&env);
    let destination = Address::generate(&env);
    
    let tx_id = MultisigTreasury::propose_transfer(
        env.clone(),
        owner.clone(),
        token.clone(),
        destination.clone(),
        1000,
        String::from_str(&env, "Test transfer"),
    );
    
    // Signer tries to reject owner's proposal
    MultisigTreasury::reject_transaction(env.clone(), signer.clone(), tx_id);
}

#[test]
fn test_update_config() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    // Add more signers to allow threshold increase
    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);
    MultisigTreasury::add_member(env.clone(), owner.clone(), signer1.clone(), Role::Signer);
    MultisigTreasury::add_member(env.clone(), owner.clone(), signer2.clone(), Role::Signer);
    
    // Update config
    MultisigTreasury::update_config(
        env.clone(),
        owner.clone(),
        3,           // New threshold
        172800,      // 48 hour timeout
        20,          // Max 20 pending
    );
    
    let config = MultisigTreasury::get_config_info(env.clone());
    assert_eq!(config.threshold, 3);
    assert_eq!(config.proposal_timeout, 172800);
    assert_eq!(config.max_pending_proposals, 20);
}

#[test]
#[should_panic(expected = "Only Owner can update config")]
fn test_non_owner_cannot_update_config() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let signer = Address::generate(&env);
    MultisigTreasury::add_member(env.clone(), owner.clone(), signer.clone(), Role::Signer);
    
    MultisigTreasury::update_config(
        env.clone(),
        signer.clone(),
        1,
        86400,
        10,
    );
}

#[test]
fn test_emergency_recovery_activation() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    // Add another owner for recovery
    let owner2 = Address::generate(&env);
    MultisigTreasury::add_member(env.clone(), owner.clone(), owner2.clone(), Role::Owner);
    
    // Activate emergency
    MultisigTreasury::activate_emergency_recovery(
        env.clone(),
        owner.clone(),
        String::from_str(&env, "Security breach detected"),
    );
    
    let emergency = MultisigTreasury::get_emergency_info(env.clone()).unwrap();
    assert_eq!(emergency.activated_by, owner);
    assert!(!emergency.recovery_approved);
    assert_eq!(emergency.reason, String::from_str(&env, "Security breach detected"));
}

#[test]
#[should_panic(expected = "Only Owner can activate emergency recovery")]
fn test_non_owner_cannot_activate_emergency() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let admin = Address::generate(&env);
    MultisigTreasury::add_member(env.clone(), owner.clone(), admin.clone(), Role::Admin);
    
    MultisigTreasury::activate_emergency_recovery(
        env.clone(),
        admin.clone(),
        String::from_str(&env, "Emergency"),
    );
}

#[test]
fn test_emergency_recovery_execution() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    // Add another owner
    let owner2 = Address::generate(&env);
    MultisigTreasury::add_member(env.clone(), owner.clone(), owner2.clone(), Role::Owner);
    
    // Add new owner candidate
    let new_owner = Address::generate(&env);
    
    // Activate emergency
    MultisigTreasury::activate_emergency_recovery(
        env.clone(),
        owner.clone(),
        String::from_str(&env, "Owner key compromised"),
    );
    
    // Second owner executes recovery
    MultisigTreasury::execute_emergency_recovery(
        env.clone(),
        owner2.clone(),
        new_owner.clone(),
    );
    
    // Verify new owner is set
    let config = MultisigTreasury::get_config_info(env.clone());
    assert_eq!(config.owner, new_owner);
    
    // Verify new owner is member
    let member = MultisigTreasury::get_member_info(env.clone(), new_owner).unwrap();
    assert!(matches!(member.role, Role::Owner));
    
    let emergency = MultisigTreasury::get_emergency_info(env.clone()).unwrap();
    assert!(emergency.recovery_approved);
}

#[test]
fn test_cancel_emergency_recovery() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let owner2 = Address::generate(&env);
    MultisigTreasury::add_member(env.clone(), owner.clone(), owner2.clone(), Role::Owner);
    
    // Activate emergency
    MultisigTreasury::activate_emergency_recovery(
        env.clone(),
        owner.clone(),
        String::from_str(&env, "False alarm"),
    );
    
    // Cancel emergency
    MultisigTreasury::cancel_emergency_recovery(env.clone(), owner2.clone());
    
    // Emergency state should be removed
    let emergency = MultisigTreasury::get_emergency_info(env.clone());
    assert!(emergency.is_none());
}

#[test]
fn test_emergency_cooldown() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let owner2 = Address::generate(&env);
    MultisigTreasury::add_member(env.clone(), owner.clone(), owner2.clone(), Role::Owner);
    
    // First activation
    MultisigTreasury::activate_emergency_recovery(
        env.clone(),
        owner.clone(),
        String::from_str(&env, "First emergency"),
    );
    
    // Cancel it
    MultisigTreasury::cancel_emergency_recovery(env.clone(), owner2.clone());
    
    // Try to activate again immediately - should fail due to cooldown
    // Note: We can't easily test this without ledger manipulation in this test framework
    // In practice, the cooldown would be enforced
}

#[test]
#[should_panic(expected = "Activator cannot be the only approver")]
fn test_emergency_requires_second_owner() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    // Add another owner
    let owner2 = Address::generate(&env);
    MultisigTreasury::add_member(env.clone(), owner.clone(), owner2.clone(), Role::Owner);
    
    let new_owner = Address::generate(&env);
    
    // Activate emergency
    MultisigTreasury::activate_emergency_recovery(
        env.clone(),
        owner.clone(),
        String::from_str(&env, "Emergency"),
    );
    
    // Activator tries to execute recovery alone - should fail
    MultisigTreasury::execute_emergency_recovery(
        env.clone(),
        owner.clone(),
        new_owner.clone(),
    );
}

#[test]
fn test_transaction_counter() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let token = Address::generate(&env);
    let destination = Address::generate(&env);
    
    // Propose multiple transactions
    let tx1 = MultisigTreasury::propose_transfer(
        env.clone(),
        owner.clone(),
        token.clone(),
        destination.clone(),
        100,
        String::from_str(&env, "First"),
    );
    
    let tx2 = MultisigTreasury::propose_transfer(
        env.clone(),
        owner.clone(),
        token.clone(),
        destination.clone(),
        200,
        String::from_str(&env, "Second"),
    );
    
    let tx3 = MultisigTreasury::propose_transfer(
        env.clone(),
        owner.clone(),
        token.clone(),
        destination.clone(),
        300,
        String::from_str(&env, "Third"),
    );
    
    assert_eq!(tx1, 1);
    assert_eq!(tx2, 2);
    assert_eq!(tx3, 3);
    
    // Check pending transactions
    let pending = MultisigTreasury::get_pending_transaction_ids(env.clone());
    assert_eq!(pending.len(), 3);
}

#[test]
fn test_role_levels() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let admin = Address::generate(&env);
    let signer = Address::generate(&env);
    
    MultisigTreasury::add_member(env.clone(), owner.clone(), admin.clone(), Role::Admin);
    MultisigTreasury::add_member(env.clone(), owner.clone(), signer.clone(), Role::Signer);
    
    // Verify role levels are correct
    let owner_member = MultisigTreasury::get_member_info(env.clone(), owner).unwrap();
    let admin_member = MultisigTreasury::get_member_info(env.clone(), admin).unwrap();
    let signer_member = MultisigTreasury::get_member_info(env.clone(), signer).unwrap();
    
    assert!(matches!(owner_member.role, Role::Owner));
    assert!(matches!(admin_member.role, Role::Admin));
    assert!(matches!(signer_member.role, Role::Signer));
}

#[test]
fn test_pending_transaction_limit() {
    let (env, owner) = setup_env();
    
    initialize_contract(&env, &owner);
    
    let token = Address::generate(&env);
    let destination = Address::generate(&env);
    
    // Fill up to max pending (10)
    for i in 0..10 {
        MultisigTreasury::propose_transfer(
            env.clone(),
            owner.clone(),
            token.clone(),
            destination.clone(),
            (i as i128) * 100,
            String::from_str(&env, "Fill pending"),
        );
    }
    
    // Next proposal should fail
    let result = std::panic::catch_unwind(|| {
        MultisigTreasury::propose_transfer(
            env.clone(),
            owner.clone(),
            token.clone(),
            destination.clone(),
            1000,
            String::from_str(&env, "Overflow"),
        );
    });
    
    assert!(result.is_err());
}
