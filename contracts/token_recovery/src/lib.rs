#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, token, Address, Env,
};

#[contracttype]
pub enum DataKey {
    Admin,
    Owed(Address),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    InvalidAmount = 4,
    NoSurplus = 5,
    ExceedsSurplus = 6,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Recovered {
    pub token: Address,
    pub to: Address,
    pub amount: i128,
}

#[contract]
pub struct TokenRecoveryContract;

fn read_admin(env: &Env) -> Result<Address, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::NotInitialized)
}

fn read_owed(env: &Env, token: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::Owed(token.clone()))
        .unwrap_or(0)
}

fn write_owed(env: &Env, token: &Address, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::Owed(token.clone()), &amount);
}

#[contractimpl]
impl TokenRecoveryContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    /// Records that the contract owes `amount` of `token` to players.
    /// Admin-only. Called by game logic when credits are assigned.
    pub fn add_owed(env: Env, token: Address, amount: i128) -> Result<(), Error> {
        let admin = read_admin(&env)?;
        admin.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let current = read_owed(&env, &token);
        write_owed(&env, &token, current + amount);
        Ok(())
    }

    /// Records that `amount` of `token` has been paid out to players,
    /// reducing the owed balance. Admin-only.
    pub fn settle_owed(env: Env, token: Address, amount: i128) -> Result<(), Error> {
        let admin = read_admin(&env)?;
        admin.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let current = read_owed(&env, &token);
        if amount > current {
            return Err(Error::InvalidAmount);
        }
        write_owed(&env, &token, current - amount);
        Ok(())
    }

    /// Returns the amount of `token` the contract owes to players.
    pub fn owed(env: Env, token: Address) -> i128 {
        read_owed(&env, &token)
    }

    /// Returns the recoverable surplus: contract balance minus owed amount.
    pub fn recoverable(env: Env, token: Address) -> i128 {
        let balance = token::Client::new(&env, &token).balance(&env.current_contract_address());
        let owed = read_owed(&env, &token);
        balance - owed
    }

    /// Admin-only: transfers surplus tokens (never owed balances) to `to`.
    pub fn recover(
        env: Env,
        token: Address,
        to: Address,
        amount: i128,
    ) -> Result<(), Error> {
        let admin = read_admin(&env)?;
        admin.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let client = token::Client::new(&env, &token);
        let this = env.current_contract_address();
        let balance = client.balance(&this);
        let owed = read_owed(&env, &token);
        let surplus = balance - owed;

        if surplus <= 0 {
            return Err(Error::NoSurplus);
        }
        if amount > surplus {
            return Err(Error::ExceedsSurplus);
        }

        client.transfer(&this, &to, &amount);

        Recovered {
            token: token.clone(),
            to: to.clone(),
            amount,
        }
        .publish(&env);

        Ok(())
    }
}
