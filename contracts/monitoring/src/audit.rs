#![no_std]

use soroban_sdk::{contracttype, Env};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEntry {
    pub snapshot_id: u64,
    pub timestamp: u64,
}

pub fn log_audit_entry(env: &Env, snapshot_id: u64) {
    // Implementation will be in lib.rs
}