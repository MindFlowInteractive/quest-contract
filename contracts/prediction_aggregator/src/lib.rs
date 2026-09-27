#![no_std]

//! Prediction market aggregator.
//!
//! Aggregates the probabilistic forecasts of a panel of registered sources into
//! a single weighted consensus forecast, and keeps the panel honest by scoring
//! it against resolved outcomes.
//!
//! Units
//! -----
//! Every probability, weight, deviation, confidence and accuracy figure is an
//! integer number of **basis points** (`10_000 == 100%`). A probability is a
//! `u32` in `0..=10_000`; signed quantities (bias, error) are `i128` in bps.
//! Nothing in this contract uses floating point, so the whole pipeline is
//! deterministic, exactly reproducible integer math.
//!
//! Rules
//! -----
//! * **Sources** - the admin curates the panel with `register_source` and
//!   `deregister_source`. Every source carries a weight; the weights of the
//!   active sources are expected to total exactly `MAX_BPS`, and the weight
//!   setters enforce that invariant: `set_source_weights` rejects a set that
//!   does not total `10_000`, and `set_source_weight` rebalances the remaining
//!   active sources so the total is preserved.
//! * **Markets** - the admin opens a market with `create_prediction_market`
//!   (question, target, resolution deadline, outcome range, minimum number of
//!   sources). A registered source submits at most one prediction per market
//!   with `submit_prediction` before the deadline.
//! * **Bias correction** - when a market is resolved, every prediction is
//!   scored: a forecast within `accuracy_tolerance_bps` of the true outcome is
//!   a *hit*, and the signed error is accumulated into the source's running
//!   bias. The average bias is subtracted from every future forecast of that
//!   source, clamped to `max_correction_bps` so a source with a pathological
//!   track record can never swing the aggregate.
//! * **Outlier detection** - the weighted mean over all predictions is computed
//!   first, then a source whose probability deviates from it by strictly more
//!   than `outlier_threshold_bps` is dropped. One trim pass around the pre-trim
//!   mean keeps the filter deterministic and cheap. The dropped sources are
//!   reported on the forecast.
//! * **Consensus** - the published probability is the weighted mean of the
//!   surviving (already bias corrected) predictions:
//!
//!   ```text
//!   probability_bps = round_down( sum(w_i * p_i) / sum(w_i) )
//!   ```
//!
//!   with `w_i` the weight captured at submission time and `p_i` the bias
//!   corrected probability. If fewer than the required number of sources
//!   survive the trim, the call fails with `InsufficientSources` rather than
//!   publishing a bogus forecast.
//! * **Confidence** - derived from how tightly the sources agree and from how
//!   many of them there are, both in bps:
//!
//!   ```text
//!   mad_bps        = sum(w_i * |p_i - probability_bps|) / sum(w_i)
//!   agreement_bps  = 10_000 - mad_bps
//!   count_bps      = min(10_000, used * 10_000 / (2 * min_sources))
//!   confidence_bps = agreement_bps * count_bps / 10_000
//!   ```
//!
//!   A tight cluster scores near `count_bps`, a panel that disagrees scores
//!   below it. A forecast is only stored and published when its confidence
//!   reaches `min_confidence_bps`.
//! * **History** - every published forecast is appended to a per-market history
//!   retaining at most `history_cap` entries (hard cap `MAX_HISTORY`).

use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Map, String, Symbol, Vec};

mod storage;
mod types;

use storage::Storage;
use types::{
    AggregatorAnalytics, AggregatorError, BiasConfig, Config, ConsensusForecast, Market,
    MarketStatus, Prediction, Resolution, Source, SourceAccuracy, SourceSummary, SourceWeight,
    MAX_BPS, MAX_HISTORY,
};

#[contract]
pub struct PredictionAggregator;

#[contractimpl]
impl PredictionAggregator {
    // -------------------------------------------------------------------- setup

