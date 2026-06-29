use soroban_sdk::{Env, Address, symbol_short};

pub(crate) fn emit_pool_created(
    env: &Env,
    token0: Address,
    token1: Address,
    fee_bps: u32,
) {
    env.events().publish(
        (symbol_short!("pool_created"), token0, token1),
        fee_bps,
    );
}

pub(crate) fn emit_liquidity_added(
    env: &Env,
    provider: Address,
    to: Address,
    amount_a: i128,
    amount_b: i128,
    liquidity: i128,
) {
    env.events().publish(
        (symbol_short!("mint"), provider),
        (to, amount_a, amount_b, liquidity),
    );
}

pub(crate) fn emit_liquidity_removed(
    env: &Env,
    owner: Address,
    to: Address,
    amount_a: i128,
    amount_b: i128,
    liquidity: i128,
) {
    env.events().publish(
        (symbol_short!("burn"), owner),
        (to, amount_a, amount_b, liquidity),
    );
}

pub(crate) fn emit_swap(
    env: &Env,
    swapper: Address,
    to: Address,
    token_in: Address,
    amount_in: i128,
    token_out: Address,
    amount_out: i128,
    fee_amount: i128,
) {
    env.events().publish(
        (symbol_short!("swap"), swapper),
        (to, token_in, amount_in, token_out, amount_out, fee_amount),
    );
}

pub(crate) fn emit_fees_collected(
    env: &Env,
    to: Address,
    fees_a: i128,
    fees_b: i128,
) {
    env.events().publish(
        (symbol_short!("collect"), to),
        (fees_a, fees_b),
    );
}