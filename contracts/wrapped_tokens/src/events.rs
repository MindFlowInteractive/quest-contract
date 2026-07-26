#![no_std]
use soroban_sdk::{symbol_short, Address, Bytes, BytesN, Env};
use crate::types::ChainId;

pub fn emit_initialized(env: &Env, admin: &Address) {
    env.events().publish((symbol_short!("init"),), (admin,));
}

pub fn emit_wrap_requested(
    env: &Env,
    nonce: u64,
    operator: &Address,
    recipient: &Address,
    gross_amount: i128,
    fee_amount: i128,
    source_chain: ChainId,
    source_tx_id: &BytesN<32>,
) {
    env.events().publish(
        (symbol_short!("wrap_req"),),
        (
            nonce,
            operator,
            recipient,
            gross_amount,
            fee_amount,
            source_chain as u32,
            source_tx_id,
        ),
    );
}

pub fn emit_wrap_confirmed(env: &Env, nonce: u64, operator: &Address, confirmations: u32) {
    env.events().publish(
        (symbol_short!("wrap_conf"),),
        (nonce, operator, confirmations),
    );
}

pub fn emit_tokens_minted(env: &Env, nonce: u64, recipient: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("minted"),),
        (nonce, recipient, amount),
    );
}

pub fn emit_unwrap_initiated(
    env: &Env,
    nonce: u64,
    user: &Address,
    gross_amount: i128,
    fee_amount: i128,
    target_chain: ChainId,
    target_recipient: &Bytes,
) {
    env.events().publish(
        (symbol_short!("unwrap"),),
        (
            nonce,
            user,
            gross_amount,
            fee_amount,
            target_chain as u32,
            target_recipient,
        ),
    );
}

pub fn emit_tokens_burned(env: &Env, nonce: u64, user: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("burned"),),
        (nonce, user, amount),
    );
}

pub fn emit_unwrap_completed(env: &Env, nonce: u64, operator: &Address) {
    env.events().publish(
        (symbol_short!("unwrap_ok"),),
        (nonce, operator),
    );
}

pub fn emit_fee_collected(env: &Env, recipient: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("fee"),),
        (recipient, amount),
    );
}

pub fn emit_operator_added(env: &Env, operator: &Address, added_by: &Address) {
    env.events().publish(
        (symbol_short!("op_add"),),
        (operator, added_by),
    );
}

pub fn emit_operator_removed(env: &Env, operator: &Address, removed_by: &Address) {
    env.events().publish(
        (symbol_short!("op_rm"),),
        (operator, removed_by),
    );
}

pub fn emit_paused(env: &Env, by: &Address) {
    env.events().publish((symbol_short!("paused"),), (by,));
}

pub fn emit_unpaused(env: &Env, by: &Address) {
    env.events().publish((symbol_short!("unpaused"),), (by,));
}

pub fn emit_transfer(env: &Env, from: &Address, to: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("transfer"),),
        (from, to, amount),
    );
}

pub fn emit_approval(env: &Env, owner: &Address, spender: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("approval"),),
        (owner, spender, amount),
    );
}
