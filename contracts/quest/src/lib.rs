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
#[derive(Clone, Debug)]
pub struct Quest {
    pub id: u64,
    pub creator: Address,
    pub title: String,
    pub description: String,
    /// Categories/tags assigned to the quest at creation time
    /// (e.g. "combat", "exploration", "crafting").
    pub tags: Vec<String>,
    pub reward: i128,
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
    /// Create a new quest with an optional list of tags/categories.
    ///
    /// Tags are stored on-chain on the quest itself and indexed so that
    /// `get_quests_by_tag` can return all quests carrying a given tag.
    pub fn create_quest(
        env: Env,
        creator: Address,
        title: String,
        description: String,
        tags: Vec<String>,
        reward: i128,
    ) -> u64 {
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
            reward,
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
