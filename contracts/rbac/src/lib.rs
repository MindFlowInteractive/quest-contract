#![no_std]

mod storage;
mod types;

use storage::Storage;
use types::{AuditEntry, RBACError};
use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Symbol, Vec};

#[contract]
pub struct RBACContract;

#[contractimpl]
impl RBACContract {
    pub fn initialize(env: Env, admin: Address, emergency_admin: Address) -> Result<(), RBACError> {
        if Storage::has_admin(&env) {
            return Err(RBACError::AlreadyInitialized);
        }
        admin.require_auth();
        Storage::set_admin(&env, &admin);
        Storage::set_emergency_admin(&env, &emergency_admin);
        Storage::set_paused(&env, false);
        env.events()
            .publish((symbol_short!("init"),), (admin, emergency_admin));
        Ok(())
    }

    pub fn assign_role(
        env: Env,
        address: Address,
        role: Symbol,
    ) -> Result<(), RBACError> {
        let admin = require_admin(&env)?;
        require_not_paused(&env)?;
        let mut roles = Storage::get_user_roles(&env, &address);
        if vec_contains(&roles, &role) {
            return Err(RBACError::RoleAlreadyAssigned);
        }
        roles.push_back(role.clone());
        Storage::set_user_roles(&env, &address, &roles);
        Storage::add_audit_entry(
            &env,
            symbol_short!("asgn_role"),
            &admin,
            Some(address.clone()),
            role.clone(),
            None,
        );
        env.events()
            .publish((symbol_short!("asgn_role"),), (admin, address, role));
        Ok(())
    }

    pub fn revoke_role(
        env: Env,
        address: Address,
        role: Symbol,
    ) -> Result<(), RBACError> {
        let admin = require_admin(&env)?;
        require_not_paused(&env)?;
        let mut roles = Storage::get_user_roles(&env, &address);
        let index = vec_find_index(&roles, &role);
        if index.is_none() {
            return Err(RBACError::RoleNotAssigned);
        }
        roles.remove(index.unwrap() as u32);
        Storage::set_user_roles(&env, &address, &roles);
        Storage::add_audit_entry(
            &env,
            symbol_short!("revk_role"),
            &admin,
            Some(address.clone()),
            role.clone(),
            None,
        );
        env.events()
            .publish((symbol_short!("revk_role"),), (admin, address, role));
        Ok(())
    }

    pub fn add_permission_to_role(
        env: Env,
        role: Symbol,
        permission: Symbol,
    ) -> Result<(), RBACError> {
        let admin = require_admin(&env)?;
        require_not_paused(&env)?;
        let mut permissions = Storage::get_role_permissions(&env, &role);
        if vec_contains(&permissions, &permission) {
            return Err(RBACError::PermissionAlreadyExists);
        }
        permissions.push_back(permission.clone());
        Storage::set_role_permissions(&env, &role, &permissions);
        Storage::add_audit_entry(
            &env,
            symbol_short!("add_perm"),
            &admin,
            None,
            role.clone(),
            Some(permission.clone()),
        );
        env.events()
            .publish((symbol_short!("add_perm"),), (admin, role, permission));
        Ok(())
    }

    pub fn remove_permission_from_role(
        env: Env,
        role: Symbol,
        permission: Symbol,
    ) -> Result<(), RBACError> {
        let admin = require_admin(&env)?;
        require_not_paused(&env)?;
        let mut permissions = Storage::get_role_permissions(&env, &role);
        let index = vec_find_index(&permissions, &permission);
        if index.is_none() {
            return Err(RBACError::PermissionNotFound);
        }
        permissions.remove(index.unwrap() as u32);
        Storage::set_role_permissions(&env, &role, &permissions);
        Storage::add_audit_entry(
            &env,
            symbol_short!("rm_perm"),
            &admin,
            None,
            role.clone(),
            Some(permission.clone()),
        );
        env.events()
            .publish((symbol_short!("rm_perm"),), (admin, role, permission));
        Ok(())
    }

    pub fn set_role_hierarchy(
        env: Env,
        child_role: Symbol,
        parent_role: Symbol,
    ) -> Result<(), RBACError> {
        let admin = require_admin(&env)?;
        require_not_paused(&env)?;
        if Storage::get_role_parent(&env, &child_role).is_some() {
            return Err(RBACError::RoleAlreadyHasParent);
        }
        if would_create_cycle(&env, &child_role, &parent_role) {
            return Err(RBACError::HierarchyCycle);
        }
        Storage::set_role_parent(&env, &child_role, &parent_role);
        Storage::add_audit_entry(
            &env,
            symbol_short!("set_hier"),
            &admin,
            None,
            child_role.clone(),
            Some(parent_role.clone()),
        );
        env.events()
            .publish((symbol_short!("set_hier"),), (admin, child_role, parent_role));
        Ok(())
    }

    pub fn remove_role_hierarchy(env: Env, child_role: Symbol) -> Result<(), RBACError> {
        let admin = require_admin(&env)?;
        require_not_paused(&env)?;
        if Storage::get_role_parent(&env, &child_role).is_none() {
            return Err(RBACError::RoleHasNoParent);
        }
        let parent = Storage::get_role_parent(&env, &child_role).unwrap();
        Storage::remove_role_parent(&env, &child_role);
        Storage::add_audit_entry(
            &env,
            symbol_short!("rm_hier"),
            &admin,
            None,
            child_role.clone(),
            Some(parent),
        );
        env.events()
            .publish((symbol_short!("rm_hier"),), (admin, child_role));
        Ok(())
    }

