#![no_std]

//! Decentralized data feed.
//!
//! The contract keeps a registry of bonded data providers, lets them submit a
//! value per feed round, and turns a round into a consensus value once enough
//! providers agree.
//!
//! Rules:
//!
//! * **Rounds** - every feed has a monotonically increasing `round`. A round is
//!   opened at `round_start` and accepts submissions for `round_duration`
//!   seconds. Finalizing (or closing) a round immediately opens the next one.
//! * **Validation** - a submission is rejected unless the provider is a
//!   registered, active provider, the round window is still open, the provider
//!   has not already answered this round, and the value lies inside the
//!   `[min_value, max_value]` bounds fixed when the feed was created.
//! * **Consensus** - a round reaches consensus when the number of distinct
//!   active providers that submitted is `>= consensus_threshold` (count based).
//!   The agreed value is the *weighted median* of the submitted values: the
//!   submissions are sorted by value and the first value whose accumulated
//!   provider weight covers at least half of the total weight wins. With equal
//!   weights this is the usual median (lower median for an even count).
//! * **Timeliness** - a consensus value is *stale* when it is older than
//!   `staleness_window`; `get_consensus_value` rejects stale values and
//!   `get_consensus` reports them through the `is_stale` flag.
//! * **History** - every finalized round is appended to a per-feed history that
//!   retains at most `max_history` entries (hard cap `MAX_HISTORY`).
//! * **Slashing** - the admin can slash a provider, and any active provider can
//!   challenge another provider with a value that the contract itself proves to
//!   be out of bounds. A slash burns `penalty_bps` of the provider stake and
//!   removes `reputation_penalty` reputation points, never going below zero.

use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Map, Symbol, Vec};

mod storage;
mod types;

use storage::Storage;
use types::{
    Analytics, Config, ConsensusRecord, ConsensusValue, DataFeedError, Feed, FeedStats, Provider,
    RoundOutcome, SlashingConfig, Submission, INITIAL_REPUTATION, MAX_BPS, MAX_HISTORY,
};

#[contract]
pub struct DataFeed;

#[contractimpl]
impl DataFeed {
    /// Initialises the protocol. Can only be called once.
    ///
    /// * `consensus_threshold` - providers required per round, must be > 0.
    /// * `round_duration` - submission window per round, must be > 0.
    /// * `staleness_window` - age at which a consensus value goes stale, > 0.
    /// * `max_history` - retained rounds per feed, `1..=MAX_HISTORY`.
    /// * `slashing` - penalty applied per slash event, `penalty_bps <= MAX_BPS`.
    pub fn initialize(
        env: Env,
        admin: Address,
        consensus_threshold: u32,
        round_duration: u64,
        staleness_window: u64,
        max_history: u32,
        slashing: SlashingConfig,
    ) -> Result<(), DataFeedError> {
        if Storage::has_config(&env) {
            return Err(DataFeedError::AlreadyInitialized);
        }
        if consensus_threshold == 0 || round_duration == 0 || staleness_window == 0 {
            return Err(DataFeedError::InvalidConfig);
        }
        if max_history == 0 || max_history > MAX_HISTORY {
            return Err(DataFeedError::InvalidConfig);
        }
        if slashing.penalty_bps > MAX_BPS {
            return Err(DataFeedError::InvalidPenalty);
        }

        let config = Config {
            admin: admin.clone(),
            consensus_threshold,
            round_duration,
            staleness_window,
            max_history,
            slashing: slashing.clone(),
        };
        Storage::set_config(&env, &config);
        Storage::set_stats(&env, &Analytics::empty());
        Storage::set_feed_count(&env, 0);

        env.events().publish(
            (symbol_short!("init"),),
            (admin, consensus_threshold, round_duration, staleness_window),
        );

        Ok(())
    }

    /// Returns the protocol configuration.
    pub fn get_config(env: Env) -> Result<Config, DataFeedError> {
        Storage::get_config(&env)
    }

