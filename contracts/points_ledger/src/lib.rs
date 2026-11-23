#![no_std]

mod tests;
mod types;

use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Vec};
pub use types::{DataKey, PointsEntry};

fn read_admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .expect("not initialized")
}

#[contract]
pub struct PointsLedgerContract;

#[contractimpl]
impl PointsLedgerContract {
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    /// Admin grants a contract permission to award points.
    pub fn add_awarder(env: Env, awarder: Address) {
        read_admin(&env).require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::Awarder(awarder), &true);
    }

    /// Admin revokes a contract's permission to award points.
    pub fn remove_awarder(env: Env, awarder: Address) {
        read_admin(&env).require_auth();
        env.storage()
            .persistent()
            .remove(&DataKey::Awarder(awarder));
    }

    pub fn is_awarder(env: Env, awarder: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Awarder(awarder))
            .unwrap_or(false)
    }

    /// Award points to a player. Only authorized awarders may call this.
    /// Each award is recorded in the player's history with its reason code.
    pub fn earn(env: Env, awarder: Address, player: Address, amount: u64, reason: u32) -> u64 {
        awarder.require_auth();
        if !Self::is_awarder(env.clone(), awarder.clone()) {
            panic!("unauthorized awarder");
        }
        if amount == 0 {
            panic!("amount must be greater than zero");
        }

        let balance = Self::balance(env.clone(), player.clone())
            .checked_add(amount)
            .expect("balance overflow");

        let mut history = Self::history(env.clone(), player.clone());
        history.push_back(PointsEntry {
            amount,
            reason,
            awarder: awarder.clone(),
            timestamp: env.ledger().timestamp(),
        });

        env.storage()
            .persistent()
            .set(&DataKey::Balance(player.clone()), &balance);
        env.storage()
            .persistent()
            .set(&DataKey::History(player.clone()), &history);

        env.events()
            .publish((symbol_short!("earned"), player), (awarder, amount, reason));

        balance
    }

    pub fn balance(env: Env, player: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(player))
            .unwrap_or(0)
    }

    pub fn history(env: Env, player: Address) -> Vec<PointsEntry> {
        env.storage()
            .persistent()
            .get(&DataKey::History(player))
            .unwrap_or(Vec::new(&env))
    }
}
