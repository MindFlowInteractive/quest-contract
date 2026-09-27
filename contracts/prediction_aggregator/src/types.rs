use soroban_sdk::{contracterror, contracttype, Address, Env, Map, String, Symbol, Vec};

/// Basis point denominator used everywhere in the aggregator: `10_000 == 100%`.
///
/// Predictions are probabilistic, so a probability is modelled as an integer
/// number of basis points in the closed range `0..=MAX_BPS`. Every average,
/// deviation, weight, confidence and accuracy figure in this contract is
/// expressed in the same unit, which keeps all of the arithmetic exact integer
/// math - no floating point, no rounding drift, fully deterministic on chain.
pub const MAX_BPS: u32 = 10_000;

/// Hard upper bound for the number of published forecasts retained per market.
pub const MAX_HISTORY: u32 = 100;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AggregatorError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    InvalidConfig = 4,
    InvalidWeight = 5,
    SourceNotFound = 6,
    SourceAlreadyExists = 7,
    SourceNotActive = 8,
    MarketNotFound = 9,
    MarketAlreadyExists = 10,
    MarketClosed = 11,
    MarketResolved = 12,
    InvalidProbability = 13,
    AlreadySubmitted = 14,
    SubmissionWindowClosed = 15,
    DeadlineNotReached = 16,
    InsufficientSources = 17,
    NoConsensus = 18,
    InvalidOutcome = 19,
    InvalidRange = 20,
    MarketNotResolved = 21,
}

/// Controls for the bias correction and the accuracy accounting.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BiasConfig {
    /// Largest bias, in bps, that may be subtracted from a source's forecast.
    /// Clamping the correction to this value is what stops a rogue source with a
    /// long history of extreme errors from swinging the aggregate.
    pub max_correction_bps: u32,
    /// A resolved forecast counts as a *hit* when it is within this many bps of
    /// the true outcome.
    pub accuracy_tolerance_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
    /// Weight handed to a source when it joins the registry.
    pub default_source_weight: u32,
    /// Protocol wide minimum number of sources required to publish a forecast.
    pub min_sources: u32,
    /// A source is an outlier when its probability deviates from the weighted
    /// mean by strictly more than this many bps.
    pub outlier_threshold_bps: u32,
    /// A forecast is only stored and published at or above this confidence.
    pub min_confidence_bps: u32,
    /// Published forecasts retained per market (`1..=MAX_HISTORY`).
    pub history_cap: u32,
    pub bias: BiasConfig,
}

/// A registered forecast source together with its track record.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Source {
    pub address: Address,
    /// Share of the consensus carried by the source, in bps. The registry
    /// normally totals `MAX_BPS`.
    pub weight_bps: u32,
    pub active: bool,
    /// Lifetime number of accepted predictions.
    pub predictions: u64,
    /// Number of resolved markets the source predicted on.
    pub resolved: u32,
    pub hits: u32,
    pub misses: u32,
    /// Running sum of the signed errors against resolved outcomes.
    pub bias_sum_bps: i128,
    /// Average signed error against resolved outcomes, in bps.
    pub bias_bps: i128,
    pub registered_at: u64,
    pub last_submission: u64,
}

/// One entry of a batch weight update.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceWeight {
    pub source: Address,
    pub weight_bps: u32,
}

/// Historical accuracy of a single source.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceAccuracy {
    pub source: Address,
    pub resolved: u32,
    pub hits: u32,
    pub misses: u32,
    /// `hits / resolved` expressed in bps, `0` when nothing resolved yet.
    pub accuracy_bps: u32,
    /// Average signed error against resolved outcomes, in bps.
    pub bias_bps: i128,
}

/// Per source row of the analytics summary.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSummary {
    pub source: Address,
    pub weight_bps: u32,
    pub active: bool,
    pub predictions: u64,
    pub resolved: u32,
    pub hits: u32,
    pub misses: u32,
    pub accuracy_bps: u32,
    pub bias_bps: i128,
}