    /// Initialises the protocol. Can only be called once.
    ///
    /// * `default_source_weight` - weight given to a new source, `1..=MAX_BPS`.
    /// * `min_sources` - sources required before a forecast may be published, > 0.
    /// * `outlier_threshold_bps` - largest tolerated deviation from the weighted
    ///   mean before a source is trimmed, `0..=MAX_BPS`.
    /// * `min_confidence_bps` - minimum confidence required to publish, `0..=MAX_BPS`.
    /// * `history_cap` - retained forecasts per market, `1..=MAX_HISTORY`.
    /// * `bias` - `max_correction_bps` and `accuracy_tolerance_bps`, both `0..=MAX_BPS`.
    pub fn initialize(
        env: Env,
        admin: Address,
        default_source_weight: u32,
        min_sources: u32,
        outlier_threshold_bps: u32,
        min_confidence_bps: u32,
        history_cap: u32,
        bias: BiasConfig,
    ) -> Result<(), AggregatorError> {
        if Storage::has_config(&env) {
            return Err(AggregatorError::AlreadyInitialized);
        }
        if default_source_weight == 0 || default_source_weight > MAX_BPS {
            return Err(AggregatorError::InvalidConfig);
        }
        if min_sources == 0 {
            return Err(AggregatorError::InvalidConfig);
        }
        if outlier_threshold_bps > MAX_BPS || min_confidence_bps > MAX_BPS {
            return Err(AggregatorError::InvalidConfig);
        }
        if history_cap == 0 || history_cap > MAX_HISTORY {
            return Err(AggregatorError::InvalidConfig);
        }
        if bias.max_correction_bps > MAX_BPS || bias.accuracy_tolerance_bps > MAX_BPS {
            return Err(AggregatorError::InvalidConfig);
        }

        let config = Config {
            admin: admin.clone(),
            default_source_weight,
            min_sources,
            outlier_threshold_bps,
            min_confidence_bps,
            history_cap,
            bias: bias.clone(),
        };
        Storage::set_config(&env, &config);
        Storage::set_stats(&env, &AggregatorAnalytics::empty(&env));
        Storage::set_market_count(&env, 0);

        env.events().publish(
            (symbol_short!("init"),),
            (admin, default_source_weight, min_sources),
        );

        Ok(())
    }

    /// Returns the protocol configuration.
    pub fn get_config(env: Env) -> Result<Config, AggregatorError> {
        Storage::get_config(&env)
    }

    /// Raises or lowers the confidence a forecast must reach to be published.
    /// Admin only.
    pub fn set_min_confidence(
        env: Env,
        admin: Address,
        min_confidence_bps: u32,
    ) -> Result<(), AggregatorError> {
        Self::require_admin(&env, &admin)?;
        if min_confidence_bps > MAX_BPS {
            return Err(AggregatorError::InvalidConfig);
        }

        let mut config = Storage::get_config(&env)?;
        config.min_confidence_bps = min_confidence_bps;
        Storage::set_config(&env, &config);

        env.events()
            .publish((symbol_short!("conf"),), min_confidence_bps);

        Ok(())
    }

    /// Raises or lowers the outlier tolerance. Admin only.
    pub fn set_outlier_threshold(
        env: Env,
        admin: Address,
        outlier_threshold_bps: u32,
    ) -> Result<(), AggregatorError> {
        Self::require_admin(&env, &admin)?;
        if outlier_threshold_bps > MAX_BPS {
            return Err(AggregatorError::InvalidConfig);
        }

        let mut config = Storage::get_config(&env)?;
        config.outlier_threshold_bps = outlier_threshold_bps;
        Storage::set_config(&env, &config);

        env.events()
            .publish((symbol_short!("thresh"),), outlier_threshold_bps);

        Ok(())
    }

    // ----------------------------------------------------------- source registry

    /// Adds a source to the panel. Admin only.
    ///
    /// The source starts with `default_source_weight`; the admin is expected to
    /// rebalance the panel with `set_source_weight`/`set_source_weights` so the
    /// active weights total `MAX_BPS` again.
    pub fn register_source(env: Env, source: Address) -> Result<(), AggregatorError> {
        let config = Self::admin_only(&env)?;

        if Storage::has_source(&env, &source) {
            return Err(AggregatorError::SourceAlreadyExists);
        }

        let now = env.ledger().timestamp();
        let record = Source {
            address: source.clone(),
            weight_bps: config.default_source_weight,
            active: true,
            predictions: 0,
            resolved: 0,
            hits: 0,
            misses: 0,
            bias_sum_bps: 0,
            bias_bps: 0,
            registered_at: now,
            last_submission: 0,
        };
        Storage::set_source(&env, &source, &record);

        let mut registry = Storage::get_registry(&env);
        registry.push_back(source.clone());
        Storage::set_registry(&env, &registry);

        let mut stats = Storage::get_stats(&env);
        stats.source_count += 1;
        stats.active_sources += 1;
        Storage::set_stats(&env, &stats);

        env.events()
            .publish((symbol_short!("src_add"), source), config.default_source_weight);

        Ok(())
    }

