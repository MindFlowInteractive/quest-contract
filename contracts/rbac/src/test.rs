#![cfg(test)]
extern crate std;
use super::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol};

fn setup() -> (Env, Address, Address, RBACContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RBACContract);
    let client = RBACContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let emergency_admin = Address::generate(&env);
    client.initialize(&admin, &emergency_admin);
    (env, admin, emergency_admin, client)
}

fn role_admin() -> Symbol {
    Symbol::new(&Env::default(), "admin")
}

fn role_moderator() -> Symbol {
    Symbol::new(&Env::default(), "moderator")
}

fn role_user() -> Symbol {
    Symbol::new(&Env::default(), "user")
}

fn perm_read() -> Symbol {
    symbol_short!("read")
}

fn perm_write() -> Symbol {
    symbol_short!("write")
}

fn perm_delete() -> Symbol {
    symbol_short!("delete")
}

// ==================== Initialization ====================

#[test]
fn test_initialize_success() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RBACContract);
    let client = RBACContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let emergency_admin = Address::generate(&env);
    client.initialize(&admin, &emergency_admin);
    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_emergency_admin_address(), emergency_admin);
    assert!(!client.is_paused());
}

#[test]
fn test_initialize_duplicate() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RBACContract);
    let client = RBACContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let emergency_admin = Address::generate(&env);
    client.initialize(&admin, &emergency_admin);
    let result = client.try_initialize(&admin, &emergency_admin);
    assert!(result.is_err());
}

#[test]
fn test_query_before_init() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RBACContract);
    let client = RBACContractClient::new(&env, &contract_id);
    let result = client.try_get_admin();
    assert!(result.is_err());
    let result = client.try_get_emergency_admin_address();
    assert!(result.is_err());
}

// ==================== Role Assignment ====================

#[test]
fn test_assign_role() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role = role_admin();
    client.assign_role(&user, &role);
    assert!(client.has_role(&user, &role));
    let roles = client.get_user_roles(&user);
    assert_eq!(roles.len(), 1);
    assert_eq!(roles.get(0).unwrap(), role);
}

#[test]
fn test_assign_role_duplicate() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role = role_admin();
    client.assign_role(&user, &role);
    let result = client.try_assign_role(&user, &role);
    assert!(result.is_err());
}

#[test]
fn test_assign_role_multiple_roles() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role1 = role_admin();
    let role2 = role_moderator();
    client.assign_role(&user, &role1);
    client.assign_role(&user, &role2);
    let roles = client.get_user_roles(&user);
    assert_eq!(roles.len(), 2);
    assert!(client.has_role(&user, &role1));
    assert!(client.has_role(&user, &role2));
}

#[test]
fn test_assign_role_paused() {
    let (_env, _admin, _emergency_admin, client) = setup();
    client.emergency_pause();
    let user = Address::generate(&_env);
    let role = role_admin();
    let result = client.try_assign_role(&user, &role);
    assert!(result.is_err());
    client.emergency_unpause();
    client.assign_role(&user, &role);
    assert!(client.has_role(&user, &role));
}

// ==================== Role Revocation ====================

#[test]
fn test_revoke_role() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role = role_admin();
    client.assign_role(&user, &role);
    assert!(client.has_role(&user, &role));
    client.revoke_role(&user, &role);
    assert!(!client.has_role(&user, &role));
}

#[test]
fn test_revoke_role_immediate() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role = role_admin();
    client.assign_role(&user, &role);
    assert!(client.has_role(&user, &role));
    client.revoke_role(&user, &role);
    assert!(!client.has_role(&user, &role));
    let roles = client.get_user_roles(&user);
    assert_eq!(roles.len(), 0);
}

#[test]
fn test_revoke_role_not_assigned() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role = role_admin();
    let result = client.try_revoke_role(&user, &role);
    assert!(result.is_err());
}

#[test]
fn test_revoke_role_paused() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role = role_admin();
    client.assign_role(&user, &role);
    client.emergency_pause();
    let result = client.try_revoke_role(&user, &role);
    assert!(result.is_err());
}

#[test]
fn test_revoke_one_role_keeps_others() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role1 = role_admin();
    let role2 = role_moderator();
    client.assign_role(&user, &role1);
    client.assign_role(&user, &role2);
    client.revoke_role(&user, &role1);
    assert!(!client.has_role(&user, &role1));
    assert!(client.has_role(&user, &role2));
    assert_eq!(client.get_user_roles(&user).len(), 1);
}

