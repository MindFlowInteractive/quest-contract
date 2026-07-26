//! Advanced access control: roles with time-bound grants, permission checks,
//! capability tokens, delegation (roles and capabilities), revocation,
//! access logging, and a deny-overrides policy engine.
#![no_std]

mod cache;
mod storage;
mod types;

use storage::Storage;
use types::{
    AccessError, AccessLogEntry, Capability, DelegatedRole, Policy, PolicyEffect, PolicyRule,
    RoleGrant,
};
use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Symbol, Vec};

#[contract]
pub struct AccessControlContract;

#[contractimpl]
impl AccessControlContract {
    // ---------------------------------------------------------------- admin

    pub fn initialize(env: Env, admin: Address) -> Result<(), AccessError> {
        if Storage::has_admin(&env) {
            return Err(AccessError::AlreadyInitialized);
        }
        admin.require_auth();
        Storage::set_admin(&env, &admin);
        Storage::set_paused(&env, false);
        env.events().publish((symbol_short!("init"),), (admin,));
        Ok(())
    }

    pub fn pause(env: Env) -> Result<(), AccessError> {
        let admin = require_admin(&env)?;
        Storage::set_paused(&env, true);
        env.events().publish((symbol_short!("pause"),), (admin,));
        Ok(())
    }

    pub fn unpause(env: Env) -> Result<(), AccessError> {
        let admin = require_admin(&env)?;
        Storage::set_paused(&env, false);
        env.events().publish((symbol_short!("unpause"),), (admin,));
        Ok(())
    }

    pub fn transfer_admin(env: Env, new_admin: Address) -> Result<(), AccessError> {
        let old_admin = require_admin(&env)?;
        Storage::set_admin(&env, &new_admin);
        env.events()
            .publish((symbol_short!("xfer_adm"),), (old_admin, new_admin));
        Ok(())
    }

    pub fn get_admin(env: Env) -> Result<Address, AccessError> {
        Storage::get_admin(&env)
    }

    pub fn is_paused(env: Env) -> bool {
        Storage::get_paused(&env)
    }

    // ---------------------------------------------------------------- roles

    /// Grant `role` to `address` until `expires_at` (0 = no expiry).
    /// Re-granting an existing role updates its expiry.
    pub fn grant_role(
        env: Env,
        address: Address,
        role: Symbol,
        expires_at: u64,
    ) -> Result<(), AccessError> {
        let admin = require_admin(&env)?;
        require_not_paused(&env)?;
        require_future_or_zero(&env, expires_at)?;
        let mut grants = Storage::get_role_grants(&env, &address);
        if let Some(i) = grant_index(&grants, &role) {
            grants.remove(i);
        }
        grants.push_back(RoleGrant {
            role: role.clone(),
            expires_at,
        });
        Storage::set_role_grants(&env, &address, &grants);
        Storage::add_log(&env, &admin, symbol_short!("grant"), role.clone(), true);
        env.events()
            .publish((symbol_short!("grant"),), (admin, address, role, expires_at));
        Ok(())
    }

    pub fn revoke_role(env: Env, address: Address, role: Symbol) -> Result<(), AccessError> {
        let admin = require_admin(&env)?;
        require_not_paused(&env)?;
        let mut grants = Storage::get_role_grants(&env, &address);
        let index = grant_index(&grants, &role).ok_or(AccessError::RoleNotGranted)?;
        grants.remove(index);
        Storage::set_role_grants(&env, &address, &grants);
        Storage::add_log(&env, &admin, symbol_short!("revoke"), role.clone(), true);
        env.events()
            .publish((symbol_short!("revoke"),), (admin, address, role));
        Ok(())
    }

    pub fn add_permission(env: Env, role: Symbol, permission: Symbol) -> Result<(), AccessError> {
        let admin = require_admin(&env)?;
        require_not_paused(&env)?;
        let mut perms = Storage::get_role_perms(&env, &role);
        if vec_contains(&perms, &permission) {
            return Err(AccessError::PermissionAlreadyExists);
        }
        perms.push_back(permission.clone());
        Storage::set_role_perms(&env, &role, &perms);
        env.events()
            .publish((symbol_short!("add_perm"),), (admin, role, permission));
        Ok(())
    }