    /// Removes a source from the panel. Admin only.
    ///
    /// The record and its track record stay on book so that history, accuracy
    /// and bias remain meaningful. The freed weight has to be redistributed by
    /// the admin to restore the `MAX_BPS` total.
    pub fn deregister_source(env: Env, source: Address) -> Result<(), AggregatorError> {
        Self::admin_only(&env)?;

        let mut record = Storage::get_source(&env, &source)?;
        if !record.active {
            return Err(AggregatorError::SourceNotActive);
        }
        record.active = false;
        Storage::set_source(&env, &source, &record);

        let registry = Storage::get_registry(&env);
        let mut next = Vec::new(&env);
        for entry in registry.iter() {
            if entry != source {
                next.push_back(entry);
            }
        }
        Storage::set_registry(&env, &next);

        let mut stats = Storage::get_stats(&env);
        stats.active_sources = stats.active_sources.saturating_sub(1);
        Storage::set_stats(&env, &stats);

        env.events()
            .publish((symbol_short!("src_del"), source), record.weight_bps);

        Ok(())
    }

    /// Sets the weight of a single source and rebalances the remaining active
    /// sources so the panel keeps totalling exactly `MAX_BPS` bps. Admin only.
    ///
    /// The other sources keep their *relative* shares: each is scaled by
    /// `(MAX_BPS - weight_bps) / (total - old_weight)`, and the rounding
    /// remainder is handed out one bps at a time in registry order.
    ///
    /// Returns the new total weight, always `MAX_BPS`.
    pub fn set_source_weight(
        env: Env,
        source: Address,
        weight_bps: u32,
        admin: Address,
    ) -> Result<u32, AggregatorError> {
        Self::require_admin(&env, &admin)?;

        if weight_bps == 0 || weight_bps > MAX_BPS {
            return Err(AggregatorError::InvalidWeight);
        }

        let mut target = Storage::get_source(&env, &source)?;
        if !target.active {
            return Err(AggregatorError::SourceNotActive);
        }

        let registry = Storage::get_registry(&env);
        let mut others: Vec<Address> = Vec::new(&env);
        let mut others_weight: Vec<u64> = Vec::new(&env);
        let mut others_total: u64 = 0;

        for entry in registry.iter() {
            if entry == source {
                continue;
            }
            let record = Storage::get_source(&env, &entry)?;
            if !record.active {
                continue;
            }
            others.push_back(entry);
            others_weight.push_back(record.weight_bps as u64);
            others_total += record.weight_bps as u64;
        }

        let new_others_total = MAX_BPS as u64 - weight_bps as u64;

        if others_total == 0 {
            // Sole active source: it has to carry the whole book by itself.
            if new_others_total != 0 {
                return Err(AggregatorError::InvalidWeight);
            }
        } else {
            let count = others.len();
            let mut assigned: u64 = 0;
            for i in 0..count {
                let share = others_weight.get(i).unwrap() * new_others_total / others_total;
                assigned += share;
                let address = others.get(i).unwrap();
                let mut record = Storage::get_source(&env, &address)?;
                record.weight_bps = share as u32;
                Storage::set_source(&env, &address, &record);
            }

            // Hand out the rounding remainder so the panel totals exactly
            // `new_others_total`; the remainder is always smaller than `count`.
            let mut leftover = new_others_total - assigned;
            let mut i: u32 = 0;
            while leftover > 0 && i < count {
                let address = others.get(i).unwrap();
                let mut record = Storage::get_source(&env, &address)?;
                record.weight_bps += 1;
                Storage::set_source(&env, &address, &record);
                leftover -= 1;
                i += 1;
            }
        }

        target.weight_bps = weight_bps;
        Storage::set_source(&env, &source, &target);

        env.events()
            .publish((symbol_short!("weight"), source), weight_bps);

        Ok(MAX_BPS)
    }