    // ------------------------------------------------------------- provider registry

    /// Registers a bonded data provider. Admin only.
    pub fn register_provider(
        env: Env,
        provider: Address,
        stake: i128,
        weight: u32,
    ) -> Result<(), DataFeedError> {
        let config = Storage::get_config(&env)?;
        config.admin.require_auth();

        if weight == 0 {
            return Err(DataFeedError::InvalidWeight);
        }
        if stake < 0 {
            return Err(DataFeedError::InvalidStake);
        }
        if Storage::has_provider(&env, &provider) {
            return Err(DataFeedError::ProviderAlreadyExists);
        }

        let record = Provider {
            address: provider.clone(),
            weight,
            stake,
            reputation: INITIAL_REPUTATION,
            active: true,
            submissions: 0,
            slash_count: 0,
            registered_at: env.ledger().timestamp(),
            last_submission: 0,
        };
        Storage::set_provider(&env, &provider, &record);

        let mut registry = Storage::get_registry(&env);
        registry.push_back(provider.clone());
        Storage::set_registry(&env, &registry);

        let mut stats = Storage::get_stats(&env);
        stats.provider_count += 1;
        stats.active_providers += 1;
        stats.total_stake += stake;
        Storage::set_stats(&env, &stats);

        env.events()
            .publish((symbol_short!("prov_add"), provider), (weight, stake));

        Ok(())
    }

    /// Deactivates a provider. Admin only.
    ///
    /// The stake and reputation stay on record so that outstanding slash
    /// obligations and analytics remain meaningful.
    pub fn deregister_provider(env: Env, provider: Address) -> Result<(), DataFeedError> {
        let config = Storage::get_config(&env)?;
        config.admin.require_auth();

        let mut record = Storage::get_provider(&env, &provider)?;
        if !record.active {
            return Err(DataFeedError::ProviderNotActive);
        }
        record.active = false;
        Storage::set_provider(&env, &provider, &record);

        let mut stats = Storage::get_stats(&env);
        stats.active_providers = stats.active_providers.saturating_sub(1);
        Storage::set_stats(&env, &stats);

        env.events()
            .publish((symbol_short!("prov_del"), provider), record.weight);

        Ok(())
    }

    /// Sets the consensus weight of a provider. Admin only.
    pub fn set_provider_weight(
        env: Env,
        provider: Address,
        weight: u32,
    ) -> Result<(), DataFeedError> {
        let config = Storage::get_config(&env)?;
        config.admin.require_auth();

        if weight == 0 {
            return Err(DataFeedError::InvalidWeight);
        }

        let mut record = Storage::get_provider(&env, &provider)?;
        record.weight = weight;
        Storage::set_provider(&env, &provider, &record);

        env.events()
            .publish((symbol_short!("weight"), provider), weight);

        Ok(())
    }

    pub fn get_provider(env: Env, provider: Address) -> Result<Provider, DataFeedError> {
        Storage::get_provider(&env, &provider)
    }

    pub fn get_providers(env: Env) -> Result<Vec<Provider>, DataFeedError> {
        Storage::get_config(&env)?;
        let registry = Storage::get_registry(&env);
        let mut result = Vec::new(&env);
        for address in registry.iter() {
            result.push_back(Storage::get_provider(&env, &address)?);
        }
        Ok(result)
    }

    // ------------------------------------------------------------------- feed setup

