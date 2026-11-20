#![no_std]

mod storage;
pub mod types;

use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, String, Symbol, Vec};

use crate::storage::*;
use crate::types::*;

#[contract]
pub struct CircuitBreaker;

#[contractimpl]
impl CircuitBreaker {
    /// Configure the guardian set, quorum and timings. Callable once.
    ///
    /// `quorum` must be a strict majority of `guardians` so that a minority can
    /// never trip the breaker on its own.
    pub fn initialize(
        env: Env,
        admin: Address,
        guardians: Vec<Address>,
        quorum: u32,
        unpause_delay: u64,
        pause_proposal_ttl: u64,
    ) -> Result<(), CircuitBreakerError> {
        if env.storage().instance().has(&DataKey::Config) {
            return Err(CircuitBreakerError::AlreadyInitialized);
        }
        admin.require_auth();

        if guardians.is_empty()
            || quorum == 0
            || quorum as u64 > guardians.len() as u64
            || quorum as u64 * 2 <= guardians.len() as u64
            || unpause_delay == 0
            || pause_proposal_ttl == 0
        {
            return Err(CircuitBreakerError::InvalidConfiguration);
        }

        let mut unique = Vec::<Address>::new(&env);
        for i in 0..guardians.len() {
            let guardian = guardians.get(i).unwrap();
            for j in 0..unique.len() {
                if unique.get(j).unwrap() == guardian {
                    return Err(CircuitBreakerError::InvalidConfiguration);
                }
            }
            unique.push_back(guardian);
        }

        let config = GuardianConfig {
            admin: admin.clone(),
            guardians: unique,
            quorum,
            unpause_delay,
            pause_proposal_ttl,
        };
        set_config(&env, &config);
        set_state(&env, &inactive_state(&env));
        set_allowlist(&env, &default_allowlist(&env));
        env.storage()
            .instance()
            .set(&DataKey::NextProposalId, &0u64);
        set_history(&env, &Vec::new(&env));

        env.events()
            .publish((symbol_short!("init"),), (admin, quorum));
        Ok(())
    }

    // ------------------------------------------------------------------
    // Pause flow — fast, guardian-quorum driven
    // ------------------------------------------------------------------

    /// A guardian opens a pause proposal. The proposer's approval is counted.
    pub fn propose_pause(
        env: Env,
        proposer: Address,
        reason: String,
    ) -> Result<u64, CircuitBreakerError> {
        proposer.require_auth();
        let config = get_config(&env)?;
        if !is_guardian(&config, &proposer) {
            return Err(CircuitBreakerError::Unauthorized);
        }
        if get_state(&env).active {
            return Err(CircuitBreakerError::AlreadyPaused);
        }

        let count = proposal_count(&env);
        for id in 0..count {
            let existing = get_proposal(&env, id)?;
            if existing.status == ProposalStatus::Pending {
                return Err(CircuitBreakerError::PendingProposalExists);
            }
        }

        let now = env.ledger().timestamp();
        let mut approvals = Vec::<Address>::new(&env);
        approvals.push_back(proposer.clone());
        let id = next_proposal_id(&env);
        let proposal = PauseProposal {
            id,
            proposer: proposer.clone(),
            reason,
            created_at: now,
            expires_at: now + config.pause_proposal_ttl,
            approvals,
            status: ProposalStatus::Pending,
            executed_at: 0,
        };
        set_proposal(&env, id, &proposal);

        env.events()
            .publish((symbol_short!("p_prop"),), (id, proposer));
        Ok(id)
    }

