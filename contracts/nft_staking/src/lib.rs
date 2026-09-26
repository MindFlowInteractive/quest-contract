#![no_std]

mod reward_history;

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol, Vec};

// ─────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────

const MIN_STAKE_SECONDS: u64 = 48 * 60 * 60; // 48h

// ─────────────────────────────────────────────────────────────
// Errors
// ─────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum NftStakingError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    PositionAlreadyExists = 4,
    PositionNotFound = 5,
    NotTokenOwner = 6,
    UnstakeTooEarly = 7,
    InvalidRewardRate = 8,
    RarityNotConfigured = 9,
    InvalidTokenRarity = 10,
}

// ─────────────────────────────────────────────────────────────
// Data Types
// ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
    pub nft_contract: Address,
    pub reward_token: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NFTStakePosition {
    pub staker: Address,
    pub token_id: u32,
    pub rarity: u32,
    pub staked_at: u64,
    pub last_claim_ledger: u32,
    pub total_claimed: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RarityConfig {
    pub rarity: u32,
    pub tokens_per_ledger: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PositionView {
    pub position: NFTStakePosition,
    pub pending_rewards: i128,
    pub days_staked: u64,
}

#[contracttype]
pub enum DataKey {
    Config,
    Position(u32),      // token_id => NFTStakePosition
    RarityConfig(u32),  // rarity => RarityConfig
    TokenRarity(u32),   // token_id => rarity
}

// ─────────────────────────────────────────────────────────────
// External Clients (minimal interfaces)
// ─────────────────────────────────────────────────────────────

#[soroban_sdk::contractclient(name = "AchievementNFTClient")]
pub trait AchievementNFT {
    fn owner_of(env: Env, token_id: u32) -> Address;
    fn transfer(env: Env, from: Address, to: Address, token_id: u32);
}

#[soroban_sdk::contractclient(name = "RewardTokenClient")]
pub trait RewardToken {
    fn mint(env: Env, minter: Address, to: Address, amount: i128);
}

// ─────────────────────────────────────────────────────────────
// Contract
// ─────────────────────────────────────────────────────────────

#[contract]
pub struct NftStakingContract;

#[contractimpl]
impl NftStakingContract {
    // ───────────── Initialization ─────────────

    pub fn initialize(env: Env, admin: Address, nft_contract: Address, reward_token: Address) {
        admin.require_auth();

        if env.storage().instance().has(&DataKey::Config) {
            soroban_sdk::panic_with_error!(&env, NftStakingError::AlreadyInitialized);
        }

        let cfg = Config {
            admin,
            nft_contract,
            reward_token,
        };

        env.storage().instance().set(&DataKey::Config, &cfg);
    }

    // ───────────── Admin: Rarity config ─────────────

    pub fn set_rarity_config(env: Env, admin: Address, rarity: u32, tokens_per_ledger: i128) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        if tokens_per_ledger <= 0 {
            soroban_sdk::panic_with_error!(&env, NftStakingError::InvalidRewardRate);
        }

        let cfg = RarityConfig {
            rarity,
            tokens_per_ledger,
        };

        let key = DataKey::RarityConfig(rarity);
        env.storage().persistent().set(&key, &cfg);
        env.storage().persistent().extend_ttl(&key, 100_000, 500_000);
    }

    /// Configure the rarity tier for a specific token id (admin only).
    ///
    /// This enables `stake(token_id)` to record the correct rarity without
    /// trusting user-supplied rarity values.
    pub fn set_token_rarity(env: Env, admin: Address, token_id: u32, rarity: u32) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        if !env
            .storage()
            .persistent()
            .has(&DataKey::RarityConfig(rarity))
        {
            soroban_sdk::panic_with_error!(&env, NftStakingError::RarityNotConfigured);
        }

        let key = DataKey::TokenRarity(token_id);
        env.storage().persistent().set(&key, &rarity);
        env.storage().persistent().extend_ttl(&key, 100_000, 500_000);
    }

    // ───────────── Core: stake/claim/unstake ─────────────

    /// Stake an NFT and begin accruing rewards per-ledger.
    ///
    /// Locks the NFT by transferring it into this contract.
    pub fn stake(env: Env, staker: Address, token_id: u32) {
        staker.require_auth();

        let cfg = Self::load_config(&env);
        let pos_key = DataKey::Position(token_id);
        if env.storage().persistent().has(&pos_key) {
            soroban_sdk::panic_with_error!(&env, NftStakingError::PositionAlreadyExists);
        }

        // Ensure the caller owns the NFT before transferring it in.
        let nft = AchievementNFTClient::new(&env, &cfg.nft_contract);
        let owner = nft.owner_of(&token_id);
        if owner != staker {
            soroban_sdk::panic_with_error!(&env, NftStakingError::NotTokenOwner);
        }

        // Resolve rarity for this token (must be set by admin).
        let rarity: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::TokenRarity(token_id))
            .unwrap_or_else(|| soroban_sdk::panic_with_error!(&env, NftStakingError::InvalidTokenRarity));

        if !env
            .storage()
            .persistent()
            .has(&DataKey::RarityConfig(rarity))
        {
            soroban_sdk::panic_with_error!(&env, NftStakingError::RarityNotConfigured);
        }

        // Transfer the NFT into this contract (lock).
        nft.transfer(&staker, &env.current_contract_address(), &token_id);

        let position = NFTStakePosition {
            staker: staker.clone(),
            token_id,
            rarity,
            staked_at: env.ledger().timestamp(),
            last_claim_ledger: env.ledger().sequence(),
            total_claimed: 0,
        };

        env.storage().persistent().set(&pos_key, &position);
        env.storage().persistent().extend_ttl(&pos_key, 100_000, 500_000);

        env.events()
            .publish((Symbol::new(&env, "NFTStaked"), token_id), staker);
    }

    /// Claim currently accrued rewards for a staked NFT.
    pub fn claim_rewards(env: Env, staker: Address, token_id: u32) -> i128 {
        staker.require_auth();

        let mut pos = Self::load_position(&env, token_id);
        if pos.staker != staker {
            soroban_sdk::panic_with_error!(&env, NftStakingError::Unauthorized);
        }

        let pending = Self::calculate_pending_rewards(&env, &pos);
        pos.last_claim_ledger = env.ledger().sequence();

        if pending > 0 {
            pos.total_claimed = pos
                .total_claimed
                .checked_add(pending)
                .unwrap_or_else(|| panic!("overflow"));

            let cfg = Self::load_config(&env);
            let rewards = RewardTokenClient::new(&env, &cfg.reward_token);
            let minter = env.current_contract_address();
            rewards.mint(&minter, &pos.staker, &pending);
            reward_history::record_claim(&env, &pos.staker, token_id, pending);

            env.events()
                .publish((Symbol::new(&env, "RewardsClaimed"), token_id), (pos.staker.clone(), pending));
        }

        let pos_key = DataKey::Position(token_id);
        env.storage().persistent().set(&pos_key, &pos);
        env.storage().persistent().extend_ttl(&pos_key, 100_000, 500_000);

        pending
    }

    /// Unstake an NFT after the 48h minimum and automatically claim any pending rewards.
    pub fn unstake(env: Env, staker: Address, token_id: u32) -> i128 {
        staker.require_auth();

        let cfg = Self::load_config(&env);
        let pos = Self::load_position(&env, token_id);

        if pos.staker != staker {
            soroban_sdk::panic_with_error!(&env, NftStakingError::Unauthorized);
        }

        let now = env.ledger().timestamp();
        if now < pos.staked_at || now - pos.staked_at < MIN_STAKE_SECONDS {
            soroban_sdk::panic_with_error!(&env, NftStakingError::UnstakeTooEarly);
        }

        // Claim pending rewards as part of unstake.
        let pending = Self::calculate_pending_rewards(&env, &pos);
        if pending > 0 {
            let rewards = RewardTokenClient::new(&env, &cfg.reward_token);
            let minter = env.current_contract_address();
            rewards.mint(&minter, &pos.staker, &pending);
            reward_history::record_claim(&env, &pos.staker, token_id, pending);

            env.events()
                .publish((Symbol::new(&env, "RewardsClaimed"), token_id), (pos.staker.clone(), pending));
        }

        // Return NFT to staker.
        let nft = AchievementNFTClient::new(&env, &cfg.nft_contract);
        nft.transfer(
            &env.current_contract_address(),
            &pos.staker,
            &token_id,
        );

        env.storage().persistent().remove(&DataKey::Position(token_id));
        env.events()
            .publish((Symbol::new(&env, "NFTUnstaked"), token_id), pos.staker.clone());

        pending
    }

    // ───────────── View functions ─────────────

    /// Estimate pending rewards for a staked token id at the current ledger.
    pub fn pending_rewards(env: Env, token_id: u32) -> i128 {
        let pos = Self::load_position(&env, token_id);
        Self::calculate_pending_rewards(&env, &pos)
    }

    /// Get this staker's past reward claims (token id, amount, ledger).
    pub fn get_reward_history(env: Env, staker: Address) -> Vec<reward_history::RewardClaimEntry> {
        reward_history::get_history(&env, &staker)
    }

    /// Get stake details plus pending rewards and days staked.
    pub fn get_position(env: Env, token_id: u32) -> PositionView {
        let pos = Self::load_position(&env, token_id);
        let pending = Self::calculate_pending_rewards(&env, &pos);
        let now = env.ledger().timestamp();
        let seconds = now.saturating_sub(pos.staked_at);
        let days_staked = seconds / 86_400;

        PositionView {
            position: pos,
            pending_rewards: pending,
            days_staked,
        }
    }

    // ───────────── Internal helpers ─────────────

    fn load_config(env: &Env) -> Config {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .unwrap_or_else(|| soroban_sdk::panic_with_error!(env, NftStakingError::NotInitialized))
    }

    fn require_admin(env: &Env, admin: &Address) {
        let cfg = Self::load_config(env);
        if &cfg.admin != admin {
            soroban_sdk::panic_with_error!(env, NftStakingError::Unauthorized);
        }
    }

    fn load_position(env: &Env, token_id: u32) -> NFTStakePosition {
        env.storage()
            .persistent()
            .get(&DataKey::Position(token_id))
            .unwrap_or_else(|| soroban_sdk::panic_with_error!(env, NftStakingError::PositionNotFound))
    }

    fn tokens_per_ledger(env: &Env, rarity: u32) -> i128 {
        let cfg: RarityConfig = env
            .storage()
            .persistent()
            .get(&DataKey::RarityConfig(rarity))
            .unwrap_or_else(|| soroban_sdk::panic_with_error!(env, NftStakingError::RarityNotConfigured));
        cfg.tokens_per_ledger
    }

    fn calculate_pending_rewards(env: &Env, pos: &NFTStakePosition) -> i128 {
        let current = env.ledger().sequence();
        if current <= pos.last_claim_ledger {
            return 0;
        }

        let ledgers = current - pos.last_claim_ledger;
        let rate = Self::tokens_per_ledger(env, pos.rarity);

        rate.checked_mul(ledgers as i128)
            .unwrap_or_else(|| panic!("overflow"))
    }
}

mod test;
