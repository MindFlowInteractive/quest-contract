#![no_std]

//! Minimal fee + validator-threshold core for issue #338 ("Cross-Chain
//! Bridge and Teleport"). Lock/mint/burn wiring is a larger follow-up; this
//! covers the "fee collection" and "validators coordinate" acceptance
//! criteria in isolation.

const BPS_DENOMINATOR: i128 = 10_000;

/// Fee charged on a bridged amount, in basis points (e.g. 30 = 0.30%).
pub fn calculate_bridge_fee(amount: i128, fee_bps: i128) -> i128 {
    if amount <= 0 || fee_bps <= 0 {
        return 0;
    }
    (amount * fee_bps) / BPS_DENOMINATOR
}

/// Net amount the destination chain should mint after the fee is taken.
pub fn amount_after_fee(amount: i128, fee_bps: i128) -> i128 {
    amount - calculate_bridge_fee(amount, fee_bps)
}

/// True once enough validators (out of `total`) have signed off, using a
/// simple majority threshold (>50%).
pub fn has_quorum(signed_count: u32, total_validators: u32) -> bool {
    if total_validators == 0 {
        return false;
    }
    (signed_count as u64) * 2 > total_validators as u64
}