// ==================== Permission Definition ====================

#[test]
fn test_add_permission_to_role() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let role = role_admin();
    client.add_permission_to_role(&role, &perm_read());
    let perms = client.get_role_permissions(&role);
    assert_eq!(perms.len(), 1);
    assert_eq!(perms.get(0).unwrap(), perm_read());
}

#[test]
fn test_add_duplicate_permission() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let role = role_admin();
    client.add_permission_to_role(&role, &perm_read());
    let result = client.try_add_permission_to_role(&role, &perm_read());
    assert!(result.is_err());
}

#[test]
fn test_remove_permission_from_role() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let role = role_admin();
    client.add_permission_to_role(&role, &perm_read());
    client.add_permission_to_role(&role, &perm_write());
    client.remove_permission_from_role(&role, &perm_read());
    let perms = client.get_role_permissions(&role);
    assert_eq!(perms.len(), 1);
    assert_eq!(perms.get(0).unwrap(), perm_write());
}

#[test]
fn test_remove_non_existent_permission() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let role = role_admin();
    let result = client.try_remove_permission_from_role(&role, &perm_read());
    assert!(result.is_err());
}

// ==================== Access Checks ====================

#[test]
fn test_has_permission_direct() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role = role_admin();
    client.add_permission_to_role(&role, &perm_read());
    client.assign_role(&user, &role);
    assert!(client.has_permission(&user, &perm_read()));
    assert!(!client.has_permission(&user, &perm_write()));
}

#[test]
fn test_has_permission_no_role() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role = role_admin();
    client.add_permission_to_role(&role, &perm_read());
    assert!(!client.has_permission(&user, &perm_read()));
}

#[test]
fn test_has_permission_multiple_roles() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role1 = role_admin();
    let role2 = role_moderator();
    client.add_permission_to_role(&role1, &perm_read());
    client.add_permission_to_role(&role2, &perm_write());
    client.assign_role(&user, &role1);
    client.assign_role(&user, &role2);
    assert!(client.has_permission(&user, &perm_read()));
    assert!(client.has_permission(&user, &perm_write()));
    assert!(!client.has_permission(&user, &perm_delete()));
}

// ==================== Role Hierarchy ====================

#[test]
fn test_set_role_hierarchy() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let parent = role_admin();
    let child = role_user();
    client.set_role_hierarchy(&child, &parent);
    let result = client.get_role_parent(&child);
    assert_eq!(result.unwrap(), parent);
}

#[test]
fn test_permission_inheritance_through_hierarchy() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let parent = role_admin();
    let child = role_user();
    let user = Address::generate(&_env);
    client.add_permission_to_role(&parent, &perm_read());
    client.set_role_hierarchy(&child, &parent);
    client.assign_role(&user, &child);
    assert!(client.has_permission(&user, &perm_read()));
}

#[test]
fn test_permission_inheritance_multi_level() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let grandparent = role_admin();
    let parent = role_moderator();
    let child = role_user();
    let user = Address::generate(&_env);
    client.add_permission_to_role(&grandparent, &perm_read());
    client.add_permission_to_role(&parent, &perm_write());
    client.set_role_hierarchy(&parent, &grandparent);
    client.set_role_hierarchy(&child, &parent);
    client.assign_role(&user, &child);
    assert!(client.has_permission(&user, &perm_read()));
    assert!(client.has_permission(&user, &perm_write()));
    assert!(!client.has_permission(&user, &perm_delete()));
}

#[test]
fn test_hierarchy_cycle_detection_direct() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let role = role_admin();
    let result = client.try_set_role_hierarchy(&role, &role);
    assert!(result.is_err());
}

#[test]
fn test_hierarchy_cycle_detection_indirect() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let role_a = role_admin();
    let role_b = role_moderator();
    let role_c = role_user();
    client.set_role_hierarchy(&role_a, &role_b);
    client.set_role_hierarchy(&role_b, &role_c);
    let result = client.try_set_role_hierarchy(&role_c, &role_a);
    assert!(result.is_err());
}

#[test]
fn test_remove_role_hierarchy() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let parent = role_admin();
    let child = role_user();
    client.set_role_hierarchy(&child, &parent);
    assert!(client.get_role_parent(&child).is_some());
    client.remove_role_hierarchy(&child);
    assert!(client.get_role_parent(&child).is_none());
}