    /// Sets several source weights in one call. Admin only.
    ///
    /// Sources that are not part of `weights` keep their current weight; the
    /// resulting panel total must equal `MAX_BPS`, otherwise the whole call is
    /// rejected. Rejects duplicates, unknown sources, inactive sources and
    /// weights outside `1..=MAX_BPS`.
    pub fn set_source_weights(
        env: Env,
        weights: Vec<SourceWeight>,
        admin: Address,
    ) -> Result<u32, AggregatorError> {
        Self::require_admin(&env, &admin)?;

        let mut updates: Map<Address, u32> = Map::new(&env);
        for item in weights.iter() {
            if item.weight_bps == 0 || item.weight_bps > MAX_BPS {
                return Err(AggregatorError::InvalidWeight);
            }
            if updates.contains_key(item.source.clone()) {
                return Err(AggregatorError::InvalidWeight);
            }
            let record = Storage::get_source(&env, &item.source)?;
            if !record.active {
                return Err(AggregatorError::SourceNotActive);
            }
            updates.set(item.source.clone(), item.weight_bps);
        }
        if updates.len() != weights.len() {
            return Err(AggregatorError::InvalidWeight);
        }

        let registry = Storage::get_registry(&env);
        let mut total: u64 = 0;
        for entry in registry.iter() {
            let record = Storage::get_source(&env, &entry)?;
            if !record.active {
                continue;
            }
            let weight = updates.get(entry.clone()).unwrap_or(record.weight_bps);
            total += weight as u64;
        }
        if total != MAX_BPS as u64 {
            return Err(AggregatorError::InvalidWeight);
        }

        for (address, weight) in updates.iter() {
            let mut record = Storage::get_source(&env, &address)?;
            record.weight_bps = weight;
            Storage::set_source(&env, &address, &record);
        }

        env.events()
            .publish((symbol_short!("weights"),), weights.len());

        Ok(MAX_BPS)
    }

    /// Summed weight of the active sources, in bps.
    pub fn total_weight(env: Env) -> Result<u32, AggregatorError> {
        Storage::get_config(&env)?;
        let registry = Storage::get_registry(&env);
        Ok(Self::sum_active_weight(&env, &registry))
    }

    pub fn get_source(env: Env, source: Address) -> Result<Source, AggregatorError> {
        Storage::get_source(&env, &source)
    }

    pub fn get_sources(env: Env) -> Result<Vec<Source>, AggregatorError> {
        Storage::get_config(&env)?;
        let registry = Storage::get_registry(&env);
        let mut result = Vec::new(&env);
        for address in registry.iter() {
            result.push_back(Storage::get_source(&env, &address)?);
        }
        Ok(result)
    }

    /// Historical hit rate of a source, in bps.
    pub fn get_source_accuracy(env: Env, source: Address) -> Result<SourceAccuracy, AggregatorError> {
        let record = Storage::get_source(&env, &source)?;
        Ok(SourceAccuracy {
            source: record.address.clone(),
            resolved: record.resolved,
            hits: record.hits,
            misses: record.misses,
            accuracy_bps: Self::accuracy_bps(record.hits, record.resolved),
            bias_bps: record.bias_bps,
        })
    }

    /// Average signed error of a source against resolved outcomes, in bps.
    ///
    /// A positive bias means the source systematically over-predicts.
    pub fn get_source_bias(env: Env, source: Address) -> Result<i128, AggregatorError> {
        let record = Storage::get_source(&env, &source)?;
        Ok(record.bias_bps)
    }

    // ------------------------------------------------------------------ markets

    /// Opens a market. Admin only.
    ///
    /// * `resolution_deadline` - ledger timestamp from which predictions are
    ///   refused, must lie in the future.
    /// * `min_outcome_bps..=max_outcome_bps` - range the true outcome must fall in.
    /// * `min_sources` - sources required for this market; the protocol wide
    ///   `min_sources` is used when a lower value is requested.
    pub fn create_prediction_market(
        env: Env,
        market_id: Symbol,
        question: String,
        target: Symbol,
        resolution_deadline: u64,
        min_outcome_bps: u32,
        max_outcome_bps: u32,
        min_sources: u32,
    ) -> Result<(), AggregatorError> {
        let config = Self::admin_only(&env)?;

        if Storage::has_market(&env, &market_id) {
            return Err(AggregatorError::MarketAlreadyExists);
        }
        if min_outcome_bps > max_outcome_bps || max_outcome_bps > MAX_BPS {
            return Err(AggregatorError::InvalidRange);
        }

        let now = env.ledger().timestamp();
        if resolution_deadline <= now {
            return Err(AggregatorError::InvalidConfig);
        }

        let required = if min_sources < config.min_sources {
            config.min_sources
        } else {
            min_sources
        };

        let market = Market {
            market_id: market_id.clone(),
            question: question.clone(),
            target: target.clone(),
            resolution_deadline,
            min_outcome_bps,
            max_outcome_bps,
            min_sources: required,
            predictions: Map::new(&env),
            prediction_count: 0,
            closed: false,
            resolved: false,
            true_probability_bps: None,
            resolved_at: 0,
            scored_sources: 0,
            hits: 0,
            misses: 0,
            last_forecast: None,
            created_at: now,
        };
        Storage::set_market(&env, &market_id, &market);
        Storage::set_market_count(&env, Storage::market_count(&env).saturating_add(1));

        let mut stats = Storage::get_stats(&env);
        stats.market_count += 1;
        Storage::set_stats(&env, &stats);

        env.events().publish(
            (symbol_short!("mkt_new"), market_id),
            (target, resolution_deadline, required),
        );

        Ok(())
    }

