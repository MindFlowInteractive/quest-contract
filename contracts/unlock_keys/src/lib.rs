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
    UpgradeRatioNotConfigured = 6,
    InvalidTier = 7,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    PuzzleKey(u32),           // puzzle_id -> required key_type (tier)
    KeyBalance(Address, u32), // (player, key_type) -> balance
    TotalSupply(u32),         // key_type -> circulating supply
    BurnRatio(u32),           // from_tier -> keys required to mint 1 key of next tier
    Unlocked(Address, u32),   // (player, puzzle_id) -> bool
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PuzzleUnlockedEvent {
    pub player: Address,
    pub puzzle_id: u32,
    pub key_type: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyUpgradedEvent {
    pub player: Address,
    pub from_tier: u32,
    pub to_tier: u32,
    pub burned_amount: u32,
    pub minted_amount: u32,
}
#[contract]
pub struct UnlockKeysContract;

#[contractimpl]
impl UnlockKeysContract {
    /// Initializes the contract with an administrative address.
    pub fn initialize(env: Env, admin: Address) -> Result<(), UnlockError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(UnlockError::NotAuthorized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    /// Maps a locked puzzle ID to the required key type/tier ID.
    pub fn set_puzzle_key(
        env: Env,
        caller: Address,
        puzzle_id: u32,
        key_type: u32,
    ) -> Result<(), UnlockError> {
        caller.require_auth();
        Self::require_admin(&env, &caller)?;

        env.storage()
            .persistent()
            .set(&DataKey::PuzzleKey(puzzle_id), &key_type);
        Ok(())
    }

    /// Configures how many keys of `from_tier` must be burned to mint 1 key of `from_tier + 1`.
    pub fn set_burn_ratio(
        env: Env,
        caller: Address,
        from_tier: u32,
        burn_ratio: u32,
    ) -> Result<(), UnlockError> {
        caller.require_auth();
        Self::require_admin(&env, &caller)?;

        if burn_ratio == 0 {
            return Err(UnlockError::InvalidAmount);
        }
        from_tier.checked_add(1).ok_or(UnlockError::InvalidTier)?;

        env.storage()
            .persistent()
            .set(&DataKey::BurnRatio(from_tier), &burn_ratio);
        Ok(())
    }

    /// Grants keys of `key_type` to `player` and increments circulating supply for that tier.
    pub fn grant_keys(
        env: Env,
        caller: Address,
        player: Address,
        key_type: u32,
        amount: u32,
    ) -> Result<(), UnlockError> {
        caller.require_auth();
        Self::require_admin(&env, &caller)?;

        if amount == 0 {
            return Err(UnlockError::InvalidAmount);
        }

        let current_balance = Self::key_balance(env.clone(), player.clone(), key_type);
        let next_balance = current_balance
            .checked_add(amount)
            .ok_or(UnlockError::InvalidAmount)?;

        let current_supply = Self::total_supply(env.clone(), key_type);
        let next_supply = current_supply
            .checked_add(amount)
            .ok_or(UnlockError::InvalidAmount)?;

        env.storage()
            .persistent()
            .set(&DataKey::KeyBalance(player, key_type), &next_balance);
        env.storage()
            .persistent()
            .set(&DataKey::TotalSupply(key_type), &next_supply);
        Ok(())
    }

    /// Burns `burn_ratio` keys of `from_tier` from `player` and mints 1 key of `from_tier + 1` to `player`.
    pub fn burn_to_upgrade(env: Env, player: Address, from_tier: u32) -> Result<u32, UnlockError> {
        player.require_auth();

        let to_tier = from_tier.checked_add(1).ok_or(UnlockError::InvalidTier)?;
        let burn_ratio: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::BurnRatio(from_tier))
            .ok_or(UnlockError::UpgradeRatioNotConfigured)?;

        let from_balance_key = DataKey::KeyBalance(player.clone(), from_tier);
        let current_from_balance: u32 = env
            .storage()
            .persistent()
            .get(&from_balance_key)
            .unwrap_or(0);

        if current_from_balance < burn_ratio {
            return Err(UnlockError::InsufficientKeys);
        }

        let next_from_balance = current_from_balance - burn_ratio;
        let current_from_supply = Self::total_supply(env.clone(), from_tier);
        let next_from_supply = current_from_supply
            .checked_sub(burn_ratio)
            .ok_or(UnlockError::InvalidAmount)?;

        let to_balance_key = DataKey::KeyBalance(player.clone(), to_tier);
        let current_to_balance: u32 = env
            .storage()
            .persistent()
            .get(&to_balance_key)
            .unwrap_or(0);
        let next_to_balance = current_to_balance
            .checked_add(1)
            .ok_or(UnlockError::InvalidAmount)?;

        let current_to_supply = Self::total_supply(env.clone(), to_tier);
        let next_to_supply = current_to_supply
            .checked_add(1)
            .ok_or(UnlockError::InvalidAmount)?;

        env.storage()
            .persistent()
            .set(&from_balance_key, &next_from_balance);
        env.storage()
            .persistent()
            .set(&DataKey::TotalSupply(from_tier), &next_from_supply);
        env.storage()
            .persistent()
            .set(&to_balance_key, &next_to_balance);
        env.storage()
            .persistent()
            .set(&DataKey::TotalSupply(to_tier), &next_to_supply);

        KeyUpgradedEvent {
            player,
            from_tier,
            to_tier,
            burned_amount: burn_ratio,
            minted_amount: 1,
        }
        .publish(&env);

        Ok(to_tier)
    }

    /// Consumes 1 key of the matching type to unlock `puzzle_id` for `player`.
    pub fn open(env: Env, player: Address, puzzle_id: u32) -> Result<bool, UnlockError> {
        player.require_auth();

        let unlock_key = DataKey::Unlocked(player.clone(), puzzle_id);
        let already_unlocked: bool = env
            .storage()
            .persistent()
            .get(&unlock_key)
            .unwrap_or(false);

        if already_unlocked {
            return Err(UnlockError::AlreadyUnlocked);
        }

        let required_key_type: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::PuzzleKey(puzzle_id))
            .ok_or(UnlockError::PuzzleKeyNotConfigured)?;

        let balance_key = DataKey::KeyBalance(player.clone(), required_key_type);
        let current_balance: u32 = env
            .storage()
            .persistent()
            .get(&balance_key)
            .unwrap_or(0);

        if current_balance < 1 {
            return Err(UnlockError::InsufficientKeys);
        }

        let next_balance = current_balance - 1;
        let current_supply = Self::total_supply(env.clone(), required_key_type);
        let next_supply = current_supply
            .checked_sub(1)
            .ok_or(UnlockError::InvalidAmount)?;

        env.storage().persistent().set(&balance_key, &next_balance);
        env.storage()
            .persistent()
            .set(&DataKey::TotalSupply(required_key_type), &next_supply);
        env.storage().persistent().set(&unlock_key, &true);

        PuzzleUnlockedEvent {
            player,
            puzzle_id,
            key_type: required_key_type,
        }
        .publish(&env);

        Ok(true)
    }

    pub fn is_unlocked(env: Env, player: Address, puzzle_id: u32) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Unlocked(player, puzzle_id))
            .unwrap_or(false)
    }

    pub fn key_balance(env: Env, player: Address, key_type: u32) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::KeyBalance(player, key_type))
            .unwrap_or(0)
    }

    pub fn total_supply(env: Env, key_type: u32) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::TotalSupply(key_type))
            .unwrap_or(0)
    }

    pub fn burn_ratio(env: Env, from_tier: u32) -> Option<u32> {
        env.storage().persistent().get(&DataKey::BurnRatio(from_tier))
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), UnlockError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(UnlockError::NotAuthorized)?;
        if caller != &admin {
            return Err(UnlockError::NotAuthorized);
        }
        Ok(())
    }
}
