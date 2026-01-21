#![no_std]

use soroban_sdk::{contract, contractimpl, Address, Env};

#[contract]
pub struct TimeAttack;

#[contractimpl]
impl TimeAttack {
    pub fn initialize(_env: Env, _admin: Address) {
        // intentionally empty scaffold (no storage/logic yet)
    }
}
