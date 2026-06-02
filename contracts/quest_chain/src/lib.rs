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
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenType {
    Native,
    ERC20,
    ERC721,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reward {
    pub token_type: TokenType,
    pub token_address: Option<Address>,
    pub amount: i128, // Also used for tokenId in ERC721
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Quest {
    pub id: u32,
    pub puzzle_id: u32,
    pub rewards: Vec<Reward>,
    pub status: QuestStatus,
    pub prerequisites: Vec<u32>, // Quest IDs that must be completed first
    pub branches: Vec<u32>,      // Alternative quest IDs (for branching paths)
    pub checkpoint: bool,        // Whether this quest saves progress
    pub expires_at: Option<u64>, // Optional expiry timestamp; None = no deadline
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct QuestChain {
    pub id: u32,
    pub admin: Address,
    pub title: Symbol,
    pub description: Symbol,
    pub quests: Vec<Quest>,
    pub start_time: Option<u64>, // None = no time limit
    pub end_time: Option<u64>, // None = no time limit
    pub max_participants: Option<u32>, // None = no participant limit
    pub created_at: u64,
    pub active: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PlayerProgress {
    pub player: Address,
    pub chain_id: u32,
    pub completed_quests: Vec<u32>,    // Quest IDs completed
    pub current_quest: Option<u32>,    // Currently active quest ID
    pub checkpoint_quest: Option<u32>, // Last checkpoint quest ID
    pub start_time: u64,
    pub completion_time: Option<u64>, // None if not completed
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
    pub owner: Address,
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
    Config,                       // ChainConfig
    ChainCounter,                 // u32
    Chain(u32),                   // QuestChain
    PlayerProgress(Address, u32), // PlayerProgress - (player, chain_id)
    CompletionLeaderboard(u32),   // Vec<CompletionRecord> - sorted by duration (fastest first)
    ChainCompletions(u32),        // u32 - total completions for chain
    RewardPool(u32, TokenType, Option<Address>),              // i128 - reward pool for chain (if using token rewards)
    PendingRewards(Address, u32), // Vec<Reward> - pending rewards for player in chain
    QuestRatings(u32), // Vec<u32> - ratings for a specific quest
    PlayerRatedQuest(Address, u32), // bool - tracks if a player has rated a specific quest
    Manager(Address),             // bool - manager role assignment
    Moderator(Address),           // bool - moderator role assignment
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
const CHAIN_STARTED: Symbol = symbol_short!("chn_start");
const QUEST_UNLOCKED: Symbol = symbol_short!("qst_unlck");
const QUEST_COMPLETED: Symbol = symbol_short!("qst_done");
const QUEST_EXPIRED: Symbol = symbol_short!("qst_exprd");
const CHAIN_COMPLETED: Symbol = symbol_short!("chn_done");
const PROGRESS_CHECKPOINT: Symbol = symbol_short!("checkpt");
const CHAIN_RESET: Symbol = symbol_short!("chn_reset");
const REWARD_CLAIMED: Symbol = symbol_short!("rwrd_clmd");
const POOL_FUNDED: Symbol = symbol_short!("pool_fund");

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
    /// * `owner` - Contract owner
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();

        if env.storage().persistent().has(&DataKey::Config) {
            panic!("Already initialized");
        }

        let config = ChainConfig {
            owner,
            max_chains: DEFAULT_MAX_CHAINS,
            min_quests_per_chain: DEFAULT_MIN_QUESTS,
            max_quests_per_chain: DEFAULT_MAX_QUESTS,
        };

        env.storage().persistent().set(&DataKey::Config, &config);
        env.storage()
            .persistent()
            .set(&DataKey::ChainCounter, &0u32);
    }

    // ───────────── CHAIN CREATION ─────────────

    /// Create a new quest chain
    ///
    /// # Arguments
    /// * `admin` - Chain creator (must be owner or manager)
    /// * `title` - Chain title
    /// * `description` - Chain description
    /// * `quests` - Vector of quests in the chain
    /// * `start_time` - Optional start time (None for no time limit)
    /// * `end_time` - Optional end time (None for no time limit)
    /// * `max_participants` - Optional maximum participants (None for no limit)
    pub fn create_chain(
        env: Env,
        admin: Address,
        title: Symbol,
        description: Symbol,
        quests: Vec<Quest>,
        start_time: Option<u64>,
        end_time: Option<u64>,
        max_participants: Option<u32>,
    ) -> u32 {
        admin.require_auth();
        Self::assert_owner_or_manager(&env, &admin);

        let config: ChainConfig = env.storage().persistent().get(&DataKey::Config).unwrap();

        if (quests.len() as u32) < config.min_quests_per_chain {
            panic!("Too few quests");
        }
        if (quests.len() as u32) > config.max_quests_per_chain {
            panic!("Too many quests");
        }

        // Validate quest structure
        Self::validate_quest_chain(&env, &quests);

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
            start_time,
            end_time,
            max_participants,
            created_at: env.ledger().timestamp(),
            active: true,
        };

        env.storage()
            .persistent()
            .set(&DataKey::ChainCounter, &counter);
        env.storage()
            .persistent()
            .set(&DataKey::Chain(counter), &chain);
        env.storage()
            .persistent()
            .set(&DataKey::ChainCompletions(counter), &0u32);
        env.storage()
            .persistent()
            .set(&DataKey::ChainParticipants(counter), &0u32);

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

        // Check participant limit
        if let Some(max) = chain.max_participants {
            let current_participants: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::ChainParticipants(chain_id))
                .unwrap_or(0);
            if current_participants >= max {
                panic!("Participant limit reached");
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
            path_taken: Vec::new(&env),
        };

        env.storage()
            .persistent()
            .set(&DataKey::PlayerProgress(player.clone(), chain_id), &progress);

        // Increment participant count
        let mut participant_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ChainParticipants(chain_id))
            .unwrap_or(0);
        participant_count += 1;
        env.storage()
            .persistent()
            .set(&DataKey::ChainParticipants(chain_id), &participant_count);
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

        // Enforce quest expiry deadline
        if let Some(expiry_timestamp) = quest.expiry_timestamp {
            let current_time = env.ledger().timestamp();
            if current_time >= expiry_timestamp {
                env.events().publish(
                    (QUEST_EXPIRED, player.clone()),
                    (chain_id, quest_id, expiry_timestamp),
                );
                panic!("Quest: expired");
            }
        }

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
        
        let mut pending_rewards: Vec<Reward> = env
            .storage()
            .persistent()
            .get(&DataKey::PendingRewards(player.clone(), chain_id))
            .unwrap_or_else(|| Vec::new(&env));

        for reward in quest.rewards.iter() {
            pending_rewards.push_back(reward);
        }

        env.storage().persistent().set(
            &DataKey::PendingRewards(player.clone(), chain_id),
            &pending_rewards,
        );

        // Save checkpoint if this quest is a checkpoint
        if quest.checkpoint {
            progress.checkpoint_quest = Some(quest_id);
            env.events().publish(
                (PROGRESS_CHECKPOINT, player.clone(), chain_id),
                (quest_id,),
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
                (CHAIN_COMPLETED, player.clone(), chain_id),
                (duration,),
            );
        }

        env.storage().persistent().set(
            &DataKey::PlayerProgress(player.clone(), chain_id),
            &progress,
        );

        env.events().publish(
            (QUEST_COMPLETED, player.clone(), chain_id),
            (quest_id, quest.reward),
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
        progress.current_quest = Self::get_next_quest(&chain, &progress, checkpoint_id);

        // Update pending rewards - recalculting for simplicity
        let mut updated_pending = Vec::new(&env);
        for qid in progress.completed_quests.iter() {
            if let Some(q) = Self::get_quest_by_id(&chain, qid) {
                for r in q.rewards.iter() {
                    updated_pending.push_back(r);
                }
            }
        }
        env.storage().persistent().set(
            &DataKey::PendingRewards(player.clone(), chain_id),
            &updated_pending,
        );

        env.storage().persistent().set(
            &DataKey::PlayerProgress(player.clone(), chain_id),
            &progress,
        env.storage()
            .persistent()
            .set(&DataKey::PlayerProgress(player.clone(), chain_id), &progress);

        env.events().publish(
            (CHAIN_RESET, player.clone(), chain_id),
            (checkpoint_id,),
        );

        env.events()
            .publish((CHAIN_RESET, player.clone()), (chain_id, checkpoint_id));
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

        // Decrement participant count
        let mut participant_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ChainParticipants(chain_id))
            .unwrap_or(0);
        if participant_count > 0 {
            participant_count -= 1;
            env.storage()
                .persistent()
                .set(&DataKey::ChainParticipants(chain_id), &participant_count);
        }

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

        env.events()
            .publish((CHAIN_RESET, player.clone()), (chain_id, 0u32));
        env.events().publish((CHAIN_RESET, player.clone(), chain_id), (0u32,));
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

        let actual_limit = limit
            .min(MAX_LEADERBOARD_ENTRIES)
            .min(leaderboard.len() as u32);
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

    /// Get current participant count for a chain
    pub fn get_chain_participants(env: Env, chain_id: u32) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::ChainParticipants(chain_id))
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

    // ───────────── QUEST RATINGS ─────────────

    /// Rate a quest that the player has completed
    ///
    /// # Arguments
    /// * `player` - Player address
    /// * `quest_id` - Quest ID to rate
    /// * `rating` - Rating value (1-5)
    pub fn rate_quest(env: Env, player: Address, quest_id: u32, rating: u32) {
        player.require_auth();

        // Validate rating is between 1 and 5
        if rating < 1 || rating > 5 {
            panic!("Rating must be between 1 and 5");
        }

        // Check if player has already rated this quest
        if env
            .storage()
            .persistent()
            .has(&DataKey::PlayerRatedQuest(player.clone(), quest_id))
        {
            panic!("Already rated this quest");
        }

        // Check if player has completed this quest in any chain
        let mut has_completed = false;
        let chain_counter: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ChainCounter)
            .unwrap_or(0);

        for chain_id in 1..=chain_counter {
            if let Some(progress) = env
                .storage()
                .persistent()
                .get::<DataKey, PlayerProgress>(&DataKey::PlayerProgress(player.clone(), chain_id))
            {
                if progress.completed_quests.contains(&quest_id) {
                    has_completed = true;
                    break;
                }
            }
        }

        if !has_completed {
            panic!("Must complete quest before rating");
        }

        // Add rating to quest ratings
        let mut ratings: Vec<u32> = env
            .storage()
            .persistent()
            .get(&DataKey::QuestRatings(quest_id))
            .unwrap_or(Vec::new(&env));
        ratings.push_back(rating);
        env.storage()
            .persistent()
            .set(&DataKey::QuestRatings(quest_id), &ratings);

        // Mark player as having rated this quest
        env.storage()
            .persistent()
            .set(&DataKey::PlayerRatedQuest(player.clone(), quest_id), &true);

        env.events()
            .publish((QUEST_RATED, player.clone()), (quest_id, rating));
    }

    /// Get the average rating for a quest
    ///
    /// # Arguments
    /// * `quest_id` - Quest ID
    ///
    /// # Returns
    /// Average rating (0 if no ratings)
    pub fn get_average_rating(env: Env, quest_id: u32) -> u32 {
        let ratings: Vec<u32> = env
            .storage()
            .persistent()
            .get(&DataKey::QuestRatings(quest_id))
            .unwrap_or(Vec::new(&env));

        if ratings.is_empty() {
            return 0;
        }

        let mut sum: u32 = 0;
        for rating in ratings.iter() {
            sum += rating;
        }

        sum / ratings.len()
    }

    /// Get all ratings for a quest
    ///
    /// # Arguments
    /// * `quest_id` - Quest ID
    pub fn get_quest_ratings(env: Env, quest_id: u32) -> Vec<u32> {
        env.storage()
            .persistent()
            .get(&DataKey::QuestRatings(quest_id))
            .unwrap_or(Vec::new(&env))
    }

    /// Check if a player has rated a specific quest
    ///
    /// # Arguments
    /// * `player` - Player address
    /// * `quest_id` - Quest ID
    pub fn has_player_rated_quest(env: Env, player: Address, quest_id: u32) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::PlayerRatedQuest(player, quest_id))
            .unwrap_or(false)
    }

    // ───────────── REWARD DISTRIBUTION ─────────────

    /// Claim rewards for completed quests in a chain
    ///
    /// # Arguments
    /// * `player` - Player address
    /// * `chain_id` - Chain ID
    pub fn claim_rewards(env: Env, player: Address, chain_id: u32) {
        player.require_auth();

        let pending: Vec<Reward> = env
            .storage()
            .persistent()
            .get(&DataKey::PendingRewards(player.clone(), chain_id))
            .unwrap_or_else(|| Vec::new(&env));

        if pending.is_empty() {
            panic!("No pending rewards");
        }

        for reward in pending.iter() {
            let pool_key = DataKey::RewardPool(chain_id, reward.token_type.clone(), reward.token_address.clone());
            let pool_balance: i128 = env.storage().persistent().get(&pool_key).unwrap_or(0);

            if pool_balance < reward.amount {
                panic!("Insufficient reward pool for token");
            }

            match reward.token_type {
                TokenType::Native => {
                    let token_client = token::Client::new(&env, &env.current_contract_address()); // Placeholder for native if needed, but usually it's a specific SAC address
                    // In Soroban, Native XLM has a specific address. If token_address is provided, use it.
                    if let Some(addr) = reward.token_address {
                        let client = token::Client::new(&env, &addr);
                        client.transfer(&env.current_contract_address(), &player, &reward.amount);
                    } else {
                        panic!("Native token address required");
                    }
                }
                TokenType::ERC20 => {
                    let addr = reward.token_address.expect("ERC20 address required");
                    let client = token::Client::new(&env, &addr);
                    client.transfer(&env.current_contract_address(), &player, &reward.amount);
                }
                TokenType::ERC721 => {
                    let addr = reward.token_address.expect("ERC721 address required");
                    // Assuming ERC721 also uses 'transfer' with tokenId as amount
                    // or we need a different interface.
                    env.invoke_contract::<()>(
                        &addr,
                        &Symbol::new(&env, "transfer"),
                        soroban_sdk::vec![&env, env.current_contract_address().into_val(&env), player.clone().into_val(&env), reward.amount.into_val(&env)]
                    );
                }
            }

            env.storage().persistent().set(&pool_key, &(pool_balance - reward.amount));
        }

        env.storage()
            .persistent()
            .remove(&DataKey::PendingRewards(player.clone(), chain_id));

        env.events().publish(
            (REWARD_CLAIMED, player.clone(), chain_id),
            (),
        );
    }

        pending
    }

    // ───────────── ADMIN FUNCTIONS ─────────────

    /// Update chain configuration (owner only)
    pub fn update_config(
        env: Env,
        admin: Address,
        max_chains: Option<u32>,
        min_quests: Option<u32>,
        max_quests: Option<u32>,
    ) {
        admin.require_auth();
        Self::assert_owner(&env, &admin);

        let mut config: ChainConfig = env.storage().persistent().get(&DataKey::Config).unwrap();

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

    /// Activate or deactivate a chain (owner, manager, or moderator only)
    pub fn set_chain_active(env: Env, admin: Address, chain_id: u32, active: bool) {
        admin.require_auth();
        Self::assert_owner_manager_or_moderator(&env, &admin);

        let mut chain: QuestChain = env
            .storage()
            .persistent()
            .get(&DataKey::Chain(chain_id))
            .unwrap();

        chain.active = active;
        env.storage()
            .persistent()
            .set(&DataKey::Chain(chain_id), &chain);
    }

    /// Fund reward pool for a chain (owner only)
    /// Admin must first approve the contract to spend tokens
    ///
    /// # Arguments
    /// * `admin` - Admin address
    /// * `chain_id` - Chain ID
    /// * `token_address` - Token address to fund
    /// * `token_type` - Type of token
    /// * `amount` - Amount of tokens to add to reward pool
    pub fn fund_reward_pool(env: Env, admin: Address, chain_id: u32, token_address: Address, token_type: TokenType, amount: i128) {
        admin.require_auth();
        Self::assert_owner(&env, &admin);

        if amount <= 0 {
            panic!("Amount must be positive");
        }

        match token_type {
            TokenType::Native | TokenType::ERC20 => {
                let token_client = token::Client::new(&env, &token_address);
                token_client.transfer(&admin, &env.current_contract_address(), &amount);
            },
            TokenType::ERC721 => {
                // For ERC721, amount is the tokenId
                env.invoke_contract::<()>(
                    &token_address,
                    &Symbol::new(&env, "transfer"),
                    soroban_sdk::vec![&env, admin.clone().into_val(&env), env.current_contract_address().into_val(&env), amount.into_val(&env)]
                );
            }
        }

        // Update reward pool
        let pool_key = DataKey::RewardPool(chain_id, token_type, Some(token_address.clone()));
        let current_pool: i128 = env
            .storage()
            .persistent()
            .get(&pool_key)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&pool_key, &(current_pool + amount));

        env.events().publish(
            (POOL_FUNDED, admin, chain_id),
            (amount, token_address),
        );
    }

    /// Assign manager role to an address (owner only)
    pub fn assign_manager(env: Env, owner: Address, manager: Address) {
        owner.require_auth();
        Self::assert_owner(&env, &owner);

        env.storage()
            .persistent()
            .set(&DataKey::Manager(manager.clone()), &true);

        env.events()
            .publish((symbol_short!("mgr_add"), owner), manager);
    }

    /// Revoke manager role from an address (owner only)
    pub fn revoke_manager(env: Env, owner: Address, manager: Address) {
        owner.require_auth();
        Self::assert_owner(&env, &owner);

        env.storage()
            .persistent()
            .remove(&DataKey::Manager(manager.clone()));

        env.events()
            .publish((symbol_short!("mgr_rm"), owner), manager);
    }

    /// Assign moderator role to an address (owner only)
    pub fn assign_moderator(env: Env, owner: Address, moderator: Address) {
        owner.require_auth();
        Self::assert_owner(&env, &owner);

        env.storage()
            .persistent()
            .set(&DataKey::Moderator(moderator.clone()), &true);

        env.events()
            .publish((symbol_short!("mod_add"), owner), moderator);
    }

    /// Revoke moderator role from an address (owner only)
    pub fn revoke_moderator(env: Env, owner: Address, moderator: Address) {
        owner.require_auth();
        Self::assert_owner(&env, &owner);

        env.storage()
            .persistent()
            .remove(&DataKey::Moderator(moderator.clone()));

        env.events()
            .publish((symbol_short!("mod_rm"), owner), moderator);
    }

    /// Check whether an address has manager privileges
    pub fn is_manager(env: Env, user: Address) -> bool {
        Self::is_owner(&env, &user) || Self::has_manager_role(&env, &user)
    }

    /// Check whether an address has moderator privileges
    pub fn is_moderator(env: Env, user: Address) -> bool {
        Self::has_moderator_role(&env, &user)
    }

    // ───────────── INTERNAL HELPERS ─────────────

    fn assert_owner(env: &Env, user: &Address) {
        let config: ChainConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        if config.owner != *user {
            panic!("Owner only");
        }
    }

    fn assert_owner_or_manager(env: &Env, user: &Address) {
        if !Self::is_owner(env, user) && !Self::has_manager_role(env, user) {
            panic!("Manager only");
        }
    }

    fn assert_owner_manager_or_moderator(env: &Env, user: &Address) {
        if !Self::is_owner(env, user)
            && !Self::has_manager_role(env, user)
            && !Self::has_moderator_role(env, user)
        {
            panic!("Manager or moderator only");
        }
    }

    fn is_owner(env: &Env, user: &Address) -> bool {
        let config: ChainConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        config.owner == *user
    }

    fn has_manager_role(env: &Env, user: &Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Manager(user.clone()))
            .unwrap_or(false)
    }

    fn has_moderator_role(env: &Env, user: &Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Moderator(user.clone()))
            .unwrap_or(false)
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

    fn get_next_quest(
        chain: &QuestChain,
        progress: &PlayerProgress,
        completed_id: u32,
    ) -> Option<u32> {
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
                let branch_unlocked =
                    Self::is_quest_unlocked_by_branch(progress, &other_quest.branches);
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
