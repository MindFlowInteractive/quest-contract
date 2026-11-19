#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, Symbol,
};

pub const DAY_SECONDS: u64 = 86_400;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    Token,
    LimitDelay,
    Config(Address),
    Pending(Address),
    DailySpend(Address, u64),
    Balance(Address),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpendingConfig {
    pub daily_limit: i128,
    pub per_tx_limit: i128,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingLimit {
    pub daily_limit: i128,
    pub per_tx_limit: i128,
    pub effective_at: u64,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum SpendingLimitError {
    AlreadyInitialized = 1,
    InvalidAmount = 2,
    InvalidLimit = 3,
    PerTransactionLimitExceeded = 4,
    DailyLimitExceeded = 5,
    InsufficientBalance = 6,
    NoPendingIncrease = 7,
    DelayNotElapsed = 8,
}

const LIMITS_SET: Symbol = symbol_short!("lim_set");
const PENDING_SET: Symbol = symbol_short!("pend_set");
const PENDING_APPLIED: Symbol = symbol_short!("pend_apl");
const DEPOSITED: Symbol = symbol_short!("deposit");
const WITHDRAWN: Symbol = symbol_short!("withdraw");
const SPENT: Symbol = symbol_short!("spent");

#[contract]
pub struct SpendingLimitContract;

#[contractimpl]
impl SpendingLimitContract {
    pub fn initialize(
        env: Env,
        admin: Address,
        token: Address,
        limit_delay: u64,
    ) -> Result<(), SpendingLimitError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(SpendingLimitError::AlreadyInitialized);
        }
        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::LimitDelay, &limit_delay);
        Ok(())
    }

    /// Configure the daily and per-transaction caps for an account.
    ///
    /// Decreases take effect immediately. Increases are queued and only become
    /// active after `apply_pending` is called once the configured delay passes.
    pub fn set_limits(
        env: Env,
        owner: Address,
        daily_limit: i128,
        per_tx_limit: i128,
    ) -> Result<(), SpendingLimitError> {
        owner.require_auth();

        if daily_limit <= 0 || per_tx_limit <= 0 || per_tx_limit > daily_limit {
            return Err(SpendingLimitError::InvalidLimit);
        }

        let now = env.ledger().timestamp();
        let key = DataKey::Config(owner.clone());
        let has_config = env.storage().persistent().has(&key);
        let current = Self::current_config(&env, &owner);

        if !has_config {
            let config = SpendingConfig {
                daily_limit,
                per_tx_limit,
                updated_at: now,
            };
            env.storage().persistent().set(&key, &config);
            env.events().publish((LIMITS_SET, owner), config);
            return Ok(());
        }

        let mut updated = current.clone();
        updated.daily_limit = daily_limit.min(current.daily_limit);
        updated.per_tx_limit = per_tx_limit.min(current.per_tx_limit);
        updated.updated_at = now;
        env.storage().persistent().set(&key, &updated);

        let daily_up = daily_limit > current.daily_limit;
        let per_tx_up = per_tx_limit > current.per_tx_limit;

        if daily_up || per_tx_up {
            let delay: u64 = env
                .storage()
                .instance()
                .get(&DataKey::LimitDelay)
                .unwrap_or(0);
            let pending = PendingLimit {
                daily_limit: if daily_up { daily_limit } else { updated.daily_limit },
                per_tx_limit: if per_tx_up {
                    per_tx_limit
                } else {
                    updated.per_tx_limit
                },
                effective_at: now.saturating_add(delay),
            };
            env.storage()
                .persistent()
                .set(&DataKey::Pending(owner.clone()), &pending);
            env.events().publish((PENDING_SET, owner.clone()), pending);
        } else {
            env.storage()
                .persistent()
                .remove(&DataKey::Pending(owner.clone()));
        }

        env.events().publish((LIMITS_SET, owner), updated);
        Ok(())
    }

    /// Apply a matured pending limit increase. Callable by anyone.
    pub fn apply_pending(env: Env, account: Address) -> Result<(), SpendingLimitError> {
        let pending: PendingLimit = env
            .storage()
            .persistent()
            .get(&DataKey::Pending(account.clone()))
            .ok_or(SpendingLimitError::NoPendingIncrease)?;

        let now = env.ledger().timestamp();
        if now < pending.effective_at {
            return Err(SpendingLimitError::DelayNotElapsed);
        }

        let config = SpendingConfig {
            daily_limit: pending.daily_limit,
            per_tx_limit: pending.per_tx_limit,
            updated_at: now,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Config(account.clone()), &config);
        env.storage()
            .persistent()
            .remove(&DataKey::Pending(account.clone()));
        env.events().publish((PENDING_APPLIED, account), config);
        Ok(())
    }

    /// Deposit tokens into the account's spending vault.
    pub fn deposit(env: Env, owner: Address, amount: i128) -> Result<(), SpendingLimitError> {
        owner.require_auth();
        if amount <= 0 {
            return Err(SpendingLimitError::InvalidAmount);
        }

        let token_address: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_address).transfer(
            &owner,
            &env.current_contract_address(),
            &amount,
        );

        let key = DataKey::Balance(owner.clone());
        let balance: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage().persistent().set(&key, &(balance + amount));
        env.events().publish((DEPOSITED, owner), amount);
        Ok(())
    }

    /// Withdraw idle funds. The owner may always withdraw directly.
    pub fn withdraw(env: Env, owner: Address, amount: i128) -> Result<(), SpendingLimitError> {
        owner.require_auth();
        if amount <= 0 {
            return Err(SpendingLimitError::InvalidAmount);
        }

        let key = DataKey::Balance(owner.clone());
        let balance: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if balance < amount {
            return Err(SpendingLimitError::InsufficientBalance);
        }
        env.storage().persistent().set(&key, &(balance - amount));

        let token_address: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_address).transfer(
            &env.current_contract_address(),
            &owner,
            &amount,
        );
        env.events().publish((WITHDRAWN, owner), amount);
        Ok(())
    }

    /// Spend from the vault, enforcing both the per-transaction and daily caps.
    pub fn spend(
        env: Env,
        owner: Address,
        to: Address,
        amount: i128,
    ) -> Result<(), SpendingLimitError> {
        owner.require_auth();
        if amount <= 0 {
            return Err(SpendingLimitError::InvalidAmount);
        }

        let config = Self::current_config(&env, &owner);
        if config.per_tx_limit <= 0 {
            return Err(SpendingLimitError::InvalidLimit);
        }
        if amount > config.per_tx_limit {
            return Err(SpendingLimitError::PerTransactionLimitExceeded);
        }

        let day = Self::day_index(&env);
        let spend_key = DataKey::DailySpend(owner.clone(), day);
        let spent_today: i128 = env.storage().persistent().get(&spend_key).unwrap_or(0);
        if spent_today + amount > config.daily_limit {
            return Err(SpendingLimitError::DailyLimitExceeded);
        }

        let balance_key = DataKey::Balance(owner.clone());
        let balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
        if balance < amount {
            return Err(SpendingLimitError::InsufficientBalance);
        }

        env.storage().persistent().set(&balance_key, &(balance - amount));
        env.storage()
            .persistent()
            .set(&spend_key, &(spent_today + amount));

        let token_address: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_address).transfer(
            &env.current_contract_address(),
            &to,
            &amount,
        );
        env.events().publish((SPENT, owner), (to, amount));
        Ok(())
    }

    pub fn get_config(env: Env, account: Address) -> SpendingConfig {
        Self::current_config(&env, &account)
    }

    pub fn get_pending(env: Env, account: Address) -> Option<PendingLimit> {
        env.storage()
            .persistent()
            .get(&DataKey::Pending(account))
    }

    pub fn daily_spend(env: Env, account: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::DailySpend(account, Self::day_index(&env)))
            .unwrap_or(0)
    }

    pub fn get_balance(env: Env, account: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(account))
            .unwrap_or(0)
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    }

    pub fn get_delay(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::LimitDelay)
            .unwrap_or(0)
    }

    fn current_config(env: &Env, account: &Address) -> SpendingConfig {
        env.storage()
            .persistent()
            .get(&DataKey::Config(account.clone()))
            .unwrap_or(SpendingConfig {
                daily_limit: 0,
                per_tx_limit: 0,
                updated_at: 0,
            })
    }

    fn day_index(env: &Env) -> u64 {
        env.ledger().timestamp() / DAY_SECONDS
    }
}

#[cfg(test)]
mod test;