    /// Creates a feed with its question, value bounds and first round. Admin only.
    pub fn create_feed(
        env: Env,
        feed_id: Symbol,
        question: Symbol,
        min_value: i128,
        max_value: i128,
    ) -> Result<(), DataFeedError> {
        let config = Storage::get_config(&env)?;
        config.admin.require_auth();

        if Storage::has_feed(&env, &feed_id) {
            return Err(DataFeedError::FeedAlreadyExists);
        }
        if min_value > max_value || max_value.checked_sub(min_value).is_none() {
            return Err(DataFeedError::InvalidBounds);
        }

        let now = env.ledger().timestamp();
        let feed = Feed {
            feed_id: feed_id.clone(),
            question: question.clone(),
            min_value,
            max_value,
            round: 1,
            round_start: now,
            submissions: Map::new(&env),
            submission_count: 0,
            submission_weight: 0,
            consensus_value: None,
            consensus_round: 0,
            consensus_timestamp: 0,
            consensus_contributors: 0,
            finalized_rounds: 0,
            failed_rounds: 0,
            total_submissions: 0,
            rejected_submissions: 0,
            created_at: now,
        };
        Storage::set_feed(&env, &feed_id, &feed);
        Storage::set_feed_count(&env, Storage::feed_count(&env).saturating_add(1));

        let mut stats = Storage::get_stats(&env);
        stats.feed_count += 1;
        Storage::set_stats(&env, &stats);

        env.events()
            .publish((symbol_short!("feed_new"), feed_id), (question, min_value, max_value));

        Ok(())
    }

    // ------------------------------------------------------------------- submissions

    /// Submits a validated value for the open round of a feed.
    ///
    /// Rejects unknown or inactive providers, closed submission windows,
    /// duplicate submissions and values outside the feed bounds.
    pub fn submit_data(
        env: Env,
        feed_id: Symbol,
        provider: Address,
        value: i128,
    ) -> Result<(), DataFeedError> {
        let config = Storage::get_config(&env)?;
        provider.require_auth();

        let mut feed = Storage::get_feed(&env, &feed_id)?;
        let mut record = Storage::get_provider(&env, &provider)?;
        if !record.active {
            return Err(DataFeedError::ProviderNotActive);
        }

        if value < feed.min_value || value > feed.max_value {
            return Err(DataFeedError::ValueOutOfBounds);
        }

        let now = env.ledger().timestamp();
        if !Self::window_open(&config, &feed, now) {
            return Err(DataFeedError::SubmissionWindowClosed);
        }
        if feed.submissions.contains_key(provider.clone()) {
            return Err(DataFeedError::AlreadySubmitted);
        }

        let submission = Submission {
            provider: provider.clone(),
            value,
            timestamp: now,
            round: feed.round,
            weight: record.weight,
        };
        feed.submissions.set(provider.clone(), submission);
        feed.submission_count += 1;
        feed.submission_weight += record.weight as u64;
        feed.total_submissions += 1;

        record.submissions += 1;
        record.last_submission = now;
        Storage::set_provider(&env, &provider, &record);
        Storage::set_feed(&env, &feed_id, &feed);

        let mut stats = Storage::get_stats(&env);
        stats.total_submissions += 1;
        Storage::set_stats(&env, &stats);

        env.events()
            .publish((symbol_short!("submit"), feed_id), (provider, value, feed.round));

        Ok(())
    }

    /// Returns the submission a provider made in the open round.
    pub fn get_submission(
        env: Env,
        feed_id: Symbol,
        provider: Address,
    ) -> Result<Submission, DataFeedError> {
        let feed = Storage::get_feed(&env, &feed_id)?;
        feed.submissions
            .get(provider)
            .ok_or(DataFeedError::ProviderNotFound)
    }

    // --------------------------------------------------------------------- consensus

