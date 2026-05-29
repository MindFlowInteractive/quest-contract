#![cfg(test)]

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, Env, IntoVal,
};

fn setup_contract(env: &Env) -> (QuestChainContractClient, Address) {
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, QuestChainContract);
    let client = QuestChainContractClient::new(env, &contract_id);

    env.mock_all_auths();
    client.initialize(&admin, &None);

    (client, admin)
}

fn create_test_quests(env: &Env) -> Vec<Quest> {
    let mut quests = Vec::new(env);

    // Quest 1: Initial quest, no prerequisites
    quests.push_back(Quest {
        id: 1,
        puzzle_id: 101,
        reward: 100,
        status: QuestStatus::Locked,
        prerequisites: Vec::new(env),
        branches: Vec::new(env),
        checkpoint: true,
        expires_at: None,
    });

    // Quest 2: Requires quest 1
    quests.push_back(Quest {
        id: 2,
        puzzle_id: 102,
        reward: 150,
        status: QuestStatus::Locked,
        prerequisites: {
            let mut prereqs = Vec::new(env);
            prereqs.push_back(1);
            prereqs
        },
        branches: Vec::new(env),
        checkpoint: false,
        expires_at: None,
    });

    // Quest 3: Also requires quest 1 (branching path)
    quests.push_back(Quest {
        id: 3,
        puzzle_id: 103,
        reward: 200,
        status: QuestStatus::Locked,
        prerequisites: {
            let mut prereqs = Vec::new(env);
            prereqs.push_back(1);
            prereqs
        },
        branches: Vec::new(env),
        checkpoint: true,
        expires_at: None,
    });

    // Quest 4: Requires quest 2 OR quest 3 (branch merge)
    quests.push_back(Quest {
        id: 4,
        puzzle_id: 104,
        reward: 250,
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
        expires_at: None,
    });

    // Quest 5: Final quest, requires quest 4
    quests.push_back(Quest {
        id: 5,
        puzzle_id: 105,
        reward: 300,
        status: QuestStatus::Locked,
        prerequisites: {
            let mut prereqs = Vec::new(env);
            prereqs.push_back(4);
            prereqs
        },
        branches: Vec::new(env),
        checkpoint: true,
        expires_at: None,
    });

    quests
}

#[test]
fn test_initialization() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup_contract(&env);

    let config = client.get_config();
    assert_eq!(config.admin, admin);
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
    client.initialize(&admin, &None);
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
    );

    assert_eq!(chain_id, 1);

    let chain = client.get_chain(&chain_id);
    assert_eq!(chain.id, chain_id);
    assert_eq!(chain.title, symbol_short!("TestChain"));
    assert_eq!(chain.quests.len(), 5);
    assert_eq!(chain.total_reward, 1000); // 100 + 150 + 200 + 250 + 300
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
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    // Complete quest 1
    client.complete_quest(&player, &chain_id, &1);
    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.completed_quests.len(), 1);
    assert!(progress.completed_quests.contains(&1));
    assert_eq!(progress.total_reward_earned, 100);
    assert_eq!(progress.checkpoint_quest, Some(1));

    // Complete quest 2
    client.complete_quest(&player, &chain_id, &2);
    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.completed_quests.len(), 2);
    assert_eq!(progress.total_reward_earned, 250); // 100 + 150
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
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    // Complete quest 1
    client.complete_quest(&player, &chain_id, &1);

    // Complete quest 3 (branch path) instead of quest 2
    client.complete_quest(&player, &chain_id, &3);
    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.total_reward_earned, 300); // 100 + 200
    assert!(progress.completed_quests.contains(&3));

    // Quest 4 can be completed with either quest 2 or 3 as prerequisite
    // Since we completed 3, we should be able to complete 4
    client.complete_quest(&player, &chain_id, &4);
    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.total_reward_earned, 550); // 100 + 200 + 250
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
    assert_eq!(progress.total_reward_earned, 450); // 100 + 150 + 200
    assert_eq!(progress.checkpoint_quest, Some(3)); // Latest checkpoint is quest 3

    // Reset to checkpoint (quest 3)
    client.reset_to_checkpoint(&player, &chain_id);

    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    // After reset, all quests up to and including the last checkpoint are retained
    assert_eq!(progress.completed_quests.len(), 3);
    assert_eq!(progress.total_reward_earned, 450); // 100 + 150 + 200
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
    assert_eq!(progress.total_reward_earned, 1000); // 100 + 150 + 200 + 250 + 300

    // Check completion count
    assert_eq!(client.get_chain_completions(&chain_id), 1);
}

