#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, token, Address, Env,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum FaucetError {
    NotAuthorized = 1,
    AlreadyInitialized = 2,
    NotInitialized = 3,
    InvalidAmount = 4,
    ExceedsMaxAmount = 5,
    CooldownActive = 6,
    InsufficientFaucetBalance = 7,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    Token,
    CooldownLedgers,
    MaxAmount,
    LastRequestLedger(Address),
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FaucetClaimedEvent {
    pub recipient: Address,
    pub amount: i128,
    pub ledger: u32,
}
#[contract]
pub struct TestnetFaucetContract;

#[contractimpl]
impl TestnetFaucetContract {
    /// Initializes the testnet faucet with admin, token contract, cooldown in ledgers, and per-request cap.
    pub fn initialize(
        env: Env,
        admin: Address,
        token: Address,
        cooldown_ledgers: u32,
        max_amount: i128,
    ) -> Result<(), FaucetError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(FaucetError::AlreadyInitialized);
        }
        if max_amount <= 0 {
            return Err(FaucetError::InvalidAmount);
        }

        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage()
            .instance()
            .set(&DataKey::CooldownLedgers, &cooldown_ledgers);
        env.storage().instance().set(&DataKey::MaxAmount, &max_amount);
        Ok(())
    }

    /// Updates cooldown period (in ledgers) and maximum tokens per request.
    pub fn set_config(
        env: Env,
        caller: Address,
        cooldown_ledgers: u32,
        max_amount: i128,
    ) -> Result<(), FaucetError> {
        caller.require_auth();
        Self::require_admin(&env, &caller)?;

        if max_amount <= 0 {
            return Err(FaucetError::InvalidAmount);
        }

        env.storage()
            .instance()
            .set(&DataKey::CooldownLedgers, &cooldown_ledgers);
        env.storage().instance().set(&DataKey::MaxAmount, &max_amount);
        Ok(())
    }

    /// Requests up to `max_amount` of test tokens for `recipient` once per cooldown window.
    pub fn request(env: Env, recipient: Address, amount: i128) -> Result<i128, FaucetError> {
        recipient.require_auth();

        if amount <= 0 {
            return Err(FaucetError::InvalidAmount);
        }

        let max_amount: i128 = env
            .storage()
            .instance()
            .get(&DataKey::MaxAmount)
            .ok_or(FaucetError::NotInitialized)?;

        if amount > max_amount {
            return Err(FaucetError::ExceedsMaxAmount);
        }

        let cooldown_ledgers: u32 = env
            .storage()
            .instance()
            .get(&DataKey::CooldownLedgers)
            .ok_or(FaucetError::NotInitialized)?;

        let current_ledger = env.ledger().sequence();
        let last_key = DataKey::LastRequestLedger(recipient.clone());

        if let Some(last_ledger) = env.storage().persistent().get::<DataKey, u32>(&last_key) {
            let eligible_ledger = last_ledger.saturating_add(cooldown_ledgers);
            if current_ledger < eligible_ledger {
                return Err(FaucetError::CooldownActive);
            }
        }

        let token_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(FaucetError::NotInitialized)?;

        let token_client = token::Client::new(&env, &token_addr);
        let contract_addr = env.current_contract_address();

        if token_client.balance(&contract_addr) < amount {
            return Err(FaucetError::InsufficientFaucetBalance);
        }

        token_client.transfer(&contract_addr, &recipient, &amount);
        env.storage().persistent().set(&last_key, &current_ledger);

        FaucetClaimedEvent {
            recipient,
            amount,
            ledger: current_ledger,
        }
        .publish(&env);

        Ok(amount)
    }

    /// Returns the ledger sequence of `recipient`'s last successful faucet claim, if any.
    pub fn last_request_ledger(env: Env, recipient: Address) -> Option<u32> {
        env.storage()
            .persistent()
            .get(&DataKey::LastRequestLedger(recipient))
    }

    /// Returns the ledger sequence when `recipient` can next request tokens (0 if immediately eligible).
    pub fn next_eligible_ledger(env: Env, recipient: Address) -> u32 {
        let cooldown_ledgers: u32 = env
            .storage()
            .instance()
            .get(&DataKey::CooldownLedgers)
            .unwrap_or(0);

        match Self::last_request_ledger(env, recipient) {
            Some(last_ledger) => last_ledger.saturating_add(cooldown_ledgers),
            None => 0,
        }
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), FaucetError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(FaucetError::NotInitialized)?;
        if caller != &admin {
            return Err(FaucetError::NotAuthorized);
        }
        Ok(())
    }
}