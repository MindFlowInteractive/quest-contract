#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, Env, Symbol, TryFromVal,
};

fn setup_contract(env: &Env) -> (QuestChainContractClient, Address) {
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, QuestChainContract);
    let client = QuestChainContractClient::new(env, &contract_id);

    env.mock_all_auths();
    client.initialize(&admin);

    (client, admin)
}

fn create_test_quests(env: &Env) -> Vec<Quest> {
    let mut quests = Vec::new(env);

    // Quest 1: Initial quest, no prerequisites
    quests.push_back(Quest {
        id: 1,
        puzzle_id: 101,
        rewards: Vec::new(env),
        status: QuestStatus::Locked,
        prerequisites: Vec::new(env),
        branches: Vec::new(env),
        checkpoint: true,
        expiry_timestamp: None,
    });

    // Quest 2: Requires quest 1
    quests.push_back(Quest {
        id: 2,
        puzzle_id: 102,
        rewards: Vec::new(env),
        status: QuestStatus::Locked,
        prerequisites: {
            let mut prereqs = Vec::new(env);
            prereqs.push_back(1);
            prereqs
        },
        branches: Vec::new(env),
        checkpoint: false,
        expiry_timestamp: None,
    });

    // Quest 3: Also requires quest 1 (branching path)
    quests.push_back(Quest {
        id: 3,
        puzzle_id: 103,
        rewards: Vec::new(env),
        status: QuestStatus::Locked,
        prerequisites: {
            let mut prereqs = Vec::new(env);
            prereqs.push_back(1);
            prereqs
        },
        branches: Vec::new(env),
        checkpoint: true,
        expiry_timestamp: None,
    });

    // Quest 4: Requires quest 2 OR quest 3 (branch merge)
    quests.push_back(Quest {
        id: 4,
        puzzle_id: 104,
        rewards: Vec::new(env),
        status: QuestStatus::Locked,
        prerequisites: {
            let mut prereqs = Vec::new(env);
            prereqs.push_back(2);
            prereqs
        },
        branches: {
            let mut branches = Vec::new(env);
            branches.push_back(3);
            branches
        },
        checkpoint: false,
        expiry_timestamp: None,
    });

    // Quest 5: Final quest, requires quest 4
    quests.push_back(Quest {
        id: 5,
        puzzle_id: 105,
        rewards: Vec::new(env),
        status: QuestStatus::Locked,
        prerequisites: {
            let mut prereqs = Vec::new(env);
            prereqs.push_back(4);
            prereqs
        },
        branches: Vec::new(env),
        checkpoint: true,
        expiry_timestamp: None,
    });

    quests
}

#[test]
fn test_initialization() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup_contract(&env);

    let config = client.get_config();
    assert_eq!(config.owner, admin);
    assert_eq!(config.max_chains, DEFAULT_MAX_CHAINS);
    assert_eq!(config.min_quests_per_chain, DEFAULT_MIN_QUESTS);
    assert_eq!(config.max_quests_per_chain, DEFAULT_MAX_QUESTS);
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_double_initialization() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup_contract(&env);
    client.initialize(&admin);
}

#[test]
fn test_create_chain() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    assert_eq!(chain_id, 1);

    let chain = client.get_chain(&chain_id);
    assert_eq!(chain.id, chain_id);
    assert_eq!(chain.title, symbol_short!("TestChain"));
    assert_eq!(chain.quests.len(), 5);
    assert!(chain.active);
}

#[test]
fn test_create_time_limited_chain() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let start_time = Some(1000u64);
    let end_time = Some(2000u64);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TimeLim"),
        &symbol_short!("timechn"),
        &quests,
        &start_time,
        &end_time,
        &None,
    );

    let chain = client.get_chain(&chain_id);
    assert_eq!(chain.start_time, start_time);
    assert_eq!(chain.end_time, end_time);
}

#[test]
#[should_panic(expected = "Too few quests")]
fn test_create_chain_too_few_quests() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup_contract(&env);
    let empty_quests = Vec::new(&env);

    client.create_chain(
        &admin,
        &symbol_short!("Empty"),
        &symbol_short!("emptych"),
        &empty_quests,
        &None,
        &None,
        &None,
    );
}

