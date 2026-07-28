#![no_std]
// Closes #300: range query data structure (starter: ordered u32-keyed storage
// with an inclusive range query). Filtering, statistics, and gas-optimized
// indexing are follow-ups.

use soroban_sdk::{contract, contractimpl, contracttype, Env, Vec};

#[contracttype]
pub enum DataKey {
    Item(u32),
}

#[contract]
pub struct RangeQueriesContract;

#[contractimpl]
impl RangeQueriesContract {
    /// Inserts or overwrites the value at `index`.
    pub fn insert(env: Env, index: u32, value: i128) {
        env.storage().instance().set(&DataKey::Item(index), &value);
    }

    /// Returns values for indices in `[start, end]`, skipping unset indices.
    pub fn range_query(env: Env, start: u32, end: u32) -> Vec<i128> {
        let mut results = Vec::new(&env);
        let mut i = start;
        while i <= end {
            if let Some(value) = env.storage().instance().get::<_, i128>(&DataKey::Item(i)) {
                results.push_back(value);
            }
            i += 1;
        }
        results
    }
}
