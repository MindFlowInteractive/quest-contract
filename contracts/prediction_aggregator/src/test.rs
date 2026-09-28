#![cfg(test)]
extern crate std;
use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, String, Symbol, TryFromVal, Vec,
};

const DEFAULT_WEIGHT: u32 = 2_500;
const MIN_SOURCES: u32 = 2;
const OUTLIER_BPS: u32 = 1_500;
const MIN_CONFIDENCE: u32 = 4_000;
const HISTORY_CAP: u32 = 3;
const MAX_CORRECTION: u32 = 2_000;
const TOLERANCE: u32 = 1_000;

fn bias(max_correction_bps: u32, accuracy_tolerance_bps: u32) -> BiasConfig {
    BiasConfig {
        max_correction_bps,
        accuracy_tolerance_bps,
    }
}

/// Registers the contract, initialises the protocol and returns the client.
fn setup(env: &Env) -> (PredictionAggregatorClient<'static>, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, PredictionAggregator);
    let client = PredictionAggregatorClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(
        &admin,
        &DEFAULT_WEIGHT,
        &MIN_SOURCES,
        &OUTLIER_BPS,
        &MIN_CONFIDENCE,
        &HISTORY_CAP,
        &bias(MAX_CORRECTION, TOLERANCE),
    );
    (client, admin)
}

/// Same as [`setup`] but with a different bias clamp, for the clamp test.
fn setup_with_correction(env: &Env, max_correction_bps: u32) -> (PredictionAggregatorClient<'static>, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, PredictionAggregator);
    let client = PredictionAggregatorClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(
        &admin,
        &DEFAULT_WEIGHT,
        &MIN_SOURCES,
        &OUTLIER_BPS,
        &MIN_CONFIDENCE,
        &HISTORY_CAP,
        &bias(max_correction_bps, TOLERANCE),
    );
    (client, admin)
}

fn question(env: &Env, text: &str) -> String {
    String::from_str(env, text)
}

fn two_weights(env: &Env, a: &Address, wa: u32, b: &Address, wb: u32) -> Vec<SourceWeight> {
    let mut result: Vec<SourceWeight> = Vec::new(env);
    result.push_back(SourceWeight {
        source: a.clone(),
        weight_bps: wa,
    });
    result.push_back(SourceWeight {
        source: b.clone(),
        weight_bps: wb,
    });
    result
}

fn three_weights(
    env: &Env,
    a: &Address,
    wa: u32,
    b: &Address,
    wb: u32,
    c: &Address,
    wc: u32,
) -> Vec<SourceWeight> {
    let mut result: Vec<SourceWeight> = Vec::new(env);
    result.push_back(SourceWeight {
        source: a.clone(),
        weight_bps: wa,
    });
    result.push_back(SourceWeight {
        source: b.clone(),
        weight_bps: wb,
    });
    result.push_back(SourceWeight {
        source: c.clone(),
        weight_bps: wc,
    });
    result
}

fn register(env: &Env, client: &PredictionAggregatorClient, count: u32) -> Vec<Address> {
    let mut sources: Vec<Address> = Vec::new(env);
    for _ in 0..count {
        let source = Address::generate(env);
        client.register_source(&source);
        sources.push_back(source);
    }
    sources
}

fn at(sources: &Vec<Address>, index: u32) -> Address {
    sources.get(index).unwrap()
}

/// Opens a market with the default outcome range and source requirement.
fn open_market(
    env: &Env,
    client: &PredictionAggregatorClient,
    name: &str,
    deadline: u64,
) -> Symbol {
    let id = Symbol::new(env, name);
    client.create_prediction_market(
        &id,
        &question(env, "Will XLM close above 1 USD this year?"),
        &Symbol::new(env, "XLM_PX"),
        &deadline,
        &0,
        &10_000,
        &MIN_SOURCES,
    );
    id
}

/// Every event topic that is a symbol, e.g. `src_add`, `consen`.
fn topic_symbols(env: &Env) -> Vec<Symbol> {
    let published = env.events().all();
    let mut names: Vec<Symbol> = Vec::new(env);
    for (_, topics, _) in published.iter() {
        for topic in topics.iter() {
            if let Ok(symbol) = Symbol::try_from_val(env, &topic) {
                names.push_back(symbol);
            }
        }
    }
    names
}

fn has_topic(topics: &Vec<Symbol>, expected: Symbol) -> bool {
    for topic in topics.iter() {
        if topic == expected {
            return true;
        }
    }
    false
}

// ------------------------------------------------------------------- initialize

#[test]
fn test_initialize() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    let config = client.get_config();
    assert_eq!(config.admin, admin);
    assert_eq!(config.default_source_weight, DEFAULT_WEIGHT);
    assert_eq!(config.min_sources, MIN_SOURCES);
    assert_eq!(config.outlier_threshold_bps, OUTLIER_BPS);
    assert_eq!(config.min_confidence_bps, MIN_CONFIDENCE);
    assert_eq!(config.history_cap, HISTORY_CAP);
    assert_eq!(config.bias.max_correction_bps, MAX_CORRECTION);
    assert_eq!(config.bias.accuracy_tolerance_bps, TOLERANCE);

    let analytics = client.get_analytics();
    assert_eq!(analytics.market_count, 0);
    assert_eq!(analytics.source_count, 0);
    assert_eq!(analytics.active_sources, 0);
    assert_eq!(analytics.total_predictions, 0);
    assert_eq!(analytics.published_forecasts, 0);
    assert_eq!(analytics.sources.len(), 0);
    assert_eq!(client.market_count(), 0);
    assert_eq!(client.total_weight(), 0);
}