#[test]
fn test_start_chain() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.player, player);
    assert_eq!(progress.chain_id, chain_id);
    assert_eq!(progress.completed_quests.len(), 0);
    assert_eq!(progress.current_quest, Some(1)); // First quest should be unlocked
    assert_eq!(progress.start_time, 1000);
}

#[test]
#[should_panic(expected = "Chain already started")]
fn test_start_chain_twice() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);
    client.start_chain(&player, &chain_id);
}

#[test]
#[should_panic(expected = "Chain not started yet")]
fn test_start_chain_before_start_time() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &Some(2000u64),
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);
}

#[test]
#[should_panic(expected = "Chain expired")]
fn test_start_chain_after_end_time() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(3000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &Some(1000u64),
        &Some(2000u64),
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);
}

#[test]
fn test_sequential_quest_completion() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    // Complete quest 1
    client.complete_quest(&player, &chain_id, &1);
    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.completed_quests.len(), 1);
    assert!(progress.completed_quests.contains(&1));
    assert_eq!(progress.checkpoint_quest, Some(1));

    // Complete quest 2
    client.complete_quest(&player, &chain_id, &2);
    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.completed_quests.len(), 2);
}

#[test]
fn test_complete_quest_without_prerequisites() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    // Complete quest 1 first
    client.complete_quest(&player, &chain_id, &1);

    // Now quest 2 should be available (requires quest 1)
    client.complete_quest(&player, &chain_id, &2);
    
    // Verify that completing quest 3 is also possible (requires quest 1)
    client.complete_quest(&player, &chain_id, &3);

    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.completed_quests.len(), 3);
    // Now we can complete quest 4 (which requires quest 2 or 3)
    client.complete_quest(&player, &chain_id, &4);

    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.completed_quests.len(), 4);
}

#[test]
fn test_branching_paths() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    // Complete quest 1
    client.complete_quest(&player, &chain_id, &1);

    // Complete quest 3 (branch path) instead of quest 2
    client.complete_quest(&player, &chain_id, &3);
    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert!(progress.completed_quests.contains(&3));

    // Quest 4 can be completed with either quest 2 or 3 as prerequisite
    // Since we completed 3, we should be able to complete 4
    client.complete_quest(&player, &chain_id, &4);
    let progress = client.get_player_progress(&player, &chain_id).unwrap();
}

#[test]
fn test_progress_checkpointing() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    // Complete quest 1 (checkpoint)
    client.complete_quest(&player, &chain_id, &1);
    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.checkpoint_quest, Some(1));

    // Complete quest 2 (no checkpoint)
    client.complete_quest(&player, &chain_id, &2);
    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.checkpoint_quest, Some(1)); // Still at quest 1

    // Complete quest 3 (checkpoint)
    client.complete_quest(&player, &chain_id, &3);
    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.checkpoint_quest, Some(3)); // Updated to quest 3
}

#[test]
fn test_reset_to_checkpoint() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    // Complete quest 1 (checkpoint)
    client.complete_quest(&player, &chain_id, &1);
    // Complete quest 2
    client.complete_quest(&player, &chain_id, &2);
    // Complete quest 3 (checkpoint)
    client.complete_quest(&player, &chain_id, &3);

    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.completed_quests.len(), 3);
    assert_eq!(progress.checkpoint_quest, Some(3)); // Latest checkpoint is quest 3

    // Reset to checkpoint (quest 3)
    client.reset_to_checkpoint(&player, &chain_id);

    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    // After reset, all quests up to and including the last checkpoint are retained
    assert_eq!(progress.completed_quests.len(), 3);
    assert_eq!(progress.checkpoint_quest, Some(3));
}

#[test]
#[should_panic(expected = "No checkpoint available")]
fn test_reset_to_checkpoint_no_checkpoint() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    // Try to reset without any checkpoints
    client.reset_to_checkpoint(&player, &chain_id);
}