#[test]
fn test_remove_non_existent_hierarchy() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let role = role_admin();
    let result = client.try_remove_role_hierarchy(&role);
    assert!(result.is_err());
}

#[test]
fn test_hierarchy_no_duplicate_parent() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let grandparent = role_admin();
    let parent = role_moderator();
    let child = role_user();
    client.set_role_hierarchy(&child, &parent);
    let result = client.try_set_role_hierarchy(&child, &grandparent);
    assert!(result.is_err());
}

#[test]
fn test_hierarchy_permission_not_inherited_after_remove() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let parent = role_admin();
    let child = role_user();
    let user = Address::generate(&_env);
    client.add_permission_to_role(&parent, &perm_read());
    client.set_role_hierarchy(&child, &parent);
    client.assign_role(&user, &child);
    assert!(client.has_permission(&user, &perm_read()));
    client.remove_role_hierarchy(&child);
    assert!(!client.has_permission(&user, &perm_read()));
}

// ==================== Role Composition ====================

#[test]
fn test_role_composition_multiple_roles() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role1 = role_admin();
    let role2 = role_moderator();
    let role3 = role_user();
    client.add_permission_to_role(&role1, &perm_read());
    client.add_permission_to_role(&role2, &perm_write());
    client.add_permission_to_role(&role3, &perm_delete());
    client.assign_role(&user, &role1);
    client.assign_role(&user, &role2);
    client.assign_role(&user, &role3);
    assert!(client.has_permission(&user, &perm_read()));
    assert!(client.has_permission(&user, &perm_write()));
    assert!(client.has_permission(&user, &perm_delete()));
}

#[test]
fn test_role_composition_inheritance_overlap() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let parent = role_admin();
    let child = role_user();
    client.add_permission_to_role(&parent, &perm_read());
    client.add_permission_to_role(&child, &perm_write());
    client.set_role_hierarchy(&child, &parent);
    client.assign_role(&user, &child);
    assert!(client.has_permission(&user, &perm_read()));
    assert!(client.has_permission(&user, &perm_write()));
}

#[test]
fn test_role_composition_different_users() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user1 = Address::generate(&_env);
    let user2 = Address::generate(&_env);
    let role = role_admin();
    client.add_permission_to_role(&role, &perm_read());
    client.assign_role(&user1, &role);
    assert!(client.has_permission(&user1, &perm_read()));
    assert!(!client.has_permission(&user2, &perm_read()));
}

// ==================== Audit Logging ====================

#[test]
fn test_audit_log_empty_after_init() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let logs = client.get_audit_logs(&0, &10);
    assert_eq!(logs.len(), 0);
}

#[test]
fn test_audit_log_assign_role() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role = role_admin();
    client.assign_role(&user, &role);
    let logs = client.get_audit_logs(&0, &10);
    assert_eq!(logs.len(), 1);
    let entry = logs.get(0).unwrap();
    assert_eq!(entry.action, symbol_short!("asgn_role"));
    assert_eq!(entry.caller, _admin);
    assert_eq!(entry.target.unwrap(), user);
    assert_eq!(entry.role, role);
    assert!(entry.permission.is_none());
}

#[test]
fn test_audit_log_revoke_role() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role = role_admin();
    client.assign_role(&user, &role);
    client.revoke_role(&user, &role);
    let logs = client.get_audit_logs(&0, &10);
    assert_eq!(logs.len(), 2);
    let entry = logs.get(1).unwrap();
    assert_eq!(entry.action, symbol_short!("revk_role"));
}

#[test]
fn test_audit_log_add_permission() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let role = role_admin();
    client.add_permission_to_role(&role, &perm_read());
    let logs = client.get_audit_logs(&0, &10);
    assert_eq!(logs.len(), 1);
    let entry = logs.get(0).unwrap();
    assert_eq!(entry.action, symbol_short!("add_perm"));
    assert_eq!(entry.role, role);
    assert_eq!(entry.permission.unwrap(), perm_read());
}

#[test]
fn test_audit_log_pagination() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role = role_admin();
    client.assign_role(&user, &role);
    client.revoke_role(&user, &role);
    let first = client.get_audit_logs(&0, &2);
    assert_eq!(first.len(), 2);
    let second = client.get_audit_logs(&2, &2);
    assert_eq!(second.len(), 0);
}

