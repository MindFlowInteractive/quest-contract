use soroban_sdk::{Address, Env, Val, Vec};
use crate::types::*;

// ──────────────────────────────────────────────
// Admin / metadata helpers
// ──────────────────────────────────────────────

pub fn get_admin(env: &Env) -> Address {
    env.storage().instance().get(&DataKey::Admin).unwrap()
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn has_admin(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Admin)
}

// ──────────────────────────────────────────────
// Token balances
// ──────────────────────────────────────────────

pub fn get_balance(env: &Env, account: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::Balance(account.clone()))
        .unwrap_or(0)
}

pub fn set_balance(env: &Env, account: &Address, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::Balance(account.clone()), &amount);
}

pub fn get_total_supply(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::TotalSupply)
        .unwrap_or(0)
}

pub fn set_total_supply(env: &Env, supply: i128) {
    env.storage().instance().set(&DataKey::TotalSupply, &supply);
}

// ──────────────────────────────────────────────
// Allowances
// ──────────────────────────────────────────────

pub fn get_allowance(env: &Env, owner: &Address, spender: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::Allowance(owner.clone(), spender.clone()))
        .unwrap_or(0)
}

pub fn set_allowance(env: &Env, owner: &Address, spender: &Address, amount: i128) {
    env.storage().persistent().set(
        &DataKey::Allowance(owner.clone(), spender.clone()),
        &amount,
    );
}

// ──────────────────────────────────────────────
// Delegation
// ──────────────────────────────────────────────

pub fn get_delegate(env: &Env, account: &Address) -> Option<Address> {
    env.storage()
        .persistent()
        .get(&DataKey::Delegate(account.clone()))
}

pub fn set_delegate(env: &Env, account: &Address, delegatee: &Address) {
    env.storage()
        .persistent()
        .set(&DataKey::Delegate(account.clone()), delegatee);
}

// ──────────────────────────────────────────────
// Voting power (current)
// ──────────────────────────────────────────────

pub fn get_voting_power(env: &Env, account: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::VotingPower(account.clone()))
        .unwrap_or(0)
}

pub fn set_voting_power(env: &Env, account: &Address, power: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::VotingPower(account.clone()), &power);
}

// ──────────────────────────────────────────────
// Checkpoints (vote‑weight snapshots)
// ──────────────────────────────────────────────

pub fn get_checkpoints(env: &Env, account: &Address) -> Vec<Checkpoint> {
    env.storage()
        .persistent()
        .get(&DataKey::Checkpoints(account.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

pub fn set_checkpoints(env: &Env, account: &Address, ckpts: &Vec<Checkpoint>) {
    env.storage()
        .persistent()
        .set(&DataKey::Checkpoints(account.clone()), ckpts);
}

pub fn get_num_checkpoints(env: &Env, account: &Address) -> u32 {
    env.storage()
        .persistent()
        .get(&DataKey::NumCheckpoints(account.clone()))
        .unwrap_or(0)
}

pub fn set_num_checkpoints(env: &Env, account: &Address, count: u32) {
    env.storage()
        .persistent()
        .set(&DataKey::NumCheckpoints(account.clone()), &count);
}

// ──────────────────────────────────────────────
// Governance config
// ──────────────────────────────────────────────

pub fn get_config(env: &Env) -> GovernanceConfig {
    env.storage().instance().get(&DataKey::Config).unwrap()
}

pub fn set_config(env: &Env, config: &GovernanceConfig) {
    env.storage().instance().set(&DataKey::Config, config);
}

// ──────────────────────────────────────────────
// Proposals
// ──────────────────────────────────────────────

pub fn get_proposal_count(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::ProposalCount)
        .unwrap_or(0)
}

pub fn increment_proposal_count(env: &Env) -> u64 {
    let count = get_proposal_count(env) + 1;
    env.storage()
        .instance()
        .set(&DataKey::ProposalCount, &count);
    count
}

pub fn get_proposal(env: &Env, id: u64) -> Option<Proposal> {
    env.storage()
        .persistent()
        .get(&DataKey::Proposal(id))
}

pub fn set_proposal(env: &Env, proposal: &Proposal) {
    env.storage()
        .persistent()
        .set(&DataKey::Proposal(proposal.id), proposal);
}

pub fn get_proposal_args(env: &Env, id: u64) -> Option<Vec<Val>> {
    env.storage()
        .persistent()
        .get(&DataKey::ProposalArgs(id))
}

pub fn set_proposal_args(env: &Env, id: u64, args: &Vec<Val>) {
    env.storage()
        .persistent()
        .set(&DataKey::ProposalArgs(id), args);
}

// ──────────────────────────────────────────────
// Voting records
// ──────────────────────────────────────────────

pub fn has_voted(env: &Env, proposal_id: u64, voter: &Address) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::HasVoted(proposal_id, voter.clone()))
}

pub fn set_has_voted(env: &Env, proposal_id: u64, voter: &Address) {
    env.storage()
        .persistent()
        .set(&DataKey::HasVoted(proposal_id, voter.clone()), &true);
}

pub fn get_vote_receipt(env: &Env, proposal_id: u64, voter: &Address) -> Option<VoteReceipt> {
    env.storage()
        .persistent()
        .get(&DataKey::VoteReceipt(proposal_id, voter.clone()))
}

pub fn set_vote_receipt(env: &Env, proposal_id: u64, voter: &Address, receipt: &VoteReceipt) {
    env.storage()
        .persistent()
        .set(&DataKey::VoteReceipt(proposal_id, voter.clone()), receipt);
}
