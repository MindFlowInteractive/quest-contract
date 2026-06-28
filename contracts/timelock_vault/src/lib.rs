#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env};
use soroban_token_sdk::TokenClient;

#[cfg(test)]
mod test;

#[contracttype]
pub enum DataKey {
    Admin,
    Vault(Address, Address), // (beneficiary, token)
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultStatus {
    pub locked_amount: i128,
    pub unlock_time: u64,
    pub is_condition_met: bool,
}

#[contract]
pub struct TimeLockVault;

#[contractimpl]
impl TimeLockVault {
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    pub fn deposit(
        env: Env,
        from: Address,
        token: Address,
        amount: i128,
        beneficiary: Address,
        unlock_time: u64,
    ) {
        from.require_auth();

        if amount <= 0 {
            panic!("amount must be positive");
        }

        let token_client = TokenClient::new(&env, &token);
        token_client.transfer(&from, &env.current_contract_address(), &amount);

        let vault_key = DataKey::Vault(beneficiary.clone(), token.clone());
        let mut status = env
            .storage()
            .persistent()
            .get::<_, VaultStatus>(&vault_key)
            .unwrap_or(VaultStatus {
                locked_amount: 0,
                unlock_time: 0,
                is_condition_met: false,
            });

        status.locked_amount += amount;
        if unlock_time > status.unlock_time {
            status.unlock_time = unlock_time;
        }

        env.storage().persistent().set(&vault_key, &status);

        env.events().publish(
            (symbol_short!("deposit"), beneficiary.clone(), token.clone()),
            (amount, unlock_time),
        );
    }

    pub fn withdraw(env: Env, beneficiary: Address, token: Address) {
        beneficiary.require_auth();

        let vault_key = DataKey::Vault(beneficiary.clone(), token.clone());
        let mut status = env
            .storage()
            .persistent()
            .get::<_, VaultStatus>(&vault_key)
            .expect("vault not found");

        if status.locked_amount <= 0 {
            panic!("no locked amount");
        }

        if env.ledger().timestamp() < status.unlock_time && !status.is_condition_met {
            panic!("funds are locked");
        }

        let amount = status.locked_amount;
        status.locked_amount = 0;
        env.storage().persistent().set(&vault_key, &status);

        let token_client = TokenClient::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &beneficiary, &amount);

        env.events().publish(
            (symbol_short!("withdraw"), beneficiary.clone(), token.clone()),
            amount,
        );
    }

    pub fn set_condition(env: Env, beneficiary: Address, token: Address, met: bool) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("not initialized");
        admin.require_auth();

        let vault_key = DataKey::Vault(beneficiary.clone(), token.clone());
        let mut status = env
            .storage()
            .persistent()
            .get::<_, VaultStatus>(&vault_key)
            .expect("vault not found");

        status.is_condition_met = met;
        env.storage().persistent().set(&vault_key, &status);

        env.events().publish(
            (symbol_short!("condition"), beneficiary.clone(), token.clone()),
            met,
        );
    }

    pub fn emergency_unlock(env: Env, beneficiary: Address, token: Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("not initialized");
        admin.require_auth();

        let vault_key = DataKey::Vault(beneficiary.clone(), token.clone());
        let mut status = env
            .storage()
            .persistent()
            .get::<_, VaultStatus>(&vault_key)
            .expect("vault not found");

        status.is_condition_met = true;
        env.storage().persistent().set(&vault_key, &status);

        env.events().publish(
            (symbol_short!("emergency"), beneficiary.clone(), token.clone()),
            true,
        );
    }

    pub fn query_status(env: Env, beneficiary: Address, token: Address) -> VaultStatus {
        let vault_key = DataKey::Vault(beneficiary, token);
        env.storage().persistent().get(&vault_key).expect("vault not found")
    }
}
