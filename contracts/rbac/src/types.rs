use soroban_sdk::{contracterror, contracttype, Address, Symbol};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RBACError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAuthorized = 3,
    RoleAlreadyAssigned = 4,
    RoleNotAssigned = 5,
    PermissionAlreadyExists = 6,
    PermissionNotFound = 7,
    HierarchyCycle = 8,
    ContractPaused = 9,
    RoleAlreadyHasParent = 10,
    RoleHasNoParent = 11,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEntry {
    pub timestamp: u64,
    pub action: Symbol,
    pub caller: Address,
    pub target: Option<Address>,
    pub role: Symbol,
    pub permission: Option<Symbol>,
}