#[test]
fn test_initialize_twice_is_rejected() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let result = client.try_initialize(
        &Address::generate(&env),
        &DEFAULT_WEIGHT,
        &MIN_SOURCES,
        &OUTLIER_BPS,
        &MIN_CONFIDENCE,
        &HISTORY_CAP,
        &bias(MAX_CORRECTION, TOLERANCE),
    );
    assert_eq!(result, Err(Ok(AggregatorError::AlreadyInitialized)));
}

#[test]
fn test_initialize_validates_config() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, PredictionAggregator);
    let client = PredictionAggregatorClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    let zero_weight = client.try_initialize(
        &admin,
        &0,
        &MIN_SOURCES,
        &OUTLIER_BPS,
        &MIN_CONFIDENCE,
        &HISTORY_CAP,
        &bias(MAX_CORRECTION, TOLERANCE),
    );
    assert_eq!(zero_weight, Err(Ok(AggregatorError::InvalidConfig)));

    let zero_sources = client.try_initialize(
        &admin,
        &DEFAULT_WEIGHT,
        &0,
        &OUTLIER_BPS,
        &MIN_CONFIDENCE,
        &HISTORY_CAP,
        &bias(MAX_CORRECTION, TOLERANCE),
    );
    assert_eq!(zero_sources, Err(Ok(AggregatorError::InvalidConfig)));

    let bad_threshold = client.try_initialize(
        &admin,
        &DEFAULT_WEIGHT,
        &MIN_SOURCES,
        &10_001,
        &MIN_CONFIDENCE,
        &HISTORY_CAP,
        &bias(MAX_CORRECTION, TOLERANCE),
    );
    assert_eq!(bad_threshold, Err(Ok(AggregatorError::InvalidConfig)));

    let bad_confidence = client.try_initialize(
        &admin,
        &DEFAULT_WEIGHT,
        &MIN_SOURCES,
        &OUTLIER_BPS,
        &10_001,
        &HISTORY_CAP,
        &bias(MAX_CORRECTION, TOLERANCE),
    );
    assert_eq!(bad_confidence, Err(Ok(AggregatorError::InvalidConfig)));

    let no_history = client.try_initialize(
        &admin,
        &DEFAULT_WEIGHT,
        &MIN_SOURCES,
        &OUTLIER_BPS,
        &MIN_CONFIDENCE,
        &0,
        &bias(MAX_CORRECTION, TOLERANCE),
    );
    assert_eq!(no_history, Err(Ok(AggregatorError::InvalidConfig)));

    let bad_correction = client.try_initialize(
        &admin,
        &DEFAULT_WEIGHT,
        &MIN_SOURCES,
        &OUTLIER_BPS,
        &MIN_CONFIDENCE,
        &HISTORY_CAP,
        &bias(10_001, TOLERANCE),
    );
    assert_eq!(bad_correction, Err(Ok(AggregatorError::InvalidConfig)));

    // None of the rejected calls installed a config.
    assert_eq!(client.try_get_config(), Err(Ok(AggregatorError::NotInitialized)));
}

#[test]
fn test_admin_can_retune_thresholds() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    client.set_min_confidence(&admin, &9_000);
    client.set_outlier_threshold(&admin, &250);
    let config = client.get_config();
    assert_eq!(config.min_confidence_bps, 9_000);
    assert_eq!(config.outlier_threshold_bps, 250);

    let stranger = Address::generate(&env);
    let bad = client.try_set_min_confidence(&stranger, &1_000);
    assert_eq!(bad, Err(Ok(AggregatorError::Unauthorized)));
    let over = client.try_set_outlier_threshold(&admin, &10_001);
    assert_eq!(over, Err(Ok(AggregatorError::InvalidConfig)));
}

// --------------------------------------------------------------- source registry

#[test]
fn test_register_and_deregister_source() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, _admin) = setup(&env);

    let source = Address::generate(&env);
    client.register_source(&source);

    let record = client.get_source(&source);
    assert_eq!(record.address, source);
    assert_eq!(record.weight_bps, DEFAULT_WEIGHT);
    assert!(record.active);
    assert_eq!(record.registered_at, 1_000);
    assert_eq!(record.predictions, 0);
    assert_eq!(record.resolved, 0);
    assert_eq!(record.bias_bps, 0);

    assert_eq!(client.get_sources().len(), 1);
    assert_eq!(client.total_weight(), DEFAULT_WEIGHT);

    let duplicate = client.try_register_source(&source);
    assert_eq!(duplicate, Err(Ok(AggregatorError::SourceAlreadyExists)));

    client.deregister_source(&source);
    assert!(!client.get_source(&source).active);
    assert_eq!(client.get_sources().len(), 0);
    assert_eq!(client.total_weight(), 0);

    let again = client.try_deregister_source(&source);
    assert_eq!(again, Err(Ok(AggregatorError::SourceNotActive)));

    let unknown = client.try_deregister_source(&Address::generate(&env));
    assert_eq!(unknown, Err(Ok(AggregatorError::SourceNotFound)));

    // A source keeps its track record after leaving the panel.
    let record = client.get_source(&source);
    assert_eq!(record.weight_bps, DEFAULT_WEIGHT);
}