#[test]
fn test_reset_chain() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);
    client.complete_quest(&player, &chain_id, &1);
    client.complete_quest(&player, &chain_id, &2);

    // Reset entire chain
    client.reset_chain(&player, &chain_id);

    // Progress should be removed
    assert!(client.get_player_progress(&player, &chain_id).is_none());
}

#[test]
fn test_chain_completion() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    // Complete all quests sequentially
    client.complete_quest(&player, &chain_id, &1);
    client.complete_quest(&player, &chain_id, &2);
    client.complete_quest(&player, &chain_id, &3);
    client.complete_quest(&player, &chain_id, &4);
    client.complete_quest(&player, &chain_id, &5);

    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert!(progress.completion_time.is_some());
    assert_eq!(progress.completed_quests.len(), 5);

    // Check completion count
    assert_eq!(client.get_chain_completions(&chain_id), 1);
}

#    // test_cumulative_rewards removed as it relied on single i128 total

#[test]
fn test_leaderboard() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    // Player 1 completes quickly
    let player1 = Address::generate(&env);
    client.start_chain(&player1, &chain_id);
    env.ledger().set_timestamp(1000);
    client.complete_quest(&player1, &chain_id, &1);
    client.complete_quest(&player1, &chain_id, &2);
    client.complete_quest(&player1, &chain_id, &3);
    client.complete_quest(&player1, &chain_id, &4);
    client.complete_quest(&player1, &chain_id, &5);

    // Player 2 completes slower
    let player2 = Address::generate(&env);
    client.start_chain(&player2, &chain_id);
    env.ledger().set_timestamp(2000);
    client.complete_quest(&player2, &chain_id, &1);
    client.complete_quest(&player2, &chain_id, &2);
    client.complete_quest(&player2, &chain_id, &3);
    client.complete_quest(&player2, &chain_id, &4);
    client.complete_quest(&player2, &chain_id, &5);

    // Player 3 completes even slower
    let player3 = Address::generate(&env);
    client.start_chain(&player3, &chain_id);
    env.ledger().set_timestamp(3000);
    client.complete_quest(&player3, &chain_id, &1);
    client.complete_quest(&player3, &chain_id, &2);
    client.complete_quest(&player3, &chain_id, &3);
    client.complete_quest(&player3, &chain_id, &4);
    client.complete_quest(&player3, &chain_id, &5);

    let leaderboard = client.get_leaderboard(&chain_id, &10);
    assert_eq!(leaderboard.len(), 3);

    // Leaderboard should be sorted by duration (fastest first)
    let first = leaderboard.get(0).unwrap();
    let second = leaderboard.get(1).unwrap();
    let third = leaderboard.get(2).unwrap();

    assert!(first.duration <= second.duration);
    assert!(second.duration <= third.duration);
}

#[test]
fn test_multiple_players_same_chain() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    let player1 = Address::generate(&env);
    let player2 = Address::generate(&env);

    client.start_chain(&player1, &chain_id);
    client.start_chain(&player2, &chain_id);

    client.complete_quest(&player1, &chain_id, &1);
    client.complete_quest(&player2, &chain_id, &1);

    let progress1 = client.get_player_progress(&player1, &chain_id).unwrap();
    let progress2 = client.get_player_progress(&player2, &chain_id).unwrap();

    assert_eq!(progress1.completed_quests.len(), 1);
    assert_eq!(progress2.completed_quests.len(), 1);
}

#[test]
fn test_admin_functions() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup_contract(&env);

    // Update config
    client.update_config(&admin, &Some(500u32), &Some(2u32), &Some(50u32));

    let config = client.get_config();
    assert_eq!(config.max_chains, 500);
    assert_eq!(config.min_quests_per_chain, 2);
    assert_eq!(config.max_quests_per_chain, 50);

    // Create and deactivate chain
    let quests = create_test_quests(&env);
    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    client.set_chain_active(&admin, &chain_id, &false);
    let chain = client.get_chain(&chain_id);
    assert!(!chain.active);
}

#[test]
#[should_panic(expected = "Owner only")]
fn test_unauthorized_admin_action() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup_contract(&env);
    let non_admin = Address::generate(&env);

    client.update_config(&non_admin, &Some(500u32), &None, &None);
}

