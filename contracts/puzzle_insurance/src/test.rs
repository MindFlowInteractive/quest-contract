#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::Client as TokenClient,
    token::StellarAssetClient,
    Address, Env, String,
};

fn create_token_contract<'a>(env: &Env, admin: &Address) -> (Address, TokenClient<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let address = sac.address();
    (address.clone(), TokenClient::new(env, &address))
}

fn setup_puzzle_verification_contract(env: &Env) -> Address {
    // Create a mock puzzle verification contract
    // In real tests, this would be the actual puzzle_verification contract
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, MockPuzzleVerification);
    let client = MockPuzzleVerificationClient::new(env, &contract_id);

    env.mock_all_auths();
    client.initialize(&admin);

    contract_id
}

fn setup_insurance_contract(
    env: &Env,
) -> (
    PuzzleInsuranceContractClient,
    Address,
    Address,
    Address,
    TokenClient,
    StellarAssetClient,
    Address,
) {
    let admin = Address::generate(env);
    let user = Address::generate(env);
    let token_admin = Address::generate(env);

    // Create payment token
    let (payment_token_addr, payment_token_client) = create_token_contract(env, &token_admin);
    let payment_admin_client = StellarAssetClient::new(env, &payment_token_addr);

    // Create puzzle verification contract
    let puzzle_verification = setup_puzzle_verification_contract(env);

    // Register insurance contract
    let contract_id = env.register_contract(None, PuzzleInsuranceContract);
    let client = PuzzleInsuranceContractClient::new(env, &contract_id);

    // Initialize with 1% base rate (100 basis points)
    let base_rate = 100u32;

    env.mock_all_auths();
    client.initialize(
        &admin,
        &payment_token_addr,
        &puzzle_verification,
        &base_rate,
    );

    (
        client,
        admin,
        user,
        token_admin,
        payment_token_client,
        payment_admin_client,
        puzzle_verification,
    )
}

// Mock puzzle verification contract for testing
#[contract]
pub struct MockPuzzleVerification;

#[contractimpl]
impl MockPuzzleVerification {
    pub fn initialize(env: Env, _admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &true);
    }

    pub fn is_completed(env: Env, _player: Address, _puzzle_id: u32) -> bool {
        // For testing, we'll track completions manually
        env.storage()
            .instance()
            .get(&DataKey::Completed(_player, _puzzle_id))
            .unwrap_or(false)
    }

    pub fn set_completed(env: Env, player: Address, puzzle_id: u32) {
        env.storage()
            .instance()
            .set(&DataKey::Completed(player, puzzle_id), &true);
    }
}

#[contracttype]
enum DataKey {
    Admin,
    Completed(Address, u32),
}

// ───────────── INITIALIZATION TESTS ─────────────

#[test]
fn test_initialization() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, _, _, _, _, puzzle_verification) = setup_insurance_contract(&env);

    let config = client.get_config();
    assert_eq!(config.admin, admin);
    assert_eq!(config.base_premium_rate, 100);
    assert_eq!(config.difficulty_multiplier, 150);
    assert!(!config.paused);

    assert_eq!(client.get_premium_pool(), 0);
    assert_eq!(client.get_total_policies(), 0);
    assert_eq!(client.get_total_claims(), 0);
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_double_initialization() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, _, _, payment_token_client, _, puzzle_verification) =
        setup_insurance_contract(&env);

    // Try to initialize again
    client.initialize(
        &admin,
        &payment_token_client.address,
        &puzzle_verification,
        &100u32,
    );
}

// ───────────── POLICY PURCHASE TESTS ─────────────

