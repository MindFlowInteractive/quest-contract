#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, vec, Address, Env, Map, Symbol, Vec, U32};

const SEASON_DURATION: u64 = 2_592_000; // 30 days in seconds
const LEVELS_PER_SEASON: u32 = 100;
const FREE_TIER_REWARDS: u32 = 50;
const PREMIUM_TIER_REWARDS: u32 = 100;
const XP_PER_LEVEL: u32 = 1000;

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Season(u32),                    // Current season number
    PlayerBattlePass(Address, u32), // (player, season) -> BattlePass
    SeasonInfo(u32),                // season -> SeasonInfo
    PlayerXP(Address, u32),         // (player, season) -> xp amount
    ClaimedRewards(Address, u32),   // (player, season) -> claimed_level
    SeasonHistory(Address),         // player -> Vec<SeasonRecord>
    BonusXPEvent(u32),              // season -> XP multiplier
}

#[derive(Clone)]
#[contracttype]
pub struct BattlePass {
    pub owner: Address,
    pub season: u32,
    pub tier: PassTier,
    pub current_level: u32,
    pub is_active: bool,
    pub purchase_time: u64,
}

#[derive(Clone, Copy)]
#[contracttype]
pub enum PassTier {
    Free = 0,
    Premium = 1,
}

#[derive(Clone)]
#[contracttype]
pub struct SeasonInfo {
    pub season_number: u32,
    pub start_time: u64,
    pub end_time: u64,
    pub is_active: bool,
    pub total_players: u32,
    pub reward_pool: u128,
}

#[derive(Clone)]
#[contracttype]
pub struct SeasonRecord {
    pub season: u32,
    pub final_level: u32,
    pub total_xp: u32,
    pub rewards_claimed: bool,
    pub tier: PassTier,
}

#[derive(Clone)]
#[contracttype]
pub struct Reward {
    pub level: u32,
    pub reward_amount: u128,
    pub is_exclusive_premium: bool,
}

#[contract]
pub struct BattlePassContract;

#[contractimpl]
impl BattlePassContract {
    /// Initialize a new season
    pub fn init_season(env: Env, season_number: u32, reward_pool: u128) {
        let current_season = Self::get_current_season(&env);
        if season_number <= current_season {
            panic!("Season number must be greater than current season");
        }

        let now = env.ledger().timestamp();
        let season_info = SeasonInfo {
            season_number,
            start_time: now,
            end_time: now + SEASON_DURATION,
            is_active: true,
            total_players: 0,
            reward_pool,
        };

        env.storage()
            .persistent()
            .set(&DataKey::SeasonInfo(season_number), &season_info);
        env.storage()
            .persistent()
            .set(&DataKey::Season(0), &season_number);
    }

    /// Purchase a battle pass (free or premium)
    pub fn purchase_pass(env: Env, player: Address, tier: PassTier) {
        player.require_auth();
        let season = Self::get_current_season(&env);
        Self::season_is_active(&env, season);

        // Check if already owns a pass this season
        let pass_key = DataKey::PlayerBattlePass(player.clone(), season);
        if env.storage().persistent().has(&pass_key) {
            panic!("Player already owns a battle pass for this season");
        }

        let now = env.ledger().timestamp();
        let battle_pass = BattlePass {
            owner: player.clone(),
            season,
            tier,
            current_level: 0,
            is_active: true,
            purchase_time: now,
        };

        env.storage().persistent().set(&pass_key, &battle_pass);

        // Increment total players
        let mut season_info = Self::get_season_info(&env, season);
        season_info.total_players += 1;
        env.storage()
            .persistent()
            .set(&DataKey::SeasonInfo(season), &season_info);

        // Initialize XP and history
        env.storage()
            .persistent()
            .set(&DataKey::PlayerXP(player.clone(), season), &0u32);
    }

    /// Add XP to player for current season
    pub fn add_xp(env: Env, player: Address, amount: u32) {
        player.require_auth();
        let season = Self::get_current_season(&env);
        Self::season_is_active(&env, season);

        // Verify player owns a pass
        let pass_key = DataKey::PlayerBattlePass(player.clone(), season);
        if !env.storage().persistent().has(&pass_key) {
            panic!("Player does not own a battle pass for this season");
        }

        // Apply bonus XP multiplier if event is active
        let bonus_key = DataKey::BonusXPEvent(season);
        let xp_amount = if env.storage().persistent().has(&bonus_key) {
            let multiplier: u32 = env.storage().persistent().get(&bonus_key).unwrap();
            (amount as u128).saturating_mul(multiplier as u128) as u32
        } else {
            amount
        };

        let xp_key = DataKey::PlayerXP(player.clone(), season);
        let current_xp: u32 = env
            .storage()
            .persistent()
            .get(&xp_key)
            .unwrap_or(0);

        let new_xp = current_xp.saturating_add(xp_amount);
        env.storage()
            .persistent()
            .set(&xp_key, &new_xp);

        // Update battle pass level
        let new_level = new_xp / XP_PER_LEVEL;
        if new_level > LEVELS_PER_SEASON {
            env.storage()
                .persistent()
                .set(&xp_key, &(LEVELS_PER_SEASON * XP_PER_LEVEL));
        }

        let mut battle_pass: BattlePass = env.storage().persistent().get(&pass_key).unwrap();
        battle_pass.current_level = new_level.min(LEVELS_PER_SEASON);
        env.storage().persistent().set(&pass_key, &battle_pass);
    }

