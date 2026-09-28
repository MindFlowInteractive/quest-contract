#![cfg(test)]
extern crate std;
use super::*;
use soroban_sdk::{testutils::Address as _, testutils::Ledger, Address, Symbol};

const THRESHOLD: u32 = 3;
const ROUND_DURATION: u64 = 300;
const STALENESS: u64 = 300;
const HISTORY_CAP: u32 = 5;

fn slashing(penalty_bps: u32, reputation_penalty: u32) -> SlashingConfig {
    SlashingConfig {
        penalty_bps,
        reputation_penalty,
    }
}

/// Registers the contract, initialises the protocol and returns the client.
fn setup(env: &Env) -> (DataFeedClient<'static>, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, DataFeed);
    let client = DataFeedClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(
        &admin,
        &THRESHOLD,
        &ROUND_DURATION,
        &STALENESS,
        &HISTORY_CAP,
        &slashing(1_000, 10),
    );
    (client, admin)
}

fn feed_id(env: &Env, name: &str) -> Symbol {
    Symbol::new(env, name)
}

fn setup_feed(env: &Env, client: &DataFeedClient, min: i128, max: i128) -> Symbol {
    let id = feed_id(env, "XLM_USD");
    client.create_feed(&id, &Symbol::new(env, "xlm_usd"), &min, &max);
    id
}

fn add_providers(env: &Env, client: &DataFeedClient, count: u32) -> Vec<Address> {
    let mut providers = Vec::new(env);
    for _ in 0..count {
        let provider = Address::generate(env);
        client.register_provider(&provider, &1_000, &1);
        providers.push_back(provider);
    }
    providers
}

fn provider_at(providers: &Vec<Address>, index: u32) -> Address {
    providers.get(index).unwrap()
}

/// Unwraps a consensus outcome, failing the test for any other variant.
fn expect_consensus(outcome: Result<RoundOutcome, DataFeedError>) -> ConsensusRecord {
    match outcome {
        Ok(RoundOutcome::Consensus(record)) => record,
        _ => panic!("consensus expected"),
    }
}

// ------------------------------------------------------------------- initialize

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, DataFeed);
    let client = DataFeedClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin, &3, &300, &300, &5, &slashing(1_000, 10));

    let config = client.get_config();
    assert_eq!(config.admin, admin);
    assert_eq!(config.consensus_threshold, 3);
    assert_eq!(config.round_duration, 300);
    assert_eq!(config.staleness_window, 300);
    assert_eq!(config.max_history, 5);
    assert_eq!(config.slashing.penalty_bps, 1_000);
    assert_eq!(config.slashing.reputation_penalty, 10);
}

#[test]
fn test_initialize_twice_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, DataFeed);
    let client = DataFeedClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin, &3, &300, &300, &5, &slashing(1_000, 10));

    let result = client.try_initialize(&admin, &3, &300, &300, &5, &slashing(1_000, 10));
    assert_eq!(result, Err(Ok(DataFeedError::AlreadyInitialized)));
}

#[test]
fn test_initialize_validates_config() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, DataFeed);
    let client = DataFeedClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    let bad_threshold = client.try_initialize(&admin, &0, &300, &300, &5, &slashing(1_000, 10));
    assert_eq!(bad_threshold, Err(Ok(DataFeedError::InvalidConfig)));

    let bad_window = client.try_initialize(&admin, &3, &0, &300, &5, &slashing(1_000, 10));
    assert_eq!(bad_window, Err(Ok(DataFeedError::InvalidConfig)));

    let bad_staleness = client.try_initialize(&admin, &3, &300, &0, &5, &slashing(1_000, 10));
    assert_eq!(bad_staleness, Err(Ok(DataFeedError::InvalidConfig)));

    let bad_history = client.try_initialize(&admin, &3, &300, &300, &0, &slashing(1_000, 10));
    assert_eq!(bad_history, Err(Ok(DataFeedError::InvalidConfig)));

    let huge_history = client.try_initialize(&admin, &3, &300, &300, &101, &slashing(1_000, 10));
    assert_eq!(huge_history, Err(Ok(DataFeedError::InvalidConfig)));

    let bad_penalty = client.try_initialize(&admin, &3, &300, &300, &5, &slashing(10_001, 10));
    assert_eq!(bad_penalty, Err(Ok(DataFeedError::InvalidPenalty)));

    // None of the rejected calls mutated the state.
    client.initialize(&admin, &3, &300, &300, &5, &slashing(1_000, 10));
    assert_eq!(client.get_config().consensus_threshold, 3);
}

