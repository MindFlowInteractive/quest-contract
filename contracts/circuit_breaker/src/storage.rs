use soroban_sdk::{Address, Env, String, Symbol, Vec};

use crate::types::{
    CircuitBreakerError, DataKey, GuardianConfig, PauseProposal, PauseRecord, PauseState,
};

pub fn get_config(env: &Env) -> Result<GuardianConfig, CircuitBreakerError> {
    env.storage()
        .instance()
        .get(&DataKey::Config)
        .ok_or(CircuitBreakerError::NotInitialized)
}

pub fn set_config(env: &Env, config: &GuardianConfig) {
    env.storage().instance().set(&DataKey::Config, config);
}

pub fn inactive_state(env: &Env) -> PauseState {
    PauseState {
        active: false,
        paused_at: 0,
        paused_by: None,
        reason: String::from_str(env, ""),
        pause_id: 0,
        unpause_requested_at: 0,
        unpause_requested_by: None,
        unpause_reason: String::from_str(env, ""),
        total_pauses: 0,
    }
}

pub fn get_state(env: &Env) -> PauseState {
    env.storage()
        .instance()
        .get(&DataKey::PauseState)
        .unwrap_or_else(|| inactive_state(env))
}

pub fn set_state(env: &Env, state: &PauseState) {
    env.storage().instance().set(&DataKey::PauseState, state);
}

pub fn get_proposal(env: &Env, id: u64) -> Result<PauseProposal, CircuitBreakerError> {
    env.storage()
        .instance()
        .get(&DataKey::PauseProposal(id))
        .ok_or(CircuitBreakerError::ProposalNotFound)
}

pub fn set_proposal(env: &Env, id: u64, proposal: &PauseProposal) {
    env.storage()
        .instance()
        .set(&DataKey::PauseProposal(id), proposal);
}

pub fn proposal_count(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::NextProposalId)
        .unwrap_or(0)
}

pub fn next_proposal_id(env: &Env) -> u64 {
    let id: u64 = env
        .storage()
        .instance()
        .get(&DataKey::NextProposalId)
        .unwrap_or(0);
    env.storage()
        .instance()
        .set(&DataKey::NextProposalId, &(id + 1));
    id
}

pub fn get_allowlist(env: &Env) -> Vec<Symbol> {
    env.storage()
        .instance()
        .get(&DataKey::Allowlist)
        .unwrap_or_else(|| Vec::new(env))
}

pub fn set_allowlist(env: &Env, functions: &Vec<Symbol>) {
    env.storage().instance().set(&DataKey::Allowlist, functions);
}

pub fn is_allowed(env: &Env, function: &Symbol) -> bool {
    let allowlist = get_allowlist(env);
    for i in 0..allowlist.len() {
        if &allowlist.get(i).unwrap() == function {
            return true;
        }
    }
    false
}

pub fn get_history(env: &Env) -> Vec<PauseRecord> {
    env.storage()
        .persistent()
        .get(&DataKey::PauseHistory)
        .unwrap_or_else(|| Vec::new(env))
}

pub fn set_history(env: &Env, history: &Vec<PauseRecord>) {
    env.storage()
        .persistent()
        .set(&DataKey::PauseHistory, history);
}

pub fn is_guardian(config: &GuardianConfig, address: &Address) -> bool {
    for i in 0..config.guardians.len() {
        if &config.guardians.get(i).unwrap() == address {
            return true;
        }
    }
    false
}
