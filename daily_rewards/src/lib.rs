use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Vec};

const DAY_IN_LEDGERS: u32 = 17280; // ~24 hours (5s per ledger)
const GRACE_PERIOD_LEDGERS: u32 = 8640; // ~12 hour grace period
const MAX_STREAK_DAYS: u32 = 30;

#[derive(Clone)]
#[contracttype]
pub struct UserStreak {
    pub current_streak: u32,
    pub last_claim_ledger: u32,
    pub total_logins: u32,
    pub last_claim_hash: u64, // Anti-cheat: store ledger hash
}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    UserStreak(Address),
    Admin,
}

#[contract]
pub struct DailyRewardsContract;

#[contractimpl]
impl DailyRewardsContract {
    /// Initialize the contract with admin
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    /// Claim daily login reward
    pub fn claim_daily(env: Env, user: Address) -> u32 {
        user.require_auth();
        
        let current_ledger = env.ledger().sequence();
        let current_hash = Self::get_ledger_hash(&env);
        
        let mut user_data = Self::get_user_streak(&env, &user);
        
        // First time user
        if user_data.last_claim_ledger == 0 {
            user_data.current_streak = 1;
            user_data.last_claim_ledger = current_ledger;
            user_data.total_logins = 1;
            user_data.last_claim_hash = current_hash;
            Self::set_user_streak(&env, &user, &user_data);
            return Self::calculate_reward(1);
        }
        
        // Anti-cheat: Verify not claiming multiple times same day
        let ledgers_since_last = current_ledger.saturating_sub(user_data.last_claim_ledger);
        
        if ledgers_since_last < DAY_IN_LEDGERS {
            panic!("Already claimed today");
        }
        
        // Check if within grace period for consecutive day
        let is_consecutive = ledgers_since_last <= (DAY_IN_LEDGERS + GRACE_PERIOD_LEDGERS);
        
        if is_consecutive {
            // Increment streak
            user_data.current_streak = (user_data.current_streak + 1).min(MAX_STREAK_DAYS);
        } else {
            // Reset streak
            user_data.current_streak = 1;
        }
        
        user_data.last_claim_ledger = current_ledger;
        user_data.total_logins += 1;
        user_data.last_claim_hash = current_hash;
        
        Self::set_user_streak(&env, &user, &user_data);
        
        Self::calculate_reward(user_data.current_streak)
    }
    
    /// Calculate reward based on streak with bonus milestones
    fn calculate_reward(streak: u32) -> u32 {
        let base_reward = match streak {
            1 => 100,
            2 => 150,
            3 => 200,
            4 => 250,
            5 => 300,
            6 => 350,
            7 => 400,
            8..=14 => 400 + ((streak - 7) * 50),
            15..=21 => 750 + ((streak - 14) * 75),
            22..=30 => 1275 + ((streak - 21) * 100),
            _ => 2175,
        };
        
        // Milestone bonuses
        let milestone_bonus = match streak {
            7 => 500,      // Week milestone
            14 => 1000,    // 2 weeks
            21 => 1500,    // 3 weeks
            30 => 3000,    // Month milestone
            _ => 0,
        };
        
        base_reward + milestone_bonus
    }
    
    /// Get user's current streak info
    pub fn get_streak(env: Env, user: Address) -> UserStreak {
        Self::get_user_streak(&env, &user)
    }
    
    /// Check if user can claim today
    pub fn can_claim(env: Env, user: Address) -> bool {
        let user_data = Self::get_user_streak(&env, &user);
        
        if user_data.last_claim_ledger == 0 {
            return true;
        }
        
        let current_ledger = env.ledger().sequence();
        let ledgers_since_last = current_ledger.saturating_sub(user_data.last_claim_ledger);
        
        ledgers_since_last >= DAY_IN_LEDGERS
    }
    
    /// Check if user is still within grace period
    pub fn is_in_grace_period(env: Env, user: Address) -> bool {
        let user_data = Self::get_user_streak(&env, &user);
        
        if user_data.last_claim_ledger == 0 {
            return false;
        }
        
        let current_ledger = env.ledger().sequence();
        let ledgers_since_last = current_ledger.saturating_sub(user_data.last_claim_ledger);
        
        ledgers_since_last > DAY_IN_LEDGERS && 
        ledgers_since_last <= (DAY_IN_LEDGERS + GRACE_PERIOD_LEDGERS)
    }
    