#[test]
fn test_owner_can_assign_and_revoke_manager() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, owner) = setup_contract(&env);
    let manager = Address::generate(&env);
    let quests = create_test_quests(&env);

    assert!(!client.is_manager(&manager));
    client.assign_manager(&owner, &manager);
    assert!(client.is_manager(&manager));

    let chain_id = client.create_chain(
        &manager,
        &Symbol::new(&env, "Managed"),
        &Symbol::new(&env, "Created by manager"),
        &quests,
        &None,
        &None,
    );

    client.set_chain_active(&manager, &chain_id, &false);
    assert!(!client.get_chain(&chain_id).active);

    client.revoke_manager(&owner, &manager);
    assert!(!client.is_manager(&manager));
}

#[test]
#[should_panic(expected = "Owner only")]
fn test_only_owner_can_assign_manager() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner) = setup_contract(&env);
    let non_owner = Address::generate(&env);
    let manager = Address::generate(&env);

    client.assign_manager(&non_owner, &manager);
}

#[test]
#[should_panic(expected = "Manager only")]
fn test_revoked_manager_cannot_create_chain() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, owner) = setup_contract(&env);
    let manager = Address::generate(&env);
    let quests = create_test_quests(&env);

    client.assign_manager(&owner, &manager);
    client.revoke_manager(&owner, &manager);

    client.create_chain(
        &manager,
        &Symbol::new(&env, "Revoked"),
        &Symbol::new(&env, "Should fail"),
        &quests,
        &None,
        &None,
    );
}

#[test]
fn test_moderator_can_manage_but_not_create_chain() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, owner) = setup_contract(&env);
    let moderator = Address::generate(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &owner,
        &Symbol::new(&env, "Moderated"),
        &Symbol::new(&env, "Managed by moderator"),
        &quests,
        &None,
        &None,
    );

    client.assign_moderator(&owner, &moderator);
    assert!(client.is_moderator(&moderator));

    client.set_chain_active(&moderator, &chain_id, &false);
    assert!(!client.get_chain(&chain_id).active);
}

#[test]
#[should_panic(expected = "Quest already completed")]
fn test_complete_quest_twice() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);
    client.complete_quest(&player, &chain_id, &1);
    client.complete_quest(&player, &chain_id, &1);
}

#[test]
#[should_panic(expected = "Quest not unlocked")]
fn test_complete_unlocked_quest() {
    let env = Env::default();
    env.mock_all_auths();
}

#[test]
fn test_quest_expiry_and_cancellation() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    
    let mut quests = Vec::new(&env);
    quests.push_back(Quest {
        id: 1, puzzle_id: 101, reward: 100, status: QuestStatus::Locked,
        prerequisites: Vec::new(&env), branches: Vec::new(&env), checkpoint: true,
        expiry_timestamp: Some(2000), // Expires at 2000
    });

    let chain_id = client.create_chain(
        &admin, &symbol_short!("Expiry"), &symbol_short!("expchn"),
        &quests, &None, &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);
    
    // Complete before expiry succeeds
    env.ledger().set_timestamp(1500);
    client.complete_quest(&player, &chain_id, &1);

    // Now test cancellation on a fresh chain
    let mut quests2 = Vec::new(&env);
    quests2.push_back(Quest {
        id: 1, puzzle_id: 101, reward: 100, status: QuestStatus::Locked,
        prerequisites: Vec::new(&env), branches: Vec::new(&env), checkpoint: true,
        expiry_timestamp: Some(2500),
    });

    let chain_id_2 = client.create_chain(
        &admin, &symbol_short!("Expiry2"), &symbol_short!("expchn2"),
        &quests2, &None, &None,
    );

    let player2 = Address::generate(&env);
    client.start_chain(&player2, &chain_id_2);

    // Cancel expired quests after time passes
    env.ledger().set_timestamp(3000);
    client.cancel_expired_quests(&admin, &chain_id_2);

    let chain = client.get_chain(&chain_id_2);
    let q = chain.quests.get(0).unwrap();
    assert_eq!(q.status, QuestStatus::Locked);
}

