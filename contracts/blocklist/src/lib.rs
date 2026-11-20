#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror, contractmeta,
    Address, Env, Vec, Symbol, symbol_short,
};

mod test;

// Contract metadata
contractmeta!(
    key = "Description",
    val = "Blocklist contract for access control, game security, and compliance"
);

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlocklistEntry {
    pub address: Address,
    pub reason: Symbol,
    pub blocked_at: u64,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum BlocklistError {
    NotAuthorized = 1,
    AlreadyBlocked = 2,
    NotBlocked = 3,
    AdminNotFound = 4,
    AlreadyInitialized = 5,
}

// Storage keys
const ADMIN: Symbol = symbol_short!("ADMIN");
const BLIST: Symbol = symbol_short!("BLIST");

#[contract]
pub struct BlocklistContract;

#[contractimpl]
impl BlocklistContract {
    /// Initialize the contract with an admin address
    pub fn initialize(env: Env, admin: Address) -> Result<(), BlocklistError> {
        admin.require_auth();
        if env.storage().instance().has(&ADMIN) {
            return Err(BlocklistError::AlreadyInitialized);
        }
        env.storage().instance().set(&ADMIN, &admin);
        Ok(())
    }

    /// Block an address with a given reason. Admin only.
    pub fn block_address(
        env: Env,
        admin: Address,
        address: Address,
        reason: Symbol,
    ) -> Result<(), BlocklistError> {
        Self::require_admin(&env, &admin)?;

        let key = (BLIST, address.clone());
        if env.storage().persistent().has(&key) {
            return Err(BlocklistError::AlreadyBlocked);
        }

        let entry = BlocklistEntry {
            address: address.clone(),
            reason: reason.clone(),
            blocked_at: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&key, &entry);

        // Emit block event
        env.events().publish((symbol_short!("block"), address), reason);

        Ok(())
    }

    /// Unblock an address. Admin only.
    pub fn unblock_address(
        env: Env,
        admin: Address,
        address: Address,
    ) -> Result<(), BlocklistError> {
        Self::require_admin(&env, &admin)?;

        let key = (BLIST, address.clone());
        if !env.storage().persistent().has(&key) {
            return Err(BlocklistError::NotBlocked);
        }

        env.storage().persistent().remove(&key);

        // Emit unblock event
        env.events().publish((symbol_short!("unblock"), address), ());

        Ok(())
    }

    /// Check if an address is blocked. Open read function for other contracts.
    pub fn is_blocked(env: Env, address: Address) -> bool {
        let key = (BLIST, address);
        env.storage().persistent().has(&key)
    }

    /// Retrieve block entry details for an address if blocked.
    pub fn get_block_entry(env: Env, address: Address) -> Option<BlocklistEntry> {
        let key = (BLIST, address);
        env.storage().persistent().get(&key)
    }

    /// Batch block addresses. Admin only.
    pub fn batch_block(
        env: Env,
        admin: Address,
        addresses: Vec<Address>,
        reason: Symbol,
    ) -> Result<(), BlocklistError> {
        Self::require_admin(&env, &admin)?;

        for addr in addresses.iter() {
            let key = (BLIST, addr.clone());
            if !env.storage().persistent().has(&key) {
                let entry = BlocklistEntry {
                    address: addr.clone(),
                    reason: reason.clone(),
                    blocked_at: env.ledger().timestamp(),
                };
                env.storage().persistent().set(&key, &entry);
                env.events().publish((symbol_short!("block"), addr), reason.clone());
            }
        }

        Ok(())
    }

    /// Batch unblock addresses. Admin only.
    pub fn batch_unblock(
        env: Env,
        admin: Address,
        addresses: Vec<Address>,
    ) -> Result<(), BlocklistError> {
        Self::require_admin(&env, &admin)?;

        for addr in addresses.iter() {
            let key = (BLIST, addr.clone());
            if env.storage().persistent().has(&key) {
                env.storage().persistent().remove(&key);
                env.events().publish((symbol_short!("unblock"), addr), ());
            }
        }

        Ok(())
    }

    /// Transfer contract admin role. Admin only.
    pub fn transfer_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), BlocklistError> {
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
    fn require_admin(env: &Env, admin: &Address) -> Result<(), BlocklistError> {
        admin.require_auth();

        let stored_admin: Option<Address> = env.storage().instance().get(&ADMIN);

        match stored_admin {
            Some(stored) if stored == *admin => Ok(()),
            Some(_) => Err(BlocklistError::NotAuthorized),
            None => Err(BlocklistError::AdminNotFound),
        }
    }
}
