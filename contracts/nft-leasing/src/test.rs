#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, String,
    token::{Client as TokenClient, StellarAssetClient},
};

struct TestSetup {
    env: Env,
    contract_id: Address,
    token_id: Address,
    admin: Address,
    lessor: Address,
    lessee: Address,
}

fn setup() -> TestSetup {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let lessor = Address::generate(&env);
    let lessee = Address::generate(&env);

    let token_id = env.register_stellar_asset_contract(admin.clone());
    let contract_id = env.register_contract(None, NFTLeasingContract);

    let client = NFTLeasingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token_id);

    let sac = StellarAssetClient::new(&env, &token_id);
    sac.mint(&lessee, &1_000_000);

    TestSetup { env, contract_id, token_id, admin, lessor, lessee }
}

fn mint_to_contract(setup: &TestSetup, amount: i128) {
    let sac = StellarAssetClient::new(&setup.env, &setup.token_id);
    sac.mint(&setup.contract_id, &amount);
}

#[test]
fn test_create_lease() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);

    let lease_id = client.create_lease(
        &s.lessor, &s.lessee, &1u64, &86400u64, &70u32, &0i128, &false,
    );
    assert_eq!(lease_id, 1);

    let lease = client.get_lease(&lease_id);
    assert_eq!(lease.lessor, s.lessor);
    assert_eq!(lease.lessee, s.lessee);
    assert_eq!(lease.nft_id, 1);
    assert_eq!(lease.lessor_share, 70);
    assert_eq!(lease.status, LeaseStatus::Pending);
    assert!(lease.collateral_deposited);
}

#[test]
fn test_activate_lease_no_collateral() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);

    let lease_id = client.create_lease(
        &s.lessor, &s.lessee, &1u64, &86400u64, &60u32, &0i128, &false,
    );
    client.activate_lease(&lease_id);

    let lease = client.get_lease(&lease_id);
    assert_eq!(lease.status, LeaseStatus::Active);
}

#[test]
fn test_collateral_deposit_and_activate() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);
    let token = TokenClient::new(&s.env, &s.token_id);

    let collateral = 10_000i128;
    let lease_id = client.create_lease(
        &s.lessor, &s.lessee, &2u64, &86400u64, &50u32, &collateral, &false,
    );

    let lease = client.get_lease(&lease_id);
    assert!(!lease.collateral_deposited);

    client.deposit_collateral(&lease_id);

    let lease = client.get_lease(&lease_id);
    assert!(lease.collateral_deposited);
    assert_eq!(token.balance(&s.contract_id), collateral);

    client.activate_lease(&lease_id);
    let lease = client.get_lease(&lease_id);
    assert_eq!(lease.status, LeaseStatus::Active);
}

#[test]
fn test_record_reward_and_distribute() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);
    let token = TokenClient::new(&s.env, &s.token_id);

    let lease_id = client.create_lease(
        &s.lessor, &s.lessee, &3u64, &86400u64, &70u32, &0i128, &false,
    );
    client.activate_lease(&lease_id);

    let reward = 1_000i128;
    mint_to_contract(&s, reward);

    client.record_reward(&lease_id, &reward);

    let lease = client.get_lease(&lease_id);
    assert_eq!(lease.total_revenue, reward);

    let lessor_before = token.balance(&s.lessor);
    let lessee_before = token.balance(&s.lessee);

    client.distribute_revenue(&lease_id);

    assert_eq!(token.balance(&s.lessor), lessor_before + 700);
    assert_eq!(token.balance(&s.lessee), lessee_before + 300);

    let lease = client.get_lease(&lease_id);
    assert_eq!(lease.lessor_earned, 700);
    assert_eq!(lease.lessee_earned, 300);
}

#[test]
fn test_multiple_reward_distributions() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);
    let token = TokenClient::new(&s.env, &s.token_id);

    let lease_id = client.create_lease(
        &s.lessor, &s.lessee, &4u64, &86400u64, &60u32, &0i128, &false,
    );
    client.activate_lease(&lease_id);

    mint_to_contract(&s, 2_000);
    client.record_reward(&lease_id, &1_000);
    client.distribute_revenue(&lease_id);

    client.record_reward(&lease_id, &1_000);
    client.distribute_revenue(&lease_id);

    let lease = client.get_lease(&lease_id);
    assert_eq!(lease.total_revenue, 2_000);
    assert_eq!(lease.lessor_earned, 1_200);
    assert_eq!(lease.lessee_earned, 800);
    assert_eq!(token.balance(&s.lessor), 1_200);
    assert_eq!(token.balance(&s.lessee), 999_800 + 800);
}

