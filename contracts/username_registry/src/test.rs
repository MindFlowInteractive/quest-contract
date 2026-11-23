#![cfg(test)]

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, String};

fn setup() -> (Env, UsernameRegistryClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, UsernameRegistry);
    let client = UsernameRegistryClient::new(&env, &id);
    (env, client)
}

#[test]
fn test_claim_and_resolve() {
    let (env, client) = setup();
    let alice = Address::generate(&env);
    let name = String::from_str(&env, "alice_01");
    client.claim(&alice, &name);
    assert_eq!(client.resolve(&name), Some(alice));
}

#[test]
fn test_duplicate_claim_rejected() {
    let (env, client) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let name = String::from_str(&env, "shared");
    client.claim(&alice, &name);
    let res = client.try_claim(&bob, &name);
    assert_eq!(res, Err(Ok(Error::AlreadyTaken)));
}

#[test]
fn test_invalid_length_rejected() {
    let (env, client) = setup();
    let alice = Address::generate(&env);
    let short = String::from_str(&env, "ab");
    assert_eq!(
        client.try_claim(&alice, &short),
        Err(Ok(Error::InvalidLength))
    );
    let long = String::from_str(&env, "a_very_long_username_that_exceeds_limit_xx");
    assert_eq!(
        client.try_claim(&alice, &long),
        Err(Ok(Error::InvalidLength))
    );
}

#[test]
fn test_invalid_characters_rejected() {
    let (env, client) = setup();
    let alice = Address::generate(&env);
    let upper = String::from_str(&env, "Alice");
    let space = String::from_str(&env, "al ice");
    assert_eq!(
        client.try_claim(&alice, &upper),
        Err(Ok(Error::InvalidCharacter))
    );
    assert_eq!(
        client.try_claim(&alice, &space),
        Err(Ok(Error::InvalidCharacter))
    );
}

#[test]
fn test_renew_extends_expiry() {
    let (env, client) = setup();
    let alice = Address::generate(&env);
    let name = String::from_str(&env, "renewme");
    client.claim(&alice, &name);
    let before = client.expiry_of(&name).unwrap();
    env.ledger().with_mut(|l| l.sequence_number += 100);
    let new_expiry = client.renew(&alice, &name);
    assert!(new_expiry > before);
    assert_eq!(client.expiry_of(&name), Some(new_expiry));
}

#[test]
fn test_renew_by_non_owner_rejected() {
    let (env, client) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let name = String::from_str(&env, "owned");
    client.claim(&alice, &name);
    assert_eq!(client.try_renew(&bob, &name), Err(Ok(Error::NotOwner)));
}

#[test]
fn test_expired_name_reclaimable() {
    let (env, client) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let name = String::from_str(&env, "expiring");
    client.claim(&alice, &name);
    let expiry = client.expiry_of(&name).unwrap();
    env.ledger().with_mut(|l| l.sequence_number = expiry);
    assert_eq!(client.resolve(&name), None);
    client.claim(&bob, &name);
    assert_eq!(client.resolve(&name), Some(bob));
}

#[test]
fn test_renew_expired_rejected() {
    let (env, client) = setup();
    let alice = Address::generate(&env);
    let name = String::from_str(&env, "gone");
    client.claim(&alice, &name);
    let expiry = client.expiry_of(&name).unwrap();
    env.ledger().with_mut(|l| l.sequence_number = expiry);
    assert_eq!(client.try_renew(&alice, &name), Err(Ok(Error::NotFound)));
}
