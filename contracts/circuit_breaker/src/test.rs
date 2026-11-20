#![cfg(test)]

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    Address, Env, String, Vec,
};

const UNPAUSE_DELAY: u64 = 7_200;
const PROPOSAL_TTL: u64 = 3_600;

struct Fixture {
    env: Env,
    client: CircuitBreakerClient<'static>,
    admin: Address,
    g2: Address,
    g3: Address,
}

impl Fixture {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        let mut ledger = env.ledger().get();
        ledger.timestamp = 1_000;
        env.ledger().set(ledger);

        let admin = Address::generate(&env);
        let g2 = Address::generate(&env);
        let g3 = Address::generate(&env);
        let mut guardians = Vec::new(&env);
        guardians.push_back(admin.clone());
        guardians.push_back(g2.clone());
        guardians.push_back(g3.clone());

        let id = env.register_contract(None, CircuitBreaker);
        let client = CircuitBreakerClient::new(&env, &id);
        client.initialize(&admin, &guardians, &2, &UNPAUSE_DELAY, &PROPOSAL_TTL);

        Self {
            env,
            client,
            admin,
            g2,
            g3,
        }
    }

    fn set_time(&self, timestamp: u64) {
        let mut ledger = self.env.ledger().get();
        ledger.timestamp = timestamp;
        ledger.sequence_number += 1;
        self.env.ledger().set(ledger);
    }

    /// Two guardians (proposer + one approval) trip the breaker.
    fn trip(&self) -> u64 {
        let id = self
            .client
            .propose_pause(&self.admin, &String::from_str(&self.env, "exploit"));
        self.client.approve_pause(&self.g2, &id);
        id
    }

    fn fully_unpause(&self) {
        self.client
            .request_unpause(&self.admin, &String::from_str(&self.env, "resolved"));
        self.set_time(1_000 + UNPAUSE_DELAY);
        self.client.execute_unpause(&self.admin);
    }
}

// ----------------------------------------------------------------------
// Initialization and configuration
// ----------------------------------------------------------------------

#[test]
fn initialize_sets_config_defaults() {
    let f = Fixture::new();
    let config = f.client.get_config();
    assert_eq!(config.admin, f.admin);
    assert_eq!(config.guardians.len(), 3);
    assert_eq!(config.quorum, 2);
    assert_eq!(config.unpause_delay, UNPAUSE_DELAY);
    assert_eq!(config.pause_proposal_ttl, PROPOSAL_TTL);
    assert!(!f.client.is_paused());
    assert!(f.client.get_pause_history().is_empty());
    assert!(!f.client.get_allowlist().is_empty());
}

#[test]
fn initialize_twice_fails() {
    let f = Fixture::new();
    let mut guardians = Vec::new(&f.env);
    guardians.push_back(f.admin.clone());
    assert!(f
        .client
        .try_initialize(&f.admin, &guardians, &1, &100, &100)
        .is_err());
}

#[test]
fn initialize_rejects_bad_configurations() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let g2 = Address::generate(&env);
    let id = env.register_contract(None, CircuitBreaker);
    let client = CircuitBreakerClient::new(&env, &id);

    let mut one = Vec::new(&env);
    one.push_back(admin.clone());
    // 1 of 3 is not a majority.
    let mut three = Vec::new(&env);
    three.push_back(admin.clone());
    three.push_back(g2.clone());
    three.push_back(Address::generate(&env));
    assert!(client
        .try_initialize(&admin, &three, &1, &100, &100)
        .is_err());

    // Duplicate guardians.
    let mut dupes = Vec::new(&env);
    dupes.push_back(admin.clone());
    dupes.push_back(admin.clone());
    assert!(client
        .try_initialize(&admin, &dupes, &2, &100, &100)
        .is_err());

    // Zero delay and zero TTL.
    assert!(client.try_initialize(&admin, &three, &2, &0, &100).is_err());
    assert!(client.try_initialize(&admin, &three, &2, &100, &0).is_err());

    // A single guardian with quorum 1 is valid.
    client.initialize(&admin, &one, &1, &100, &100);
    assert!(!client.is_paused());
}

#[test]
fn guardian_management_enforces_quorum() {
    let f = Fixture::new();
    assert!(f.client.is_guardian(&f.g2));

    let outsider = Address::generate(&f.env);
    assert!(f.client.try_add_guardian(&f.g2, &outsider).is_err());
    assert!(f.client.try_remove_guardian(&f.g2, &f.g3).is_err());

    // Removing one of three guardians leaves a valid 2-of-2 majority.
    f.client.remove_guardian(&f.admin, &f.g3);
    assert_eq!(f.client.get_guardians().len(), 2);
    assert!(!f.client.is_guardian(&f.g3));

    // A 2-of-2 set cannot lose another guardian (would leave quorum of 2
    // with a single member).
    assert!(f.client.try_remove_guardian(&f.admin, &f.g2).is_err());

    f.client.add_guardian(&f.admin, &f.g3);
    assert!(f.client.try_add_guardian(&f.admin, &f.g3).is_err());
    assert!(f.client.try_remove_guardian(&f.admin, &outsider).is_err());
}

