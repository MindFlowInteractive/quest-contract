#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, vec, Address, Env, String, Vec};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, QuestContract);
    let creator = Address::generate(&env);
    (env, contract_id, creator)
}

fn setup_initialized() -> (Env, Address, Address, Address, QuestContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, QuestContract);
    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let client = QuestContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    (env, contract_id, admin, creator, client)
}

fn s(env: &Env, v: &str) -> String {
    String::from_str(env, v)
}

// ──────────────────────────────────────────────────────────
// Issue #238 — Difficulty tiers with reward multipliers
// ──────────────────────────────────────────────────────────

#[test]
fn difficulty_easy_applies_1x_multiplier() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);

    let id = client.create_quest(
        &creator,
        &s(&env, "Easy Quest"),
        &s(&env, "desc"),
        &Vec::<String>::new(&env),
        &100_i128,
        &Difficulty::Easy,
    );

    let quest = client.get_quest(&id);
    assert_eq!(quest.difficulty, Difficulty::Easy);
    // 100 * 2 / 2 = 100
    assert_eq!(client.effective_reward(&id), 100);
}

#[test]
fn difficulty_medium_applies_1_5x_multiplier() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);

    let id = client.create_quest(
        &creator,
        &s(&env, "Medium Quest"),
        &s(&env, "desc"),
        &Vec::<String>::new(&env),
        &100_i128,
        &Difficulty::Medium,
    );

    // 100 * 3 / 2 = 150
    assert_eq!(client.effective_reward(&id), 150);
}

#[test]
fn difficulty_hard_applies_2x_multiplier() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);

    let id = client.create_quest(
        &creator,
        &s(&env, "Hard Quest"),
        &s(&env, "desc"),
        &Vec::<String>::new(&env),
        &100_i128,
        &Difficulty::Hard,
    );

    // 100 * 4 / 2 = 200
    assert_eq!(client.effective_reward(&id), 200);
}

#[test]
fn difficulty_legendary_applies_3x_multiplier() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);

    let id = client.create_quest(
        &creator,
        &s(&env, "Legendary Quest"),
        &s(&env, "desc"),
        &Vec::<String>::new(&env),
        &100_i128,
        &Difficulty::Legendary,
    );

    // 100 * 6 / 2 = 300
    assert_eq!(client.effective_reward(&id), 300);
}

#[test]
fn difficulty_is_immutable_after_creation() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);

    let id = client.create_quest(
        &creator,
        &s(&env, "Quest"),
        &s(&env, "desc"),
        &Vec::<String>::new(&env),
        &50_i128,
        &Difficulty::Hard,
    );

    // Difficulty stored on-chain cannot be changed — verify it persists.
    let quest = client.get_quest(&id);
    assert_eq!(quest.difficulty, Difficulty::Hard);
    assert_eq!(client.effective_reward(&id), 100); // 50 * 4 / 2
}

#[test]
fn all_four_difficulty_tiers_produce_correct_rewards() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);
    let base = 200_i128;

    let easy_id = client.create_quest(
        &creator,
        &s(&env, "E"),
        &s(&env, "d"),
        &Vec::<String>::new(&env),
        &base,
        &Difficulty::Easy,
    );
    let med_id = client.create_quest(
        &creator,
        &s(&env, "M"),
        &s(&env, "d"),
        &Vec::<String>::new(&env),
        &base,
        &Difficulty::Medium,
    );
    let hard_id = client.create_quest(
        &creator,
        &s(&env, "H"),
        &s(&env, "d"),
        &Vec::<String>::new(&env),
        &base,
        &Difficulty::Hard,
    );
    let leg_id = client.create_quest(
        &creator,
        &s(&env, "L"),
        &s(&env, "d"),
        &Vec::<String>::new(&env),
        &base,
        &Difficulty::Legendary,
    );

    assert_eq!(client.effective_reward(&easy_id), 200); // 1x
    assert_eq!(client.effective_reward(&med_id), 300); // 1.5x
    assert_eq!(client.effective_reward(&hard_id), 400); // 2x
    assert_eq!(client.effective_reward(&leg_id), 600); // 3x
}

// ──────────────────────────────────────────────────────────
// Issue #239 — Prevent duplicate reward claims
// ──────────────────────────────────────────────────────────

#[test]
fn first_claim_succeeds_and_returns_effective_reward() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);
    let player = Address::generate(&env);

    let id = client.create_quest(
        &creator,
        &s(&env, "Quest"),
        &s(&env, "desc"),
        &Vec::<String>::new(&env),
        &100_i128,
        &Difficulty::Hard, // 2x → 200
    );

    let reward = client.claim_reward(&player, &id);
    assert_eq!(reward, 200);
    assert!(client.has_claimed(&player, &id));
}

