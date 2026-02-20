#![cfg(test)]

use super::*;
use achievement_nft::AchievementNFTClient;
use reward_token::RewardTokenClient;
use soroban_sdk::{testutils::Address as _, symbol_short, Address, Env, String, Vec};

fn setup(
    env: &Env,
) -> (
    AchievementSetsClient,
    AchievementNFTClient,
    RewardTokenClient,
    Address,
    Address,
) {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let nft_id = env.register_contract(None, achievement_nft::AchievementNFT);
    let token_id = env.register_contract(None, reward_token::RewardToken);
    let sets_id = env.register_contract(None, AchievementSets);

    let nft = AchievementNFTClient::new(env, &nft_id);
    let token = RewardTokenClient::new(env, &token_id);
    let sets = AchievementSetsClient::new(env, &sets_id);

    nft.initialize(&admin);
    token.initialize(
        &admin,
        &String::from_str(env, "Reward"),
        &String::from_str(env, "RWD"),
        &6,
    );
    sets.initialize(&admin, &nft_id, &token_id, &10);

    // Authorize achievement_sets contract to mint reward tokens
    token.authorize_minter(&sets_id);

    (sets, nft, token, admin, nft_id)
}

#[test]
fn test_set_creation_and_claim() {
    let env = Env::default();
    let (sets, nft, token, admin, nft_id) = setup(&env);

    let user = Address::generate(&env);
    let name = String::from_str(&env, "Starter Set");
    let mut puzzles = Vec::new(&env);
    puzzles.push_back(1);
    puzzles.push_back(2);
    puzzles.push_back(3);

    let set_id = sets.create_set(
        &admin,
        &name,
        &puzzles,
        &SetTier::Common,
        &100,
        &None,
        &symbol_short!("starter"),
    );
    assert_eq!(set_id, 1);

    let progress = sets.progress(&user, &set_id);
    assert_eq!(progress.is_completed, false);
    assert_eq!(progress.completed_count, 0);
    assert_eq!(progress.required_count, 3);

    // Mint achievement NFTs via craftmint (no puzzle completion required in test)
    nft.craftmint(&user, &1, &String::from_str(&env, "A1"));
    nft.craftmint(&user, &2, &String::from_str(&env, "A2"));
    nft.craftmint(&user, &3, &String::from_str(&env, "A3"));

    let progress = sets.sync_player_set(&user, &set_id);
    assert_eq!(progress.is_completed, true);
    assert_eq!(progress.completed_count, 3);

    let bonus = sets.claim_set_bonus(&user, &set_id);
    assert_eq!(bonus, 100); // base 100 + tier 0
    assert_eq!(token.balance(&user), 100);

    let progress = sets.progress(&user, &set_id);
    assert_eq!(progress.is_claimed, true);

    let unlocks = sets.get_unlocks(&user);
    assert_eq!(unlocks.len(), 1);
    assert_eq!(unlocks.get(0).unwrap(), symbol_short!("starter"));
}

#[test]
#[should_panic(expected = "already claimed")]
fn test_double_claim_panics() {
    let env = Env::default();
    let (sets, nft, token, admin, _) = setup(&env);

    let user = Address::generate(&env);
    let name = String::from_str(&env, "Single Set");
    let mut puzzles = Vec::new(&env);
    puzzles.push_back(10);
    puzzles.push_back(20);

    let set_id = sets.create_set(
        &admin,
        &name,
        &puzzles,
        &SetTier::Common,
        &50,
        &None,
        &symbol_short!("single"),
    );

    nft.craftmint(&user, &10, &String::from_str(&env, "A"));
    nft.craftmint(&user, &20, &String::from_str(&env, "B"));

    sets.claim_set_bonus(&user, &set_id);
    sets.claim_set_bonus(&user, &set_id); // panic
}

#[test]
fn test_limited_edition_and_edition_tokens() {
    let env = Env::default();
    let (sets, nft, token, admin, _) = setup(&env);

    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let name = String::from_str(&env, "Limited Set");
    let mut puzzles = Vec::new(&env);
    puzzles.push_back(100);
    puzzles.push_back(200);

    let set_id = sets.create_set(
        &admin,
        &name,
        &puzzles,
        &SetTier::Rare,
        &200,
        &Some(2), // cap 2
        &symbol_short!("limited"),
    );

    // User A completes and claims
    nft.craftmint(&user_a, &100, &String::from_str(&env, "X"));
    nft.craftmint(&user_a, &200, &String::from_str(&env, "Y"));
    let bonus_a = sets.claim_set_bonus(&user_a, &set_id);
    assert_eq!(bonus_a, 250); // 200 base + 50 rare tier

    let tokens_a = sets.edition_tokens_of(&user_a);
    assert_eq!(tokens_a.len(), 1);
    let ed = sets.get_edition_token(&tokens_a.get(0).unwrap()).unwrap();
    assert_eq!(ed.set_id, set_id);
    assert_eq!(ed.serial, 1);
    assert_eq!(ed.owner, user_a);

    // User B completes and claims (2nd of 2)
    nft.craftmint(&user_b, &100, &String::from_str(&env, "X2"));
    nft.craftmint(&user_b, &200, &String::from_str(&env, "Y2"));
    sets.claim_set_bonus(&user_b, &set_id);

    // Third player cannot claim (cap exhausted) - tested in test_limited_edition_cap_exhausted
}