#[test]
fn admin_can_update_policy() {
    let f = Fixture::new();
    assert!(f.client.try_set_quorum(&f.admin, &1).is_err());
    assert!(f.client.try_set_quorum(&f.g2, &3).is_err());
    assert!(f.client.try_set_unpause_delay(&f.admin, &0).is_err());
    assert!(f.client.try_set_pause_proposal_ttl(&f.admin, &0).is_err());

    f.client.set_quorum(&f.admin, &3);
    f.client.set_unpause_delay(&f.admin, &500);
    f.client.set_pause_proposal_ttl(&f.admin, &250);
    let config = f.client.get_config();
    assert_eq!(config.quorum, 3);
    assert_eq!(config.unpause_delay, 500);
    assert_eq!(config.pause_proposal_ttl, 250);
}

// ----------------------------------------------------------------------
// Pause requires a guardian quorum
// ----------------------------------------------------------------------

#[test]
fn single_approval_does_not_pause() {
    let f = Fixture::new();
    let id = f
        .client
        .propose_pause(&f.admin, &String::from_str(&f.env, "suspicious"));
    assert!(!f.client.is_paused());
    assert_eq!(f.client.get_status().pending_approvals, 1);

    let proposal = f.client.get_pause_proposal(id);
    assert_eq!(proposal.status, ProposalStatus::Pending);
    assert_eq!(proposal.approvals.len(), 1);
}

#[test]
fn quorum_trips_breaker_immediately() {
    let f = Fixture::new();
    let id = f.trip();
    assert!(f.client.is_paused());

    let state = f.client.get_pause_state();
    assert!(state.active);
    assert_eq!(state.pause_id, id);
    assert_eq!(state.paused_at, 1_000);
    assert_eq!(state.paused_by, Some(f.admin.clone()));
    assert_eq!(state.reason, String::from_str(&f.env, "exploit"));
    assert_eq!(state.total_pauses, 1);

    let proposal = f.client.get_pause_proposal(id);
    assert_eq!(proposal.status, ProposalStatus::Executed);
    assert_eq!(proposal.executed_at, 1_000);
    assert_eq!(f.client.get_pause_history().len(), 1);
}

#[test]
fn non_guardian_cannot_propose_or_approve() {
    let f = Fixture::new();
    let outsider = Address::generate(&f.env);
    assert!(f
        .client
        .try_propose_pause(&outsider, &String::from_str(&f.env, "nope"))
        .is_err());

    let id = f
        .client
        .propose_pause(&f.admin, &String::from_str(&f.env, "exploit"));
    assert!(f.client.try_approve_pause(&outsider, &id).is_err());
    assert!(!f.client.is_paused());
}

#[test]
fn duplicate_and_stale_approvals_are_rejected() {
    let f = Fixture::new();
    let id = f
        .client
        .propose_pause(&f.g2, &String::from_str(&f.env, "exploit"));

    // The proposer's implicit approval cannot be cast twice.
    assert!(f.client.try_approve_pause(&f.g2, &id).is_err());
    assert!(f
        .client
        .try_approve_pause(&Address::generate(&f.env), &id)
        .is_err());

    f.set_time(1_000 + PROPOSAL_TTL + 1);
    assert!(f.client.try_approve_pause(&f.admin, &id).is_err());
    assert!(!f.client.is_paused());
}

#[test]
fn only_one_pending_proposal_and_only_proposer_can_cancel() {
    let f = Fixture::new();
    let id = f
        .client
        .propose_pause(&f.g2, &String::from_str(&f.env, "first"));
    assert!(f
        .client
        .try_propose_pause(&f.admin, &String::from_str(&f.env, "second"))
        .is_err());
    assert!(f.client.try_cancel_pause_proposal(&f.admin, &id).is_err());

    f.client.cancel_pause_proposal(&f.g2, &id);
    assert_eq!(
        f.client.get_pause_proposal(id).status,
        ProposalStatus::Canceled
    );
    let next = f
        .client
        .propose_pause(&f.admin, &String::from_str(&f.env, "second"));
    assert!(f.client.try_approve_pause(&f.g2, &next).is_ok());
    assert!(f.client.is_paused());
}

// ----------------------------------------------------------------------
// Restricted callable surface while paused
// ----------------------------------------------------------------------

#[test]
fn paused_breaker_rejects_non_allowlisted_functions() {
    let f = Fixture::new();
    f.trip();

    assert!(f
        .client
        .try_assert_callable(&symbol_short!("deposit"))
        .is_err());
    assert!(f
        .client
        .try_assert_callable(&symbol_short!("withdraw"))
        .is_err());
    assert!(f
        .client
        .try_assert_callable(&symbol_short!("is_paused"))
        .is_ok());
    assert!(f
        .client
        .try_assert_callable(&symbol_short!("request_unpause"))
        .is_ok());
}

