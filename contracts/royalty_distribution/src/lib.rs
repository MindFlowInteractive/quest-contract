#![no_std]

//! # Royalty Distribution Contract
//!
//! A Soroban smart contract that automatically splits royalty revenue among
//! multiple creators based on percentage allocations. The contract supports:
//!
//! * Adding/removing recipients with percentage shares
//! * Automatic proportional distribution of royalties
//! * Withdrawal system for recipients
//! * Distribution history tracking
//! * Rounding fairness to ensure no dust is lost
//! * Emergency withdrawal for admin
//! * Comprehensive recipient queries

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, Vec,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RoyaltyError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAuthorized = 3,
    InvalidPercentage = 4,
    ZeroAmount = 5,
    RecipientNotFound = 6,
    DuplicateRecipient = 7,
    PercentageExceeds100 = 8,
    NoRecipients = 9,
    InsufficientBalance = 10,
    NothingToWithdraw = 11,
    InvalidRecipient = 12,
    MaxRecipientsExceeded = 13,
    EmergencyNotActive = 14,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of recipients supported
pub const MAX_RECIPIENTS: u32 = 50;

/// 100% in basis points (1 bp = 0.01%)
pub const BPS_DENOMINATOR: u32 = 10_000;

/// Maximum history entries stored
pub const MAX_HISTORY_ENTRIES: u32 = 1_000;

/// Emergency withdrawal timelock in seconds (7 days)
pub const EMERGENCY_TIMELOCK: u64 = 604_800;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Recipient information with share percentage
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Recipient {
    pub address: Address,
    pub basis_points: u32,
}

/// Distribution history entry
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DistributionEntry {
    pub timestamp: u64,
    pub total_amount: i128,
    pub recipient_count: u32,
    pub distributor: Address,
}

/// Individual recipient withdrawal record
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalRecord {
    pub timestamp: u64,
    pub amount: i128,
    pub recipient: Address,
}

/// Contract configuration
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
    pub token: Address,
    pub total_distributed: i128,
    pub distribution_count: u32,
    pub emergency_withdrawal_requested: bool,
    pub emergency_withdrawal_requested_at: u64,
}

// ---------------------------------------------------------------------------
// Storage Key
// ---------------------------------------------------------------------------

#[contracttype]
pub enum DataKey {
    /// Contract configuration
    Config,
    /// List of recipients
    Recipients,
    /// Recipient index mapping (address -> index)
    RecipientIndex(Address),
    /// Pending balance for each recipient
    PendingBalance(Address),
    /// Total withdrawn by each recipient
    TotalWithdrawn(Address),
    /// Distribution history (index -> entry)
    DistributionHistory(u32),
    /// Distribution history length
    DistributionHistoryLen,
    /// Withdrawal history (index -> record)
    WithdrawalHistory(u32),
    /// Withdrawal history length
    WithdrawalHistoryLen,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct RoyaltyDistributionContract;

#[contractimpl]
impl RoyaltyDistributionContract {
    // =======================================================================
    // Initialization
    // =======================================================================

    /// Initialize the royalty distribution contract
    pub fn initialize(env: Env, admin: Address, token: Address) -> Result<(), RoyaltyError> {
        if env.storage().instance().has(&DataKey::Config) {
            return Err(RoyaltyError::AlreadyInitialized);
        }
        admin.require_auth();

        let config = Config {
            admin: admin.clone(),
            token: token.clone(),
            total_distributed: 0,
            distribution_count: 0,
            emergency_withdrawal_requested: false,
            emergency_withdrawal_requested_at: 0,
        };

        env.storage().instance().set(&DataKey::Config, &config);
        env.storage().instance().set(&DataKey::Recipients, &Vec::<Recipient>::new(&env));
        env.storage()
            .instance()
            .set(&DataKey::DistributionHistoryLen, &0u32);
        env.storage()
            .instance()
            .set(&DataKey::WithdrawalHistoryLen, &0u32);

        env.events()
            .publish((symbol_short!("rd_init"),), (admin, token));
        Ok(())
    }

    // =======================================================================
    // Recipient Management
    // =======================================================================