#[test]
fn test_set_source_weights_requires_fair_total() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let sources = register(&env, &client, 3);
    let (s0, s1, s2) = (at(&sources, 0), at(&sources, 1), at(&sources, 2));

    // 5_000 + 3_000 + 2_000 == 10_000, accepted.
    let fair = three_weights(&env, &s0, 5_000, &s1, 3_000, &s2, 2_000);
    assert_eq!(client.set_source_weights(&fair, &admin), 10_000);
    assert_eq!(client.get_source(&s0).weight_bps, 5_000);
    assert_eq!(client.get_source(&s1).weight_bps, 3_000);
    assert_eq!(client.get_source(&s2).weight_bps, 2_000);
    assert_eq!(client.total_weight(), 10_000);

    // Too much weight.
    let over = three_weights(&env, &s0, 6_000, &s1, 3_000, &s2, 2_000);
    assert_eq!(
        client.try_set_source_weights(&over, &admin),
        Err(Ok(AggregatorError::InvalidWeight))
    );

    // Too little weight.
    let under = three_weights(&env, &s0, 4_000, &s1, 3_000, &s2, 2_000);
    assert_eq!(
        client.try_set_source_weights(&under, &admin),
        Err(Ok(AggregatorError::InvalidWeight))
    );

    // A zero weight would silence a source, weights must stay positive.
    let zero = three_weights(&env, &s0, 10_000, &s1, 0, &s2, 0);
    assert_eq!(
        client.try_set_source_weights(&zero, &admin),
        Err(Ok(AggregatorError::InvalidWeight))
    );

    // The same source twice.
    let mut duplicated: Vec<SourceWeight> = Vec::new(&env);
    duplicated.push_back(SourceWeight {
        source: s0.clone(),
        weight_bps: 5_000,
    });
    duplicated.push_back(SourceWeight {
        source: s0.clone(),
        weight_bps: 5_000,
    });
    assert_eq!(
        client.try_set_source_weights(&duplicated, &admin),
        Err(Ok(AggregatorError::InvalidWeight))
    );

    // Unknown and deregistered sources.
    let stranger = Address::generate(&env);
    let unknown = two_weights(&env, &s0, 5_000, &stranger, 5_000);
    assert_eq!(
        client.try_set_source_weights(&unknown, &admin),
        Err(Ok(AggregatorError::SourceNotFound))
    );
    client.deregister_source(&s2);
    let inactive = three_weights(&env, &s0, 4_000, &s1, 4_000, &s2, 2_000);
    assert_eq!(
        client.try_set_source_weights(&inactive, &admin),
        Err(Ok(AggregatorError::SourceNotActive))
    );

    // A rejected batch changes nothing.
    assert_eq!(client.get_source(&s0).weight_bps, 5_000);
    assert_eq!(client.total_weight(), 5_000);
}

#[test]
fn test_set_source_weight_rebalances_others() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let sources = register(&env, &client, 3);
    let (s0, s1, s2) = (at(&sources, 0), at(&sources, 1), at(&sources, 2));
    let fair = three_weights(&env, &s0, 5_000, &s1, 3_000, &s2, 2_000);
    client.set_source_weights(&fair, &admin);

    // 4_000 leaves 6_000 to share between the other two in their old ratio
    // 3:2, which is 3_600 and 2_400 - no rounding loss.
    assert_eq!(client.set_source_weight(&s0, &4_000, &admin), 10_000);
    assert_eq!(client.get_source(&s0).weight_bps, 4_000);
    assert_eq!(client.get_source(&s1).weight_bps, 3_600);
    assert_eq!(client.get_source(&s2).weight_bps, 2_400);
    assert_eq!(client.total_weight(), 10_000);

    // 3_333 leaves 6_667 for 3_600 / 2_400: 4_000.2 and 2_666.8, so the single
    // rounding bps is handed to the first source of the panel.
    assert_eq!(client.set_source_weight(&s0, &3_333, &admin), 10_000);
    assert_eq!(client.get_source(&s0).weight_bps, 3_333);
    assert_eq!(client.get_source(&s1).weight_bps, 4_001);
    assert_eq!(client.get_source(&s2).weight_bps, 2_666);
    assert_eq!(client.total_weight(), 10_000);
}

#[test]
fn test_set_source_weight_validation() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let sources = register(&env, &client, 2);
    let (s0, s1) = (at(&sources, 0), at(&sources, 1));
    let fair = two_weights(&env, &s0, 5_000, &s1, 5_000);
    client.set_source_weights(&fair, &admin);

    let zero = client.try_set_source_weight(&s0, &0, &admin);
    assert_eq!(zero, Err(Ok(AggregatorError::InvalidWeight)));

    let over = client.try_set_source_weight(&s0, &10_001, &admin);
    assert_eq!(over, Err(Ok(AggregatorError::InvalidWeight)));

    let unknown = client.try_set_source_weight(&Address::generate(&env), &5_000, &admin);
    assert_eq!(unknown, Err(Ok(AggregatorError::SourceNotFound)));

    client.deregister_source(&s1);
    let inactive = client.try_set_source_weight(&s1, &5_000, &admin);
    assert_eq!(inactive, Err(Ok(AggregatorError::SourceNotActive)));

    // The lone active source has to carry the whole book on its own.
    let orphan = client.try_set_source_weight(&s0, &4_000, &admin);
    assert_eq!(orphan, Err(Ok(AggregatorError::InvalidWeight)));
    assert_eq!(client.set_source_weight(&s0, &10_000, &admin), 10_000);
    assert_eq!(client.get_source(&s0).weight_bps, 10_000);
}