#[test]
fn allowlist_can_be_extended_by_admin_only() {
    let f = Fixture::new();
    f.trip();
    assert!(f
        .client
        .try_allow_function(&f.g2, &symbol_short!("deposit"))
        .is_err());

    f.client.allow_function(&f.admin, &symbol_short!("deposit"));
    assert!(f
        .client
        .try_assert_callable(&symbol_short!("deposit"))
        .is_ok());

    f.client
        .disallow_function(&f.admin, &symbol_short!("deposit"));
    assert!(f
        .client
        .try_assert_callable(&symbol_short!("deposit"))
        .is_err());

    // Critical unpause functions can never be removed from the allowlist.
    assert!(f
        .client
        .try_disallow_function(&f.admin, &symbol_short!("execute_unpause"))
        .is_err());
    assert!(f
        .client
        .try_disallow_function(&f.admin, &symbol_short!("is_paused"))
        .is_ok());
}

#[test]
fn when_not_paused_every_function_is_callable() {
    let f = Fixture::new();
    assert!(f
        .client
        .try_assert_callable(&symbol_short!("deposit"))
        .is_ok());
}

// ----------------------------------------------------------------------
// Delayed, admin-approved unpause
// ----------------------------------------------------------------------

#[test]
fn unpause_requires_request_admin_and_delay() {
    let f = Fixture::new();
    f.trip();

    // No request yet.
    assert!(f.client.try_execute_unpause(&f.admin).is_err());

    // Only guardians or the admin may request.
    let outsider = Address::generate(&f.env);
    assert!(f
        .client
        .try_request_unpause(&outsider, &String::from_str(&f.env, "done"))
        .is_err());

    let earliest = f
        .client
        .request_unpause(&f.g2, &String::from_str(&f.env, "fixed"));
    assert_eq!(earliest, 1_000 + UNPAUSE_DELAY);

    // Duplicate requests are rejected.
    assert!(f
        .client
        .try_request_unpause(&f.admin, &String::from_str(&f.env, "again"))
        .is_err());

    // A non-admin cannot approve, and the delay is still enforced.
    assert!(f.client.try_execute_unpause(&f.g2).is_err());
    f.set_time(1_000 + UNPAUSE_DELAY - 1);
    assert!(f.client.try_execute_unpause(&f.admin).is_err());
    assert!(f.client.is_paused());
    assert_eq!(f.client.get_status().seconds_until_unpause, 1);

    f.set_time(1_000 + UNPAUSE_DELAY);
    f.client.execute_unpause(&f.admin);
    assert!(!f.client.is_paused());

    let state = f.client.get_pause_state();
    assert_eq!(state.unpause_requested_at, 0);
    assert_eq!(state.total_pauses, 1);
    let history = f.client.get_pause_history();
    assert_eq!(history.len(), 1);
    assert_eq!(history.get(0).unwrap().unpaused_at, 1_000 + UNPAUSE_DELAY);

    // A fresh pause must get a fresh request.
    f.trip();
    assert!(f.client.try_execute_unpause(&f.admin).is_err());
}

#[test]
fn updated_delay_is_used_for_new_pause() {
    let f = Fixture::new();
    f.client.set_unpause_delay(&f.admin, &100);
    f.trip();
    assert_eq!(f.client.get_status().unpause_delay, 100);

    f.client
        .request_unpause(&f.admin, &String::from_str(&f.env, "ok"));
    f.set_time(1_000 + 100 - 1);
    assert!(f.client.try_execute_unpause(&f.admin).is_err());
    f.set_time(1_000 + 100);
    f.client.execute_unpause(&f.admin);
    assert!(!f.client.is_paused());
}

#[test]
fn cannot_pause_while_already_paused() {
    let f = Fixture::new();
    f.trip();
    assert!(f
        .client
        .try_propose_pause(&f.admin, &String::from_str(&f.env, "again"))
        .is_err());
    assert!(f
        .client
        .try_request_unpause(&f.g3, &String::from_str(&f.env, "undo"))
        .is_ok());
}

#[test]
fn request_unpause_requires_active_pause() {
    let f = Fixture::new();
    assert!(f
        .client
        .try_request_unpause(&f.admin, &String::from_str(&f.env, "why"))
        .is_err());
}

#[test]
fn circuit_can_be_tripped_more_than_once() {
    let f = Fixture::new();
    f.trip();
    f.fully_unpause();

    let second = f
        .client
        .propose_pause(&f.g3, &String::from_str(&f.env, "second incident"));
    f.client.approve_pause(&f.admin, &second);
    assert!(f.client.is_paused());
    let state = f.client.get_pause_state();
    assert_eq!(state.total_pauses, 2);
    assert_eq!(state.pause_id, second);
    assert_eq!(f.client.get_pause_history().len(), 2);
}
