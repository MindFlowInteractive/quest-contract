#![no_std]

//! Quest contract.
//!
//! Provides on-chain quest creation with:
//! - Category/tag support
//! - Difficulty tiers with reward multipliers (issue #238)
//! - Duplicate reward claim prevention (issue #239)
//! - Batch quest creation (issue #237)

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, Vec};

mod events;

#[cfg(test)]
mod test;

//
// ──────────────────────────────────────────────────────────
// DATA STRUCTURES
// ──────────────────────────────────────────────────────────
//

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuestStatus {
    Active,
    Completed,
    Cancelled,
}

/// Difficulty tier for a quest.  Set at creation time and immutable afterwards.
/// Multipliers applied at payout: Easy=1x, Medium=1.5x, Hard=2x, Legendary=3x.
/// Stored as integer numerators over a denominator of 2 to avoid floating point:
///   Easy=2, Medium=3, Hard=4, Legendary=6  (divide by 2 to get the multiplier).
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Difficulty {
    Easy = 0,
    Medium = 1,
    Hard = 2,
    Legendary = 3,
}

impl Difficulty {
    /// Returns the reward multiplier numerator (denominator is always 2).
    /// Easy → 2/2 = 1x, Medium → 3/2 = 1.5x, Hard → 4/2 = 2x, Legendary → 6/2 = 3x.
    pub fn multiplier_numerator(self) -> i128 {
        match self {
            Difficulty::Easy => 2,
            Difficulty::Medium => 3,
            Difficulty::Hard => 4,
            Difficulty::Legendary => 6,
        }
    }

    /// Apply the difficulty multiplier to a base reward amount.
    pub fn apply_to(self, base_reward: i128) -> i128 {
        base_reward * self.multiplier_numerator() / 2
    }
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
    pub id: u64,
    pub creator: Address,
    pub title: String,
    pub description: String,
    /// Categories/tags assigned to the quest at creation time.
    pub tags: Vec<String>,
    pub rewards: Vec<Reward>,
    pub reward: i128,
    pub difficulty: Difficulty,
    pub status: QuestStatus,
    pub created_at: u64,
}

/// Input type used for batch quest creation (issue #237).
#[contracttype]
#[derive(Clone, Debug)]
pub struct QuestInput {
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub reward: i128,
    pub difficulty: Difficulty,
    pub rewards: Vec<Reward>,
}

//
// ──────────────────────────────────────────────────────────
// DATA KEYS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
pub enum DataKey {
    /// Auto-incrementing quest id counter.
    QuestCounter,
    /// Stored Quest by id.
    Quest(u64),
    /// Index of quest ids per tag, for efficient lookup.
    TagIndex(String),
    /// Authorized admin address (can pause/unpause and is set on init).
    Admin,
    /// Whether the contract is currently paused (true = paused).
    Paused,
    /// Tracks whether a participant has already claimed a quest reward.
    /// Key: (quest_id, participant_address) → bool  (issue #239)
    Claimed(u64, Address),
}

//
// ──────────────────────────────────────────────────────────
// ERRORS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum QuestError {
    QuestNotFound = 1,
    EmptyTitle = 2,
    TooManyTags = 3,
    EmptyTag = 4,
    NotInitialized = 5,
    AlreadyInitialized = 6,
    Unauthorized = 7,
    Paused = 8,
    NotPaused = 9,
    /// Returned when a participant tries to claim a reward they already claimed.
    AlreadyClaimed = 10,
    /// Returned when the batch input vector is empty.
    EmptyBatch = 11,
}

const MAX_TAGS_PER_QUEST: u32 = 10;

//
// ──────────────────────────────────────────────────────────
// CONTRACT
// ──────────────────────────────────────────────────────────
//

#[contract]
pub struct QuestContract;