#[test]
fn test_unauthorized_weight_change_is_rejected() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let sources = register(&env, &client, 2);
    let (s0, s1) = (at(&sources, 0), at(&sources, 1));
    let fair = two_weights(&env, &s0, 5_000, &s1, 5_000);
    client.set_source_weights(&fair, &admin);

    let stranger = Address::generate(&env);
    let single = client.try_set_source_weight(&s0, &8_000, &stranger);
    assert_eq!(single, Err(Ok(AggregatorError::Unauthorized)));

    let batch = client.try_set_source_weights(&fair, &stranger);
    assert_eq!(batch, Err(Ok(AggregatorError::Unauthorized)));

    // Nothing moved.
    assert_eq!(client.get_source(&s0).weight_bps, 5_000);
    assert_eq!(client.get_source(&s1).weight_bps, 5_000);
}

// ---------------------------------------------------------------------- markets

#[test]
fn test_create_market_validation() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, _admin) = setup(&env);

    let id = open_market(&env, &client, "MKT_A", 2_000);
    let market = client.get_market(&id);
    assert_eq!(market.market_id, id);
    assert_eq!(market.resolution_deadline, 2_000);
    assert_eq!(market.min_sources, MIN_SOURCES);
    assert_eq!(market.min_outcome_bps, 0);
    assert_eq!(market.max_outcome_bps, 10_000);
    assert_eq!(market.prediction_count, 0);
    assert!(!market.closed);
    assert!(!market.resolved);
    assert_eq!(market.created_at, 1_000);
    assert_eq!(client.get_market_status(&id), MarketStatus::Open);
    assert_eq!(client.market_count(), 1);

    let duplicate = client.try_create_prediction_market(
        &id,
        &question(&env, "again"),
        &Symbol::new(&env, "XLM_PX"),
        &2_000,
        &0,
        &10_000,
        &MIN_SOURCES,
    );
    assert_eq!(duplicate, Err(Ok(AggregatorError::MarketAlreadyExists)));

    let bad_range = client.try_create_prediction_market(
        &Symbol::new(&env, "MKT_B"),
        &question(&env, "range"),
        &Symbol::new(&env, "XLM_PX"),
        &2_000,
        &8_000,
        &2_000,
        &MIN_SOURCES,
    );
    assert_eq!(bad_range, Err(Ok(AggregatorError::InvalidRange)));

    let past_deadline = client.try_create_prediction_market(
        &Symbol::new(&env, "MKT_C"),
        &question(&env, "past"),
        &Symbol::new(&env, "XLM_PX"),
        &500,
        &0,
        &10_000,
        &MIN_SOURCES,
    );
    assert_eq!(past_deadline, Err(Ok(AggregatorError::InvalidConfig)));

    let unknown = client.try_get_market(&Symbol::new(&env, "MKT_Z"));
    assert_eq!(unknown, Err(Ok(AggregatorError::MarketNotFound)));
}

#[test]
fn test_market_min_sources_never_drops_below_protocol() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, _admin) = setup(&env);

    let id = Symbol::new(&env, "MKT_A");
    client.create_prediction_market(
        &id,
        &question(&env, "single source?"),
        &Symbol::new(&env, "XLM_PX"),
        &2_000,
        &0,
        &10_000,
        &1,
    );
    assert_eq!(client.get_market(&id).min_sources, MIN_SOURCES);
}

#[test]
fn test_submit_prediction_validation() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, _admin) = setup(&env);
    let sources = register(&env, &client, 2);
    let (s0, s1) = (at(&sources, 0), at(&sources, 1));
    let id = open_market(&env, &client, "MKT_A", 2_000);

    // Only registered sources may forecast.
    let unknown = client.try_submit_prediction(&id, &Address::generate(&env), &5_000);
    assert_eq!(unknown, Err(Ok(AggregatorError::SourceNotFound)));

    // Probabilities are basis points, 10_001 bps is out of range.
    let too_high = client.try_submit_prediction(&id, &s0, &10_001);
    assert_eq!(too_high, Err(Ok(AggregatorError::InvalidProbability)));

    // Zero is a valid, if bold, probability.
    client.submit_prediction(&id, &s0, &0);
    let stored = client.get_prediction(&id, &s0);
    assert_eq!(stored.raw_probability_bps, 0);
    assert_eq!(stored.probability_bps, 0);
    assert_eq!(stored.weight_bps, DEFAULT_WEIGHT);
    assert_eq!(stored.timestamp, 1_000);
    assert_eq!(client.get_predictions(&id).len(), 1);

    // One prediction per source and market.
    let duplicate = client.try_submit_prediction(&id, &s0, &5_000);
    assert_eq!(duplicate, Err(Ok(AggregatorError::AlreadySubmitted)));

    // The window closes at the deadline.
    env.ledger().set_timestamp(2_000);
    let late = client.try_submit_prediction(&id, &s1, &5_000);
    assert_eq!(late, Err(Ok(AggregatorError::SubmissionWindowClosed)));

    let resolution = client.resolve_market(&id, &5_000);
    assert_eq!(resolution.scored_sources, 1);
    assert_eq!(client.get_market_status(&id), MarketStatus::Resolved);

    let resolved = client.try_submit_prediction(&id, &s1, &5_000);
    assert_eq!(resolved, Err(Ok(AggregatorError::MarketResolved)));
}