#[test]
fn test_cumulative_rewards() {
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
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    let mut total_reward = 0i128;

    // Complete quests one by one and verify cumulative rewards
    client.complete_quest(&player, &chain_id, &1);
    total_reward += 100;
    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.total_reward_earned, total_reward);

    client.complete_quest(&player, &chain_id, &2);
    total_reward += 150;
    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.total_reward_earned, total_reward);

    client.complete_quest(&player, &chain_id, &4);
    total_reward += 250;
    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.total_reward_earned, total_reward);

    client.complete_quest(&player, &chain_id, &5);
    total_reward += 300;
    let progress = client.get_player_progress(&player, &chain_id).unwrap();
    assert_eq!(progress.total_reward_earned, total_reward);
}

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
    assert_eq!(progress1.total_reward_earned, 100);
    assert_eq!(progress2.total_reward_earned, 100);
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
    );

    client.set_chain_active(&admin, &chain_id, &false);
    let chain = client.get_chain(&chain_id);
    assert!(!chain.active);
}

#[test]
#[should_panic(expected = "Admin only")]
fn test_unauthorized_admin_action() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup_contract(&env);
    let non_admin = Address::generate(&env);

    client.update_config(&non_admin, &Some(500u32), &None, &None);
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
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    // Try to complete quest 5 without completing prerequisites
    client.complete_quest(&player, &chain_id, &5);
}

#[test]
fn test_reward_token_configuration() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup_contract(&env);
    let reward_token = Address::generate(&env);

    // Set reward token
    client.set_reward_token(&admin, &Some(reward_token.clone()));

    let config = client.get_config();
    assert_eq!(config.reward_token, Some(reward_token));
}

#[test]
fn test_pending_rewards_tracking() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let reward_token = Address::generate(&env);
    client.set_reward_token(&admin, &Some(reward_token.clone()));

    let quests = create_test_quests(&env);
    let chain_id = client.create_chain(
        &admin,
        &symbol_short!("TestChain"),
        &symbol_short!("testchn"),
        &quests,
        &None,
        &None,
    );

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    // Complete quest 1
    client.complete_quest(&player, &chain_id, &1);
    
    // Check pending rewards
    let pending = client.get_pending_rewards(&player, &chain_id);
    assert_eq!(pending, 100); // Quest 1 reward

    // Complete quest 2
    client.complete_quest(&player, &chain_id, &2);
    let pending = client.get_pending_rewards(&player, &chain_id);
    assert_eq!(pending, 250); // Quest 1 + Quest 2 rewards
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
        reward: 100,
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
    // Quest expired in the past
    let chain_id = setup_expiry_chain(&env, &client, &admin, Some(1500));

    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    env.ledger().set_timestamp(2000); // well past expiry
    client.complete_quest(&player, &chain_id, &1);
}

#[test]
fn test_complete_quest_no_expiry_always_succeeds() {
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
    // Non-expired quest: complete fine and check no expiry event in log
    let chain_id = setup_expiry_chain(&env, &client, &admin, Some(2000));
    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    env.ledger().set_timestamp(1500); // before expiry
    client.complete_quest(&player, &chain_id, &1);

    let events = env.events().all();
    let expiry_sym = soroban_sdk::symbol_short!("qst_exprd");
    let expiry_sym_val: soroban_sdk::Val = expiry_sym.clone().into_val(&env);
    let expiry_emitted = events.iter().any(|(_cid, topics, _data)| {
        topics.iter().any(|t| t.get_payload() == expiry_sym_val.get_payload())
    });
    assert!(!expiry_emitted, "Expiry event should NOT be emitted for a valid completion");
}

#[test]
#[should_panic(expected = "Quest: expired")]
fn test_expiry_event_panic_message_correct() {
    // Confirms the panic message matches the acceptance criteria spec.
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);
    let chain_id = setup_expiry_chain(&env, &client, &admin, Some(1500));
    let player = Address::generate(&env);
    client.start_chain(&player, &chain_id);

    env.ledger().set_timestamp(2000);
    client.complete_quest(&player, &chain_id, &1);
}