    /// Closes a market without resolving it. Admin only, after the deadline.
    pub fn close_market(env: Env, market_id: Symbol) -> Result<(), AggregatorError> {
        Self::admin_only(&env)?;

        let mut market = Storage::get_market(&env, &market_id)?;
        if market.resolved {
            return Err(AggregatorError::MarketResolved);
        }
        if market.closed {
            return Err(AggregatorError::MarketClosed);
        }
        if env.ledger().timestamp() < market.resolution_deadline {
            return Err(AggregatorError::DeadlineNotReached);
        }

        market.closed = true;
        Storage::set_market(&env, &market_id, &market);

        let mut stats = Storage::get_stats(&env);
        stats.closed_count += 1;
        Storage::set_stats(&env, &stats);

        env.events()
            .publish((symbol_short!("mkt_close"), market_id), env.ledger().timestamp());

        Ok(())
    }

    /// Resolves a market with the true outcome and scores every prediction.
    /// Admin only, at or after the resolution deadline.
    ///
    /// The true outcome must lie inside the range fixed when the market was
    /// created. Each prediction is a *hit* when
    /// `|corrected_probability - outcome| <= accuracy_tolerance_bps`; the signed
    /// error is added to the source's running bias, so later forecasts of that
    /// source arrive bias corrected.
    pub fn resolve_market(
        env: Env,
        market_id: Symbol,
        true_probability_bps: u32,
    ) -> Result<Resolution, AggregatorError> {
        let config = Self::admin_only(&env)?;

        let mut market = Storage::get_market(&env, &market_id)?;
        if market.resolved {
            return Err(AggregatorError::MarketResolved);
        }
        if true_probability_bps > MAX_BPS {
            return Err(AggregatorError::InvalidProbability);
        }
        if true_probability_bps < market.min_outcome_bps
            || true_probability_bps > market.max_outcome_bps
        {
            return Err(AggregatorError::InvalidOutcome);
        }
        if env.ledger().timestamp() < market.resolution_deadline {
            return Err(AggregatorError::DeadlineNotReached);
        }

        let tolerance = config.bias.accuracy_tolerance_bps as i128;
        let truth = true_probability_bps as i128;

        let predictions = Self::predictions(&env, &market);
        let mut hits: u32 = 0;
        let mut misses: u32 = 0;
        let mut scored: u32 = 0;

        for prediction in predictions.iter() {
            let mut record = Storage::get_source(&env, &prediction.source)?;
            let error = prediction.probability_bps as i128 - truth;
            let magnitude = if error < 0 { -error } else { error };

            record.resolved += 1;
            if magnitude <= tolerance {
                record.hits += 1;
                hits += 1;
            } else {
                record.misses += 1;
                misses += 1;
            }
            record.bias_sum_bps += error;
            record.bias_bps = record.bias_sum_bps / record.resolved as i128;
            Storage::set_source(&env, &prediction.source, &record);

            scored += 1;
        }

        let now = env.ledger().timestamp();
        market.resolved = true;
        market.closed = true;
        market.true_probability_bps = Some(true_probability_bps);
        market.resolved_at = now;
        market.scored_sources = scored;
        market.hits = hits;
        market.misses = misses;
        Storage::set_market(&env, &market_id, &market);

        let mut stats = Storage::get_stats(&env);
        stats.resolved_count += 1;
        stats.hits += hits as u64;
        stats.misses += misses as u64;
        Storage::set_stats(&env, &stats);

        let resolution = Resolution {
            market_id: market_id.clone(),
            true_probability_bps,
            resolved_at: now,
            scored_sources: scored,
            hits,
            misses,
        };

        env.events().publish(
            (symbol_short!("resolve"), market_id),
            (true_probability_bps, scored, hits),
        );

        Ok(resolution)
    }

    pub fn get_market(env: Env, market_id: Symbol) -> Result<Market, AggregatorError> {
        Storage::get_market(&env, &market_id)
    }

