#![cfg(test)]

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    vec, Address, Env, Symbol,
};
use types::{PolicyEffect, PolicyRule};

fn setup() -> (Env, AccessControlContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 1000);
    let contract_id = env.register_contract(None, AccessControlContract);
    let client = AccessControlContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

fn set_time(env: &Env, ts: u64) {
    env.ledger().with_mut(|li| li.timestamp = ts);
}

// ------------------------------------------------------------ initialization

#[test]
fn test_initialize_and_admin() {
    let (_env, client, admin) = setup();
    assert_eq!(client.get_admin(), admin);
    assert!(!client.is_paused());
}

#[test]
fn test_double_initialize_fails() {
    let (env, client, _admin) = setup();
    let other = Address::generate(&env);
    let result = client.try_initialize(&other);
    assert_eq!(result, Err(Ok(AccessError::AlreadyInitialized)));
}

#[test]
fn test_transfer_admin() {
    let (env, client, _admin) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&new_admin);
    assert_eq!(client.get_admin(), new_admin);
}

// -------------------------------------------------------------------- roles

#[test]
fn test_grant_and_check_role() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let role = symbol_short!("editor");
    assert!(!client.has_role(&user, &role));
    client.grant_role(&user, &role, &0);
    assert!(client.has_role(&user, &role));
}

#[test]
fn test_revoke_role() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let role = symbol_short!("editor");
    client.grant_role(&user, &role, &0);
    client.revoke_role(&user, &role);
    assert!(!client.has_role(&user, &role));
}

#[test]
fn test_revoke_unassigned_role_fails() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let result = client.try_revoke_role(&user, &symbol_short!("editor"));
    assert_eq!(result, Err(Ok(AccessError::RoleNotGranted)));
}

#[test]
fn test_time_bound_role_expires() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let role = symbol_short!("temp");
    client.grant_role(&user, &role, &2000);
    assert!(client.has_role(&user, &role));
    set_time(&env, 2000);
    assert!(!client.has_role(&user, &role));
}

#[test]
fn test_grant_role_with_past_expiry_fails() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let result = client.try_grant_role(&user, &symbol_short!("temp"), &500);
    assert_eq!(result, Err(Ok(AccessError::InvalidExpiry)));
}

#[test]
fn test_regrant_updates_expiry() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let role = symbol_short!("temp");
    client.grant_role(&user, &role, &2000);
    client.grant_role(&user, &role, &5000);
    set_time(&env, 3000);
    assert!(client.has_role(&user, &role));
    assert_eq!(client.get_role_grants(&user).len(), 1);
}

// -------------------------------------------------------------- permissions

#[test]
fn test_permission_checks() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let role = symbol_short!("editor");
    let perm = symbol_short!("write");
    client.grant_role(&user, &role, &0);
    assert!(!client.has_permission(&user, &perm));
    client.add_permission(&role, &perm);
    assert!(client.has_permission(&user, &perm));
    client.remove_permission(&role, &perm);
    assert!(!client.has_permission(&user, &perm));
}

#[test]
fn test_duplicate_permission_fails() {
    let (_env, client, _admin) = setup();
    let role = symbol_short!("editor");
    let perm = symbol_short!("write");
    client.add_permission(&role, &perm);
    let result = client.try_add_permission(&role, &perm);
    assert_eq!(result, Err(Ok(AccessError::PermissionAlreadyExists)));
}

#[test]
fn test_permission_lost_when_role_expires() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let role = symbol_short!("editor");
    let perm = symbol_short!("write");
    client.grant_role(&user, &role, &2000);
    client.add_permission(&role, &perm);
    assert!(client.has_permission(&user, &perm));
    set_time(&env, 2500);
    assert!(!client.has_permission(&user, &perm));
}

// ---------------------------------------------------------- role delegation

#[test]
fn test_delegate_role() {
    let (env, client, _admin) = setup();
    let delegator = Address::generate(&env);
    let delegate = Address::generate(&env);
    let role = symbol_short!("editor");
    client.grant_role(&delegator, &role, &0);
    client.delegate_role(&delegator, &delegate, &role, &0);
    assert!(client.has_role(&delegate, &role));
}

#[test]
fn test_delegate_role_without_grant_fails() {
    let (env, client, _admin) = setup();
    let delegator = Address::generate(&env);
    let delegate = Address::generate(&env);
    let result = client.try_delegate_role(&delegator, &delegate, &symbol_short!("editor"), &0);
    assert_eq!(result, Err(Ok(AccessError::RoleNotGranted)));
}