    /// Claim reward for a specific level
    pub fn claim_reward(env: Env, player: Address, level: u32) -> u128 {
        player.require_auth();
        let season = Self::get_current_season(&env);

        // Check player owns pass for season
        let pass_key = DataKey::PlayerBattlePass(player.clone(), season);
        let battle_pass: BattlePass = env
            .storage()
            .persistent()
            .get(&pass_key)
            .unwrap_or_else(|| panic!("No battle pass found"));

        // Check level is unlocked
        if level > battle_pass.current_level {
            panic!("Level not yet unlocked");
        }

        // Check level is within valid range
        if level == 0 || level > LEVELS_PER_SEASON {
            panic!("Invalid level");
        }

        // Determine if this is an exclusive premium reward
        let is_premium_only = level > FREE_TIER_REWARDS;
        if is_premium_only && matches!(battle_pass.tier, PassTier::Free) {
            panic!("Premium rewards only available with premium tier");
        }

        // Check if already claimed
        let claimed_key = DataKey::ClaimedRewards(player.clone(), season);
        let mut claimed_up_to: u32 = env
            .storage()
            .persistent()
            .get(&claimed_key)
            .unwrap_or(0);

        if level <= claimed_up_to {
            panic!("Reward already claimed");
        }

        // Calculate reward amount (progressive)
        let reward_amount = Self::calculate_reward(level);
        
        // Update claimed level
        env.storage()
            .persistent()
            .set(&claimed_key, &level);

        reward_amount
    }

    /// Claim all available rewards retroactively (for those who purchased premium after leveling)
    pub fn claim_retroactive_rewards(env: Env, player: Address) -> u128 {
        player.require_auth();
        let season = Self::get_current_season(&env);

        let pass_key = DataKey::PlayerBattlePass(player.clone(), season);
        let battle_pass: BattlePass = env
            .storage()
            .persistent()
            .get(&pass_key)
            .unwrap_or_else(|| panic!("No battle pass found"));

        // Only works for premium tier
        if !matches!(battle_pass.tier, PassTier::Premium) {
            panic!("Only premium tier can claim retroactive rewards");
        }

        let claimed_key = DataKey::ClaimedRewards(player.clone(), season);
        let claimed_up_to: u32 = env
            .storage()
            .persistent()
            .get(&claimed_key)
            .unwrap_or(0);

        // Calculate total rewards from claimed_up_to + 1 to current level
        let mut total_reward: u128 = 0;
        for level in (claimed_up_to + 1)..=battle_pass.current_level {
            if level <= LEVELS_PER_SEASON {
                total_reward += Self::calculate_reward(level);
            }
        }

        if total_reward > 0 {
            env.storage()
                .persistent()
                .set(&claimed_key, &battle_pass.current_level);
        }

        total_reward
    }

    /// Transfer battle pass to another player
    pub fn transfer_pass(env: Env, from: Address, to: Address) {
        from.require_auth();
        let season = Self::get_current_season(&env);

        let pass_key_from = DataKey::PlayerBattlePass(from.clone(), season);
        let battle_pass: BattlePass = env
            .storage()
            .persistent()
            .get(&pass_key_from)
            .unwrap_or_else(|| panic!("No battle pass to transfer"));

        // Check recipient doesn't already have a pass
        let pass_key_to = DataKey::PlayerBattlePass(to.clone(), season);
        if env.storage().persistent().has(&pass_key_to) {
            panic!("Recipient already owns a battle pass for this season");
        }

        // Transfer pass
        let mut transferred_pass = battle_pass;
        transferred_pass.owner = to.clone();

        env.storage()
            .persistent()
            .remove(&pass_key_from);
        env.storage()
            .persistent()
            .set(&pass_key_to, &transferred_pass);

        // Transfer XP
        let xp_key_from = DataKey::PlayerXP(from.clone(), season);
        let xp_key_to = DataKey::PlayerXP(to.clone(), season);
        let xp: u32 = env
            .storage()
            .persistent()
            .get(&xp_key_from)
            .unwrap_or(0);

        env.storage()
            .persistent()
            .remove(&xp_key_from);
        env.storage()
            .persistent()
            .set(&xp_key_to, &xp);
    }