#[test]
fn test_close_market_before_resolution() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, _admin) = setup(&env);
    let sources = register(&env, &client, 2);
    let id = open_market(&env, &client, "MKT_A", 2_000);
    client.submit_prediction(&id, &at(&sources, 0), &5_000);

    let early = client.try_close_market(&id);
    assert_eq!(early, Err(Ok(AggregatorError::DeadlineNotReached)));
    let early_resolve = client.try_resolve_market(&id, &5_000);
    assert_eq!(early_resolve, Err(Ok(AggregatorError::DeadlineNotReached)));

    env.ledger().set_timestamp(2_000);
    client.close_market(&id);
    assert_eq!(client.get_market_status(&id), MarketStatus::Closed);
    let again = client.try_close_market(&id);
    assert_eq!(again, Err(Ok(AggregatorError::MarketClosed)));

    // A closed market refuses further predictions.
    let late = client.try_submit_prediction(&id, &at(&sources, 1), &5_000);
    assert_eq!(late, Err(Ok(AggregatorError::SubmissionWindowClosed)));

    let resolution = client.resolve_market(&id, &5_000);
    assert_eq!(resolution.true_probability_bps, 5_000);
    assert_eq!(resolution.scored_sources, 1);
    assert_eq!(resolution.hits, 1);
    assert_eq!(client.get_market_status(&id), MarketStatus::Resolved);
    let twice = client.try_resolve_market(&id, &5_000);
    assert_eq!(twice, Err(Ok(AggregatorError::MarketResolved)));
    let close_after = client.try_close_market(&id);
    assert_eq!(close_after, Err(Ok(AggregatorError::MarketResolved)));
}

#[test]
fn test_resolve_validates_outcome() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, _admin) = setup(&env);
    register(&env, &client, 2);

    let id = Symbol::new(&env, "MKT_A");
    client.create_prediction_market(
        &id,
        &question(&env, "bounded outcome"),
        &Symbol::new(&env, "XLM_PX"),
        &2_000,
        &2_000,
        &8_000,
        &MIN_SOURCES,
    );

    env.ledger().set_timestamp(2_000);
    let too_high = client.try_resolve_market(&id, &10_001);
    assert_eq!(too_high, Err(Ok(AggregatorError::InvalidProbability)));

    let below_range = client.try_resolve_market(&id, &1_000);
    assert_eq!(below_range, Err(Ok(AggregatorError::InvalidOutcome)));

    let above_range = client.try_resolve_market(&id, &9_000);
    assert_eq!(above_range, Err(Ok(AggregatorError::InvalidOutcome)));

    // Inside the range.
    client.resolve_market(&id, &8_000);
    let stored = client.get_resolved_market(&id);
    assert_eq!(stored.true_probability_bps, 8_000);
    assert_eq!(stored.resolved_at, 2_000);
    assert_eq!(stored.scored_sources, 0);
}

#[test]
fn test_get_resolved_market_requires_resolution() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, _admin) = setup(&env);
    let id = open_market(&env, &client, "MKT_A", 2_000);

    let pending = client.try_get_resolved_market(&id);
    assert_eq!(pending, Err(Ok(AggregatorError::MarketNotResolved)));
}

// --------------------------------------------------------------------- consensus

#[test]
fn test_aggregate_weighted_average() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, admin) = setup(&env);
    let sources = register(&env, &client, 3);
    let (s0, s1, s2) = (at(&sources, 0), at(&sources, 1), at(&sources, 2));
    let weights = three_weights(&env, &s0, 5_000, &s1, 3_000, &s2, 2_000);
    client.set_source_weights(&weights, &admin);

    let id = open_market(&env, &client, "MKT_A", 2_000);
    client.submit_prediction(&id, &s0, &6_000);
    client.submit_prediction(&id, &s1, &5_000);
    client.submit_prediction(&id, &s2, &4_000);

    // (5_000*6_000 + 3_000*5_000 + 2_000*4_000) / 10_000 == 5_300
    let forecast = client.aggregate(&id);
    assert_eq!(forecast.market_id, id);
    assert_eq!(forecast.probability_bps, 5_300);
    assert_eq!(forecast.sources_used, 3);
    assert_eq!(forecast.total_sources, 3);
    assert_eq!(forecast.weight_used, 10_000);
    assert_eq!(forecast.excluded.len(), 0);
    assert_eq!(forecast.timestamp, 1_000);
    // deviations 700 / 300 / 1_300, weighted MAD 700 bps, agreement 9_300 bps,
    // count factor 7_500 bps for three of the two required sources.
    assert_eq!(forecast.confidence_bps, 6_975);
    assert!(forecast.published);

    let stored = client.get_forecast(&id);
    assert_eq!(stored.probability_bps, 5_300);
    assert_eq!(stored.confidence_bps, 6_975);
}

#[test]
fn test_outlier_is_excluded_from_the_consensus() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, admin) = setup(&env);
    let sources = register(&env, &client, 3);
    let (s0, s1, s2) = (at(&sources, 0), at(&sources, 1), at(&sources, 2));
    let weights = three_weights(&env, &s0, 4_000, &s1, 4_000, &s2, 2_000);
    client.set_source_weights(&weights, &admin);

    let id = open_market(&env, &client, "MKT_A", 2_000);
    client.submit_prediction(&id, &s0, &5_000);
    client.submit_prediction(&id, &s1, &5_100);
    client.submit_prediction(&id, &s2, &9_000);

    // Pre-trim mean 5_840, so s2 deviates 3_160 bps and is trimmed; the mean of
    // the two survivors is 5_050.
    let forecast = client.aggregate(&id);
    assert_eq!(forecast.probability_bps, 5_050);
    assert_eq!(forecast.sources_used, 2);
    assert_eq!(forecast.total_sources, 3);
    assert_eq!(forecast.weight_used, 8_000);
    assert_eq!(forecast.excluded.len(), 1);
    assert_eq!(forecast.excluded.get(0).unwrap(), s2);
    // MAD 50 bps, agreement 9_950, count factor 5_000.
    assert_eq!(forecast.confidence_bps, 4_975);
    assert!(forecast.published);

    let analytics = client.get_analytics();
    assert_eq!(analytics.excluded_observations, 1);
}

