#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, BytesN, Env,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_hash(env: &Env, seed: u8) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    BytesN::from_array(env, &bytes)
}

fn setup(env: &Env) -> (PuzzleBountyContractClient<'_>, Address, token::Client<'_>) {
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, PuzzleBountyContract);
    let client = PuzzleBountyContractClient::new(env, &contract_id);
    client.initialize(&admin);

    let token_admin = Address::generate(env);
    let token_id = env.register_stellar_asset_contract(token_admin.clone());
    let token_client = token::Client::new(env, &token_id);

    (client, admin, token_client)
}

fn mint(env: &Env, token: &token::Client<'_>, to: &Address, amount: i128) {
    let asset_client = token::StellarAssetClient::new(env, &token.address);
    asset_client.mint(to, &amount);
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn test_create_bounty_escrows_tokens() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, token) = setup(&env);

    let sponsor = Address::generate(&env);
    mint(&env, &token, &sponsor, 1000);

    let hash = make_hash(&env, 42);
    let id = client.create_bounty(&sponsor, &token.address, &1, &hash, &500, &300, &100, &3600);

    assert_eq!(id, 1);
    // Sponsor paid out 900 total; contract holds it
    assert_eq!(token.balance(&sponsor), 100);
    assert_eq!(token.balance(&client.address), 900);

    let bounty = client.get_bounty(&id).unwrap();
    assert_eq!(bounty.status, BountyStatus::Open);
    assert_eq!(bounty.winner_count, 0);
}

#[test]
fn test_first_solver_gets_top_reward() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, token) = setup(&env);

    let sponsor = Address::generate(&env);
    let solver = Address::generate(&env);
    mint(&env, &token, &sponsor, 1000);

    let correct = make_hash(&env, 7);
    let id = client.create_bounty(
        &sponsor,
        &token.address,
        &1,
        &correct,
        &500,
        &300,
        &100,
        &3600,
    );

    let rank = client.claim_bounty(&solver, &id, &correct);
    assert_eq!(rank, 1);
    assert_eq!(token.balance(&solver), 500);

    let bounty = client.get_bounty(&id).unwrap();
    assert_eq!(bounty.winner_count, 1);
    assert_eq!(bounty.status, BountyStatus::Open); // still open for 2nd/3rd
}

#[test]
fn test_multi_winner_top3_distribution() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, token) = setup(&env);

    let sponsor = Address::generate(&env);
    let solver1 = Address::generate(&env);
    let solver2 = Address::generate(&env);
    let solver3 = Address::generate(&env);
    mint(&env, &token, &sponsor, 1000);

    let hash = make_hash(&env, 99);
    let id = client.create_bounty(&sponsor, &token.address, &2, &hash, &500, &300, &100, &3600);

    let r1 = client.claim_bounty(&solver1, &id, &hash);
    let r2 = client.claim_bounty(&solver2, &id, &hash);
    let r3 = client.claim_bounty(&solver3, &id, &hash);

    assert_eq!((r1, r2, r3), (1, 2, 3));
    assert_eq!(token.balance(&solver1), 500);
    assert_eq!(token.balance(&solver2), 300);
    assert_eq!(token.balance(&solver3), 100);
    assert_eq!(token.balance(&client.address), 0);

    let bounty = client.get_bounty(&id).unwrap();
    assert_eq!(bounty.status, BountyStatus::Completed);

    let lb = client.get_leaderboard(&id);
    assert_eq!(lb.len(), 3);
    assert_eq!(lb.get(0).unwrap().rank, 1);
    assert_eq!(lb.get(1).unwrap().rank, 2);
    assert_eq!(lb.get(2).unwrap().rank, 3);
}

