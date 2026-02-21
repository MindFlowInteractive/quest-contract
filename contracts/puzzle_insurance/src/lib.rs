#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, IntoVal, String, Vec,
};

//
// ──────────────────────────────────────────────────────────
// DATA KEYS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
pub enum DataKey {
    Config,                       // InsuranceConfig
    Policy(Address, u32),         // InsurancePolicy for user and puzzle_id
    UserPolicies(Address),        // Vec<u32> of puzzle_ids user has policies for
    Claim(u64),                   // Claim by ID
    ClaimCounter,                 // u64 counter for generating claim IDs
    UserClaims(Address),          // Vec<u64> of user's claim IDs
    PremiumPool,                  // i128 total premium pool
    TotalPolicies,                // u64 counter
    TotalClaims,                  // u64 counter
    FraudFlags(Address),          // FraudMetrics per user
    PuzzleAttempts(Address, u32), // Attempt tracking for fraud detection
}

//
// ──────────────────────────────────────────────────────────
// ENUMS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyStatus {
    Active = 1,
    Expired = 2,
    Cancelled = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimStatus {
    Submitted = 1,
    UnderReview = 2,
    Approved = 3,
    Rejected = 4,
    Paid = 5,
}

//
// ──────────────────────────────────────────────────────────
// STRUCTS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
#[derive(Clone, Debug)]
pub struct InsuranceConfig {
    pub admin: Address,
    pub payment_token: Address,       // Token used for premiums/payouts
    pub puzzle_verification: Address, // Address of puzzle_verification contract
    pub base_premium_rate: u32,       // In basis points (100 = 1%)
    pub difficulty_multiplier: u32,   // Multiplier per difficulty level (in basis points)
    pub min_coverage_period: u64,     // Minimum coverage period in seconds
    pub max_coverage_period: u64,     // Maximum coverage period in seconds
    pub max_coverage_amount: i128,    // Maximum coverage amount per policy
    pub claim_review_period: u64,     // Time for admin to review claims
    pub max_claims_per_period: u32,   // Fraud detection: max claims per 30 days
    pub claim_cooldown: u64,          // Fraud detection: time between claims
    pub max_attempts_per_puzzle: u32, // Max attempts per puzzle before flagging
    pub paused: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct InsurancePolicy {
    pub owner: Address,
    pub puzzle_id: u32,
    pub difficulty: u32,       // Puzzle difficulty (1-10)
    pub coverage_amount: i128, // Coverage amount in payment token
    pub premium_paid: i128,    // Premium paid for this policy
    pub start_time: u64,
    pub end_time: u64,
    pub status: PolicyStatus,
    pub attempts_covered: u32, // Number of attempts covered
    pub attempts_used: u32,    // Number of attempts already used
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Claim {
    pub claim_id: u64,
    pub policy_owner: Address,
    pub puzzle_id: u32,
    pub claim_amount: i128,
    pub description: String, // Max 200 chars
    pub submission_time: u64,
    pub status: ClaimStatus,
    pub review_notes: String, // Review notes from admin
    pub payout_amount: i128,
    pub payout_time: u64,       // 0 if not paid yet
    pub attempt_timestamp: u64, // Timestamp of the failed attempt
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct FraudMetrics {
    pub total_claims: u32,
    pub recent_claims: Vec<u64>, // Claim IDs in last 30 days
    pub last_claim_time: u64,
    pub flagged: bool,
    pub flag_reason: String,
    pub puzzle_attempts: Vec<(u32, u64)>, // (puzzle_id, timestamp) pairs
}

//
// ──────────────────────────────────────────────────────────
// CONSTANTS
// ──────────────────────────────────────────────────────────
//

const SECONDS_PER_DAY: u64 = 86_400;
const BASIS_POINTS: u64 = 10_000;
const FRAUD_LOOKBACK_PERIOD: u64 = 30 * SECONDS_PER_DAY; // 30 days
const MAX_DIFFICULTY: u32 = 10;
const MIN_DIFFICULTY: u32 = 1;

//
// ──────────────────────────────────────────────────────────
// CONTRACT
// ──────────────────────────────────────────────────────────
//

#[contract]
pub struct PuzzleInsuranceContract;

#[contractimpl]
impl PuzzleInsuranceContract {
    // ───────────── INITIALIZATION ─────────────

    /// Initialize the puzzle insurance contract
    ///
    /// # Arguments
    /// * `admin` - Contract administrator
    /// * `payment_token` - Token address for premiums and payouts
    /// * `puzzle_verification` - Address of puzzle_verification contract
    /// * `base_premium_rate` - Base premium rate in basis points (e.g., 100 = 1%)
    pub fn initialize(
        env: Env,
        admin: Address,
        payment_token: Address,
        puzzle_verification: Address,
        base_premium_rate: u32,
    ) {
        admin.require_auth();

        if env.storage().persistent().has(&DataKey::Config) {
            panic!("Already initialized");
        }

        let config = InsuranceConfig {
            admin,
            payment_token,
            puzzle_verification,
            base_premium_rate,
            difficulty_multiplier: 150, // 1.5x per difficulty level
            min_coverage_period: 1 * SECONDS_PER_DAY, // 1 day minimum
            max_coverage_period: 365 * SECONDS_PER_DAY, // 1 year maximum
            max_coverage_amount: 1_000_000_000_000, // 1M tokens max
            claim_review_period: 7 * SECONDS_PER_DAY, // 7 days review time
            max_claims_per_period: 5,   // Max 5 claims per 30 days
            claim_cooldown: 1 * SECONDS_PER_DAY, // 1 day between claims
            max_attempts_per_puzzle: 10, // Max 10 attempts per puzzle before flagging
            paused: false,
        };

        env.storage().persistent().set(&DataKey::Config, &config);
        env.storage()
            .persistent()
            .set(&DataKey::PremiumPool, &0i128);
        env.storage()
            .persistent()
            .set(&DataKey::ClaimCounter, &0u64);
        env.storage()
            .persistent()
            .set(&DataKey::TotalPolicies, &0u64);
        env.storage().persistent().set(&DataKey::TotalClaims, &0u64);
    }

    // ───────────── POLICY MANAGEMENT ─────────────

    /// Purchase an insurance policy for a specific puzzle
    ///
    /// # Arguments
    /// * `owner` - Policy owner
    /// * `puzzle_id` - ID of the puzzle to insure
    /// * `difficulty` - Difficulty level of the puzzle (1-10)
    /// * `coverage_amount` - Amount of coverage
    /// * `coverage_period` - Coverage period in seconds
    /// * `attempts_covered` - Number of attempts covered by this policy
    pub fn purchase_policy(
        env: Env,
        owner: Address,
        puzzle_id: u32,
        difficulty: u32,
        coverage_amount: i128,
        coverage_period: u64,
        attempts_covered: u32,
    ) {
        owner.require_auth();
        Self::assert_not_paused(&env);

        let config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();

        // Validations
        if difficulty < MIN_DIFFICULTY || difficulty > MAX_DIFFICULTY {
            panic!("Difficulty must be between 1 and 10");
        }

        if coverage_amount <= 0 || coverage_amount > config.max_coverage_amount {
            panic!("Invalid coverage amount");
        }

        if coverage_period < config.min_coverage_period
            || coverage_period > config.max_coverage_period
        {
            panic!("Invalid coverage period");
        }

        if attempts_covered == 0 || attempts_covered > 100 {
            panic!("Invalid attempts_covered (must be 1-100)");
        }

        // Check if user already has an active policy for this puzzle
        if let Some(existing_policy) = Self::get_policy(env.clone(), owner.clone(), puzzle_id) {
            if existing_policy.status == PolicyStatus::Active {
                panic!("User already has an active policy for this puzzle");
            }
        }

        // Calculate premium based on difficulty and coverage
        let premium = Self::calculate_premium_internal(
            &env,
            &config,
            difficulty,
            coverage_amount,
            coverage_period,
            attempts_covered,
        );

        // Transfer premium from user to contract
        let token_client = token::Client::new(&env, &config.payment_token);
        token_client.transfer(&owner, &env.current_contract_address(), &premium);

        // Create policy
        let start_time = env.ledger().timestamp();
        let end_time = start_time + coverage_period;

        let policy = InsurancePolicy {
            owner: owner.clone(),
            puzzle_id,
            difficulty,
            coverage_amount,
            premium_paid: premium,
            start_time,
            end_time,
            status: PolicyStatus::Active,
            attempts_covered,
            attempts_used: 0,
        };

        // Store policy
        env.storage()
            .persistent()
            .set(&DataKey::Policy(owner.clone(), puzzle_id), &policy);

        // Add to user's policy list
        Self::add_to_user_policies(&env, owner.clone(), puzzle_id);

        // Update premium pool
        let pool: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::PremiumPool)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::PremiumPool, &(pool + premium));

        // Increment total policies
        let total: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalPolicies)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::TotalPolicies, &(total + 1));
    }

    /// Cancel a policy and receive prorated refund
    ///
    /// # Arguments
    /// * `owner` - Policy owner
    /// * `puzzle_id` - ID of the puzzle
    pub fn cancel_policy(env: Env, owner: Address, puzzle_id: u32) {
        owner.require_auth();

        let mut policy: InsurancePolicy = env
            .storage()
            .persistent()
            .get(&DataKey::Policy(owner.clone(), puzzle_id))
            .expect("Policy not found");

        if policy.status != PolicyStatus::Active {
            panic!("Policy is not active");
        }

        let current_time = env.ledger().timestamp();
        let config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();

        // Calculate refund (prorated based on unused time and attempts)
        let total_period = policy.end_time - policy.start_time;
        let remaining_period = if policy.end_time > current_time {
            policy.end_time - current_time
        } else {
            0
        };

        // Refund based on unused time and unused attempts
        let time_refund = if remaining_period > 0 && total_period > 0 {
            (policy.premium_paid * remaining_period as i128) / total_period as i128
        } else {
            0
        };

        let attempts_refund = if policy.attempts_used < policy.attempts_covered {
            let unused_attempts = policy.attempts_covered - policy.attempts_used;
            (policy.premium_paid * unused_attempts as i128) / (policy.attempts_covered as i128 * 2)
        } else {
            0
        };

        let refund = time_refund + attempts_refund;

        // Update policy status
        policy.status = PolicyStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Policy(owner.clone(), puzzle_id), &policy);

        // Process refund if applicable
        if refund > 0 {
            let token_client = token::Client::new(&env, &config.payment_token);
            let contract_balance = token_client.balance(&env.current_contract_address());

            // Only refund what the contract actually has
            let actual_refund = if contract_balance < refund {
                contract_balance
            } else {
                refund
            };

            if actual_refund > 0 {
                token_client.transfer(&env.current_contract_address(), &owner, &actual_refund);

                // Update premium pool
                let pool: i128 = env
                    .storage()
                    .persistent()
                    .get(&DataKey::PremiumPool)
                    .unwrap_or(0);
                env.storage()
                    .persistent()
                    .set(&DataKey::PremiumPool, &(pool - actual_refund));
            }
        }
    }

    // ───────────── CLAIM MANAGEMENT ─────────────

    /// Submit an insurance claim for a failed puzzle attempt
    ///
    /// # Arguments
    /// * `claimant` - User submitting the claim
    /// * `puzzle_id` - ID of the puzzle that was attempted
    /// * `claim_amount` - Amount being claimed (refund amount)
    /// * `description` - Description of the failed attempt
    /// * `attempt_timestamp` - Timestamp when the attempt was made
    ///
    /// # Returns
    /// * Claim ID
    pub fn submit_claim(
        env: Env,
        claimant: Address,
        puzzle_id: u32,
        claim_amount: i128,
        description: String,
        attempt_timestamp: u64,
    ) -> u64 {
        claimant.require_auth();
        Self::assert_not_paused(&env);

        // Get policy
        let mut policy: InsurancePolicy = env
            .storage()
            .persistent()
            .get(&DataKey::Policy(claimant.clone(), puzzle_id))
            .expect("No active policy found for this puzzle");

        // Validations
        let current_time = env.ledger().timestamp();

        // Check policy is active
        if policy.status != PolicyStatus::Active {
            panic!("Policy is not active");
        }

        // Check within coverage period
        if current_time < policy.start_time || current_time > policy.end_time {
            panic!("Outside coverage period");
        }

        // Check attempt timestamp is reasonable (within last 24 hours)
        if attempt_timestamp > current_time
            || current_time - attempt_timestamp > 24 * SECONDS_PER_DAY
        {
            panic!("Invalid attempt timestamp");
        }

        // Check claim amount
        if claim_amount <= 0 || claim_amount > policy.coverage_amount {
            panic!("Invalid claim amount");
        }

        // Check attempts limit
        if policy.attempts_used >= policy.attempts_covered {
            panic!("All covered attempts have been used");
        }

        // Verify the attempt actually failed by checking puzzle_verification contract
        Self::verify_failed_attempt(&env, &claimant, puzzle_id, attempt_timestamp);

        // Fraud checks
        Self::check_fraud(&env, &claimant, puzzle_id);

        // Generate claim ID
        let claim_id: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::ClaimCounter)
            .unwrap_or(0);
        let new_claim_id = claim_id + 1;
        env.storage()
            .persistent()
            .set(&DataKey::ClaimCounter, &new_claim_id);

        // Create claim
        let claim = Claim {
            claim_id: new_claim_id,
            policy_owner: claimant.clone(),
            puzzle_id,
            claim_amount,
            description,
            submission_time: current_time,
            status: ClaimStatus::Submitted,
            review_notes: String::from_str(&env, ""),
            payout_amount: 0,
            payout_time: 0,
            attempt_timestamp,
        };

        // Store claim
        env.storage()
            .persistent()
            .set(&DataKey::Claim(new_claim_id), &claim);

        // Add to user's claims list
        Self::add_to_user_claims(&env, claimant.clone(), new_claim_id);

        // Update policy attempts_used
        policy.attempts_used += 1;
        env.storage()
            .persistent()
            .set(&DataKey::Policy(claimant.clone(), puzzle_id), &policy);

        // Update fraud metrics
        Self::update_fraud_metrics(&env, claimant, new_claim_id, puzzle_id, current_time);

        // Increment total claims
        let total: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalClaims)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::TotalClaims, &(total + 1));

        new_claim_id
    }

    /// Review a claim (admin only)
    ///
    /// # Arguments
    /// * `admin` - Admin address
    /// * `claim_id` - Claim ID to review
    /// * `approved` - Whether claim is approved
    /// * `review_notes` - Review notes
    /// * `payout_amount` - Approved payout amount (if approved)
    pub fn review_claim(
        env: Env,
        admin: Address,
        claim_id: u64,
        approved: bool,
        review_notes: String,
        payout_amount: i128,
    ) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut claim: Claim = env
            .storage()
            .persistent()
            .get(&DataKey::Claim(claim_id))
            .expect("Claim not found");

        if claim.status != ClaimStatus::Submitted && claim.status != ClaimStatus::UnderReview {
            panic!("Claim cannot be reviewed");
        }

        if approved {
            if payout_amount <= 0 || payout_amount > claim.claim_amount {
                panic!("Invalid payout amount");
            }
            claim.status = ClaimStatus::Approved;
            claim.payout_amount = payout_amount;
        } else {
            claim.status = ClaimStatus::Rejected;
            claim.payout_amount = 0;
        }

        claim.review_notes = review_notes;

        env.storage()
            .persistent()
            .set(&DataKey::Claim(claim_id), &claim);
    }

    /// Process payout for an approved claim (admin only)
    ///
    /// # Arguments
    /// * `admin` - Admin address
    /// * `claim_id` - Claim ID to process
    pub fn process_payout(env: Env, admin: Address, claim_id: u64) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut claim: Claim = env
            .storage()
            .persistent()
            .get(&DataKey::Claim(claim_id))
            .expect("Claim not found");

        if claim.status != ClaimStatus::Approved {
            panic!("Claim is not approved");
        }

        let config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let pool: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::PremiumPool)
            .unwrap_or(0);

        // Check pool has sufficient funds
        if pool < claim.payout_amount {
            panic!("Insufficient premium pool");
        }

        // Transfer payout to claimant
        let token_client = token::Client::new(&env, &config.payment_token);
        token_client.transfer(
            &env.current_contract_address(),
            &claim.policy_owner,
            &claim.payout_amount,
        );

        // Update claim
        claim.status = ClaimStatus::Paid;
        claim.payout_time = env.ledger().timestamp();
        env.storage()
            .persistent()
            .set(&DataKey::Claim(claim_id), &claim);

        // Update premium pool
        env.storage()
            .persistent()
            .set(&DataKey::PremiumPool, &(pool - claim.payout_amount));
    }

    // ───────────── PREMIUM POOL MANAGEMENT ─────────────

    /// Add funds to premium pool (admin only)
    ///
    /// # Arguments
    /// * `admin` - Admin address
    /// * `amount` - Amount to add
    pub fn add_to_pool(env: Env, admin: Address, amount: i128) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        if amount <= 0 {
            panic!("Amount must be positive");
        }

        let config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let token_client = token::Client::new(&env, &config.payment_token);

        token_client.transfer(&admin, &env.current_contract_address(), &amount);

        let pool: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::PremiumPool)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::PremiumPool, &(pool + amount));
    }

    /// Withdraw from premium pool (admin only)
    ///
    /// # Arguments
    /// * `admin` - Admin address
    /// * `amount` - Amount to withdraw
    pub fn withdraw_from_pool(env: Env, admin: Address, amount: i128) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        if amount <= 0 {
            panic!("Amount must be positive");
        }

        let pool: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::PremiumPool)
            .unwrap_or(0);

        if pool < amount {
            panic!("Insufficient pool balance");
        }

        let config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let token_client = token::Client::new(&env, &config.payment_token);

        token_client.transfer(&env.current_contract_address(), &admin, &amount);

        env.storage()
            .persistent()
            .set(&DataKey::PremiumPool, &(pool - amount));
    }

    // ───────────── FRAUD MANAGEMENT ─────────────

    /// Flag a user for suspicious activity (admin only)
    ///
    /// # Arguments
    /// * `admin` - Admin address
    /// * `user` - User to flag
    /// * `reason` - Reason for flagging
    pub fn flag_user(env: Env, admin: Address, user: Address, reason: String) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut metrics =
            Self::get_fraud_metrics(env.clone(), user.clone()).unwrap_or(FraudMetrics {
                total_claims: 0,
                recent_claims: Vec::new(&env),
                last_claim_time: 0,
                flagged: false,
                flag_reason: String::from_str(&env, ""),
                puzzle_attempts: Vec::new(&env),
            });

        metrics.flagged = true;
        metrics.flag_reason = reason;

        env.storage()
            .persistent()
            .set(&DataKey::FraudFlags(user), &metrics);
    }

    /// Unflag a user (admin only)
    ///
    /// # Arguments
    /// * `admin` - Admin address
    /// * `user` - User to unflag
    pub fn unflag_user(env: Env, admin: Address, user: Address) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        if let Some(mut metrics) = Self::get_fraud_metrics(env.clone(), user.clone()) {
            metrics.flagged = false;
            metrics.flag_reason = String::from_str(&env, "");
            env.storage()
                .persistent()
                .set(&DataKey::FraudFlags(user), &metrics);
        }
    }

    // ───────────── VIEW FUNCTIONS ─────────────

    /// Get policy information for a user and puzzle
    pub fn get_policy(env: Env, user: Address, puzzle_id: u32) -> Option<InsurancePolicy> {
        env.storage()
            .persistent()
            .get(&DataKey::Policy(user, puzzle_id))
    }

    /// Get all puzzle IDs a user has policies for
    pub fn get_user_policies(env: Env, user: Address) -> Vec<u32> {
        env.storage()
            .persistent()
            .get(&DataKey::UserPolicies(user))
            .unwrap_or(Vec::new(&env))
    }

    /// Get claim information
    pub fn get_claim(env: Env, claim_id: u64) -> Option<Claim> {
        env.storage().persistent().get(&DataKey::Claim(claim_id))
    }

    /// Get user's claim history
    pub fn get_user_claims(env: Env, user: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::UserClaims(user))
            .unwrap_or(Vec::new(&env))
    }

    /// Get total policies count
    pub fn get_total_policies(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::TotalPolicies)
            .unwrap_or(0)
    }

    /// Get total claims count
    pub fn get_total_claims(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::TotalClaims)
            .unwrap_or(0)
    }

    /// Check if policy is active
    pub fn is_policy_active(env: Env, user: Address, puzzle_id: u32) -> bool {
        if let Some(policy) = Self::get_policy(env.clone(), user, puzzle_id) {
            let current_time = env.ledger().timestamp();
            policy.status == PolicyStatus::Active
                && current_time >= policy.start_time
                && current_time <= policy.end_time
        } else {
            false
        }
    }

    /// Get premium pool balance
    pub fn get_premium_pool(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::PremiumPool)
            .unwrap_or(0)
    }

    /// Get configuration
    pub fn get_config(env: Env) -> InsuranceConfig {
        env.storage().persistent().get(&DataKey::Config).unwrap()
    }

    /// Get fraud metrics for a user
    pub fn get_fraud_metrics(env: Env, user: Address) -> Option<FraudMetrics> {
        env.storage().persistent().get(&DataKey::FraudFlags(user))
    }

    /// Calculate premium for given parameters
    pub fn calculate_premium(
        env: Env,
        difficulty: u32,
        coverage_amount: i128,
        coverage_period: u64,
        attempts_covered: u32,
    ) -> i128 {
        let config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        Self::calculate_premium_internal(
            &env,
            &config,
            difficulty,
            coverage_amount,
            coverage_period,
            attempts_covered,
        )
    }

    // ───────────── ADMIN FUNCTIONS ─────────────

    /// Update premium rates (admin only)
    pub fn update_premium_rates(env: Env, admin: Address, base_rate: u32, difficulty_mult: u32) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();

        config.base_premium_rate = base_rate;
        config.difficulty_multiplier = difficulty_mult;

        env.storage().persistent().set(&DataKey::Config, &config);
    }

    /// Update coverage limits (admin only)
    pub fn update_coverage_limits(
        env: Env,
        admin: Address,
        min_period: u64,
        max_period: u64,
        max_amount: i128,
    ) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();

        config.min_coverage_period = min_period;
        config.max_coverage_period = max_period;
        config.max_coverage_amount = max_amount;

        env.storage().persistent().set(&DataKey::Config, &config);
    }

    /// Update fraud detection parameters (admin only)
    pub fn update_fraud_params(
        env: Env,
        admin: Address,
        max_claims: u32,
        cooldown: u64,
        max_attempts: u32,
    ) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();

        config.max_claims_per_period = max_claims;
        config.claim_cooldown = cooldown;
        config.max_attempts_per_puzzle = max_attempts;

        env.storage().persistent().set(&DataKey::Config, &config);
    }

    /// Pause/unpause contract (admin only)
    pub fn set_paused(env: Env, admin: Address, paused: bool) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        config.paused = paused;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    /// Emergency withdrawal of entire pool (admin only)
    pub fn emergency_withdraw(env: Env, admin: Address) -> i128 {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let pool: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::PremiumPool)
            .unwrap_or(0);

        if pool > 0 {
            let config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
            let token_client = token::Client::new(&env, &config.payment_token);

            token_client.transfer(&env.current_contract_address(), &admin, &pool);

            env.storage()
                .persistent()
                .set(&DataKey::PremiumPool, &0i128);
        }

        pool
    }

    // ───────────── INTERNAL HELPERS ─────────────

    fn calculate_premium_internal(
        _env: &Env,
        config: &InsuranceConfig,
        difficulty: u32,
        coverage_amount: i128,
        coverage_period: u64,
        attempts_covered: u32,
    ) -> i128 {
        // Base premium calculation
        // Premium = coverage_amount * base_rate * difficulty_multiplier^difficulty * (period_days / 365) * attempts_factor / BASIS_POINTS^2

        let coverage_days = coverage_period / SECONDS_PER_DAY;

        // Difficulty multiplier: (difficulty_multiplier / 100) ^ difficulty
        // For difficulty 1: 1.5x, difficulty 2: 2.25x, difficulty 3: 3.375x, etc.
        // Calculate multiplier^difficulty / 100^difficulty
        // Use larger scale to avoid integer division precision loss
        let mut difficulty_factor_numerator = 1i128;
        let multiplier_base = config.difficulty_multiplier as i128;
        for _ in 0..difficulty {
            difficulty_factor_numerator = difficulty_factor_numerator * multiplier_base;
        }
        // Divide by 100^difficulty
        let mut difficulty_factor_denominator = 1i128;
        for _ in 0..difficulty {
            difficulty_factor_denominator = difficulty_factor_denominator * 100;
        }
        let difficulty_factor = difficulty_factor_numerator / difficulty_factor_denominator;

        // Attempts factor: more attempts = higher premium (linear scaling)
        let attempts_factor = attempts_covered as i128;

        // Annual rate calculation
        let annual_rate = (config.base_premium_rate as i128 * difficulty_factor) / 100;

        // Premium = coverage_amount * annual_rate * (coverage_days / 365) * attempts_factor / BASIS_POINTS
        let premium = (coverage_amount * annual_rate * coverage_days as i128 * attempts_factor)
            / (365 * BASIS_POINTS as i128);

        // Ensure minimum premium of 1
        if premium < 1 {
            1
        } else {
            premium
        }
    }

    fn verify_failed_attempt(env: &Env, player: &Address, puzzle_id: u32, attempt_timestamp: u64) {
        use soroban_sdk::Symbol;
        let config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();

        // Call puzzle_verification contract to check if puzzle was completed
        // If completed, the attempt was successful, so claim should be rejected
        let args = (player.clone(), puzzle_id).into_val(env);
        let is_completed: bool = env.invoke_contract(
            &config.puzzle_verification,
            &Symbol::new(env, "is_completed"),
            args,
        );

        // If puzzle is completed, the attempt was successful, not a failure
        if is_completed {
            panic!("Puzzle was successfully completed, cannot claim for failure");
        }

        // Additional verification: check that attempt_timestamp is recent and reasonable
        let current_time = env.ledger().timestamp();
        if attempt_timestamp > current_time {
            panic!("Attempt timestamp is in the future");
        }
    }

    fn check_fraud(env: &Env, user: &Address, puzzle_id: u32) {
        let config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let current_time = env.ledger().timestamp();

        // Get or create fraud metrics
        let metrics = env
            .storage()
            .persistent()
            .get::<DataKey, FraudMetrics>(&DataKey::FraudFlags(user.clone()))
            .unwrap_or(FraudMetrics {
                total_claims: 0,
                recent_claims: Vec::new(env),
                last_claim_time: 0,
                flagged: false,
                flag_reason: String::from_str(env, ""),
                puzzle_attempts: Vec::new(env),
            });

        // Check if user is flagged
        if metrics.flagged {
            panic!("User is flagged for suspicious activity");
        }

        // Check claim cooldown
        if metrics.last_claim_time > 0 {
            let time_since_last = current_time - metrics.last_claim_time;
            if time_since_last < config.claim_cooldown {
                panic!("Claim submitted too soon after previous claim");
            }
        }

        // Check recent claim frequency
        let lookback_time = if current_time > FRAUD_LOOKBACK_PERIOD {
            current_time - FRAUD_LOOKBACK_PERIOD
        } else {
            0
        };

        let mut recent_count = 0u32;
        for claim_id in metrics.recent_claims.iter() {
            if let Some(claim) = env
                .storage()
                .persistent()
                .get::<DataKey, Claim>(&DataKey::Claim(claim_id))
            {
                if claim.submission_time >= lookback_time {
                    recent_count += 1;
                }
            }
        }

        if recent_count >= config.max_claims_per_period {
            panic!("Too many claims in recent period");
        }

        // Check attempts per puzzle
        let mut puzzle_attempt_count = 0u32;
        for (pid, timestamp) in metrics.puzzle_attempts.iter() {
            if pid == puzzle_id && timestamp >= lookback_time {
                puzzle_attempt_count += 1;
            }
        }

        if puzzle_attempt_count >= config.max_attempts_per_puzzle {
            panic!("Too many attempts for this puzzle");
        }
    }

    fn update_fraud_metrics(
        env: &Env,
        user: Address,
        claim_id: u64,
        puzzle_id: u32,
        current_time: u64,
    ) {
        let mut metrics = env
            .storage()
            .persistent()
            .get::<DataKey, FraudMetrics>(&DataKey::FraudFlags(user.clone()))
            .unwrap_or(FraudMetrics {
                total_claims: 0,
                recent_claims: Vec::new(env),
                last_claim_time: 0,
                flagged: false,
                flag_reason: String::from_str(env, ""),
                puzzle_attempts: Vec::new(env),
            });

        metrics.total_claims += 1;
        metrics.last_claim_time = current_time;

        // Add to recent claims, removing old ones
        let lookback_time = if current_time > FRAUD_LOOKBACK_PERIOD {
            current_time - FRAUD_LOOKBACK_PERIOD
        } else {
            0
        };

        let mut new_recent: Vec<u64> = Vec::new(env);
        for id in metrics.recent_claims.iter() {
            if let Some(claim) = env
                .storage()
                .persistent()
                .get::<DataKey, Claim>(&DataKey::Claim(id))
            {
                if claim.submission_time >= lookback_time {
                    new_recent.push_back(id);
                }
            }
        }
        new_recent.push_back(claim_id);
        metrics.recent_claims = new_recent;

        // Add puzzle attempt
        let mut new_attempts: Vec<(u32, u64)> = Vec::new(env);
        for (pid, timestamp) in metrics.puzzle_attempts.iter() {
            if timestamp >= lookback_time {
                new_attempts.push_back((pid, timestamp));
            }
        }
        new_attempts.push_back((puzzle_id, current_time));
        metrics.puzzle_attempts = new_attempts;

        env.storage()
            .persistent()
            .set(&DataKey::FraudFlags(user), &metrics);
    }

    fn add_to_user_policies(env: &Env, user: Address, puzzle_id: u32) {
        let mut policies: Vec<u32> = env
            .storage()
            .persistent()
            .get(&DataKey::UserPolicies(user.clone()))
            .unwrap_or(Vec::new(env));

        if !policies.contains(&puzzle_id) {
            policies.push_back(puzzle_id);
            env.storage()
                .persistent()
                .set(&DataKey::UserPolicies(user), &policies);
        }
    }

    fn add_to_user_claims(env: &Env, user: Address, claim_id: u64) {
        let mut claims: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::UserClaims(user.clone()))
            .unwrap_or(Vec::new(env));

        claims.push_back(claim_id);
        env.storage()
            .persistent()
            .set(&DataKey::UserClaims(user), &claims);
    }

    fn assert_admin(env: &Env, user: &Address) {
        let config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        if config.admin != *user {
            panic!("Admin only");
        }
    }

    fn assert_not_paused(env: &Env) {
        let config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        if config.paused {
            panic!("Contract is paused");
        }
    }
}

mod test;