#[contractimpl]
impl QuestContract {
    /// One-time initialization: sets the authorized admin who can pause/unpause.
    /// Subsequent calls panic with `AlreadyInitialized`.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error(&env, QuestError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
        events::contract_initialized(&env, admin);
    }

    /// Pause the contract. Only the admin may call this. While paused,
    /// state-mutating ("sensitive") functions like `create_quest` are
    /// rejected; read-only getters remain available.
    pub fn pause(env: Env, caller: Address) {
        require_admin(&env, &caller);
        if is_paused_internal(&env) {
            panic_with_error(&env, QuestError::Paused);
        }
        env.storage().instance().set(&DataKey::Paused, &true);
        events::contract_paused(&env, caller);
    }

    /// Resume normal operation. Only the admin may call this.
    pub fn unpause(env: Env, caller: Address) {
        require_admin(&env, &caller);
        if !is_paused_internal(&env) {
            panic_with_error(&env, QuestError::NotPaused);
        }
        env.storage().instance().set(&DataKey::Paused, &false);
        events::contract_unpaused(&env, caller);
    }

    /// Returns whether the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        is_paused_internal(&env)
    }

    /// Returns the configured admin address. Panics if the contract has not
    /// been initialized.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error(&env, QuestError::NotInitialized))
    }

    // ──────────────────────────────────────────────────────
    // QUEST CREATION  (issues #237, #238)
    // ──────────────────────────────────────────────────────

    /// Create a new quest with an optional list of tags/categories and a
    /// difficulty tier.  The effective payout is `reward * difficulty_multiplier`.
    ///
    /// Sensitive: rejected while the contract is paused.
    pub fn create_quest(
        env: Env,
        creator: Address,
        title: String,
        description: String,
        tags: Vec<String>,
        rewards: Vec<Reward>,
        reward: i128,
        difficulty: Difficulty,
    ) -> u64 {
        require_not_paused(&env);
        creator.require_auth();

        if title.is_empty() {
            panic_with_error(&env, QuestError::EmptyTitle);
        }
        if tags.len() > MAX_TAGS_PER_QUEST {
            panic_with_error(&env, QuestError::TooManyTags);
        }
        for i in 0..tags.len() {
            let t = tags.get(i).unwrap();
            if t.is_empty() {
                panic_with_error(&env, QuestError::EmptyTag);
            }
        }

        let id = next_quest_id(&env);

        let quest = Quest {
            id,
            creator: creator.clone(),
            title,
            description,
            tags: tags.clone(),
            rewards,
            reward,
            difficulty,
            status: QuestStatus::Active,
            created_at: env.ledger().timestamp(),
        };

        save_quest(&env, &quest);
        index_tags(&env, id, &tags);

        events::quest_created(&env, id, creator);

        id
    }

    /// Batch-create multiple quests in a single transaction (issue #237).
    ///
    /// Accepts a `Vec<QuestInput>` and creates each quest atomically.
    /// If any individual quest fails validation the entire transaction reverts,
    /// so partial failure is impossible.
    ///
    /// Returns the list of newly created quest IDs in the same order as the
    /// input vector.
    ///
    /// Sensitive: rejected while the contract is paused.
    pub fn create_quest_batch(env: Env, creator: Address, quests: Vec<QuestInput>) -> Vec<u64> {
        require_not_paused(&env);
        creator.require_auth();

        if quests.is_empty() {
            panic_with_error(&env, QuestError::EmptyBatch);
        }

        // Validate all inputs up-front before writing anything.
        for i in 0..quests.len() {
            let q = quests.get(i).unwrap();
            if q.title.is_empty() {
                panic_with_error(&env, QuestError::EmptyTitle);
            }
            if q.tags.len() > MAX_TAGS_PER_QUEST {
                panic_with_error(&env, QuestError::TooManyTags);
            }
            for j in 0..q.tags.len() {
                let t = q.tags.get(j).unwrap();
                if t.is_empty() {
                    panic_with_error(&env, QuestError::EmptyTag);
                }
            }
        }

        // All inputs are valid — now persist them.
        let mut ids: Vec<u64> = Vec::new(&env);
        for i in 0..quests.len() {
            let q = quests.get(i).unwrap();
            let id = next_quest_id(&env);

            let quest = Quest {
                id,
                creator: creator.clone(),
                title: q.title,
                description: q.description,
                tags: q.tags.clone(),
                rewards: q.rewards.clone(),
                reward: q.reward,
                difficulty: q.difficulty,
                status: QuestStatus::Active,
                created_at: env.ledger().timestamp(),
            };

            save_quest(&env, &quest);
            index_tags(&env, id, &q.tags);
            events::quest_created(&env, id, creator.clone());
            ids.push_back(id);
        }

        ids
    }

    // ──────────────────────────────────────────────────────
    // REWARD CLAIMING  (issue #239)
    // ──────────────────────────────────────────────────────

    /// Record that `participant` has claimed the reward for `quest_id`.
    ///
    /// Panics with `AlreadyClaimed` if the participant has already claimed.
    /// Returns the effective reward amount after applying the difficulty
    /// multiplier so callers can use it for token transfers.
    ///
    /// NOTE: This function records the claim on-chain but does NOT transfer
    /// tokens itself — token transfer is the responsibility of the calling
    /// contract or off-chain system.  The returned amount is the authoritative
    /// figure to use.
    pub fn claim_reward(env: Env, participant: Address, quest_id: u64) -> i128 {
        require_not_paused(&env);
        participant.require_auth();

        // Verify quest exists.
        let quest: Quest = env
            .storage()
            .persistent()
            .get(&DataKey::Quest(quest_id))
            .unwrap_or_else(|| panic_with_error(&env, QuestError::QuestNotFound));

        // Prevent duplicate claims (issue #239).
        let claim_key = DataKey::Claimed(quest_id, participant.clone());
        if env.storage().persistent().has(&claim_key) {
            panic_with_error(&env, QuestError::AlreadyClaimed);
        }

        // Mark as claimed in persistent storage.
        env.storage().persistent().set(&claim_key, &true);

        // Compute effective reward with difficulty multiplier.
        let effective_reward = quest.difficulty.apply_to(quest.reward);

        events::quest_completed(&env, quest_id, participant);

        effective_reward
    }

    /// Mark a quest as completed by `participant`.
    ///
    /// Sensitive: rejected while the contract is paused.
    pub fn complete_quest(env: Env, participant: Address, quest_id: u64) {
        require_not_paused(&env);
        participant.require_auth();

        let mut quest: Quest = env
            .storage()
            .persistent()
            .get(&DataKey::Quest(quest_id))
            .unwrap_or_else(|| panic_with_error(&env, QuestError::QuestNotFound));

        quest.status = QuestStatus::Completed;
        save_quest(&env, &quest);

        events::quest_completed(&env, quest_id, participant);
    }

    /// Returns whether `participant` has already claimed the reward for
    /// `quest_id`.
    pub fn has_claimed(env: Env, participant: Address, quest_id: u64) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::Claimed(quest_id, participant))
    }

    // ──────────────────────────────────────────────────────
    // QUERIES
    // ──────────────────────────────────────────────────────

    /// Retrieve a quest by id. Panics if not found.
    pub fn get_quest(env: Env, quest_id: u64) -> Quest {
        env.storage()
            .persistent()
            .get(&DataKey::Quest(quest_id))
            .unwrap_or_else(|| panic_with_error(&env, QuestError::QuestNotFound))
    }

    /// Retrieve all quests carrying a given tag/category.
    pub fn get_quests_by_tag(env: Env, tag: String) -> Vec<Quest> {
        let ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::TagIndex(tag))
            .unwrap_or_else(|| Vec::new(&env));

        let mut quests: Vec<Quest> = Vec::new(&env);
        for i in 0..ids.len() {
            let qid = ids.get(i).unwrap();
            if let Some(q) = env
                .storage()
                .persistent()
                .get::<DataKey, Quest>(&DataKey::Quest(qid))
            {
                quests.push_back(q);
            }
        }
        quests
    }

    /// Return the full tag list assigned to a quest.
    pub fn get_quest_tags(env: Env, quest_id: u64) -> Vec<String> {
        Self::get_quest(env, quest_id).tags
    }

    /// Return the ids of all quests for a tag (useful for paginating).
    pub fn get_quest_ids_by_tag(env: Env, tag: String) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::TagIndex(tag))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Return the effective reward for a quest after applying the difficulty
    /// multiplier.
    pub fn effective_reward(env: Env, quest_id: u64) -> i128 {
        let quest = Self::get_quest(env, quest_id);
        quest.difficulty.apply_to(quest.reward)
    }
}