#[test]
fn test_refund_expired_unclaimed() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, token) = setup(&env);

    let sponsor = Address::generate(&env);
    mint(&env, &token, &sponsor, 1000);

    let hash = make_hash(&env, 1);
    let id = client.create_bounty(
        &sponsor,
        &token.address,
        &3,
        &hash,
        &500,
        &200,
        &100,
        &100, // 100 seconds duration
    );
    assert_eq!(token.balance(&sponsor), 200); // 1000 - 800

    // One solver claims 1st place
    let solver = Address::generate(&env);
    client.claim_bounty(&solver, &id, &hash);
    assert_eq!(token.balance(&solver), 500);

    // Fast-forward past expiration
    env.ledger().with_mut(|l| l.timestamp = l.timestamp + 200);

    // Refund: 2nd + 3rd (300) should go back to sponsor
    client.refund_expired(&id);
    // sponsor had 200 remaining, now gets 200+100 = 300 back → total 500
    assert_eq!(token.balance(&sponsor), 200 + 300);
    assert_eq!(token.balance(&client.address), 0);

    let bounty = client.get_bounty(&id).unwrap();
    assert_eq!(bounty.status, BountyStatus::Expired);
}

#[test]
fn test_cancel_open_bounty_refunds_sponsor() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, token) = setup(&env);

    let sponsor = Address::generate(&env);
    mint(&env, &token, &sponsor, 1000);

    let hash = make_hash(&env, 5);
    let id = client.create_bounty(&sponsor, &token.address, &4, &hash, &400, &200, &100, &3600);
    assert_eq!(token.balance(&sponsor), 300);

    client.cancel_bounty(&sponsor, &id);
    // Full 700 returned
    assert_eq!(token.balance(&sponsor), 1000);
    assert_eq!(token.balance(&client.address), 0);

    let bounty = client.get_bounty(&id).unwrap();
    assert_eq!(bounty.status, BountyStatus::Cancelled);
}

#[test]
#[should_panic(expected = "Bounty has expired")]
fn test_cannot_claim_after_expiry() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, token) = setup(&env);

    let sponsor = Address::generate(&env);
    let solver = Address::generate(&env);
    mint(&env, &token, &sponsor, 1000);

    let hash = make_hash(&env, 11);
    let id = client.create_bounty(&sponsor, &token.address, &5, &hash, &500, &0, &0, &100);

    // Fast-forward past expiration
    env.ledger().with_mut(|l| l.timestamp = l.timestamp + 200);
    client.claim_bounty(&solver, &id, &hash);
}

#[test]
#[should_panic(expected = "Incorrect solution")]
fn test_wrong_solution_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, token) = setup(&env);

    let sponsor = Address::generate(&env);
    let solver = Address::generate(&env);
    mint(&env, &token, &sponsor, 1000);

    let correct = make_hash(&env, 77);
    let wrong = make_hash(&env, 88);
    let id = client.create_bounty(&sponsor, &token.address, &6, &correct, &500, &0, &0, &3600);

    client.claim_bounty(&solver, &id, &wrong);
}

#[test]
#[should_panic(expected = "Cannot cancel after winners have claimed")]
fn test_cannot_cancel_after_claim() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, token) = setup(&env);

    let sponsor = Address::generate(&env);
    let solver = Address::generate(&env);
    mint(&env, &token, &sponsor, 1000);

    let hash = make_hash(&env, 33);
    let id = client.create_bounty(&sponsor, &token.address, &7, &hash, &500, &300, &100, &3600);

    client.claim_bounty(&solver, &id, &hash);
    client.cancel_bounty(&sponsor, &id);
}

#[test]
fn test_leaderboard_tracks_winners() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, token) = setup(&env);

    let sponsor = Address::generate(&env);
    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    mint(&env, &token, &sponsor, 1000);

    let hash = make_hash(&env, 55);
    let id = client.create_bounty(&sponsor, &token.address, &8, &hash, &400, &200, &100, &3600);

    client.claim_bounty(&s1, &id, &hash);
    client.claim_bounty(&s2, &id, &hash);

    let lb = client.get_leaderboard(&id);
    assert_eq!(lb.len(), 2);
    assert_eq!(lb.get(0).unwrap().solver, s1);
    assert_eq!(lb.get(0).unwrap().rank, 1);
    assert_eq!(lb.get(1).unwrap().solver, s2);
    assert_eq!(lb.get(1).unwrap().rank, 2);
}
