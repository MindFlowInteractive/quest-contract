#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

fn setup_contract(env: &Env) -> (LeaderboardContractClient, Address) {
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, LeaderboardContract);
    let client = LeaderboardContractClient::new(env, &contract_id);

    client.initialize(&admin, &100u32);

    (client, admin)
}

#[test]
fn test_initialization() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup_contract(&env);

    let config = client.get_config();
    assert_eq!(config.admin, admin);
    assert_eq!(config.max_top_entries, 100);
    assert_eq!(config.daily_period_length, 86_400);
    assert_eq!(config.weekly_period_length, 604_800);
    assert!(!config.paused);
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_double_initialization() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup_contract(&env);

    // Try to initialize again
    client.initialize(&admin, &100u32);
}

#[test]
fn test_submit_score() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);

    let player = Address::generate(&env);

    // Submit score as admin
    client.submit_score(&admin, &player, &5000);

    // Verify player's score
    let score = client.get_player_score(&player, &TimePeriod::AllTime).unwrap();
    assert_eq!(score.score, 5000);
    assert_eq!(score.player, player);
    assert_eq!(score.timestamp, 1000);

    // Verify all-time total
    assert_eq!(client.get_player_all_time_total(&player), 5000);

    // Verify total players
    assert_eq!(client.get_total_players(), 1);
}

#[test]
fn test_cumulative_score() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);

    let player = Address::generate(&env);

    // Submit multiple scores
    client.submit_score(&admin, &player, &1000);
    client.submit_score(&admin, &player, &2000);
    client.submit_score(&admin, &player, &1500);

    // Verify cumulative score within same period
    let score = client.get_player_score(&player, &TimePeriod::AllTime).unwrap();
    assert_eq!(score.score, 4500);

    // Verify all-time total
    assert_eq!(client.get_player_all_time_total(&player), 4500);

    // Total players should still be 1
    assert_eq!(client.get_total_players(), 1);
}

#[test]
fn test_top_players_ranking() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);

    let player1 = Address::generate(&env);
    let player2 = Address::generate(&env);
    let player3 = Address::generate(&env);
    let player4 = Address::generate(&env);

    // Submit scores in different order than ranking
    client.submit_score(&admin, &player2, &2000);
    client.submit_score(&admin, &player4, &4000);
    client.submit_score(&admin, &player1, &1000);
    client.submit_score(&admin, &player3, &3000);

    // Get top players
    let top = client.get_top_players(&TimePeriod::AllTime, &10);
    assert_eq!(top.len(), 4);

    // Verify sorted order (descending)
    assert_eq!(top.get(0).unwrap().player, player4);
    assert_eq!(top.get(0).unwrap().score, 4000);
    assert_eq!(top.get(1).unwrap().player, player3);
    assert_eq!(top.get(1).unwrap().score, 3000);
    assert_eq!(top.get(2).unwrap().player, player2);
    assert_eq!(top.get(2).unwrap().score, 2000);
    assert_eq!(top.get(3).unwrap().player, player1);
    assert_eq!(top.get(3).unwrap().score, 1000);
}

#[test]
fn test_player_rank() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);

    let player1 = Address::generate(&env);
    let player2 = Address::generate(&env);
    let player3 = Address::generate(&env);

    client.submit_score(&admin, &player1, &1000);
    client.submit_score(&admin, &player2, &3000);
    client.submit_score(&admin, &player3, &2000);

    // Verify ranks
    assert_eq!(client.get_player_rank(&player2, &TimePeriod::AllTime), 1);
    assert_eq!(client.get_player_rank(&player3, &TimePeriod::AllTime), 2);
    assert_eq!(client.get_player_rank(&player1, &TimePeriod::AllTime), 3);

    // Non-existent player
    let unknown = Address::generate(&env);
    assert_eq!(client.get_player_rank(&unknown, &TimePeriod::AllTime), 0);
}