    /// Set bonus XP multiplier for current season (admin only)
    pub fn set_bonus_xp_event(env: Env, multiplier: u32) {
        if multiplier < 1 || multiplier > 10 {
            panic!("Multiplier must be between 1 and 10");
        }

        let season = Self::get_current_season(&env);
        env.storage()
            .persistent()
            .set(&DataKey::BonusXPEvent(season), &multiplier);
    }

    /// Clear bonus XP event
    pub fn clear_bonus_xp_event(env: Env) {
        let season = Self::get_current_season(&env);
        env.storage()
            .persistent()
            .remove(&DataKey::BonusXPEvent(season));
    }

    /// Archive current season to history
    pub fn archive_season(env: Env, player: Address) {
        let season = Self::get_current_season(&env);
        let pass_key = DataKey::PlayerBattlePass(player.clone(), season);

        let battle_pass: BattlePass = env
            .storage()
            .persistent()
            .get(&pass_key)
            .unwrap_or_else(|| panic!("No battle pass found"));

        let xp_key = DataKey::PlayerXP(player.clone(), season);
        let total_xp: u32 = env
            .storage()
            .persistent()
            .get(&xp_key)
            .unwrap_or(0);

        let claimed_key = DataKey::ClaimedRewards(player.clone(), season);
        let claimed: u32 = env
            .storage()
            .persistent()
            .get(&claimed_key)
            .unwrap_or(0);

        let record = SeasonRecord {
            season,
            final_level: battle_pass.current_level,
            total_xp,
            rewards_claimed: claimed > 0,
            tier: battle_pass.tier,
        };

        // Add to history
        let history_key = DataKey::SeasonHistory(player);
        let mut history: Vec<SeasonRecord> = env
            .storage()
            .persistent()
            .get(&history_key)
            .unwrap_or_else(|| Vec::new(&env));

        history.push_back(record);
        env.storage()
            .persistent()
            .set(&history_key, &history);
    }

    /// Get player's battle pass for current season
    pub fn get_player_pass(env: Env, player: Address) -> Option<BattlePass> {
        let season = Self::get_current_season(&env);
        let pass_key = DataKey::PlayerBattlePass(player, season);
        env.storage().persistent().get(&pass_key)
    }

    /// Get player's current XP for season
    pub fn get_player_xp(env: Env, player: Address) -> u32 {
        let season = Self::get_current_season(&env);
        let xp_key = DataKey::PlayerXP(player, season);
        env.storage().persistent().get(&xp_key).unwrap_or(0)
    }

    /// Get player's season history
    pub fn get_season_history(env: Env, player: Address) -> Vec<SeasonRecord> {
        let history_key = DataKey::SeasonHistory(player);
        env.storage()
            .persistent()
            .get(&history_key)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Get current season
    pub fn get_current_season(env: &Env) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::Season(0))
            .unwrap_or(1)
    }

    /// Get season info
    pub fn get_season_info(env: &Env, season: u32) -> SeasonInfo {
        env.storage()
            .persistent()
            .get(&DataKey::SeasonInfo(season))
            .unwrap_or_else(|| panic!("Season not found"))
    }

    /// Check if season is active
    fn season_is_active(env: &Env, season: u32) {
        let season_info = Self::get_season_info(env, season);
        let now = env.ledger().timestamp();
        
        if now > season_info.end_time {
            panic!("Season has expired");
        }
        
        if !season_info.is_active {
            panic!("Season is not active");
        }
    }

    /// Calculate reward amount for a level (progressive scaling)
    fn calculate_reward(level: u32) -> u128 {
        // Base reward: 100 tokens per level
        // Bonus for higher levels: 10% increase per 10 levels
        let base_reward: u128 = 100;
        let bonus_tier = (level / 10) as u128;
        let bonus_multiplier = bonus_tier + 1; // 1x at level 1-9, 2x at level 10-19, etc.
        
        (base_reward * level as u128 * bonus_multiplier) / 10
    }

    /// Get bonus XP multiplier for current season
    pub fn get_bonus_xp_multiplier(env: Env) -> u32 {
        let season = Self::get_current_season(&env);
        let bonus_key = DataKey::BonusXPEvent(season);
        env.storage()
            .persistent()
            .get(&bonus_key)
            .unwrap_or(1)
    }

    /// Deactivate season
    pub fn deactivate_season(env: Env, season: u32) {
        let mut season_info = Self::get_season_info(&env, season);
        season_info.is_active = false;
        env.storage()
            .persistent()
            .set(&DataKey::SeasonInfo(season), &season_info);
    }
}
