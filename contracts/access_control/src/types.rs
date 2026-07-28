use soroban_sdk::{contracterror, contracttype, Address, Symbol, Vec};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AccessError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAuthorized = 3,
    ContractPaused = 4,
    RoleNotGranted = 5,
    PermissionAlreadyExists = 6,
    PermissionNotFound = 7,
    CapabilityNotFound = 8,
    CapabilityRevoked = 9,
    CapabilityExpired = 10,
    CapabilityExhausted = 11,
    NotDelegatable = 12,
    DelegationDepthExceeded = 13,
    InvalidExpiry = 14,
    DelegationNotFound = 15,
    DelegationAlreadyExists = 16,
    PolicyNotFound = 17,
    SelfDelegation = 18,
}

/// A role granted to an address. `expires_at == 0` means the grant never expires.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleGrant {
    pub role: Symbol,
    pub expires_at: u64,
}

/// A role delegated from `delegator` to another address. The delegation is only
/// valid while the delegator still holds an active direct grant of the role.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DelegatedRole {
    pub role: Symbol,
    pub delegator: Address,
    pub expires_at: u64,
}

/// A capability token: unforgeable, transferable-by-delegation proof that the
/// holder may perform `action` on `resource`.
/// `expires_at == 0` means no expiry; `max_uses == 0` means unlimited uses.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Capability {
    pub id: u64,
    pub holder: Address,
    pub issuer: Address,
    pub resource: Symbol,
    pub action: Symbol,
    pub expires_at: u64,
    pub max_uses: u32,
    pub uses: u32,
    pub delegatable: bool,
    pub depth: u32,
    pub parent: Option<u64>,
    pub revoked: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyEffect {
    Allow,
    Deny,
}

/// A single policy rule: applies to holders of `role` with the given effect.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyRule {
    pub effect: PolicyEffect,
    pub role: Symbol,
}

/// Policy for a (resource, action) pair. Deny rules override allow rules;
/// if no rule matches the caller's roles, `default_allow` decides.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Policy {
    pub resource: Symbol,
    pub action: Symbol,
    pub rules: Vec<PolicyRule>,
    pub default_allow: bool,
}

/// A recorded access decision or administrative action.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessLogEntry {
    pub timestamp: u64,
    pub actor: Address,
    pub action: Symbol,
    pub resource: Symbol,
    pub allowed: bool,
}
