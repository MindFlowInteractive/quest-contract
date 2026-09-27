use soroban_sdk::{contracterror, contracttype, Address, Map, Symbol};

/// Basis point denominator used by the slashing configuration (100% == 10_000).
pub const MAX_BPS: u32 = 10_000;

/// Hard upper bound for the number of finalized rounds retained per feed.
///
/// `Config::max_history` is validated against this constant so that a feed can
/// never grow an unbounded per-feed history entry.
pub const MAX_HISTORY: u32 = 100;

/// Reputation a freshly registered provider starts with.
pub const INITIAL_REPUTATION: u32 = 100;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum DataFeedError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    InvalidConfig = 4,
    InvalidPenalty = 5,
    FeedNotFound = 6,
    FeedAlreadyExists = 7,
    InvalidBounds = 8,
    ProviderNotFound = 9,
    ProviderAlreadyExists = 10,
    ProviderNotActive = 11,
    InvalidWeight = 12,
    InvalidStake = 13,
    ValueOutOfBounds = 14,
    AlreadySubmitted = 15,
    SubmissionWindowClosed = 16,
    NoConsensusValue = 17,
    StaleData = 18,
    RoundNotFound = 19,
    NoViolationProven = 20,
    NothingToSlash = 21,
}

/// Slashing parameters of the protocol.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlashingConfig {
    /// Share of the provider stake that is burned per slash event, in bps.
    pub penalty_bps: u32,
    /// Absolute amount of reputation removed per slash event.
    pub reputation_penalty: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
    /// Number of distinct active providers that must submit in a round before
    /// consensus can be reached (count based threshold).
    pub consensus_threshold: u32,
    /// Length of the per-round submission window, in seconds.
    pub round_duration: u64,
    /// A consensus value older than this many seconds is reported as stale.
    pub staleness_window: u64,
    /// Number of finalized rounds retained per feed (1..=`MAX_HISTORY`).
    pub max_history: u32,
    pub slashing: SlashingConfig,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Provider {
    pub address: Address,
    /// Consensus weight of the provider.
    pub weight: u32,
    /// Bonded stake; slashing can never push this below zero.
    pub stake: i128,
    pub reputation: u32,
    pub active: bool,
    /// Lifetime number of accepted submissions.
    pub submissions: u64,
    /// Lifetime number of slash events.
    pub slash_count: u32,
    pub registered_at: u64,
    pub last_submission: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Submission {
    pub provider: Address,
    pub value: i128,
    pub timestamp: u64,
    pub round: u64,
    /// Provider weight captured at submission time, used by the weighted median.
    pub weight: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Feed {
    pub feed_id: Symbol,
    /// Short key describing what the providers report, e.g. `XLM_USD`.
    pub question: Symbol,
    pub min_value: i128,
    pub max_value: i128,
    /// Number of the round that is currently open for submissions.
    pub round: u64,
    /// Ledger timestamp at which the open round started.
    pub round_start: u64,
    /// Submissions of the open round, keyed by provider.
    pub submissions: Map<Address, Submission>,
    /// Distinct providers that submitted in the open round.
    pub submission_count: u32,
    /// Summed provider weight of the open round submissions.
    pub submission_weight: u64,
    /// Last consensus value, `None` until the first round is finalized.
    pub consensus_value: Option<i128>,
    pub consensus_round: u64,
    pub consensus_timestamp: u64,
    /// Providers that contributed to the last consensus value.
    pub consensus_contributors: u32,
    pub finalized_rounds: u64,
    pub failed_rounds: u64,
    pub total_submissions: u64,
    /// Submissions proven to be out of bounds by a challenge.
    pub rejected_submissions: u64,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsensusRecord {
    pub feed_id: Symbol,
    pub round: u64,
    pub value: i128,
    pub timestamp: u64,
    pub contributors: u32,
    /// Summed weight of the contributing providers.
    pub weight: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsensusValue {
    pub feed_id: Symbol,
    pub round: u64,
    pub value: i128,
    pub timestamp: u64,
    pub contributors: u32,
    /// `true` when the value is older than the configured staleness window.
    pub is_stale: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoundOutcome {
    /// The round reached consensus and was finalized.
    Consensus(ConsensusRecord),
    /// The submission window closed without reaching the threshold.
    NoConsensus,
    /// The window is still open and the threshold has not been reached yet.
    Pending,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Analytics {
    pub feed_count: u32,
    pub provider_count: u32,
    pub active_providers: u32,
    pub total_submissions: u64,
    pub finalized_rounds: u64,
    pub failed_rounds: u64,
    pub rejected_submissions: u64,
    pub slash_events: u32,
    pub total_stake: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedStats {
    pub feed_id: Symbol,
    pub current_round: u64,
    pub round_submissions: u32,
    pub round_submission_weight: u64,
    pub consensus_reached: bool,
    pub has_consensus: bool,
    pub is_stale: bool,
    pub total_submissions: u64,
    pub finalized_rounds: u64,
    pub failed_rounds: u64,
    pub rejected_submissions: u64,
    pub history_len: u32,
}

impl Analytics {
    /// Zeroed aggregate used before the first state changing call.
    pub fn empty() -> Self {
        Analytics {
            feed_count: 0,
            provider_count: 0,
            active_providers: 0,
            total_submissions: 0,
            finalized_rounds: 0,
            failed_rounds: 0,
            rejected_submissions: 0,
            slash_events: 0,
            total_stake: 0,
        }
    }
}