#[test]
fn test_calls_before_initialize_are_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, DataFeed);
    let client = DataFeedClient::new(&env, &contract_id);

    let result = client.try_get_analytics();
    assert_eq!(result, Err(Ok(DataFeedError::NotInitialized)));
}

// ----------------------------------------------------------- provider registry

#[test]
fn test_register_and_deregister_provider() {
    let env = Env::default();
    let (client, _admin) = setup(&env);
    env.ledger().set_timestamp(1_000);

    let provider = Address::generate(&env);
    client.register_provider(&provider, &5_000, &2);

    let record = client.get_provider(&provider);
    assert_eq!(record.address, provider);
    assert_eq!(record.weight, 2);
    assert_eq!(record.stake, 5_000);
    assert_eq!(record.reputation, INITIAL_REPUTATION);
    assert!(record.active);
    assert_eq!(record.submissions, 0);
    assert_eq!(record.slash_count, 0);
    assert_eq!(record.registered_at, 1_000);

    assert_eq!(client.get_providers().len(), 1);

    // Duplicate registration is rejected.
    let duplicate = client.try_register_provider(&provider, &5_000, &2);
    assert_eq!(duplicate, Err(Ok(DataFeedError::ProviderAlreadyExists)));

    // Zero weight and negative stake are rejected.
    let zero_weight = client.try_register_provider(&Address::generate(&env), &100, &0);
    assert_eq!(zero_weight, Err(Ok(DataFeedError::InvalidWeight)));

    let negative_stake = client.try_register_provider(&Address::generate(&env), &-1, &1);
    assert_eq!(negative_stake, Err(Ok(DataFeedError::InvalidStake)));

    client.deregister_provider(&provider);
    assert!(!client.get_provider(&provider).active);
    assert_eq!(client.get_analytics().active_providers, 0);
    // The bond stays on record after deregistration.
    assert_eq!(client.get_provider(&provider).stake, 5_000);

    let again = client.try_deregister_provider(&provider);
    assert_eq!(again, Err(Ok(DataFeedError::ProviderNotActive)));

    let unknown = client.try_deregister_provider(&Address::generate(&env));
    assert_eq!(unknown, Err(Ok(DataFeedError::ProviderNotFound)));
}

#[test]
fn test_register_provider_requires_admin() {
    // No auths are mocked, so the admin signature cannot be provided.
    let env = Env::default();

    let contract_id = env.register_contract(None, DataFeed);
    let client = DataFeedClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &3, &300, &300, &5, &slashing(1_000, 10));

    let provider = Address::generate(&env);
    let result = client.try_register_provider(&provider, &1_000, &1);
    assert!(result.is_err());
    // The rejected call left no trace.
    let missing = client.try_get_provider(&provider);
    assert_eq!(missing, Err(Ok(DataFeedError::ProviderNotFound)));
}

#[test]
fn test_set_provider_weight() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let provider = Address::generate(&env);
    client.register_provider(&provider, &1_000, &1);

    client.set_provider_weight(&provider, &7);
    assert_eq!(client.get_provider(&provider).weight, 7);

    let zero = client.try_set_provider_weight(&provider, &0);
    assert_eq!(zero, Err(Ok(DataFeedError::InvalidWeight)));

    let unknown = client.try_set_provider_weight(&Address::generate(&env), &3);
    assert_eq!(unknown, Err(Ok(DataFeedError::ProviderNotFound)));
}

#[test]
fn test_inactive_provider_cannot_submit() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let id = setup_feed(&env, &client, 0, 1_000);
    let provider = Address::generate(&env);
    client.register_provider(&provider, &1_000, &1);
    client.deregister_provider(&provider);

    let result = client.try_submit_data(&id, &provider, &100);
    assert_eq!(result, Err(Ok(DataFeedError::ProviderNotActive)));

    let unknown = client.try_submit_data(&id, &Address::generate(&env), &100);
    assert_eq!(unknown, Err(Ok(DataFeedError::ProviderNotFound)));
}

// ------------------------------------------------------------------ feed setup

