#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

fn setup_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| {
        l.timestamp = 1000;
        l.sequence_number = 1;
    });
    env
}

fn setup_token<'a>(
    env: &'a Env,
    admin: &Address,
) -> (Address, TokenClient<'a>, StellarAssetClient<'a>) {
    let contract_address = env.register_stellar_asset_contract_v2(admin.clone()).address();
    let token = TokenClient::new(env, &contract_address);
    let token_admin = StellarAssetClient::new(env, &contract_address);
    (contract_address, token, token_admin)
}

fn setup_lottery(env: &Env) -> (Address, PuzzleLotteryContractClient) {
    let contract_id = env.register_contract(None, PuzzleLotteryContract);
    let client = PuzzleLotteryContractClient::new(env, &contract_id);
    (contract_id, client)
}

fn default_tiers(env: &Env) -> Vec<PrizeTier> {
    let mut tiers = Vec::new(env);
    tiers.push_back(PrizeTier {
        percent_bps: 5000,
        winner_count: 1,
    });
    tiers.push_back(PrizeTier {
        percent_bps: 3000,
        winner_count: 1,
    });
    tiers.push_back(PrizeTier {
        percent_bps: 2000,
        winner_count: 1,
    });
    tiers
}

#[test]
fn test_init() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let (token_id, _, _) = setup_token(&env, &owner);
    let (_, client) = setup_lottery(&env);

    client.init(&owner, &token_id);

    let current = client.get_current_round_id();
    assert_eq!(current, 0);
}

#[test]
fn test_start_round() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let (token_id, _, _) = setup_token(&env, &owner);
    let (_, client) = setup_lottery(&env);

    client.init(&owner, &token_id);
    let tiers = default_tiers(&env);
    let round_id = client.start_round(&owner, &100, &ScheduleType::Weekly, &tiers);

    assert_eq!(round_id, 1);
    let round = client.get_round(&1);
    assert_eq!(round.ticket_price, 100);
    assert_eq!(round.status, RoundStatus::Open);
    assert_eq!(round.schedule_type, ScheduleType::Weekly);
    assert_eq!(round.tiers.len(), 3);
}

#[test]
fn test_start_round_with_rollover() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let (token_id, _, _) = setup_token(&env, &owner);
    let (_, client) = setup_lottery(&env);

    client.init(&owner, &token_id);
    let tiers = default_tiers(&env);
    let round_id = client.start_round_with_rollover(
        &owner,
        &50,
        &ScheduleType::Monthly,
        &tiers,
        &1000,
    );

    assert_eq!(round_id, 1);
    let round = client.get_round(&1);
    assert_eq!(round.rollover, 1000);
    assert_eq!(round.schedule_type, ScheduleType::Monthly);
}

#[test]
fn test_buy_ticket() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let user = Address::generate(&env);
    let (token_id, token, token_admin) = setup_token(&env, &owner);
    let (contract_id, client) = setup_lottery(&env);

    client.init(&owner, &token_id);
    let tiers = default_tiers(&env);
    client.start_round(&owner, &100, &ScheduleType::Weekly, &tiers);

    token_admin.mint(&user, &500);
    token.approve(&user, &contract_id, &500, &env.ledger().sequence());

    client.buy_ticket(&user, &2);

    let round = client.get_round(&1);
    assert_eq!(round.total_tickets, 2);
    assert_eq!(round.prize_pool, 200);

    let count = client.get_ticket_count(&1, &user);
    assert_eq!(count, 2);

    let players = client.get_players(&1);
    assert_eq!(players.len(), 1);
    assert_eq!(players.get(0).unwrap().count, 2);
}

#[test]
#[should_panic(expected = "Round not active")]
fn test_buy_ticket_after_end() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let user = Address::generate(&env);
    let (token_id, token, token_admin) = setup_token(&env, &owner);
    let (contract_id, client) = setup_lottery(&env);

    client.init(&owner, &token_id);
    let tiers = default_tiers(&env);
    client.start_round(&owner, &100, &ScheduleType::Weekly, &tiers);

    env.ledger().with_mut(|l| l.timestamp += schedule_duration_sec(ScheduleType::Weekly) + 1);

    token_admin.mint(&user, &100);
    token.approve(&user, &contract_id, &100, &env.ledger().sequence());
    client.buy_ticket(&user, &1);
}

