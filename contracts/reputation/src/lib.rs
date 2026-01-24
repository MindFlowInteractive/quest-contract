#![no_std]

mod types;

use soroban_sdk::{contract, contractimpl, Address, Env};
use types::{Config, ContractError, DataKey, Feedback, Milestone, ReputationScore};

#[contract]
pub struct ReputationContract;

#[contractimpl]
impl ReputationContract {
    pub fn initialize(
        env: Env,
        admin: Address,
        decay_rate: u32,
        decay_period: u64,
        min_feedback_gap: u64,
        recovery_cap: u32,
    ) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Config) {
            return Err(ContractError::AlreadyInitialized);
        }

        let config = Config {
            admin: admin.clone(),
            decay_rate,
            decay_period,
            min_feedback_gap,
            recovery_cap,
        };

        env.storage().instance().set(&DataKey::Config, &config);
        Self::set_default_milestones(&env);

        Ok(())
    }

    pub fn record_feedback(
        env: Env,
        from: Address,
        to: Address,
        is_positive: bool,
        weight: u32,
        reason: u32,
    ) -> Result<(), ContractError> {
        from.require_auth();

        if from == to {
            return Err(ContractError::SelfFeedback);
        }

        Self::check_feedback_rate_limit(&env, &from, &to)?;

        let feedback_count = Self::get_feedback_count(&env, &to);
        let feedback = Feedback {
            from: from.clone(),
            to: to.clone(),
            is_positive,
            weight,
            timestamp: env.ledger().timestamp(),
            reason,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Feedback(to.clone(), feedback_count), &feedback);

        env.storage()
            .persistent()
            .set(&DataKey::FeedbackCount(to.clone()), &(feedback_count + 1));

        Self::update_reputation(&env, &to, is_positive, weight)?;

        Ok(())
    }

    pub fn get_reputation(env: Env, player: Address) -> ReputationScore {
        let mut reputation = Self::get_or_create_reputation(&env, &player);
        Self::apply_decay(&env, &mut reputation);
        
        env.storage()
            .persistent()
            .set(&DataKey::Reputation(player), &reputation);
        
        reputation
    }

    pub fn calculate_score(env: Env, player: Address) -> u32 {
        let reputation = Self::get_reputation(env.clone(), player);
        let activity_score = Self::calculate_activity_score(&env, &reputation);
        
        (reputation.positive_feedback * 40 / 100)
            + (reputation.quests_completed * 30 / 100)
            + (reputation.contributions * 20 / 100)
            + (activity_score * 10 / 100)
    }

    pub fn record_quest_completion(
        env: Env,
        player: Address,
        points: u32,
    ) -> Result<(), ContractError> {
        let mut reputation = Self::get_or_create_reputation(&env, &player);
        reputation.quests_completed = reputation.quests_completed.saturating_add(points);
        reputation.last_activity = env.ledger().timestamp();
        
        env.storage()
            .persistent()
            .set(&DataKey::Reputation(player.clone()), &reputation);
        
        Self::check_milestones(&env, &player, &reputation);
        Ok(())
    }

    pub fn record_contribution(
        env: Env,
        player: Address,
        points: u32,
    ) -> Result<(), ContractError> {
        let mut reputation = Self::get_or_create_reputation(&env, &player);
        reputation.contributions = reputation.contributions.saturating_add(points);
        reputation.last_activity = env.ledger().timestamp();
        
        env.storage()
            .persistent()
            .set(&DataKey::Reputation(player), &reputation);
        
        Ok(())
    }

    pub fn has_milestone(env: Env, player: Address, level: u32) -> bool {
        let milestones: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::PlayerMilestones(player))
            .unwrap_or(0);
        
        if level > 0 && level <= 32 {
            (milestones & (1 << (level - 1))) != 0
        } else {
            false
        }
    }

    pub fn request_recovery(
        env: Env,
        player: Address,
        points: u32,
    ) -> Result<(), ContractError> {
        player.require_auth();
        
        let config = Self::get_config(&env)?;
        let recovery_points = points.min(config.recovery_cap);
        let mut reputation = Self::get_or_create_reputation(&env, &player);
        
        reputation.total_score = reputation.total_score.saturating_add(recovery_points);
        
        if reputation.negative_feedback > 0 {
            let reduction = (recovery_points / 10).min(reputation.negative_feedback);
            reputation.negative_feedback = reputation.negative_feedback.saturating_sub(reduction);
        }
        
        reputation.last_activity = env.ledger().timestamp();
        
        env.storage()
            .persistent()
            .set(&DataKey::Reputation(player), &reputation);
        
        Ok(())
    }
}