#[test]
fn test_create_feed() {
    let env = Env::default();
    let (client, _admin) = setup(&env);
    env.ledger().set_timestamp(500);

    let id = feed_id(&env, "XLM_USD");
    let question = Symbol::new(&env, "xlm_usd");
    client.create_feed(&id, &question, &10, &2_000);

    let feed = client.get_feed(&id);
    assert_eq!(feed.feed_id, id);
    assert_eq!(feed.question, question);
    assert_eq!(feed.min_value, 10);
    assert_eq!(feed.max_value, 2_000);
    assert_eq!(feed.round, 1);
    assert_eq!(feed.round_start, 500);
    assert_eq!(feed.submission_count, 0);
    assert_eq!(feed.consensus_value, None);
    assert_eq!(feed.finalized_rounds, 0);
    assert_eq!(feed.created_at, 500);

    assert_eq!(client.feed_count(), 1);
    assert_eq!(client.get_analytics().feed_count, 1);

    let duplicate = client.try_create_feed(&id, &question, &10, &2_000);
    assert_eq!(duplicate, Err(Ok(DataFeedError::FeedAlreadyExists)));

    let inverted = client.try_create_feed(
        &feed_id(&env, "BAD"),
        &Symbol::new(&env, "bad"),
        &2_000,
        &10,
    );
    assert_eq!(inverted, Err(Ok(DataFeedError::InvalidBounds)));

    let missing = client.try_get_feed(&feed_id(&env, "NOPE"));
    assert_eq!(missing, Err(Ok(DataFeedError::FeedNotFound)));
}

// ----------------------------------------------------------------- submissions

#[test]
fn test_submit_data() {
    let env = Env::default();
    let (client, _admin) = setup(&env);
    env.ledger().set_timestamp(1_000);

    let id = setup_feed(&env, &client, 0, 1_000);
    let provider = Address::generate(&env);
    client.register_provider(&provider, &1_000, &3);

    client.submit_data(&id, &provider, &250);

    let submission = client.get_submission(&id, &provider);
    assert_eq!(submission.provider, provider);
    assert_eq!(submission.value, 250);
    assert_eq!(submission.timestamp, 1_000);
    assert_eq!(submission.round, 1);
    assert_eq!(submission.weight, 3);

    let record = client.get_provider(&provider);
    assert_eq!(record.submissions, 1);
    assert_eq!(record.last_submission, 1_000);

    let feed = client.get_feed(&id);
    assert_eq!(feed.submission_count, 1);
    assert_eq!(feed.submission_weight, 3);
    assert_eq!(feed.total_submissions, 1);

    assert_eq!(client.get_analytics().total_submissions, 1);

    // One answer per provider per round.
    let repeated = client.try_submit_data(&id, &provider, &300);
    assert_eq!(repeated, Err(Ok(DataFeedError::AlreadySubmitted)));
}

#[test]
fn test_submit_data_rejects_out_of_bounds() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let id = setup_feed(&env, &client, 100, 1_000);
    let provider = Address::generate(&env);
    client.register_provider(&provider, &1_000, &1);

    let too_low = client.try_submit_data(&id, &provider, &99);
    assert_eq!(too_low, Err(Ok(DataFeedError::ValueOutOfBounds)));

    let too_high = client.try_submit_data(&id, &provider, &1_001);
    assert_eq!(too_high, Err(Ok(DataFeedError::ValueOutOfBounds)));

    // The bounds themselves are accepted.
    client.submit_data(&id, &provider, &100);
    client.submit_data(&id, &provider, &1_000);
    assert_eq!(client.get_feed(&id).submission_count, 2);
}

#[test]
fn test_submission_window_is_enforced() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    env.ledger().set_timestamp(1_000);
    let id = setup_feed(&env, &client, 0, 1_000);
    let provider = Address::generate(&env);
    client.register_provider(&provider, &1_000, &1);

    client.submit_data(&id, &provider, &100);

    // The round is still open one second before the window closes.
    env.ledger().set_timestamp(1_000 + ROUND_DURATION - 1);
    assert!(!client.is_consensus_reached(&id));

    // Once the window closed the round rejects further submissions.
    env.ledger().set_timestamp(1_000 + ROUND_DURATION);
    let closed = client.try_submit_data(&id, &provider, &200);
    assert_eq!(closed, Err(Ok(DataFeedError::SubmissionWindowClosed)));

    // Finalizing opens the next round at the current timestamp.
    assert_eq!(client.finalize(&id), Ok(RoundOutcome::NoConsensus));
    env.ledger().set_timestamp(1_000 + ROUND_DURATION + 10);
    client.submit_data(&id, &provider, &200);
    let submission = client.get_submission(&id, &provider);
    assert_eq!(submission.round, 2);
    assert_eq!(submission.value, 200);
}