    pub fn remove_permission(
        env: Env,
        role: Symbol,
        permission: Symbol,
    ) -> Result<(), AccessError> {
        let admin = require_admin(&env)?;
        require_not_paused(&env)?;
        let mut perms = Storage::get_role_perms(&env, &role);
        let index = vec_find_index(&perms, &permission).ok_or(AccessError::PermissionNotFound)?;
        perms.remove(index);
        Storage::set_role_perms(&env, &role, &perms);
        env.events()
            .publish((symbol_short!("rm_perm"),), (admin, role, permission));
        Ok(())
    }

    /// True if `address` holds `role` right now, directly or via a valid
    /// delegation. Expired grants and delegations are ignored.
    pub fn has_role(env: Env, address: Address, role: Symbol) -> bool {
        vec_contains(&active_roles(&env, &address), &role)
    }

    /// True if any of the caller's active roles carries `permission`.
    pub fn has_permission(env: Env, address: Address, permission: Symbol) -> bool {
        for role in active_roles(&env, &address).iter() {
            if vec_contains(&Storage::get_role_perms(&env, &role), &permission) {
                return true;
            }
        }
        false
    }

    pub fn get_role_grants(env: Env, address: Address) -> Vec<RoleGrant> {
        Storage::get_role_grants(&env, &address)
    }

    pub fn get_role_permissions(env: Env, role: Symbol) -> Vec<Symbol> {
        Storage::get_role_perms(&env, &role)
    }

    // ------------------------------------------------------ role delegation

    /// Delegate a directly-held role to `delegate` until `expires_at`
    /// (0 = no expiry, capped implicitly by the delegator's own grant).
    pub fn delegate_role(
        env: Env,
        delegator: Address,
        delegate: Address,
        role: Symbol,
        expires_at: u64,
    ) -> Result<(), AccessError> {
        delegator.require_auth();
        require_not_paused(&env)?;
        require_future_or_zero(&env, expires_at)?;
        if delegator == delegate {
            return Err(AccessError::SelfDelegation);
        }
        if !has_active_direct_grant(&env, &delegator, &role) {
            return Err(AccessError::RoleNotGranted);
        }
        let mut delegations = Storage::get_delegated_roles(&env, &delegate);
        for d in delegations.iter() {
            if d.role == role && d.delegator == delegator {
                return Err(AccessError::DelegationAlreadyExists);
            }
        }
        delegations.push_back(DelegatedRole {
            role: role.clone(),
            delegator: delegator.clone(),
            expires_at,
        });
        Storage::set_delegated_roles(&env, &delegate, &delegations);
        Storage::add_log(&env, &delegator, symbol_short!("dlg_role"), role.clone(), true);
        env.events()
            .publish((symbol_short!("dlg_role"),), (delegator, delegate, role, expires_at));
        Ok(())
    }

    /// Revoke a role delegation. Callable by the original delegator or admin.
    pub fn revoke_role_delegation(
        env: Env,
        caller: Address,
        delegate: Address,
        role: Symbol,
        delegator: Address,
    ) -> Result<(), AccessError> {
        caller.require_auth();
        let admin = Storage::get_admin(&env)?;
        if caller != delegator && caller != admin {
            return Err(AccessError::NotAuthorized);
        }
        let mut delegations = Storage::get_delegated_roles(&env, &delegate);
        let mut index: Option<u32> = None;
        for (i, d) in delegations.iter().enumerate() {
            if d.role == role && d.delegator == delegator {
                index = Some(i as u32);
                break;
            }
        }
        let index = index.ok_or(AccessError::DelegationNotFound)?;
        delegations.remove(index);
        Storage::set_delegated_roles(&env, &delegate, &delegations);
        Storage::add_log(&env, &caller, symbol_short!("rvk_dlg"), role.clone(), true);
        env.events()
            .publish((symbol_short!("rvk_dlg"),), (caller, delegate, role));
        Ok(())
    }

    pub fn get_delegated_roles(env: Env, address: Address) -> Vec<DelegatedRole> {
        Storage::get_delegated_roles(&env, &address)
    }

    // --------------------------------------------------------- capabilities

