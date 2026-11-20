#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, String, Symbol,
    Vec,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    Minter(Address),
    Balance(String, Address),
    TotalSupply(String),
    KeyTypes,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum KeyError {
    AlreadyInitialized = 1,
    NotAuthorized = 2,
    InvalidAmount = 3,
    InsufficientBalance = 4,
    TypeAlreadyRegistered = 5,
    TypeNotRegistered = 6,
}

const TYPE_REGISTERED: Symbol = symbol_short!("key_type");
const KEYS_MINTED: Symbol = symbol_short!("key_mint");
const KEYS_TRANSFERRED: Symbol = symbol_short!("key_xfer");
const KEYS_BURNED: Symbol = symbol_short!("key_burn");

#[contract]
pub struct UnlockKeysContract;

#[contractimpl]
impl UnlockKeysContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), KeyError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(KeyError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::KeyTypes, &Vec::<String>::new(&env));
        Ok(())
    }

    /// Register a new key type with an initial supply of zero.
    pub fn register_key_type(
        env: Env,
        admin: Address,
        key_type: String,
    ) -> Result<(), KeyError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;

        if env
            .storage()
            .instance()
            .has(&DataKey::TotalSupply(key_type.clone()))
        {
            return Err(KeyError::TypeAlreadyRegistered);
        }

        env.storage()
            .instance()
            .set(&DataKey::TotalSupply(key_type.clone()), &0i128);
        let key = DataKey::KeyTypes;
        let mut types: Vec<String> = env.storage().instance().get(&key).unwrap_or_else(|| Vec::new(&env));
        types.push_back(key_type.clone());
        env.storage().instance().set(&key, &types);
        env.events().publish((TYPE_REGISTERED,), key_type);
        Ok(())
    }

    /// Authorize an address to mint keys (admin only).
    pub fn authorize_minter(env: Env, admin: Address, minter: Address) -> Result<(), KeyError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::Minter(minter.clone()), &true);
        env.events().publish((symbol_short!("minter_ok"),), minter);
        Ok(())
    }

    /// Revoke a minter (admin only).
    pub fn revoke_minter(env: Env, admin: Address, minter: Address) -> Result<(), KeyError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .remove(&DataKey::Minter(minter.clone()));
        env.events().publish((symbol_short!("minter_no"),), minter);
        Ok(())
    }

    /// Mint keys of a registered type to a player. Admin or authorized minter only.
    pub fn mint(
        env: Env,
        minter: Address,
        to: Address,
        key_type: String,
        amount: i128,
    ) -> Result<(), KeyError> {
        minter.require_auth();
        if amount <= 0 {
            return Err(KeyError::InvalidAmount);
        }
        Self::require_minter(&env, &minter)?;
        Self::require_registered(&env, &key_type)?;

        let balance_key = DataKey::Balance(key_type.clone(), to.clone());
        let balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
        env.storage().persistent().set(&balance_key, &(balance + amount));

        let supply_key = DataKey::TotalSupply(key_type.clone());
        let supply: i128 = env.storage().instance().get(&supply_key).unwrap_or(0);
        env.storage().instance().set(&supply_key, &(supply + amount));

        env.events().publish((KEYS_MINTED, key_type), (to, amount));
        Ok(())
    }

    /// Transfer keys between players.
    pub fn transfer(
        env: Env,
        from: Address,
        to: Address,
        key_type: String,
        amount: i128,
    ) -> Result<(), KeyError> {
        from.require_auth();
        if amount <= 0 {
            return Err(KeyError::InvalidAmount);
        }
        Self::require_registered(&env, &key_type)?;

        let from_key = DataKey::Balance(key_type.clone(), from.clone());
        let from_balance: i128 = env.storage().persistent().get(&from_key).unwrap_or(0);
        if from_balance < amount {
            return Err(KeyError::InsufficientBalance);
        }

        let to_key = DataKey::Balance(key_type.clone(), to.clone());
        let to_balance: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);

        env.storage()
            .persistent()
            .set(&from_key, &(from_balance - amount));
        env.storage()
            .persistent()
            .set(&to_key, &(to_balance + amount));

        env.events()
            .publish((KEYS_TRANSFERRED, key_type), (from, to, amount));
        Ok(())
    }

    /// Consume keys (e.g. to open a locked puzzle), reducing circulating supply.
    pub fn burn(
        env: Env,
        holder: Address,
        key_type: String,
        amount: i128,
    ) -> Result<(), KeyError> {
        holder.require_auth();
        if amount <= 0 {
            return Err(KeyError::InvalidAmount);
        }
        Self::require_registered(&env, &key_type)?;

        let balance_key = DataKey::Balance(key_type.clone(), holder.clone());
        let balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
        if balance < amount {
            return Err(KeyError::InsufficientBalance);
        }
        env.storage()
            .persistent()
            .set(&balance_key, &(balance - amount));

        let supply_key = DataKey::TotalSupply(key_type.clone());
        let supply: i128 = env.storage().instance().get(&supply_key).unwrap_or(0);
        env.storage().instance().set(&supply_key, &(supply - amount));

        env.events().publish((KEYS_BURNED, key_type), (holder, amount));
        Ok(())
    }

    pub fn balance(env: Env, holder: Address, key_type: String) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(key_type, holder))
            .unwrap_or(0)
    }

    pub fn total_supply(env: Env, key_type: String) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalSupply(key_type))
            .unwrap_or(0)
    }

    pub fn key_types(env: Env) -> Vec<String> {
        env.storage()
            .instance()
            .get(&DataKey::KeyTypes)
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn is_authorized_minter(env: Env, minter: Address) -> bool {
        if minter == Self::admin(&env) {
            return true;
        }
        env.storage()
            .instance()
            .get(&DataKey::Minter(minter))
            .unwrap_or(false)
    }

    pub fn admin(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), KeyError> {
        if admin != &Self::admin(env) {
            return Err(KeyError::NotAuthorized);
        }
        Ok(())
    }

    fn require_minter(env: &Env, minter: &Address) -> Result<(), KeyError> {
        if Self::is_authorized_minter(env.clone(), minter.clone()) {
            Ok(())
        } else {
            Err(KeyError::NotAuthorized)
        }
    }

    fn require_registered(env: &Env, key_type: &String) -> Result<(), KeyError> {
        if env
            .storage()
            .instance()
            .has(&DataKey::TotalSupply(key_type.clone()))
        {
            Ok(())
        } else {
            Err(KeyError::TypeNotRegistered)
        }
    }
}

#[cfg(test)]
mod test;

#![no_std]

use soroban_sdk::{contract, contracterror, contractevent, contractimpl, contracttype, Address, Env};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum UnlockError {
    NotAuthorized = 1,
    PuzzleKeyNotConfigured = 2,
    InsufficientKeys = 3,
    AlreadyUnlocked = 4,
    InvalidAmount = 5,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    PuzzleKey(u32),          // puzzle_id -> required key_type
    KeyBalance(Address, u32), // (player, key_type) -> balance
    Unlocked(Address, u32),   // (player, puzzle_id) -> bool
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PuzzleUnlockedEvent {
    pub player: Address,
    pub puzzle_id: u32,
    pub key_type: u32,
}