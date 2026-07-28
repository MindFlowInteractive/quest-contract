#![no_std]
// Closes #306: collateral pool (starter: per-asset deposit tracking).
// Risk weighting, liquidation, and auctions are follow-ups.

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

#[contracttype]
pub enum DataKey {
    Balance(Address, Address), // (depositor, asset)
    TotalCollateral(Address),  // asset
}

#[contract]
pub struct CollateralPoolContract;

#[contractimpl]
impl CollateralPoolContract {
    /// Deposits `amount` of `asset` on behalf of `depositor`.
    pub fn deposit(env: Env, depositor: Address, asset: Address, amount: i128) {
        depositor.require_auth();

        let key = DataKey::Balance(depositor.clone(), asset.clone());
        let current: i128 = env.storage().instance().get(&key).unwrap_or(0);
        env.storage().instance().set(&key, &(current + amount));

        let total_key = DataKey::TotalCollateral(asset);
        let total: i128 = env.storage().instance().get(&total_key).unwrap_or(0);
        env.storage().instance().set(&total_key, &(total + amount));
    }

    /// Returns `depositor`'s deposited balance of `asset`.
    pub fn balance_of(env: Env, depositor: Address, asset: Address) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::Balance(depositor, asset))
            .unwrap_or(0)
    }

    /// Returns the pool's total deposited collateral for `asset`.
    pub fn total_collateral(env: Env, asset: Address) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalCollateral(asset))
            .unwrap_or(0)
    }
}