    /// Add a new recipient with their percentage share
    pub fn add_recipient(
        env: Env,
        admin: Address,
        recipient_address: Address,
        basis_points: u32,
    ) -> Result<(), RoyaltyError> {
        Self::require_admin(&env, &admin)?;
        
        if basis_points == 0 || basis_points > BPS_DENOMINATOR {
            return Err(RoyaltyError::InvalidPercentage);
        }

        let mut recipients: Vec<Recipient> = env
            .storage()
            .instance()
            .get(&DataKey::Recipients)
            .unwrap_or(Vec::new(&env));

        if recipients.len() >= MAX_RECIPIENTS {
            return Err(RoyaltyError::MaxRecipientsExceeded);
        }

        // Check for duplicate recipient
        if env.storage().instance().has(&DataKey::RecipientIndex(recipient_address.clone())) {
            return Err(RoyaltyError::DuplicateRecipient);
        }

        // Calculate new total percentage
        let mut total_bps: u32 = 0;
        let mut i = 0;
        while i < recipients.len() {
            total_bps += recipients.get(i).unwrap().basis_points;
            i += 1;
        }

        if total_bps.saturating_add(basis_points) > BPS_DENOMINATOR {
            return Err(RoyaltyError::PercentageExceeds100);
        }

        // Add recipient
        let recipient = Recipient {
            address: recipient_address.clone(),
            basis_points,
        };
        recipients.push_back(recipient.clone());

        // Update storage
        env.storage()
            .instance()
            .set(&DataKey::Recipients, &recipients);
        env.storage().instance().set(
            &DataKey::RecipientIndex(recipient_address.clone()),
            &recipients.len() - 1,
        );
        env.storage()
            .instance()
            .set(&DataKey::PendingBalance(recipient_address.clone()), &0i128);
        env.storage()
            .instance()
            .set(&DataKey::TotalWithdrawn(recipient_address.clone()), &0i128);

        env.events()
            .publish((symbol_short!("rd_add"),), (recipient_address, basis_points));
        Ok(())
    }

    /// Remove a recipient
    pub fn remove_recipient(
        env: Env,
        admin: Address,
        recipient_address: Address,
    ) -> Result<(), RoyaltyError> {
        Self::require_admin(&env, &admin)?;

        let index_opt: Option<u32> = env
            .storage()
            .instance()
            .get(&DataKey::RecipientIndex(recipient_address.clone()));

        let index = index_opt.ok_or(RoyaltyError::RecipientNotFound)?;

        let mut recipients: Vec<Recipient> = env
            .storage()
            .instance()
            .get(&DataKey::Recipients)
            .unwrap();

        // Check if recipient has pending balance
        let pending: i128 = env
            .storage()
            .instance()
            .get(&DataKey::PendingBalance(recipient_address.clone()))
            .unwrap_or(0);

        if pending > 0 {
            // Auto-withdraw pending balance before removal
            Self::withdraw_to_recipient(&env, &recipient_address, pending)?;
        }

        // Remove recipient from vector
        recipients.remove(index);

        // Update indices for all recipients after the removed one
        let mut i = index;
        while i < recipients.len() {
            let r = recipients.get(i).unwrap();
            env.storage()
                .instance()
                .set(&DataKey::RecipientIndex(r.address.clone()), &i);
            i += 1;
        }

        // Update storage
        env.storage()
            .instance()
            .set(&DataKey::Recipients, &recipients);
        env.storage()
            .instance()
            .remove(&DataKey::RecipientIndex(recipient_address.clone()));
        env.storage()
            .instance()
            .remove(&DataKey::PendingBalance(recipient_address.clone()));

        env.events()
            .publish((symbol_short!("rd_rem"),), recipient_address);
        Ok(())
    }