#[test]
fn test_purchase_policy() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, _, user, _, _, payment_admin_client, _) = setup_insurance_contract(&env);

    let puzzle_id = 1u32;
    let difficulty = 5u32;
    let coverage_amount = 1_000_000_000i128; // 1,000 tokens
    let coverage_period = 30 * 86_400u64; // 30 days
    let attempts_covered = 5u32;

    // Calculate expected premium
    let expected_premium = client.calculate_premium(
        &difficulty,
        &coverage_amount,
        &coverage_period,
        &attempts_covered,
    );

    // Mint tokens to user for premium payment
    payment_admin_client.mint(&user, &(expected_premium * 2));

    // Purchase policy
    client.purchase_policy(
        &user,
        &puzzle_id,
        &difficulty,
        &coverage_amount,
        &coverage_period,
        &attempts_covered,
    );

    // Verify policy was created
    let policy = client.get_policy(&user, &puzzle_id).unwrap();
    assert_eq!(policy.owner, user);
    assert_eq!(policy.puzzle_id, puzzle_id);
    assert_eq!(policy.difficulty, difficulty);
    assert_eq!(policy.coverage_amount, coverage_amount);
    assert_eq!(policy.premium_paid, expected_premium);
    assert_eq!(policy.status, PolicyStatus::Active);
    assert_eq!(policy.start_time, 1000);
    assert_eq!(policy.end_time, 1000 + coverage_period);
    assert_eq!(policy.attempts_covered, attempts_covered);
    assert_eq!(policy.attempts_used, 0);

    // Verify premium pool updated
    assert_eq!(client.get_premium_pool(), expected_premium);
    assert_eq!(client.get_total_policies(), 1);

    // Verify user policies list
    let user_policies = client.get_user_policies(&user);
    assert_eq!(user_policies.len(), 1);
    assert_eq!(user_policies.get(0).unwrap(), puzzle_id);
}

#[test]
fn test_premium_calculation_by_difficulty() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _, _, _, _, _, _) = setup_insurance_contract(&env);

    let coverage_amount = 1_000_000_000i128;
    let coverage_period = 365 * 86_400u64; // 1 year
    let attempts_covered = 5u32;

    // Lower difficulty should have lower premium
    let premium_difficulty_1 =
        client.calculate_premium(&1, &coverage_amount, &coverage_period, &attempts_covered);
    let premium_difficulty_5 =
        client.calculate_premium(&5, &coverage_amount, &coverage_period, &attempts_covered);
    let premium_difficulty_10 =
        client.calculate_premium(&10, &coverage_amount, &coverage_period, &attempts_covered);

    assert!(premium_difficulty_1 < premium_difficulty_5);
    assert!(premium_difficulty_5 < premium_difficulty_10);
    assert!(premium_difficulty_1 > 0);
}

#[test]
#[should_panic(expected = "Difficulty must be between 1 and 10")]
fn test_purchase_policy_invalid_difficulty() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _, user, _, _, _, _) = setup_insurance_contract(&env);

    client.purchase_policy(
        &user,
        &1u32,
        &11u32, // Invalid difficulty
        &1_000_000_000i128,
        &(30 * 86_400u64),
        &5u32,
    );
}

#[test]
#[should_panic(expected = "User already has an active policy for this puzzle")]
fn test_cannot_purchase_multiple_policies_same_puzzle() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, _, user, _, _, payment_admin_client, _) = setup_insurance_contract(&env);

    payment_admin_client.mint(&user, &10_000_000_000i128);

    // Purchase first policy
    client.purchase_policy(
        &user,
        &1u32,
        &5u32,
        &1_000_000_000i128,
        &(30 * 86_400u64),
        &5u32,
    );

    // Try to purchase second policy for same puzzle (should fail)
    client.purchase_policy(
        &user,
        &1u32,
        &5u32,
        &500_000_000i128,
        &(30 * 86_400u64),
        &3u32,
    );
}

// ───────────── CLAIM SUBMISSION TESTS ─────────────