#[test]
fn test_self_delegation_fails() {
    let (env, client, _admin) = setup();
    let delegator = Address::generate(&env);
    let role = symbol_short!("editor");
    client.grant_role(&delegator, &role, &0);
    let result = client.try_delegate_role(&delegator, &delegator, &role, &0);
    assert_eq!(result, Err(Ok(AccessError::SelfDelegation)));
}

#[test]
fn test_duplicate_delegation_fails() {
    let (env, client, _admin) = setup();
    let delegator = Address::generate(&env);
    let delegate = Address::generate(&env);
    let role = symbol_short!("editor");
    client.grant_role(&delegator, &role, &0);
    client.delegate_role(&delegator, &delegate, &role, &0);
    let result = client.try_delegate_role(&delegator, &delegate, &role, &0);
    assert_eq!(result, Err(Ok(AccessError::DelegationAlreadyExists)));
}

#[test]
fn test_delegation_expires() {
    let (env, client, _admin) = setup();
    let delegator = Address::generate(&env);
    let delegate = Address::generate(&env);
    let role = symbol_short!("editor");
    client.grant_role(&delegator, &role, &0);
    client.delegate_role(&delegator, &delegate, &role, &2000);
    assert!(client.has_role(&delegate, &role));
    set_time(&env, 2000);
    assert!(!client.has_role(&delegate, &role));
    // Delegator keeps the role.
    assert!(client.has_role(&delegator, &role));
}

#[test]
fn test_delegation_dies_with_delegator_grant() {
    let (env, client, _admin) = setup();
    let delegator = Address::generate(&env);
    let delegate = Address::generate(&env);
    let role = symbol_short!("editor");
    client.grant_role(&delegator, &role, &0);
    client.delegate_role(&delegator, &delegate, &role, &0);
    client.revoke_role(&delegator, &role);
    assert!(!client.has_role(&delegate, &role));
}

#[test]
fn test_revoke_role_delegation() {
    let (env, client, _admin) = setup();
    let delegator = Address::generate(&env);
    let delegate = Address::generate(&env);
    let role = symbol_short!("editor");
    client.grant_role(&delegator, &role, &0);
    client.delegate_role(&delegator, &delegate, &role, &0);
    client.revoke_role_delegation(&delegator, &delegate, &role, &delegator);
    assert!(!client.has_role(&delegate, &role));
}

#[test]
fn test_revoke_delegation_by_stranger_fails() {
    let (env, client, _admin) = setup();
    let delegator = Address::generate(&env);
    let delegate = Address::generate(&env);
    let stranger = Address::generate(&env);
    let role = symbol_short!("editor");
    client.grant_role(&delegator, &role, &0);
    client.delegate_role(&delegator, &delegate, &role, &0);
    let result = client.try_revoke_role_delegation(&stranger, &delegate, &role, &delegator);
    assert_eq!(result, Err(Ok(AccessError::NotAuthorized)));
}

// --------------------------------------------------------------- capability

#[test]
fn test_cache_invalidation() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let role = symbol_short!("editor");

    // Grant the role and check that it's cached
    client.grant_role(&user, &role, &0);
    assert!(client.has_role(&user, &role));

    // Revoke the role and check that the cache is invalidated
    client.revoke_role(&user, &role);
    assert!(!client.has_role(&user, &role));
}

#[test]
fn test_issue_and_use_capability() {
    let (env, client, _admin) = setup();
    let holder = Address::generate(&env);
    let id = client.issue_capability(
        &holder,
        &symbol_short!("vault"),
        &symbol_short!("open"),
        &0,
        &0,
        &false,
        &0,
    );
    assert!(client.is_capability_valid(&id));
    client.use_capability(&id);
    assert_eq!(client.get_capability(&id).uses, 1);
}

#[test]
fn test_capability_max_uses_exhausted() {
    let (env, client, _admin) = setup();
    let holder = Address::generate(&env);
    let id = client.issue_capability(
        &holder,
        &symbol_short!("vault"),
        &symbol_short!("open"),
        &0,
        &2,
        &false,
        &0,
    );
    client.use_capability(&id);
    client.use_capability(&id);
    let result = client.try_use_capability(&id);
    assert_eq!(result, Err(Ok(AccessError::CapabilityExhausted)));
    assert!(!client.is_capability_valid(&id));
}

#[test]
fn test_capability_expires() {
    let (env, client, _admin) = setup();
    let holder = Address::generate(&env);
    let id = client.issue_capability(
        &holder,
        &symbol_short!("vault"),
        &symbol_short!("open"),
        &2000,
        &0,
        &false,
        &0,
    );
    assert!(client.is_capability_valid(&id));
    set_time(&env, 2000);
    assert!(!client.is_capability_valid(&id));
    let result = client.try_use_capability(&id);
    assert_eq!(result, Err(Ok(AccessError::CapabilityExpired)));
}