#[test]
fn test_lease_renewal() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);

    let lease_id = client.create_lease(
        &s.lessor, &s.lessee, &5u64, &1000u64, &50u32, &0i128, &true,
    );
    client.activate_lease(&lease_id);

    s.env.ledger().with_mut(|li| li.timestamp = 2000);

    client.renew_lease(&lease_id, &5000u64);

    let lease = client.get_lease(&lease_id);
    assert_eq!(lease.status, LeaseStatus::Active);
    assert_eq!(lease.duration, 5000);
    assert_eq!(lease.renewal_count, 1);
}

#[test]
fn test_lease_termination_returns_collateral() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);
    let token = TokenClient::new(&s.env, &s.token_id);

    let collateral = 5_000i128;
    let lease_id = client.create_lease(
        &s.lessor, &s.lessee, &6u64, &86400u64, &50u32, &collateral, &false,
    );

    client.deposit_collateral(&lease_id);
    client.activate_lease(&lease_id);

    let lessee_before = token.balance(&s.lessee);
    client.terminate_lease(&lease_id, &s.lessor);

    assert_eq!(token.balance(&s.lessee), lessee_before + collateral);

    let lease = client.get_lease(&lease_id);
    assert_eq!(lease.status, LeaseStatus::Terminated);
}

#[test]
fn test_terminate_distributes_remaining_revenue() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);
    let token = TokenClient::new(&s.env, &s.token_id);

    let lease_id = client.create_lease(
        &s.lessor, &s.lessee, &7u64, &86400u64, &80u32, &0i128, &false,
    );
    client.activate_lease(&lease_id);

    mint_to_contract(&s, 500);
    client.record_reward(&lease_id, &500);

    let lessor_before = token.balance(&s.lessor);
    let lessee_before = token.balance(&s.lessee);

    client.terminate_lease(&lease_id, &s.lessee);

    assert_eq!(token.balance(&s.lessor), lessor_before + 400);
    assert_eq!(token.balance(&s.lessee), lessee_before + 100);
}

#[test]
fn test_marketplace_listing_and_take() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);

    let listing_id = client.list_on_marketplace(
        &s.lessor, &8u64, &60u32, &86400u64, &0i128, &true,
    );
    assert_eq!(listing_id, 1);

    let listing = client.get_listing(&listing_id);
    assert!(listing.active);
    assert_eq!(listing.lessor_share, 60);

    let lease_id = client.take_marketplace_listing(&listing_id, &s.lessee);

    let lease = client.get_lease(&lease_id);
    assert_eq!(lease.lessor, s.lessor);
    assert_eq!(lease.lessee, s.lessee);
    assert_eq!(lease.nft_id, 8);
    assert_eq!(lease.status, LeaseStatus::Pending);
    assert!(lease.renewable);

    let listing = client.get_listing(&listing_id);
    assert!(!listing.active);
}

#[test]
fn test_marketplace_listing_with_collateral() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);

    let listing_id = client.list_on_marketplace(
        &s.lessor, &9u64, &70u32, &3600u64, &2_000i128, &false,
    );

    let lease_id = client.take_marketplace_listing(&listing_id, &s.lessee);

    let lease = client.get_lease(&lease_id);
    assert_eq!(lease.collateral, 2_000);
    assert!(!lease.collateral_deposited);

    client.deposit_collateral(&lease_id);
    client.activate_lease(&lease_id);

    let lease = client.get_lease(&lease_id);
    assert_eq!(lease.status, LeaseStatus::Active);
}

#[test]
fn test_open_and_resolve_dispute_favor_lessor() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);
    let token = TokenClient::new(&s.env, &s.token_id);

    let collateral = 3_000i128;
    let lease_id = client.create_lease(
        &s.lessor, &s.lessee, &10u64, &86400u64, &50u32, &collateral, &false,
    );
    client.deposit_collateral(&lease_id);
    client.activate_lease(&lease_id);

    let reason = String::from_str(&s.env, "Lessee violated terms");
    let dispute_id = client.open_dispute(&lease_id, &s.lessor, &reason);

    let lease = client.get_lease(&lease_id);
    assert_eq!(lease.status, LeaseStatus::Disputed);

    let dispute = client.get_dispute(&dispute_id);
    assert_eq!(dispute.status, DisputeStatus::Open);

    let lessor_before = token.balance(&s.lessor);
    client.resolve_dispute(&dispute_id, &true);

    assert_eq!(token.balance(&s.lessor), lessor_before + collateral);

    let dispute = client.get_dispute(&dispute_id);
    assert_eq!(dispute.status, DisputeStatus::ResolvedForLessor);

    let lease = client.get_lease(&lease_id);
    assert_eq!(lease.status, LeaseStatus::Terminated);
}

