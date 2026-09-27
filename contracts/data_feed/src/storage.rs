use crate::types::{Analytics, Config, ConsensusRecord, DataFeedError, Feed, Provider};
use soroban_sdk::{symbol_short, Address, Env, Symbol, Vec};

/// Thin typed wrapper around the contract storage.
///
/// * `instance()` holds the small, hot pieces of protocol state: the config, the
///   aggregate analytics counters and the number of registered feeds.
/// * `persistent()` holds per-feed and per-provider data plus the provider
///   registry and the bounded per-feed history.
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

    pub fn get_config(env: &Env) -> Result<Config, DataFeedError> {
        env.storage()
            .instance()
            .get(&symbol_short!("config"))
            .ok_or(DataFeedError::NotInitialized)
    }

    // ---------------------------------------------------------------- analytics

    pub fn set_stats(env: &Env, stats: &Analytics) {
        env.storage()
            .instance()
            .set(&symbol_short!("stats"), stats);
    }

    pub fn get_stats(env: &Env) -> Analytics {
        env.storage()
            .instance()
            .get(&symbol_short!("stats"))
            .unwrap_or_else(Analytics::empty)
    }

    // ---------------------------------------------------------------- providers

    pub fn has_provider(env: &Env, provider: &Address) -> bool {
        env.storage().persistent().has(&Self::provider_key(provider))
    }

    pub fn set_provider(env: &Env, provider: &Address, data: &Provider) {
        env.storage()
            .persistent()
            .set(&Self::provider_key(provider), data);
    }

    pub fn get_provider(env: &Env, provider: &Address) -> Result<Provider, DataFeedError> {
        env.storage()
            .persistent()
            .get(&Self::provider_key(provider))
            .ok_or(DataFeedError::ProviderNotFound)
    }

    fn provider_key(provider: &Address) -> (Symbol, Address) {
        (symbol_short!("prov"), provider.clone())
    }

    pub fn get_registry(env: &Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&symbol_short!("provs"))
            .unwrap_or_else(|| Vec::new(env))
    }

    pub fn set_registry(env: &Env, registry: &Vec<Address>) {
        env.storage()
            .persistent()
            .set(&symbol_short!("provs"), registry);
    }

    // -------------------------------------------------------------------- feeds

    pub fn has_feed(env: &Env, feed_id: &Symbol) -> bool {
        env.storage().persistent().has(&Self::feed_key(feed_id))
    }

    pub fn set_feed(env: &Env, feed_id: &Symbol, feed: &Feed) {
        env.storage().persistent().set(&Self::feed_key(feed_id), feed);
    }

    pub fn get_feed(env: &Env, feed_id: &Symbol) -> Result<Feed, DataFeedError> {
        env.storage()
            .persistent()
            .get(&Self::feed_key(feed_id))
            .ok_or(DataFeedError::FeedNotFound)
    }

    fn feed_key(feed_id: &Symbol) -> (Symbol, Symbol) {
        (symbol_short!("feed"), feed_id.clone())
    }

    pub fn feed_count(env: &Env) -> u32 {
        env.storage()
            .instance()
            .get(&symbol_short!("feedcnt"))
            .unwrap_or(0)
    }

    pub fn set_feed_count(env: &Env, count: u32) {
        env.storage().instance().set(&symbol_short!("feedcnt"), &count);
    }

    // ------------------------------------------------------------------ history

    fn history_key(feed_id: &Symbol) -> (Symbol, Symbol) {
        (symbol_short!("history"), feed_id.clone())
    }

    /// Appends a finalized round to the feed history, keeping only the most
    /// recent `max` entries.
    pub fn add_history(env: &Env, feed_id: &Symbol, record: &ConsensusRecord, max: u32) {
        let key = Self::history_key(feed_id);
        let mut history: Vec<ConsensusRecord> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));

        history.push_back(record.clone());

        while history.len() > max {
            history.remove(0);
        }

        env.storage().persistent().set(&key, &history);
    }

    /// Returns at most `limit` of the most recent history entries, oldest first.
    pub fn get_history(env: &Env, feed_id: &Symbol, limit: u32) -> Vec<ConsensusRecord> {
        let key = Self::history_key(feed_id);
        let history: Vec<ConsensusRecord> = env
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