#[test]
fn test_rank_update_on_score_change() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);

    let player1 = Address::generate(&env);
    let player2 = Address::generate(&env);

    // Initial scores
    client.submit_score(&admin, &player1, &1000);
    client.submit_score(&admin, &player2, &2000);

    // Player2 is rank 1
    assert_eq!(client.get_player_rank(&player2, &TimePeriod::AllTime), 1);
    assert_eq!(client.get_player_rank(&player1, &TimePeriod::AllTime), 2);

    // Player1 gets more points and overtakes
    client.submit_score(&admin, &player1, &2500);

    // Ranks should be swapped
    assert_eq!(client.get_player_rank(&player1, &TimePeriod::AllTime), 1);
    assert_eq!(client.get_player_rank(&player2, &TimePeriod::AllTime), 2);
}

#[test]
fn test_daily_period() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup_contract(&env);
    let player = Address::generate(&env);

    // Day 1 (timestamp 0)
    env.ledger().set_timestamp(0);
    client.submit_score(&admin, &player, &1000);

    // Verify daily score
    let daily_score = client.get_player_score(&player, &TimePeriod::Daily);
    assert!(daily_score.is_some());
    assert_eq!(daily_score.unwrap().score, 1000);

    // Move to Day 2 (timestamp 86400)
    env.ledger().set_timestamp(86_400);
    
    // Daily score should be None for new period
    let daily_score = client.get_player_score(&player, &TimePeriod::Daily);
    assert!(daily_score.is_none());

    // Submit score for Day 2
    client.submit_score(&admin, &player, &2000);
    let daily_score = client.get_player_score(&player, &TimePeriod::Daily);
    assert_eq!(daily_score.unwrap().score, 2000);

    // All-time should be cumulative
    assert_eq!(client.get_player_all_time_total(&player), 3000);
}

#[test]
fn test_weekly_period() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup_contract(&env);
    let player = Address::generate(&env);

    // Week 1 (timestamp 0)
    env.ledger().set_timestamp(0);
    client.submit_score(&admin, &player, &5000);

    // Verify weekly score
    let weekly_score = client.get_player_score(&player, &TimePeriod::Weekly);
    assert!(weekly_score.is_some());
    assert_eq!(weekly_score.unwrap().score, 5000);

    // Move to Week 2 (timestamp 604800)
    env.ledger().set_timestamp(604_800);
    
    // Weekly score should be None for new period
    let weekly_score = client.get_player_score(&player, &TimePeriod::Weekly);
    assert!(weekly_score.is_none());

    // Submit score for Week 2
    client.submit_score(&admin, &player, &7000);
    let weekly_score = client.get_player_score(&player, &TimePeriod::Weekly);
    assert_eq!(weekly_score.unwrap().score, 7000);

    // All-time should be cumulative
    assert_eq!(client.get_player_all_time_total(&player), 12000);
}

#[test]
fn test_high_score_record() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);

    let player1 = Address::generate(&env);
    let player2 = Address::generate(&env);

    // Initial high score
    client.submit_score(&admin, &player1, &5000);
    assert_eq!(client.get_high_score(&TimePeriod::AllTime), 5000);

    // New high score
    client.submit_score(&admin, &player2, &8000);
    assert_eq!(client.get_high_score(&TimePeriod::AllTime), 8000);

    // Score that doesn't beat high score
    client.submit_score(&admin, &player1, &1000);
    assert_eq!(client.get_high_score(&TimePeriod::AllTime), 8000);
}

#[test]
fn test_verifier_system() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);

    let verifier = Address::generate(&env);
    let player = Address::generate(&env);

    // Initially not a verifier
    assert!(!client.is_verifier(&verifier));

    // Add verifier
    client.add_verifier(&admin, &verifier);
    assert!(client.is_verifier(&verifier));

    // Verifier can submit scores
    client.submit_score(&verifier, &player, &3000);
    assert_eq!(client.get_player_all_time_total(&player), 3000);

    // Remove verifier
    client.remove_verifier(&admin, &verifier);
    assert!(!client.is_verifier(&verifier));
}

#[test]
#[should_panic(expected = "Unauthorized submitter")]
fn test_unauthorized_score_submission() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, _admin) = setup_contract(&env);

    let random_user = Address::generate(&env);
    let player = Address::generate(&env);

    // Non-admin, non-verifier tries to submit
    client.submit_score(&random_user, &player, &1000);
}

#[test]
fn test_update_score_admin() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);

    let player = Address::generate(&env);

    // Submit initial score
    client.submit_score(&admin, &player, &5000);

    // Admin updates score directly (replace, not add)
    client.update_score(&admin, &player, &7000, &TimePeriod::AllTime);

    let score = client.get_player_score(&player, &TimePeriod::AllTime).unwrap();
    assert_eq!(score.score, 7000);
}