    /// Open, closed or resolved.
    pub fn get_market_status(env: Env, market_id: Symbol) -> Result<MarketStatus, AggregatorError> {
        let market = Storage::get_market(&env, &market_id)?;
        Ok(Self::status(&market))
    }

    /// Returns a market that has been resolved.
    pub fn get_resolved_market(
        env: Env,
        market_id: Symbol,
    ) -> Result<Resolution, AggregatorError> {
        let market = Storage::get_market(&env, &market_id)?;
        let outcome = market
            .true_probability_bps
            .ok_or(AggregatorError::MarketNotResolved)?;
        Ok(Resolution {
            market_id: market_id.clone(),
            true_probability_bps: outcome,
            resolved_at: market.resolved_at,
            scored_sources: market.scored_sources,
            hits: market.hits,
            misses: market.misses,
        })
    }

    pub fn market_count(env: Env) -> Result<u32, AggregatorError> {
        Storage::get_config(&env)?;
        Ok(Storage::market_count(&env))
    }

    // --------------------------------------------------------------- predictions

    /// Submits a probability forecast for a market. One per source per market.
    ///
    /// Rejects unknown or inactive sources, probabilities above `MAX_BPS`, a
    /// second prediction by the same source, resolved markets and any
    /// submission at or after the resolution deadline.
    ///
    /// The stored probability is bias corrected with the source's running bias,
    /// clamped to the configured `max_correction_bps` and to `0..=MAX_BPS`.
    pub fn submit_prediction(
        env: Env,
        market_id: Symbol,
        source: Address,
        probability_bps: u32,
    ) -> Result<(), AggregatorError> {
        let config = Storage::get_config(&env)?;
        source.require_auth();

        if probability_bps > MAX_BPS {
            return Err(AggregatorError::InvalidProbability);
        }

        let mut market = Storage::get_market(&env, &market_id)?;
        let mut record = Storage::get_source(&env, &source)?;
        if !record.active {
            return Err(AggregatorError::SourceNotActive);
        }
        if market.resolved {
            return Err(AggregatorError::MarketResolved);
        }

        let now = env.ledger().timestamp();
        if market.closed || now >= market.resolution_deadline {
            return Err(AggregatorError::SubmissionWindowClosed);
        }
        if market.predictions.contains_key(source.clone()) {
            return Err(AggregatorError::AlreadySubmitted);
        }

        let correction = Self::clamp_bias(record.bias_bps, config.bias.max_correction_bps);
        let corrected =
            Self::clamp_bps(probability_bps as i128 - correction, 0, MAX_BPS as i128) as u32;

        let prediction = Prediction {
            market_id: market_id.clone(),
            source: source.clone(),
            raw_probability_bps: probability_bps,
            probability_bps: corrected,
            bias_applied_bps: correction,
            weight_bps: record.weight_bps,
            timestamp: now,
        };

        market.predictions.set(source.clone(), prediction);
        market.prediction_count += 1;

        record.predictions += 1;
        record.last_submission = now;
        Storage::set_source(&env, &source, &record);
        Storage::set_market(&env, &market_id, &market);

        let mut stats = Storage::get_stats(&env);
        stats.total_predictions += 1;
        Storage::set_stats(&env, &stats);

        env.events().publish(
            (symbol_short!("predict"), market_id),
            (source, probability_bps, corrected),
        );

        Ok(())
    }

    pub fn get_prediction(
        env: Env,
        market_id: Symbol,
        source: Address,
    ) -> Result<Prediction, AggregatorError> {
        let market = Storage::get_market(&env, &market_id)?;
        market
            .predictions
            .get(source)
            .ok_or(AggregatorError::SourceNotFound)
    }

    pub fn get_predictions(
        env: Env,
        market_id: Symbol,
    ) -> Result<Vec<Prediction>, AggregatorError> {
        let market = Storage::get_market(&env, &market_id)?;
        Ok(Self::predictions(&env, &market))
    }

    // ----------------------------------------------------------------- consensus