    /// Update recipient's percentage share
    pub fn update_recipient_share(
        env: Env,
        admin: Address,
        recipient_address: Address,
        new_basis_points: u32,
    ) -> Result<(), RoyaltyError> {
        Self::require_admin(&env, &admin)?;

        if new_basis_points == 0 || new_basis_points > BPS_DENOMINATOR {
            return Err(RoyaltyError::InvalidPercentage);
        }

        let index_opt: Option<u32> = env
            .storage()
            .instance()
            .get(&DataKey::RecipientIndex(recipient_address.clone()));

        let index = index_opt.ok_or(RoyaltyError::RecipientNotFound)?;

        let mut recipients: Vec<Recipient> = env
            .storage()
            .instance()
            .get(&DataKey::Recipients)
            .unwrap();

        let old_bps = recipients.get(index).unwrap().basis_points;

        // Calculate new total percentage
        let mut total_bps: u32 = 0;
        let mut i = 0;
        while i < recipients.len() {
            if i != index {
                total_bps += recipients.get(i).unwrap().basis_points;
            }
            i += 1;
        }

        if total_bps.saturating_add(new_basis_points) > BPS_DENOMINATOR {
            return Err(RoyaltyError::PercentageExceeds100);
        }

        // Update recipient
        recipients.get(index).unwrap().basis_points = new_basis_points;

        env.storage()
            .instance()
            .set(&DataKey::Recipients, &recipients);

        env.events().publish(
            (symbol_short!("rd_upd"),),
            (recipient_address, old_bps, new_basis_points),
        );
        Ok(())
    }

    // =======================================================================
    // Distribution Logic
    // =======================================================================

    /// Distribute royalties among recipients proportionally
    pub fn distribute(
        env: Env,
        from: Address,
        amount: i128,
    ) -> Result<(), RoyaltyError> {
        if amount <= 0 {
            return Err(RoyaltyError::ZeroAmount);
        }
        from.require_auth();

        let recipients: Vec<Recipient> = env
            .storage()
            .instance()
            .get(&DataKey::Recipients)
            .unwrap();

        if recipients.is_empty() {
            return Err(RoyaltyError::NoRecipients);
        }

        let config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .expect("config not set");

        // Transfer tokens to contract
        token::Client::new(&env, &config.token).transfer(
            &from,
            &env.current_contract_address(),
            &amount,
        );

        // Calculate total basis points
        let mut total_bps: u32 = 0;
        let mut i = 0;
        while i < recipients.len() {
            total_bps += recipients.get(i).unwrap().basis_points;
            i += 1;
        }

        // Distribute with rounding fairness
        let mut distributed: i128 = 0;
        let mut remainder: i128 = amount;

        let mut i = 0;
        while i < recipients.len() {
            let recipient = recipients.get(i).unwrap();
            
            // Calculate share with rounding
            let share = if i == recipients.len() - 1 {
                // Last recipient gets remainder to ensure no dust is lost
                remainder
            } else {
                let exact_share = (amount * recipient.basis_points as i128) / total_bps as i128;
                remainder -= exact_share;
                exact_share
            };

            if share > 0 {
                let mut pending: i128 = env
                    .storage()
                    .instance()
                    .get(&DataKey::PendingBalance(recipient.address.clone()))
                    .unwrap_or(0);
                pending += share;
                env.storage()
                    .instance()
                    .set(&DataKey::PendingBalance(recipient.address.clone()), &pending);
            }

            distributed += share;
            i += 1;
        }

        // Update config
        let mut config = config;
        config.total_distributed += amount;
        config.distribution_count += 1;
        env.storage().instance().set(&DataKey::Config, &config);

        // Record history
        Self::record_distribution(&env, amount, recipients.len() as u32, from.clone());

        env.events()
            .publish((symbol_short!("rd_dist"),), (amount, recipients.len()));
        Ok(())
    }

    // =======================================================================
    // Withdrawal System
    // =======================================================================

    /// Withdraw available balance for a recipient
    pub fn withdraw(env: Env, recipient: Address) -> Result<i128, RoyaltyError> {
        recipient.require_auth();

        let pending: i128 = env
            .storage()
            .instance()
            .get(&DataKey::PendingBalance(recipient.clone()))
            .unwrap_or(0);

        if pending <= 0 {
            return Err(RoyaltyError::NothingToWithdraw);
        }

        Self::withdraw_to_recipient(&env, &recipient, pending)?;

        env.events()
            .publish((symbol_short!("rd_wd"),), (recipient.clone(), pending));
        Ok(pending)
    }