    /// Issue a capability token granting `holder` the right to perform
    /// `action` on `resource`. `depth` bounds further delegation hops.
    pub fn issue_capability(
        env: Env,
        holder: Address,
        resource: Symbol,
        action: Symbol,
        expires_at: u64,
        max_uses: u32,
        delegatable: bool,
        depth: u32,
    ) -> Result<u64, AccessError> {
        let admin = require_admin(&env)?;
        require_not_paused(&env)?;
        require_future_or_zero(&env, expires_at)?;
        let id = Storage::next_cap_id(&env);
        let cap = Capability {
            id,
            holder: holder.clone(),
            issuer: admin.clone(),
            resource: resource.clone(),
            action: action.clone(),
            expires_at,
            max_uses,
            uses: 0,
            delegatable,
            depth,
            parent: None,
            revoked: false,
        };
        Storage::set_capability(&env, &cap);
        let mut ids = Storage::get_holder_caps(&env, &holder);
        ids.push_back(id);
        Storage::set_holder_caps(&env, &holder, &ids);
        Storage::add_log(&env, &admin, symbol_short!("cap_issue"), resource.clone(), true);
        env.events()
            .publish((symbol_short!("cap_issue"),), (id, holder, resource, action));
        Ok(id)
    }

    /// Exercise a capability: validates it (including its delegation chain),
    /// consumes one use, and records the access.
    pub fn use_capability(env: Env, cap_id: u64) -> Result<(), AccessError> {
        require_not_paused(&env)?;
        let mut cap = Storage::get_capability(&env, cap_id)?;
        cap.holder.require_auth();
        let check = validate_capability(&env, &cap);
        Storage::add_log(
            &env,
            &cap.holder,
            cap.action.clone(),
            cap.resource.clone(),
            check.is_ok(),
        );
        check?;
        cap.uses += 1;
        Storage::set_capability(&env, &cap);
        env.events()
            .publish((symbol_short!("cap_use"),), (cap_id, cap.holder));
        Ok(())
    }

    /// Delegate a capability to `to`, producing a child capability whose
    /// lifetime is bounded by the parent's expiry and delegation depth.
    pub fn delegate_capability(
        env: Env,
        cap_id: u64,
        to: Address,
        expires_at: u64,
    ) -> Result<u64, AccessError> {
        require_not_paused(&env)?;
        require_future_or_zero(&env, expires_at)?;
        let parent = Storage::get_capability(&env, cap_id)?;
        parent.holder.require_auth();
        validate_capability(&env, &parent)?;
        if !parent.delegatable {
            return Err(AccessError::NotDelegatable);
        }
        if parent.depth == 0 {
            return Err(AccessError::DelegationDepthExceeded);
        }
        if to == parent.holder {
            return Err(AccessError::SelfDelegation);
        }
        // Child expiry may not outlive a bounded parent.
        if parent.expires_at != 0 && (expires_at == 0 || expires_at > parent.expires_at) {
            return Err(AccessError::InvalidExpiry);
        }
        let id = Storage::next_cap_id(&env);
        let child = Capability {
            id,
            holder: to.clone(),
            issuer: parent.holder.clone(),
            resource: parent.resource.clone(),
            action: parent.action.clone(),
            expires_at,
            max_uses: parent.max_uses,
            uses: 0,
            delegatable: parent.delegatable,
            depth: parent.depth - 1,
            parent: Some(cap_id),
            revoked: false,
        };
        Storage::set_capability(&env, &child);
        let mut ids = Storage::get_holder_caps(&env, &to);
        ids.push_back(id);
        Storage::set_holder_caps(&env, &to, &ids);
        Storage::add_log(
            &env,
            &parent.holder,
            symbol_short!("cap_dlg"),
            parent.resource.clone(),
            true,
        );
        env.events()
            .publish((symbol_short!("cap_dlg"),), (cap_id, id, to));
        Ok(id)
    }

    /// Revoke a capability. Callable by its issuer or the admin. Child
    /// capabilities delegated from it become invalid transitively.
    pub fn revoke_capability(env: Env, caller: Address, cap_id: u64) -> Result<(), AccessError> {
        caller.require_auth();
        let mut cap = Storage::get_capability(&env, cap_id)?;
        let admin = Storage::get_admin(&env)?;
        if caller != cap.issuer && caller != admin {
            return Err(AccessError::NotAuthorized);
        }
        if cap.revoked {
            return Err(AccessError::CapabilityRevoked);
        }
        cap.revoked = true;
        Storage::set_capability(&env, &cap);
        Storage::add_log(&env, &caller, symbol_short!("cap_rvk"), cap.resource.clone(), true);
        env.events()
            .publish((symbol_short!("cap_rvk"),), (caller, cap_id));
        Ok(())
    }