#[test]
#[should_panic(expected = "quest contract error")]
fn duplicate_claim_panics_with_already_claimed_error() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);
    let player = Address::generate(&env);

    let id = client.create_quest(
        &creator,
        &s(&env, "Quest"),
        &s(&env, "desc"),
        &Vec::<String>::new(&env),
        &100_i128,
        &Difficulty::Easy,
    );

    client.claim_reward(&player, &id);
    // Second claim must panic.
    client.claim_reward(&player, &id);
}

#[test]
fn different_players_can_each_claim_once() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);
    let player_a = Address::generate(&env);
    let player_b = Address::generate(&env);

    let id = client.create_quest(
        &creator,
        &s(&env, "Quest"),
        &s(&env, "desc"),
        &Vec::<String>::new(&env),
        &100_i128,
        &Difficulty::Medium, // 1.5x → 150
    );

    let reward_a = client.claim_reward(&player_a, &id);
    let reward_b = client.claim_reward(&player_b, &id);

    assert_eq!(reward_a, 150);
    assert_eq!(reward_b, 150);
    assert!(client.has_claimed(&player_a, &id));
    assert!(client.has_claimed(&player_b, &id));
}

#[test]
fn has_claimed_returns_false_before_any_claim() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);
    let player = Address::generate(&env);

    let id = client.create_quest(
        &creator,
        &s(&env, "Quest"),
        &s(&env, "desc"),
        &Vec::<String>::new(&env),
        &50_i128,
        &Difficulty::Easy,
    );

    assert!(!client.has_claimed(&player, &id));
}

#[test]
#[should_panic(expected = "quest contract error")]
fn claim_on_nonexistent_quest_panics() {
    let (env, contract_id, _creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);
    let player = Address::generate(&env);

    client.claim_reward(&player, &9999_u64);
}

// ──────────────────────────────────────────────────────────
// Issue #237 — Batch quest creation
// ──────────────────────────────────────────────────────────

#[test]
fn batch_creates_all_quests_and_returns_ids_in_order() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);

    let inputs: Vec<QuestInput> = vec![
        &env,
        QuestInput {
            title: s(&env, "Quest A"),
            description: s(&env, "desc A"),
            tags: vec![&env, s(&env, "combat")],
            reward: 100_i128,
            difficulty: Difficulty::Easy,
        },
        QuestInput {
            title: s(&env, "Quest B"),
            description: s(&env, "desc B"),
            tags: vec![&env, s(&env, "exploration")],
            reward: 200_i128,
            difficulty: Difficulty::Hard,
        },
        QuestInput {
            title: s(&env, "Quest C"),
            description: s(&env, "desc C"),
            tags: Vec::<String>::new(&env),
            reward: 50_i128,
            difficulty: Difficulty::Legendary,
        },
    ];

    let ids = client.create_quest_batch(&creator, &inputs);

    assert_eq!(ids.len(), 3);

    let qa = client.get_quest(&ids.get(0).unwrap());
    assert_eq!(qa.title, s(&env, "Quest A"));
    assert_eq!(qa.difficulty, Difficulty::Easy);
    assert_eq!(client.effective_reward(&ids.get(0).unwrap()), 100);

    let qb = client.get_quest(&ids.get(1).unwrap());
    assert_eq!(qb.title, s(&env, "Quest B"));
    assert_eq!(qb.difficulty, Difficulty::Hard);
    assert_eq!(client.effective_reward(&ids.get(1).unwrap()), 400); // 200 * 2x

    let qc = client.get_quest(&ids.get(2).unwrap());
    assert_eq!(qc.title, s(&env, "Quest C"));
    assert_eq!(qc.difficulty, Difficulty::Legendary);
    assert_eq!(client.effective_reward(&ids.get(2).unwrap()), 150); // 50 * 3x
}

#[test]
fn batch_ids_are_sequential_and_unique() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);

    let inputs: Vec<QuestInput> = vec![
        &env,
        QuestInput {
            title: s(&env, "Q1"),
            description: s(&env, "d"),
            tags: Vec::<String>::new(&env),
            reward: 10_i128,
            difficulty: Difficulty::Easy,
        },
        QuestInput {
            title: s(&env, "Q2"),
            description: s(&env, "d"),
            tags: Vec::<String>::new(&env),
            reward: 10_i128,
            difficulty: Difficulty::Easy,
        },
    ];

    let ids = client.create_quest_batch(&creator, &inputs);
    assert_eq!(ids.len(), 2);
    // IDs must be different.
    assert_ne!(ids.get(0).unwrap(), ids.get(1).unwrap());
}

