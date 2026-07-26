#![no_std]
use soroban_sdk::{contracttype, Address, BytesN, Env, Vec};
use crate::types::{CustodyInfo, UnwrapRequest, WrapRequest, WrappedTokenConfig};

/// All storage keys used by the wrapped tokens contract.
#[contracttype]
pub enum DataKey {
    /// WrappedTokenConfig struct — stored in instance storage
    Config,
    /// Balance of an account: DataKey::Balance(address) -> i128
    Balance(Address),
    /// Allowance: DataKey::Allowance(owner, spender) -> i128
    Allowance(Address, Address),
    /// Global nonce counter for wrap requests (instance)
    WrapNonce,
    /// Global nonce counter for unwrap requests (instance)
    UnwrapNonce,
    /// Wrap request by nonce: DataKey::WrapReq(nonce) -> WrapRequest
    WrapReq(u64),
    /// Unwrap request by nonce: DataKey::UnwrapReq(nonce) -> UnwrapRequest
    UnwrapReq(u64),
    /// Replay prevention: DataKey::UsedTxId(tx_id) -> bool
    UsedTxId(BytesN<32>),
    /// Whether an address is an authorized operator: DataKey::Operator(addr) -> bool
    Operator(Address),
    /// Ordered list of all operator addresses
    OperatorList,
    /// Per-operator confirmation for a nonce: DataKey::Confirmation(nonce, operator) -> bool
    Confirmation(u64, Address),
    /// Count of confirmations for a given nonce
    ConfirmationCount(u64),
    /// Aggregate custody/statistics
    Custody,
}

// ── Persistent storage TTL constants ────────────────────────────────────────

/// ~1 year worth of ledgers at 5s per ledger
const PERSISTENT_TTL_LEDGERS: u32 = 6_307_200;

// ── Config ───────────────────────────────────────────────────────────────────

pub fn get_config(env: &Env) -> Option<WrappedTokenConfig> {
    env.storage().instance().get(&DataKey::Config)
}

pub fn set_config(env: &Env, config: &WrappedTokenConfig) {
    env.storage().instance().set(&DataKey::Config, config);
}

// ── Balances ─────────────────────────────────────────────────────────────────

pub fn get_balance(env: &Env, account: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::Balance(account.clone()))
        .unwrap_or(0)
}

pub fn set_balance(env: &Env, account: &Address, amount: i128) {
    let key = DataKey::Balance(account.clone());
    if amount == 0 {
        env.storage().persistent().remove(&key);
    } else {
        env.storage().persistent().set(&key, &amount);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
    }
}

// ── Allowances ───────────────────────────────────────────────────────────────

pub fn get_allowance(env: &Env, owner: &Address, spender: &Address) -> i128 {
    env.storage()
        .temporary()
        .get(&DataKey::Allowance(owner.clone(), spender.clone()))
        .unwrap_or(0)
}

pub fn set_allowance(env: &Env, owner: &Address, spender: &Address, amount: i128) {
    let key = DataKey::Allowance(owner.clone(), spender.clone());
    if amount == 0 {
        env.storage().temporary().remove(&key);
    } else {
        // Allowances expire after ~1 year
        env.storage().temporary().set(&key, &amount);
        env.storage()
            .temporary()
            .extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
    }
}

// ── Nonces ────────────────────────────────────────────────────────────────────

pub fn get_wrap_nonce(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::WrapNonce)
        .unwrap_or(0u64)
}

/// Atomically reads the current nonce, increments it in storage, and returns
/// the value that was consumed (i.e., the nonce assigned to the new request).
pub fn increment_wrap_nonce(env: &Env) -> u64 {
    let nonce = get_wrap_nonce(env);
    let next = nonce.checked_add(1).expect("nonce overflow");
    env.storage().instance().set(&DataKey::WrapNonce, &next);
    nonce
}

pub fn get_unwrap_nonce(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::UnwrapNonce)
        .unwrap_or(0u64)
}

pub fn increment_unwrap_nonce(env: &Env) -> u64 {
    let nonce = get_unwrap_nonce(env);
    let next = nonce.checked_add(1).expect("nonce overflow");
    env.storage().instance().set(&DataKey::UnwrapNonce, &next);
    nonce
}

// ── Wrap requests ─────────────────────────────────────────────────────────────

pub fn get_wrap_request(env: &Env, nonce: u64) -> Option<WrapRequest> {
    env.storage().persistent().get(&DataKey::WrapReq(nonce))
}

pub fn set_wrap_request(env: &Env, nonce: u64, req: &WrapRequest) {
    let key = DataKey::WrapReq(nonce);
    env.storage().persistent().set(&key, req);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
}

// ── Unwrap requests ───────────────────────────────────────────────────────────

pub fn get_unwrap_request(env: &Env, nonce: u64) -> Option<UnwrapRequest> {
    env.storage().persistent().get(&DataKey::UnwrapReq(nonce))
}

pub fn set_unwrap_request(env: &Env, nonce: u64, req: &UnwrapRequest) {
    let key = DataKey::UnwrapReq(nonce);
    env.storage().persistent().set(&key, req);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
}

// ── Replay prevention ─────────────────────────────────────────────────────────

pub fn is_tx_used(env: &Env, tx_id: &BytesN<32>) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::UsedTxId(tx_id.clone()))
        .unwrap_or(false)
}

pub fn mark_tx_used(env: &Env, tx_id: &BytesN<32>) {
    let key = DataKey::UsedTxId(tx_id.clone());
    env.storage().persistent().set(&key, &true);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
}

// ── Operators ─────────────────────────────────────────────────────────────────

pub fn is_operator(env: &Env, addr: &Address) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Operator(addr.clone()))
        .unwrap_or(false)
}

pub fn set_operator(env: &Env, addr: &Address, active: bool) {
    env.storage()
        .instance()
        .set(&DataKey::Operator(addr.clone()), &active);
}

pub fn get_operator_list(env: &Env) -> Vec<Address> {
    env.storage()
        .instance()
        .get(&DataKey::OperatorList)
        .unwrap_or(Vec::new(env))
}

pub fn set_operator_list(env: &Env, list: &Vec<Address>) {
    env.storage().instance().set(&DataKey::OperatorList, list);
}

// ── Confirmations ─────────────────────────────────────────────────────────────

pub fn has_confirmed(env: &Env, nonce: u64, operator: &Address) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::Confirmation(nonce, operator.clone()))
        .unwrap_or(false)
}

pub fn set_confirmed(env: &Env, nonce: u64, operator: &Address) {
    let key = DataKey::Confirmation(nonce, operator.clone());
    env.storage().persistent().set(&key, &true);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
}

pub fn get_confirmation_count(env: &Env, nonce: u64) -> u32 {
    env.storage()
        .persistent()
        .get(&DataKey::ConfirmationCount(nonce))
        .unwrap_or(0u32)
}

pub fn increment_confirmation_count(env: &Env, nonce: u64) -> u32 {
    let count = get_confirmation_count(env, nonce);
    let next = count + 1;
    let key = DataKey::ConfirmationCount(nonce);
    env.storage().persistent().set(&key, &next);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
    next
}

// ── Custody stats ─────────────────────────────────────────────────────────────

pub fn get_custody(env: &Env) -> CustodyInfo {
    env.storage()
        .instance()
        .get(&DataKey::Custody)
        .unwrap_or(CustodyInfo {
            total_supply: 0,
            total_fees_collected: 0,
            total_wraps: 0,
            total_unwraps: 0,
            last_operation_at: 0,
        })
}

pub fn set_custody(env: &Env, info: &CustodyInfo) {
    env.storage().instance().set(&DataKey::Custody, info);
}
