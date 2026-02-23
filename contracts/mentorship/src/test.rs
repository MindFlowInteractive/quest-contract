#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, String, Vec,
    token::{Client as TokenClient, StellarAssetClient},
};

struct TestSetup {
    env: Env,
    contract_id: Address,
    token_id: Address,
    admin: Address,
    mentor: Address,
    student: Address,
}

fn setup() -> TestSetup {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let mentor = Address::generate(&env);
    let student = Address::generate(&env);

    let token_id = env.register_stellar_asset_contract(admin.clone());
    let contract_id = env.register_contract(None, MentorshipContract);

    let client = MentorshipContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token_id);

    StellarAssetClient::new(&env, &token_id).mint(&student, &1_000_000);

    TestSetup { env, contract_id, token_id, admin, mentor, student }
}

fn make_milestones(env: &Env, count: u32, reward_each: i128) -> Vec<MilestoneInput> {
    let mut milestones = Vec::new(env);
    for i in 0..count {
        milestones.push_back(MilestoneInput {
            description: String::from_str(env, "Learn and complete task"),
            reward_amount: reward_each,
        });
        let _ = i;
    }
    milestones
}

fn register_both(client: &MentorshipContractClient, s: &TestSetup) {
    client.register_mentor(
        &s.mentor,
        &String::from_str(&s.env, "Blockchain Gaming"),
        &100i128,
    );
    client.register_student(&s.student);
}

#[test]
fn test_register_mentor() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);

    client.register_mentor(
        &s.mentor,
        &String::from_str(&s.env, "Stellar Development"),
        &200i128,
    );

    let profile = client.get_mentor_profile(&s.mentor);
    assert_eq!(profile.mentor, s.mentor);
    assert_eq!(profile.hourly_rate, 200);
    assert_eq!(profile.total_sessions, 0);
    assert_eq!(profile.rating_count, 0);
    assert!(profile.active);
}

#[test]
fn test_register_student() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);

    client.register_student(&s.student);

    let profile = client.get_student_profile(&s.student);
    assert_eq!(profile.student, s.student);
    assert_eq!(profile.total_sessions, 0);
    assert_eq!(profile.certificates_earned, 0);
    assert!(profile.active);
}

#[test]
fn test_duplicate_registration_rejected() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);

    client.register_mentor(&s.mentor, &String::from_str(&s.env, "Rust"), &50i128);

    let result = client.try_register_mentor(
        &s.mentor,
        &String::from_str(&s.env, "Rust"),
        &50i128,
    );
    assert_eq!(result, Err(Ok(Error::AlreadyRegistered)));
}

#[test]
fn test_create_session() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);
    register_both(&client, &s);

    let milestones = make_milestones(&s.env, 3, 500);
    let session_id = client.create_session(&s.student, &s.mentor, &milestones, &86400u64, &70u32);

    assert_eq!(session_id, 1);

    let session = client.get_session(&session_id);
    assert_eq!(session.mentor, s.mentor);
    assert_eq!(session.student, s.student);
    assert_eq!(session.milestone_count, 3);
    assert_eq!(session.reward_pool, 1_500);
    assert_eq!(session.mentor_share, 70);
    assert_eq!(session.status, SessionStatus::Pending);
    assert!(!session.pool_deposited);

    let mentor_profile = client.get_mentor_profile(&s.mentor);
    assert_eq!(mentor_profile.total_sessions, 1);

    let student_profile = client.get_student_profile(&s.student);
    assert_eq!(student_profile.total_sessions, 1);
}

#[test]
fn test_deposit_and_start_session() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);
    let token = TokenClient::new(&s.env, &s.token_id);
    register_both(&client, &s);

    let milestones = make_milestones(&s.env, 2, 1_000);
    let session_id = client.create_session(&s.student, &s.mentor, &milestones, &86400u64, &80u32);

    client.deposit_reward_pool(&session_id, &s.student);
    assert_eq!(token.balance(&s.contract_id), 2_000);

    let session = client.get_session(&session_id);
    assert!(session.pool_deposited);

    client.start_session(&session_id);
    let session = client.get_session(&session_id);
    assert_eq!(session.status, SessionStatus::Active);
}

#[test]
fn test_cannot_start_without_pool() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);
    register_both(&client, &s);

    let milestones = make_milestones(&s.env, 1, 500);
    let session_id = client.create_session(&s.student, &s.mentor, &milestones, &86400u64, &70u32);

    let result = client.try_start_session(&session_id);
    assert_eq!(result, Err(Ok(Error::PoolNotDeposited)));
}