#[test]
fn batch_indexes_tags_correctly() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);

    let inputs: Vec<QuestInput> = vec![
        &env,
        QuestInput {
            title: s(&env, "Q1"),
            description: s(&env, "d"),
            tags: vec![&env, s(&env, "pvp")],
            reward: 10_i128,
            difficulty: Difficulty::Easy,
        },
        QuestInput {
            title: s(&env, "Q2"),
            description: s(&env, "d"),
            tags: vec![&env, s(&env, "pvp")],
            reward: 10_i128,
            difficulty: Difficulty::Medium,
        },
    ];

    client.create_quest_batch(&creator, &inputs);

    let pvp_quests = client.get_quests_by_tag(&s(&env, "pvp"));
    assert_eq!(pvp_quests.len(), 2);
}

#[test]
#[should_panic(expected = "quest contract error")]
fn batch_with_empty_vec_panics() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);

    client.create_quest_batch(&creator, &Vec::<QuestInput>::new(&env));
}

#[test]
#[should_panic(expected = "quest contract error")]
fn batch_with_empty_title_panics_and_rolls_back() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);

    let inputs: Vec<QuestInput> = vec![
        &env,
        QuestInput {
            title: s(&env, "Valid"),
            description: s(&env, "d"),
            tags: Vec::<String>::new(&env),
            reward: 10_i128,
            difficulty: Difficulty::Easy,
        },
        QuestInput {
            title: s(&env, ""), // invalid — should cause full rollback
            description: s(&env, "d"),
            tags: Vec::<String>::new(&env),
            reward: 10_i128,
            difficulty: Difficulty::Easy,
        },
    ];

    client.create_quest_batch(&creator, &inputs);
}

// ──────────────────────────────────────────────────────────
// Existing tag / pause tests (preserved)
// ──────────────────────────────────────────────────────────

#[test]
fn create_quest_stores_tags_on_chain() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);

    let tags: Vec<String> = vec![&env, s(&env, "combat"), s(&env, "exploration")];

    let quest_id = client.create_quest(
        &creator,
        &s(&env, "Slay the dragon"),
        &s(&env, "Defeat the ancient dragon"),
        &tags,
        &1000_i128,
        &Difficulty::Easy,
    );

    let quest = client.get_quest(&quest_id);
    assert_eq!(quest.id, quest_id);
    assert_eq!(quest.creator, creator);
    assert_eq!(quest.tags.len(), 2);
    assert_eq!(quest.tags.get(0).unwrap(), s(&env, "combat"));
    assert_eq!(quest.tags.get(1).unwrap(), s(&env, "exploration"));
    assert_eq!(quest.status, QuestStatus::Active);
}

#[test]
fn get_quests_by_tag_returns_only_matching_quests() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);

    let a = client.create_quest(
        &creator,
        &s(&env, "Goblin Hunt"),
        &s(&env, "Hunt goblins"),
        &vec![&env, s(&env, "combat")],
        &100_i128,
        &Difficulty::Easy,
    );
    let b = client.create_quest(
        &creator,
        &s(&env, "Forge a Sword"),
        &s(&env, "Craft sword"),
        &vec![&env, s(&env, "crafting")],
        &50_i128,
        &Difficulty::Medium,
    );
    let c = client.create_quest(
        &creator,
        &s(&env, "Dungeon Dive"),
        &s(&env, "Clear dungeon"),
        &vec![&env, s(&env, "combat"), s(&env, "exploration")],
        &200_i128,
        &Difficulty::Hard,
    );

    let combat_quests = client.get_quests_by_tag(&s(&env, "combat"));
    assert_eq!(combat_quests.len(), 2);
    assert_eq!(combat_quests.get(0).unwrap().id, a);
    assert_eq!(combat_quests.get(1).unwrap().id, c);

    let crafting_quests = client.get_quests_by_tag(&s(&env, "crafting"));
    assert_eq!(crafting_quests.len(), 1);
    assert_eq!(crafting_quests.get(0).unwrap().id, b);
}

#[test]
fn create_quest_with_no_tags_is_allowed() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);

    let id = client.create_quest(
        &creator,
        &s(&env, "Untagged"),
        &s(&env, "no tags"),
        &Vec::<String>::new(&env),
        &0_i128,
        &Difficulty::Easy,
    );

    let quest = client.get_quest(&id);
    assert_eq!(quest.tags.len(), 0);
}

#[test]
#[should_panic(expected = "quest contract error")]
fn create_quest_rejects_empty_title() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);

    client.create_quest(
        &creator,
        &s(&env, ""),
        &s(&env, "desc"),
        &vec![&env, s(&env, "combat")],
        &0_i128,
        &Difficulty::Easy,
    );
}