// -------------------------------------------------------------------- consensus

#[test]
fn test_consensus_reached_exactly_at_threshold() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    env.ledger().set_timestamp(1_000);
    let id = setup_feed(&env, &client, 0, 10_000);
    let providers = add_providers(&env, &client, THRESHOLD);

    // One below the threshold the round is still pending.
    client.submit_data(&id, &provider_at(&providers, 0), &100);
    client.submit_data(&id, &provider_at(&providers, 1), &200);
    assert_eq!(client.finalize(&id), Ok(RoundOutcome::Pending));
    assert!(!client.is_consensus_reached(&id));

    // The third submission reaches the threshold exactly.
    client.submit_data(&id, &provider_at(&providers, 2), &300);
    assert!(client.is_consensus_reached(&id));

    let record = expect_consensus(client.finalize(&id));
    assert_eq!(record.round, 1);
    assert_eq!(record.value, 200);
    assert_eq!(record.contributors, THRESHOLD);
    assert_eq!(record.weight, 3);
    assert_eq!(record.timestamp, 1_000);

    let (value, round, timestamp) = client.get_consensus_value(&id);
    assert_eq!(value, 200);
    assert_eq!(round, 1);
    assert_eq!(timestamp, 1_000);

    // The round rolled over and the submissions were cleared.
    let feed = client.get_feed(&id);
    assert_eq!(feed.round, 2);
    assert_eq!(feed.submission_count, 0);
    assert_eq!(feed.submission_weight, 0);
    assert_eq!(feed.consensus_round, 1);
    assert_eq!(feed.consensus_contributors, THRESHOLD);
    assert_eq!(feed.finalized_rounds, 1);

    let consensus = client.get_consensus(&id);
    assert_eq!(consensus.value, 200);
    assert_eq!(consensus.contributors, THRESHOLD);
    assert!(!consensus.is_stale);

    assert_eq!(client.get_analytics().finalized_rounds, 1);
}

#[test]
fn test_consensus_uses_provider_weights() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let id = setup_feed(&env, &client, 0, 10_000);
    let whale = Address::generate(&env);
    let small_a = Address::generate(&env);
    let small_b = Address::generate(&env);
    client.register_provider(&whale, &1_000, &10);
    client.register_provider(&small_a, &1_000, &1);
    client.register_provider(&small_b, &1_000, &1);

    client.submit_data(&id, &small_a, &100);
    client.submit_data(&id, &small_b, &200);
    client.submit_data(&id, &whale, &500);

    let record = expect_consensus(client.finalize(&id));
    // Sorted values are 100 (w1), 200 (w1) and 500 (w10); half of the total
    // weight of 12 is 6, so the weighted median is 500.
    assert_eq!(record.value, 500);
    assert_eq!(record.weight, 12);
}

#[test]
fn test_consensus_not_reached_below_threshold() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    env.ledger().set_timestamp(1_000);
    let id = setup_feed(&env, &client, 0, 10_000);
    let providers = add_providers(&env, &client, 2);

    client.submit_data(&id, &provider_at(&providers, 0), &100);
    client.submit_data(&id, &provider_at(&providers, 1), &200);

    // The window is still open, so nothing happened yet.
    assert_eq!(client.finalize(&id), Ok(RoundOutcome::Pending));
    assert_eq!(client.get_feed(&id).failed_rounds, 0);
    let early = client.try_get_consensus_value(&id);
    assert_eq!(early, Err(Ok(DataFeedError::NoConsensusValue)));
    let early_flag = client.try_get_consensus(&id);
    assert_eq!(early_flag, Err(Ok(DataFeedError::NoConsensusValue)));

    // The window closed without the threshold: the round fails and no value is
    // stored.
    env.ledger().set_timestamp(1_000 + ROUND_DURATION);
    assert_eq!(client.finalize(&id), Ok(RoundOutcome::NoConsensus));

    let feed = client.get_feed(&id);
    assert_eq!(feed.round, 2);
    assert_eq!(feed.failed_rounds, 1);
    assert_eq!(feed.finalized_rounds, 0);
    assert_eq!(feed.consensus_value, None);
    assert_eq!(feed.total_submissions, 2);

    assert_eq!(client.get_analytics().failed_rounds, 1);
    assert_eq!(client.get_analytics().finalized_rounds, 0);
    assert_eq!(client.get_history(&id, &10).len(), 0);
    let missing = client.try_get_round(&id, &1);
    assert_eq!(missing, Err(Ok(DataFeedError::RoundNotFound)));
}