    /// Withdraw a specific amount
    pub fn withdraw_amount(
        env: Env,
        recipient: Address,
        amount: i128,
    ) -> Result<(), RoyaltyError> {
        if amount <= 0 {
            return Err(RoyaltyError::ZeroAmount);
        }
        recipient.require_auth();

        let pending: i128 = env
            .storage()
            .instance()
            .get(&DataKey::PendingBalance(recipient.clone()))
            .unwrap_or(0);

        if amount > pending {
            return Err(RoyaltyError::InsufficientBalance);
        }

        Self::withdraw_to_recipient(&env, &recipient, amount)?;

        env.events()
            .publish((symbol_short!("rd_wda"),), (recipient.clone(), amount));
        Ok(())
    }

    fn withdraw_to_recipient(env: &Env, recipient: &Address, amount: i128) -> Result<(), RoyaltyError> {
        let config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .expect("config not set");

        // Transfer tokens to recipient
        token::Client::new(env, &config.token).transfer(
            &env.current_contract_address(),
            recipient,
            &amount,
        );

        // Update pending balance
        let mut pending: i128 = env
            .storage()
            .instance()
            .get(&DataKey::PendingBalance(recipient.clone()))
            .unwrap_or(0);
        pending -= amount;
        env.storage()
            .instance()
            .set(&DataKey::PendingBalance(recipient.clone()), &pending);

        // Update total withdrawn
        let mut total_withdrawn: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalWithdrawn(recipient.clone()))
            .unwrap_or(0);
        total_withdrawn += amount;
        env.storage()
            .instance()
            .set(&DataKey::TotalWithdrawn(recipient.clone()), &total_withdrawn);

        // Record withdrawal history
        Self::record_withdrawal(env, amount, recipient.clone());

        Ok(())
    }

    // =======================================================================
    // Emergency Withdrawal
    // =======================================================================

    /// Request emergency withdrawal (starts timelock)
    pub fn request_emergency_withdrawal(env: Env, admin: Address) -> Result<(), RoyaltyError> {
        Self::require_admin(&env, &admin)?;

        let mut config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .expect("config not set");

        if config.emergency_withdrawal_requested {
            return Err(RoyaltyError::EmergencyNotActive);
        }

        config.emergency_withdrawal_requested = true;
        config.emergency_withdrawal_requested_at = env.ledger().timestamp();

        env.storage().instance().set(&DataKey::Config, &config);

        env.events()
            .publish((symbol_short!("rd_ew_req"),), admin);
        Ok(())
    }

    /// Execute emergency withdrawal after timelock
    pub fn execute_emergency_withdrawal(
        env: Env,
        admin: Address,
        to: Address,
    ) -> Result<i128, RoyaltyError> {
        Self::require_admin(&env, &admin)?;

        let mut config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .expect("config not set");

        if !config.emergency_withdrawal_requested {
            return Err(RoyaltyError::EmergencyNotActive);
        }

        let now = env.ledger().timestamp();
        if now < config.emergency_withdrawal_requested_at + EMERGENCY_TIMELOCK {
            return Err(RoyaltyError::EmergencyNotActive);
        }

        let token_client = token::Client::new(&env, &config.token);
        let balance = token_client.balance(&env.current_contract_address());

        if balance <= 0 {
            return Err(RoyaltyError::NothingToWithdraw);
        }

        // Transfer all tokens
        token_client.transfer(&env.current_contract_address(), &to, &balance);

        // Reset emergency state
        config.emergency_withdrawal_requested = false;
        config.emergency_withdrawal_requested_at = 0;
        env.storage().instance().set(&DataKey::Config, &config);

        env.events()
            .publish((symbol_short!("rd_ew_exe"),), (to.clone(), balance));
        Ok(balance)
    }

    /// Cancel emergency withdrawal request
    pub fn cancel_emergency_withdrawal(env: Env, admin: Address) -> Result<(), RoyaltyError> {
        Self::require_admin(&env, &admin)?;

        let mut config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .expect("config not set");

        config.emergency_withdrawal_requested = false;
        config.emergency_withdrawal_requested_at = 0;

        env.storage().instance().set(&DataKey::Config, &config);

        env.events()
            .publish((symbol_short!("rd_ew_can"),), admin);
        Ok(())
    }

    // =======================================================================
    // Query Functions
    // =======================================================================

    /// Get all recipients
    pub fn get_recipients(env: Env) -> Vec<Recipient> {
        env.storage()
            .instance()
            .get(&DataKey::Recipients)
            .unwrap_or(Vec::new(&env))
    }

