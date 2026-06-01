#![no_std]

//! Quest contract.
//!
//! Provides on-chain quest creation with category/tag support and getters
//! to retrieve quests by id or by tag (category).

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env, String, Vec,
};

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
    /// Categories/tags assigned to the quest at creation time
    /// (e.g. "combat", "exploration", "crafting").
    pub tags: Vec<String>,
    pub rewards: Vec<Reward>,
    pub status: QuestStatus,
    pub created_at: u64,
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

    /// Returns whether the contract is currently paused. Defaults to `false`
    /// (i.e. unpaused) before `initialize` is called.
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

    /// Create a new quest with an optional list of tags/categories.
    ///
    /// Tags are stored on-chain on the quest itself and indexed so that
    /// `get_quests_by_tag` can return all quests carrying a given tag.
    ///
    /// Sensitive: rejected while the contract is paused.
    pub fn create_quest(
        env: Env,
        creator: Address,
        title: String,
        description: String,
        tags: Vec<String>,
        rewards: Vec<Reward>,
    ) -> u64 {
        require_not_paused(&env);
        creator.require_auth();

        if title.is_empty() {
            panic_with_error(&env, QuestError::EmptyTitle);
        }
        if tags.len() > MAX_TAGS_PER_QUEST {
            panic_with_error(&env, QuestError::TooManyTags);
        }
        // Reject empty tag strings.
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
            status: QuestStatus::Active,
            created_at: env.ledger().timestamp(),
        };

        save_quest(&env, &quest);
        index_tags(&env, id, &tags);

        events::quest_created(&env, id, creator);

        id
    }

    /// Retrieve a quest by id. Panics if not found.
    pub fn get_quest(env: Env, quest_id: u64) -> Quest {
        env.storage()
            .persistent()
            .get(&DataKey::Quest(quest_id))
            .unwrap_or_else(|| panic_with_error(&env, QuestError::QuestNotFound))
    }

    /// Retrieve all quests carrying a given tag/category.
    ///
    /// Returns an empty `Vec` when no quests exist for the tag.
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
    env.storage()
        .instance()
        .set(&DataKey::QuestCounter, &next);
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
    // Publish the error code so it shows up in events for off-chain debugging,
    // then panic to abort the invocation.
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
