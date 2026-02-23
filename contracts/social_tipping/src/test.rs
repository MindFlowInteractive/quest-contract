#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, String,
};

fn create_token_contract(e: &Env, admin: &Address) -> Address {
    let sac = e.register_stellar_asset_contract_v2(admin.clone());
    let address = sac.address();
    address.clone()
}

fn create_tipping_contract(e: &Env) -> Address {
    e.register_contract(None, SocialTippingContract)
}

fn setup_contract(e: &Env) -> (Address, Address, Address) {
    e.mock_all_auths();
    
    let admin = Address::generate(e);
    let token_admin_addr = Address::generate(e);
    let token_address = create_token_contract(e, &token_admin_addr);
    let tipping_contract = create_tipping_contract(e);
    
    let token_admin_client = StellarAssetClient::new(e, &token_address);

    let client = SocialTippingContractClient::new(e, &tipping_contract);
    client.initialize(
        &admin,
        &token_address,
        &1_000_000,  // max_tip_per_transaction
        &10,         // max_tips_per_day
        &60,         // cooldown_seconds
        &5,          // max_batch_size
    );

    // Mint tokens to users
    token_admin_client.mint(&admin, &100_000_000);

    (tipping_contract, token_address, admin)
}

#[test]
fn test_initialize() {
    let e = Env::default();
    e.mock_all_auths();
    
    let admin = Address::generate(&e);
    let _token_admin = Address::generate(&e);
    let token_address = create_token_contract(&e, &_token_admin);
    let tipping_contract = create_tipping_contract(&e);

    let client = SocialTippingContractClient::new(&e, &tipping_contract);
    
    client.initialize(
        &admin,
        &token_address,
        &1_000_000,
        &10,
        &60,
        &5,
    );
    
    let config = client.get_config();
    assert_eq!(config.admin, admin);
    assert_eq!(config.token, token_address);
    assert_eq!(config.max_tip_per_transaction, 1_000_000);
    assert_eq!(config.max_tips_per_day, 10);
    assert_eq!(config.cooldown_seconds, 60);
    assert_eq!(config.max_batch_size, 5);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_initialize_twice_should_fail() {
    let e = Env::default();
    e.mock_all_auths();
    
    let admin = Address::generate(&e);
    let _token_admin = Address::generate(&e);
    let token_address = create_token_contract(&e, &_token_admin);
    let tipping_contract = create_tipping_contract(&e);

    let client = SocialTippingContractClient::new(&e, &tipping_contract);
    client.initialize(&admin, &token_address, &1_000_000, &10, &60, &5);
    client.initialize(&admin, &token_address, &1_000_000, &10, &60, &5);
}

#[test]
fn test_direct_tip() {
    let e = Env::default();
    let (tipping_contract, token_address, _) = setup_contract(&e);
    
    let tipper = Address::generate(&e);
    let recipient = Address::generate(&e);
    
    // Mint tokens to tipper
    let _token_admin = Address::generate(&e);
    let token_admin_client = StellarAssetClient::new(&e, &token_address);
    token_admin_client.mint(&tipper, &10_000_000);

    let client = SocialTippingContractClient::new(&e, &tipping_contract);
    
    // Send a direct tip
    client.tip(&tipper, &recipient, &100_000);
    
    // Check tipper stats
    let tipper_stats = client.get_tipper_stats(&tipper);
    assert_eq!(tipper_stats.total_tipped, 100_000);
    assert_eq!(tipper_stats.tip_count, 1);
    
    // Check recipient stats
    let recipient_stats = client.get_recipient_stats(&recipient);
    assert_eq!(recipient_stats.total_received, 100_000);
    assert_eq!(recipient_stats.tip_count, 1);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_tip_with_invalid_amount() {
    let e = Env::default();
    let (tipping_contract, token_address, _) = setup_contract(&e);
    
    let tipper = Address::generate(&e);
    let recipient = Address::generate(&e);
    
    // Mint tokens to tipper
    let _token_admin = Address::generate(&e);
    let token_admin_client = StellarAssetClient::new(&e, &token_address);
    token_admin_client.mint(&tipper, &10_000_000);

    let client = SocialTippingContractClient::new(&e, &tipping_contract);
    
    // Try to send zero tip
    client.tip(&tipper, &recipient, &0);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_tip_to_self_should_fail() {
    let e = Env::default();
    let (tipping_contract, token_address, _) = setup_contract(&e);
    
    let tipper = Address::generate(&e);
    
    // Mint tokens to tipper
    let _token_admin = Address::generate(&e);
    let token_admin_client = StellarAssetClient::new(&e, &token_address);
    token_admin_client.mint(&tipper, &10_000_000);

    let client = SocialTippingContractClient::new(&e, &tipping_contract);
    
    // Try to tip self
    client.tip(&tipper, &tipper, &100_000);
}

#[test]
fn test_tip_with_message() {
    let e = Env::default();
    let (tipping_contract, token_address, _) = setup_contract(&e);
    
    let tipper = Address::generate(&e);
    let recipient = Address::generate(&e);
    
    // Mint tokens to tipper
    let _token_admin = Address::generate(&e);
    let token_admin_client = StellarAssetClient::new(&e, &token_address);
    token_admin_client.mint(&tipper, &10_000_000);

    let client = SocialTippingContractClient::new(&e, &tipping_contract);
    
    let message = String::from_str(&e, "Great puzzle!");
    
    // Send a tip with message
    client.tip_with_message(&tipper, &recipient, &100_000, &message);
    
    // Check tip history
    let history = client.get_tip_history(&recipient);
    assert_eq!(history.len(), 1);
    
    let tip = history.get(0).unwrap();
    assert_eq!(tip.from, tipper);
    assert_eq!(tip.to, recipient);
    assert_eq!(tip.amount, 100_000);
    assert_eq!(tip.message, message);
}

#[test]
fn test_batch_tipping() {
    let e = Env::default();
    let (tipping_contract, token_address, _) = setup_contract(&e);
    
    let tipper = Address::generate(&e);
    let recipient1 = Address::generate(&e);
    let recipient2 = Address::generate(&e);
    let recipient3 = Address::generate(&e);
    
    // Mint tokens to tipper
    let _token_admin = Address::generate(&e);
    let token_admin_client = StellarAssetClient::new(&e, &token_address);
    token_admin_client.mint(&tipper, &10_000_000);

    let client = SocialTippingContractClient::new(&e, &tipping_contract);
    
    // Create recipient list and amounts
    let mut recipients = soroban_sdk::Vec::new(&e);
    recipients.push_back(recipient1.clone());
    recipients.push_back(recipient2.clone());
    recipients.push_back(recipient3.clone());
    
    let mut amounts = soroban_sdk::Vec::new(&e);
    amounts.push_back(100_000);
    amounts.push_back(150_000);
    amounts.push_back(75_000);
    
    // Send batch tips
    client.batch_tip(&tipper, &recipients, &amounts);
    
    // Check tipper stats
    let tipper_stats = client.get_tipper_stats(&tipper);
    assert_eq!(tipper_stats.total_tipped, 325_000); // Sum of all tips
    assert_eq!(tipper_stats.tip_count, 3);
    
    // Check recipient1 stats
    let recipient1_stats = client.get_recipient_stats(&recipient1);
    assert_eq!(recipient1_stats.total_received, 100_000);
    assert_eq!(recipient1_stats.tip_count, 1);
    
    // Check recipient2 stats
    let recipient2_stats = client.get_recipient_stats(&recipient2);
    assert_eq!(recipient2_stats.total_received, 150_000);
    assert_eq!(recipient2_stats.tip_count, 1);
    
    // Check recipient3 stats
    let recipient3_stats = client.get_recipient_stats(&recipient3);
    assert_eq!(recipient3_stats.total_received, 75_000);
    assert_eq!(recipient3_stats.tip_count, 1);
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_batch_tipping_size_mismatch() {
    let e = Env::default();
    let (tipping_contract, token_address, _) = setup_contract(&e);
    
    let tipper = Address::generate(&e);
    let recipient1 = Address::generate(&e);
    
    // Mint tokens to tipper
    let _token_admin = Address::generate(&e);
    let token_admin_client = StellarAssetClient::new(&e, &token_address);
    token_admin_client.mint(&tipper, &10_000_000);

    let client = SocialTippingContractClient::new(&e, &tipping_contract);
    
    let mut recipients = soroban_sdk::Vec::new(&e);
    recipients.push_back(recipient1);
    
    let mut amounts = soroban_sdk::Vec::new(&e);
    amounts.push_back(100_000);
    amounts.push_back(150_000); // Mismatch!
    
    client.batch_tip(&tipper, &recipients, &amounts);
}

#[test]
fn test_tip_history_tracking() {
    let e = Env::default();
    let (tipping_contract, token_address, _) = setup_contract(&e);
    
    let tipper1 = Address::generate(&e);
    let tipper2 = Address::generate(&e);
    let recipient = Address::generate(&e);
    
    // Mint tokens to tippers
    let _token_admin = Address::generate(&e);
    let token_admin_client = StellarAssetClient::new(&e, &token_address);
    token_admin_client.mint(&tipper1, &10_000_000);
    token_admin_client.mint(&tipper2, &10_000_000);

    let client = SocialTippingContractClient::new(&e, &tipping_contract);
    
    // Tipper1 sends tip with message
    let msg1 = String::from_str(&e, "Great work!");
    client.tip_with_message(&tipper1, &recipient, &100_000, &msg1);
    
    // Tipper2 sends tip with message
    let msg2 = String::from_str(&e, "Amazing!");
    client.tip_with_message(&tipper2, &recipient, &200_000, &msg2);
    
    // Check tip history
    let history = client.get_tip_history(&recipient);
    assert_eq!(history.len(), 2);
    
    let tip1 = history.get(0).unwrap();
    assert_eq!(tip1.from, tipper1);
    assert_eq!(tip1.amount, 100_000);
    assert_eq!(tip1.message, msg1);
    
    let tip2 = history.get(1).unwrap();
    assert_eq!(tip2.from, tipper2);
    assert_eq!(tip2.amount, 200_000);
    assert_eq!(tip2.message, msg2);
}

#[test]
fn test_top_tippers_leaderboard() {
    let e = Env::default();
    let (tipping_contract, token_address, _) = setup_contract(&e);
    
    let tipper1 = Address::generate(&e);
    let tipper2 = Address::generate(&e);
    let tipper3 = Address::generate(&e);
    let recipient = Address::generate(&e);
    
    // Mint tokens to tippers
    let _token_admin = Address::generate(&e);
    let token_admin_client = StellarAssetClient::new(&e, &token_address);
    token_admin_client.mint(&tipper1, &10_000_000);
    token_admin_client.mint(&tipper2, &10_000_000);
    token_admin_client.mint(&tipper3, &10_000_000);

    let client = SocialTippingContractClient::new(&e, &tipping_contract);
    
    // Send tips from different tippers
    client.tip(&tipper1, &recipient, &500_000);
    client.tip(&tipper2, &recipient, &300_000);
    client.tip(&tipper3, &recipient, &700_000);
    
    // Get top tippers
    let top_tippers = client.get_top_tippers(&5);
    assert!(top_tippers.len() >= 3);
    
    // Verify ordering (highest first)
    let (top1_addr, top1_amount) = top_tippers.get(0).unwrap();
    assert_eq!(top1_addr, tipper3);
    assert_eq!(top1_amount, 700_000);
}

#[test]
fn test_top_recipients_leaderboard() {
    let e = Env::default();
    let (tipping_contract, token_address, _) = setup_contract(&e);
    
    let tipper = Address::generate(&e);
    let recipient1 = Address::generate(&e);
    let recipient2 = Address::generate(&e);
    let recipient3 = Address::generate(&e);
    
    // Mint tokens to tipper
    let _token_admin = Address::generate(&e);
    let token_admin_client = StellarAssetClient::new(&e, &token_address);
    token_admin_client.mint(&tipper, &10_000_000);

    let client = SocialTippingContractClient::new(&e, &tipping_contract);
    
    // Send tips to different recipients
    client.tip(&tipper, &recipient1, &200_000);
    client.tip(&tipper, &recipient2, &500_000);
    client.tip(&tipper, &recipient3, &100_000);
    
    // Get top recipients
    let top_recipients = client.get_top_recipients(&5);
    assert!(top_recipients.len() >= 3);
    
    // Verify ordering (highest first)
    let (top1_addr, top1_amount) = top_recipients.get(0).unwrap();
    assert_eq!(top1_addr, recipient2);
    assert_eq!(top1_amount, 500_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_daily_tip_limit() {
    let e = Env::default();
    let (tipping_contract, token_address, _) = setup_contract(&e);
    
    let tipper = Address::generate(&e);
    
    // Create 11 recipients
    let mut recipients = soroban_sdk::Vec::new(&e);
    for _ in 0..11 {
        recipients.push_back(Address::generate(&e));
    }
    
    // Mint tokens to tipper
    let _token_admin = Address::generate(&e);
    let token_admin_client = StellarAssetClient::new(&e, &token_address);
    token_admin_client.mint(&tipper, &100_000_000);

    let client = SocialTippingContractClient::new(&e, &tipping_contract);
    
    // Send 11 tips (limit is 10 per day)
    for i in 0..recipients.len() {
        let recipient = recipients.get(i).unwrap();
        client.tip(&tipper, &recipient, &100_000);
    }
}

#[test]
fn test_cooldown_mechanism() {
    let e = Env::default();
    
    // Set up with 60 second cooldown
    e.mock_all_auths();
    e.ledger().set_timestamp(1000); // Start with a non-zero timestamp
    
    let admin = Address::generate(&e);
    let _token_admin = Address::generate(&e);
    let token_address = create_token_contract(&e, &_token_admin);
    let tipping_contract = create_tipping_contract(&e);
    
    let token_admin_client = StellarAssetClient::new(&e, &token_address);
    token_admin_client.mint(&admin, &100_000_000);

    let client = SocialTippingContractClient::new(&e, &tipping_contract);
    client.initialize(
        &admin,
        &token_address,
        &1_000_000,
        &10,
        &60, // 60 second cooldown
        &5,
    );
    
    let tipper = Address::generate(&e);
    let recipient1 = Address::generate(&e);
    let recipient2 = Address::generate(&e);
    
    token_admin_client.mint(&tipper, &10_000_000);
    
    // First tip should succeed
    client.tip(&tipper, &recipient1, &100_000);
    
    // Verify tipper stats were updated with timestamp
    let tipper_stats = client.get_tipper_stats(&tipper);
    assert_eq!(tipper_stats.tip_count, 1);
    assert!(tipper_stats.last_tip_time == 1000);
    
    // Advance ledger time by 61 seconds to bypass cooldown
    e.ledger().set_timestamp(1061);
    
    // Now the second tip should succeed
    client.tip(&tipper, &recipient2, &100_000);
    
    // Verify both tips were recorded
    let tipper_stats = client.get_tipper_stats(&tipper);
    assert_eq!(tipper_stats.tip_count, 2);
    assert_eq!(tipper_stats.total_tipped, 200_000);
}

#[test]
fn test_tipper_stats_accumulation() {
    let e = Env::default();
    let (tipping_contract, token_address, _) = setup_contract(&e);
    
    let tipper = Address::generate(&e);
    let recipient = Address::generate(&e);
    
    // Mint tokens to tipper
    let _token_admin = Address::generate(&e);
    let token_admin_client = StellarAssetClient::new(&e, &token_address);
    token_admin_client.mint(&tipper, &10_000_000);

    let client = SocialTippingContractClient::new(&e, &tipping_contract);
    
    // Send multiple tips
    client.tip(&tipper, &recipient, &100_000);
    
    e.ledger().set_timestamp(61); // Advance time to bypass cooldown
    
    client.tip(&tipper, &recipient, &200_000);
    
    e.ledger().set_timestamp(122); // Advance time again
    
    client.tip(&tipper, &recipient, &300_000);
    
    // Check accumulated stats
    let tipper_stats = client.get_tipper_stats(&tipper);
    assert_eq!(tipper_stats.total_tipped, 600_000);
    assert_eq!(tipper_stats.tip_count, 3);
}

#[test]
fn test_recipient_stats_accumulation() {
    let e = Env::default();
    let (tipping_contract, token_address, _) = setup_contract(&e);
    
    let tipper1 = Address::generate(&e);
    let tipper2 = Address::generate(&e);
    let recipient = Address::generate(&e);
    
    // Mint tokens to tippers
    let _token_admin = Address::generate(&e);
    let token_admin_client = StellarAssetClient::new(&e, &token_address);
    token_admin_client.mint(&tipper1, &10_000_000);
    token_admin_client.mint(&tipper2, &10_000_000);

    let client = SocialTippingContractClient::new(&e, &tipping_contract);
    
    // Send tips from different sources
    client.tip(&tipper1, &recipient, &100_000);
    client.tip(&tipper2, &recipient, &200_000);
    
    e.ledger().set_timestamp(61); // Advance time
    
    client.tip(&tipper1, &recipient, &300_000);
    
    // Check accumulated stats
    let recipient_stats = client.get_recipient_stats(&recipient);
    assert_eq!(recipient_stats.total_received, 600_000);
    assert_eq!(recipient_stats.tip_count, 3);
}

#[test]
fn test_get_config() {
    let e = Env::default();
    e.mock_all_auths();
    
    let admin = Address::generate(&e);
    let _token_admin = Address::generate(&e);
    let token_address = create_token_contract(&e, &_token_admin);
    let tipping_contract = create_tipping_contract(&e);

    let client = SocialTippingContractClient::new(&e, &tipping_contract);
    
    client.initialize(
        &admin,
        &token_address,
        &2_000_000,
        &20,
        &120,
        &10,
    );
    
    let config = client.get_config();
    assert_eq!(config.admin, admin);
    assert_eq!(config.token, token_address);
    assert_eq!(config.max_tip_per_transaction, 2_000_000);
    assert_eq!(config.max_tips_per_day, 20);
    assert_eq!(config.cooldown_seconds, 120);
    assert_eq!(config.max_batch_size, 10);
    assert!(config.is_initialized);
}
