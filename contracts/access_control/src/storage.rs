use crate::cache::Cached;
use crate::types::{
    AccessError, AccessLogEntry, Capability, DelegatedRole, Policy, RoleGrant,
};
use soroban_sdk::{contracttype, Box, Address, Env, Symbol, Vec};

/// Maximum number of retained access log entries; oldest entries are dropped.
const MAX_LOGS: u32 = 500;

const DEFAULT_TTL: u64 = 60 * 5; // 5 minutes

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Paused,
    NextCapId,
    AccessLogs,
    RoleGrants(Address),
    RolePerms(Symbol),
    AllRoleGrants,
    AllRolePerms,
    Capability(u64),
    HolderCaps(Address),
    DelegatedRoles(Address),
    Policy(Symbol, Symbol),
    Cached(Box<DataKey>),
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

    pub fn get_cached<T: soroban_sdk::TryFrom<soroban_sdk::Val>>(env: &Env, key: &DataKey) -> Option<T> {
        let cached_key = DataKey::Cached(Box::new(key.clone()));
        if let Some(cached) = env.storage().persistent().get::<Cached<T>>(&cached_key) {
            if !cached.is_expired(env) {
                return Some(cached.data);
            }
        }
        None
    }

    pub fn set_cached<T: soroban_sdk::IntoVal<Env, soroban_sdk::Val>>(env: &Env, key: &DataKey, data: &T, ttl: u64) {
        let cached_key = DataKey::Cached(Box::new(key.clone()));
        let expires_at = if ttl > 0 { env.ledger().timestamp() + ttl } else { 0 };
        let cached = Cached::new(data, expires_at);
        env.storage().persistent().set(&cached_key, &cached);
    }

    pub fn get_role_grants(env: &Env, address: &Address) -> Vec<RoleGrant> {
        let key = DataKey::RoleGrants(address.clone());
        if let Some(grants) = Self::get_cached(env, &key) {
            return grants;
        }
        let grants = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        Self::set_cached(env, &key, &grants, DEFAULT_TTL);
        grants
    }

    pub fn set_role_grants(env: &Env, address: &Address, grants: &Vec<RoleGrant>) {
        let key = DataKey::RoleGrants(address.clone());
        env.storage().persistent().set(&key, grants);
        env.storage().persistent().remove(&DataKey::Cached(Box::new(key)));
    }

    pub fn get_role_perms(env: &Env, role: &Symbol) -> Vec<Symbol> {
        let key = DataKey::RolePerms(role.clone());
        if let Some(perms) = Self::get_cached(env, &key) {
            return perms;
        }
        let perms = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        Self::set_cached(env, &key, &perms, DEFAULT_TTL);
        perms
    }

    pub fn set_role_perms(env: &Env, role: &Symbol, perms: &Vec<Symbol>) {
        let key = DataKey::RolePerms(role.clone());
        env.storage().persistent().set(&key, perms);
        env.storage().persistent().remove(&DataKey::Cached(Box::new(key)));
    }

    pub fn add_role_grant_address(env: &Env, address: &Address) {
        let mut addresses = Self::get_all_role_grant_addresses(env);
        if !addresses.contains(address) {
            addresses.push_back(address.clone());
            env.storage().persistent().set(&DataKey::AllRoleGrants, &addresses);
        }
    }

    pub fn get_all_role_grant_addresses(env: &Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::AllRoleGrants)
            .unwrap_or_else(|| Vec::new(env))
    }

    pub fn add_role_perm_role(env: &Env, role: &Symbol) {
        let mut roles = Self::get_all_role_perm_roles(env);
        if !roles.contains(role) {
            roles.push_back(role.clone());
            env.storage().persistent().set(&DataKey::AllRolePerms, &roles);
        }
    }

    pub fn get_all_role_perm_roles(env: &Env) -> Vec<Symbol> {
        env.storage()
            .persistent()
            .get(&DataKey::AllRolePerms)
            .unwrap_or_else(|| Vec::new(env))
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
        let key = DataKey::Capability(id);
        if let Some(cap) = Self::get_cached(env, &key) {
            return Ok(cap);
        }
        let cap = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(AccessError::CapabilityNotFound)?;
        Self::set_cached(env, &key, &cap, DEFAULT_TTL);
        Ok(cap)
    }

    pub fn set_capability(env: &Env, cap: &Capability) {
        let key = DataKey::Capability(cap.id);
        env.storage().persistent().set(&key, cap);
        env.storage().persistent().remove(&DataKey::Cached(Box::new(key)));
    }

    pub fn get_holder_caps(env: &Env, holder: &Address) -> Vec<u64> {
        let key = DataKey::HolderCaps(holder.clone());
        if let Some(ids) = Self::get_cached(env, &key) {
            return ids;
        }
        let ids = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        Self::set_cached(env, &key, &ids, DEFAULT_TTL);
        ids
    }

    pub fn set_holder_caps(env: &Env, holder: &Address, ids: &Vec<u64>) {
        let key = DataKey::HolderCaps(holder.clone());
        env.storage().persistent().set(&key, ids);
        env.storage().persistent().remove(&DataKey::Cached(Box::new(key)));
    }

    pub fn get_delegated_roles(env: &Env, address: &Address) -> Vec<DelegatedRole> {
        let key = DataKey::DelegatedRoles(address.clone());
        if let Some(delegations) = Self::get_cached(env, &key) {
            return delegations;
        }
        let delegations = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        Self::set_cached(env, &key, &delegations, DEFAULT_TTL);
        delegations
    }

    pub fn set_delegated_roles(env: &Env, address: &Address, delegations: &Vec<DelegatedRole>) {
        let key = DataKey::DelegatedRoles(address.clone());
        env.storage().persistent().set(&key, delegations);
        env.storage().persistent().remove(&DataKey::Cached(Box::new(key)));
    }

    pub fn get_policy(env: &Env, resource: &Symbol, action: &Symbol) -> Option<Policy> {
        let key = DataKey::Policy(resource.clone(), action.clone());
        if let Some(policy) = Self::get_cached(env, &key) {
            return Some(policy);
        }
        let policy = env.storage().persistent().get(&key);
        if let Some(p) = &policy {
            Self::set_cached(env, &key, p, DEFAULT_TTL);
        }
        policy
    }

    pub fn set_policy(env: &Env, policy: &Policy) {
        let key = DataKey::Policy(policy.resource.clone(), policy.action.clone());
        env.storage().persistent().set(&key, policy);
        env.storage().persistent().remove(&DataKey::Cached(Box::new(key)));
    }

    pub fn remove_policy(env: &Env, resource: &Symbol, action: &Symbol) {
        let key = DataKey::Policy(resource.clone(), action.clone());
        env.storage().persistent().remove(&key);
        env.storage().persistent().remove(&DataKey::Cached(Box::new(key)));
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