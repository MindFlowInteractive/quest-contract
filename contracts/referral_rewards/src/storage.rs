use crate::types::{Analytics, Config, PayoutRecord, ReferralError, UserStats, MAX_REFERRALS};
use soroban_sdk::{symbol_short, Address, Env, Symbol, Vec};

/// Thin typed wrapper around the contract storage.
///
/// * `instance()` holds the config, the code counter and the protocol wide
///   counters.
/// * `persistent()` holds the bidirectional code index, the referrer links, the
///   per user counters, the bounded referral list and the bounded payout
///   history. Every key is namespaced by a `symbol_short!` prefix so the
///   namespaces can never collide.
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

    pub fn get_config(env: &Env) -> Result<Config, ReferralError> {
        env.storage()
            .instance()
            .get(&symbol_short!("config"))
            .ok_or(ReferralError::NotInitialized)
    }

    // ---------------------------------------------------------------- counters

    pub fn get_stats(env: &Env) -> Analytics {
        env.storage()
            .instance()
            .get(&symbol_short!("stats"))
            .unwrap_or_else(Analytics::empty)
    }

    pub fn set_stats(env: &Env, stats: &Analytics) {
        env.storage().instance().set(&symbol_short!("stats"), stats);
    }

    pub fn get_code_counter(env: &Env) -> u32 {
        env.storage()
            .instance()
            .get(&symbol_short!("ccnt"))
            .unwrap_or(0)
    }

    pub fn set_code_counter(env: &Env, counter: u32) {
        env.storage().instance().set(&symbol_short!("ccnt"), &counter);
    }

    // ------------------------------------------------------------------- codes

    fn code_key(user: &Address) -> (Symbol, Address) {
        (symbol_short!("code"), user.clone())
    }

    fn owner_key(code: &Symbol) -> (Symbol, Symbol) {
        (symbol_short!("owner"), code.clone())
    }

    pub fn has_code(env: &Env, user: &Address) -> bool {
        env.storage().persistent().has(&Self::code_key(user))
    }

    pub fn set_code(env: &Env, user: &Address, code: &Symbol) {
        env.storage().persistent().set(&Self::code_key(user), code);
    }

    pub fn get_code(env: &Env, user: &Address) -> Result<Symbol, ReferralError> {
        env.storage()
            .persistent()
            .get(&Self::code_key(user))
            .ok_or(ReferralError::CodeNotFound)
    }

    pub fn has_code_owner(env: &Env, code: &Symbol) -> bool {
        env.storage().persistent().has(&Self::owner_key(code))
    }

    pub fn set_code_owner(env: &Env, code: &Symbol, user: &Address) {
        env.storage()
            .persistent()
            .set(&Self::owner_key(code), user);
    }

    pub fn get_code_owner(env: &Env, code: &Symbol) -> Result<Address, ReferralError> {
        env.storage()
            .persistent()
            .get(&Self::owner_key(code))
            .ok_or(ReferralError::CodeNotFound)
    }

    /// Code owners, in the order their codes were minted.
    pub fn get_registry(env: &Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&symbol_short!("usrs"))
            .unwrap_or_else(|| Vec::new(env))
    }

    pub fn add_to_registry(env: &Env, user: &Address) {
        let mut registry = Self::get_registry(env);
        registry.push_back(user.clone());
        env.storage().persistent().set(&symbol_short!("usrs"), &registry);
    }

    // --------------------------------------------------------------- referrers

    fn referrer_key(user: &Address) -> (Symbol, Address) {
        (symbol_short!("referr"), user.clone())
    }

    pub fn has_referrer(env: &Env, user: &Address) -> bool {
        env.storage().persistent().has(&Self::referrer_key(user))
    }

    pub fn set_referrer(env: &Env, user: &Address, referrer: &Address) {
        env.storage()
            .persistent()
            .set(&Self::referrer_key(user), referrer);
    }

    pub fn get_referrer(env: &Env, user: &Address) -> Option<Address> {
        env.storage().persistent().get(&Self::referrer_key(user))
    }

    fn referrals_key(user: &Address) -> (Symbol, Address) {
        (symbol_short!("refs"), user.clone())
    }

    /// Appends `user` to the retained direct referral list of `referrer`.
    ///
    /// The list is capped at `MAX_REFERRALS` entries (oldest entries are
    /// dropped first) to keep the per referrer entry bounded.
    pub fn add_referral(env: &Env, referrer: &Address, user: &Address) {
        let key = Self::referrals_key(referrer);
        let mut list: Vec<Address> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));

        list.push_back(user.clone());
        while list.len() > MAX_REFERRALS {
            let _ = list.remove(0);
        }

        env.storage().persistent().set(&key, &list);
    }

    pub fn get_referrals(env: &Env, referrer: &Address) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&Self::referrals_key(referrer))
            .unwrap_or_else(|| Vec::new(env))
    }

    // ----------------------------------------------------------- user counters

    fn user_key(user: &Address) -> (Symbol, Address) {
        (symbol_short!("ustat"), user.clone())
    }

    pub fn set_user(env: &Env, user: &Address, stats: &UserStats) {
        env.storage().persistent().set(&Self::user_key(user), stats);
    }

    /// Counters of `user`, zeroed when the user is still untouched.
    pub fn get_user(env: &Env, user: &Address) -> UserStats {
        env.storage()
            .persistent()
            .get(&Self::user_key(user))
            .unwrap_or_else(|| UserStats::new(user))
    }

    // ----------------------------------------------------------------- history

    fn history_key(user: &Address) -> (Symbol, Address) {
        (symbol_short!("hist"), user.clone())
    }

    /// Appends a payout to the history of `user`, keeping only the most recent
    /// `max` entries.
    pub fn add_history(env: &Env, user: &Address, record: &PayoutRecord, max: u32) {
        let key = Self::history_key(user);
        let mut history: Vec<PayoutRecord> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));

        history.push_back(record.clone());
        while history.len() > max {
            let _ = history.remove(0);
        }

        env.storage().persistent().set(&key, &history);
    }

    /// Returns at most `limit` of the most recent payouts of `user`, oldest
    /// first.
    pub fn get_history(env: &Env, user: &Address, limit: u32) -> Vec<PayoutRecord> {
        let history: Vec<PayoutRecord> = env
            .storage()
            .persistent()
            .get(&Self::history_key(user))
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