#[test]
fn test_insufficient_sources_is_rejected() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, admin) = setup(&env);
    let sources = register(&env, &client, 3);
    let (s0, s1, s2) = (at(&sources, 0), at(&sources, 1), at(&sources, 2));
    let weights = three_weights(&env, &s0, 3_000, &s1, 3_000, &s2, 4_000);
    client.set_source_weights(&weights, &admin);

    let id = open_market(&env, &client, "MKT_A", 2_000);
    client.submit_prediction(&id, &s0, &5_000);

    // A single prediction can never satisfy the two source requirement.
    let too_few = client.try_aggregate(&id);
    assert_eq!(too_few, Err(Ok(AggregatorError::InsufficientSources)));
    assert_eq!(client.try_get_forecast(&id), Err(Ok(AggregatorError::NoConsensus)));

    // The pre-trim mean of 5_000 / 5_000 / 500 is 3_200, every source deviates by
    // more than the 1_500 bps tolerance, so nothing survives the trim.
    client.submit_prediction(&id, &s1, &5_000);
    client.submit_prediction(&id, &s2, &500);
    let trimmed = client.try_aggregate(&id);
    assert_eq!(trimmed, Err(Ok(AggregatorError::InsufficientSources)));
    assert_eq!(client.get_market_history(&id, &10).len(), 0);
}

#[test]
fn test_confidence_reflects_agreement() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, admin) = setup(&env);
    let sources = register(&env, &client, 3);
    let (s0, s1, s2) = (at(&sources, 0), at(&sources, 1), at(&sources, 2));
    let weights = three_weights(&env, &s0, 3_400, &s1, 3_300, &s2, 3_300);
    client.set_source_weights(&weights, &admin);

    let tight = open_market(&env, &client, "MKT_TIGHT", 2_000);
    client.submit_prediction(&tight, &s0, &5_000);
    client.submit_prediction(&tight, &s1, &5_100);
    client.submit_prediction(&tight, &s2, &4_900);
    let tight_forecast = client.aggregate(&tight);

    let spread = open_market(&env, &client, "MKT_SPREAD", 2_000);
    client.submit_prediction(&spread, &s0, &3_600);
    client.submit_prediction(&spread, &s1, &5_000);
    client.submit_prediction(&spread, &s2, &6_400);
    let spread_forecast = client.aggregate(&spread);

    // A tight cluster scores near the count factor of 7_500 bps, a panel that
    // disagrees by 2_800 bps across the extremes scores well below it.
    assert_eq!(tight_forecast.probability_bps, 5_000);
    assert_eq!(tight_forecast.confidence_bps, 7_450);
    assert_eq!(spread_forecast.confidence_bps, 6_793);
    assert!(tight_forecast.confidence_bps > spread_forecast.confidence_bps);
}

#[test]
fn test_min_confidence_blocks_publication() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, admin) = setup(&env);
    let sources = register(&env, &client, 2);
    let (s0, s1) = (at(&sources, 0), at(&sources, 1));
    let weights = two_weights(&env, &s0, 5_000, &s1, 5_000);
    client.set_source_weights(&weights, &admin);

    let id = open_market(&env, &client, "MKT_A", 2_000);
    client.submit_prediction(&id, &s0, &4_000);
    client.submit_prediction(&id, &s1, &6_000);

    // MAD 1_000 bps, agreement 9_000, count factor 5_000.
    let forecast = client.aggregate(&id);
    assert_eq!(forecast.probability_bps, 5_000);
    assert_eq!(forecast.confidence_bps, 4_500);
    assert!(forecast.published);
    assert_eq!(client.get_market_history(&id, &10).len(), 1);

    // Raising the bar keeps the forecast unpublished.
    client.set_min_confidence(&admin, &5_000);
    let rejected = client.aggregate(&id);
    assert_eq!(rejected.probability_bps, 5_000);
    assert_eq!(rejected.confidence_bps, 4_500);
    assert!(!rejected.published);
    assert_eq!(client.get_market_history(&id, &10).len(), 1);
    let analytics = client.get_analytics();
    assert_eq!(analytics.published_forecasts, 1);
}

#[test]
fn test_market_history_is_capped() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, admin) = setup(&env);
    let sources = register(&env, &client, 2);
    let (s0, s1) = (at(&sources, 0), at(&sources, 1));
    let weights = two_weights(&env, &s0, 5_000, &s1, 5_000);
    client.set_source_weights(&weights, &admin);

    let id = open_market(&env, &client, "MKT_A", 9_000);
    client.submit_prediction(&id, &s0, &5_000);
    client.submit_prediction(&id, &s1, &5_000);

    for step in 0..5u64 {
        env.ledger().set_timestamp(1_000 + step);
        let forecast = client.aggregate(&id);
        assert!(forecast.published);
    }

    // HISTORY_CAP entries, oldest first, and the last call is the last forecast.
    let history = client.get_market_history(&id, &10);
    assert_eq!(history.len(), HISTORY_CAP as u32);
    assert_eq!(history.get(0).unwrap().timestamp, 1_002);
    assert_eq!(history.get(1).unwrap().timestamp, 1_003);
    assert_eq!(history.get(2).unwrap().timestamp, 1_004);
    assert_eq!(client.get_forecast(&id).timestamp, 1_004);
    assert_eq!(client.get_market_history(&id, &1).len(), 1);
    assert_eq!(client.get_market_history(&id, &0).len(), 0);
}

