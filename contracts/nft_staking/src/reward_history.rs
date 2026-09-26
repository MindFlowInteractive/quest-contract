//! Reward claim history for issue #337 ("Add reward history"). Records each
//! claim so a staker can look back at what they've been paid, independent of
//! the existing pending/claim accounting in `lib.rs`.

use soroban_sdk::{contracttype, symbol_short, Address, Env, Vec};

#[derive(Clone)]
#[contracttype]
pub struct RewardClaimEntry {
    pub token_id: u32,
    pub amount: i128,
    pub ledger: u32,
}

fn history_key(env: &Env, staker: &Address) -> (soroban_sdk::Symbol, Address) {
    (symbol_short!("rw_hist"), staker.clone())
}

pub fn record_claim(env: &Env, staker: &Address, token_id: u32, amount: i128) {
    let key = history_key(env, staker);
    let mut log: Vec<RewardClaimEntry> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));
    log.push_back(RewardClaimEntry {
        token_id,
        amount,
        ledger: env.ledger().sequence(),
    });
    env.storage().persistent().set(&key, &log);
}

pub fn get_history(env: &Env, staker: &Address) -> Vec<RewardClaimEntry> {
    env.storage()
        .persistent()
        .get(&history_key(env, staker))
        .unwrap_or(Vec::new(env))
}