#[test]
fn test_failed_round_does_not_carry_submissions_over() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    env.ledger().set_timestamp(1_000);
    let id = setup_feed(&env, &client, 0, 10_000);
    let providers = add_providers(&env, &client, THRESHOLD);

    client.submit_data(&id, &provider_at(&providers, 0), &100);
    env.ledger().set_timestamp(1_000 + ROUND_DURATION);
    assert_eq!(client.finalize(&id), Ok(RoundOutcome::NoConsensus));

    // The next round starts empty and can reach consensus.
    env.ledger().set_timestamp(1_000 + ROUND_DURATION + 1);
    client.submit_data(&id, &provider_at(&providers, 0), &400);
    client.submit_data(&id, &provider_at(&providers, 1), &400);
    client.submit_data(&id, &provider_at(&providers, 2), &400);

    let record = expect_consensus(client.finalize(&id));
    assert_eq!(record.round, 2);
    assert_eq!(record.value, 400);
    assert_eq!(record.contributors, THRESHOLD);
}

// -------------------------------------------------------------------- staleness

#[test]
fn test_stale_consensus_value_is_reported() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    env.ledger().set_timestamp(1_000);
    let id = setup_feed(&env, &client, 0, 10_000);
    let providers = add_providers(&env, &client, THRESHOLD);
    client.submit_data(&id, &provider_at(&providers, 0), &100);
    client.submit_data(&id, &provider_at(&providers, 1), &100);
    client.submit_data(&id, &provider_at(&providers, 2), &100);
    expect_consensus(client.finalize(&id));

    // Exactly on the boundary the value is still fresh.
    env.ledger().set_timestamp(1_000 + STALENESS);
    assert!(!client.is_stale(&id));
    assert!(!client.get_consensus(&id).is_stale);
    assert_eq!(client.get_consensus_value(&id).0, 100);

    // One second later the value is stale and strict reads fail.
    env.ledger().set_timestamp(1_000 + STALENESS + 1);
    assert!(client.is_stale(&id));
    assert!(client.get_consensus(&id).is_stale);
    let strict = client.try_get_consensus_value(&id);
    assert_eq!(strict, Err(Ok(DataFeedError::StaleData)));

    let stats = client.get_feed_stats(&id);
    assert!(stats.has_consensus);
    assert!(stats.is_stale);

    // Rolling the round over and finalizing again refreshes the value.
    assert_eq!(client.finalize(&id), Ok(RoundOutcome::NoConsensus));
    client.submit_data(&id, &provider_at(&providers, 0), &120);
    client.submit_data(&id, &provider_at(&providers, 1), &120);
    client.submit_data(&id, &provider_at(&providers, 2), &120);
    expect_consensus(client.finalize(&id));
    assert!(!client.is_stale(&id));
    assert_eq!(client.get_consensus_value(&id).0, 120);
}

// ---------------------------------------------------------------------- history

