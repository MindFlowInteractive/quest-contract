use soroban_sdk::{Address, Env, Vec};
use crate::types::{DataKey, Member, Transaction, TransactionRecord, TreasuryConfig, EmergencyState};

/// Store treasury configuration
pub fn set_config(env: &Env, config: &TreasuryConfig) {
    env.storage().instance().set(&DataKey::Config, config);
}

/// Get treasury configuration
pub fn get_config(env: &Env) -> Option<TreasuryConfig> {
    env.storage().instance().get(&DataKey::Config)
}

/// Store member data
pub fn set_member(env: &Env, address: &Address, member: &Member) {
    env.storage().persistent().set(&DataKey::Member(address.clone()), member);
}

/// Get member data
pub fn get_member(env: &Env, address: &Address) -> Option<Member> {
    env.storage().persistent().get(&DataKey::Member(address.clone()))
}

/// Remove member data
pub fn remove_member(env: &Env, address: &Address) {
    env.storage().persistent().remove(&DataKey::Member(address.clone()));
}

/// Store the list of all members
pub fn set_members(env: &Env, members: &Vec<Address>) {
    env.storage().persistent().set(&DataKey::Members, members);
}

/// Get the list of all members
pub fn get_members(env: &Env) -> Vec<Address> {
    env.storage().persistent().get(&DataKey::Members).unwrap_or_else(|| Vec::new(env))
}

/// Store a transaction
pub fn set_transaction(env: &Env, transaction: &Transaction) {
    env.storage().persistent().set(&DataKey::Transaction(transaction.id), transaction);
}

/// Get a transaction by ID
pub fn get_transaction(env: &Env, id: u64) -> Option<Transaction> {
    env.storage().persistent().get(&DataKey::Transaction(id))
}

/// Get the next transaction ID and increment counter
pub fn increment_transaction_id(env: &Env) -> u64 {
    let key = DataKey::NextTransactionId;
    let current: u64 = env.storage().instance().get(&key).unwrap_or(0);
    let next = current + 1;
    env.storage().instance().set(&key, &next);
    next
}

/// Get current transaction ID without incrementing
pub fn get_current_transaction_id(env: &Env) -> u64 {
    env.storage().instance().get(&DataKey::NextTransactionId).unwrap_or(0)
}

/// Check if an address has signed a transaction
pub fn has_signed(env: &Env, tx_id: u64, signer: &Address) -> bool {
    env.storage().persistent().has(&DataKey::HasSigned(tx_id, signer.clone()))
}

/// Mark that an address has signed a transaction
pub fn set_signed(env: &Env, tx_id: u64, signer: &Address) {
    env.storage().persistent().set(&DataKey::HasSigned(tx_id, signer.clone()), &true);
}

/// Store transaction history record
pub fn set_transaction_history(env: &Env, id: u64, record: &TransactionRecord) {
    env.storage().persistent().set(&DataKey::TransactionHistory(id), record);
}

/// Get transaction history record
pub fn get_transaction_history(env: &Env, id: u64) -> Option<TransactionRecord> {
    env.storage().persistent().get(&DataKey::TransactionHistory(id))
}

/// Get total transaction count
pub fn get_transaction_count(env: &Env) -> u64 {
    env.storage().persistent().get(&DataKey::TransactionCount).unwrap_or(0)
}

/// Increment transaction count
pub fn increment_transaction_count(env: &Env) {
    let current = get_transaction_count(env);
    env.storage().persistent().set(&DataKey::TransactionCount, &(current + 1));
}

/// Set emergency state
pub fn set_emergency_state(env: &Env, state: &EmergencyState) {
    env.storage().persistent().set(&DataKey::EmergencyState, state);
}

/// Get emergency state
pub fn get_emergency_state(env: &Env) -> Option<EmergencyState> {
    env.storage().persistent().get(&DataKey::EmergencyState)
}

/// Set last emergency timestamp
pub fn set_last_emergency_at(env: &Env, timestamp: u64) {
    env.storage().persistent().set(&DataKey::LastEmergencyAt, &timestamp);
}

/// Get last emergency timestamp
pub fn get_last_emergency_at(env: &Env) -> u64 {
    env.storage().persistent().get(&DataKey::LastEmergencyAt).unwrap_or(0)
}

/// Store pending transaction IDs
pub fn set_pending_transactions(env: &Env, ids: &Vec<u64>) {
    env.storage().persistent().set(&DataKey::PendingTransactions, ids);
}

/// Get pending transaction IDs
pub fn get_pending_transactions(env: &Env) -> Vec<u64> {
    env.storage().persistent().get(&DataKey::PendingTransactions).unwrap_or_else(|| Vec::new(env))
}