    /// Advances the open round of a feed.
    ///
    /// * threshold reached -> the weighted median becomes the consensus value,
    ///   the round is appended to the history and the next round is opened;
    /// * window still open without the threshold -> `Pending`;
    /// * window closed without the threshold -> the round counts as failed, the
    ///   next round is opened and `NoConsensus` is returned.
    pub fn finalize(env: Env, feed_id: Symbol) -> Result<RoundOutcome, DataFeedError> {
        let config = Storage::get_config(&env)?;
        let mut feed = Storage::get_feed(&env, &feed_id)?;
        let now = env.ledger().timestamp();

        if feed.submission_count >= config.consensus_threshold {
            let value = match Self::weighted_median(&env, &feed) {
                Some(median) => median,
                None => feed.consensus_value.unwrap_or(0),
            };
            let record = ConsensusRecord {
                feed_id: feed_id.clone(),
                round: feed.round,
                value,
                timestamp: now,
                contributors: feed.submission_count,
                weight: feed.submission_weight,
            };

            feed.consensus_value = Some(value);
            feed.consensus_round = feed.round;
            feed.consensus_timestamp = now;
            feed.consensus_contributors = feed.submission_count;
            feed.finalized_rounds += 1;
            Self::open_next_round(&env, &mut feed, now);
            Storage::set_feed(&env, &feed_id, &feed);
            Storage::add_history(&env, &feed_id, &record, config.max_history);

            let mut stats = Storage::get_stats(&env);
            stats.finalized_rounds += 1;
            Storage::set_stats(&env, &stats);

            env.events()
                .publish((symbol_short!("final"), feed_id), record.clone());

            return Ok(RoundOutcome::Consensus(record));
        }

        if Self::window_open(&config, &feed, now) {
            return Ok(RoundOutcome::Pending);
        }

        let failed_round = feed.round;
        feed.failed_rounds += 1;
        Self::open_next_round(&env, &mut feed, now);
        Storage::set_feed(&env, &feed_id, &feed);

        let mut stats = Storage::get_stats(&env);
        stats.failed_rounds += 1;
        Storage::set_stats(&env, &stats);

        env.events()
            .publish((symbol_short!("roundfail"), feed_id), failed_round);

        Ok(RoundOutcome::NoConsensus)
    }

    /// Returns the current consensus value, rejecting stale values.
    pub fn get_consensus_value(
        env: Env,
        feed_id: Symbol,
    ) -> Result<(i128, u64, u64), DataFeedError> {
        let config = Storage::get_config(&env)?;
        let feed = Storage::get_feed(&env, &feed_id)?;
        let value = feed.consensus_value.ok_or(DataFeedError::NoConsensusValue)?;
        if Self::is_stale_now(&env, &config, &feed) {
            return Err(DataFeedError::StaleData);
        }
        Ok((value, feed.consensus_round, feed.consensus_timestamp))
    }

    /// Returns the current consensus value together with an `is_stale` flag.
    pub fn get_consensus(env: Env, feed_id: Symbol) -> Result<ConsensusValue, DataFeedError> {
        let config = Storage::get_config(&env)?;
        let feed = Storage::get_feed(&env, &feed_id)?;
        let value = feed.consensus_value.ok_or(DataFeedError::NoConsensusValue)?;
        Ok(ConsensusValue {
            feed_id: feed_id.clone(),
            round: feed.consensus_round,
            value,
            timestamp: feed.consensus_timestamp,
            contributors: feed.consensus_contributors,
            is_stale: Self::is_stale_now(&env, &config, &feed),
        })
    }

    /// `true` when the feed holds a consensus value that is older than the
    /// staleness window.
    pub fn is_stale(env: Env, feed_id: Symbol) -> Result<bool, DataFeedError> {
        let config = Storage::get_config(&env)?;
        let feed = Storage::get_feed(&env, &feed_id)?;
        if feed.consensus_value.is_none() {
            return Err(DataFeedError::NoConsensusValue);
        }
        Ok(Self::is_stale_now(&env, &config, &feed))
    }

    /// `true` when the open round already has enough submissions.
    pub fn is_consensus_reached(env: Env, feed_id: Symbol) -> Result<bool, DataFeedError> {
        let config = Storage::get_config(&env)?;
        let feed = Storage::get_feed(&env, &feed_id)?;
        Ok(feed.submission_count >= config.consensus_threshold)
    }

    pub fn get_feed(env: Env, feed_id: Symbol) -> Result<Feed, DataFeedError> {
        let feed = Storage::get_feed(&env, &feed_id)?;
        Ok(feed)
    }

    pub fn feed_count(env: Env) -> Result<u32, DataFeedError> {
        Storage::get_config(&env)?;
        Ok(Storage::feed_count(&env))
    }