#[test]
fn test_submit_claim_for_failed_attempt() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, _, user, _, _, payment_admin_client, _) = setup_insurance_contract(&env);

    payment_admin_client.mint(&user, &10_000_000_000i128);

    let puzzle_id = 1u32;
    let coverage_amount = 1_000_000_000i128;

    // Purchase policy
    client.purchase_policy(
        &user,
        &puzzle_id,
        &5u32,
        &coverage_amount,
        &(30 * 86_400u64),
        &5u32,
    );

    // Submit claim for failed attempt
    env.ledger().set_timestamp(1000 + 10 * 86_400);
    let claim_amount = 500_000_000i128;
    let description = String::from_str(&env, "Failed puzzle attempt");
    let attempt_timestamp = 1000 + 10 * 86_400;

    let claim_id = client.submit_claim(
        &user,
        &puzzle_id,
        &claim_amount,
        &description,
        &attempt_timestamp,
    );

    assert_eq!(claim_id, 1);

    // Verify claim
    let claim = client.get_claim(&claim_id).unwrap();
    assert_eq!(claim.claim_id, claim_id);
    assert_eq!(claim.policy_owner, user);
    assert_eq!(claim.puzzle_id, puzzle_id);
    assert_eq!(claim.claim_amount, claim_amount);
    assert_eq!(claim.status, ClaimStatus::Submitted);
    assert_eq!(claim.attempt_timestamp, attempt_timestamp);

    // Verify policy attempts_used updated
    let policy = client.get_policy(&user, &puzzle_id).unwrap();
    assert_eq!(policy.attempts_used, 1);
    assert_eq!(policy.attempts_covered, 5);

    // Verify user claims list
    let user_claims = client.get_user_claims(&user);
    assert_eq!(user_claims.len(), 1);
    assert_eq!(user_claims.get(0).unwrap(), claim_id);

    assert_eq!(client.get_total_claims(), 1);
}

#[test]
#[should_panic(expected = "No active policy found for this puzzle")]
fn test_submit_claim_without_policy() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _, user, _, _, _, _) = setup_insurance_contract(&env);

    client.submit_claim(
        &user,
        &1u32,
        &1_000_000_000i128,
        &String::from_str(&env, "Test"),
        &env.ledger().timestamp(),
    );
}

#[test]
#[should_panic(expected = "Outside coverage period")]
fn test_submit_claim_after_policy_expires() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, _, user, _, _, payment_admin_client, _) = setup_insurance_contract(&env);

    payment_admin_client.mint(&user, &10_000_000_000i128);

    let coverage_period = 30 * 86_400u64;

    client.purchase_policy(
        &user,
        &1u32,
        &5u32,
        &1_000_000_000i128,
        &coverage_period,
        &5u32,
    );

    // Try to submit claim after expiration
    env.ledger().set_timestamp(1000 + coverage_period + 1);

    client.submit_claim(
        &user,
        &1u32,
        &500_000_000i128,
        &String::from_str(&env, "Test"),
        &env.ledger().timestamp(),
    );
}

#[test]
#[should_panic(expected = "All covered attempts have been used")]
fn test_submit_claim_exceeds_attempts() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, _, user, _, _, payment_admin_client, _) = setup_insurance_contract(&env);

    payment_admin_client.mint(&user, &10_000_000_000i128);

    let puzzle_id = 1u32;
    let attempts_covered = 2u32;

    client.purchase_policy(
        &user,
        &puzzle_id,
        &5u32,
        &1_000_000_000i128,
        &(30 * 86_400u64),
        &attempts_covered,
    );

    env.ledger().set_timestamp(1000 + 10 * 86_400);
    let attempt_time = env.ledger().timestamp();

    // Submit first claim
    client.submit_claim(
        &user,
        &puzzle_id,
        &500_000_000i128,
        &String::from_str(&env, "First attempt"),
        &attempt_time,
    );

    // Submit second claim
    env.ledger().set_timestamp(1000 + 11 * 86_400);
    client.submit_claim(
        &user,
        &puzzle_id,
        &500_000_000i128,
        &String::from_str(&env, "Second attempt"),
        &(attempt_time + 86_400),
    );

    // Try to submit third claim (should fail - only 2 attempts covered)
    env.ledger().set_timestamp(1000 + 12 * 86_400);
    client.submit_claim(
        &user,
        &puzzle_id,
        &500_000_000i128,
        &String::from_str(&env, "Third attempt"),
        &(attempt_time + 2 * 86_400),
    );
}