#[test]
fn test_revoke_capability() {
    let (env, client, admin) = setup();
    let holder = Address::generate(&env);
    let id = client.issue_capability(
        &holder,
        &symbol_short!("vault"),
        &symbol_short!("open"),
        &0,
        &0,
        &false,
        &0,
    );
    client.revoke_capability(&admin, &id);
    assert!(!client.is_capability_valid(&id));
    let result = client.try_use_capability(&id);
    assert_eq!(result, Err(Ok(AccessError::CapabilityRevoked)));
}

#[test]
fn test_revoke_capability_by_stranger_fails() {
    let (env, client, _admin) = setup();
    let holder = Address::generate(&env);
    let stranger = Address::generate(&env);
    let id = client.issue_capability(
        &holder,
        &symbol_short!("vault"),
        &symbol_short!("open"),
        &0,
        &0,
        &false,
        &0,
    );
    let result = client.try_revoke_capability(&stranger, &id);
    assert_eq!(result, Err(Ok(AccessError::NotAuthorized)));
}

// ---------------------------------------------------- capability delegation

#[test]
fn test_delegate_capability() {
    let (env, client, _admin) = setup();
    let holder = Address::generate(&env);
    let delegate = Address::generate(&env);
    let id = client.issue_capability(
        &holder,
        &symbol_short!("vault"),
        &symbol_short!("open"),
        &0,
        &0,
        &true,
        &2,
    );
    let child_id = client.delegate_capability(&id, &delegate, &0);
    assert!(client.is_capability_valid(&child_id));
    let child = client.get_capability(&child_id);
    assert_eq!(child.holder, delegate);
    assert_eq!(child.depth, 1);
    assert_eq!(child.parent, Some(id));
    client.use_capability(&child_id);
}

#[test]
fn test_non_delegatable_capability_fails() {
    let (env, client, _admin) = setup();
    let holder = Address::generate(&env);
    let delegate = Address::generate(&env);
    let id = client.issue_capability(
        &holder,
        &symbol_short!("vault"),
        &symbol_short!("open"),
        &0,
        &0,
        &false,
        &2,
    );
    let result = client.try_delegate_capability(&id, &delegate, &0);
    assert_eq!(result, Err(Ok(AccessError::NotDelegatable)));
}

#[test]
fn test_delegation_depth_enforced() {
    let (env, client, _admin) = setup();
    let holder = Address::generate(&env);
    let second = Address::generate(&env);
    let third = Address::generate(&env);
    let id = client.issue_capability(
        &holder,
        &symbol_short!("vault"),
        &symbol_short!("open"),
        &0,
        &0,
        &true,
        &1,
    );
    let child_id = client.delegate_capability(&id, &second, &0);
    let result = client.try_delegate_capability(&child_id, &third, &0);
    assert_eq!(result, Err(Ok(AccessError::DelegationDepthExceeded)));
}

#[test]
fn test_child_expiry_bounded_by_parent() {
    let (env, client, _admin) = setup();
    let holder = Address::generate(&env);
    let delegate = Address::generate(&env);
    let id = client.issue_capability(
        &holder,
        &symbol_short!("vault"),
        &symbol_short!("open"),
        &3000,
        &0,
        &true,
        &1,
    );
    // Child may not outlive the parent (unbounded or later expiry).
    let result = client.try_delegate_capability(&id, &delegate, &0);
    assert_eq!(result, Err(Ok(AccessError::InvalidExpiry)));
    let result = client.try_delegate_capability(&id, &delegate, &4000);
    assert_eq!(result, Err(Ok(AccessError::InvalidExpiry)));
    let child_id = client.delegate_capability(&id, &delegate, &2500);
    assert!(client.is_capability_valid(&child_id));
}

#[test]
fn test_parent_revocation_cascades() {
    let (env, client, admin) = setup();
    let holder = Address::generate(&env);
    let second = Address::generate(&env);
    let third = Address::generate(&env);
    let id = client.issue_capability(
        &holder,
        &symbol_short!("vault"),
        &symbol_short!("open"),
        &0,
        &0,
        &true,
        &2,
    );
    let child_id = client.delegate_capability(&id, &second, &0);
    let grandchild_id = client.delegate_capability(&child_id, &third, &0);
    client.revoke_capability(&admin, &id);
    assert!(!client.is_capability_valid(&child_id));
    assert!(!client.is_capability_valid(&grandchild_id));
    let result = client.try_use_capability(&grandchild_id);
    assert_eq!(result, Err(Ok(AccessError::CapabilityRevoked)));
}

// ------------------------------------------------------------ policy engine