//
// ──────────────────────────────────────────────────────────
// HELPERS
// ──────────────────────────────────────────────────────────
//

fn next_quest_id(env: &Env) -> u64 {
    let current: u64 = env
        .storage()
        .instance()
        .get(&DataKey::QuestCounter)
        .unwrap_or(0);
    let next = current + 1;
    env.storage().instance().set(&DataKey::QuestCounter, &next);
    next
}

fn save_quest(env: &Env, quest: &Quest) {
    env.storage()
        .persistent()
        .set(&DataKey::Quest(quest.id), quest);
}

/// Append the quest id to each tag's index entry.
fn index_tags(env: &Env, quest_id: u64, tags: &Vec<String>) {
    for i in 0..tags.len() {
        let tag = tags.get(i).unwrap();
        let key = DataKey::TagIndex(tag);
        let mut ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        ids.push_back(quest_id);
        env.storage().persistent().set(&key, &ids);
    }
}

fn panic_with_error(env: &Env, err: QuestError) -> ! {
    env.events().publish(("quest_error",), err as u32);
    panic!("quest contract error");
}

fn is_paused_internal(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

fn require_not_paused(env: &Env) {
    if is_paused_internal(env) {
        panic_with_error(env, QuestError::Paused);
    }
}

fn require_admin(env: &Env, caller: &Address) {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .unwrap_or_else(|| panic_with_error(env, QuestError::NotInitialized));
    if &admin != caller {
        panic_with_error(env, QuestError::Unauthorized);
    }
    caller.require_auth();
}