    // ---------------------------------------------------------------------- history

    /// Returns up to `limit` of the most recent finalized rounds, oldest first.
    pub fn get_history(
        env: Env,
        feed_id: Symbol,
        limit: u32,
    ) -> Result<Vec<ConsensusRecord>, DataFeedError> {
        Storage::get_feed(&env, &feed_id)?;
        Ok(Storage::get_history(&env, &feed_id, limit))
    }

    /// Returns a single finalized round from the retained history.
    pub fn get_round(
        env: Env,
        feed_id: Symbol,
        round: u64,
    ) -> Result<ConsensusRecord, DataFeedError> {
        Storage::get_config(&env)?;
        Storage::get_feed(&env, &feed_id)?;
        let history = Storage::get_history(&env, &feed_id, MAX_HISTORY);
        for record in history.iter() {
            if record.round == round {
                return Ok(record);
            }
        }
        Err(DataFeedError::RoundNotFound)
    }

    // --------------------------------------------------------------------- slashing

    /// Slashes a provider with the configured penalty. Admin only.
    ///
    /// Returns the stake that was actually burned; the stake and reputation can
    /// never drop below zero.
    pub fn slash_provider(
        env: Env,
        provider: Address,
        reason: Symbol,
    ) -> Result<i128, DataFeedError> {
        let config = Storage::get_config(&env)?;
        config.admin.require_auth();
        Self::apply_slash(&env, &provider, reason, &config)
    }

    /// Challenges a provider with a value the contract itself proves to be out
    /// of the feed bounds. Any other active provider may file the challenge.
    ///
    /// Returns the stake that was actually burned.
    pub fn report_bad_data(
        env: Env,
        feed_id: Symbol,
        challenger: Address,
        provider: Address,
        value: i128,
    ) -> Result<i128, DataFeedError> {
        let config = Storage::get_config(&env)?;
        challenger.require_auth();

        if !Storage::has_provider(&env, &challenger) || challenger == provider {
            return Err(DataFeedError::Unauthorized);
        }
        let challenger_record = Storage::get_provider(&env, &challenger)?;
        if !challenger_record.active {
            return Err(DataFeedError::Unauthorized);
        }

        let mut feed = Storage::get_feed(&env, &feed_id)?;
        let target = Storage::get_provider(&env, &provider)?;
        if !target.active {
            return Err(DataFeedError::ProviderNotActive);
        }
        if value >= feed.min_value && value <= feed.max_value {
            return Err(DataFeedError::NoViolationProven);
        }

        feed.rejected_submissions += 1;
        Storage::set_feed(&env, &feed_id, &feed);

        let mut stats = Storage::get_stats(&env);
        stats.rejected_submissions += 1;
        Storage::set_stats(&env, &stats);

        let penalty = Self::apply_slash(&env, &provider, symbol_short!("oob"), &config)?;

        env.events().publish(
            (symbol_short!("report"), feed_id),
            (challenger, provider, value, penalty),
        );

        Ok(penalty)
    }

    // ------------------------------------------------------------------- analytics

    /// Protocol wide counters, derived from the stored state.
    pub fn get_analytics(env: Env) -> Result<Analytics, DataFeedError> {
        Storage::get_config(&env)?;
        Ok(Storage::get_stats(&env))
    }

    /// Per-feed counters, derived from the stored feed and its history.
    pub fn get_feed_stats(env: Env, feed_id: Symbol) -> Result<FeedStats, DataFeedError> {
        let config = Storage::get_config(&env)?;
        let feed = Storage::get_feed(&env, &feed_id)?;
        let has_consensus = feed.consensus_value.is_some();
        let is_stale = has_consensus && Self::is_stale_now(&env, &config, &feed);

        Ok(FeedStats {
            feed_id: feed_id.clone(),
            current_round: feed.round,
            round_submissions: feed.submission_count,
            round_submission_weight: feed.submission_weight,
            consensus_reached: feed.submission_count >= config.consensus_threshold,
            has_consensus,
            is_stale,
            total_submissions: feed.total_submissions,
            finalized_rounds: feed.finalized_rounds,
            failed_rounds: feed.failed_rounds,
            rejected_submissions: feed.rejected_submissions,
            history_len: Storage::get_history(&env, &feed_id, MAX_HISTORY).len(),
        })
    }