#[test]
#[should_panic(expected = "Puzzle was successfully completed")]
fn test_submit_claim_for_completed_puzzle() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, _, user, _, _, payment_admin_client, puzzle_verification) =
        setup_insurance_contract(&env);

    payment_admin_client.mint(&user, &10_000_000_000i128);

    let puzzle_id = 1u32;

    client.purchase_policy(
        &user,
        &puzzle_id,
        &5u32,
        &1_000_000_000i128,
        &(30 * 86_400u64),
        &5u32,
    );

    // Mark puzzle as completed in verification contract
    let mock_client = MockPuzzleVerificationClient::new(&env, &puzzle_verification);
    mock_client.set_completed(&user, &puzzle_id);

    env.ledger().set_timestamp(1000 + 10 * 86_400);

    // Try to submit claim (should fail - puzzle was completed)
    client.submit_claim(
        &user,
        &puzzle_id,
        &500_000_000i128,
        &String::from_str(&env, "Test"),
        &env.ledger().timestamp(),
    );
}

// ───────────── CLAIM REVIEW AND PAYOUT TESTS ─────────────

#[test]
fn test_review_and_approve_claim() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin, user, _, _, payment_admin_client, _) = setup_insurance_contract(&env);

    payment_admin_client.mint(&user, &10_000_000_000i128);

    client.purchase_policy(
        &user,
        &1u32,
        &5u32,
        &1_000_000_000i128,
        &(30 * 86_400u64),
        &5u32,
    );

    env.ledger().set_timestamp(1000 + 10 * 86_400);

    let claim_id = client.submit_claim(
        &user,
        &1u32,
        &500_000_000i128,
        &String::from_str(&env, "Failed attempt"),
        &env.ledger().timestamp(),
    );

    // Admin reviews and approves
    let payout_amount = 450_000_000i128;
    client.review_claim(
        &admin,
        &claim_id,
        &true,
        &String::from_str(&env, "Approved after investigation"),
        &payout_amount,
    );

    let claim = client.get_claim(&claim_id).unwrap();
    assert_eq!(claim.status, ClaimStatus::Approved);
    assert_eq!(claim.payout_amount, payout_amount);
}

#[test]
fn test_process_payout() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin, user, _, payment_token_client, payment_admin_client, _) =
        setup_insurance_contract(&env);

    // Add funds to premium pool
    payment_admin_client.mint(&admin, &10_000_000_000i128);
    client.add_to_pool(&admin, &5_000_000_000i128);

    payment_admin_client.mint(&user, &10_000_000_000i128);

    client.purchase_policy(
        &user,
        &1u32,
        &5u32,
        &1_000_000_000i128,
        &(30 * 86_400u64),
        &5u32,
    );

    env.ledger().set_timestamp(1000 + 10 * 86_400);

    let claim_id = client.submit_claim(
        &user,
        &1u32,
        &500_000_000i128,
        &String::from_str(&env, "Failed attempt"),
        &env.ledger().timestamp(),
    );

    let payout_amount = 450_000_000i128;
    client.review_claim(
        &admin,
        &claim_id,
        &true,
        &String::from_str(&env, "Approved"),
        &payout_amount,
    );

    let pool_before = client.get_premium_pool();
    let balance_before = payment_token_client.balance(&user);

    // Process payout
    client.process_payout(&admin, &claim_id);

    // Verify payout
    let claim = client.get_claim(&claim_id).unwrap();
    assert_eq!(claim.status, ClaimStatus::Paid);
    assert!(claim.payout_time > 0);

    // Verify balances
    let pool_after = client.get_premium_pool();
    let balance_after = payment_token_client.balance(&user);

    assert_eq!(pool_after, pool_before - payout_amount);
    assert_eq!(balance_after, balance_before + payout_amount);
}

// ───────────── POLICY CANCELLATION TESTS ─────────────

