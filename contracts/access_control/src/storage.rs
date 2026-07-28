use crate::types::{
    AccessError, AccessLogEntry, Capability, DelegatedRole, Policy, RoleGrant,
};
use soroban_sdk::{contracttype, Address, Env, Symbol, Vec};

/// Maximum number of retained access log entries; oldest entries are dropped.
const MAX_LOGS: u32 = 500;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Paused,
    NextCapId,
    AccessLogs,
    RoleGrants(Address),
    RolePerms(Symbol),
    Capability(u64),
    HolderCaps(Address),
    DelegatedRoles(Address),
    Policy(Symbol, Symbol),
}

pub struct Storage;

impl Storage {
    pub fn has_admin(env: &Env) -> bool {
        env.storage().instance().has(&DataKey::Admin)
    }

    pub fn set_admin(env: &Env, admin: &Address) {
        env.storage().instance().set(&DataKey::Admin, admin);
    }

    pub fn get_admin(env: &Env) -> Result<Address, AccessError> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(AccessError::NotInitialized)
    }

    pub fn set_paused(env: &Env, paused: bool) {
        env.storage().instance().set(&DataKey::Paused, &paused);
    }

    pub fn get_paused(env: &Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    pub fn get_role_grants(env: &Env, address: &Address) -> Vec<RoleGrant> {
        env.storage()
            .persistent()
            .get(&DataKey::RoleGrants(address.clone()))
            .unwrap_or_else(|| Vec::new(env))
    }

    pub fn set_role_grants(env: &Env, address: &Address, grants: &Vec<RoleGrant>) {
        env.storage()
            .persistent()
            .set(&DataKey::RoleGrants(address.clone()), grants);
    }

    pub fn get_role_perms(env: &Env, role: &Symbol) -> Vec<Symbol> {
        env.storage()
            .persistent()
            .get(&DataKey::RolePerms(role.clone()))
            .unwrap_or_else(|| Vec::new(env))
    }

    pub fn set_role_perms(env: &Env, role: &Symbol, perms: &Vec<Symbol>) {
        env.storage()
            .persistent()
            .set(&DataKey::RolePerms(role.clone()), perms);
    }

    pub fn next_cap_id(env: &Env) -> u64 {
        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextCapId)
            .unwrap_or(1);
        env.storage().instance().set(&DataKey::NextCapId, &(id + 1));
        id
    }

    pub fn get_capability(env: &Env, id: u64) -> Result<Capability, AccessError> {
        env.storage()
            .persistent()
            .get(&DataKey::Capability(id))
            .ok_or(AccessError::CapabilityNotFound)
    }

    pub fn set_capability(env: &Env, cap: &Capability) {
        env.storage()
            .persistent()
            .set(&DataKey::Capability(cap.id), cap);
    }

    pub fn get_holder_caps(env: &Env, holder: &Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::HolderCaps(holder.clone()))
            .unwrap_or_else(|| Vec::new(env))
    }

    pub fn set_holder_caps(env: &Env, holder: &Address, ids: &Vec<u64>) {
        env.storage()
            .persistent()
            .set(&DataKey::HolderCaps(holder.clone()), ids);
    }

    pub fn get_delegated_roles(env: &Env, address: &Address) -> Vec<DelegatedRole> {
        env.storage()
            .persistent()
            .get(&DataKey::DelegatedRoles(address.clone()))
            .unwrap_or_else(|| Vec::new(env))
    }

    pub fn set_delegated_roles(env: &Env, address: &Address, delegations: &Vec<DelegatedRole>) {
        env.storage()
            .persistent()
            .set(&DataKey::DelegatedRoles(address.clone()), delegations);
    }

    pub fn get_policy(env: &Env, resource: &Symbol, action: &Symbol) -> Option<Policy> {
        env.storage()
            .persistent()
            .get(&DataKey::Policy(resource.clone(), action.clone()))
    }

    pub fn set_policy(env: &Env, policy: &Policy) {
        env.storage().persistent().set(
            &DataKey::Policy(policy.resource.clone(), policy.action.clone()),
            policy,
        );
    }

    pub fn remove_policy(env: &Env, resource: &Symbol, action: &Symbol) {
        env.storage()
            .persistent()
            .remove(&DataKey::Policy(resource.clone(), action.clone()));
    }

    pub fn get_logs(env: &Env) -> Vec<AccessLogEntry> {
        env.storage()
            .persistent()
            .get(&DataKey::AccessLogs)
            .unwrap_or_else(|| Vec::new(env))
    }

    pub fn add_log(env: &Env, actor: &Address, action: Symbol, resource: Symbol, allowed: bool) {
        let mut logs = Self::get_logs(env);
        logs.push_back(AccessLogEntry {
            timestamp: env.ledger().timestamp(),
            actor: actor.clone(),
            action,
            resource,
            allowed,
        });
        while logs.len() > MAX_LOGS {
            logs.remove(0);
        }
        env.storage().persistent().set(&DataKey::AccessLogs, &logs);
    }
}