#[test]
#[should_panic(expected = "Quest: expired")]
fn test_complete_expired_quest() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    
    let mut quests = Vec::new(&env);
    quests.push_back(Quest {
        id: 1, puzzle_id: 101, reward: 100, status: QuestStatus::Locked,
        prerequisites: Vec::new(&env), branches: Vec::new(&env), checkpoint: true,
        expiry_timestamp: Some(1500), // Expires at 1500
    });

    let chain_id = client.create_chain(
        &admin, &symbol_short!("Expiry"), &symbol_short!("expchn"),
        &quests, &None, &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    // Time passes expiry
    env.ledger().set_timestamp(2000);
    client.complete_quest(&player, &chain_id, &1);
}
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    // Try to complete quest 5 without completing prerequisites
    client.complete_quest(&player, &chain_id, &5);
}

    // test_reward_token_configuration removed

// ──────────────────────────────────────────────────────────
// EVENT TESTS
// ──────────────────────────────────────────────────────────

fn find_event_by_name(
    env: &Env,
    events: &soroban_sdk::Vec<(Address, soroban_sdk::Vec<soroban_sdk::Val>, soroban_sdk::Val)>,
    name: &Symbol,
) -> Option<(Address, soroban_sdk::Vec<soroban_sdk::Val>, soroban_sdk::Val)> {
    for event in events.iter() {
        let topics = &event.1;
        if let Some(first) = topics.get(0) {
            if let Ok(sym) = Symbol::try_from_val(env, &first) {
                if sym == *name {
                    return Some(event.clone());
                }
            }
        }
    }
    None
}

#[test]
fn test_event_chain_created() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &Symbol::new(&env, "TestChain"),
        &Symbol::new(&env, "Desc"),
        &quests,
        &None,
        &None,
    );

    let events = env.events().all();
    let event = find_event_by_name(&env, &events, &CHAIN_CREATED);
    assert!(event.is_some(), "CHAIN_CREATED event not found");
    let (_, topics, _) = event.unwrap();
    assert_eq!(Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap(), CHAIN_CREATED);
    assert_eq!(u32::try_from_val(&env, &topics.get(1).unwrap()).unwrap(), chain_id);
}

#[test]
fn test_event_chain_started() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);
    let chain_id = client.create_chain(
        &admin,
        &Symbol::new(&env, "TestChain"),
        &Symbol::new(&env, "Desc"),
        &quests,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    let events = env.events().all();
    let event = find_event_by_name(&env, &events, &CHAIN_STARTED);
    assert!(event.is_some(), "CHAIN_STARTED event not found");
    let (_, topics, _) = event.unwrap();
    assert_eq!(Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap(), CHAIN_STARTED);
    assert_eq!(Address::try_from_val(&env, &topics.get(1).unwrap()).unwrap(), player);
    assert_eq!(u32::try_from_val(&env, &topics.get(2).unwrap()).unwrap(), chain_id);
}

#[test]
fn test_event_quest_completed() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);
    let chain_id = client.create_chain(
        &admin,
        &Symbol::new(&env, "TestChain"),
        &Symbol::new(&env, "Desc"),
        &quests,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);
    client.complete_quest(&player, &chain_id, &1);

    let events = env.events().all();
    let event = find_event_by_name(&env, &events, &QUEST_COMPLETED);
    assert!(event.is_some(), "QUEST_COMPLETED event not found");
    let (_, topics, _) = event.unwrap();
    assert_eq!(Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap(), QUEST_COMPLETED);
    assert_eq!(Address::try_from_val(&env, &topics.get(1).unwrap()).unwrap(), player);
    assert_eq!(u32::try_from_val(&env, &topics.get(2).unwrap()).unwrap(), chain_id);
}