#[test]
fn test_cancel_policy_with_refund() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, _, user, _, payment_token_client, payment_admin_client, _) =
        setup_insurance_contract(&env);

    let coverage_period = 30 * 86_400u64;
    let coverage_amount = 1_000_000_000i128;

    let premium = client.calculate_premium(&5u32, &coverage_amount, &coverage_period, &5u32);

    payment_admin_client.mint(&user, &premium);

    // Purchase policy
    client.purchase_policy(
        &user,
        &1u32,
        &5u32,
        &coverage_amount,
        &coverage_period,
        &5u32,
    );

    let initial_balance = payment_token_client.balance(&user);

    // Cancel after 10 days (1/3 of period used)
    env.ledger().set_timestamp(1000 + 10 * 86_400);
    client.cancel_policy(&user, &1u32);

    // Should receive refund
    let final_balance = payment_token_client.balance(&user);
    assert!(final_balance > initial_balance);

    // Verify policy status
    let policy = client.get_policy(&user, &1u32).unwrap();
    assert_eq!(policy.status, PolicyStatus::Cancelled);
}

// ───────────── FRAUD DETECTION TESTS ─────────────

#[test]
#[should_panic(expected = "Claim submitted too soon after previous claim")]
fn test_claim_cooldown_enforcement() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, _, user, _, _, payment_admin_client, _) = setup_insurance_contract(&env);

    payment_admin_client.mint(&user, &10_000_000_000i128);

    client.purchase_policy(
        &user,
        &1u32,
        &5u32,
        &1_000_000_000i128,
        &(90 * 86_400u64),
        &10u32,
    );

    // Submit first claim
    env.ledger().set_timestamp(1000 + 10 * 86_400);
    client.submit_claim(
        &user,
        &1u32,
        &100_000_000i128,
        &String::from_str(&env, "First claim"),
        &env.ledger().timestamp(),
    );

    // Try to submit second claim too soon (cooldown is 1 day)
    env.ledger().set_timestamp(1000 + 10 * 86_400 + 12 * 3600); // Only 12 hours later
    client.submit_claim(
        &user,
        &1u32,
        &100_000_000i128,
        &String::from_str(&env, "Second claim"),
        &env.ledger().timestamp(),
    );
}

#[test]
fn test_claim_after_cooldown() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, _, user, _, _, payment_admin_client, _) = setup_insurance_contract(&env);

    payment_admin_client.mint(&user, &10_000_000_000i128);

    client.purchase_policy(
        &user,
        &1u32,
        &5u32,
        &1_000_000_000i128,
        &(90 * 86_400u64),
        &10u32,
    );

    // Submit first claim
    env.ledger().set_timestamp(1000 + 10 * 86_400);
    let claim_id_1 = client.submit_claim(
        &user,
        &1u32,
        &100_000_000i128,
        &String::from_str(&env, "First claim"),
        &env.ledger().timestamp(),
    );

    // Submit second claim after cooldown (1+ days)
    env.ledger().set_timestamp(1000 + 10 * 86_400 + 25 * 3600); // 25 hours later
    let claim_id_2 = client.submit_claim(
        &user,
        &1u32,
        &100_000_000i128,
        &String::from_str(&env, "Second claim"),
        &env.ledger().timestamp(),
    );

    assert_eq!(claim_id_2, claim_id_1 + 1);
}

#[test]
#[should_panic(expected = "User is flagged for suspicious activity")]
fn test_flagged_user_cannot_claim() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin, user, _, _, payment_admin_client, _) = setup_insurance_contract(&env);

    payment_admin_client.mint(&user, &10_000_000_000i128);

    client.purchase_policy(
        &user,
        &1u32,
        &5u32,
        &1_000_000_000i128,
        &(30 * 86_400u64),
        &5u32,
    );

    // Admin flags user
    client.flag_user(&admin, &user, &String::from_str(&env, "Suspicious pattern"));

    // Try to submit claim
    env.ledger().set_timestamp(1000 + 10 * 86_400);
    client.submit_claim(
        &user,
        &1u32,
        &500_000_000i128,
        &String::from_str(&env, "Test"),
        &env.ledger().timestamp(),
    );
}

// ───────────── PREMIUM POOL TESTS ─────────────

#[test]
fn test_add_to_pool() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, _, _, _, payment_admin_client, _) = setup_insurance_contract(&env);

    payment_admin_client.mint(&admin, &10_000_000_000i128);

    let initial_pool = client.get_premium_pool();

    client.add_to_pool(&admin, &5_000_000_000i128);

    let final_pool = client.get_premium_pool();
    assert_eq!(final_pool, initial_pool + 5_000_000_000);
}