#[test]
#[should_panic(expected = "quest contract error")]
fn create_quest_rejects_empty_tag() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);

    client.create_quest(
        &creator,
        &s(&env, "Title"),
        &s(&env, "desc"),
        &vec![&env, s(&env, "")],
        &0_i128,
        &Difficulty::Easy,
    );
}

#[test]
#[should_panic(expected = "quest contract error")]
fn create_quest_rejects_too_many_tags() {
    let (env, contract_id, creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);

    let mut tags: Vec<String> = Vec::new(&env);
    for _ in 0..(MAX_TAGS_PER_QUEST + 1) {
        tags.push_back(s(&env, "tag"));
    }

    client.create_quest(
        &creator,
        &s(&env, "Title"),
        &s(&env, "desc"),
        &tags,
        &0_i128,
        &Difficulty::Easy,
    );
}

#[test]
#[should_panic(expected = "quest contract error")]
fn get_quest_panics_for_unknown_id() {
    let (env, contract_id, _creator) = setup();
    let client = QuestContractClient::new(&env, &contract_id);
    client.get_quest(&999_u64);
}

#[test]
fn initialize_sets_admin_and_unpaused_state() {
    let (_env, _id, admin, _creator, client) = setup_initialized();
    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.is_paused(), false);
}

#[test]
#[should_panic(expected = "quest contract error")]
fn initialize_twice_is_rejected() {
    let (env, _id, _admin, _creator, client) = setup_initialized();
    let other = Address::generate(&env);
    client.initialize(&other);
}

#[test]
fn admin_can_pause_and_unpause() {
    let (_env, _id, admin, _creator, client) = setup_initialized();
    assert_eq!(client.is_paused(), false);
    client.pause(&admin);
    assert_eq!(client.is_paused(), true);
    client.unpause(&admin);
    assert_eq!(client.is_paused(), false);
}

#[test]
#[should_panic(expected = "quest contract error")]
fn non_admin_cannot_pause() {
    let (env, _id, _admin, _creator, client) = setup_initialized();
    let stranger = Address::generate(&env);

    client.pause(&stranger);
}

#[test]
fn create_quest_allowed_when_unpaused() {
    let (env, _id, _admin, creator, client) = setup_initialized();

    let quest_id = client.create_quest(
        &creator,
        &s(&env, "Title"),
        &s(&env, "desc"),
        &Vec::<String>::new(&env),
        &10_i128,
        &Difficulty::Easy,
    );

    assert_eq!(client.get_quest(&quest_id).status, QuestStatus::Active);
}

#[test]
#[should_panic(expected = "quest contract error")]
fn create_quest_blocked_while_paused() {
    let (env, _id, admin, creator, client) = setup_initialized();
    client.pause(&admin);

    client.create_quest(
        &creator,
        &s(&env, "Title"),
        &s(&env, "desc"),
        &vec![&env, s(&env, "combat")],
        &0_i128,
        &Difficulty::Easy,
    );
}

#[test]
#[should_panic(expected = "quest contract error")]
fn batch_create_blocked_while_paused() {
    let (env, _id, admin, creator, client) = setup_initialized();
    client.pause(&admin);

    let inputs: Vec<QuestInput> = vec![
        &env,
        QuestInput {
            title: s(&env, "Q"),
            description: s(&env, "d"),
            tags: Vec::<String>::new(&env),
            reward: 10_i128,
            difficulty: Difficulty::Easy,
        },
    ];
    client.create_quest_batch(&creator, &inputs);
}

#[test]
fn complete_quest_allowed_when_unpaused() {
    let (env, _id, _admin, creator, client) = setup_initialized();
    let player = Address::generate(&env);

    let quest_id = client.create_quest(
        &creator,
        &s(&env, "Title"),
        &s(&env, "desc"),
        &Vec::<String>::new(&env),
        &10_i128,
        &Difficulty::Easy,
    );

    client.complete_quest(&player, &quest_id);

    assert_eq!(client.get_quest(&quest_id).status, QuestStatus::Completed);
}

#[test]
#[should_panic(expected = "quest contract error")]
fn complete_quest_blocked_while_paused() {
    let (env, _id, admin, creator, client) = setup_initialized();
    let player = Address::generate(&env);

    let quest_id = client.create_quest(
        &creator,
        &s(&env, "Title"),
        &s(&env, "desc"),
        &Vec::<String>::new(&env),
        &10_i128,
        &Difficulty::Easy,
    );

    client.pause(&admin);

    client.complete_quest(&player, &quest_id);
}