    /// Aggregates the stored predictions of a market into a consensus forecast.
    ///
    /// Fails with `InsufficientSources` when fewer than the market requires
    /// survive the outlier trim. The forecast is stored, appended to the market
    /// history and published as an event only when its confidence reaches the
    /// configured `min_confidence_bps`; `published` reports which happened.
    pub fn aggregate(env: Env, market_id: Symbol) -> Result<ConsensusForecast, AggregatorError> {
        let config = Storage::get_config(&env)?;
        let mut market = Storage::get_market(&env, &market_id)?;

        let forecast = Self::compute(&env, &config, &market)?;

        if forecast.published {
            market.last_forecast = Some(forecast.clone());
            Storage::set_market(&env, &market_id, &market);
            Storage::add_forecast(&env, &market_id, &forecast, config.history_cap);

            let mut stats = Storage::get_stats(&env);
            stats.published_forecasts += 1;
            stats.excluded_observations += forecast.excluded.len() as u64;
            stats.confidence_sum += forecast.confidence_bps as u64;
            Storage::set_stats(&env, &stats);

            env.events()
                .publish((symbol_short!("consen"), market_id), forecast.clone());
        }

        Ok(forecast)
    }

    /// The last forecast of a market that met the minimum confidence.
    pub fn get_forecast(env: Env, market_id: Symbol) -> Result<ConsensusForecast, AggregatorError> {
        let market = Storage::get_market(&env, &market_id)?;
        market.last_forecast.ok_or(AggregatorError::NoConsensus)
    }

    /// Up to `limit` of the most recently published forecasts, oldest first.
    pub fn get_market_history(
        env: Env,
        market_id: Symbol,
        limit: u32,
    ) -> Result<Vec<ConsensusForecast>, AggregatorError> {
        Storage::get_market(&env, &market_id)?;
        Ok(Storage::get_history(&env, &market_id, limit))
    }

    // ----------------------------------------------------------------- analytics

    /// Protocol wide counters plus the per source accuracy summary.
    pub fn get_analytics(env: Env) -> Result<AggregatorAnalytics, AggregatorError> {
        Storage::get_config(&env)?;

        let mut stats = Storage::get_stats(&env);
        let registry = Storage::get_registry(&env);

        let mut total_weight: u64 = 0;
        let mut summaries: Vec<SourceSummary> = Vec::new(&env);
        for address in registry.iter() {
            let record = Storage::get_source(&env, &address)?;
            if record.active {
                total_weight += record.weight_bps as u64;
            }
            summaries.push_back(SourceSummary {
                source: record.address.clone(),
                weight_bps: record.weight_bps,
                active: record.active,
                predictions: record.predictions,
                resolved: record.resolved,
                hits: record.hits,
                misses: record.misses,
                accuracy_bps: Self::accuracy_bps(record.hits, record.resolved),
                bias_bps: record.bias_bps,
            });
        }

        stats.total_weight = total_weight as u32;
        stats.avg_confidence_bps = if stats.published_forecasts == 0 {
            0
        } else {
            (stats.confidence_sum / stats.published_forecasts as u64) as u32
        };
        let scored = stats.hits + stats.misses;
        stats.accuracy_bps = if scored == 0 {
            0
        } else {
            (stats.hits * MAX_BPS as u64 / scored) as u32
        };
        stats.sources = summaries;

        Ok(stats)
    }

    // ------------------------------------------------------------------ helpers

    /// Loads the config and demands the stored admin's authorisation.
    fn admin_only(env: &Env) -> Result<Config, AggregatorError> {
        let config = Storage::get_config(env)?;
        config.admin.require_auth();
        Ok(config)
    }

    /// Loads the config and demands the authorisation of `admin`, rejecting any
    /// address that is not the stored admin.
    fn require_admin(env: &Env, admin: &Address) -> Result<Config, AggregatorError> {
        let config = Storage::get_config(env)?;
        if admin != &config.admin {
            return Err(AggregatorError::Unauthorized);
        }
        config.admin.require_auth();
        Ok(config)
    }

    fn status(market: &Market) -> MarketStatus {
        if market.resolved {
            MarketStatus::Resolved
        } else if market.closed {
            MarketStatus::Closed
        } else {
            MarketStatus::Open
        }
    }

    /// Materialises the predictions of a market into a host vector so that the
    /// source records can be rewritten while iterating.
    fn predictions(env: &Env, market: &Market) -> Vec<Prediction> {
        let mut result: Vec<Prediction> = Vec::new(env);
        for (_, prediction) in market.predictions.iter() {
            result.push_back(prediction);
        }
        result
    }

    fn sum_active_weight(env: &Env, registry: &Vec<Address>) -> u32 {
        let mut total: u64 = 0;
        for address in registry.iter() {
            if let Ok(record) = Storage::get_source(env, &address) {
                if record.active {
                    total += record.weight_bps as u64;
                }
            }
        }
        total as u32
    }

