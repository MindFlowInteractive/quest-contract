#![no_std]
use soroban_sdk::{symbol_short, Address, Env};
use crate::types::DistributionKind;

pub fn emit_initialized(env: &Env, admin: &Address) {
    env.events().publish((symbol_short!("init"),), (admin,));
}

pub fn emit_dist_created(
    env: &Env,
    id: u32,
    kind: DistributionKind,
    token: &Address,
    total: i128,
    expiry: u64,
    creator: &Address,
) {
    env.events().publish(
        (symbol_short!("dist_new"),),
        (id, kind as u32, token, total, expiry, creator),
    );
}

pub fn emit_claimed(env: &Env, dist_id: u32, claimer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("claimed"),),
        (dist_id, claimer, amount),
    );
}

pub fn emit_batch_created(env: &Env, ids: &soroban_sdk::Vec<u32>) {
    env.events().publish((symbol_short!("batch_new"),), (ids,));
}

pub fn emit_expired(env: &Env, dist_id: u32) {
    env.events().publish((symbol_short!("expired"),), (dist_id,));
}

pub fn emit_recovered(env: &Env, dist_id: u32, recipient: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("recovered"),),
        (dist_id, recipient, amount),
    );
}

pub fn emit_cancelled(env: &Env, dist_id: u32, recovered: i128) {
    env.events().publish(
        (symbol_short!("cancelled"),),
        (dist_id, recovered),
    );
}
