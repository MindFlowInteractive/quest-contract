#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror, contractmeta,
    Address, Env, Symbol, symbol_short,
};

mod test;

// Contract metadata
contractmeta!(
    key = "Description",
    val = "Item Durability contract for tracking and decrementing game item durability"
);

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    pub owner: Address,
    pub max_durability: u32,
    pub current_durability: u32,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum DurabilityError {
    NotAuthorized = 1,
    ItemNotFound = 2,
    ZeroDurability = 3,
    InvalidMaxDurability = 4,
    ItemAlreadyExists = 5,
    AdminNotFound = 6,
    AlreadyInitialized = 7,
}

// Storage keys
const ADMIN: Symbol = symbol_short!("ADMIN");
const ITEM: Symbol = symbol_short!("ITEM");

#[contract]
pub struct ItemDurabilityContract;

#[contractimpl]
impl ItemDurabilityContract {
    /// Initialize the contract with an admin address
    pub fn initialize(env: Env, admin: Address) -> Result<(), DurabilityError> {
        admin.require_auth();
        if env.storage().instance().has(&ADMIN) {
            return Err(DurabilityError::AlreadyInitialized);
        }
        env.storage().instance().set(&ADMIN, &admin);
        Ok(())
    }

    /// Register a new item with max durability. Admin only.
    pub fn register_item(
        env: Env,
        admin: Address,
        item_id: u64,
        owner: Address,
        max_durability: u32,
    ) -> Result<(), DurabilityError> {
        Self::require_admin(&env, &admin)?;

        if max_durability == 0 {
            return Err(DurabilityError::InvalidMaxDurability);
        }

        let key = (ITEM, item_id);
        if env.storage().persistent().has(&key) {
            return Err(DurabilityError::ItemAlreadyExists);
        }

        let item = Item {
            owner,
            max_durability,
            current_durability: max_durability,
        };

        env.storage().persistent().set(&key, &item);
        Ok(())
    }

    /// Use an item in a puzzle/game, decrementing its durability by 1.
    /// Rejects if durability is zero. Emits durability change event.
    pub fn use_item(
        env: Env,
        user: Address,
        item_id: u64,
    ) -> Result<u32, DurabilityError> {
        user.require_auth();

        let key = (ITEM, item_id);
        let mut item: Item = match env.storage().persistent().get(&key) {
            Some(i) => i,
            None => return Err(DurabilityError::ItemNotFound),
        };

        if item.owner != user {
            return Err(DurabilityError::NotAuthorized);
        }

        if item.current_durability == 0 {
            return Err(DurabilityError::ZeroDurability);
        }

        item.current_durability -= 1;
        env.storage().persistent().set(&key, &item);

        // Emit durability change event: (use, item_id) -> (user, current_durability)
        env.events().publish(
            (symbol_short!("use"), item_id),
            (user, item.current_durability),
        );

        Ok(item.current_durability)
    }

    /// Get item details by item ID
    pub fn get_item(env: Env, item_id: u64) -> Option<Item> {
        let key = (ITEM, item_id);
        env.storage().persistent().get(&key)
    }

    /// Get current durability for an item
    pub fn get_durability(env: Env, item_id: u64) -> Result<u32, DurabilityError> {
        let key = (ITEM, item_id);
        let item: Option<Item> = env.storage().persistent().get(&key);
        match item {
            Some(i) => Ok(i.current_durability),
            None => Err(DurabilityError::ItemNotFound),
        }
    }

    /// Transfer contract admin role. Admin only.
    pub fn transfer_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), DurabilityError> {
        Self::require_admin(&env, &current_admin)?;
        new_admin.require_auth();

        env.storage().instance().set(&ADMIN, &new_admin);
        Ok(())
    }

    /// Get current admin
    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&ADMIN)
    }

    // Helper functions
    fn require_admin(env: &Env, admin: &Address) -> Result<(), DurabilityError> {
        admin.require_auth();

        let stored_admin: Option<Address> = env.storage().instance().get(&ADMIN);

        match stored_admin {
            Some(stored) if stored == *admin => Ok(()),
            Some(_) => Err(DurabilityError::NotAuthorized),
            None => Err(DurabilityError::AdminNotFound),
        }
    }
}
