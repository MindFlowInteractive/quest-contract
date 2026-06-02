#![allow(dead_code)]

use soroban_sdk::{Address, Env};

pub fn quest_created(env: &Env, quest_id: u64, creator: Address) {
    env.events().publish(("quest_created", quest_id), creator);
}

pub fn quest_updated(env: &Env, quest_id: u64) {
    env.events().publish(("quest_updated", quest_id), ());
}

pub fn quest_completed(env: &Env, quest_id: u64, player: Address) {
    env.events().publish(("quest_completed", quest_id), player);
}

pub fn quest_cancelled(env: &Env, quest_id: u64) {
    env.events().publish(("quest_cancelled", quest_id), ());
}

pub fn contract_initialized(env: &Env, admin: Address) {
    env.events().publish(("initialized",), admin);
}

pub fn contract_paused(env: &Env, by: Address) {
    env.events().publish(("paused",), by);
}

pub fn contract_unpaused(env: &Env, by: Address) {
    env.events().publish(("unpaused",), by);
}