    fn accuracy_bps(hits: u32, resolved: u32) -> u32 {
        if resolved == 0 {
            0
        } else {
            (hits as u64 * MAX_BPS as u64 / resolved as u64) as u32
        }
    }

    /// Clamps a signed bias to `+/- max_bps`.
    fn clamp_bias(bias: i128, max_bps: u32) -> i128 {
        let max = max_bps as i128;
        if bias > max {
            max
        } else if bias < -max {
            -max
        } else {
            bias
        }
    }

    fn clamp_bps(value: i128, min: i128, max: i128) -> i128 {
        if value < min {
            min
        } else if value > max {
            max
        } else {
            value
        }
    }

    /// Absolute difference of two unsigned quantities.
    fn deviation(a: u64, b: u64) -> u64 {
        if a > b {
            a - b
        } else {
            b - a
        }
    }

    /// The consensus algorithm.
    ///
    /// 1. weighted mean over every prediction, trimmed sources included;
    /// 2. a prediction deviating by more than `outlier_threshold_bps` from that
    ///    mean is an outlier and is dropped;
    /// 3. the weighted mean of the survivors is the published probability;
    /// 4. confidence is the weighted mean absolute deviation, turned into an
    ///    agreement score, scaled by the source count.
    ///
    /// Predictions carrying a zero weight cannot move a mean, so they are
    /// ignored entirely.
    fn compute(
        env: &Env,
        config: &Config,
        market: &Market,
    ) -> Result<ConsensusForecast, AggregatorError> {
        let predictions = Self::predictions(env, market);
        if predictions.len() < market.min_sources {
            return Err(AggregatorError::InsufficientSources);
        }

        // Step 1 - pre-trim weighted mean.
        let mut weight_sum: u64 = 0;
        let mut weighted_sum: u64 = 0;
        for prediction in predictions.iter() {
            let weight = prediction.weight_bps as u64;
            if weight == 0 {
                continue;
            }
            weight_sum += weight;
            weighted_sum += weight * prediction.probability_bps as u64;
        }
        if weight_sum == 0 {
            return Err(AggregatorError::InsufficientSources);
        }
        let pre_trim_mean = weighted_sum / weight_sum;

        // Step 2 - trim the outliers around the pre-trim mean.
        let threshold = config.outlier_threshold_bps as u64;
        let mut used: Vec<u64> = Vec::new(env);
        let mut used_weight: Vec<u64> = Vec::new(env);
        let mut excluded: Vec<Address> = Vec::new(env);

        for prediction in predictions.iter() {
            let weight = prediction.weight_bps as u64;
            if weight == 0 {
                continue;
            }
            let probability = prediction.probability_bps as u64;
            if Self::deviation(probability, pre_trim_mean) > threshold {
                excluded.push_back(prediction.source.clone());
            } else {
                used.push_back(probability);
                used_weight.push_back(weight);
            }
        }

        let used_count = used.len();
        if used_count < market.min_sources {
            return Err(AggregatorError::InsufficientSources);
        }

        // Step 3 - weighted mean of the survivors.
        let mut used_weight_sum: u64 = 0;
        let mut used_weighted_sum: u64 = 0;
        for i in 0..used_count {
            let weight = used_weight.get(i).unwrap();
            used_weight_sum += weight;
            used_weighted_sum += weight * used.get(i).unwrap();
        }
        let mean = used_weighted_sum / used_weight_sum;

        // Step 4 - agreement and count, i.e. the confidence.
        let mut deviation_sum: u64 = 0;
        for i in 0..used_count {
            let weight = used_weight.get(i).unwrap();
            deviation_sum += weight * Self::deviation(used.get(i).unwrap(), mean);
        }
        let mad = deviation_sum / used_weight_sum;
        let agreement = if mad >= MAX_BPS as u64 {
            0
        } else {
            MAX_BPS as u64 - mad
        };
        let denominator = 2 * market.min_sources as u64;
        let mut count = used_count as u64 * MAX_BPS as u64 / denominator;
        if count > MAX_BPS as u64 {
            count = MAX_BPS as u64;
        }
        let confidence = agreement * count / MAX_BPS as u64;

        Ok(ConsensusForecast {
            market_id: market.market_id.clone(),
            probability_bps: mean as u32,
            confidence_bps: confidence as u32,
            sources_used: used_count,
            total_sources: predictions.len(),
            weight_used: used_weight_sum as u32,
            excluded,
            timestamp: env.ledger().timestamp(),
            published: confidence >= config.min_confidence_bps as u64,
        })
    }
}

mod test;