#[test]
fn test_draw_winner_and_claim() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);
    let (token_id, token, token_admin) = setup_token(&env, &owner);
    let (contract_id, client) = setup_lottery(&env);

    client.init(&owner, &token_id);
    let tiers = default_tiers(&env);
    client.start_round(&owner, &100, &ScheduleType::Weekly, &tiers);

    token_admin.mint(&u1, &100);
    token.approve(&u1, &contract_id, &100, &env.ledger().sequence());
    client.buy_ticket(&u1, &1);

    token_admin.mint(&u2, &200);
    token.approve(&u2, &contract_id, &200, &env.ledger().sequence());
    client.buy_ticket(&u2, &2);

    env.ledger().with_mut(|l| l.timestamp += schedule_duration_sec(ScheduleType::Weekly) + 1);
    client.draw_winner();

    let round = client.get_round(&1);
    assert_eq!(round.status, RoundStatus::Completed);
    assert!(round.winners.len() >= 1);
    assert!(round.winners.len() <= 3);

    let winner0 = round.winners.get(0).unwrap();
    let bal_before = token.balance(&winner0);
    client.claim_prize(&winner0, &1, &0);
    let bal_after = token.balance(&winner0);
    assert!(bal_after > bal_before);
}

#[test]
fn test_cancel_round_and_refund() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let user = Address::generate(&env);
    let (token_id, token, token_admin) = setup_token(&env, &owner);
    let (contract_id, client) = setup_lottery(&env);

    client.init(&owner, &token_id);
    let tiers = default_tiers(&env);
    client.start_round(&owner, &100, &ScheduleType::Weekly, &tiers);

    token_admin.mint(&user, &300);
    token.approve(&user, &contract_id, &300, &env.ledger().sequence());
    client.buy_ticket(&user, &3);

    client.cancel_round(&owner);

    let round = client.get_round(&1);
    assert_eq!(round.status, RoundStatus::Cancelled);

    let bal_before = token.balance(&user);
    client.refund(&user, &1);
    let bal_after = token.balance(&user);
    assert_eq!(bal_after - bal_before, 300);

    let count = client.get_ticket_count(&1, &user);
    assert_eq!(count, 0);
}

#[test]
#[should_panic(expected = "Round not cancelled")]
fn test_refund_when_not_cancelled() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let user = Address::generate(&env);
    let (token_id, token, token_admin) = setup_token(&env, &owner);
    let (contract_id, client) = setup_lottery(&env);

    client.init(&owner, &token_id);
    let tiers = default_tiers(&env);
    client.start_round(&owner, &100, &ScheduleType::Weekly, &tiers);

    token_admin.mint(&user, &100);
    token.approve(&user, &contract_id, &100, &env.ledger().sequence());
    client.buy_ticket(&user, &1);

    client.refund(&user, &1);
}

#[test]
fn test_prize_distribution_multiple_tiers() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);
    let u3 = Address::generate(&env);
    let (token_id, token, token_admin) = setup_token(&env, &owner);
    let (contract_id, client) = setup_lottery(&env);

    client.init(&owner, &token_id);
    let tiers = default_tiers(&env);
    client.start_round(&owner, &1000, &ScheduleType::Weekly, &tiers);

    for (addr, amount) in [(u1.clone(), 1000i128), (u2.clone(), 1000i128), (u3.clone(), 1000i128)]
    {
        token_admin.mint(&addr, &amount);
        token.approve(&addr, &contract_id, &amount, &env.ledger().sequence());
        client.buy_ticket(&addr, &1);
    }

    env.ledger().with_mut(|l| l.timestamp += schedule_duration_sec(ScheduleType::Weekly) + 1);
    client.draw_winner();

    let round = client.get_round(&1);
    assert_eq!(round.prize_pool, 3000);
    assert_eq!(round.winners.len(), 3);

    for i in 0..round.winners.len() {
        let w = round.winners.get(i).unwrap();
        client.claim_prize(&w, &1, &(i as u32));
    }

    let total_in_users = token.balance(&u1) + token.balance(&u2) + token.balance(&u3);
    assert_eq!(total_in_users, 3000);
}

#[test]
fn test_guaranteed_single_winner() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let user = Address::generate(&env);
    let (token_id, token, token_admin) = setup_token(&env, &owner);
    let (contract_id, client) = setup_lottery(&env);

    client.init(&owner, &token_id);
    let mut tiers = Vec::new(&env);
    tiers.push_back(PrizeTier {
        percent_bps: 10000,
        winner_count: 1,
    });

    client.start_round(&owner, &100, &ScheduleType::Weekly, &tiers);

    token_admin.mint(&user, &100);
    token.approve(&user, &contract_id, &100, &env.ledger().sequence());
    client.buy_ticket(&user, &1);

    env.ledger().with_mut(|l| l.timestamp += schedule_duration_sec(ScheduleType::Weekly) + 1);
    client.draw_winner();

    let round = client.get_round(&1);
    assert_eq!(round.status, RoundStatus::Completed);
    assert_eq!(round.winners.len(), 1);
    assert_eq!(round.winners.get(0).unwrap(), user);

    let bal_before = token.balance(&user);
    client.claim_prize(&user, &1, &0);
    assert_eq!(token.balance(&user) - bal_before, 100);
}
