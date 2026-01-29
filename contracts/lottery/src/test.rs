#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient},
    Address, Env,
};

fn setup_env() -> Env {
    let env = Env::default();
    env.ledger().with_mut(|l| {
        l.timestamp = 100;
        l.sequence_number = 1;
    });
    env
}

fn setup_token(env: &Env, admin: &Address) -> (Address, TokenClient) {
    let token_id = env.register_stellar_asset_contract(admin.clone());
    let token = TokenClient::new(env, &token_id);
    (token_id, token)
}

fn setup_lottery(env: &Env, owner: &Address, token: &Address) {
    LotteryContract::init(env.clone(), owner.clone(), token.clone());
}

#[test]
fn test_init() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let (token, _) = setup_token(&env, &owner);

    setup_lottery(&env, &owner, &token);

    let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
    assert_eq!(stored_owner, owner);
}

#[test]
fn test_start_round() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let (token, _) = setup_token(&env, &owner);

    setup_lottery(&env, &owner, &token);

    start_round(env.clone(), 100, 50);

    let round_id: u32 = env.storage().instance().get(&DataKey::CurrentRound).unwrap();
    let round: LotteryRound = env.storage().persistent().get(&DataKey::Round(round_id)).unwrap();

    assert_eq!(round.ticket_price, 100);
    assert_eq!(round.status, RoundStatus::Open);
}

#[test]
fn test_buy_ticket() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    let (token_id, token) = setup_token(&env, &owner);
    setup_lottery(&env, &owner, &token_id);

    start_round(env.clone(), 100, 50);

    token.mint(&user, &100);
    token.approve(
        &user,
        &env.current_contract_address(),
        &100,
        &env.ledger().sequence(),
    );

    buy_ticket(env.clone(), user.clone());

    let round_id: u32 = env.storage().instance().get(&DataKey::CurrentRound).unwrap();
    let players: Vec<Address> =
        env.storage().persistent().get(&DataKey::Players(round_id)).unwrap();

    assert_eq!(players.len(), 1);
}

#[test]
#[should_panic(expected = "Round not open")]
fn test_buy_ticket_closed_round() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    let (token_id, _) = setup_token(&env, &owner);
    setup_lottery(&env, &owner, &token_id);

    start_round(env.clone(), 100, 0);

    env.ledger().with_mut(|l| l.timestamp += 100);

    buy_ticket(env.clone(), user);
}

#[test]
fn test_draw_winner() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    let (token_id, token) = setup_token(&env, &owner);
    setup_lottery(&env, &owner, &token_id);

    start_round(env.clone(), 100, 10);

    token.mint(&user, &100);
    token.approve(
        &user,
        &env.current_contract_address(),
        &100,
        &env.ledger().sequence(),
    );

    buy_ticket(env.clone(), user.clone());

    env.ledger().with_mut(|l| l.timestamp += 20);

    draw_winner(env.clone());

    let round_id: u32 = env.storage().instance().get(&DataKey::CurrentRound).unwrap();
    let round: LotteryRound = env.storage().persistent().get(&DataKey::Round(round_id)).unwrap();

    assert_eq!(round.status, RoundStatus::Completed);
    assert!(round.winner.is_some());
}

#[test]
#[should_panic(expected = "Not winner")]
fn test_claim_prize_not_winner() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let user = Address::generate(&env);

    let (token_id, _) = setup_token(&env, &owner);
    setup_lottery(&env, &owner, &token_id);

    start_round(env.clone(), 100, 0);
    env.ledger().with_mut(|l| l.timestamp += 1);

    draw_winner(env.clone());

    claim_prize(env.clone(), user, 1);
}


#[test]
fn test_cancel_round() {
    let env = setup_env();
    let owner = Address::generate(&env);
    let (token_id, _) = setup_token(&env, &owner);

    setup_lottery(&env, &owner, &token_id);
    start_round(env.clone(), 100, 50);

    cancel_round(env.clone());

    let round_id: u32 = env.storage().instance().get(&DataKey::CurrentRound).unwrap();
    let round: LotteryRound = env.storage().persistent().get(&DataKey::Round(round_id)).unwrap();

    assert_eq!(round.status, RoundStatus::Cancelled);
}

#[test]
#[should_panic]
fn test_refund_unimplemented() {
    let env = setup_env();
    let user = Address::generate(&env);

    refund(env, 1, user);
}