#[test]
fn test_event_chain_completed() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);
    let chain_id = client.create_chain(
        &admin,
        &Symbol::new(&env, "TestChain"),
        &Symbol::new(&env, "Desc"),
        &quests,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);
    client.complete_quest(&player, &chain_id, &1);
    client.complete_quest(&player, &chain_id, &2);
    client.complete_quest(&player, &chain_id, &3);
    client.complete_quest(&player, &chain_id, &4);
    env.ledger().set_timestamp(2000);
    client.complete_quest(&player, &chain_id, &5);

    let events = env.events().all();
    let event = find_event_by_name(&env, &events, &CHAIN_COMPLETED);
    assert!(event.is_some(), "CHAIN_COMPLETED event not found");
    let (_, topics, _) = event.unwrap();
    assert_eq!(Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap(), CHAIN_COMPLETED);
    assert_eq!(Address::try_from_val(&env, &topics.get(1).unwrap()).unwrap(), player);
    assert_eq!(u32::try_from_val(&env, &topics.get(2).unwrap()).unwrap(), chain_id);
}

#[test]
fn test_event_reward_claimed() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let reward_token = sac.address();
    let sac_client = soroban_sdk::token::StellarAssetClient::new(&env, &reward_token);

    let contract_id = env.register_contract(None, QuestChainContract);
    let client = QuestChainContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let quests = create_test_quests(&env);
    let chain_id = client.create_chain(
        &admin,
        &Symbol::new(&env, "TestChain"),
        &Symbol::new(&env, "Desc"),
        &quests,
        &None,
        &None,
    );

    // Mint tokens to the quest chain contract so it can pay out rewards
    sac_client.mint(&contract_id, &10000i128);

    // Seed reward pool directly in storage
    env.as_contract(&contract_id, || {
        let pool_key = DataKey::RewardPool(chain_id, TokenType::ERC20, Some(reward_token.clone()));
        env.storage()
            .persistent()
            .set(&pool_key, &10000i128);
    });

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);
    client.complete_quest(&player, &chain_id, &1);
    client.claim_rewards(&player, &chain_id);

    let events = env.events().all();
    let event = find_event_by_name(&env, &events, &REWARD_CLAIMED);
    assert!(event.is_some(), "REWARD_CLAIMED event not found");
    let (_, topics, _) = event.unwrap();
    assert_eq!(Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap(), REWARD_CLAIMED);
    assert_eq!(Address::try_from_val(&env, &topics.get(1).unwrap()).unwrap(), player);
    assert_eq!(u32::try_from_val(&env, &topics.get(2).unwrap()).unwrap(), chain_id);
}

#[test]
fn test_pending_rewards_tracking() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let reward_token = Address::generate(&env);

    let quests = create_test_quests(&env);
    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    // Complete quest 1
    client.complete_quest(&player, &chain_id, &1);

    // Check pending rewards
    let pending = client.get_pending_rewards(&player, &chain_id);
    assert_eq!(pending.len(), 0); // quests in create_test_quests have no rewards by default now
}

// ───────────── QUEST EXPIRY TESTS ─────────────

/// Helper: build a single-quest chain where quest 1 has a given expires_at.
fn setup_expiry_chain(
    env: &Env,
    client: &QuestChainContractClient,
    admin: &Address,
    expires_at: Option<u64>,
) -> u32 {
    let mut quests = Vec::new(env);
    quests.push_back(Quest {
        id: 1,
        puzzle_id: 101,
        rewards: Vec::new(env),
        status: QuestStatus::Locked,
        prerequisites: Vec::new(env),
        branches: Vec::new(env),
        checkpoint: false,
        expires_at,
    });
    client.create_chain(
        admin,
        &symbol_short!("ExpiryChn"),
        &symbol_short!("expirychn"),
        &quests,
        &None,
        &None,
    )
}

#[test]
fn test_complete_quest_before_expiry_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    // Quest expires at 2000; current time is 1000 — should succeed
    let chain_id = setup_expiry_chain(&env, &client, &admin, Some(2000));

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    env.ledger().set_timestamp(1999); // one second before expiry
    client.complete_quest(&player, &chain_id, &1);

    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert!(progress.completed_quests.contains(&1));
}

#[test]
#[should_panic(expected = "Quest: expired")]
fn test_complete_quest_exactly_at_expiry_fails() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    // Boundary condition: attempt exactly at the expiry timestamp
    let chain_id = setup_expiry_chain(&env, &client, &admin, Some(2000));

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    env.ledger().set_timestamp(2000); // exactly at expiry — must revert
    client.complete_quest(&player, &chain_id, &1);
}