#[test]
fn test_pause_functionality() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);

    // Pause contract
    client.set_paused(&admin, &true);

    let config = client.get_config();
    assert!(config.paused);

    // Unpause
    client.set_paused(&admin, &false);
    let config = client.get_config();
    assert!(!config.paused);
}

#[test]
#[should_panic(expected = "Contract is paused")]
fn test_submit_when_paused() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);

    let player = Address::generate(&env);

    // Pause contract
    client.set_paused(&admin, &true);

    // Try to submit score (should fail)
    client.submit_score(&admin, &player, &1000);
}

#[test]
fn test_update_period_lengths() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup_contract(&env);

    // Update period lengths
    client.update_period_lengths(&admin, &43_200, &302_400);

    let config = client.get_config();
    assert_eq!(config.daily_period_length, 43_200);   // 12 hours
    assert_eq!(config.weekly_period_length, 302_400); // 3.5 days
}

#[test]
fn test_update_max_entries() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup_contract(&env);

    client.update_max_entries(&admin, &50);

    let config = client.get_config();
    assert_eq!(config.max_top_entries, 50);
}

#[test]
#[should_panic(expected = "Max entries must be positive")]
fn test_update_max_entries_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup_contract(&env);

    client.update_max_entries(&admin, &0);
}

#[test]
#[should_panic(expected = "Admin only")]
fn test_add_verifier_non_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin) = setup_contract(&env);

    let random_user = Address::generate(&env);
    let verifier = Address::generate(&env);

    client.add_verifier(&random_user, &verifier);
}

#[test]
#[should_panic(expected = "Score must be non-negative")]
fn test_negative_score() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);

    let player = Address::generate(&env);

    client.submit_score(&admin, &player, &-100);
}

#[test]
fn test_top_players_limit() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, LeaderboardContract);
    let client = LeaderboardContractClient::new(&env, &contract_id);

    // Initialize with max 3 entries
    client.initialize(&admin, &3);

    let player1 = Address::generate(&env);
    let player2 = Address::generate(&env);
    let player3 = Address::generate(&env);
    let player4 = Address::generate(&env);
    let player5 = Address::generate(&env);

    // Submit 5 scores
    client.submit_score(&admin, &player1, &1000);
    client.submit_score(&admin, &player2, &2000);
    client.submit_score(&admin, &player3, &3000);
    client.submit_score(&admin, &player4, &4000);
    client.submit_score(&admin, &player5, &5000);

    // Should only keep top 3
    let top = client.get_top_players(&TimePeriod::AllTime, &10);
    assert_eq!(top.len(), 3);

    // Verify only top 3 scores are kept
    assert_eq!(top.get(0).unwrap().score, 5000);
    assert_eq!(top.get(1).unwrap().score, 4000);
    assert_eq!(top.get(2).unwrap().score, 3000);

    // Players with lower scores should have rank 0
    assert_eq!(client.get_player_rank(&player5, &TimePeriod::AllTime), 1);
    assert_eq!(client.get_player_rank(&player4, &TimePeriod::AllTime), 2);
    assert_eq!(client.get_player_rank(&player3, &TimePeriod::AllTime), 3);
    assert_eq!(client.get_player_rank(&player2, &TimePeriod::AllTime), 0); // Not in top 3
    assert_eq!(client.get_player_rank(&player1, &TimePeriod::AllTime), 0); // Not in top 3
}

#[test]
fn test_multiple_periods_independent() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);

    let player1 = Address::generate(&env);
    let player2 = Address::generate(&env);

    // Submit scores
    client.submit_score(&admin, &player1, &1000);
    client.submit_score(&admin, &player2, &2000);

    // All periods should have same rankings initially
    assert_eq!(client.get_player_rank(&player2, &TimePeriod::Daily), 1);
    assert_eq!(client.get_player_rank(&player2, &TimePeriod::Weekly), 1);
    assert_eq!(client.get_player_rank(&player2, &TimePeriod::AllTime), 1);

    // Move to next day
    env.ledger().set_timestamp(86_400 + 1000);

    // Daily rankings should reset
    assert_eq!(client.get_player_rank(&player2, &TimePeriod::Daily), 0);
    
    // Weekly and All-time should remain
    assert_eq!(client.get_player_rank(&player2, &TimePeriod::Weekly), 1);
    assert_eq!(client.get_player_rank(&player2, &TimePeriod::AllTime), 1);
}

