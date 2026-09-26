#![no_std]

//! Minimal expiry/tier core for issue #339 ("Puzzle Pass and Access NFT").
//! Covers the "passes valid until expiry" and "tiers provide benefits"
//! acceptance criteria; minting/transfer-restriction wiring is a follow-up.

use soroban_sdk::contracttype;

const GRACE_PERIOD_SECONDS: u64 = 24 * 60 * 60; // 24h renewal grace window

#[derive(Clone, Copy)]
#[contracttype]
pub enum PassTier {
    Basic,
    Premium,
    Vip,
}

/// True while the pass is inside its paid window (before expiry).
pub fn is_active(now: u64, expires_at: u64) -> bool {
    now < expires_at
}

/// True while a lapsed pass can still be renewed without losing access.
pub fn in_grace_period(now: u64, expires_at: u64) -> bool {
    now >= expires_at && now < expires_at + GRACE_PERIOD_SECONDS
}

/// Content-gating rule: a piece of content requires a minimum tier.
pub fn grants_access(pass_tier: PassTier, required_tier: PassTier) -> bool {
    (pass_tier as u32) >= (required_tier as u32)
}
