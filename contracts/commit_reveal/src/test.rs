#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Bytes, BytesN, Env};

fn setup() -> (Env, Address, CommitRevealContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, CommitRevealContract);
    let client = CommitRevealContractClient::new(&env, &contract_id);
    client.set_window(&100, &200);
    let player = Address::generate(&env);
    (env, player, client)
}

fn set_time(env: &Env, t: u64) {
    env.ledger().with_mut(|li| li.timestamp = t);
}

#[test]
fn valid_reveal_is_scored() {
    let (env, player, client) = setup();
    let solution = Bytes::from_slice(&env, b"hello");
    let salt = BytesN::from_array(&env, &[7u8; 32]);
    let hash = compute_hash(&env, &solution, &salt);

    set_time(&env, 50);
    client.commit(&player, &hash);

    set_time(&env, 150);
    let score = client.reveal(&player, &solution, &salt);
    assert_eq!(score, 5);
    assert_eq!(client.get_score(&player), 5);
    assert!(client.get_commitment(&player).unwrap().revealed);
}

#[test]
fn mismatched_reveal_is_rejected() {
    let (env, player, client) = setup();
    let solution = Bytes::from_slice(&env, b"hello");
    let salt = BytesN::from_array(&env, &[7u8; 32]);
    let hash = compute_hash(&env, &solution, &salt);
    set_time(&env, 50);
    client.commit(&player, &hash);

    set_time(&env, 150);
    let wrong = Bytes::from_slice(&env, b"world");
    let res = client.try_reveal(&player, &wrong, &salt);
    assert_eq!(res, Err(Ok(Error::HashMismatch)));
    assert_eq!(client.get_score(&player), 0);
}

#[test]
fn reveal_before_window_rejected() {
    let (env, player, client) = setup();
    let solution = Bytes::from_slice(&env, b"hi");
    let salt = BytesN::from_array(&env, &[1u8; 32]);
    let hash = compute_hash(&env, &solution, &salt);
    set_time(&env, 50);
    client.commit(&player, &hash);

    set_time(&env, 99);
    let res = client.try_reveal(&player, &solution, &salt);
    assert_eq!(res, Err(Ok(Error::RevealWindowNotOpen)));
}

#[test]
fn reveal_after_window_rejected() {
    let (env, player, client) = setup();
    let solution = Bytes::from_slice(&env, b"hi");
    let salt = BytesN::from_array(&env, &[1u8; 32]);
    let hash = compute_hash(&env, &solution, &salt);
    set_time(&env, 50);
    client.commit(&player, &hash);

    set_time(&env, 201);
    let res = client.try_reveal(&player, &solution, &salt);
    assert_eq!(res, Err(Ok(Error::RevealWindowClosed)));
}

#[test]
fn double_reveal_rejected() {
    let (env, player, client) = setup();
    let solution = Bytes::from_slice(&env, b"abc");
    let salt = BytesN::from_array(&env, &[2u8; 32]);
    let hash = compute_hash(&env, &solution, &salt);
    set_time(&env, 50);
    client.commit(&player, &hash);
    set_time(&env, 150);
    client.reveal(&player, &solution, &salt);
    let res = client.try_reveal(&player, &solution, &salt);
    assert_eq!(res, Err(Ok(Error::AlreadyRevealed)));
}

#[test]
fn unrevealed_commitment_is_forfeited_after_window() {
    let (env, player, client) = setup();
    let solution = Bytes::from_slice(&env, b"abc");
    let salt = BytesN::from_array(&env, &[3u8; 32]);
    let hash = compute_hash(&env, &solution, &salt);
    set_time(&env, 50);
    client.commit(&player, &hash);

    set_time(&env, 150);
    let early = client.try_forfeit(&player);
    assert_eq!(early, Err(Ok(Error::NotExpired)));

    set_time(&env, 201);
    client.forfeit(&player);
    assert!(client.get_commitment(&player).unwrap().forfeited);

    let res = client.try_reveal(&player, &solution, &salt);
    assert_eq!(res, Err(Ok(Error::RevealWindowClosed)));
}

#[test]
fn forfeit_of_revealed_commitment_rejected() {
    let (env, player, client) = setup();
    let solution = Bytes::from_slice(&env, b"abc");
    let salt = BytesN::from_array(&env, &[4u8; 32]);
    let hash = compute_hash(&env, &solution, &salt);
    set_time(&env, 50);
    client.commit(&player, &hash);
    set_time(&env, 150);
    client.reveal(&player, &solution, &salt);
    set_time(&env, 201);
    let res = client.try_forfeit(&player);
    assert_eq!(res, Err(Ok(Error::AlreadyRevealed)));
}