    pub fn get_capability(env: Env, cap_id: u64) -> Result<Capability, AccessError> {
        Storage::get_capability(&env, cap_id)
    }

    pub fn is_capability_valid(env: Env, cap_id: u64) -> bool {
        match Storage::get_capability(&env, cap_id) {
            Ok(cap) => validate_capability(&env, &cap).is_ok(),
            Err(_) => false,
        }
    }

    pub fn get_holder_capabilities(env: Env, holder: Address) -> Vec<u64> {
        Storage::get_holder_caps(&env, &holder)
    }

    // -------------------------------------------------------- policy engine

    /// Install (or replace) the policy for a (resource, action) pair.
    pub fn set_policy(
        env: Env,
        resource: Symbol,
        action: Symbol,
        rules: Vec<PolicyRule>,
        default_allow: bool,
    ) -> Result<(), AccessError> {
        let admin = require_admin(&env)?;
        require_not_paused(&env)?;
        let policy = Policy {
            resource: resource.clone(),
            action: action.clone(),
            rules,
            default_allow,
        };
        Storage::set_policy(&env, &policy);
        env.events()
            .publish((symbol_short!("set_pol"),), (admin, resource, action));
        Ok(())
    }

    pub fn remove_policy(env: Env, resource: Symbol, action: Symbol) -> Result<(), AccessError> {
        let admin = require_admin(&env)?;
        require_not_paused(&env)?;
        if Storage::get_policy(&env, &resource, &action).is_none() {
            return Err(AccessError::PolicyNotFound);
        }
        Storage::remove_policy(&env, &resource, &action);
        env.events()
            .publish((symbol_short!("rm_pol"),), (admin, resource, action));
        Ok(())
    }

    pub fn get_policy(env: Env, resource: Symbol, action: Symbol) -> Result<Policy, AccessError> {
        Storage::get_policy(&env, &resource, &action).ok_or(AccessError::PolicyNotFound)
    }

    /// Full access decision for `address` on (resource, action), recorded in
    /// the access log. Order: valid capability > policy > permission fallback.
    pub fn check_access(env: Env, address: Address, resource: Symbol, action: Symbol) -> bool {
        let allowed = decide_access(&env, &address, &resource, &action);
        Storage::add_log(&env, &address, action, resource, allowed);
        allowed
    }

    /// Same decision as `check_access` but read-only (no log entry).
    pub fn can_access(env: Env, address: Address, resource: Symbol, action: Symbol) -> bool {
        decide_access(&env, &address, &resource, &action)
    }

    // ----------------------------------------------------------------- logs

    pub fn get_access_logs(env: Env, from: u32, max: u32) -> Vec<AccessLogEntry> {
        let all = Storage::get_logs(&env);
        let total = all.len();
        let mut result = Vec::new(&env);
        let start = from.min(total);
        let end = (from + max).min(total);
        let mut i = start;
        while i < end {
            if let Some(entry) = all.get(i) {
                result.push_back(entry);
            }
            i += 1;
        }
        result
    }

    pub fn get_access_log_count(env: Env) -> u32 {
        Storage::get_logs(&env).len()
    }
}

// ------------------------------------------------------------------ helpers

fn require_admin(env: &Env) -> Result<Address, AccessError> {
    let admin = Storage::get_admin(env)?;
    admin.require_auth();
    Ok(admin)
}

fn require_not_paused(env: &Env) -> Result<(), AccessError> {
    if Storage::get_paused(env) {
        return Err(AccessError::ContractPaused);
    }
    Ok(())
}

fn require_future_or_zero(env: &Env, expires_at: u64) -> Result<(), AccessError> {
    if expires_at != 0 && expires_at <= env.ledger().timestamp() {
        return Err(AccessError::InvalidExpiry);
    }
    Ok(())
}

fn is_active(expires_at: u64, now: u64) -> bool {
    expires_at == 0 || now < expires_at
}