#[test]
fn test_policy_allow_rule() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let role = symbol_short!("editor");
    let resource = symbol_short!("doc");
    let action = symbol_short!("write");
    client.grant_role(&user, &role, &0);
    let rules = vec![
        &env,
        PolicyRule {
            effect: PolicyEffect::Allow,
            role: role.clone(),
        },
    ];
    client.set_policy(&resource, &action, &rules, &false);
    assert!(client.check_access(&user, &resource, &action));
    let outsider = Address::generate(&env);
    assert!(!client.check_access(&outsider, &resource, &action));
}

#[test]
fn test_policy_deny_overrides_allow() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let editor = symbol_short!("editor");
    let banned = symbol_short!("banned");
    let resource = symbol_short!("doc");
    let action = symbol_short!("write");
    client.grant_role(&user, &editor, &0);
    client.grant_role(&user, &banned, &0);
    let rules = vec![
        &env,
        PolicyRule {
            effect: PolicyEffect::Allow,
            role: editor.clone(),
        },
        PolicyRule {
            effect: PolicyEffect::Deny,
            role: banned.clone(),
        },
    ];
    client.set_policy(&resource, &action, &rules, &false);
    assert!(!client.check_access(&user, &resource, &action));
}

#[test]
fn test_policy_default_allow() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let resource = symbol_short!("park");
    let action = symbol_short!("enter");
    let rules = vec![
        &env,
        PolicyRule {
            effect: PolicyEffect::Deny,
            role: symbol_short!("banned"),
        },
    ];
    client.set_policy(&resource, &action, &rules, &true);
    assert!(client.check_access(&user, &resource, &action));
    client.grant_role(&user, &symbol_short!("banned"), &0);
    assert!(!client.check_access(&user, &resource, &action));
}

#[test]
fn test_remove_policy() {
    let (env, client, _admin) = setup();
    let resource = symbol_short!("doc");
    let action = symbol_short!("write");
    client.set_policy(&resource, &action, &vec![&env], &true);
    client.remove_policy(&resource, &action);
    let result = client.try_get_policy(&resource, &action);
    assert_eq!(result, Err(Ok(AccessError::PolicyNotFound)));
}

#[test]
fn test_check_access_permission_fallback() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let role = symbol_short!("editor");
    let action = symbol_short!("write");
    client.grant_role(&user, &role, &0);
    client.add_permission(&role, &action);
    // No policy for (doc, write): falls back to permission check.
    assert!(client.check_access(&user, &symbol_short!("doc"), &action));
    assert!(!client.check_access(&user, &symbol_short!("doc"), &symbol_short!("delete")));
}

#[test]
fn test_check_access_via_capability() {
    let (env, client, _admin) = setup();
    let holder = Address::generate(&env);
    let resource = symbol_short!("vault");
    let action = symbol_short!("open");
    assert!(!client.check_access(&holder, &resource, &action));
    client.issue_capability(&holder, &resource, &action, &0, &0, &false, &0);
    assert!(client.check_access(&holder, &resource, &action));
}

// -------------------------------------------------------------- access logs

#[test]
fn test_access_logs_recorded() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let resource = symbol_short!("doc");
    let action = symbol_short!("write");
    let before = client.get_access_log_count();
    client.check_access(&user, &resource, &action);
    let logs = client.get_access_logs(&before, &10);
    assert_eq!(logs.len(), 1);
    let entry = logs.get(0).unwrap();
    assert_eq!(entry.actor, user);
    assert_eq!(entry.resource, resource);
    assert_eq!(entry.action, action);
    assert!(!entry.allowed);
}

#[test]
fn test_can_access_does_not_log() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let before = client.get_access_log_count();
    client.can_access(&user, &symbol_short!("doc"), &symbol_short!("write"));
    assert_eq!(client.get_access_log_count(), before);
}

#[test]
fn test_capability_use_logged() {
    let (env, client, _admin) = setup();
    let holder = Address::generate(&env);
    let id = client.issue_capability(
        &holder,
        &symbol_short!("vault"),
        &symbol_short!("open"),
        &0,
        &0,
        &false,
        &0,
    );
    let before = client.get_access_log_count();
    client.use_capability(&id);
    let logs = client.get_access_logs(&before, &10);
    assert_eq!(logs.len(), 1);
    assert!(logs.get(0).unwrap().allowed);
}

// -------------------------------------------------------------------- pause

#[test]
fn test_pause_blocks_mutations() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    client.pause();
    assert!(client.is_paused());
    let result = client.try_grant_role(&user, &symbol_short!("editor"), &0);
    assert_eq!(result, Err(Ok(AccessError::ContractPaused)));
    client.unpause();
    client.grant_role(&user, &symbol_short!("editor"), &0);
    assert!(client.has_role(&user, &symbol_short!("editor")));
}