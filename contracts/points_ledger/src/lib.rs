#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror, contractmeta,
    Address, Env, Vec, Symbol, symbol_short,
};

mod test;

// Contract metadata
contractmeta!(
    key = "Description",
    val = "Achievement Points Ledger contract for tracking earned points with full audit history"
);

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PointsEntry {
    pub player: Address,
    pub amount: u64,
    pub reason_code: Symbol,
    pub timestamp: u64,
    pub caller: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerLedger {
    pub balance: u64,
    pub history: Vec<PointsEntry>,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum PointsLedgerError {
    NotAuthorized = 1,
    AdminNotFound = 2,
    AlreadyInitialized = 3,
    InvalidAmount = 4,
}

// Storage keys
const ADMIN: Symbol = symbol_short!("ADMIN");
const AUTH: Symbol = symbol_short!("AUTH");
const LEDGER: Symbol = symbol_short!("LEDGER");

#[contract]
pub struct PointsLedgerContract;

#[contractimpl]
impl PointsLedgerContract {
    /// Initialize the contract with an admin address
    pub fn initialize(env: Env, admin: Address) -> Result<(), PointsLedgerError> {
        admin.require_auth();
        if env.storage().instance().has(&ADMIN) {
            return Err(PointsLedgerError::AlreadyInitialized);
        }
        env.storage().instance().set(&ADMIN, &admin);
        Ok(())
    }

    /// Authorize a contract or caller address to award points. Admin only.
    pub fn authorize_caller(
        env: Env,
        admin: Address,
        caller: Address,
    ) -> Result<(), PointsLedgerError> {
        Self::require_admin(&env, &admin)?;
        let key = (AUTH, caller);
        env.storage().persistent().set(&key, &true);
        Ok(())
    }

    /// Deauthorize a contract or caller address. Admin only.
    pub fn deauthorize_caller(
        env: Env,
        admin: Address,
        caller: Address,
    ) -> Result<(), PointsLedgerError> {
        Self::require_admin(&env, &admin)?;
        let key = (AUTH, caller);
        env.storage().persistent().remove(&key);
        Ok(())
    }

    /// Check if a contract or address is authorized to award points
    pub fn is_authorized(env: Env, caller: Address) -> bool {
        let stored_admin: Option<Address> = env.storage().instance().get(&ADMIN);
        if let Some(admin) = stored_admin {
            if admin == caller {
                return true;
            }
        }
        let key = (AUTH, caller);
        env.storage().persistent().has(&key)
    }

    /// Award points to a player. Caller must be authorized (admin or authorized contract).
    pub fn earn(
        env: Env,
        caller: Address,
        player: Address,
        amount: u64,
        reason_code: Symbol,
    ) -> Result<(), PointsLedgerError> {
        caller.require_auth();

        if !Self::is_authorized(env.clone(), caller.clone()) {
            return Err(PointsLedgerError::NotAuthorized);
        }

        if amount == 0 {
            return Err(PointsLedgerError::InvalidAmount);
        }

        let key = (LEDGER, player.clone());
        let mut player_ledger: PlayerLedger = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(PlayerLedger {
                balance: 0,
                history: Vec::new(&env),
            });

        player_ledger.balance = player_ledger.balance.checked_add(amount).unwrap();

        let entry = PointsEntry {
            player: player.clone(),
            amount,
            reason_code: reason_code.clone(),
            timestamp: env.ledger().timestamp(),
            caller: caller.clone(),
        };

        player_ledger.history.push_back(entry);
        env.storage().persistent().set(&key, &player_ledger);

        // Emit earn event
        env.events().publish(
            (symbol_short!("earn"), player),
            (amount, reason_code, caller),
        );

        Ok(())
    }

    /// Get current total points balance for a player
    pub fn get_balance(env: Env, player: Address) -> u64 {
        let key = (LEDGER, player);
        let player_ledger: Option<PlayerLedger> = env.storage().persistent().get(&key);
        match player_ledger {
            Some(l) => l.balance,
            None => 0,
        }
    }

    /// Get complete points history for a player
    pub fn get_history(env: Env, player: Address) -> Vec<PointsEntry> {
        let key = (LEDGER, player);
        let player_ledger: Option<PlayerLedger> = env.storage().persistent().get(&key);
        match player_ledger {
            Some(l) => l.history,
            None => Vec::new(&env),
        }
    }

    /// Transfer contract admin role. Admin only.
    pub fn transfer_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), PointsLedgerError> {
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
    fn require_admin(env: &Env, admin: &Address) -> Result<(), PointsLedgerError> {
        admin.require_auth();

        let stored_admin: Option<Address> = env.storage().instance().get(&ADMIN);

        match stored_admin {
            Some(stored) if stored == *admin => Ok(()),
            Some(_) => Err(PointsLedgerError::NotAuthorized),
            None => Err(PointsLedgerError::AdminNotFound),
        }
    }
}
