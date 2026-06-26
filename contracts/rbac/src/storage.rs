use crate::types::{AuditEntry, RBACError};
use soroban_sdk::{symbol_short, Address, Env, Symbol, Vec};

pub struct Storage;

impl Storage {
    pub fn has_admin(env: &Env) -> bool {
        env.storage().instance().has(&symbol_short!("admin"))
    }

    pub fn set_admin(env: &Env, admin: &Address) {
        env.storage().instance().set(&symbol_short!("admin"), admin);
    }

    pub fn get_admin(env: &Env) -> Result<Address, RBACError> {
        env.storage()
            .instance()
            .get(&symbol_short!("admin"))
            .ok_or(RBACError::NotInitialized)
    }

    pub fn set_emergency_admin(env: &Env, admin: &Address) {
        env.storage()
            .instance()
            .set(&symbol_short!("em_admin"), admin);
    }

    pub fn get_emergency_admin(env: &Env) -> Result<Address, RBACError> {
        env.storage()
            .instance()
            .get(&symbol_short!("em_admin"))
            .ok_or(RBACError::NotInitialized)
    }

    pub fn set_paused(env: &Env, paused: bool) {
        env.storage().instance().set(&symbol_short!("paused"), &paused);
    }

    pub fn get_paused(env: &Env) -> bool {
        env.storage()
            .instance()
            .get(&symbol_short!("paused"))
            .unwrap_or(false)
    }

    pub fn set_user_roles(env: &Env, address: &Address, roles: &Vec<Symbol>) {
        let key = (symbol_short!("roles"), address.clone());
        env.storage().persistent().set(&key, roles);
    }

    pub fn get_user_roles(env: &Env, address: &Address) -> Vec<Symbol> {
        let key = (symbol_short!("roles"), address.clone());
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env))
    }

    pub fn set_role_permissions(env: &Env, role: &Symbol, permissions: &Vec<Symbol>) {
        let key = (symbol_short!("perms"), role.clone());
        env.storage().persistent().set(&key, permissions);
    }

    pub fn get_role_permissions(env: &Env, role: &Symbol) -> Vec<Symbol> {
        let key = (symbol_short!("perms"), role.clone());
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env))
    }

    pub fn set_role_parent(env: &Env, child: &Symbol, parent: &Symbol) {
        let key = (symbol_short!("parent"), child.clone());
        env.storage().persistent().set(&key, parent);
    }

    pub fn get_role_parent(env: &Env, child: &Symbol) -> Option<Symbol> {
        let key = (symbol_short!("parent"), child.clone());
        env.storage().persistent().get(&key)
    }

    pub fn remove_role_parent(env: &Env, child: &Symbol) {
        let key = (symbol_short!("parent"), child.clone());
        env.storage().persistent().remove(&key);
    }

    pub fn get_audit_logs(env: &Env) -> Vec<AuditEntry> {
        env.storage()
            .instance()
            .get(&symbol_short!("audits"))
            .unwrap_or_else(|| Vec::new(env))
    }

    pub fn set_audit_logs(env: &Env, logs: &Vec<AuditEntry>) {
        env.storage()
            .instance()
            .set(&symbol_short!("audits"), logs);
    }

    pub fn add_audit_entry(
        env: &Env,
        action: Symbol,
        caller: &Address,
        target: Option<Address>,
        role: Symbol,
        permission: Option<Symbol>,
    ) {
        let mut logs = Self::get_audit_logs(env);
        let entry = AuditEntry {
            timestamp: env.ledger().timestamp(),
            action,
            caller: caller.clone(),
            target,
            role,
            permission,
        };
        logs.push_back(entry);
        Self::set_audit_logs(env, &logs);
    }
}
