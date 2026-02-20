#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, token, Address, Env, Symbol, Vec,
};

//
// ──────────────────────────────────────────────────────────
// DATA STRUCTURES
// ──────────────────────────────────────────────────────────
//

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuestStatus {
    Locked,
    Unlocked,
    InProgress,
    Completed,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Quest {
    pub id: u32,
    pub puzzle_id: u32,
    pub reward: i128,
    pub status: QuestStatus,
    pub prerequisites: Vec<u32>, // Quest IDs that must be completed first
    pub branches: Vec<u32>, // Alternative quest IDs (for branching paths)
    pub checkpoint: bool, // Whether this quest saves progress
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct QuestChain {
    pub id: u32,
    pub admin: Address,
    pub title: Symbol,
    pub description: Symbol,
    pub quests: Vec<Quest>,
    pub total_reward: i128,
    pub start_time: Option<u64>, // None = no time limit
    pub end_time: Option<u64>, // None = no time limit
    pub created_at: u64,
    pub active: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PlayerProgress {
    pub player: Address,
    pub chain_id: u32,
    pub completed_quests: Vec<u32>, // Quest IDs completed
    pub current_quest: Option<u32>, // Currently active quest ID
    pub checkpoint_quest: Option<u32>, // Last checkpoint quest ID
    pub start_time: u64,
    pub completion_time: Option<u64>, // None if not completed
    pub total_reward_earned: i128,
    pub path_taken: Vec<u32>, // Sequence of quest IDs completed (for branching)
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct CompletionRecord {
    pub player: Address,
    pub chain_id: u32,
    pub completion_time: u64,
    pub duration: u64, // Time taken to complete (in seconds)
    pub path_taken: Vec<u32>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct ChainConfig {
    pub admin: Address,
    pub reward_token: Option<Address>, // Optional reward token for distributing rewards
    pub max_chains: u32,
    pub min_quests_per_chain: u32,
    pub max_quests_per_chain: u32,
}

//
// ──────────────────────────────────────────────────────────
// DATA KEYS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
pub enum DataKey {
    Config, // ChainConfig
    ChainCounter, // u32
    Chain(u32), // QuestChain
    PlayerProgress(Address, u32), // PlayerProgress - (player, chain_id)
    CompletionLeaderboard(u32), // Vec<CompletionRecord> - sorted by duration (fastest first)
    ChainCompletions(u32), // u32 - total completions for chain
    RewardPool(u32), // i128 - reward pool for chain (if using token rewards)
    PendingRewards(Address, u32), // i128 - pending rewards for player in chain
}

//
// ──────────────────────────────────────────────────────────
// CONSTANTS
// ──────────────────────────────────────────────────────────
//

const DEFAULT_MAX_CHAINS: u32 = 1000;
const DEFAULT_MIN_QUESTS: u32 = 1;
const DEFAULT_MAX_QUESTS: u32 = 100;
const MAX_LEADERBOARD_ENTRIES: u32 = 100;

//
// ──────────────────────────────────────────────────────────
// EVENTS
// ──────────────────────────────────────────────────────────
//

const CHAIN_CREATED: Symbol = symbol_short!("chain_crt");
const QUEST_UNLOCKED: Symbol = symbol_short!("qst_unlck");
const QUEST_COMPLETED: Symbol = symbol_short!("qst_done");
const CHAIN_COMPLETED: Symbol = symbol_short!("chn_done");
const PROGRESS_CHECKPOINT: Symbol = symbol_short!("checkpt");
const CHAIN_RESET: Symbol = symbol_short!("chn_reset");

//
// ──────────────────────────────────────────────────────────
// CONTRACT
// ──────────────────────────────────────────────────────────
//

#[contract]
pub struct QuestChainContract;

#[contractimpl]
impl QuestChainContract {
    // ───────────── INITIALIZATION ─────────────

    /// Initialize the quest chain contract
    ///
    /// # Arguments
    /// * `admin` - Contract administrator
    /// * `reward_token` - Optional reward token address for distributing rewards
    pub fn initialize(env: Env, admin: Address, reward_token: Option<Address>) {
        admin.require_auth();

        if env.storage().persistent().has(&DataKey::Config) {
            panic!("Already initialized");
        }

        let config = ChainConfig {
            admin,
            reward_token,
            max_chains: DEFAULT_MAX_CHAINS,
            min_quests_per_chain: DEFAULT_MIN_QUESTS,
            max_quests_per_chain: DEFAULT_MAX_QUESTS,
        };

        env.storage().persistent().set(&DataKey::Config, &config);
        env.storage().persistent().set(&DataKey::ChainCounter, &0u32);
    }

    // ───────────── CHAIN CREATION ─────────────

    /// Create a new quest chain
    ///
    /// # Arguments
    /// * `admin` - Chain creator (must be admin)
    /// * `title` - Chain title
    /// * `description` - Chain description
    /// * `quests` - Vector of quests in the chain
    /// * `start_time` - Optional start time (None for no time limit)
    /// * `end_time` - Optional end time (None for no time limit)
    pub fn create_chain(
        env: Env,
        admin: Address,
        title: Symbol,
        description: Symbol,
        quests: Vec<Quest>,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> u32 {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let config: ChainConfig = env.storage().persistent().get(&DataKey::Config).unwrap();

        if (quests.len() as u32) < config.min_quests_per_chain {
            panic!("Too few quests");
        }
        if (quests.len() as u32) > config.max_quests_per_chain {
            panic!("Too many quests");
        }

        // Validate quest structure
        Self::validate_quest_chain(&env, &quests);

        // Calculate total reward
        let mut total_reward = 0i128;
        for quest in quests.iter() {
            total_reward += quest.reward;
        }

        // Generate chain ID
        let mut counter: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ChainCounter)
            .unwrap_or(0);
        counter += 1;

        if counter > config.max_chains {
            panic!("Max chains reached");
        }

        let chain = QuestChain {
            id: counter,
            admin: admin.clone(),
            title: title.clone(),
            description: description.clone(),
            quests: quests.clone(),
            total_reward,
            start_time,
            end_time,
            created_at: env.ledger().timestamp(),
            active: true,
        };

        env.storage().persistent().set(&DataKey::ChainCounter, &counter);
        env.storage().persistent().set(&DataKey::Chain(counter), &chain);
        env.storage()
            .persistent()
            .set(&DataKey::ChainCompletions(counter), &0u32);

        // Initialize empty leaderboard
        let leaderboard: Vec<CompletionRecord> = Vec::new(&env);
        env.storage()
            .persistent()
            .set(&DataKey::CompletionLeaderboard(counter), &leaderboard);

        env.events().publish(
            (CHAIN_CREATED, counter),
            (admin, title, description, quests.len() as u32),
        );

        counter
    }

    // ───────────── QUEST PROGRESSION ─────────────

    /// Start a quest chain for a player
    ///
    /// # Arguments
    /// * `player` - Player address
    /// * `chain_id` - Chain ID to start
    pub fn start_chain(env: Env, player: Address, chain_id: u32) {
        player.require_auth();

        let chain: QuestChain = env
            .storage()
            .persistent()
            .get(&DataKey::Chain(chain_id))
            .unwrap();

        if !chain.active {
            panic!("Chain not active");
        }

        // Check time limits
        let current_time = env.ledger().timestamp();
        if let Some(start) = chain.start_time {
            if current_time < start {
                panic!("Chain not started yet");
            }
        }
        if let Some(end) = chain.end_time {
            if current_time > end {
                panic!("Chain expired");
            }
        }

        // Check if player already has progress
        if env
            .storage()
            .persistent()
            .has(&DataKey::PlayerProgress(player.clone(), chain_id))
        {
            panic!("Chain already started");
        }

        // Initialize progress
        let progress = PlayerProgress {
            player: player.clone(),
            chain_id,
            completed_quests: Vec::new(&env),
            current_quest: Self::get_initial_quest(&chain),
            checkpoint_quest: None,
            start_time: current_time,
            completion_time: None,
            total_reward_earned: 0i128,
            path_taken: Vec::new(&env),
        };

        env.storage()
            .persistent()
            .set(&DataKey::PlayerProgress(player.clone(), chain_id), &progress);
    }

    /// Complete a quest in a chain
    ///
    /// # Arguments
    /// * `player` - Player address
    /// * `chain_id` - Chain ID
    /// * `quest_id` - Quest ID to complete
    pub fn complete_quest(env: Env, player: Address, chain_id: u32, quest_id: u32) {
        player.require_auth();

        let chain: QuestChain = env
            .storage()
            .persistent()
            .get(&DataKey::Chain(chain_id))
            .unwrap();

        if !chain.active {
            panic!("Chain not active");
        }

        let mut progress: PlayerProgress = env
            .storage()
            .persistent()
            .get(&DataKey::PlayerProgress(player.clone(), chain_id))
            .unwrap();

        // Verify quest exists and is unlockable
        let quest = Self::get_quest_by_id(&chain, quest_id);
        if quest.is_none() {
            panic!("Quest not found");
        }
        let quest = quest.unwrap();

        // Check if quest is already completed
        if progress.completed_quests.contains(&quest_id) {
            panic!("Quest already completed");
        }

        // Check if quest is unlocked
        // A quest can be unlocked if:
        // 1. All prerequisites are met, OR
        // 2. Any quest in its branches field is completed (alternative unlock path)
        let prerequisites_met = Self::are_prerequisites_met(&progress, &quest.prerequisites);
        let branch_unlocked = Self::is_quest_unlocked_by_branch(&progress, &quest.branches);
        let is_current = progress.current_quest == Some(quest_id);
        
        if !prerequisites_met && !branch_unlocked && !is_current {
            panic!("Quest not unlocked");
        }

        // Mark quest as completed
        progress.completed_quests.push_back(quest_id);
        progress.path_taken.push_back(quest_id);
        progress.total_reward_earned += quest.reward;

        // Track pending rewards if reward token is configured
        let config: ChainConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        if config.reward_token.is_some() {
            let current_pending: i128 = env
                .storage()
                .persistent()
                .get(&DataKey::PendingRewards(player.clone(), chain_id))
                .unwrap_or(0);
            env.storage()
                .persistent()
                .set(&DataKey::PendingRewards(player.clone(), chain_id), &(current_pending + quest.reward));
        }

        // Save checkpoint if this quest is a checkpoint
        if quest.checkpoint {
            progress.checkpoint_quest = Some(quest_id);
            env.events().publish(
                (PROGRESS_CHECKPOINT, player.clone()),
                (chain_id, quest_id),
            );
        }

        // Determine next quest(s)
        progress.current_quest = Self::get_next_quest(&chain, &progress, quest_id);

        // Check if chain is completed
        if progress.completed_quests.len() == chain.quests.len() {
            progress.completion_time = Some(env.ledger().timestamp());
            let duration = progress.completion_time.unwrap() - progress.start_time;

            // Add to leaderboard
            Self::add_to_leaderboard(&env, chain_id, &player, duration, &progress.path_taken);

            // Update completion count
            let mut completions: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::ChainCompletions(chain_id))
                .unwrap_or(0);
            completions += 1;
            env.storage()
                .persistent()
                .set(&DataKey::ChainCompletions(chain_id), &completions);

            env.events().publish(
                (CHAIN_COMPLETED, player.clone()),
                (chain_id, duration, progress.total_reward_earned),
            );
        }

        env.storage()
            .persistent()
            .set(&DataKey::PlayerProgress(player.clone(), chain_id), &progress);

        env.events().publish(
            (QUEST_COMPLETED, player.clone()),
            (chain_id, quest_id, quest.reward),
        );
    }

    // ───────────── CHECKPOINT & RESET ─────────────

    /// Reset player progress to last checkpoint
    ///
    /// # Arguments
    /// * `player` - Player address
    /// * `chain_id` - Chain ID
    pub fn reset_to_checkpoint(env: Env, player: Address, chain_id: u32) {
        player.require_auth();

        let mut progress: PlayerProgress = env
            .storage()
            .persistent()
            .get(&DataKey::PlayerProgress(player.clone(), chain_id))
            .unwrap();

        if progress.checkpoint_quest.is_none() {
            panic!("No checkpoint available");
        }

        let checkpoint_id = progress.checkpoint_quest.unwrap();
        let chain: QuestChain = env
            .storage()
            .persistent()
            .get(&DataKey::Chain(chain_id))
            .unwrap();

        // Remove all quests completed after checkpoint
        let mut new_completed = Vec::new(&env);
        let mut new_path = Vec::new(&env);
        let mut reward_lost = 0i128;
        let mut found_checkpoint = false;

        for quest_id in progress.completed_quests.iter() {
            if quest_id == checkpoint_id {
                new_completed.push_back(quest_id);
                new_path.push_back(quest_id);
                found_checkpoint = true;
                break;
            }
            new_completed.push_back(quest_id);
            new_path.push_back(quest_id);
        }

        // Calculate lost rewards for quests after checkpoint
        if found_checkpoint {
            let mut after_checkpoint = false;
            for quest_id in progress.completed_quests.iter() {
                if quest_id == checkpoint_id {
                    after_checkpoint = true;
                    continue;
                }
                if after_checkpoint {
                    let quest = Self::get_quest_by_id(&chain, quest_id);
                    if let Some(q) = quest {
                        reward_lost += q.reward;
                    }
                }
            }
        }

        progress.completed_quests = new_completed;
        progress.path_taken = new_path;
        progress.total_reward_earned -= reward_lost;
        progress.current_quest = Self::get_next_quest(&chain, &progress, checkpoint_id);

        // Update pending rewards if reward token is configured
        let config: ChainConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        if config.reward_token.is_some() {
            let current_pending: i128 = env
                .storage()
                .persistent()
                .get(&DataKey::PendingRewards(player.clone(), chain_id))
                .unwrap_or(0);
            env.storage()
                .persistent()
                .set(&DataKey::PendingRewards(player.clone(), chain_id), &(current_pending - reward_lost));
        }

        env.storage()
            .persistent()
            .set(&DataKey::PlayerProgress(player.clone(), chain_id), &progress);

        env.events().publish(
            (CHAIN_RESET, player.clone()),
            (chain_id, checkpoint_id),
        );
    }

    /// Reset entire chain progress for a player
    ///
    /// # Arguments
    /// * `player` - Player address
    /// * `chain_id` - Chain ID
    pub fn reset_chain(env: Env, player: Address, chain_id: u32) {
        player.require_auth();

        if !env
            .storage()
            .persistent()
            .has(&DataKey::PlayerProgress(player.clone(), chain_id))
        {
            panic!("No progress to reset");
        }

        env.storage()
            .persistent()
            .remove(&DataKey::PlayerProgress(player.clone(), chain_id));

        // Clear pending rewards if any
        if env
            .storage()
            .persistent()
            .has(&DataKey::PendingRewards(player.clone(), chain_id))
        {
            env.storage()
                .persistent()
                .remove(&DataKey::PendingRewards(player.clone(), chain_id));
        }

        env.events().publish((CHAIN_RESET, player.clone()), (chain_id, 0u32));
    }

    // ───────────── VIEW FUNCTIONS ─────────────

    /// Get quest chain details
    pub fn get_chain(env: Env, chain_id: u32) -> QuestChain {
        env.storage()
            .persistent()
            .get(&DataKey::Chain(chain_id))
            .unwrap()
    }

    /// Get player progress for a chain
    pub fn get_player_progress(env: Env, player: Address, chain_id: u32) -> Option<PlayerProgress> {
        env.storage()
            .persistent()
            .get(&DataKey::PlayerProgress(player, chain_id))
    }

    /// Get completion leaderboard for a chain
    ///
    /// # Arguments
    /// * `chain_id` - Chain ID
    /// * `limit` - Maximum number of entries to return
    pub fn get_leaderboard(env: Env, chain_id: u32, limit: u32) -> Vec<CompletionRecord> {
        let leaderboard: Vec<CompletionRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::CompletionLeaderboard(chain_id))
            .unwrap_or(Vec::new(&env));

        let actual_limit = limit.min(MAX_LEADERBOARD_ENTRIES).min(leaderboard.len() as u32);
        let mut result = Vec::new(&env);

        for i in 0..actual_limit {
            result.push_back(leaderboard.get(i).unwrap());
        }

        result
    }

    /// Get total completions for a chain
    pub fn get_chain_completions(env: Env, chain_id: u32) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::ChainCompletions(chain_id))
            .unwrap_or(0)
    }

    /// Get configuration
    pub fn get_config(env: Env) -> ChainConfig {
        env.storage().persistent().get(&DataKey::Config).unwrap()
    }

    /// Get pending rewards for a player in a chain
    pub fn get_pending_rewards(env: Env, player: Address, chain_id: u32) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::PendingRewards(player, chain_id))
            .unwrap_or(0)
    }

    /// Get reward pool balance for a chain
    pub fn get_reward_pool(env: Env, chain_id: u32) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::RewardPool(chain_id))
            .unwrap_or(0)
    }

    // ───────────── REWARD DISTRIBUTION ─────────────

    /// Claim rewards for completed quests in a chain
    ///
    /// # Arguments
    /// * `player` - Player address
    /// * `chain_id` - Chain ID
    pub fn claim_rewards(env: Env, player: Address, chain_id: u32) -> i128 {
        player.require_auth();

        let config: ChainConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let reward_token = match config.reward_token {
            Some(token) => token,
            None => panic!("Reward token not configured"),
        };

        let pending: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::PendingRewards(player.clone(), chain_id))
            .unwrap_or(0);

        if pending <= 0 {
            panic!("No pending rewards");
        }

        // Check reward pool has enough
        let pool: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::RewardPool(chain_id))
            .unwrap_or(0);

        if pool < pending {
            panic!("Insufficient reward pool");
        }

        // Transfer rewards
        let token_client = token::Client::new(&env, &reward_token);
        token_client.transfer(&env.current_contract_address(), &player, &pending);

        // Update pool and pending rewards
        env.storage()
            .persistent()
            .set(&DataKey::RewardPool(chain_id), &(pool - pending));
        env.storage()
            .persistent()
            .remove(&DataKey::PendingRewards(player.clone(), chain_id));

        env.events().publish(
            (symbol_short!("rwrd_clmd"), player.clone()),
            (chain_id, pending),
        );

        pending
    }

    // ───────────── ADMIN FUNCTIONS ─────────────

    /// Update chain configuration (admin only)
    pub fn update_config(
        env: Env,
        admin: Address,
        max_chains: Option<u32>,
        min_quests: Option<u32>,
        max_quests: Option<u32>,
    ) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut config: ChainConfig =
            env.storage().persistent().get(&DataKey::Config).unwrap();

        if let Some(max) = max_chains {
            config.max_chains = max;
        }
        if let Some(min) = min_quests {
            config.min_quests_per_chain = min;
        }
        if let Some(max) = max_quests {
            config.max_quests_per_chain = max;
        }

        env.storage().persistent().set(&DataKey::Config, &config);
    }

    /// Activate or deactivate a chain (admin only)
    pub fn set_chain_active(env: Env, admin: Address, chain_id: u32, active: bool) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut chain: QuestChain = env
            .storage()
            .persistent()
            .get(&DataKey::Chain(chain_id))
            .unwrap();

        chain.active = active;
        env.storage().persistent().set(&DataKey::Chain(chain_id), &chain);
    }

    /// Set reward token for the contract (admin only)
    pub fn set_reward_token(env: Env, admin: Address, reward_token: Option<Address>) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut config: ChainConfig =
            env.storage().persistent().get(&DataKey::Config).unwrap();
        config.reward_token = reward_token;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    /// Fund reward pool for a chain (admin only)
    /// Admin must first approve the contract to spend tokens
    ///
    /// # Arguments
    /// * `admin` - Admin address
    /// * `chain_id` - Chain ID
    /// * `amount` - Amount of tokens to add to reward pool
    pub fn fund_reward_pool(env: Env, admin: Address, chain_id: u32, amount: i128) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        if amount <= 0 {
            panic!("Amount must be positive");
        }

        let config: ChainConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let reward_token = match config.reward_token {
            Some(token) => token,
            None => panic!("Reward token not configured"),
        };

        // Transfer tokens from admin to contract
        let token_client = token::Client::new(&env, &reward_token);
        token_client.transfer(&admin, &env.current_contract_address(), &amount);

        // Update reward pool
        let current_pool: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::RewardPool(chain_id))
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::RewardPool(chain_id), &(current_pool + amount));

        env.events().publish(
            (symbol_short!("pool_fund"), admin),
            (chain_id, amount, current_pool + amount),
        );
    }

    // ───────────── INTERNAL HELPERS ─────────────

    fn assert_admin(env: &Env, user: &Address) {
        let config: ChainConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        if config.admin != *user {
            panic!("Admin only");
        }
    }

    fn validate_quest_chain(env: &Env, quests: &Vec<Quest>) {
        // Check for duplicate quest IDs
        let mut seen_ids = Vec::new(env);
        for quest in quests.iter() {
            if seen_ids.contains(&quest.id) {
                panic!("Duplicate quest ID");
            }
            seen_ids.push_back(quest.id);
        }

        // Validate prerequisites reference existing quests
        for quest in quests.iter() {
            for prereq_id in quest.prerequisites.iter() {
                let mut found = false;
                for other_quest in quests.iter() {
                    if other_quest.id == prereq_id {
                        found = true;
                        break;
                    }
                }
                if !found {
                    panic!("Invalid prerequisite");
                }
            }

            // Validate branches reference existing quests
            for branch_id in quest.branches.iter() {
                let mut found = false;
                for other_quest in quests.iter() {
                    if other_quest.id == branch_id {
                        found = true;
                        break;
                    }
                }
                if !found {
                    panic!("Invalid branch");
                }
            }
        }
    }

    fn get_quest_by_id(chain: &QuestChain, quest_id: u32) -> Option<Quest> {
        for quest in chain.quests.iter() {
            if quest.id == quest_id {
                return Some(quest.clone());
            }
        }
        None
    }

    fn get_initial_quest(chain: &QuestChain) -> Option<u32> {
        // Find quest with no prerequisites
        for quest in chain.quests.iter() {
            if quest.prerequisites.len() == 0 {
                return Some(quest.id);
            }
        }
        None
    }

    fn are_prerequisites_met(progress: &PlayerProgress, prerequisites: &Vec<u32>) -> bool {
        for prereq_id in prerequisites.iter() {
            if !progress.completed_quests.contains(prereq_id) {
                return false;
            }
        }
        true
    }

    fn is_quest_unlocked_by_branch(progress: &PlayerProgress, branches: &Vec<u32>) -> bool {
        // Check if any quest in the branches vector is completed
        // This allows alternative unlock paths
        for branch_id in branches.iter() {
            if progress.completed_quests.contains(branch_id) {
                return true;
            }
        }
        false
    }

    fn get_next_quest(chain: &QuestChain, progress: &PlayerProgress, completed_id: u32) -> Option<u32> {
        let completed_quest = Self::get_quest_by_id(chain, completed_id);
        if completed_quest.is_none() {
            return None;
        }

        let quest = completed_quest.unwrap();

        // Check branches first (alternative paths from this quest)
        for branch_id in quest.branches.iter() {
            if !progress.completed_quests.contains(branch_id) {
                return Some(branch_id);
            }
        }

        // Find quests that have this quest in their branches (alternative unlock)
        for other_quest in chain.quests.iter() {
            if other_quest.branches.contains(&completed_id)
                && !progress.completed_quests.contains(&other_quest.id)
            {
                // Check if prerequisites are met or if it's unlocked by branch
                let prereqs_met = Self::are_prerequisites_met(progress, &other_quest.prerequisites);
                let branch_unlocked = Self::is_quest_unlocked_by_branch(progress, &other_quest.branches);
                if prereqs_met || branch_unlocked {
                    return Some(other_quest.id);
                }
            }
        }

        // Find next sequential quest (quest that has this one as prerequisite)
        for other_quest in chain.quests.iter() {
            if other_quest.prerequisites.contains(&completed_id)
                && !progress.completed_quests.contains(&other_quest.id)
            {
                return Some(other_quest.id);
            }
        }

        None
    }

    fn add_to_leaderboard(
        env: &Env,
        chain_id: u32,
        player: &Address,
        duration: u64,
        path: &Vec<u32>,
    ) {
        let leaderboard: Vec<CompletionRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::CompletionLeaderboard(chain_id))
            .unwrap_or(Vec::new(env));

        let record = CompletionRecord {
            player: player.clone(),
            chain_id,
            completion_time: env.ledger().timestamp(),
            duration,
            path_taken: path.clone(),
        };

        // Insert in sorted order (fastest first)
        let mut inserted = false;
        let mut new_leaderboard = Vec::new(env);

        for existing in leaderboard.iter() {
            if !inserted && duration < existing.duration {
                new_leaderboard.push_back(record.clone());
                inserted = true;
            }
            if (new_leaderboard.len() as u32) < MAX_LEADERBOARD_ENTRIES {
                new_leaderboard.push_back(existing);
            }
        }

        if !inserted && (new_leaderboard.len() as u32) < MAX_LEADERBOARD_ENTRIES {
            new_leaderboard.push_back(record);
        }

        env.storage()
            .persistent()
            .set(&DataKey::CompletionLeaderboard(chain_id), &new_leaderboard);
    }
}

mod test;