impl ReputationContract {
    fn set_default_milestones(env: &Env) {
        let milestones = vec![
            env,
            Milestone {
                level: 1,
                score_required: 100,
                badge_id: 1,
                features_unlocked: 1,
            },
            Milestone {
                level: 2,
                score_required: 300,
                badge_id: 2,
                features_unlocked: 3,
            },
            Milestone {
                level: 3,
                score_required: 600,
                badge_id: 3,
                features_unlocked: 7,
            },
            Milestone {
                level: 4,
                score_required: 850,
                badge_id: 4,
                features_unlocked: 15,
            },
        ];

        for milestone in milestones.iter() {
            env.storage()
                .persistent()
                .set(&DataKey::Milestone(milestone.level), &milestone);
        }
    }

    fn check_feedback_rate_limit(
        env: &Env,
        from: &Address,
        to: &Address,
    ) -> Result<(), ContractError> {
        let config = Self::get_config(env)?;
        let feedback_count = Self::get_feedback_count(env, to);
        
        for i in 0..feedback_count {
            if let Some(feedback) = env
                .storage()
                .persistent()
                .get::<DataKey, Feedback>(&DataKey::Feedback(to.clone(), i))
            {
                if feedback.from == *from {
                    let time_since_last = env.ledger().timestamp() - feedback.timestamp;
                    if time_since_last < config.min_feedback_gap {
                        return Err(ContractError::RateLimitExceeded);
                    }
                }
            }
        }

        Ok(())
    }

    fn update_reputation(
        env: &Env,
        player: &Address,
        is_positive: bool,
        weight: u32,
    ) -> Result<(), ContractError> {
        let mut reputation = Self::get_or_create_reputation(env, player);

        if is_positive {
            reputation.positive_feedback += 1;
            reputation.total_score += weight;
        } else {
            reputation.negative_feedback += 1;
            reputation.total_score = reputation.total_score.saturating_sub(weight);
        }

        reputation.last_activity = env.ledger().timestamp();

        env.storage()
            .persistent()
            .set(&DataKey::Reputation(player.clone()), &reputation);

        Ok(())
    }

    fn get_config(env: &Env) -> Result<Config, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(ContractError::NotInitialized)
    }

    fn get_feedback_count(env: &Env, player: &Address) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::FeedbackCount(player.clone()))
            .unwrap_or(0)
    }

    fn get_or_create_reputation(env: &Env, player: &Address) -> ReputationScore {
        env.storage()
            .persistent()
            .get(&DataKey::Reputation(player.clone()))
            .unwrap_or(ReputationScore {
                total_score: 0,
                positive_feedback: 0,
                negative_feedback: 0,
                quests_completed: 0,
                contributions: 0,
                last_activity: env.ledger().timestamp(),
                created_at: env.ledger().timestamp(),
            })
    }

    fn apply_decay(env: &Env, reputation: &mut ReputationScore) {
        let config = match Self::get_config(env) {
            Ok(c) => c,
            Err(_) => return,
        };

        let current_time = env.ledger().timestamp();
        let time_elapsed = current_time.saturating_sub(reputation.last_activity);
        
        if config.decay_period > 0 && time_elapsed >= config.decay_period {
            let periods_elapsed = time_elapsed / config.decay_period;
            
            for _ in 0..periods_elapsed {
                let decay_amount = (reputation.total_score * config.decay_rate) / 10000;
                reputation.total_score = reputation.total_score.saturating_sub(decay_amount);
            }
            
            reputation.last_activity = current_time;
        }
    }

    fn calculate_activity_score(env: &Env, reputation: &ReputationScore) -> u32 {
        let current_time = env.ledger().timestamp();
        let time_since_activity = current_time.saturating_sub(reputation.last_activity);
        const SEVEN_DAYS: u64 = 7 * 24 * 60 * 60;
        
        if time_since_activity < SEVEN_DAYS {
            100
        } else {
            50
        }
    }

    fn check_milestones(env: &Env, player: &Address, reputation: &ReputationScore) {
        let total_score = reputation.total_score;
        let mut milestones_bitfield: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::PlayerMilestones(player.clone()))
            .unwrap_or(0);
        
        for level in 1..=4 {
            if let Some(milestone) = env
                .storage()
                .persistent()
                .get::<DataKey, Milestone>(&DataKey::Milestone(level))
            {
                if total_score >= milestone.score_required {
                    milestones_bitfield |= 1 << (level - 1);
                }
            }
        }
        
        env.storage()
            .persistent()
            .set(&DataKey::PlayerMilestones(player.clone()), &milestones_bitfield);
    }
}
