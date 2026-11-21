#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contractmeta, contracttype, symbol_short,
    Address, Env, Symbol, Vec,
};

#[cfg(test)]
mod test;

// Contract metadata
contractmeta!(
    key = "Description",
    val = "Achievement Points Ledger contract with authorized earning, FIFO point spending, and expiration"
);

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PointsEntry {
    pub player: Address,
    pub amount: u64,
    pub remaining: u64,
    pub reason_code: Symbol,
    pub earned_at: u64,
    pub expires_at: u64,
    pub caller: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerLedger {
    pub total_earned: u64,
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
    InsufficientBalance = 5,
}

// Storage keys
const ADMIN: Symbol = symbol_short!("ADMIN");
const AUTH: Symbol = symbol_short!("AUTH");
const LEDGER: Symbol = symbol_short!("LEDGER");
const DEF_EXP: Symbol = symbol_short!("DEF_EXP");

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

    /// Set default point expiration duration in seconds (0 for no expiration). Admin only.
    pub fn set_default_expiry(
        env: Env,
        admin: Address,
        duration: u64,
    ) -> Result<(), PointsLedgerError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DEF_EXP, &duration);
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

    /// Award points to a player with custom validity duration (0 uses default).
    /// Caller must be authorized (admin or authorized contract).
    pub fn earn_with_expiry(
        env: Env,
        caller: Address,
        player: Address,
        amount: u64,
        reason_code: Symbol,
        validity_duration: u64,
    ) -> Result<(), PointsLedgerError> {
        caller.require_auth();

        if !Self::is_authorized(env.clone(), caller.clone()) {
            return Err(PointsLedgerError::NotAuthorized);
        }

        if amount == 0 {
            return Err(PointsLedgerError::InvalidAmount);
        }

        let now = env.ledger().timestamp();
        let default_expiry: u64 = env.storage().instance().get(&DEF_EXP).unwrap_or(0);
        let duration = if validity_duration > 0 {
            validity_duration
        } else {
            default_expiry
        };

        let expires_at = if duration > 0 {
            now.saturating_add(duration)
        } else {
            0
        };

        let key = (LEDGER, player.clone());
        let mut player_ledger: PlayerLedger = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(PlayerLedger {
                total_earned: 0,
                history: Vec::new(&env),
            });

        player_ledger.total_earned = player_ledger.total_earned.checked_add(amount).unwrap();

        let entry = PointsEntry {
            player: player.clone(),
            amount,
            remaining: amount,
            reason_code: reason_code.clone(),
            earned_at: now,
            expires_at,
            caller: caller.clone(),
        };

        player_ledger.history.push_back(entry);
        env.storage().persistent().set(&key, &player_ledger);

        // Emit earn event: (earn, player) -> (amount, reason_code, caller, expires_at)
        env.events().publish(
            (symbol_short!("earn"), player),
            (amount, reason_code, caller, expires_at),
        );

        Ok(())
    }

    /// Standard earn function using default expiration
    pub fn earn(
        env: Env,
        caller: Address,
        player: Address,
        amount: u64,
        reason_code: Symbol,
    ) -> Result<(), PointsLedgerError> {
        Self::earn_with_expiry(env, caller, player, amount, reason_code, 0)
    }

    /// Spend points at in-game shop, strictly enforcing FIFO consumption of oldest unexpired points.
    /// Rejects if spending exceeds available unexpired points balance.
    pub fn spend(
        env: Env,
        player: Address,
        amount_to_spend: u64,
        reason_code: Symbol,
    ) -> Result<u64, PointsLedgerError> {
        player.require_auth();

        if amount_to_spend == 0 {
            return Err(PointsLedgerError::InvalidAmount);
        }

        let key = (LEDGER, player.clone());
        let mut player_ledger: PlayerLedger = match env.storage().persistent().get(&key) {
            Some(l) => l,
            None => return Err(PointsLedgerError::InsufficientBalance),
        };

        let now = env.ledger().timestamp();

        // Calculate available unexpired balance
        let mut available: u64 = 0;
        for i in 0..player_ledger.history.len() {
            let entry = player_ledger.history.get(i).unwrap();
            if entry.remaining > 0 && (entry.expires_at == 0 || now <= entry.expires_at) {
                available = available.saturating_add(entry.remaining);
            }
        }

        if available < amount_to_spend {
            return Err(PointsLedgerError::InsufficientBalance);
        }

        // Enforce FIFO consumption: consume oldest unexpired points first
        let mut remaining_to_spend = amount_to_spend;
        let mut updated_history = Vec::new(&env);

        for i in 0..player_ledger.history.len() {
            let mut entry = player_ledger.history.get(i).unwrap();
            let is_unexpired = entry.expires_at == 0 || now <= entry.expires_at;

            if remaining_to_spend > 0 && entry.remaining > 0 && is_unexpired {
                let to_consume = remaining_to_spend.min(entry.remaining);
                entry.remaining -= to_consume;
                remaining_to_spend -= to_consume;
            }
            updated_history.push_back(entry);
        }

        player_ledger.history = updated_history;
        env.storage().persistent().set(&key, &player_ledger);

        let new_balance = available - amount_to_spend;

        // Emit spend event: (spend, player) -> (amount, reason_code, new_balance)
        env.events().publish(
            (symbol_short!("spend"), player),
            (amount_to_spend, reason_code, new_balance),
        );

        Ok(new_balance)
    }

    /// Get current spendable (unexpired) points balance for a player
    pub fn get_balance(env: Env, player: Address) -> u64 {
        let key = (LEDGER, player);
        let player_ledger: Option<PlayerLedger> = env.storage().persistent().get(&key);
        let now = env.ledger().timestamp();

        match player_ledger {
            Some(l) => {
                let mut sum: u64 = 0;
                for i in 0..l.history.len() {
                    let entry = l.history.get(i).unwrap();
                    if entry.remaining > 0 && (entry.expires_at == 0 || now <= entry.expires_at) {
                        sum = sum.saturating_add(entry.remaining);
                    }
                }
                sum
            }
            None => 0,
        }
    }

    /// Get total cumulative points earned over all time
    pub fn get_total_earned(env: Env, player: Address) -> u64 {
        let key = (LEDGER, player);
        let player_ledger: Option<PlayerLedger> = env.storage().persistent().get(&key);
        match player_ledger {
            Some(l) => l.total_earned,
            None => 0,
        }
    }

    /// Get complete points ledger history for a player
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
