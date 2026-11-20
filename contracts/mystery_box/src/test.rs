#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Env};

#[test]
fn test_init_and_config() {
    let env = Env::default();
    let contract_id = env.register_contract(None, MysteryBoxContract);
    let client = MysteryBoxContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.init(&admin, &5);

    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_pity_threshold(), 5);

    // Double init must fail
    let res = client.try_init(&admin, &5);
    assert!(res.is_err());
}

#[test]
fn test_open_box_and_pity_mechanism() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, MysteryBoxContract);
    let client = MysteryBoxContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let player = Address::generate(&env);

    // Set pity threshold to 3 opens
    client.init(&admin, &3);

    // Configure weights with 0% rare so pity must trigger
    client.set_weights(&80, &20, &0);

    assert_eq!(client.get_pity_counter(&player), 0);
    assert_eq!(client.get_player_boxes_opened(&player), 0);

    // Open 1: pity counter -> 1
    let tier1 = client.open_box(&player);
    assert_ne!(tier1, RewardTier::Rare);
    assert_eq!(client.get_pity_counter(&player), 1);
    assert_eq!(client.get_player_boxes_opened(&player), 1);

    // Open 2: pity counter -> 2
    let tier2 = client.open_box(&player);
    assert_ne!(tier2, RewardTier::Rare);
    assert_eq!(client.get_pity_counter(&player), 2);
    assert_eq!(client.get_player_boxes_opened(&player), 2);

    // Open 3: pity threshold (3) is reached! Must guarantee Rare!
    let tier3 = client.open_box(&player);
    assert_eq!(tier3, RewardTier::Rare);
    assert_eq!(client.get_pity_counter(&player), 0); // Pity counter reset!
    assert_eq!(client.get_player_boxes_opened(&player), 3);
    assert_eq!(client.get_total_boxes_opened(), 3);
}

#[test]
fn test_natural_rare_resets_pity() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, MysteryBoxContract);
    let client = MysteryBoxContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let player = Address::generate(&env);

    client.init(&admin, &10);
    // Set 100% rare weight
    client.set_weights(&0, &0, &100);

    let tier = client.open_box(&player);
    assert_eq!(tier, RewardTier::Rare);
    assert_eq!(client.get_pity_counter(&player), 0);
}
