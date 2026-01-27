use soroban_sdk::{Env, Address, Vec, Val};
use crate::types::{DataKey, GovernanceConfig, Proposal};

pub fn set_config(env: &Env, config: &GovernanceConfig) {
    env.storage().instance().set(&DataKey::Config, config);
}

pub fn get_config(env: &Env) -> GovernanceConfig {
    env.storage().instance().get(&DataKey::Config).unwrap()
}

pub fn get_proposal_count(env: &Env) -> u64 {
    env.storage().instance().get(&DataKey::ProposalCount).unwrap_or(0)
}

pub fn increment_proposal_count(env: &Env) -> u64 {
    let count = get_proposal_count(env);
    let new_count = count + 1;
    env.storage().instance().set(&DataKey::ProposalCount, &new_count);
    new_count
}

pub fn set_proposal(env: &Env, proposal: &Proposal) {
    env.storage().persistent().set(&DataKey::Proposal(proposal.id), proposal);
}

pub fn get_proposal(env: &Env, proposal_id: u64) -> Option<Proposal> {
    env.storage().persistent().get(&DataKey::Proposal(proposal_id))
}

pub fn set_proposal_args(env: &Env, proposal_id: u64, args: &Vec<Val>) {
    env.storage().persistent().set(&DataKey::ProposalArgs(proposal_id), args);
}

pub fn get_proposal_args(env: &Env, proposal_id: u64) -> Option<Vec<Val>> {
    env.storage().persistent().get(&DataKey::ProposalArgs(proposal_id))
}

pub fn get_token_balance(env: &Env, user: &Address) -> i128 {
    env.storage().persistent().get(&DataKey::TokenBalance(user.clone())).unwrap_or(0)
}

pub fn set_token_balance(env: &Env, user: &Address, amount: i128) {
    env.storage().persistent().set(&DataKey::TokenBalance(user.clone()), &amount);
}

pub fn get_voting_power(env: &Env, user: &Address) -> i128 {
    env.storage().persistent().get(&DataKey::VotingPower(user.clone())).unwrap_or(0)
}

pub fn set_voting_power(env: &Env, user: &Address, amount: i128) {
    env.storage().persistent().set(&DataKey::VotingPower(user.clone()), &amount);
}

pub fn get_delegate(env: &Env, user: &Address) -> Option<Address> {
    env.storage().persistent().get(&DataKey::Delegation(user.clone()))
}

pub fn set_delegate(env: &Env, user: &Address, delegatee: &Address) {
    env.storage().persistent().set(&DataKey::Delegation(user.clone()), delegatee);
}

pub fn has_voted(env: &Env, proposal_id: u64, user: &Address) -> bool {
    env.storage().persistent().has(&DataKey::Vote(proposal_id, user.clone()))
}

pub fn set_voted(env: &Env, proposal_id: u64, user: &Address) {
    env.storage().persistent().set(&DataKey::Vote(proposal_id, user.clone()), &true);
}