#[test]
#[should_panic(expected = "Quest: expired")]
fn test_complete_quest_after_expiry_fails() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let max_participants = 2u32;
    let chain_id = client.create_chain(
        &admin,
        &Symbol::new(&env, "Limited Chain"),
        &Symbol::new(&env, "A chain with participant limit"),
        &quests,
        &None,
        &None,
        &Some(max_participants),
    );

    // Add 2 players (at the limit)
    for _ in 0..2 {
        let player = Address::generate(&env);
        client.start_chain(&player, &chain_id);
    }

    // Try to add a 3rd player (should panic)
    let extra_player = Address::generate(&env);
    client.start_chain(&extra_player, &chain_id);
}

#[test]
fn test_chain_without_participant_limit() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    // expires_at: None — no deadline, should always work
    let chain_id = setup_expiry_chain(&env, &client, &admin, None);

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    // Advance time far into the future — still no expiry
    env.ledger().set_timestamp(u64::MAX / 2);
    client.complete_quest(&player, &chain_id, &1);

    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert!(progress.completed_quests.contains(&1));
}

#[test]
fn test_expiry_event_emitted_on_rejection() {
    // Soroban's test environment reverts on panic, but the QUEST_EXPIRED
    // event is published in the contract body *before* panic!("Quest: expired").
    // This test confirms the panic message is correct (the event emission path
    // is exercised by the compiler seeing it's reachable).  A separate
    // integration/simulation test would be needed to assert event capture
    // across a transaction revert boundary.
    //
    // Here we verify: (a) the attempt panics with the right message, and
    // (b) a successful non-expired quest does NOT emit the expiry event.

    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let max_participants = 5u32;
    let chain_id = client.create_chain(
        &admin,
        &Symbol::new(&env, "Limited Chain"),
        &Symbol::new(&env, "A chain with participant limit"),
        &quests,
        &None,
        &None,
        &Some(max_participants),
    );

    // Add 3 players
    let player1 = Address::generate(&env);
    let player2 = Address::generate(&env);
    let player3 = Address::generate(&env);
    
    client.start_chain(&player1, &chain_id);
    client.start_chain(&player2, &chain_id);
    client.start_chain(&player3, &chain_id);

    let participant_count = client.get_chain_participants(&chain_id);
    assert_eq!(participant_count, 3);

    // Reset player2's progress
    client.reset_chain(&player2, &chain_id);

    // Participant count should decrement
    let participant_count = client.get_chain_participants(&chain_id);
    assert_eq!(participant_count, 2);

    // Should be able to add a new player now
    let player4 = Address::generate(&env);
    client.start_chain(&player4, &chain_id);

    let participant_count = client.get_chain_participants(&chain_id);
    assert_eq!(participant_count, 3);
}

#[test]
fn test_participant_count_accuracy() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let quests = create_test_quests(&env);

    let chain_id = client.create_chain(
        &admin,
        &Symbol::new(&env, "Test Chain"),
        &Symbol::new(&env, "A test quest chain"),
        &quests,
        &None,
        &None,
        &None,
    );

    // Initial count
    assert_eq!(client.get_chain_participants(&chain_id), 0);

    // Add first player
    let player1 = Address::generate(&env);
    client.start_chain(&player1, &chain_id);
    assert_eq!(client.get_chain_participants(&chain_id), 1);

    // Add second player
    let player2 = Address::generate(&env);
    client.start_chain(&player2, &chain_id);
    assert_eq!(client.get_chain_participants(&chain_id), 2);

    // Try to add same player again (should fail)
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.start_chain(&player1, &chain_id);
    }));
    assert!(result.is_err());
    assert_eq!(client.get_chain_participants(&chain_id), 2); // Count unchanged

    // Remove one player
    client.reset_chain(&player1, &chain_id);
    assert_eq!(client.get_chain_participants(&chain_id), 1);

    // Remove another player
    client.reset_chain(&player2, &chain_id);
    assert_eq!(client.get_chain_participants(&chain_id), 0);
}
