#![no_std]
// Closes #302: wrapped token bridge (starter: mint/burn + supply tracking).
// Custody proofs, bridge security, fees, and emergency pause are follow-ups.

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

#[contracttype]
pub enum DataKey {
    Admin,
    Supply,
    Balance(Address),
}

#[contract]
pub struct WrappedTokenContract;

#[contractimpl]
impl WrappedTokenContract {
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Supply, &0i128);
    }

    /// Mints `amount` wrapped tokens to `to`; caller must be the admin (bridge relay).
    pub fn mint(env: Env, to: Address, amount: i128) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("not initialized");
        admin.require_auth();

        let balance: i128 = env.storage().instance().get(&DataKey::Balance(to.clone())).unwrap_or(0);
        env.storage().instance().set(&DataKey::Balance(to), &(balance + amount));
        let supply: i128 = env.storage().instance().get(&DataKey::Supply).unwrap_or(0);
        env.storage().instance().set(&DataKey::Supply, &(supply + amount));
    }

    /// Burns `amount` from `from`'s balance to unwrap back to the origin chain.
    pub fn burn(env: Env, from: Address, amount: i128) {
        from.require_auth();
        let balance: i128 = env.storage().instance().get(&DataKey::Balance(from.clone())).unwrap_or(0);
        if balance < amount {
            panic!("insufficient balance");
        }
        env.storage().instance().set(&DataKey::Balance(from), &(balance - amount));
        let supply: i128 = env.storage().instance().get(&DataKey::Supply).unwrap_or(0);
        env.storage().instance().set(&DataKey::Supply, &(supply - amount));
    }

    pub fn total_supply(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::Supply).unwrap_or(0)
    }
}