// -------------------------------------------------------------------- accuracy

#[test]
fn test_resolution_updates_accuracy_and_bias() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, admin) = setup(&env);
    let sources = register(&env, &client, 2);
    let (s0, s1) = (at(&sources, 0), at(&sources, 1));
    let weights = two_weights(&env, &s0, 5_000, &s1, 5_000);
    client.set_source_weights(&weights, &admin);

    let first = open_market(&env, &client, "MKT_A", 2_000);
    client.submit_prediction(&first, &s0, &7_000);
    client.submit_prediction(&first, &s1, &5_000);

    // Before the first resolution no bias is known, nothing is corrected.
    assert_eq!(client.get_source_bias(&s0), 0);
    assert_eq!(client.get_prediction(&first, &s0).probability_bps, 7_000);

    env.ledger().set_timestamp(2_000);
    let resolution = client.resolve_market(&first, &6_000);
    assert_eq!(resolution.scored_sources, 2);
    assert_eq!(resolution.hits, 2);
    assert_eq!(resolution.misses, 0);
    assert_eq!(resolution.true_probability_bps, 6_000);

    // s0 over-predicts by 1_000 bps, s1 under-predicts by 1_000 bps; both are
    // within the 1_000 bps tolerance and therefore hits.
    assert_eq!(client.get_source_bias(&s0), 1_000);
    assert_eq!(client.get_source_bias(&s1), -1_000);
    let accuracy = client.get_source_accuracy(&s0);
    assert_eq!(accuracy.resolved, 1);
    assert_eq!(accuracy.hits, 1);
    assert_eq!(accuracy.misses, 0);
    assert_eq!(accuracy.accuracy_bps, 10_000);
    assert_eq!(accuracy.bias_bps, 1_000);
    assert_eq!(client.get_source_accuracy(&s1).hits, 1);

    let resolved = client.get_resolved_market(&first);
    assert_eq!(resolved.true_probability_bps, 6_000);
    assert_eq!(resolved.hits, 2);
    assert_eq!(resolved.resolved_at, 2_000);
}

#[test]
fn test_bias_correction_adjusts_the_forecast() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, admin) = setup(&env);
    let sources = register(&env, &client, 2);
    let (s0, s1) = (at(&sources, 0), at(&sources, 1));
    let weights = two_weights(&env, &s0, 5_000, &s1, 5_000);
    client.set_source_weights(&weights, &admin);

    let first = open_market(&env, &client, "MKT_A", 2_000);
    client.submit_prediction(&first, &s0, &7_000);
    client.submit_prediction(&first, &s1, &5_000);
    env.ledger().set_timestamp(2_000);
    client.resolve_market(&first, &6_000);

    // s0 is 1_000 bps too bullish and s1 1_000 bps too bearish, so identical raw
    // forecasts of 5_000 are pulled apart in opposite directions.
    let second = open_market(&env, &client, "MKT_B", 5_000);
    client.submit_prediction(&second, &s0, &5_000);
    client.submit_prediction(&second, &s1, &5_000);

    let corrected = client.get_prediction(&second, &s0);
    assert_eq!(corrected.raw_probability_bps, 5_000);
    assert_eq!(corrected.probability_bps, 4_000);
    assert_eq!(corrected.bias_applied_bps, 1_000);

    let mirrored = client.get_prediction(&second, &s1);
    assert_eq!(mirrored.raw_probability_bps, 5_000);
    assert_eq!(mirrored.probability_bps, 6_000);
    assert_eq!(mirrored.bias_applied_bps, -1_000);

    // The corrected forecasts still meet in the middle.
    let forecast = client.aggregate(&second);
    assert_eq!(forecast.probability_bps, 5_000);
    assert_eq!(forecast.sources_used, 2);
}

#[test]
fn test_bias_correction_is_clamped() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, _admin) = setup_with_correction(&env, 500);
    let sources = register(&env, &client, 2);
    let (s0, _s1) = (at(&sources, 0), at(&sources, 1));

    let first = open_market(&env, &client, "MKT_A", 2_000);
    client.submit_prediction(&first, &s0, &9_000);
    env.ledger().set_timestamp(2_000);
    client.resolve_market(&first, &5_000);

    // The source built up a 4_000 bps bullish bias, but only 500 bps of it may be
    // used to move a forecast.
    assert_eq!(client.get_source_bias(&s0), 4_000);

    let second = open_market(&env, &client, "MKT_B", 5_000);
    client.submit_prediction(&second, &s0, &5_000);
    let corrected = client.get_prediction(&second, &s0);
    assert_eq!(corrected.raw_probability_bps, 5_000);
    assert_eq!(corrected.probability_bps, 4_500);
    assert_eq!(corrected.bias_applied_bps, 500);
}

