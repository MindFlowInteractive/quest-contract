#![no_std]

//! Minimal rarity scoring core for issue #335: given a fixed set of trait
//! weights, compute a token's rarity score and tier. Ranking/leaderboard
//! wiring is left for a follow-up once the contract shell exists.

use soroban_sdk::{contracttype, Env, Vec};

#[derive(Clone)]
#[contracttype]
pub enum RarityTier {
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
}

/// Score = sum(trait_weight) / trait_count, scaled to basis points (0-10000).
pub fn score_traits(_env: &Env, trait_weights: &Vec<u32>) -> u32 {
    if trait_weights.is_empty() {
        return 0;
    }
    let mut total: u64 = 0;
    for w in trait_weights.iter() {
        total += w as u64;
    }
    let avg = total / trait_weights.len() as u64;
    avg.min(10_000) as u32
}

pub fn tier_for_score(score: u32) -> RarityTier {
    match score {
        0..=2000 => RarityTier::Common,
        2001..=4000 => RarityTier::Uncommon,
        4001..=6500 => RarityTier::Rare,
        6501..=8500 => RarityTier::Epic,
        _ => RarityTier::Legendary,
    }
}