#[test]
fn test_edition_token_trading() {
    let env = Env::default();
    let (sets, nft, token, admin, _) = setup(&env);

    let seller = Address::generate(&env);
    let buyer = Address::generate(&env);

    let mut puzzles = Vec::new(&env);
    puzzles.push_back(1);
    puzzles.push_back(2);

    let set_id = sets.create_set(
        &admin,
        &String::from_str(&env, "Tradable"),
        &puzzles,
        &SetTier::Epic,
        &150,
        &Some(5),
        &symbol_short!("tradable"),
    );

    nft.craftmint(&seller, &1, &String::from_str(&env, "M"));
    nft.craftmint(&seller, &2, &String::from_str(&env, "N"));
    sets.claim_set_bonus(&seller, &set_id);

    let tokens = sets.edition_tokens_of(&seller);
    let token_id = tokens.get(0).unwrap();

    sets.transfer_edition_token(&seller, &buyer, &token_id);

    assert_eq!(sets.edition_tokens_of(&seller).len(), 0);
    assert_eq!(sets.edition_tokens_of(&buyer).len(), 1);
    let ed = sets.get_edition_token(&token_id).unwrap();
    assert_eq!(ed.owner, buyer);
}

#[test]
fn test_synergy_bonus() {
    let env = Env::default();
    let (sets, nft, token, admin, _) = setup(&env);

    let user = Address::generate(&env);

    let mut p1 = Vec::new(&env);
    p1.push_back(1);
    p1.push_back(2);
    let set1 = sets.create_set(
        &admin,
        &String::from_str(&env, "Set A"),
        &p1,
        &SetTier::Common,
        &50,
        &None,
        &symbol_short!("set_a"),
    );

    let mut p2 = Vec::new(&env);
    p2.push_back(3);
    p2.push_back(4);
    let set2 = sets.create_set(
        &admin,
        &String::from_str(&env, "Set B"),
        &p2,
        &SetTier::Common,
        &50,
        &None,
        &symbol_short!("set_b"),
    );

    let mut set_ids = Vec::new(&env);
    set_ids.push_back(set1);
    set_ids.push_back(set2);
    let syn_id = sets.create_synergy(
        &admin,
        &String::from_str(&env, "Synergy AB"),
        &set_ids,
        &75,
        &symbol_short!("syn_ab"),
    );

    nft.craftmint(&user, &1, &String::from_str(&env, "1"));
    nft.craftmint(&user, &2, &String::from_str(&env, "2"));
    nft.craftmint(&user, &3, &String::from_str(&env, "3"));
    nft.craftmint(&user, &4, &String::from_str(&env, "4"));

    sets.claim_set_bonus(&user, &set1);
    sets.claim_set_bonus(&user, &set2);

    let syn_bonus = sets.claim_synergy_bonus(&user, &syn_id);
    assert_eq!(syn_bonus, 75);
    assert_eq!(token.balance(&user), 50 + 50 + 75);
}

#[test]
#[should_panic(expected = "Limited edition exhausted")]
fn test_limited_edition_cap_exhausted() {
    let env = Env::default();
    let (sets, nft, _token, admin, _) = setup(&env);

    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let user_c = Address::generate(&env);

    let mut puzzles = Vec::new(&env);
    puzzles.push_back(100);
    puzzles.push_back(200);

    let set_id = sets.create_set(
        &admin,
        &String::from_str(&env, "Cap 2"),
        &puzzles,
        &SetTier::Common,
        &50,
        &Some(2),
        &symbol_short!("cap2"),
    );

    nft.craftmint(&user_a, &100, &String::from_str(&env, "a1"));
    nft.craftmint(&user_a, &200, &String::from_str(&env, "a2"));
    sets.claim_set_bonus(&user_a, &set_id);

    nft.craftmint(&user_b, &100, &String::from_str(&env, "b1"));
    nft.craftmint(&user_b, &200, &String::from_str(&env, "b2"));
    sets.claim_set_bonus(&user_b, &set_id);

    nft.craftmint(&user_c, &100, &String::from_str(&env, "c1"));
    nft.craftmint(&user_c, &200, &String::from_str(&env, "c2"));
    sets.claim_set_bonus(&user_c, &set_id); // panic: cap exhausted
}

#[test]
fn test_leaderboards() {
    let env = Env::default();
    let (sets, nft, token, admin, _) = setup(&env);

    let mut puzzles = Vec::new(&env);
    puzzles.push_back(1);
    puzzles.push_back(2);
    let set_id = sets.create_set(
        &admin,
        &String::from_str(&env, "LB Set"),
        &puzzles,
        &SetTier::Legendary,
        &100,
        &None,
        &symbol_short!("lb"),
    );

    let a = Address::generate(&env);
    let b = Address::generate(&env);

    nft.craftmint(&a, &1, &String::from_str(&env, "a1"));
    nft.craftmint(&a, &2, &String::from_str(&env, "a2"));
    sets.claim_set_bonus(&a, &set_id); // 100 + 300 = 400

    nft.craftmint(&b, &1, &String::from_str(&env, "b1"));
    nft.craftmint(&b, &2, &String::from_str(&env, "b2"));
    sets.claim_set_bonus(&b, &set_id); // 400

    let lb = sets.get_set_leaderboard(&set_id, &5);
    assert!(lb.len() >= 1);
    let global = sets.get_global_leaderboard(&5);
    assert!(global.len() >= 1);
}