    /// Get pending balance for a recipient
    pub fn get_pending_balance(env: Env, recipient: Address) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::PendingBalance(recipient))
            .unwrap_or(0)
    }

    /// Get total withdrawn by a recipient
    pub fn get_total_withdrawn(env: Env, recipient: Address) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalWithdrawn(recipient))
            .unwrap_or(0)
    }

    /// Get contract configuration
    pub fn get_config(env: Env) -> Config {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .expect("config not set")
    }

    /// Get distribution history
    pub fn get_distribution_history(env: Env, offset: u32, limit: u32) -> Vec<DistributionEntry> {
        let history_len: u32 = env
            .storage()
            .instance()
            .get(&DataKey::DistributionHistoryLen)
            .unwrap_or(0);

        let mut result = Vec::new(&env);
        let mut i = offset;
        let end = std::cmp::min(offset + limit, history_len);

        while i < end {
            if let Some(entry) = env
                .storage()
                .instance()
                .get(&DataKey::DistributionHistory(i))
            {
                result.push_back(entry);
            }
            i += 1;
        }

        result
    }

    /// Get withdrawal history
    pub fn get_withdrawal_history(env: Env, offset: u32, limit: u32) -> Vec<WithdrawalRecord> {
        let history_len: u32 = env
            .storage()
            .instance()
            .get(&DataKey::WithdrawalHistoryLen)
            .unwrap_or(0);

        let mut result = Vec::new(&env);
        let mut i = offset;
        let end = std::cmp::min(offset + limit, history_len);

        while i < end {
            if let Some(record) = env
                .storage()
                .instance()
                .get(&DataKey::WithdrawalHistory(i))
            {
                result.push_back(record);
            }
            i += 1;
        }

        result
    }

    /// Get total contract balance
    pub fn get_contract_balance(env: Env) -> i128 {
        let config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .expect("config not set");
        token::Client::new(&env, &config.token).balance(&env.current_contract_address())
    }

    // =======================================================================
    // Helper Functions
    // =======================================================================

    fn require_admin(env: &Env, admin: &Address) -> Result<(), RoyaltyError> {
        let config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(RoyaltyError::NotInitialized)?;
        
        if config.admin != *admin {
            return Err(RoyaltyError::NotAuthorized);
        }
        
        admin.require_auth();
        Ok(())
    }

    fn record_distribution(env: &Env, amount: i128, recipient_count: u32, distributor: Address) {
        let history_len: u32 = env
            .storage()
            .instance()
            .get(&DataKey::DistributionHistoryLen)
            .unwrap_or(0);

        if history_len >= MAX_HISTORY_ENTRIES {
            // Remove oldest entry
            env.storage()
                .instance()
                .remove(&DataKey::DistributionHistory(0));
        }

        let entry = DistributionEntry {
            timestamp: env.ledger().timestamp(),
            total_amount: amount,
            recipient_count,
            distributor,
        };

        let new_len = if history_len >= MAX_HISTORY_ENTRIES {
            history_len
        } else {
            history_len + 1
        };

        env.storage()
            .instance()
            .set(&DataKey::DistributionHistory(history_len), &entry);
        env.storage()
            .instance()
            .set(&DataKey::DistributionHistoryLen, &new_len);
    }

    fn record_withdrawal(env: &Env, amount: i128, recipient: Address) {
        let history_len: u32 = env
            .storage()
            .instance()
            .get(&DataKey::WithdrawalHistoryLen)
            .unwrap_or(0);

        if history_len >= MAX_HISTORY_ENTRIES {
            // Remove oldest entry
            env.storage()
                .instance()
                .remove(&DataKey::WithdrawalHistory(0));
        }

        let record = WithdrawalRecord {
            timestamp: env.ledger().timestamp(),
            amount,
            recipient,
        };

        let new_len = if history_len >= MAX_HISTORY_ENTRIES {
            history_len
        } else {
            history_len + 1
        };

        env.storage()
            .instance()
            .set(&DataKey::WithdrawalHistory(history_len), &record);
        env.storage()
            .instance()
            .set(&DataKey::WithdrawalHistoryLen, &new_len);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::Address;

    #[test]
    fn test_initialize() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone())
            .unwrap();

        let config = RoyaltyDistributionContract::get_config(&env);
        assert_eq!(config.admin, admin);
        assert_eq!(config.token, token);
    }

    #[test]
    fn test_add_recipient() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let recipient = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient.clone(), 5000)
            .unwrap();

        let recipients = RoyaltyDistributionContract::get_recipients(&env);
        assert_eq!(recipients.len(), 1);
        assert_eq!(recipients.get(0).unwrap().address, recipient);
        assert_eq!(recipients.get(0).unwrap().basis_points, 5000);
    }

    #[test]
    fn test_add_multiple_recipients() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let recipient1 = Address::generate(&env);
        let recipient2 = Address::generate(&env);
        let recipient3 = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient1.clone(), 3000)
            .unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient2.clone(), 3000)
            .unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient3.clone(), 4000)
            .unwrap();

        let recipients = RoyaltyDistributionContract::get_recipients(&env);
        assert_eq!(recipients.len(), 3);
    }

    #[test]
    fn test_percentage_exceeds_100() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let recipient = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient.clone(), 10001)
            .unwrap_err();
    }

    #[test]
    fn test_duplicate_recipient() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let recipient = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient.clone(), 5000)
            .unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient.clone(), 2000)
            .unwrap_err();
    }

    #[test]
    fn test_remove_recipient() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let recipient = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient.clone(), 5000)
            .unwrap();
        RoyaltyDistributionContract::remove_recipient(&env, admin.clone(), recipient.clone())
            .unwrap();

        let recipients = RoyaltyDistributionContract::get_recipients(&env);
        assert_eq!(recipients.len(), 0);
    }

    #[test]
    fn test_update_recipient_share() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let recipient = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient.clone(), 5000)
            .unwrap();
        RoyaltyDistributionContract::update_recipient_share(&env, admin.clone(), recipient.clone(), 7000)
            .unwrap();

        let recipients = RoyaltyDistributionContract::get_recipients(&env);
        assert_eq!(recipients.get(0).unwrap().basis_points, 7000);
    }

    #[test]
    fn test_distribute() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let recipient1 = Address::generate(&env);
        let recipient2 = Address::generate(&env);
        let distributor = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient1.clone(), 5000)
            .unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient2.clone(), 5000)
            .unwrap();

        // Mock token transfer
        env.register_contract(&token, &token);
        
        RoyaltyDistributionContract::distribute(&env, distributor.clone(), 1000).unwrap();

        let pending1 = RoyaltyDistributionContract::get_pending_balance(&env, recipient1.clone());
        let pending2 = RoyaltyDistributionContract::get_pending_balance(&env, recipient2.clone());
        
        assert_eq!(pending1, 500);
        assert_eq!(pending2, 500);
    }

    #[test]
    fn test_distribute_rounding_fairness() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let recipient1 = Address::generate(&env);
        let recipient2 = Address::generate(&env);
        let recipient3 = Address::generate(&env);
        let distributor = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient1.clone(), 3333)
            .unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient2.clone(), 3333)
            .unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient3.clone(), 3334)
            .unwrap();

        env.register_contract(&token, &token);
        
        RoyaltyDistributionContract::distribute(&env, distributor.clone(), 100).unwrap();

        let pending1 = RoyaltyDistributionContract::get_pending_balance(&env, recipient1.clone());
        let pending2 = RoyaltyDistributionContract::get_pending_balance(&env, recipient2.clone());
        let pending3 = RoyaltyDistributionContract::get_pending_balance(&env, recipient3.clone());
        
        // Total should equal 100 (no dust lost)
        let total = pending1 + pending2 + pending3;
        assert_eq!(total, 100);
    }

    #[test]
    fn test_withdraw() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let recipient = Address::generate(&env);
        let distributor = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient.clone(), 10000)
            .unwrap();

        env.register_contract(&token, &token);
        
        RoyaltyDistributionContract::distribute(&env, distributor.clone(), 1000).unwrap();
        
        let withdrawn = RoyaltyDistributionContract::withdraw(&env, recipient.clone()).unwrap();
        assert_eq!(withdrawn, 1000);

        let pending = RoyaltyDistributionContract::get_pending_balance(&env, recipient.clone());
        assert_eq!(pending, 0);

        let total_withdrawn = RoyaltyDistributionContract::get_total_withdrawn(&env, recipient.clone());
        assert_eq!(total_withdrawn, 1000);
    }

    #[test]
    fn test_withdraw_amount() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let recipient = Address::generate(&env);
        let distributor = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient.clone(), 10000)
            .unwrap();

        env.register_contract(&token, &token);
        
        RoyaltyDistributionContract::distribute(&env, distributor.clone(), 1000).unwrap();
        
        RoyaltyDistributionContract::withdraw_amount(&env, recipient.clone(), 500).unwrap();

        let pending = RoyaltyDistributionContract::get_pending_balance(&env, recipient.clone());
        assert_eq!(pending, 500);
    }

    #[test]
    fn test_emergency_withdrawal() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let recipient = Address::generate(&env);
        let distributor = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient.clone(), 10000)
            .unwrap();

        env.register_contract(&token, &token);
        
        RoyaltyDistributionContract::distribute(&env, distributor.clone(), 1000).unwrap();
        
        RoyaltyDistributionContract::request_emergency_withdrawal(&env, admin.clone()).unwrap();

        let config = RoyaltyDistributionContract::get_config(&env);
        assert!(config.emergency_withdrawal_requested);

        // Advance time past timelock
        env.ledger().set(config.emergency_withdrawal_requested_at + EMERGENCY_TIMELOCK + 1);

        let to = Address::generate(&env);
        RoyaltyDistributionContract::execute_emergency_withdrawal(&env, admin.clone(), to.clone())
            .unwrap();
    }

    #[test]
    fn test_distribution_history() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let recipient = Address::generate(&env);
        let distributor = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient.clone(), 10000)
            .unwrap();

        env.register_contract(&token, &token);
        
        RoyaltyDistributionContract::distribute(&env, distributor.clone(), 1000).unwrap();
        RoyaltyDistributionContract::distribute(&env, distributor.clone(), 500).unwrap();

        let history = RoyaltyDistributionContract::get_distribution_history(&env, 0, 10);
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_withdrawal_history() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let recipient = Address::generate(&env);
        let distributor = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient.clone(), 10000)
            .unwrap();

        env.register_contract(&token, &token);
        
        RoyaltyDistributionContract::distribute(&env, distributor.clone(), 1000).unwrap();
        RoyaltyDistributionContract::withdraw(&env, recipient.clone()).unwrap();

        let history = RoyaltyDistributionContract::get_withdrawal_history(&env, 0, 10);
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn test_not_authorized() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let other = Address::generate(&env);
        let recipient = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();
        
        RoyaltyDistributionContract::add_recipient(&env, other.clone(), recipient.clone(), 5000)
            .unwrap_err();
    }

    #[test]
    fn test_no_recipients_distribution() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let distributor = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();

        env.register_contract(&token, &token);
        
        RoyaltyDistributionContract::distribute(&env, distributor.clone(), 1000).unwrap_err();
    }

    #[test]
    fn test_zero_amount_distribution() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let recipient = Address::generate(&env);
        let distributor = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient.clone(), 5000)
            .unwrap();

        RoyaltyDistributionContract::distribute(&env, distributor.clone(), 0).unwrap_err();
    }

    #[test]
    fn test_remove_recipient_with_pending_balance() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let recipient = Address::generate(&env);
        let distributor = Address::generate(&env);

        RoyaltyDistributionContract::initialize(&env, admin.clone(), token.clone()).unwrap();
        RoyaltyDistributionContract::add_recipient(&env, admin.clone(), recipient.clone(), 10000)
            .unwrap();

        env.register_contract(&token, &token);
        
        RoyaltyDistributionContract::distribute(&env, distributor.clone(), 1000).unwrap();
        
        // Remove should auto-withdraw
        RoyaltyDistributionContract::remove_recipient(&env, admin.clone(), recipient.clone())
            .unwrap();

        let total_withdrawn = RoyaltyDistributionContract::get_total_withdrawn(&env, recipient.clone());
        assert_eq!(total_withdrawn, 1000);
    }
}
