#![no_std]
use soroban_sdk::{contracttype, Address, Env, Vec};
use crate::types::{ClaimHistoryEntry, ClaimRecord, Distribution};

// ─── Storage keys ─────────────────────────────────────────────────────────────

#[contracttype]
pub enum DataKey {
    /// Admin address
    Admin,
    /// Auto-increment counter for distribution IDs
    DistCounter,
    /// Distribution struct by ID
    Dist(u32),
    /// Whether (dist_id, claimer) has been claimed: bool
    Claimed(u32, Address),
    /// ClaimRecord for a specific (dist_id, claimer) – full record
    ClaimRec(u32, Address),
    /// Per-claimer history list: Vec<ClaimHistoryEntry>
    ClaimHistory(Address),
}

// ─── TTL ──────────────────────────────────────────────────────────────────────

/// ~1 year at ~5 s/ledger
const TTL: u32 = 6_307_200;

// ─── Admin ────────────────────────────────────────────────────────────────────

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

// ─── Distribution counter ────────────────────────────────────────────────────

pub fn next_dist_id(env: &Env) -> u32 {
    let current: u32 = env
        .storage()
        .instance()
        .get(&DataKey::DistCounter)
        .unwrap_or(0);
    let next = current + 1;
    env.storage().instance().set(&DataKey::DistCounter, &next);
    next
}

// ─── Distributions ───────────────────────────────────────────────────────────

pub fn get_distribution(env: &Env, id: u32) -> Option<Distribution> {
    env.storage().persistent().get(&DataKey::Dist(id))
}

pub fn set_distribution(env: &Env, id: u32, dist: &Distribution) {
    let key = DataKey::Dist(id);
    env.storage().persistent().set(&key, dist);
    env.storage().persistent().extend_ttl(&key, TTL, TTL);
}

// ─── Claim flags ─────────────────────────────────────────────────────────────

pub fn is_claimed(env: &Env, dist_id: u32, claimer: &Address) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::Claimed(dist_id, claimer.clone()))
        .unwrap_or(false)
}

pub fn mark_claimed(env: &Env, dist_id: u32, claimer: &Address) {
    let key = DataKey::Claimed(dist_id, claimer.clone());
    env.storage().persistent().set(&key, &true);
    env.storage().persistent().extend_ttl(&key, TTL, TTL);
}

// ─── Claim records ───────────────────────────────────────────────────────────

pub fn set_claim_record(env: &Env, dist_id: u32, claimer: &Address, record: &ClaimRecord) {
    let key = DataKey::ClaimRec(dist_id, claimer.clone());
    env.storage().persistent().set(&key, record);
    env.storage().persistent().extend_ttl(&key, TTL, TTL);
}

pub fn get_claim_record(env: &Env, dist_id: u32, claimer: &Address) -> Option<ClaimRecord> {
    env.storage()
        .persistent()
        .get(&DataKey::ClaimRec(dist_id, claimer.clone()))
}

// ─── Claim history ───────────────────────────────────────────────────────────

pub fn append_claim_history(env: &Env, claimer: &Address, entry: ClaimHistoryEntry) {
    let key = DataKey::ClaimHistory(claimer.clone());
    let mut history: Vec<ClaimHistoryEntry> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));
    history.push_back(entry);
    env.storage().persistent().set(&key, &history);
    env.storage().persistent().extend_ttl(&key, TTL, TTL);
}

pub fn get_claim_history(env: &Env, claimer: &Address) -> Vec<ClaimHistoryEntry> {
    let key = DataKey::ClaimHistory(claimer.clone());
    env.storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env))
}