#[test]
fn test_history_accumulates_and_is_bounded() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    env.ledger().set_timestamp(1_000);
    let id = setup_feed(&env, &client, 0, 10_000);
    let providers = add_providers(&env, &client, THRESHOLD);

    // Seven rounds with a history cap of five.
    for round in 1..=7u32 {
        let value = 100 + round as i128;
        client.submit_data(&id, &provider_at(&providers, 0), &value);
        client.submit_data(&id, &provider_at(&providers, 1), &value);
        client.submit_data(&id, &provider_at(&providers, 2), &value);
        expect_consensus(client.finalize(&id));
        env.ledger().set_timestamp(env.ledger().timestamp() + 1);
    }

    assert_eq!(client.get_feed(&id).finalized_rounds, 7);

    // Only the most recent five rounds are retained.
    let history = client.get_history(&id, &100);
    assert_eq!(history.len(), 5);
    assert_eq!(history.get(0).unwrap().round, 3);
    assert_eq!(history.get(4).unwrap().round, 7);
    assert_eq!(history.get(4).unwrap().value, 107);

    // The caller can ask for fewer entries.
    let tail = client.get_history(&id, &2);
    assert_eq!(tail.len(), 2);
    assert_eq!(tail.get(0).unwrap().round, 6);
    assert_eq!(client.get_history(&id, &0).len(), 0);

    // Evicted rounds are gone, retained rounds stay readable.
    let evicted = client.try_get_round(&id, &1);
    assert_eq!(evicted, Err(Ok(DataFeedError::RoundNotFound)));
    let kept = client.get_round(&id, &7);
    assert_eq!(kept.value, 107);
    assert_eq!(kept.contributors, THRESHOLD);

    assert_eq!(client.get_analytics().finalized_rounds, 7);
    assert_eq!(client.get_feed_stats(&id).history_len, 5);
}

// --------------------------------------------------------------------- slashing

#[test]
fn test_slash_reduces_stake_and_reputation() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let provider = Address::generate(&env);
    client.register_provider(&provider, &1_000, &1);

    let reason = Symbol::new(&env, "manipulate");
    let burned = client.slash_provider(&provider, &reason);
    assert_eq!(burned, 100);

    let record = client.get_provider(&provider);
    assert_eq!(record.stake, 900);
    assert_eq!(record.reputation, INITIAL_REPUTATION - 10);
    assert_eq!(record.slash_count, 1);

    assert_eq!(client.get_analytics().slash_events, 1);
    assert_eq!(client.get_analytics().total_stake, 900);

    let unknown = client.try_slash_provider(&Address::generate(&env), &reason);
    assert_eq!(unknown, Err(Ok(DataFeedError::ProviderNotFound)));
}

#[test]
fn test_slashing_never_goes_below_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, DataFeed);
    let client = DataFeedClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    // Burn the whole bond, then the whole reputation.
    client.initialize(&admin, &1, &300, &300, &5, &slashing(10_000, 50));

    let provider = Address::generate(&env);
    client.register_provider(&provider, &50, &1);
    let reason = Symbol::new(&env, "slash_all");

    let burned = client.slash_provider(&provider, &reason);
    assert_eq!(burned, 50);
    let record = client.get_provider(&provider);
    assert_eq!(record.stake, 0);
    assert_eq!(record.reputation, INITIAL_REPUTATION - 50);
    assert_eq!(record.slash_count, 1);

    // A further slash burns no stake, the balance stays clamped at zero.
    let again = client.slash_provider(&provider, &reason);
    assert_eq!(again, 0);
    let record = client.get_provider(&provider);
    assert_eq!(record.stake, 0);
    assert_eq!(record.reputation, 0);
    assert_eq!(record.slash_count, 2);
    assert_eq!(client.get_analytics().total_stake, 0);
    assert_eq!(client.get_analytics().slash_events, 2);

    // Nothing is left to take.
    let exhausted = client.try_slash_provider(&provider, &reason);
    assert_eq!(exhausted, Err(Ok(DataFeedError::NothingToSlash)));
}

#[test]
fn test_report_bad_data_slashes_the_offender() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let id = setup_feed(&env, &client, 100, 1_000);
    let challenger = Address::generate(&env);
    let offender = Address::generate(&env);
    client.register_provider(&challenger, &1_000, &1);
    client.register_provider(&offender, &1_000, &1);

    // A value inside the bounds cannot be proven to be a violation.
    let not_a_violation = client.try_report_bad_data(&id, &challenger, &offender, &500);
    assert_eq!(not_a_violation, Err(Ok(DataFeedError::NoViolationProven)));

    // An out of bounds value is proven by the contract itself.
    let burned = client.report_bad_data(&id, &challenger, &offender, &5_000);
    assert_eq!(burned, 100);
    assert_eq!(client.get_provider(&offender).stake, 900);
    assert_eq!(client.get_provider(&offender).slash_count, 1);

    assert_eq!(client.get_feed(&id).rejected_submissions, 1);
    let analytics = client.get_analytics();
    assert_eq!(analytics.rejected_submissions, 1);
    assert_eq!(analytics.slash_events, 1);
    assert_eq!(analytics.total_stake, 1_900);

    // Only registered providers can challenge.
    let stranger = client.try_report_bad_data(&id, &Address::generate(&env), &offender, &5_000);
    assert_eq!(stranger, Err(Ok(DataFeedError::Unauthorized)));

    // Providers cannot report themselves.
    let selfish = client.try_report_bad_data(&id, &offender, &offender, &5_000);
    assert_eq!(selfish, Err(Ok(DataFeedError::Unauthorized)));

    // A deregistered provider cannot be challenged either.
    client.deregister_provider(&offender);
    let inactive = client.try_report_bad_data(&id, &challenger, &offender, &5_000);
    assert_eq!(inactive, Err(Ok(DataFeedError::ProviderNotActive)));
}

