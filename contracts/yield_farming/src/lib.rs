#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, Vec,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssetType {
    Token,
    NFT,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PoolConfig {
    pub asset_address: Address,
    pub asset_type: AssetType,
    pub apy_basis_points: u32,      // 1000 = 10%
    pub lock_period_days: u32,
    pub early_withdrawal_penalty_bp: u32, // 500 = 5%
    pub multiplier_bp: u32,         // 10000 = 1x, 15000 = 1.5x
    pub auto_compound: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct StakePosition {
    pub staker: Address,
    pub pool_id: u32,
    pub amount: i128,
    pub nft_id: Option<u32>,
    pub stake_time: u64,
    pub last_claim_time: u64,
    pub unlock_time: u64,
    pub accumulated_rewards: i128,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PoolStats {
    pub total_staked: i128,
    pub total_stakers: u32,
    pub total_rewards_distributed: i128,
}

#[contracttype]
pub enum DataKey {
    Admin,
    RewardToken,
    Pool(u32),
    PoolCounter,
    PoolStats(u32),
    Stake(Address, u32),
    StakeCounter(Address),
    UserStakes(Address),
}

const SECONDS_PER_YEAR: u64 = 31_536_000;
const BASIS_POINTS: i128 = 10_000;

#[contract]
pub struct YieldFarmingContract;

#[contractimpl]
impl YieldFarmingContract {
    
    pub fn initialize(env: Env, admin: Address, reward_token: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::RewardToken, &reward_token);
        env.storage().instance().set(&DataKey::PoolCounter, &0u32);
    }

    pub fn create_pool(
        env: Env,
        asset_address: Address,
        asset_type: AssetType,
        apy_basis_points: u32,
        lock_period_days: u32,
        early_withdrawal_penalty_bp: u32,
        multiplier_bp: u32,
        auto_compound: bool,
    ) -> u32 {
        Self::require_admin(&env);
        
        let pool_id: u32 = env.storage().instance().get(&DataKey::PoolCounter).unwrap_or(0);
        let next_id = pool_id + 1;
        
        let config = PoolConfig {
            asset_address,
            asset_type,
            apy_basis_points,
            lock_period_days,
            early_withdrawal_penalty_bp,
            multiplier_bp,
            auto_compound,
        };
        
        env.storage().persistent().set(&DataKey::Pool(next_id), &config);
        env.storage().persistent().set(&DataKey::PoolStats(next_id), &PoolStats {
            total_staked: 0,
            total_stakers: 0,
            total_rewards_distributed: 0,
        });
        env.storage().instance().set(&DataKey::PoolCounter, &next_id);
        
        next_id
    }

    pub fn stake_tokens(env: Env, staker: Address, pool_id: u32, amount: i128) {
        staker.require_auth();
        
        if amount <= 0 {
            panic!("Invalid amount");
        }
        
        let pool: PoolConfig = env.storage().persistent()
            .get(&DataKey::Pool(pool_id))
            .expect("Pool not found");
        
        if pool.asset_type != AssetType::Token {
            panic!("Pool is for NFTs");
        }
        
        let token_client = token::Client::new(&env, &pool.asset_address);
        token_client.transfer(&staker, &env.current_contract_address(), &amount);
        
        let now = env.ledger().timestamp();
        let unlock_time = now + (pool.lock_period_days as u64 * 86_400);
        
        let stake_count: u32 = env.storage().persistent()
            .get(&DataKey::StakeCounter(staker.clone()))
            .unwrap_or(0);
        let stake_id = stake_count + 1;
        
        let position = StakePosition {
            staker: staker.clone(),
            pool_id,
            amount,
            nft_id: None,
            stake_time: now,
            last_claim_time: now,
            unlock_time,
            accumulated_rewards: 0,
        };
        
        env.storage().persistent().set(&DataKey::Stake(staker.clone(), stake_id), &position);
        env.storage().persistent().set(&DataKey::StakeCounter(staker.clone()), &stake_id);
        
        let mut user_stakes: Vec<u32> = env.storage().persistent()
            .get(&DataKey::UserStakes(staker.clone()))
            .unwrap_or(Vec::new(&env));
        user_stakes.push_back(stake_id);
        env.storage().persistent().set(&DataKey::UserStakes(staker.clone()), &user_stakes);
        
        Self::update_pool_stats(&env, pool_id, amount, true);
    }

    pub fn stake_nft(env: Env, staker: Address, pool_id: u32, nft_id: u32) {
        staker.require_auth();
        
        let pool: PoolConfig = env.storage().persistent()
            .get(&DataKey::Pool(pool_id))
            .expect("Pool not found");
        
        if pool.asset_type != AssetType::NFT {
            panic!("Pool is for tokens");
        }
        
        let token_client = token::Client::new(&env, &pool.asset_address);
        token_client.transfer(&staker, &env.current_contract_address(), &1);
        
        let now = env.ledger().timestamp();
        let unlock_time = now + (pool.lock_period_days as u64 * 86_400);
        
        let stake_count: u32 = env.storage().persistent()
            .get(&DataKey::StakeCounter(staker.clone()))
            .unwrap_or(0);
        let stake_id = stake_count + 1;
        
        let position = StakePosition {
            staker: staker.clone(),
            pool_id,
            amount: 1,
            nft_id: Some(nft_id),
            stake_time: now,
            last_claim_time: now,
            unlock_time,
            accumulated_rewards: 0,
        };
        
        env.storage().persistent().set(&DataKey::Stake(staker.clone(), stake_id), &position);
        env.storage().persistent().set(&DataKey::StakeCounter(staker.clone()), &stake_id);
        
        let mut user_stakes: Vec<u32> = env.storage().persistent()
            .get(&DataKey::UserStakes(staker.clone()))
            .unwrap_or(Vec::new(&env));
        user_stakes.push_back(stake_id);
        env.storage().persistent().set(&DataKey::UserStakes(staker.clone()), &user_stakes);
        
        Self::update_pool_stats(&env, pool_id, 1, true);
    }

    pub fn calculate_rewards(env: Env, staker: Address, stake_id: u32) -> i128 {
        let position: StakePosition = env.storage().persistent()
            .get(&DataKey::Stake(staker.clone(), stake_id))
            .expect("Stake not found");
        
        let pool: PoolConfig = env.storage().persistent()
            .get(&DataKey::Pool(position.pool_id))
            .expect("Pool not found");
        
        let now = env.ledger().timestamp();
        let time_elapsed = now - position.last_claim_time;
        
        let base_reward = (position.amount * (pool.apy_basis_points as i128) * (time_elapsed as i128)) 
            / (BASIS_POINTS * SECONDS_PER_YEAR as i128);
        
        let multiplied_reward = (base_reward * (pool.multiplier_bp as i128)) / BASIS_POINTS;
        
        position.accumulated_rewards + multiplied_reward
    }

    pub fn claim_rewards(env: Env, staker: Address, stake_id: u32) -> i128 {
        staker.require_auth();
        
        let rewards = Self::calculate_rewards(env.clone(), staker.clone(), stake_id);
        
        if rewards <= 0 {
            return 0;
        }
        
        let mut position: StakePosition = env.storage().persistent()
            .get(&DataKey::Stake(staker.clone(), stake_id))
            .expect("Stake not found");
        
        let pool: PoolConfig = env.storage().persistent()
            .get(&DataKey::Pool(position.pool_id))
            .expect("Pool not found");
        
        let now = env.ledger().timestamp();
        
        if pool.auto_compound {
            position.amount += rewards;
            position.accumulated_rewards = 0;
        } else {
            let reward_token: Address = env.storage().instance()
                .get(&DataKey::RewardToken)
                .expect("Reward token not set");
            let token_client = token::Client::new(&env, &reward_token);
            token_client.transfer(&env.current_contract_address(), &staker, &rewards);
            position.accumulated_rewards = 0;
        }
        
        position.last_claim_time = now;
        env.storage().persistent().set(&DataKey::Stake(staker.clone(), stake_id), &position);
        
        let mut stats: PoolStats = env.storage().persistent()
            .get(&DataKey::PoolStats(position.pool_id))
            .unwrap();
        stats.total_rewards_distributed += rewards;
        env.storage().persistent().set(&DataKey::PoolStats(position.pool_id), &stats);
        
        rewards
    }

    pub fn unstake(env: Env, staker: Address, stake_id: u32) -> i128 {
        staker.require_auth();
        
        let position: StakePosition = env.storage().persistent()
            .get(&DataKey::Stake(staker.clone(), stake_id))
            .expect("Stake not found");
        
        let pool: PoolConfig = env.storage().persistent()
            .get(&DataKey::Pool(position.pool_id))
            .expect("Pool not found");
        
        let now = env.ledger().timestamp();
        let is_early = now < position.unlock_time;
        
        let pending_rewards = Self::calculate_rewards(env.clone(), staker.clone(), stake_id);
        
        let mut return_amount = position.amount;
        let mut penalty = 0i128;
        
        if is_early {
            penalty = (return_amount * (pool.early_withdrawal_penalty_bp as i128)) / BASIS_POINTS;
            return_amount -= penalty;
        }
        
        let token_client = token::Client::new(&env, &pool.asset_address);
        token_client.transfer(&env.current_contract_address(), &staker, &return_amount);
        
        if pending_rewards > 0 && !pool.auto_compound {
            let reward_token: Address = env.storage().instance()
                .get(&DataKey::RewardToken)
                .expect("Reward token not set");
            let reward_client = token::Client::new(&env, &reward_token);
            reward_client.transfer(&env.current_contract_address(), &staker, &pending_rewards);
        }
        
        env.storage().persistent().remove(&DataKey::Stake(staker.clone(), stake_id));
        
        Self::update_pool_stats(&env, position.pool_id, position.amount, false);
        
        return_amount
    }

    pub fn get_pool(env: Env, pool_id: u32) -> PoolConfig {
        env.storage().persistent()
            .get(&DataKey::Pool(pool_id))
            .expect("Pool not found")
    }

    pub fn get_pool_stats(env: Env, pool_id: u32) -> PoolStats {
        env.storage().persistent()
            .get(&DataKey::PoolStats(pool_id))
            .expect("Pool not found")
    }

    pub fn get_stake(env: Env, staker: Address, stake_id: u32) -> StakePosition {
        env.storage().persistent()
            .get(&DataKey::Stake(staker, stake_id))
            .expect("Stake not found")
    }

    pub fn get_user_stakes(env: Env, staker: Address) -> Vec<u32> {
        env.storage().persistent()
            .get(&DataKey::UserStakes(staker))
            .unwrap_or(Vec::new(&env))
    }

    fn require_admin(env: &Env) {
        let admin: Address = env.storage().instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        admin.require_auth();
    }

    fn update_pool_stats(env: &Env, pool_id: u32, amount: i128, is_stake: bool) {
        let mut stats: PoolStats = env.storage().persistent()
            .get(&DataKey::PoolStats(pool_id))
            .unwrap();
        
        if is_stake {
            stats.total_staked += amount;
            stats.total_stakers += 1;
        } else {
            stats.total_staked -= amount;
            stats.total_stakers = stats.total_stakers.saturating_sub(1);
        }
        
        env.storage().persistent().set(&DataKey::PoolStats(pool_id), &stats);
    }
}

#[cfg(test)]
mod test;