#[test]
fn test_submit_and_verify_milestone() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);
    let token = TokenClient::new(&s.env, &s.token_id);
    register_both(&client, &s);

    let milestones = make_milestones(&s.env, 1, 1_000);
    let session_id = client.create_session(&s.student, &s.mentor, &milestones, &86400u64, &60u32);
    client.deposit_reward_pool(&session_id, &s.student);
    client.start_session(&session_id);

    client.submit_milestone(&session_id, &0u32);

    let milestone = client.get_milestone(&session_id, &0u32);
    assert_eq!(milestone.status, MilestoneStatus::Submitted);

    let mentor_before = token.balance(&s.mentor);
    let student_before = token.balance(&s.student);

    client.verify_milestone(&session_id, &0u32);

    assert_eq!(token.balance(&s.mentor), mentor_before + 600);
    assert_eq!(token.balance(&s.student), student_before + 400);

    let milestone = client.get_milestone(&session_id, &0u32);
    assert_eq!(milestone.status, MilestoneStatus::Verified);

    let session = client.get_session(&session_id);
    assert_eq!(session.milestones_verified, 1);
    assert_eq!(session.reward_distributed, 1_000);
}

#[test]
fn test_reject_and_resubmit_milestone() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);
    register_both(&client, &s);

    let milestones = make_milestones(&s.env, 1, 500);
    let session_id = client.create_session(&s.student, &s.mentor, &milestones, &86400u64, &70u32);
    client.deposit_reward_pool(&session_id, &s.student);
    client.start_session(&session_id);

    client.submit_milestone(&session_id, &0u32);
    client.reject_milestone(&session_id, &0u32);

    let milestone = client.get_milestone(&session_id, &0u32);
    assert_eq!(milestone.status, MilestoneStatus::Rejected);

    client.submit_milestone(&session_id, &0u32);
    client.verify_milestone(&session_id, &0u32);

    let milestone = client.get_milestone(&session_id, &0u32);
    assert_eq!(milestone.status, MilestoneStatus::Verified);
}

#[test]
fn test_cannot_verify_unsubmitted_milestone() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);
    register_both(&client, &s);

    let milestones = make_milestones(&s.env, 1, 500);
    let session_id = client.create_session(&s.student, &s.mentor, &milestones, &86400u64, &70u32);
    client.deposit_reward_pool(&session_id, &s.student);
    client.start_session(&session_id);

    let result = client.try_verify_milestone(&session_id, &0u32);
    assert_eq!(result, Err(Ok(Error::MilestoneNotSubmitted)));
}

#[test]
fn test_complete_session_and_certificate() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);
    register_both(&client, &s);

    let milestones = make_milestones(&s.env, 2, 500);
    let session_id = client.create_session(&s.student, &s.mentor, &milestones, &86400u64, &70u32);
    client.deposit_reward_pool(&session_id, &s.student);
    client.start_session(&session_id);

    for idx in 0..2u32 {
        client.submit_milestone(&session_id, &idx);
        client.verify_milestone(&session_id, &idx);
    }

    client.complete_session(&session_id);

    let session = client.get_session(&session_id);
    assert_eq!(session.status, SessionStatus::Completed);

    let cert = client.get_certificate(&session_id);
    assert_eq!(cert.session_id, session_id);
    assert_eq!(cert.student, s.student);
    assert_eq!(cert.mentor, s.mentor);
    assert_eq!(cert.milestones_completed, 2);

    let mentor_profile = client.get_mentor_profile(&s.mentor);
    assert_eq!(mentor_profile.completed_sessions, 1);

    let student_profile = client.get_student_profile(&s.student);
    assert_eq!(student_profile.completed_sessions, 1);
    assert_eq!(student_profile.certificates_earned, 1);
}

#[test]
fn test_cannot_complete_with_unfinished_milestones() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);
    register_both(&client, &s);

    let milestones = make_milestones(&s.env, 3, 300);
    let session_id = client.create_session(&s.student, &s.mentor, &milestones, &86400u64, &70u32);
    client.deposit_reward_pool(&session_id, &s.student);
    client.start_session(&session_id);

    client.submit_milestone(&session_id, &0u32);
    client.verify_milestone(&session_id, &0u32);

    let result = client.try_complete_session(&session_id);
    assert_eq!(result, Err(Ok(Error::MilestonesIncomplete)));
}

#[test]
fn test_rate_mentor_and_reputation() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);
    register_both(&client, &s);

    let milestones = make_milestones(&s.env, 1, 500);
    let session_id = client.create_session(&s.student, &s.mentor, &milestones, &86400u64, &70u32);
    client.deposit_reward_pool(&session_id, &s.student);
    client.start_session(&session_id);
    client.submit_milestone(&session_id, &0u32);
    client.verify_milestone(&session_id, &0u32);
    client.complete_session(&session_id);

    let review_id = client.rate_mentor(
        &session_id,
        &5u32,
        &String::from_str(&s.env, "Excellent mentor!"),
    );

    let review = client.get_review(&review_id);
    assert_eq!(review.rating, 5);
    assert_eq!(review.mentor, s.mentor);
    assert_eq!(review.student, s.student);

    let profile = client.get_mentor_profile(&s.mentor);
    assert_eq!(profile.rating_sum, 5);
    assert_eq!(profile.rating_count, 1);

    let avg = client.get_mentor_rating(&s.mentor);
    assert_eq!(avg, 500);
}

