#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, Vec};

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AchievementTier {
    Common = 1,
    Rare = 2,
    Epic = 3,
    Legendary = 4,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AchievementRecord {
    pub player: Address,
    pub achievement_id: u32,
    pub tier: AchievementTier,
    pub multiplier_bps: u32, // Basis points multiplier (e.g. 15000 = 1.5x / 150%)
    pub unlocked_at: u64,
}

#[contracttype]
pub enum DataKey {
    Achievements(Address),
    PlayerTotalBoost(Address),
    History,
}

#[contract]
pub struct AchievementRarityBoostContract;

#[contractimpl]
impl AchievementRarityBoostContract {
    /// Unlocks an achievement with a specified tier and rarity multiplier for a player.
    pub fn unlock_achievement(
        env: Env,
        authority: Address,
        player: Address,
        achievement_id: u32,
        tier: AchievementTier,
    ) {
        authority.require_auth();

        let multiplier_bps = match tier {
            AchievementTier::Common => 10000,    // 1.0x (100%)
            AchievementTier::Rare => 12500,      // 1.25x (125%)
            AchievementTier::Epic => 15000,      // 1.5x (150%)
            AchievementTier::Legendary => 20000, // 2.0x (200%)
        };

        let record = AchievementRecord {
            player: player.clone(),
            achievement_id,
            tier,
            multiplier_bps,
            unlocked_at: env.ledger().timestamp(),
        };

        let mut player_achievements: Vec<AchievementRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::Achievements(player.clone()))
            .unwrap_or_else(|| Vec::new(&env));

        // Prevent duplicate achievement unlocks
        for a in player_achievements.iter() {
            if a.achievement_id == achievement_id {
                panic!("Achievement already unlocked by player");
            }
        }

        player_achievements.push_back(record);
        env.storage().persistent().set(&DataKey::Achievements(player.clone()), &player_achievements);

        // Update player's aggregated rarity boost multiplier
        let current_total_boost: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::PlayerTotalBoost(player.clone()))
            .unwrap_or(10000);

        // Cascading bonus calculation: compounding or additive bonus stacking
        let new_total_boost = current_total_boost.checked_add(multiplier_bps - 10000).unwrap_or_else(|| {
            panic!("Overflow in total boost calculation");
        });

        env.storage().persistent().set(&DataKey::PlayerTotalBoost(player.clone()), &new_total_boost);

        // Track analytics history
        let mut history: Vec<String> = env
            .storage()
            .instance()
            .get(&DataKey::History)
            .unwrap_or_else(|| Vec::new(&env));
        
        history.push_back(String::from_str(&env, "AchievementUnlocked"));
        env.storage().instance().set(&DataKey::History, &history);

        env.events().publish(
            (String::from_str(&env, "AchievementUnlocked"), player),
            (achievement_id, tier as u32),
        );
    }

    /// Retrieves all unlocked achievements for a player.
    pub fn get_player_achievements(env: Env, player: Address) -> Vec<AchievementRecord> {
        env.storage()
            .persistent()
            .get(&DataKey::Achievements(player))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Retrieves the aggregated reward and XP multiplier for a player based on their achievement rarities.
    pub fn get_player_total_boost(env: Env, player: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::PlayerTotalBoost(player))
            .unwrap_or(10000) // Default 10000 bps (1.0x)
    }
}