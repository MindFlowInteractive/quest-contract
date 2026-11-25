#![cfg(test)]

use super::*;
use soroban_sdk::testutils::Ledger as _;

fn setup() -> (Env, SeedBeaconContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, SeedBeaconContract);
    let client = SeedBeaconContractClient::new(&env, &id);
    client.initialize();
    (env, client)
}

#[test]
#[should_panic(expected = "already initialized")]
fn initialize_rejects_double_initialization() {
    let (_env, client) = setup();
    client.initialize();
}

#[test]
#[should_panic(expected = "seed not yet ready")]
fn seed_requests_are_rejected_before_the_committed_ledger() {
    let (env, client) = setup();
    env.ledger().set_sequence_number(0);
    client.begin_round();
    client.request_seed();
}

#[test]
fn seeds_are_revealed_once_the_committed_ledger_is_reached() {
    let (env, client) = setup();
    env.ledger().set_sequence_number(0);
    let round = client.begin_round();

    assert_eq!(round, 1);
    assert_eq!(client.committed_ledger(), 5);

    env.ledger().set_sequence_number(5);
    let seed = client.request_seed();
    assert_eq!(seed.to_array().len(), 32);
    assert_eq!(client.verify_seed(), Some(seed));
}

#[test]
fn repeated_requests_return_the_same_seed() {
    let (env, client) = setup();
    env.ledger().set_sequence_number(0);
    client.begin_round();
    env.ledger().set_sequence_number(10);
    let first = client.request_seed();
    let second = client.request_seed();
    assert_eq!(first, second);
}

#[test]
fn seeds_differ_per_round() {
    let (env, client) = setup();
    env.ledger().set_sequence_number(0);
    client.begin_round();
    env.ledger().set_sequence_number(5);
    let first = client.request_seed();
    env.ledger().set_sequence_number(10);
    assert_eq!(client.begin_round(), 2);
    env.ledger().set_sequence_number(15);
    let second = client.request_seed();
    assert_ne!(first, second);
}

#[test]
fn begin_round_commits_to_a_new_ledger_and_increments_round_number() {
    let (env, client) = setup();
    env.ledger().set_sequence_number(10);
    assert_eq!(client.begin_round(), 1);
    assert_eq!(client.committed_ledger(), 15);
    env.ledger().set_sequence_number(20);
    assert_eq!(client.begin_round(), 2);
    assert_eq!(client.round_number(), 2);
    assert_eq!(client.committed_ledger(), 25);
}

#[test]
fn verify_seed_is_none_before_a_seed_is_revealed() {
    let (_env, client) = setup();
    client.begin_round();
    assert_eq!(client.verify_seed(), None);
}