#[test]
fn test_invalid_rating_rejected() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);
    register_both(&client, &s);

    let milestones = make_milestones(&s.env, 1, 500);
    let session_id = client.create_session(&s.student, &s.mentor, &milestones, &86400u64, &70u32);
    client.deposit_reward_pool(&session_id, &s.student);
    client.start_session(&session_id);
    client.submit_milestone(&session_id, &0u32);
    client.verify_milestone(&session_id, &0u32);
    client.complete_session(&session_id);

    let result = client.try_rate_mentor(
        &session_id,
        &6u32,
        &String::from_str(&s.env, "Great"),
    );
    assert_eq!(result, Err(Ok(Error::InvalidRating)));

    let result = client.try_rate_mentor(
        &session_id,
        &0u32,
        &String::from_str(&s.env, "Bad"),
    );
    assert_eq!(result, Err(Ok(Error::InvalidRating)));
}

#[test]
fn test_cannot_rate_twice() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);
    register_both(&client, &s);

    let milestones = make_milestones(&s.env, 1, 500);
    let session_id = client.create_session(&s.student, &s.mentor, &milestones, &86400u64, &70u32);
    client.deposit_reward_pool(&session_id, &s.student);
    client.start_session(&session_id);
    client.submit_milestone(&session_id, &0u32);
    client.verify_milestone(&session_id, &0u32);
    client.complete_session(&session_id);

    client.rate_mentor(&session_id, &4u32, &String::from_str(&s.env, "Good"));

    let result = client.try_rate_mentor(
        &session_id,
        &5u32,
        &String::from_str(&s.env, "Changed mind"),
    );
    assert_eq!(result, Err(Ok(Error::AlreadyReviewed)));
}

#[test]
fn test_marketplace_listing_and_take() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);
    register_both(&client, &s);

    let listing_id = client.list_on_marketplace(
        &s.mentor,
        &String::from_str(&s.env, "NFT Gaming Strategy"),
        &86400u64,
        &2u32,
        &2_000i128,
        &75u32,
    );
    assert_eq!(listing_id, 1);

    let listing = client.get_listing(&listing_id);
    assert!(listing.active);
    assert_eq!(listing.mentor_share, 75);
    assert_eq!(listing.milestone_count, 2);

    let milestones = make_milestones(&s.env, 2, 1_000);
    let session_id = client.take_from_marketplace(&listing_id, &s.student, &milestones);

    let session = client.get_session(&session_id);
    assert_eq!(session.mentor, s.mentor);
    assert_eq!(session.student, s.student);
    assert_eq!(session.milestone_count, 2);
    assert_eq!(session.status, SessionStatus::Pending);

    let listing = client.get_listing(&listing_id);
    assert!(!listing.active);
}

#[test]
fn test_marketplace_wrong_milestone_count_rejected() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);
    register_both(&client, &s);

    let listing_id = client.list_on_marketplace(
        &s.mentor,
        &String::from_str(&s.env, "Expert"),
        &86400u64,
        &3u32,
        &3_000i128,
        &70u32,
    );

    let milestones = make_milestones(&s.env, 2, 1_000);
    let result = client.try_take_from_marketplace(&listing_id, &s.student, &milestones);
    assert_eq!(result, Err(Ok(Error::InvalidMilestoneCount)));
}

#[test]
fn test_full_mentorship_flow() {
    let s = setup();
    let client = MentorshipContractClient::new(&s.env, &s.contract_id);
    let token = TokenClient::new(&s.env, &s.token_id);
    register_both(&client, &s);

    let mut milestones = Vec::new(&s.env);
    milestones.push_back(MilestoneInput {
        description: String::from_str(&s.env, "Understand Soroban basics"),
        reward_amount: 400,
    });
    milestones.push_back(MilestoneInput {
        description: String::from_str(&s.env, "Deploy first contract"),
        reward_amount: 600,
    });

    let session_id = client.create_session(&s.student, &s.mentor, &milestones, &86400u64, &80u32);

    client.deposit_reward_pool(&session_id, &s.student);
    client.start_session(&session_id);

    let student_after_deposit = token.balance(&s.student);

    client.submit_milestone(&session_id, &0u32);
    client.verify_milestone(&session_id, &0u32);

    assert_eq!(token.balance(&s.mentor), 320);
    assert_eq!(token.balance(&s.student), student_after_deposit + 80);

    client.submit_milestone(&session_id, &1u32);
    client.verify_milestone(&session_id, &1u32);

    assert_eq!(token.balance(&s.mentor), 320 + 480);
    assert_eq!(token.balance(&s.student), student_after_deposit + 80 + 120);

    client.complete_session(&session_id);

    let cert = client.get_certificate(&session_id);
    assert_eq!(cert.milestones_completed, 2);
    assert_eq!(cert.student, s.student);

    let review_id = client.rate_mentor(
        &session_id,
        &5u32,
        &String::from_str(&s.env, "Best mentor ever!"),
    );

    let mentor_profile = client.get_mentor_profile(&s.mentor);
    assert_eq!(mentor_profile.completed_sessions, 1);
    assert_eq!(mentor_profile.rating_sum, 5);

    let student_profile = client.get_student_profile(&s.student);
    assert_eq!(student_profile.certificates_earned, 1);

    let avg_rating = client.get_mentor_rating(&s.mentor);
    assert_eq!(avg_rating, 500);

    let _ = review_id;
}
