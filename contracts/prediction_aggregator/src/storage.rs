use crate::types::{AggregatorAnalytics, AggregatorError, Config, ConsensusForecast, Market, Source};
use soroban_sdk::{symbol_short, Address, Env, Symbol, Vec};

/// Thin typed wrapper around the contract storage.
///
/// * `instance()` holds the small, hot pieces of protocol state: the config, the
///   aggregate analytics counters and the number of created markets.
/// * `persistent()` holds the source registry, the per-source track records,
///   the markets with their predictions and the bounded per-market history.
pub struct Storage;

impl Storage {
    // ------------------------------------------------------------------ config

    pub fn has_config(env: &Env) -> bool {
        env.storage().instance().has(&symbol_short!("config"))
    }

    pub fn set_config(env: &Env, config: &Config) {
        env.storage()
            .instance()
            .set(&symbol_short!("config"), config);
    }

    pub fn get_config(env: &Env) -> Result<Config, AggregatorError> {
        env.storage()
            .instance()
            .get(&symbol_short!("config"))
            .ok_or(AggregatorError::NotInitialized)
    }

    // --------------------------------------------------------------- analytics

    pub fn set_stats(env: &Env, stats: &AggregatorAnalytics) {
        env.storage().instance().set(&symbol_short!("stats"), stats);
    }

    pub fn get_stats(env: &Env) -> AggregatorAnalytics {
        env.storage()
            .instance()
            .get(&symbol_short!("stats"))
            .unwrap_or_else(|| AggregatorAnalytics::empty(env))
    }

    // ----------------------------------------------------------------- sources

    fn source_key(source: &Address) -> (Symbol, Address) {
        (symbol_short!("src"), source.clone())
    }

    pub fn has_source(env: &Env, source: &Address) -> bool {
        env.storage().persistent().has(&Self::source_key(source))
    }

    pub fn set_source(env: &Env, source: &Address, data: &Source) {
        env.storage()
            .persistent()
            .set(&Self::source_key(source), data);
    }

    pub fn get_source(env: &Env, source: &Address) -> Result<Source, AggregatorError> {
        env.storage()
            .persistent()
            .get(&Self::source_key(source))
            .ok_or(AggregatorError::SourceNotFound)
    }

    /// Every source that ever joined the registry, including deregistered ones.
    pub fn get_registry(env: &Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&symbol_short!("srcs"))
            .unwrap_or_else(|| Vec::new(env))
    }

    pub fn set_registry(env: &Env, registry: &Vec<Address>) {
        env.storage()
            .persistent()
            .set(&symbol_short!("srcs"), registry);
    }

    // ----------------------------------------------------------------- markets

    fn market_key(market_id: &Symbol) -> (Symbol, Symbol) {
        (symbol_short!("mkt"), market_id.clone())
    }

    pub fn has_market(env: &Env, market_id: &Symbol) -> bool {
        env.storage().persistent().has(&Self::market_key(market_id))
    }

    pub fn set_market(env: &Env, market_id: &Symbol, market: &Market) {
        env.storage()
            .persistent()
            .set(&Self::market_key(market_id), market);
    }

    pub fn get_market(env: &Env, market_id: &Symbol) -> Result<Market, AggregatorError> {
        env.storage()
            .persistent()
            .get(&Self::market_key(market_id))
            .ok_or(AggregatorError::MarketNotFound)
    }

    pub fn market_count(env: &Env) -> u32 {
        env.storage()
            .instance()
            .get(&symbol_short!("mktcnt"))
            .unwrap_or(0)
    }

    pub fn set_market_count(env: &Env, count: u32) {
        env.storage().instance().set(&symbol_short!("mktcnt"), &count);
    }

    // ----------------------------------------------------------------- history

    fn history_key(market_id: &Symbol) -> (Symbol, Symbol) {
        (symbol_short!("hist"), market_id.clone())
    }

    /// Appends a published forecast to the market history, keeping only the most
    /// recent `max` entries.
    pub fn add_forecast(
        env: &Env,
        market_id: &Symbol,
        forecast: &ConsensusForecast,
        max: u32,
    ) {
        let key = Self::history_key(market_id);
        let mut history: Vec<ConsensusForecast> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));

        history.push_back(forecast.clone());

        while history.len() > max {
            history.remove(0);
        }

        env.storage().persistent().set(&key, &history);
    }

    /// Returns at most `limit` of the most recent published forecasts, oldest
    /// first.
    pub fn get_history(
        env: &Env,
        market_id: &Symbol,
        limit: u32,
    ) -> Vec<ConsensusForecast> {
        let key = Self::history_key(market_id);
        let history: Vec<ConsensusForecast> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));

        let len = history.len();
        if limit == 0 || len == 0 {
            return Vec::new(env);
        }

        let start = if len > limit { len - limit } else { 0 };

        let mut result = Vec::new(env);
        for i in start..len {
            result.push_back(history.get(i).unwrap());
        }

        result
    }
}