    /// A guardian approves a pending proposal. Reaching quorum trips the
    /// breaker immediately, which is the emergency fast path.
    pub fn approve_pause(
        env: Env,
        guardian: Address,
        proposal_id: u64,
    ) -> Result<u32, CircuitBreakerError> {
        guardian.require_auth();
        let config = get_config(&env)?;
        if !is_guardian(&config, &guardian) {
            return Err(CircuitBreakerError::Unauthorized);
        }

        let mut proposal = get_proposal(&env, proposal_id)?;
        if proposal.status != ProposalStatus::Pending {
            return Err(CircuitBreakerError::ProposalNotPending);
        }

        let now = env.ledger().timestamp();
        if now > proposal.expires_at {
            proposal.status = ProposalStatus::Expired;
            set_proposal(&env, proposal_id, &proposal);
            return Err(CircuitBreakerError::ProposalExpired);
        }

        for i in 0..proposal.approvals.len() {
            if proposal.approvals.get(i).unwrap() == guardian {
                return Err(CircuitBreakerError::DuplicateApproval);
            }
        }
        proposal.approvals.push_back(guardian.clone());
        let approvals = proposal.approvals.len() as u32;

        if approvals >= config.quorum {
            let mut state = get_state(&env);
            if state.active {
                return Err(CircuitBreakerError::AlreadyPaused);
            }

            state.active = true;
            state.paused_at = now;
            state.paused_by = Some(proposal.proposer.clone());
            state.reason = proposal.reason.clone();
            state.pause_id = proposal.id;
            state.unpause_requested_at = 0;
            state.unpause_requested_by = None;
            state.unpause_reason = String::from_str(&env, "");
            state.total_pauses += 1;
            set_state(&env, &state);

            proposal.status = ProposalStatus::Executed;
            proposal.executed_at = now;

            let mut history = get_history(&env);
            history.push_back(PauseRecord {
                id: proposal.id,
                paused_at: now,
                paused_by: proposal.proposer.clone(),
                reason: proposal.reason.clone(),
                unpaused_at: 0,
            });
            set_history(&env, &history);

            env.events().publish(
                (symbol_short!("tripped"),),
                (proposal.id, proposal.reason.clone()),
            );
        }

        set_proposal(&env, proposal_id, &proposal);
        env.events().publish(
            (symbol_short!("p_appr"),),
            (proposal_id, guardian, approvals),
        );
        Ok(approvals)
    }