/// A prediction stored for one source on one market.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Prediction {
    pub market_id: Symbol,
    pub source: Address,
    /// Exactly what the source submitted, before bias correction.
    pub raw_probability_bps: u32,
    /// Bias corrected probability used by the consensus algorithm, clamped to
    /// `0..=MAX_BPS`.
    pub probability_bps: u32,
    /// Bias that was subtracted, already clamped to the configured maximum.
    pub bias_applied_bps: i128,
    /// Weight of the source captured at submission time.
    pub weight_bps: u32,
    pub timestamp: u64,
}

/// The weighted consensus produced for a market.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsensusForecast {
    pub market_id: Symbol,
    /// Bias corrected, outlier trimmed weighted mean probability, in bps.
    pub probability_bps: u32,
    /// `0..=MAX_BPS`, derived from the agreement of the sources and their count.
    pub confidence_bps: u32,
    /// Number of non-outlier sources that carried the mean.
    pub sources_used: u32,
    /// Number of predictions the market holds.
    pub total_sources: u32,
    /// Summed weight of the sources that carried the mean, in bps.
    pub weight_used: u32,
    /// Sources dropped by the outlier filter.
    pub excluded: Vec<Address>,
    pub timestamp: u64,
    /// `true` when the confidence reached the configured minimum and the
    /// forecast was therefore stored and published.
    pub published: bool,
}

/// Outcome of a resolved market.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Resolution {
    pub market_id: Symbol,
    pub true_probability_bps: u32,
    pub resolved_at: u64,
    /// Number of predictions that were scored.
    pub scored_sources: u32,
    pub hits: u32,
    pub misses: u32,
}

/// A prediction market that sources forecast.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Market {
    pub market_id: Symbol,
    pub question: String,
    /// Short key describing what is being forecast, e.g. `XLM_PRICE`.
    pub target: Symbol,
    /// Ledger timestamp from which no further prediction is accepted.
    pub resolution_deadline: u64,
    /// Lower bound the true outcome must respect, in bps.
    pub min_outcome_bps: u32,
    /// Upper bound the true outcome must respect, in bps.
    pub max_outcome_bps: u32,
    /// Sources required to publish a forecast, never below the protocol minimum.
    pub min_sources: u32,
    pub predictions: Map<Address, Prediction>,
    pub prediction_count: u32,
    pub closed: bool,
    pub resolved: bool,
    pub true_probability_bps: Option<u32>,
    pub resolved_at: u64,
    /// Number of predictions scored at resolution.
    pub scored_sources: u32,
    pub hits: u32,
    pub misses: u32,
    /// Last forecast that met the minimum confidence.
    pub last_forecast: Option<ConsensusForecast>,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum MarketStatus {
    Open = 0,
    Closed = 1,
    Resolved = 2,
}

/// Protocol wide counters plus the per source accuracy summary.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AggregatorAnalytics {
    pub market_count: u32,
    pub closed_count: u32,
    pub resolved_count: u32,
    pub source_count: u32,
    pub active_sources: u32,
    pub total_predictions: u64,
    pub published_forecasts: u32,
    /// Summed confidence of the published forecasts, used for the average.
    pub confidence_sum: u64,
    /// Source observations dropped by the outlier filter.
    pub excluded_observations: u64,
    /// Summed weight of the active sources, in bps.
    pub total_weight: u32,
    /// Mean confidence of the published forecasts, in bps.
    pub avg_confidence_bps: u32,
    pub hits: u64,
    pub misses: u64,
    /// `hits / (hits + misses)` over all resolved markets, in bps.
    pub accuracy_bps: u32,
    pub sources: Vec<SourceSummary>,
}

impl AggregatorAnalytics {
    /// Zeroed aggregate used before the first state changing call.
    pub fn empty(env: &Env) -> Self {
        AggregatorAnalytics {
            market_count: 0,
            closed_count: 0,
            resolved_count: 0,
            source_count: 0,
            active_sources: 0,
            total_predictions: 0,
            published_forecasts: 0,
            confidence_sum: 0,
            excluded_observations: 0,
            total_weight: 0,
            avg_confidence_bps: 0,
            hits: 0,
            misses: 0,
            accuracy_bps: 0,
            sources: Vec::new(env),
        }
    }
}