    // ---------------------------------------------------------------------- helpers

    fn window_open(config: &Config, feed: &Feed, now: u64) -> bool {
        now < feed.round_start.saturating_add(config.round_duration)
    }

    fn is_stale_now(env: &Env, config: &Config, feed: &Feed) -> bool {
        let deadline = feed
            .consensus_timestamp
            .saturating_add(config.staleness_window);
        env.ledger().timestamp() > deadline
    }

    fn open_next_round(env: &Env, feed: &mut Feed, now: u64) {
        feed.round = feed.round.saturating_add(1);
        feed.round_start = now;
        feed.submissions = Map::new(env);
        feed.submission_count = 0;
        feed.submission_weight = 0;
    }

    /// Weighted median of the values submitted in the open round.
    fn weighted_median(env: &Env, feed: &Feed) -> Option<i128> {
        let mut values: Vec<i128> = Vec::new(env);
        let mut weights: Vec<u32> = Vec::new(env);
        for (_, submission) in feed.submissions.iter() {
            values.push_back(submission.value);
            weights.push_back(submission.weight);
        }

        let len = values.len();
        if len == 0 {
            return None;
        }

        let mut total: u64 = 0;
        for weight in weights.iter() {
            total += weight as u64;
        }

        for i in 0..len {
            for j in (i + 1)..len {
                if values.get(i).unwrap() > values.get(j).unwrap() {
                    let value = values.get(i).unwrap();
                    values.set(i, values.get(j).unwrap());
                    values.set(j, value);
                    let weight = weights.get(i).unwrap();
                    weights.set(i, weights.get(j).unwrap());
                    weights.set(j, weight);
                }
            }
        }

        let half = (total + 1) / 2;
        let mut accumulated: u64 = 0;
        for i in 0..len {
            accumulated += weights.get(i).unwrap() as u64;
            if accumulated >= half {
                return values.get(i);
            }
        }

        values.get(len - 1)
    }

    /// Burns the configured share of the provider stake plus reputation.
    ///
    /// Both quantities are clamped at zero, so a provider can never be slashed
    /// below zero. Returns the stake that was actually burned.
    fn apply_slash(
        env: &Env,
        provider: &Address,
        reason: Symbol,
        config: &Config,
    ) -> Result<i128, DataFeedError> {
        let mut record = Storage::get_provider(env, provider)?;
        let bps = config.slashing.penalty_bps as i128;
        let denominator = MAX_BPS as i128;

        // Split division keeps the multiplication away from an i128 overflow.
        let raw =
            (record.stake / denominator) * bps + (record.stake % denominator) * bps / denominator;
        let reputation_penalty = config.slashing.reputation_penalty;

        if raw == 0 && reputation_penalty == 0 {
            return Err(DataFeedError::NothingToSlash);
        }
        if record.stake == 0 && record.reputation == 0 {
            return Err(DataFeedError::NothingToSlash);
        }

        let burned = if raw > record.stake { record.stake } else { raw };
        record.stake -= burned;
        if reputation_penalty > record.reputation {
            record.reputation = 0;
        } else {
            record.reputation -= reputation_penalty;
        }
        record.slash_count += 1;
        Storage::set_provider(env, provider, &record);

        let mut stats = Storage::get_stats(env);
        stats.slash_events += 1;
        stats.total_stake = stats.total_stake.saturating_sub(burned);
        Storage::set_stats(env, &stats);

        env.events().publish(
            (symbol_short!("slash"), provider.clone()),
            (reason, burned, record.stake, record.reputation),
        );

        Ok(burned)
    }
}

mod test;
