#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror, token, Address, Env, Vec,
    String,
};

// ──────────────────────────────────────────────────────────
// ERRORS
// ──────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum TippingError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    InvalidAmount = 3,
    InvalidRecipient = 4,
    UnauthorizedWithdrawal = 5,
    TipLimitExceeded = 6,
    CooldownActive = 7,
    InsufficientBalance = 8,
    InvalidBatchSize = 9,
}

// ──────────────────────────────────────────────────────────
// DATA STRUCTURES
// ──────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TipRecord {
    pub from: Address,
    pub to: Address,
    pub amount: i128,
    pub message: String,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TipperStats {
    pub total_tipped: i128,
    pub tip_count: u32,
    pub last_tip_time: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipientStats {
    pub total_received: i128,
    pub tip_count: u32,
    pub total_from_unique_tippers: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TippingConfig {
    pub admin: Address,
    pub token: Address,
    pub max_tip_per_transaction: i128,
    pub max_tips_per_day: u32,
    pub cooldown_seconds: u64,
    pub max_batch_size: u32,
    pub is_initialized: bool,
}

#[contracttype]
pub enum DataKey {
    Config,                          // TippingConfig
    TipperBalance(Address),          // Address -> i128 (withdrawable balance)
    TipperStats(Address),            // TipperStats
    RecipientStats(Address),         // RecipientStats
    TipHistory(Address),             // Vec<TipRecord> (tip history for recipient)
    DailyTipCount(Address, u64),     // u32 (tips sent on a given day)
    TopTippers,                      // Vec<(Address, i128)> sorted by amount
    TopRecipients,                   // Vec<(Address, i128)> sorted by amount
}

// ──────────────────────────────────────────────────────────
// CONTRACT
// ──────────────────────────────────────────────────────────

#[contract]
pub struct SocialTippingContract;

#[contractimpl]
impl SocialTippingContract {
    /// Initialize the tipping contract with configuration
    pub fn initialize(
        env: Env,
        admin: Address,
        token: Address,
        max_tip_per_transaction: i128,
        max_tips_per_day: u32,
        cooldown_seconds: u64,
        max_batch_size: u32,
    ) -> Result<(), TippingError> {
        if env.storage().instance().get::<_, TippingConfig>(&DataKey::Config).is_some() {
            return Err(TippingError::AlreadyInitialized);
        }

        let config = TippingConfig {
            admin: admin.clone(),
            token: token.clone(),
            max_tip_per_transaction,
            max_tips_per_day,
            cooldown_seconds,
            max_batch_size,
            is_initialized: true,
        };

        env.storage().instance().set(&DataKey::Config, &config);

        Ok(())
    }

    /// Send a direct tip to a recipient
    pub fn tip(
        env: Env,
        from: Address,
        to: Address,
        amount: i128,
    ) -> Result<(), TippingError> {
        from.require_auth();

        let config = Self::get_config(&env)?;
        
        // Validation
        if amount <= 0 || amount > config.max_tip_per_transaction {
            return Err(TippingError::InvalidAmount);
        }

        if from == to {
            return Err(TippingError::InvalidRecipient);
        }

        // Check cooldown and daily limit
        Self::check_tip_limits(&env, &from, &config)?;

        // Transfer tokens
        let token_client = token::Client::new(&env, &config.token);
        token_client.transfer(&from, &to, &amount);

        // Update stats
        Self::update_tipper_stats(&env, &from, amount);
        Self::update_recipient_stats(&env, &to, amount);
        Self::record_tip(&env, &from, &to, amount, String::from_str(&env, ""));

        Ok(())
    }

    /// Send a tip with a message/note
    pub fn tip_with_message(
        env: Env,
        from: Address,
        to: Address,
        amount: i128,
        message: String,
    ) -> Result<(), TippingError> {
        from.require_auth();

        let config = Self::get_config(&env)?;
        
        // Validation
        if amount <= 0 || amount > config.max_tip_per_transaction {
            return Err(TippingError::InvalidAmount);
        }

        if from == to {
            return Err(TippingError::InvalidRecipient);
        }

        // Check cooldown and daily limit
        Self::check_tip_limits(&env, &from, &config)?;

        // Transfer tokens
        let token_client = token::Client::new(&env, &config.token);
        token_client.transfer(&from, &to, &amount);

        // Update stats
        Self::update_tipper_stats(&env, &from, amount);
        Self::update_recipient_stats(&env, &to, amount);
        Self::record_tip(&env, &from, &to, amount, message);

        Ok(())
    }

    /// Send tips to multiple recipients (batch tipping)
    pub fn batch_tip(
        env: Env,
        from: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
    ) -> Result<(), TippingError> {
        from.require_auth();

        let config = Self::get_config(&env)?;

        // Validation
        if recipients.len() != amounts.len() {
            return Err(TippingError::InvalidBatchSize);
        }

        if recipients.len() as u32 > config.max_batch_size {
            return Err(TippingError::InvalidBatchSize);
        }

        let token_client = token::Client::new(&env, &config.token);

        // Process each tip
        for i in 0..recipients.len() {
            let to = recipients.get(i).unwrap();
            let amount = amounts.get(i).unwrap();

            // Validation
            if amount <= 0 || amount > config.max_tip_per_transaction {
                return Err(TippingError::InvalidAmount);
            }

            if from == to {
                return Err(TippingError::InvalidRecipient);
            }

            // Transfer tokens
            token_client.transfer(&from, &to, &amount);

            // Update stats
            Self::update_recipient_stats(&env, &to, amount);
            Self::record_tip(&env, &from, &to, amount, String::from_str(&env, ""));
        }

        // Update tipper stats once
        let mut total_amount: i128 = 0;
        for i in 0..amounts.len() {
            total_amount += amounts.get(i).unwrap();
        }
        Self::update_tipper_stats_batch(&env, &from, total_amount, amounts.len() as u32);
        
        // Check daily limit after all transfers
        Self::check_tip_limits(&env, &from, &config)?;

        Ok(())
    }

    /// Get tip history for a recipient
    pub fn get_tip_history(
        env: Env,
        recipient: Address,
    ) -> Result<Vec<TipRecord>, TippingError> {
        Self::get_config(&env)?;

        let history = env
            .storage()
            .instance()
            .get::<_, Vec<TipRecord>>(&DataKey::TipHistory(recipient.clone()))
            .unwrap_or(Vec::new(&env));

        Ok(history)
    }

    /// Get tip statistics for a tipper
    pub fn get_tipper_stats(
        env: Env,
        tipper: Address,
    ) -> Result<TipperStats, TippingError> {
        Self::get_config(&env)?;

        let stats = env
            .storage()
            .instance()
            .get::<_, TipperStats>(&DataKey::TipperStats(tipper))
            .unwrap_or(TipperStats {
                total_tipped: 0,
                tip_count: 0,
                last_tip_time: 0,
            });

        Ok(stats)
    }

    /// Get tip statistics for a recipient
    pub fn get_recipient_stats(
        env: Env,
        recipient: Address,
    ) -> Result<RecipientStats, TippingError> {
        Self::get_config(&env)?;

        let stats = env
            .storage()
            .instance()
            .get::<_, RecipientStats>(&DataKey::RecipientStats(recipient))
            .unwrap_or(RecipientStats {
                total_received: 0,
                tip_count: 0,
                total_from_unique_tippers: 0,
            });

        Ok(stats)
    }

    /// Get top tippers leaderboard
    pub fn get_top_tippers(
        env: Env,
        limit: u32,
    ) -> Result<Vec<(Address, i128)>, TippingError> {
        Self::get_config(&env)?;

        let leaderboard = env
            .storage()
            .instance()
            .get::<_, Vec<(Address, i128)>>(&DataKey::TopTippers)
            .unwrap_or(Vec::new(&env));

        let end = core::cmp::min(limit, leaderboard.len());
        let mut result = Vec::new(&env);
        for i in 0..end {
            result.push_back(leaderboard.get(i).unwrap());
        }
        Ok(result)
    }

    /// Get top recipients leaderboard
    pub fn get_top_recipients(
        env: Env,
        limit: u32,
    ) -> Result<Vec<(Address, i128)>, TippingError> {
        Self::get_config(&env)?;

        let leaderboard = env
            .storage()
            .instance()
            .get::<_, Vec<(Address, i128)>>(&DataKey::TopRecipients)
            .unwrap_or(Vec::new(&env));

        let end = core::cmp::min(limit, leaderboard.len());
        let mut result = Vec::new(&env);
        for i in 0..end {
            result.push_back(leaderboard.get(i).unwrap());
        }
        Ok(result)
    }

    /// Get configuration
    pub fn get_config(env: &Env) -> Result<TippingConfig, TippingError> {
        env.storage()
            .instance()
            .get::<_, TippingConfig>(&DataKey::Config)
            .ok_or(TippingError::NotInitialized)
    }

    /// Get the current timestamp (ledger sequence)
    pub fn get_timestamp(env: &Env) -> u64 {
        env.ledger().timestamp()
    }

    // ──────────────────────────────────────────────────────────
    // INTERNAL HELPERS
    // ──────────────────────────────────────────────────────────

    fn check_tip_limits(env: &Env, from: &Address, config: &TippingConfig) -> Result<(), TippingError> {
        let current_time = Self::get_timestamp(env);
        let day_key = current_time / 86400; // Seconds in a day

        let daily_count = env
            .storage()
            .instance()
            .get::<_, u32>(&DataKey::DailyTipCount(from.clone(), day_key))
            .unwrap_or(0);

        if daily_count >= config.max_tips_per_day {
            return Err(TippingError::TipLimitExceeded);
        }

        let stats = env
            .storage()
            .instance()
            .get::<_, TipperStats>(&DataKey::TipperStats(from.clone()))
            .unwrap_or(TipperStats {
                total_tipped: 0,
                tip_count: 0,
                last_tip_time: 0,
            });

        if stats.last_tip_time > 0 {
            let time_since_last_tip = current_time.saturating_sub(stats.last_tip_time);
            if time_since_last_tip < config.cooldown_seconds {
                return Err(TippingError::CooldownActive);
            }
        }

        Ok(())
    }

    fn update_tipper_stats(env: &Env, tipper: &Address, amount: i128) {
        let current_time = Self::get_timestamp(env);
        let day_key = current_time / 86400;

        let mut stats = env
            .storage()
            .instance()
            .get::<_, TipperStats>(&DataKey::TipperStats(tipper.clone()))
            .unwrap_or(TipperStats {
                total_tipped: 0,
                tip_count: 0,
                last_tip_time: 0,
            });

        stats.total_tipped += amount;
        stats.tip_count += 1;
        stats.last_tip_time = current_time;

        env.storage()
            .instance()
            .set(&DataKey::TipperStats(tipper.clone()), &stats);

        // Update daily count
        let mut daily_count = env
            .storage()
            .instance()
            .get::<_, u32>(&DataKey::DailyTipCount(tipper.clone(), day_key))
            .unwrap_or(0);

        daily_count += 1;

        env.storage()
            .instance()
            .set(&DataKey::DailyTipCount(tipper.clone(), day_key), &daily_count);

        // Update top tippers leaderboard
        Self::update_top_tippers(env, tipper, stats.total_tipped);
    }

    fn update_tipper_stats_batch(env: &Env, tipper: &Address, amount: i128, tip_count: u32) {
        let current_time = Self::get_timestamp(env);
        let day_key = current_time / 86400;

        let mut stats = env
            .storage()
            .instance()
            .get::<_, TipperStats>(&DataKey::TipperStats(tipper.clone()))
            .unwrap_or(TipperStats {
                total_tipped: 0,
                tip_count: 0,
                last_tip_time: 0,
            });

        stats.total_tipped += amount;
        stats.tip_count += tip_count;
        stats.last_tip_time = current_time;

        env.storage()
            .instance()
            .set(&DataKey::TipperStats(tipper.clone()), &stats);

        // Update daily count
        let mut daily_count = env
            .storage()
            .instance()
            .get::<_, u32>(&DataKey::DailyTipCount(tipper.clone(), day_key))
            .unwrap_or(0);

        daily_count += tip_count;

        env.storage()
            .instance()
            .set(&DataKey::DailyTipCount(tipper.clone(), day_key), &daily_count);

        // Update top tippers leaderboard
        Self::update_top_tippers(env, tipper, stats.total_tipped);
    }

    fn update_recipient_stats(env: &Env, recipient: &Address, amount: i128) {
        let mut stats = env
            .storage()
            .instance()
            .get::<_, RecipientStats>(&DataKey::RecipientStats(recipient.clone()))
            .unwrap_or(RecipientStats {
                total_received: 0,
                tip_count: 0,
                total_from_unique_tippers: 0,
            });

        stats.total_received += amount;
        stats.tip_count += 1;
        // Note: Tracking unique tippers would require more complex logic

        env.storage()
            .instance()
            .set(&DataKey::RecipientStats(recipient.clone()), &stats);

        // Update top recipients leaderboard
        Self::update_top_recipients(env, recipient, stats.total_received);
    }

    fn record_tip(
        env: &Env,
        from: &Address,
        to: &Address,
        amount: i128,
        message: String,
    ) {
        let current_time = Self::get_timestamp(env);

        let record = TipRecord {
            from: from.clone(),
            to: to.clone(),
            amount,
            message,
            timestamp: current_time,
        };

        let mut history = env
            .storage()
            .instance()
            .get::<_, Vec<TipRecord>>(&DataKey::TipHistory(to.clone()))
            .unwrap_or(Vec::new(env));

        history.push_back(record);

        env.storage()
            .instance()
            .set(&DataKey::TipHistory(to.clone()), &history);
    }

    fn update_top_tippers(env: &Env, tipper: &Address, total_amount: i128) {
        let mut leaderboard = env
            .storage()
            .instance()
            .get::<_, Vec<(Address, i128)>>(&DataKey::TopTippers)
            .unwrap_or(Vec::new(env));

        // Find and update or insert
        let mut found = false;
        for i in 0..leaderboard.len() {
            let (addr, _) = leaderboard.get(i).unwrap();
            if addr == tipper.clone() {
                leaderboard.set(i, (tipper.clone(), total_amount));
                found = true;
                break;
            }
        }

        if !found {
            leaderboard.push_back((tipper.clone(), total_amount));
        }

        // Sort by amount descending (simple bubble sort for small lists)
        for i in 0..leaderboard.len() {
            let limit = leaderboard.len().saturating_sub(i as u32).saturating_sub(1);
            for j in 0..limit {
                let j_plus_1 = j + 1;
                let (_, amount_j) = leaderboard.get(j).unwrap();
                let (_, amount_j1) = leaderboard.get(j_plus_1).unwrap();
                if amount_j < amount_j1 {
                    let temp_j = leaderboard.get(j).unwrap();
                    let temp_j1 = leaderboard.get(j_plus_1).unwrap();
                    leaderboard.set(j, temp_j1);
                    leaderboard.set(j_plus_1, temp_j);
                }
            }
        }

        env.storage()
            .instance()
            .set(&DataKey::TopTippers, &leaderboard);
    }

    fn update_top_recipients(env: &Env, recipient: &Address, total_amount: i128) {
        let mut leaderboard = env
            .storage()
            .instance()
            .get::<_, Vec<(Address, i128)>>(&DataKey::TopRecipients)
            .unwrap_or(Vec::new(env));

        // Find and update or insert
        let mut found = false;
        for i in 0..leaderboard.len() {
            let (addr, _) = leaderboard.get(i).unwrap();
            if addr == recipient.clone() {
                leaderboard.set(i, (recipient.clone(), total_amount));
                found = true;
                break;
            }
        }

        if !found {
            leaderboard.push_back((recipient.clone(), total_amount));
        }

        // Sort by amount descending
        for i in 0..leaderboard.len() {
            let limit = leaderboard.len().saturating_sub(i as u32).saturating_sub(1);
            for j in 0..limit {
                let j_plus_1 = j + 1;
                let (_, amount_j) = leaderboard.get(j).unwrap();
                let (_, amount_j1) = leaderboard.get(j_plus_1).unwrap();
                if amount_j < amount_j1 {
                    let temp_j = leaderboard.get(j).unwrap();
                    let temp_j1 = leaderboard.get(j_plus_1).unwrap();
                    leaderboard.set(j, temp_j1);
                    leaderboard.set(j_plus_1, temp_j);
                }
            }
        }

        env.storage()
            .instance()
            .set(&DataKey::TopRecipients, &leaderboard);
    }
}

#[cfg(test)]
mod test;