#[test]
fn test_withdraw_from_pool() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, _, _, payment_token_client, payment_admin_client, _) =
        setup_insurance_contract(&env);

    payment_admin_client.mint(&admin, &10_000_000_000i128);
    client.add_to_pool(&admin, &5_000_000_000i128);

    let pool_before = client.get_premium_pool();
    let balance_before = payment_token_client.balance(&admin);

    client.withdraw_from_pool(&admin, &2_000_000_000i128);

    let pool_after = client.get_premium_pool();
    let balance_after = payment_token_client.balance(&admin);

    assert_eq!(pool_after, pool_before - 2_000_000_000);
    assert_eq!(balance_after, balance_before + 2_000_000_000);
}

// ───────────── INTEGRATION TESTS ─────────────

#[test]
fn test_full_insurance_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, admin, user, _, payment_token_client, payment_admin_client, _) =
        setup_insurance_contract(&env);

    // 1. Admin adds funds to pool
    payment_admin_client.mint(&admin, &20_000_000_000i128);
    client.add_to_pool(&admin, &10_000_000_000i128);

    // 2. User purchases policy
    payment_admin_client.mint(&user, &10_000_000_000i128);
    let puzzle_id = 1u32;

    client.purchase_policy(
        &user,
        &puzzle_id,
        &7u32, // Difficulty 7
        &2_000_000_000i128,
        &(60 * 86_400u64),
        &5u32,
    );

    assert!(client.is_policy_active(&user, &puzzle_id));

    // 3. Time passes, user submits claim
    env.ledger().set_timestamp(1000 + 30 * 86_400);

    let claim_id = client.submit_claim(
        &user,
        &puzzle_id,
        &1_500_000_000i128,
        &String::from_str(&env, "Failed puzzle attempt"),
        &env.ledger().timestamp(),
    );

    // 4. Admin reviews and approves claim
    client.review_claim(
        &admin,
        &claim_id,
        &true,
        &String::from_str(&env, "Verified failure, approved payout"),
        &1_400_000_000i128,
    );

    // 5. Admin processes payout
    let balance_before = payment_token_client.balance(&user);
    client.process_payout(&admin, &claim_id);

    let balance_after = payment_token_client.balance(&user);
    assert_eq!(balance_after, balance_before + 1_400_000_000);

    // 6. Verify final state
    let claim = client.get_claim(&claim_id).unwrap();
    assert_eq!(claim.status, ClaimStatus::Paid);
    assert!(claim.payout_time > 0);

    let user_claims = client.get_user_claims(&user);
    assert_eq!(user_claims.len(), 1);

    let policy = client.get_policy(&user, &puzzle_id).unwrap();
    assert_eq!(policy.attempts_used, 1);
    assert_eq!(policy.attempts_covered, 5);
}

#[test]
fn test_multiple_policies_different_puzzles() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, _, user, _, _, payment_admin_client, _) = setup_insurance_contract(&env);

    payment_admin_client.mint(&user, &20_000_000_000i128);

    // Purchase policy for puzzle 1
    client.purchase_policy(
        &user,
        &1u32,
        &5u32,
        &1_000_000_000i128,
        &(30 * 86_400u64),
        &5u32,
    );

    // Purchase policy for puzzle 2
    client.purchase_policy(
        &user,
        &2u32,
        &8u32,
        &2_000_000_000i128,
        &(30 * 86_400u64),
        &3u32,
    );

    // Verify both policies exist
    let policy1 = client.get_policy(&user, &1u32).unwrap();
    let policy2 = client.get_policy(&user, &2u32).unwrap();

    assert_eq!(policy1.puzzle_id, 1);
    assert_eq!(policy2.puzzle_id, 2);
    assert_eq!(policy1.difficulty, 5);
    assert_eq!(policy2.difficulty, 8);

    // Verify user policies list
    let user_policies = client.get_user_policies(&user);
    assert_eq!(user_policies.len(), 2);
    assert!(user_policies.contains(&1u32));
    assert!(user_policies.contains(&2u32));

    assert_eq!(client.get_total_policies(), 2);
}
