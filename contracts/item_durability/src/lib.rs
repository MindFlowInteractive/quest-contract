#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contractmeta, contracttype, symbol_short, token,
    Address, Env, Symbol,
};

#[cfg(test)]
mod test;

// Contract metadata
contractmeta!(
    key = "Description",
    val = "Item Durability contract supporting use decrement, repair with token sink, and salvage of broken items"
);

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    pub owner: Address,
    pub max_durability: u32,
    pub current_durability: u32,
    pub is_broken: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepairConfig {
    pub token: Address,
    pub cost_per_unit: i128,
    pub sink_address: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SalvageConfig {
    pub material_token: Address,
    pub material_payout: i128,
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
    ItemBroken = 8,
    ItemNotBroken = 9,
    RepairNotConfigured = 10,
    SalvageNotConfigured = 11,
    InvalidAmount = 12,
}

// Storage keys
const ADMIN: Symbol = symbol_short!("ADMIN");
const ITEM: Symbol = symbol_short!("ITEM");
const REPAIR_CFG: Symbol = symbol_short!("REP_CFG");
const SALVAGE_CFG: Symbol = symbol_short!("SAL_CFG");

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

    /// Configure repair parameters: token accepted, cost per durability unit, and sink address.
    pub fn set_repair_config(
        env: Env,
        admin: Address,
        token: Address,
        cost_per_unit: i128,
        sink_address: Address,
    ) -> Result<(), DurabilityError> {
        Self::require_admin(&env, &admin)?;

        if cost_per_unit <= 0 {
            return Err(DurabilityError::InvalidAmount);
        }

        let cfg = RepairConfig {
            token,
            cost_per_unit,
            sink_address,
        };
        env.storage().instance().set(&REPAIR_CFG, &cfg);
        Ok(())
    }

    /// Configure salvage payout parameters: material token address and payout per salvaged item.
    pub fn set_salvage_config(
        env: Env,
        admin: Address,
        material_token: Address,
        material_payout: i128,
    ) -> Result<(), DurabilityError> {
        Self::require_admin(&env, &admin)?;

        if material_payout <= 0 {
            return Err(DurabilityError::InvalidAmount);
        }

        let cfg = SalvageConfig {
            material_token,
            material_payout,
        };
        env.storage().instance().set(&SALVAGE_CFG, &cfg);
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
            is_broken: false,
        };

        env.storage().persistent().set(&key, &item);
        Ok(())
    }

    /// Use an item in a puzzle/game, decrementing its durability by 1.
    /// When durability drops to zero, marks item as broken and emits a broken event.
    /// Rejects if durability is already zero or item is broken.
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

        if item.is_broken || item.current_durability == 0 {
            return Err(DurabilityError::ZeroDurability);
        }

        item.current_durability -= 1;
        if item.current_durability == 0 {
            item.is_broken = true;
            env.events().publish(
                (symbol_short!("broken"), item_id),
                (user.clone(), item_id),
            );
        }

        env.storage().persistent().set(&key, &item);

        // Emit durability change event: (use, item_id) -> (user, current_durability)
        env.events().publish(
            (symbol_short!("use"), item_id),
            (user, item.current_durability),
        );

        Ok(item.current_durability)
    }

    /// Restore item durability by paying tokens to the designated sink address.
    /// Repair cost is proportional to missing durability.
    /// Durability is capped at max durability.
    /// Blocks repair if item is broken (zero durability reached).
    pub fn repair_item(
        env: Env,
        user: Address,
        item_id: u64,
        amount_to_repair: u32,
    ) -> Result<u32, DurabilityError> {
        user.require_auth();

        if amount_to_repair == 0 {
            return Err(DurabilityError::InvalidAmount);
        }

        let key = (ITEM, item_id);
        let mut item: Item = match env.storage().persistent().get(&key) {
            Some(i) => i,
            None => return Err(DurabilityError::ItemNotFound),
        };

        if item.owner != user {
            return Err(DurabilityError::NotAuthorized);
        }

        // Broken items cannot be repaired
        if item.is_broken || item.current_durability == 0 {
            return Err(DurabilityError::ItemBroken);
        }

        let cfg: RepairConfig = match env.storage().instance().get(&REPAIR_CFG) {
            Some(c) => c,
            None => return Err(DurabilityError::RepairNotConfigured),
        };

        let missing = item.max_durability.saturating_sub(item.current_durability);
        if missing == 0 {
            return Ok(item.current_durability);
        }

        // Cap repair at max durability
        let units_to_repair = amount_to_repair.min(missing);
        let total_cost = (units_to_repair as i128)
            .checked_mul(cfg.cost_per_unit)
            .unwrap_or(0);

        if total_cost > 0 {
            token::Client::new(&env, &cfg.token).transfer(
                &user,
                &cfg.sink_address,
                &total_cost,
            );
        }

        item.current_durability += units_to_repair;
        env.storage().persistent().set(&key, &item);

        // Emit repair event: (repair, item_id) -> (user, units_repaired, total_cost)
        env.events().publish(
            (symbol_short!("repair"), item_id),
            (user, units_to_repair, total_cost),
        );

        Ok(item.current_durability)
    }

    /// Salvage a broken item for material payout.
    /// Once salvaged, item record is destroyed and cannot be salvaged twice.
    pub fn salvage_item(
        env: Env,
        user: Address,
        item_id: u64,
    ) -> Result<i128, DurabilityError> {
        user.require_auth();

        let key = (ITEM, item_id);
        let item: Item = match env.storage().persistent().get(&key) {
            Some(i) => i,
            None => return Err(DurabilityError::ItemNotFound),
        };

        if item.owner != user {
            return Err(DurabilityError::NotAuthorized);
        }

        if !item.is_broken && item.current_durability > 0 {
            return Err(DurabilityError::ItemNotBroken);
        }

        let cfg: SalvageConfig = match env.storage().instance().get(&SALVAGE_CFG) {
            Some(c) => c,
            None => return Err(DurabilityError::SalvageNotConfigured),
        };

        // Destroy item record so item cannot be salvaged twice
        env.storage().persistent().remove(&key);

        // Transfer configured material payout to owner
        if cfg.material_payout > 0 {
            token::Client::new(&env, &cfg.material_token).transfer(
                &env.current_contract_address(),
                &user,
                &cfg.material_payout,
            );
        }

        // Emit salvage event: (salvage, item_id) -> (user, material_payout)
        env.events().publish(
            (symbol_short!("salvage"), item_id),
            (user, cfg.material_payout),
        );

        Ok(cfg.material_payout)
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

    /// Check if an item is broken
    pub fn is_broken(env: Env, item_id: u64) -> Result<bool, DurabilityError> {
        let key = (ITEM, item_id);
        let item: Option<Item> = env.storage().persistent().get(&key);
        match item {
            Some(i) => Ok(i.is_broken),
            None => Err(DurabilityError::ItemNotFound),
        }
    }

    /// Calculate repair cost without executing repair
    pub fn calculate_repair_cost(
        env: Env,
        item_id: u64,
        amount_to_repair: u32,
    ) -> Result<i128, DurabilityError> {
        let key = (ITEM, item_id);
        let item: Item = match env.storage().persistent().get(&key) {
            Some(i) => i,
            None => return Err(DurabilityError::ItemNotFound),
        };

        if item.is_broken || item.current_durability == 0 {
            return Err(DurabilityError::ItemBroken);
        }

        let cfg: RepairConfig = match env.storage().instance().get(&REPAIR_CFG) {
            Some(c) => c,
            None => return Err(DurabilityError::RepairNotConfigured),
        };

        let missing = item.max_durability.saturating_sub(item.current_durability);
        let units = amount_to_repair.min(missing);
        Ok((units as i128).checked_mul(cfg.cost_per_unit).unwrap_or(0))
    }

    /// Get current repair configuration
    pub fn get_repair_config(env: Env) -> Option<RepairConfig> {
        env.storage().instance().get(&REPAIR_CFG)
    }

    /// Get current salvage configuration
    pub fn get_salvage_config(env: Env) -> Option<SalvageConfig> {
        env.storage().instance().get(&SALVAGE_CFG)
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
