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
    /// Each award is recorded in the player's history with its reason code
    /// and an expiry; `expires_at` of `0` means the points never expire.
    pub fn earn(
        env: Env,
        awarder: Address,
        player: Address,
        amount: u64,
        reason: u32,
        expires_at: u64,
    ) -> u64 {
        awarder.require_auth();
        if !Self::is_awarder(env.clone(), awarder.clone()) {
            panic!("unauthorized awarder");
        }
        if amount == 0 {
            panic!("amount must be greater than zero");
        }
        if expires_at != 0 && expires_at <= env.ledger().timestamp() {
            panic!("expires_at must be in the future");
        }

        let mut history = Self::history(env.clone(), player.clone());
        history.push_back(PointsEntry {
            amount,
            reason,
            awarder: awarder.clone(),
            timestamp: env.ledger().timestamp(),
            expires_at,
        });

        env.storage()
            .persistent()
            .set(&DataKey::History(player.clone()), &history);
        let balance = Self::balance(env.clone(), player.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Balance(player.clone()), &balance);

        env.events()
            .publish((symbol_short!("earned"), player), (awarder, amount, reason));

        balance
    }

    /// Spend points from a player's balance. Only the player may call this.
    ///
    /// Consumption is FIFO over the player's history: the oldest unexpired
    /// entries are consumed first, and an entry that covers the remainder of
    /// the spend is reduced rather than removed. Points that already expired
    /// are excluded from the balance and can never be spent.
    pub fn spend(env: Env, player: Address, amount: u64) -> u64 {
        player.require_auth();
        if amount == 0 {
            panic!("amount must be greater than zero");
        }

        let mut history = Self::history(env.clone(), player.clone());
        let mut to_spend = amount;
        while to_spend > 0 {
            let idx = (0..history.len())
                .find(|&i| {
                    !Self::is_expired_at(&env, &history.get(i).unwrap(), env.ledger().timestamp())
                })
                .expect("insufficient unexpired balance");
            let entry = history.get(idx).unwrap();
            if entry.amount > to_spend {
                let mut entry = entry;
                entry.amount -= to_spend;
                history.set(idx, entry);
                to_spend = 0;
            } else {
                to_spend -= entry.amount;
                history.remove(idx);
            }
        }

        let balance = Self::unexpired_sum(&env, &history);
        env.storage()
            .persistent()
            .set(&DataKey::History(player.clone()), &history);
        env.storage()
            .persistent()
            .set(&DataKey::Balance(player.clone()), &balance);

        env.events()
            .publish((symbol_short!("spent"), player), amount);

        balance
    }

    fn is_expired_at(env: &Env, entry: &PointsEntry, now: u64) -> bool {
        entry.expires_at != 0 && now >= entry.expires_at
    }

    /// Sum of the points in the history that have not yet expired.
    fn unexpired_sum(env: &Env, history: &Vec<PointsEntry>) -> u64 {
        let now = env.ledger().timestamp();
        history
            .iter()
            .filter(|entry| !Self::is_expired_at(env, entry, now))
            .map(|entry| entry.amount)
            .sum()
    }

    pub fn balance(env: Env, player: Address) -> u64 {
        let history = Self::history(env.clone(), player);
        Self::unexpired_sum(&env, &history)
    }

    pub fn history(env: Env, player: Address) -> Vec<PointsEntry> {
        env.storage()
            .persistent()
            .get(&DataKey::History(player))
            .unwrap_or(Vec::new(&env))
    }
}