#[test]
fn test_accuracy_counts_misses() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, _admin) = setup(&env);
    let source = at(&register(&env, &client, 1), 0);

    let first = open_market(&env, &client, "MKT_A", 2_000);
    client.submit_prediction(&first, &source, &9_000);
    env.ledger().set_timestamp(2_000);
    let resolution = client.resolve_market(&first, &5_000);
    assert_eq!(resolution.hits, 0);
    assert_eq!(resolution.misses, 1);

    let accuracy = client.get_source_accuracy(&source);
    assert_eq!(accuracy.resolved, 1);
    assert_eq!(accuracy.misses, 1);
    assert_eq!(accuracy.accuracy_bps, 0);
    assert_eq!(accuracy.bias_bps, 4_000);

    // The averaged bias over two resolutions: (+4_000 + 0) / 2. The second raw
    // forecast of 7_000 is bias corrected down to 5_000 and therefore hits.
    let second = open_market(&env, &client, "MKT_B", 5_000);
    client.submit_prediction(&second, &source, &7_000);
    assert_eq!(client.get_prediction(&second, &source).probability_bps, 5_000);
    env.ledger().set_timestamp(5_000);
    client.resolve_market(&second, &5_000);
    assert_eq!(client.get_source_accuracy(&source).resolved, 2);
    assert_eq!(client.get_source_accuracy(&source).hits, 1);
    assert_eq!(client.get_source_accuracy(&source).misses, 1);
    assert_eq!(client.get_source_accuracy(&source).accuracy_bps, 5_000);
    assert_eq!(client.get_source_bias(&source), 2_000);

    let unknown = client.try_get_source_accuracy(&Address::generate(&env));
    assert_eq!(unknown, Err(Ok(AggregatorError::SourceNotFound)));
    let unknown_bias = client.try_get_source_bias(&Address::generate(&env));
    assert_eq!(unknown_bias, Err(Ok(AggregatorError::SourceNotFound)));
}

// -------------------------------------------------------------------- analytics

#[test]
fn test_analytics_counters() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, admin) = setup(&env);
    let sources = register(&env, &client, 4);
    let (s0, s1, s2, s3) = (
        at(&sources, 0),
        at(&sources, 1),
        at(&sources, 2),
        at(&sources, 3),
    );
    let weights = three_weights(&env, &s0, 5_000, &s1, 3_000, &s2, 2_000);
    client.set_source_weights(&weights, &admin);
    // s3 keeps the default weight, so the panel now totals 12_500 and the
    // weighted mean is normalised by the actual total.
    assert_eq!(client.total_weight(), 12_500);

    let first = open_market(&env, &client, "MKT_A", 2_000);
    let second = open_market(&env, &client, "MKT_B", 2_000);
    assert_eq!(client.market_count(), 2);

    client.submit_prediction(&first, &s0, &6_000);
    client.submit_prediction(&first, &s1, &5_000);
    client.submit_prediction(&first, &s2, &4_000);
    client.submit_prediction(&second, &s0, &6_500);
    client.submit_prediction(&second, &s1, &5_500);
    client.submit_prediction(&second, &s3, &5_000);

    let forecast = client.aggregate(&first);
    assert!(forecast.published);

    env.ledger().set_timestamp(2_000);
    client.resolve_market(&first, &5_200);

    let analytics = client.get_analytics();
    assert_eq!(analytics.market_count, 2);
    assert_eq!(analytics.resolved_count, 1);
    assert_eq!(analytics.closed_count, 0);
    assert_eq!(analytics.source_count, 4);
    assert_eq!(analytics.active_sources, 4);
    assert_eq!(analytics.total_predictions, 6);
    assert_eq!(analytics.published_forecasts, 1);
    assert_eq!(analytics.excluded_observations, 0);
    assert_eq!(analytics.total_weight, 12_500);
    assert_eq!(analytics.avg_confidence_bps, forecast.confidence_bps);
    // Resolved against 5_200: s0 off by 800 and s1 off by 200 are hits, s2 is
    // 1_200 off and is a miss.
    assert_eq!(analytics.hits, 2);
    assert_eq!(analytics.misses, 1);
    assert_eq!(analytics.accuracy_bps, 6_666);
    assert_eq!(analytics.sources.len(), 4);
    assert_eq!(analytics.sources.get(0).unwrap().weight_bps, 5_000);
    assert_eq!(analytics.sources.get(0).unwrap().predictions, 2);
    assert_eq!(analytics.sources.get(0).unwrap().resolved, 1);
    assert_eq!(analytics.sources.get(2).unwrap().predictions, 1);
    assert_eq!(analytics.sources.get(2).unwrap().resolved, 1);
    assert_eq!(analytics.sources.get(3).unwrap().predictions, 1);
    assert_eq!(analytics.sources.get(3).unwrap().resolved, 0);

    // Deregistering takes the weight and the source out of the panel.
    client.deregister_source(&s3);
    let analytics = client.get_analytics();
    assert_eq!(analytics.active_sources, 3);
    assert_eq!(analytics.total_weight, 10_000);
}

#[test]
fn test_events_are_published() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, admin) = setup(&env);
    let sources = register(&env, &client, 2);
    let (s0, s1) = (at(&sources, 0), at(&sources, 1));
    let weights = two_weights(&env, &s0, 5_000, &s1, 5_000);
    client.set_source_weights(&weights, &admin);
    client.set_source_weight(&s0, &6_000, &admin);

    let id = open_market(&env, &client, "MKT_A", 2_000);
    client.submit_prediction(&id, &s0, &5_000);
    client.submit_prediction(&id, &s1, &5_000);
    client.aggregate(&id);
    env.ledger().set_timestamp(2_000);
    client.resolve_market(&id, &5_000);

    let topics = topic_symbols(&env);
    assert!(has_topic(&topics, Symbol::new(&env, "init")));
    assert!(has_topic(&topics, Symbol::new(&env, "src_add")));
    assert!(has_topic(&topics, Symbol::new(&env, "weights")));
    assert!(has_topic(&topics, Symbol::new(&env, "weight")));
    assert!(has_topic(&topics, Symbol::new(&env, "mkt_new")));
    assert!(has_topic(&topics, Symbol::new(&env, "predict")));
    assert!(has_topic(&topics, Symbol::new(&env, "consen")));
    assert!(has_topic(&topics, Symbol::new(&env, "resolve")));
}