// -------------------------------------------------------------------- analytics

#[test]
fn test_analytics_counters() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    env.ledger().set_timestamp(1_000);
    let providers = add_providers(&env, &client, 3);
    let extra = Address::generate(&env);
    client.register_provider(&extra, &500, &2);
    client.deregister_provider(&extra);

    let id = setup_feed(&env, &client, 0, 10_000);
    let second = feed_id(&env, "BTC_USD");
    client.create_feed(&second, &Symbol::new(&env, "btc_usd"), &0, &100_000);

    // Round 1 reaches consensus, round 2 fails, round 3 records a challenge.
    client.submit_data(&id, &provider_at(&providers, 0), &100);
    client.submit_data(&id, &provider_at(&providers, 1), &100);
    client.submit_data(&id, &provider_at(&providers, 2), &100);
    expect_consensus(client.finalize(&id));

    env.ledger().set_timestamp(1_100);
    client.submit_data(&id, &provider_at(&providers, 0), &200);
    env.ledger().set_timestamp(1_100 + ROUND_DURATION);
    assert_eq!(client.finalize(&id), Ok(RoundOutcome::NoConsensus));

    client.submit_data(&id, &provider_at(&providers, 0), &9_999);
    let offender = provider_at(&providers, 0);
    let challenger = provider_at(&providers, 1);
    client.report_bad_data(&id, &challenger, &offender, &20_000);

    let analytics = client.get_analytics();
    assert_eq!(analytics.feed_count, 2);
    assert_eq!(analytics.provider_count, 4);
    assert_eq!(analytics.active_providers, 3);
    assert_eq!(analytics.total_submissions, 5);
    assert_eq!(analytics.finalized_rounds, 1);
    assert_eq!(analytics.failed_rounds, 1);
    assert_eq!(analytics.rejected_submissions, 1);
    assert_eq!(analytics.slash_events, 1);
    assert_eq!(analytics.total_stake, 3_400);

    assert_eq!(client.feed_count(), 2);

    let stats = client.get_feed_stats(&id);
    assert_eq!(stats.feed_id, id);
    assert_eq!(stats.current_round, 3);
    assert_eq!(stats.round_submissions, 1);
    assert_eq!(stats.round_submission_weight, 1);
    assert!(!stats.consensus_reached);
    assert!(stats.has_consensus);
    // The consensus value dates from t=1000 and the window is 300 seconds.
    assert!(stats.is_stale);
    assert_eq!(stats.total_submissions, 5);
    assert_eq!(stats.finalized_rounds, 1);
    assert_eq!(stats.failed_rounds, 1);
    assert_eq!(stats.rejected_submissions, 1);
    assert_eq!(stats.history_len, 1);

    let other = client.get_feed_stats(&second);
    assert_eq!(other.total_submissions, 0);
    assert!(!other.has_consensus);
}

#[test]
fn test_full_lifecycle() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    env.ledger().set_timestamp(1_000);
    let id = setup_feed(&env, &client, 0, 10_000);
    let providers = add_providers(&env, &client, THRESHOLD);

    client.submit_data(&id, &provider_at(&providers, 0), &100);
    client.submit_data(&id, &provider_at(&providers, 1), &110);
    client.submit_data(&id, &provider_at(&providers, 2), &900);
    let record = expect_consensus(client.finalize(&id));
    assert_eq!(record.value, 110);

    let history = client.get_history(&id, &10);
    assert_eq!(history.len(), 1);
    assert_eq!(history.get(0).unwrap().value, 110);

    assert_eq!(client.get_consensus_value(&id), (110, 1, 1_000));
}