#[test]
fn test_open_and_resolve_dispute_favor_lessee() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);
    let token = TokenClient::new(&s.env, &s.token_id);

    let collateral = 2_000i128;
    let lease_id = client.create_lease(
        &s.lessor, &s.lessee, &11u64, &86400u64, &50u32, &collateral, &false,
    );
    client.deposit_collateral(&lease_id);
    client.activate_lease(&lease_id);

    let reason = String::from_str(&s.env, "Lessor withheld NFT access");
    let dispute_id = client.open_dispute(&lease_id, &s.lessee, &reason);

    let lessee_before = token.balance(&s.lessee);
    client.resolve_dispute(&dispute_id, &false);

    assert_eq!(token.balance(&s.lessee), lessee_before + collateral);

    let dispute = client.get_dispute(&dispute_id);
    assert_eq!(dispute.status, DisputeStatus::ResolvedForLessee);
}

#[test]
fn test_lease_expiry_on_reward_record() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);

    let lease_id = client.create_lease(
        &s.lessor, &s.lessee, &12u64, &1000u64, &50u32, &0i128, &false,
    );
    client.activate_lease(&lease_id);

    s.env.ledger().with_mut(|li| li.timestamp = 5000);

    let result = client.try_record_reward(&lease_id, &100i128);
    assert!(result.is_err());

    let lease = client.get_lease(&lease_id);
    assert_eq!(lease.status, LeaseStatus::Expired);
}

#[test]
fn test_invalid_split_rejected() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);

    let result = client.try_create_lease(
        &s.lessor, &s.lessee, &1u64, &86400u64, &101u32, &0i128, &false,
    );
    assert_eq!(result, Err(Ok(Error::InvalidSplit)));
}

#[test]
fn test_zero_duration_rejected() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);

    let result = client.try_create_lease(
        &s.lessor, &s.lessee, &1u64, &0u64, &50u32, &0i128, &false,
    );
    assert_eq!(result, Err(Ok(Error::InvalidTerms)));
}

#[test]
fn test_non_renewable_lease_cannot_renew() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);

    let lease_id = client.create_lease(
        &s.lessor, &s.lessee, &13u64, &86400u64, &50u32, &0i128, &false,
    );
    client.activate_lease(&lease_id);

    let result = client.try_renew_lease(&lease_id, &86400u64);
    assert_eq!(result, Err(Ok(Error::InvalidTerms)));
}

#[test]
fn test_activate_without_collateral_rejected() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);

    let lease_id = client.create_lease(
        &s.lessor, &s.lessee, &14u64, &86400u64, &50u32, &5_000i128, &false,
    );

    let result = client.try_activate_lease(&lease_id);
    assert_eq!(result, Err(Ok(Error::InsufficientCollateral)));
}

#[test]
fn test_full_lease_flow() {
    let s = setup();
    let client = NFTLeasingContractClient::new(&s.env, &s.contract_id);
    let token = TokenClient::new(&s.env, &s.token_id);

    let collateral = 1_000i128;
    let lease_id = client.create_lease(
        &s.lessor, &s.lessee, &99u64, &86400u64, &70u32, &collateral, &true,
    );

    client.deposit_collateral(&lease_id);
    client.activate_lease(&lease_id);

    mint_to_contract(&s, 3_000);
    client.record_reward(&lease_id, &1_000);
    client.record_reward(&lease_id, &2_000);
    client.distribute_revenue(&lease_id);

    assert_eq!(token.balance(&s.lessor), 2_100);
    assert_eq!(token.balance(&s.lessee), 998_000 + 900);

    s.env.ledger().with_mut(|li| li.timestamp = 100_000);
    client.renew_lease(&lease_id, &86400u64);

    let lease = client.get_lease(&lease_id);
    assert_eq!(lease.renewal_count, 1);
    assert_eq!(lease.status, LeaseStatus::Active);

    client.terminate_lease(&lease_id, &s.lessor);

    let final_lessee_balance = token.balance(&s.lessee);
    let lease = client.get_lease(&lease_id);
    assert_eq!(lease.status, LeaseStatus::Terminated);
    assert_eq!(final_lessee_balance, 998_000 + 900 + collateral);
}