fn vec_contains(v: &Vec<Symbol>, item: &Symbol) -> bool {
    for elem in v.iter() {
        if elem == *item {
            return true;
        }
    }
    false
}

fn vec_find_index(v: &Vec<Symbol>, item: &Symbol) -> Option<u32> {
    for (i, elem) in v.iter().enumerate() {
        if elem == *item {
            return Some(i as u32);
        }
    }
    None
}

fn grant_index(grants: &Vec<RoleGrant>, role: &Symbol) -> Option<u32> {
    for (i, g) in grants.iter().enumerate() {
        if g.role == *role {
            return Some(i as u32);
        }
    }
    None
}

fn has_active_direct_grant(env: &Env, address: &Address, role: &Symbol) -> bool {
    let now = env.ledger().timestamp();
    for g in Storage::get_role_grants(env, address).iter() {
        if g.role == *role && is_active(g.expires_at, now) {
            return true;
        }
    }
    false
}

/// All roles `address` holds right now: active direct grants plus delegations
/// whose delegator still holds an active direct grant of the role.
fn active_roles(env: &Env, address: &Address) -> Vec<Symbol> {
    let now = env.ledger().timestamp();
    let mut roles = Vec::new(env);
    for g in Storage::get_role_grants(env, address).iter() {
        if is_active(g.expires_at, now) && !vec_contains(&roles, &g.role) {
            roles.push_back(g.role);
        }
    }
    for d in Storage::get_delegated_roles(env, address).iter() {
        if is_active(d.expires_at, now)
            && has_active_direct_grant(env, &d.delegator, &d.role)
            && !vec_contains(&roles, &d.role)
        {
            roles.push_back(d.role);
        }
    }
    roles
}

/// Validate a capability and its whole delegation chain: not revoked, not
/// expired, not exhausted, and every ancestor still alive.
fn validate_capability(env: &Env, cap: &Capability) -> Result<(), AccessError> {
    let now = env.ledger().timestamp();
    if cap.revoked {
        return Err(AccessError::CapabilityRevoked);
    }
    if !is_active(cap.expires_at, now) {
        return Err(AccessError::CapabilityExpired);
    }
    if cap.max_uses != 0 && cap.uses >= cap.max_uses {
        return Err(AccessError::CapabilityExhausted);
    }
    let mut parent_id = cap.parent;
    while let Some(id) = parent_id {
        let parent = Storage::get_capability(env, id)?;
        if parent.revoked {
            return Err(AccessError::CapabilityRevoked);
        }
        if !is_active(parent.expires_at, now) {
            return Err(AccessError::CapabilityExpired);
        }
        parent_id = parent.parent;
    }
    Ok(())
}

fn holds_valid_capability(env: &Env, address: &Address, resource: &Symbol, action: &Symbol) -> bool {
    for id in Storage::get_holder_caps(env, address).iter() {
        if let Ok(cap) = Storage::get_capability(env, id) {
            if cap.resource == *resource
                && cap.action == *action
                && validate_capability(env, &cap).is_ok()
            {
                return true;
            }
        }
    }
    false
}

fn decide_access(env: &Env, address: &Address, resource: &Symbol, action: &Symbol) -> bool {
    if holds_valid_capability(env, address, resource, action) {
        return true;
    }
    if let Some(policy) = Storage::get_policy(env, resource, action) {
        return evaluate_policy(env, address, &policy);
    }
    // No policy installed: fall back to permission semantics, treating the
    // action name as the required permission.
    for role in active_roles(env, address).iter() {
        if vec_contains(&Storage::get_role_perms(env, &role), action) {
            return true;
        }
    }
    false
}

/// Deny-overrides evaluation: any matching Deny rule wins, then any matching
/// Allow rule, otherwise the policy default.
fn evaluate_policy(env: &Env, address: &Address, policy: &Policy) -> bool {
    let roles = active_roles(env, address);
    let mut allowed = false;
    for rule in policy.rules.iter() {
        if vec_contains(&roles, &rule.role) {
            match rule.effect {
                PolicyEffect::Deny => return false,
                PolicyEffect::Allow => allowed = true,
            }
        }
    }
    if allowed {
        return true;
    }
    policy.default_allow
}

#[cfg(test)]
mod test;