    /// The proposer can withdraw an unexecuted proposal.
    pub fn cancel_pause_proposal(
        env: Env,
        proposer: Address,
        proposal_id: u64,
    ) -> Result<(), CircuitBreakerError> {
        proposer.require_auth();
        let mut proposal = get_proposal(&env, proposal_id)?;
        if proposal.proposer != proposer {
            return Err(CircuitBreakerError::NotProposer);
        }
        if proposal.status != ProposalStatus::Pending {
            return Err(CircuitBreakerError::ProposalNotPending);
        }
        proposal.status = ProposalStatus::Canceled;
        set_proposal(&env, proposal_id, &proposal);
        env.events()
            .publish((symbol_short!("p_cancel"),), proposal_id);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Unpause flow — slow, requires a request plus admin approval
    // ------------------------------------------------------------------

    /// A guardian (or the admin) starts the unpause process. The actual
    /// unpause still has to wait out `unpause_delay` and be approved by admin.
    pub fn request_unpause(
        env: Env,
        caller: Address,
        reason: String,
    ) -> Result<u64, CircuitBreakerError> {
        caller.require_auth();
        let config = get_config(&env)?;
        if !is_guardian(&config, &caller) && config.admin != caller {
            return Err(CircuitBreakerError::Unauthorized);
        }

        let mut state = get_state(&env);
        if !state.active {
            return Err(CircuitBreakerError::NotPaused);
        }
        if state.unpause_requested_at != 0 {
            return Err(CircuitBreakerError::UnpauseAlreadyRequested);
        }

        let now = env.ledger().timestamp();
        state.unpause_requested_at = now;
        state.unpause_requested_by = Some(caller.clone());
        state.unpause_reason = reason;
        set_state(&env, &state);

        let earliest = state.paused_at + config.unpause_delay;
        env.events()
            .publish((symbol_short!("u_req"),), (caller, earliest));
        Ok(earliest)
    }

    /// Admin approval that finally re-closes the circuit. The delay is
    /// re-checked here so it cannot be bypassed by requesting early.
    pub fn execute_unpause(env: Env, admin: Address) -> Result<(), CircuitBreakerError> {
        admin.require_auth();
        let config = get_config(&env)?;
        if config.admin != admin {
            return Err(CircuitBreakerError::Unauthorized);
        }

        let mut state = get_state(&env);
        if !state.active {
            return Err(CircuitBreakerError::NotPaused);
        }
        if state.unpause_requested_at == 0 {
            return Err(CircuitBreakerError::UnpauseNotRequested);
        }

        let now = env.ledger().timestamp();
        if now < state.paused_at + config.unpause_delay {
            return Err(CircuitBreakerError::UnpauseDelayNotElapsed);
        }

        state.active = false;
        state.unpause_requested_at = 0;
        state.unpause_requested_by = None;
        state.unpause_reason = String::from_str(&env, "");
        set_state(&env, &state);

        let history = get_history(&env);
        let mut updated = Vec::<PauseRecord>::new(&env);
        for i in 0..history.len() {
            let mut record = history.get(i).unwrap();
            if i == history.len() - 1 {
                record.unpaused_at = now;
            }
            updated.push_back(record);
        }
        set_history(&env, &updated);

        env.events()
            .publish((symbol_short!("reclosed"),), (now, admin));
        Ok(())
    }

    // ------------------------------------------------------------------
    // Integration surface
    // ------------------------------------------------------------------

    /// Returns `Ok(())` when `function` may run. Integrating contracts call
    /// this at the entry point of every state-changing function.
    pub fn assert_callable(env: Env, function: Symbol) -> Result<(), CircuitBreakerError> {
        get_config(&env)?;
        if !get_state(&env).active || is_allowed(&env, &function) {
            return Ok(());
        }
        Err(CircuitBreakerError::FunctionNotCallable)
    }

    pub fn is_paused(env: Env) -> bool {
        get_state(&env).active
    }

    pub fn get_status(env: Env) -> Result<CircuitStatus, CircuitBreakerError> {
        let config = get_config(&env)?;
        let state = get_state(&env);
        let now = env.ledger().timestamp();
        let seconds_until_unpause = if state.active {
            let earliest = state.paused_at + config.unpause_delay;
            if earliest > now {
                earliest - now
            } else {
                0
            }
        } else {
            0
        };

        let mut pending_approvals = 0u32;
        if !state.active {
            let count = proposal_count(&env);
            for id in 0..count {
                let proposal = get_proposal(&env, id)?;
                if proposal.status == ProposalStatus::Pending {
                    pending_approvals = proposal.approvals.len() as u32;
                    break;
                }
            }
        }

        Ok(CircuitStatus {
            paused: state.active,
            quorum: config.quorum,
            pending_approvals,
            unpause_delay: config.unpause_delay,
            seconds_until_unpause,
            total_pauses: state.total_pauses,
        })
    }

    // ------------------------------------------------------------------
    // Views
    // ------------------------------------------------------------------

    pub fn get_config(env: Env) -> Result<GuardianConfig, CircuitBreakerError> {
        get_config(&env)
    }

    pub fn get_pause_state(env: Env) -> Result<PauseState, CircuitBreakerError> {
        get_config(&env)?;
        Ok(get_state(&env))
    }

    pub fn get_pause_proposal(env: Env, id: u64) -> Result<PauseProposal, CircuitBreakerError> {
        get_proposal(&env, id)
    }

    pub fn get_guardians(env: Env) -> Result<Vec<Address>, CircuitBreakerError> {
        Ok(get_config(&env)?.guardians)
    }

    pub fn is_guardian(env: Env, address: Address) -> Result<bool, CircuitBreakerError> {
        Ok(is_guardian(&get_config(&env)?, &address))
    }

    pub fn get_allowlist(env: Env) -> Result<Vec<Symbol>, CircuitBreakerError> {
        get_config(&env)?;
        Ok(get_allowlist(&env))
    }

    pub fn get_pause_history(env: Env) -> Result<Vec<PauseRecord>, CircuitBreakerError> {
        get_config(&env)?;
        Ok(get_history(&env))
    }

    // ------------------------------------------------------------------
    // Admin configuration
    // ------------------------------------------------------------------

    pub fn add_guardian(
        env: Env,
        admin: Address,
        guardian: Address,
    ) -> Result<(), CircuitBreakerError> {
        let mut config = require_admin(&env, admin)?;
        if is_guardian(&config, &guardian) {
            return Err(CircuitBreakerError::GuardianAlreadyExists);
        }
        config.guardians.push_back(guardian.clone());
        set_config(&env, &config);
        env.events().publish((symbol_short!("g_add"),), guardian);
        Ok(())
    }

    pub fn remove_guardian(
        env: Env,
        admin: Address,
        guardian: Address,
    ) -> Result<(), CircuitBreakerError> {
        let mut config = require_admin(&env, admin)?;
        let mut remaining = Vec::<Address>::new(&env);
        let mut found = false;
        for i in 0..config.guardians.len() {
            let current = config.guardians.get(i).unwrap();
            if current == guardian {
                found = true;
            } else {
                remaining.push_back(current);
            }
        }
        if !found {
            return Err(CircuitBreakerError::GuardianNotFound);
        }
        if remaining.is_empty()
            || config.quorum as u64 > remaining.len() as u64
            || config.quorum as u64 * 2 <= remaining.len() as u64
        {
            return Err(CircuitBreakerError::InvalidConfiguration);
        }
        config.guardians = remaining;
        set_config(&env, &config);
        env.events().publish((symbol_short!("g_rem"),), guardian);
        Ok(())
    }

    pub fn set_quorum(env: Env, admin: Address, quorum: u32) -> Result<(), CircuitBreakerError> {
        let mut config = require_admin(&env, admin)?;
        if quorum == 0
            || quorum as u64 > config.guardians.len() as u64
            || quorum as u64 * 2 <= config.guardians.len() as u64
        {
            return Err(CircuitBreakerError::InvalidConfiguration);
        }
        config.quorum = quorum;
        set_config(&env, &config);
        Ok(())
    }

    pub fn set_unpause_delay(
        env: Env,
        admin: Address,
        delay: u64,
    ) -> Result<(), CircuitBreakerError> {
        let mut config = require_admin(&env, admin)?;
        if delay == 0 {
            return Err(CircuitBreakerError::InvalidConfiguration);
        }
        config.unpause_delay = delay;
        set_config(&env, &config);
        Ok(())
    }

    pub fn set_pause_proposal_ttl(
        env: Env,
        admin: Address,
        ttl: u64,
    ) -> Result<(), CircuitBreakerError> {
        let mut config = require_admin(&env, admin)?;
        if ttl == 0 {
            return Err(CircuitBreakerError::InvalidConfiguration);
        }
        config.pause_proposal_ttl = ttl;
        set_config(&env, &config);
        Ok(())
    }

    /// Allow an additional function to run while the breaker is open.
    pub fn allow_function(
        env: Env,
        admin: Address,
        function: Symbol,
    ) -> Result<(), CircuitBreakerError> {
        require_admin(&env, admin)?;
        let mut allowlist = get_allowlist(&env);
        for i in 0..allowlist.len() {
            if allowlist.get(i).unwrap() == function {
                return Ok(());
            }
        }
        allowlist.push_back(function.clone());
        set_allowlist(&env, &allowlist);
        env.events().publish((symbol_short!("allow"),), function);
        Ok(())
    }

    /// Remove a function from the pause-time allowlist. The unpause flow can
    /// never be removed, otherwise a paused contract could be bricked.
    pub fn disallow_function(
        env: Env,
        admin: Address,
        function: Symbol,
    ) -> Result<(), CircuitBreakerError> {
        require_admin(&env, admin)?;
        if is_unpause_function(&env, &function) {
            return Err(CircuitBreakerError::InvalidConfiguration);
        }
        let allowlist = get_allowlist(&env);
        let mut updated = Vec::<Symbol>::new(&env);
        for i in 0..allowlist.len() {
            let current = allowlist.get(i).unwrap();
            if current != function {
                updated.push_back(current);
            }
        }
        set_allowlist(&env, &updated);
        Ok(())
    }
}

/// Convenience guard for integrating contracts. Add this crate as a dependency
/// and call `enforce_not_paused(&env, &breaker, &symbol_short!("my_function"))`
/// at the start of any function that changes state.
pub fn enforce_not_paused(env: &Env, breaker: &Address, function: &Symbol) {
    CircuitBreakerClient::new(env, breaker).assert_callable(function);
}

fn require_admin(env: &Env, admin: Address) -> Result<GuardianConfig, CircuitBreakerError> {
    admin.require_auth();
    let config = get_config(env)?;
    if config.admin != admin {
        return Err(CircuitBreakerError::Unauthorized);
    }
    Ok(config)
}

fn is_unpause_function(env: &Env, function: &Symbol) -> bool {
    function == &Symbol::new(env, "request_unpause")
        || function == &Symbol::new(env, "execute_unpause")
        || function == &Symbol::new(env, "assert_callable")
}

#[cfg(test)]
mod test;