    /// Get next reward amount without claiming
    pub fn preview_next_reward(env: Env, user: Address) -> u32 {
        let user_data = Self::get_user_streak(&env, &user);
        let current_ledger = env.ledger().sequence();
        let ledgers_since_last = current_ledger.saturating_sub(user_data.last_claim_ledger);
        
        let next_streak = if ledgers_since_last <= (DAY_IN_LEDGERS + GRACE_PERIOD_LEDGERS) {
            (user_data.current_streak + 1).min(MAX_STREAK_DAYS)
        } else {
            1
        };
        
        Self::calculate_reward(next_streak)
    }
    
    // Helper functions
    
    fn get_user_streak(env: &Env, user: &Address) -> UserStreak {
        let key = DataKey::UserStreak(user.clone());
        env.storage().persistent().get(&key).unwrap_or(UserStreak {
            current_streak: 0,
            last_claim_ledger: 0,
            total_logins: 0,
            last_claim_hash: 0,
        })
    }
    
    fn set_user_streak(env: &Env, user: &Address, data: &UserStreak) {
        let key = DataKey::UserStreak(user.clone());
        env.storage().persistent().set(&key, data);
    }
    
    fn get_ledger_hash(env: &Env) -> u64 {
        // Use ledger timestamp and sequence as pseudo-hash for anti-cheat
        let ledger = env.ledger().sequence() as u64;
        let timestamp = env.ledger().timestamp();
        ledger.wrapping_mul(timestamp)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, Env};

    #[test]
    fn test_initialize() {
        let env = Env::default();
        let contract_id = env.register_contract(None, DailyRewardsContract);
        let client = DailyRewardsContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);

