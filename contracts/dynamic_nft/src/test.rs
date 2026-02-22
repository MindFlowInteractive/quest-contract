#![cfg(test)]

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};

fn setup() -> (Env, Address, Address, DynamicNftContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, DynamicNftContract);
    let client = DynamicNftContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin);

    (env, admin, user, client)
}

#[test]
fn mint_and_get() {
    let (env, _admin, user, client) = setup();
    env.ledger().set_timestamp(100);
    let token = client.mint(&user, &user, &String::from_str(&env, "meta_v1"), &String::from_str(&env, "traitA"));
    assert_eq!(token, 1);

    let nft = client.get_nft(&token).unwrap();
    assert_eq!(nft.owner, user);
    assert_eq!(nft.level, 1);
    // no history on fresh mint
    let history = client.get_history(&token).unwrap();
    assert_eq!(history.len(), 0);
}

#[test]
fn time_evolution_changes_level() {
    let (env, _admin, user, client) = setup();
    env.ledger().set_timestamp(100);

    let token = client.mint(&user, &user, &String::from_str(&env, "meta_v1"), &String::from_str(&env, "traitA"));

    // not ready yet
    let res = std::panic::catch_unwind(|| client.evolve_time(&user, &token, &10u64));
    assert!(res.is_err());

    env.ledger().set_timestamp(200);
    client.evolve_time(&user, &token, &50u64);
    let nft = client.get_nft(&token).unwrap();
    assert_eq!(nft.level, 2);
    let history = client.get_history(&token).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history.get(0).unwrap(), &String::from_str(&env, "time_evolved"));
}

#[test]
fn fuse_two_tokens() {
    let (env, _admin, user, client) = setup();
    env.ledger().set_timestamp(100);
    let a = client.mint(&user, &user, &String::from_str(&env, "a"), &String::from_str(&env, "A"));
    let b = client.mint(&user, &user, &String::from_str(&env, "b"), &String::from_str(&env, "B"));

    let fused = client.fuse(&user, &a, &b);
    assert_eq!(fused, 3);
    // originals should be removed
    let orig_a = client.get_nft(&a);
    let orig_b = client.get_nft(&b);
    assert!(orig_a.is_none());
    assert!(orig_b.is_none());

    let nft = client.get_nft(&fused).unwrap();
    assert_eq!(nft.level, 2);
    let history = client.get_history(&fused).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history.get(0).unwrap(), &String::from_str(&env, "fused"));
}

#[test]
fn evolve_milestone_by_admin() {
    let (env, admin, user, client) = setup();
    env.ledger().set_timestamp(100);

    let token = client.mint(&user, &user, &String::from_str(&env, "meta"), &String::from_str(&env, "trait1"));
    let nft_before = client.get_nft(&token).unwrap();
    assert_eq!(nft_before.level, 1);
    assert_eq!(nft_before.rarity, 1);

    // admin evolves token
    client.evolve_milestone(&admin, &token, &2u32, &1u32, &None);
    let nft_after = client.get_nft(&token).unwrap();
    assert_eq!(nft_after.level, 3);
    assert_eq!(nft_after.rarity, 2);
    let history = client.get_history(&token).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history.get(0).unwrap(), &String::from_str(&env, "evolved_milestone"));
}

#[test]
fn downgrade_by_verifier() {
    let (env, admin, user, client) = setup();
    env.ledger().set_timestamp(100);

    let verifier = Address::generate(&env);
    client.add_verifier(&admin, &verifier);

    let token = client.mint(&user, &user, &String::from_str(&env, "meta"), &String::from_str(&env, "trait1"));
    // first evolve it
    client.evolve_milestone(&admin, &token, &3u32, &0u32, &None);
    let nft_evolved = client.get_nft(&token).unwrap();
    assert_eq!(nft_evolved.level, 4);

    // verifier downgrades
    client.downgrade(&verifier, &token, &2u32);
    let nft_downgraded = client.get_nft(&token).unwrap();
    assert_eq!(nft_downgraded.level, 2);
    let history = client.get_history(&token).unwrap();
    assert!(history.iter().any(|e| e == &String::from_str(&env, "downgraded")));
}

#[test]
#[should_panic(expected = "Unauthorized")]
fn evolve_milestone_requires_admin_or_verifier() {
    let (env, _admin, user, client) = setup();
    env.ledger().set_timestamp(100);

    let token = client.mint(&user, &user, &String::from_str(&env, "meta"), &String::from_str(&env, "trait1"));
    // random user cannot evolve
    let random = Address::generate(&env);
    client.evolve_milestone(&random, &token, &1u32, &0u32, &None);
}

#[test]
#[should_panic(expected = "Only owner can fuse tokens")]
fn fuse_requires_owner() {
    let (env, _admin, user, client) = setup();
    env.ledger().set_timestamp(100);

    let a = client.mint(&user, &user, &String::from_str(&env, "a"), &String::from_str(&env, "A"));
    let b = client.mint(&user, &user, &String::from_str(&env, "b"), &String::from_str(&env, "B"));

    let attacker = Address::generate(&env);
    // attacker cannot fuse user's tokens
    client.fuse(&attacker, &a, &b);
}

#[test]
fn remove_verifier() {
    let (env, admin, user, client) = setup();
    env.ledger().set_timestamp(100);

    let verifier = Address::generate(&env);
    client.add_verifier(&admin, &verifier);
    client.remove_verifier(&admin, &verifier);

    let token = client.mint(&user, &user, &String::from_str(&env, "meta"), &String::from_str(&env, "trait1"));
    // removed verifier cannot evolve
    let res = std::panic::catch_unwind(|| client.evolve_milestone(&verifier, &token, &1u32, &0u32, &None));
    assert!(res.is_err());
}