#[test]
fn test_get_current_period_id() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin) = setup_contract(&env);

    // Day 0
    env.ledger().set_timestamp(0);
    assert_eq!(client.get_current_period_id_view(&TimePeriod::Daily), 0);
    assert_eq!(client.get_current_period_id_view(&TimePeriod::Weekly), 0);
    assert_eq!(client.get_current_period_id_view(&TimePeriod::AllTime), 0);

    // Day 1
    env.ledger().set_timestamp(86_400);
    assert_eq!(client.get_current_period_id_view(&TimePeriod::Daily), 1);
    assert_eq!(client.get_current_period_id_view(&TimePeriod::Weekly), 0);

    // Week 1
    env.ledger().set_timestamp(604_800);
    assert_eq!(client.get_current_period_id_view(&TimePeriod::Daily), 7);
    assert_eq!(client.get_current_period_id_view(&TimePeriod::Weekly), 1);
    assert_eq!(client.get_current_period_id_view(&TimePeriod::AllTime), 0);
}

#[test]
fn test_full_leaderboard_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(0);

    let (client, admin) = setup_contract(&env);

    // Add a verifier
    let verifier = Address::generate(&env);
    client.add_verifier(&admin, &verifier);

    // Create players
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let charlie = Address::generate(&env);

    // Day 1: Initial scores
    client.submit_score(&verifier, &alice, &1000);
    client.submit_score(&verifier, &bob, &1500);
    client.submit_score(&admin, &charlie, &800);

    // Verify rankings
    assert_eq!(client.get_player_rank(&bob, &TimePeriod::Daily), 1);
    assert_eq!(client.get_player_rank(&alice, &TimePeriod::Daily), 2);
    assert_eq!(client.get_player_rank(&charlie, &TimePeriod::Daily), 3);

    // Day 1: More scores
    client.submit_score(&verifier, &alice, &1000); // Alice now has 2000
    assert_eq!(client.get_player_rank(&alice, &TimePeriod::Daily), 1);
    assert_eq!(client.get_player_rank(&bob, &TimePeriod::Daily), 2);

    // Check high score
    assert_eq!(client.get_high_score(&TimePeriod::Daily), 2000);

    // Day 2: New period
    env.ledger().set_timestamp(86_400);

    // Daily rankings reset
    let daily_top = client.get_top_players(&TimePeriod::Daily, &10);
    assert_eq!(daily_top.len(), 0);

    // Weekly still has scores
    let weekly_top = client.get_top_players(&TimePeriod::Weekly, &10);
    assert_eq!(weekly_top.len(), 3);

    // Submit new daily scores
    client.submit_score(&verifier, &charlie, &3000);
    assert_eq!(client.get_player_rank(&charlie, &TimePeriod::Daily), 1);

    // All-time totals
    assert_eq!(client.get_player_all_time_total(&alice), 2000);
    assert_eq!(client.get_player_all_time_total(&bob), 1500);
    assert_eq!(client.get_player_all_time_total(&charlie), 3800); // 800 + 3000

    // Total unique players
    assert_eq!(client.get_total_players(), 3);

    // Week 2: Weekly reset
    env.ledger().set_timestamp(604_800);

    let weekly_top = client.get_top_players(&TimePeriod::Weekly, &10);
    assert_eq!(weekly_top.len(), 0);

    // All-time still preserved
    let all_time_top = client.get_top_players(&TimePeriod::AllTime, &10);
    assert_eq!(all_time_top.len(), 3);
    assert_eq!(all_time_top.get(0).unwrap().player, charlie);
}

#[test]
fn test_zero_score_submission() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin) = setup_contract(&env);

    let player = Address::generate(&env);

    // Zero score should be allowed
    client.submit_score(&admin, &player, &0);

    let score = client.get_player_score(&player, &TimePeriod::AllTime);
    assert!(score.is_some());
    assert_eq!(score.unwrap().score, 0);
}