        client.initialize(&admin);
    }

    #[test]
    fn test_first_login() {
        let env = Env::default();
        let contract_id = env.register_contract(None, DailyRewardsContract);
        let client = DailyRewardsContractClient::new(&env, &contract_id);
        let user = Address::generate(&env);
        let admin = Address::generate(&env);

        client.initialize(&admin);
        
        env.mock_all_auths();
        let reward = client.claim_daily(&user);
        
        assert_eq!(reward, 100); // Day 1 reward
        
        let streak = client.get_streak(&user);
        assert_eq!(streak.current_streak, 1);
        assert_eq!(streak.total_logins, 1);
    }

    #[test]
    fn test_consecutive_days_streak() {
        let env = Env::default();
        let contract_id = env.register_contract(None, DailyRewardsContract);
        let client = DailyRewardsContractClient::new(&env, &contract_id);
        let user = Address::generate(&env);
        let admin = Address::generate(&env);

        client.initialize(&admin);
        env.mock_all_auths();

        // Day 1
        let reward1 = client.claim_daily(&user);
        assert_eq!(reward1, 100);

        // Advance 1 day
        env.ledger().with_mut(|li| li.sequence_number += DAY_IN_LEDGERS);

        // Day 2
        let reward2 = client.claim_daily(&user);
        assert_eq!(reward2, 150);
        
        let streak = client.get_streak(&user);
        assert_eq!(streak.current_streak, 2);
    }

    #[test]
    fn test_week_milestone_bonus() {
        let env = Env::default();
        let contract_id = env.register_contract(None, DailyRewardsContract);
        let client = DailyRewardsContractClient::new(&env, &contract_id);
        let user = Address::generate(&env);
        let admin = Address::generate(&env);

        client.initialize(&admin);
        env.mock_all_auths();

        // Claim 7 consecutive days
        for day in 0..7 {
            if day > 0 {
                env.ledger().with_mut(|li| li.sequence_number += DAY_IN_LEDGERS);
            }
            let reward = client.claim_daily(&user);
            
            if day == 6 {
                assert_eq!(reward, 400 + 500); // Base + milestone bonus
            }
        }
        
        let streak = client.get_streak(&user);
        assert_eq!(streak.current_streak, 7);
    }

    #[test]
    fn test_grace_period_maintains_streak() {
        let env = Env::default();
        let contract_id = env.register_contract(None, DailyRewardsContract);
        let client = DailyRewardsContractClient::new(&env, &contract_id);
        let user = Address::generate(&env);
        let admin = Address::generate(&env);

        client.initialize(&admin);
        env.mock_all_auths();

        // Day 1
        client.claim_daily(&user);

        // Advance 1 day + some grace period (but within 12h grace)
        env.ledger().with_mut(|li| li.sequence_number += DAY_IN_LEDGERS + 4000);

        assert!(client.is_in_grace_period(&user));
        
        // Should maintain streak
        client.claim_daily(&user);
        let streak = client.get_streak(&user);
        assert_eq!(streak.current_streak, 2);
    }

    #[test]
    fn test_streak_reset_after_grace_period() {
        let env = Env::default();
        let contract_id = env.register_contract(None, DailyRewardsContract);
        let client = DailyRewardsContractClient::new(&env, &contract_id);
        let user = Address::generate(&env);
        let admin = Address::generate(&env);

        client.initialize(&admin);
        env.mock_all_auths();

        // Day 1
        client.claim_daily(&user);

        // Advance beyond grace period
        env.ledger().with_mut(|li| {
            li.sequence_number += DAY_IN_LEDGERS + GRACE_PERIOD_LEDGERS + 100
        });

        // Streak should reset
        client.claim_daily(&user);
        let streak = client.get_streak(&user);
        assert_eq!(streak.current_streak, 1);
    }

    #[test]
    #[should_panic(expected = "Already claimed today")]
    fn test_prevent_double_claim_same_day() {
        let env = Env::default();
        let contract_id = env.register_contract(None, DailyRewardsContract);
        let client = DailyRewardsContractClient::new(&env, &contract_id);
        let user = Address::generate(&env);
        let admin = Address::generate(&env);

        client.initialize(&admin);
        env.mock_all_auths();

        client.claim_daily(&user);
        client.claim_daily(&user); // Should panic
    }

    #[test]
    fn test_can_claim_check() {
        let env = Env::default();
        let contract_id = env.register_contract(None, DailyRewardsContract);
        let client = DailyRewardsContractClient::new(&env, &contract_id);
        let user = Address::generate(&env);
        let admin = Address::generate(&env);

        client.initialize(&admin);
        env.mock_all_auths();

        // First time should be claimable
        assert!(client.can_claim(&user));

        client.claim_daily(&user);
        
        // Immediately after claim, should not be claimable
        assert!(!client.can_claim(&user));

        // After 1 day, should be claimable again
        env.ledger().with_mut(|li| li.sequence_number += DAY_IN_LEDGERS);
        assert!(client.can_claim(&user));
    }

    #[test]
    fn test_preview_next_reward() {
        let env = Env::default();
        let contract_id = env.register_contract(None, DailyRewardsContract);
        let client = DailyRewardsContractClient::new(&env, &contract_id);
        let user = Address::generate(&env);
        let admin = Address::generate(&env);

        client.initialize(&admin);
        env.mock_all_auths();

        client.claim_daily(&user);
        
        env.ledger().with_mut(|li| li.sequence_number += DAY_IN_LEDGERS);
        
        let preview = client.preview_next_reward(&user);
        assert_eq!(preview, 150); // Day 2 reward
    }

    #[test]
    fn test_max_streak_cap() {
        let env = Env::default();
        let contract_id = env.register_contract(None, DailyRewardsContract);
        let client = DailyRewardsContractClient::new(&env, &contract_id);
        let user = Address::generate(&env);
        let admin = Address::generate(&env);

        client.initialize(&admin);
        env.mock_all_auths();

        // Claim 31 days
        for _ in 0..31 {
            env.ledger().with_mut(|li| {
                if li.sequence_number > 0 {
                    li.sequence_number += DAY_IN_LEDGERS;
                }
            });
            client.claim_daily(&user);
        }
        
        let streak = client.get_streak(&user);
        assert_eq!(streak.current_streak, MAX_STREAK_DAYS); // Capped at 30
    }
}