#[test]
fn test_audit_log_out_of_range() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let logs = client.get_audit_logs(&100, &10);
    assert_eq!(logs.len(), 0);
}

// ==================== Emergency Admin Controls ====================

#[test]
fn test_emergency_pause() {
    let (_env, _admin, _emergency_admin, client) = setup();
    assert!(!client.is_paused());
    client.emergency_pause();
    assert!(client.is_paused());
}

#[test]
fn test_emergency_unpause() {
    let (_env, _admin, _emergency_admin, client) = setup();
    client.emergency_pause();
    assert!(client.is_paused());
    client.emergency_unpause();
    assert!(!client.is_paused());
}

#[test]
fn test_admin_functions_blocked_when_paused() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role = role_admin();
    client.emergency_pause();
    let result = client.try_assign_role(&user, &role);
    assert!(result.is_err());
}

#[test]
fn test_read_functions_work_when_paused() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role = role_admin();
    client.assign_role(&user, &role);
    client.emergency_pause();
    assert!(client.has_role(&user, &role));
    assert!(client.is_paused());
}

#[test]
fn test_emergency_functions_work_when_paused() {
    let (_env, _admin, _emergency_admin, client) = setup();
    client.emergency_pause();
    assert!(client.is_paused());
    client.emergency_unpause();
    assert!(!client.is_paused());
}

#[test]
fn test_admin_cannot_pause() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RBACContract);
    let client = RBACContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let emergency_admin = Address::generate(&env);
    client.initialize(&admin, &emergency_admin);
    let _result = client.try_emergency_pause();
    client.emergency_unpause();
}

// ==================== Admin Transfer ====================

#[test]
fn test_transfer_admin() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let new_admin = Address::generate(&_env);
    client.transfer_admin(&new_admin);
    assert_eq!(client.get_admin(), new_admin);
}

#[test]
fn test_transfer_emergency_admin() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let new_emergency_admin = Address::generate(&_env);
    client.transfer_emergency_admin(&new_emergency_admin);
    assert_eq!(
        client.get_emergency_admin_address(),
        new_emergency_admin
    );
}

// ==================== Edge Cases ====================

#[test]
fn test_empty_roles_list() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let roles = client.get_user_roles(&user);
    assert_eq!(roles.len(), 0);
}

#[test]
fn test_empty_permissions_list() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let role = role_admin();
    let perms = client.get_role_permissions(&role);
    assert_eq!(perms.len(), 0);
}

#[test]
fn test_multiple_users_independent_roles() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user1 = Address::generate(&_env);
    let user2 = Address::generate(&_env);
    let role1 = role_admin();
    let role2 = role_moderator();
    client.assign_role(&user1, &role1);
    client.assign_role(&user2, &role2);
    assert!(client.has_role(&user1, &role1));
    assert!(!client.has_role(&user1, &role2));
    assert!(client.has_role(&user2, &role2));
    assert!(!client.has_role(&user2, &role1));
}

#[test]
fn test_full_permission_lifecycle() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let role = role_admin();
    client.add_permission_to_role(&role, &perm_read());
    client.add_permission_to_role(&role, &perm_write());
    client.add_permission_to_role(&role, &perm_delete());
    client.assign_role(&user, &role);
    assert!(client.has_permission(&user, &perm_read()));
    assert!(client.has_permission(&user, &perm_write()));
    assert!(client.has_permission(&user, &perm_delete()));
    client.remove_permission_from_role(&role, &perm_write());
    assert!(client.has_permission(&user, &perm_read()));
    assert!(!client.has_permission(&user, &perm_write()));
    assert!(client.has_permission(&user, &perm_delete()));
    client.revoke_role(&user, &role);
    assert!(!client.has_permission(&user, &perm_read()));
    assert!(!client.has_permission(&user, &perm_delete()));
}

#[test]
fn test_symbol_names() {
    let (_env, _admin, _emergency_admin, client) = setup();
    let user = Address::generate(&_env);
    let long_role = Symbol::new(&_env, "super_custom_role");
    let long_perm = Symbol::new(&_env, "super_custom_permission");
    client.assign_role(&user, &long_role);
    client.add_permission_to_role(&long_role, &long_perm);
    assert!(client.has_role(&user, &long_role));
    assert!(client.has_permission(&user, &long_perm));
}