    pub fn has_role(env: Env, address: Address, role: Symbol) -> bool {
        let roles = Storage::get_user_roles(&env, &address);
        vec_contains(&roles, &role)
    }

    pub fn has_permission(env: Env, address: Address, permission: Symbol) -> bool {
        let roles = Storage::get_user_roles(&env, &address);
        for role in roles.iter() {
            let resolved = resolve_role_permissions(&env, &role);
            if vec_contains(&resolved, &permission) {
                return true;
            }
        }
        false
    }

    pub fn get_user_roles(env: Env, address: Address) -> Vec<Symbol> {
        Storage::get_user_roles(&env, &address)
    }

    pub fn get_role_permissions(env: Env, role: Symbol) -> Vec<Symbol> {
        resolve_role_permissions(&env, &role)
    }

    pub fn get_role_parent(env: Env, role: Symbol) -> Option<Symbol> {
        Storage::get_role_parent(&env, &role)
    }

    pub fn get_audit_logs(env: Env, from: u32, max: u32) -> Vec<AuditEntry> {
        let all_logs = Storage::get_audit_logs(&env);
        let total = all_logs.len();
        let mut result = Vec::new(&env);
        let start = from.min(total);
        let end = (from + max).min(total);
        let mut i = start;
        while i < end {
            if let Some(entry) = all_logs.get(i) {
                result.push_back(entry);
            }
            i += 1;
        }
        result
    }

    pub fn emergency_pause(env: Env) -> Result<(), RBACError> {
        let emergency_admin = Storage::get_emergency_admin(&env)?;
        emergency_admin.require_auth();
        Storage::set_paused(&env, true);
        Storage::add_audit_entry(
            &env,
            symbol_short!("pause"),
            &emergency_admin,
            None,
            Symbol::new(&env, ""),
            None,
        );
        env.events()
            .publish((symbol_short!("pause"),), (emergency_admin,));
        Ok(())
    }

    pub fn emergency_unpause(env: Env) -> Result<(), RBACError> {
        let emergency_admin = Storage::get_emergency_admin(&env)?;
        emergency_admin.require_auth();
        Storage::set_paused(&env, false);
        Storage::add_audit_entry(
            &env,
            symbol_short!("unpause"),
            &emergency_admin,
            None,
            Symbol::new(&env, ""),
            None,
        );
        env.events()
            .publish((symbol_short!("unpause"),), (emergency_admin,));
        Ok(())
    }

    pub fn transfer_admin(env: Env, new_admin: Address) -> Result<(), RBACError> {
        let old_admin = require_admin(&env)?;
        require_not_paused(&env)?;
        Storage::set_admin(&env, &new_admin);
        Storage::add_audit_entry(
            &env,
            symbol_short!("xfer_adm"),
            &old_admin,
            Some(new_admin.clone()),
            Symbol::new(&env, ""),
            None,
        );
        env.events()
            .publish((symbol_short!("xfer_adm"),), (old_admin, new_admin));
        Ok(())
    }

    pub fn transfer_emergency_admin(env: Env, new_emergency_admin: Address) -> Result<(), RBACError> {
        let old_emergency_admin = Storage::get_emergency_admin(&env)?;
        old_emergency_admin.require_auth();
        Storage::set_emergency_admin(&env, &new_emergency_admin);
        Storage::add_audit_entry(
            &env,
            symbol_short!("xfer_eadm"),
            &old_emergency_admin,
            Some(new_emergency_admin.clone()),
            Symbol::new(&env, ""),
            None,
        );
        env.events()
            .publish(
                (symbol_short!("xfer_eadm"),),
                (old_emergency_admin, new_emergency_admin),
            );
        Ok(())
    }

    pub fn is_paused(env: Env) -> bool {
        Storage::get_paused(&env)
    }

    pub fn get_admin(env: Env) -> Result<Address, RBACError> {
        Storage::get_admin(&env)
    }

    pub fn get_emergency_admin_address(env: Env) -> Result<Address, RBACError> {
        Storage::get_emergency_admin(&env)
    }
}

fn require_admin(env: &Env) -> Result<Address, RBACError> {
    let admin = Storage::get_admin(env)?;
    admin.require_auth();
    Ok(admin)
}

fn require_not_paused(env: &Env) -> Result<(), RBACError> {
    if Storage::get_paused(env) {
        return Err(RBACError::ContractPaused);
    }
    Ok(())
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

fn resolve_role_permissions(env: &Env, role: &Symbol) -> Vec<Symbol> {
    let mut all_perms = Vec::new(env);
    let mut current = role.clone();
    let mut visited = Vec::new(env);
    loop {
        for v in visited.iter() {
            if v == current {
                return all_perms;
            }
        }
        visited.push_back(current.clone());
        let perms = Storage::get_role_permissions(env, &current);
        for p in perms.iter() {
            if !vec_contains(&all_perms, &p) {
                all_perms.push_back(p);
            }
        }
        match Storage::get_role_parent(env, &current) {
            Some(parent) => current = parent,
            None => break,
        }
    }
    all_perms
}

fn would_create_cycle(env: &Env, child_role: &Symbol, parent_role: &Symbol) -> bool {
    let mut current = parent_role.clone();
    loop {
        if current == *child_role {
            return true;
        }
        match Storage::get_role_parent(env, &current) {
            Some(parent) => current = parent,
            None => return false,
        }
    }
}

#[cfg(test)]
mod test;
