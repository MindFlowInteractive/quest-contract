#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, String, Vec};

//
// ──────────────────────────────────────────────────────────
// DATA KEYS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
pub enum DataKey {
    Config,                    // PoolConfig
    ContractPolicy(Address),    // ContractPolicy for covered contract
    PolicyList,                 // Vec<Address> of all covered contracts
    Claim(u64),                 // Claim by ID
    ClaimCounter,              // u64 counter for generating claim IDs
    ContractClaims(Address),   // Vec<u64> of contract's claim IDs
    PremiumPool,               // i128 total premium pool
    ReservePool,               // i128 reserve pool for emergencies
    TotalPolicies,             // u64 counter
    TotalClaims,               // u64 counter
    RiskScore(Address),        // RiskScore for contract
    PoolMetrics,               // PoolMetrics for overall pool health
}

//
// ──────────────────────────────────────────────────────────
// ENUMS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RiskLevel {
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyStatus {
    Active = 1,
    Expired = 2,
    Cancelled = 3,
    Suspended = 4,
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

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureType {
    LogicError = 1,
    Reentrancy = 2,
    AccessControl = 3,
    ArithmeticOverflow = 4,
    OracleManipulation = 5,
    Other = 6,
}

//
// ──────────────────────────────────────────────────────────
// STRUCTS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
#[derive(Clone, Debug)]
pub struct PoolConfig {
    pub admin: Address,
    pub payment_token: Address,        // Token used for premiums/payouts
    pub base_premium_rate: u32,        // In basis points (100 = 1%)
    pub min_coverage_period: u64,      // Minimum coverage period in seconds
    pub max_coverage_period: u64,      // Maximum coverage period in seconds
    pub max_coverage_amount: i128,     // Maximum coverage amount
    pub claim_review_period: u64,      // Time for admin to review claims
    pub reserve_ratio: u32,             // Percentage of pool to keep as reserve (basis points)
    pub max_payout_ratio: u32,         // Max payout as percentage of coverage (basis points)
    pub paused: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct ContractPolicy {
    pub contract_address: Address,
    pub coverage_amount: i128,
    pub premium_paid: i128,
    pub start_time: u64,
    pub end_time: u64,
    pub status: PolicyStatus,
    pub risk_level: RiskLevel,
    pub premium_rate: u32,             // Actual premium rate applied
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Claim {
    pub claim_id: u64,
    pub contract_address: Address,
    pub failure_type: FailureType,
    pub claim_amount: i128,
    pub description: String,
    pub evidence_hash: String,          // Hash of evidence provided
    pub submission_time: u64,
    pub status: ClaimStatus,
    pub review_notes: String,
    pub payout_amount: i128,
    pub payout_time: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct RiskScore {
    pub contract_address: Address,
    pub risk_level: RiskLevel,
    pub score: u32,                     // 0-100, higher = riskier
    pub total_claims: u32,
    pub approved_claims: u32,
    pub total_payout: i128,
    pub last_updated: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PoolMetrics {
    pub total_premiums: i128,
    pub total_payouts: i128,
    pub active_policies: u64,
    pub total_claims_submitted: u64,
    pub total_claims_approved: u64,
    pub pool_utilization: u32,         // In basis points
    pub reserve_ratio: u32,            // In basis points
}

//
// ──────────────────────────────────────────────────────────
// CONSTANTS
// ──────────────────────────────────────────────────────────
//

const SECONDS_PER_DAY: u64 = 86_400;
const BASIS_POINTS: u64 = 10_000;
const MAX_DESCRIPTION_LENGTH: u32 = 500;
const MAX_EVIDENCE_HASH_LENGTH: u32 = 100;

//
// ──────────────────────────────────────────────────────────
// CONTRACT
// ──────────────────────────────────────────────────────────
//

#[contract]
pub struct InsurancePoolContract;

#[contractimpl]
impl InsurancePoolContract {
    // ───────────── INITIALIZATION ─────────────

    /// Initialize the insurance pool contract
    ///
    /// # Arguments
    /// * `admin` - Contract administrator
    /// * `payment_token` - Token address for premiums and payouts
    /// * `base_premium_rate` - Base premium rate in basis points (e.g., 100 = 1%)
    pub fn initialize(
        env: Env,
        admin: Address,
        payment_token: Address,
        base_premium_rate: u32,
    ) {
        admin.require_auth();

        if env.storage().persistent().has(&DataKey::Config) {
            panic!("Already initialized");
        }

        let config = PoolConfig {
            admin: admin.clone(),
            payment_token,
            base_premium_rate,
            min_coverage_period: 30 * SECONDS_PER_DAY,    // 30 days minimum
            max_coverage_period: 365 * SECONDS_PER_DAY,  // 1 year maximum
            max_coverage_amount: 10_000_000_000_000,      // 10M tokens max
            claim_review_period: 14 * SECONDS_PER_DAY,   // 14 days review time
            reserve_ratio: 2000,                          // 20% reserve
            max_payout_ratio: 10000,                     // 100% of coverage
            paused: false,
        };

        env.storage().persistent().set(&DataKey::Config, &config);
        env.storage().persistent().set(&DataKey::PremiumPool, &0i128);
        env.storage().persistent().set(&DataKey::ReservePool, &0i128);
        env.storage().persistent().set(&DataKey::ClaimCounter, &0u64);
        env.storage().persistent().set(&DataKey::TotalPolicies, &0u64);
        env.storage().persistent().set(&DataKey::TotalClaims, &0u64);

        // Initialize pool metrics
        let metrics = PoolMetrics {
            total_premiums: 0,
            total_payouts: 0,
            active_policies: 0,
            total_claims_submitted: 0,
            total_claims_approved: 0,
            pool_utilization: 0,
            reserve_ratio: 2000,
        };
        env.storage().persistent().set(&DataKey::PoolMetrics, &metrics);
    }

    // ───────────── POLICY MANAGEMENT ─────────────

    /// Purchase insurance coverage for a smart contract
    ///
    /// # Arguments
    /// * `contract_address` - Address of the contract to insure
    /// * `coverage_amount` - Amount of coverage desired
    /// * `coverage_period` - Coverage period in seconds
    /// * `payer` - Address paying for the coverage
    pub fn purchase_coverage(
        env: Env,
        contract_address: Address,
        coverage_amount: i128,
        coverage_period: u64,
        payer: Address,
    ) {
        payer.require_auth();
        Self::assert_not_paused(&env);

        let config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();

        // Validations
        if coverage_amount <= 0 || coverage_amount > config.max_coverage_amount {
            panic!("Invalid coverage amount");
        }

        if coverage_period < config.min_coverage_period || coverage_period > config.max_coverage_period {
            panic!("Invalid coverage period");
        }

        // Check if contract already has an active policy
        if let Some(existing_policy) = Self::get_policy(env.clone(), contract_address.clone()) {
            if existing_policy.status == PolicyStatus::Active {
                panic!("Contract already has active coverage");
            }
        }

        // Assess risk for the contract
        let risk_level = Self::assess_contract_risk(&env, &contract_address);
        let premium_rate = Self::calculate_premium_rate(&config, risk_level);

        // Calculate premium
        let premium = Self::calculate_premium(
            &env,
            &config,
            premium_rate,
            coverage_amount,
            coverage_period,
        );

        // Transfer premium from payer to contract
        let token_client = token::Client::new(&env, &config.payment_token);
        token_client.transfer(&payer, &env.current_contract_address(), &premium);

        // Create policy
        let start_time = env.ledger().timestamp();
        let end_time = start_time + coverage_period;

        let policy = ContractPolicy {
            contract_address: contract_address.clone(),
            coverage_amount,
            premium_paid: premium,
            start_time,
            end_time,
            status: PolicyStatus::Active,
            risk_level,
            premium_rate,
        };

        // Store policy
        env.storage().persistent().set(&DataKey::ContractPolicy(contract_address.clone()), &policy);

        // Add to policy list
        Self::add_to_policy_list(&env, contract_address.clone());

        // Update premium pool
        let pool: i128 = env.storage().persistent().get(&DataKey::PremiumPool).unwrap_or(0);
        env.storage().persistent().set(&DataKey::PremiumPool, &(pool + premium));

        // Update reserve pool
        let reserve_amount = (premium * config.reserve_ratio as i128) / BASIS_POINTS as i128;
        let reserve: i128 = env.storage().persistent().get(&DataKey::ReservePool).unwrap_or(0);
        env.storage().persistent().set(&DataKey::ReservePool, &(reserve + reserve_amount));

        // Update pool metrics
        Self::update_metrics_on_purchase(&env, premium);

        // Increment total policies
        let total: u64 = env.storage().persistent().get(&DataKey::TotalPolicies).unwrap_or(0);
        env.storage().persistent().set(&DataKey::TotalPolicies, &(total + 1));

        // Initialize risk score if not exists
        if !env.storage().persistent().has(&DataKey::RiskScore(contract_address.clone())) {
            let risk_score = RiskScore {
                contract_address: contract_address.clone(),
                risk_level,
                score: Self::risk_level_to_score(risk_level),
                total_claims: 0,
                approved_claims: 0,
                total_payout: 0,
                last_updated: start_time,
            };
            env.storage().persistent().set(&DataKey::RiskScore(contract_address), &risk_score);
        }
    }

    /// Renew coverage for an existing contract
    ///
    /// # Arguments
    /// * `contract_address` - Address of the contract
    /// * `additional_period` - Additional coverage period in seconds
    /// * `payer` - Address paying for the renewal
    pub fn renew_coverage(
        env: Env,
        contract_address: Address,
        additional_period: u64,
        payer: Address,
    ) {
        payer.require_auth();
        Self::assert_not_paused(&env);

        let config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let mut policy: ContractPolicy = env.storage().persistent()
            .get(&DataKey::ContractPolicy(contract_address.clone()))
            .expect("Policy not found");

        // Validations
        if policy.status != PolicyStatus::Active && policy.status != PolicyStatus::Expired {
            panic!("Coverage cannot be renewed");
        }

        let current_time = env.ledger().timestamp();
        let new_end_time = if policy.end_time > current_time {
            policy.end_time + additional_period
        } else {
            current_time + additional_period
        };

        let total_period = new_end_time - policy.start_time;
        if total_period > config.max_coverage_period {
            panic!("Total coverage period exceeds maximum");
        }

        // Calculate additional premium
        let additional_premium = Self::calculate_premium(
            &env,
            &config,
            policy.premium_rate,
            policy.coverage_amount,
            additional_period,
        );

        // Transfer premium from payer to contract
        let token_client = token::Client::new(&env, &config.payment_token);
        token_client.transfer(&payer, &env.current_contract_address(), &additional_premium);

        // Update policy
        policy.end_time = new_end_time;
        policy.premium_paid += additional_premium;
        policy.status = PolicyStatus::Active;

        env.storage().persistent().set(&DataKey::ContractPolicy(contract_address), &policy);

        // Update premium pool
        let pool: i128 = env.storage().persistent().get(&DataKey::PremiumPool).unwrap_or(0);
        env.storage().persistent().set(&DataKey::PremiumPool, &(pool + additional_premium));

        // Update reserve pool
        let reserve_amount = (additional_premium * config.reserve_ratio as i128) / BASIS_POINTS as i128;
        let reserve: i128 = env.storage().persistent().get(&DataKey::ReservePool).unwrap_or(0);
        env.storage().persistent().set(&DataKey::ReservePool, &(reserve + reserve_amount));

        // Update pool metrics
        Self::update_metrics_on_purchase(&env, additional_premium);
    }

    /// Cancel coverage and receive prorated refund
    ///
    /// # Arguments
    /// * `contract_address` - Address of the contract
    /// * `payer` - Address that purchased the coverage
    pub fn cancel_coverage(env: Env, contract_address: Address, payer: Address) {
        payer.require_auth();

        let mut policy: ContractPolicy = env.storage().persistent()
            .get(&DataKey::ContractPolicy(contract_address.clone()))
            .expect("Policy not found");

        if policy.status != PolicyStatus::Active {
            panic!("Coverage is not active");
        }

        let current_time = env.ledger().timestamp();
        let config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();

        // Calculate refund (prorated based on unused time)
        let total_period = policy.end_time - policy.start_time;
        let remaining_period = if policy.end_time > current_time {
            policy.end_time - current_time
        } else {
            0
        };

        let refund = if remaining_period > 0 {
            (policy.premium_paid * remaining_period as i128) / total_period as i128
        } else {
            0
        };

        // Update policy status
        policy.status = PolicyStatus::Cancelled;
        env.storage().persistent().set(&DataKey::ContractPolicy(contract_address.clone()), &policy);

        // Process refund if applicable
        if refund > 0 {
            let token_client = token::Client::new(&env, &config.payment_token);
            token_client.transfer(&env.current_contract_address(), &payer, &refund);

            // Update premium pool
            let pool: i128 = env.storage().persistent().get(&DataKey::PremiumPool).unwrap_or(0);
            env.storage().persistent().set(&DataKey::PremiumPool, &(pool - refund));

            // Update reserve pool proportionally
            let reserve_refund = (refund * config.reserve_ratio as i128) / BASIS_POINTS as i128;
            let reserve: i128 = env.storage().persistent().get(&DataKey::ReservePool).unwrap_or(0);
            env.storage().persistent().set(&DataKey::ReservePool, &(reserve - reserve_refund));
        }

        // Update pool metrics
        Self::update_metrics_on_cancel(&env);
    }

    // ───────────── CLAIM MANAGEMENT ─────────────

    /// Submit a claim for contract failure
    ///
    /// # Arguments
    /// * `contract_address` - Address of the failed contract
    /// * `failure_type` - Type of failure
    /// * `claim_amount` - Amount being claimed
    /// * `description` - Description of the failure
    /// * `evidence_hash` - Hash of evidence provided
    /// * `submitter` - Address submitting the claim
    ///
    /// # Returns
    /// * Claim ID
    pub fn submit_claim(
        env: Env,
        contract_address: Address,
        failure_type: FailureType,
        claim_amount: i128,
        description: String,
        evidence_hash: String,
        submitter: Address,
    ) -> u64 {
        submitter.require_auth();
        Self::assert_not_paused(&env);

        // Get policy
        let policy: ContractPolicy = env.storage().persistent()
            .get(&DataKey::ContractPolicy(contract_address.clone()))
            .expect("No active coverage found");

        // Validations
        let current_time = env.ledger().timestamp();

        // Check coverage is active
        if policy.status != PolicyStatus::Active {
            panic!("Coverage is not active");
        }

        // Check within coverage period
        if current_time < policy.start_time || current_time > policy.end_time {
            panic!("Outside coverage period");
        }

        // Check claim amount
        if claim_amount <= 0 || claim_amount > policy.coverage_amount {
            panic!("Invalid claim amount");
        }

        // Validate description length
        if description.len() as u32 > MAX_DESCRIPTION_LENGTH {
            panic!("Description too long");
        }

        // Validate evidence hash length
        if evidence_hash.len() as u32 > MAX_EVIDENCE_HASH_LENGTH {
            panic!("Evidence hash too long");
        }

        // Verify claim (fraud prevention)
        Self::verify_claim(&env, &contract_address, &submitter);

        // Generate claim ID
        let claim_id: u64 = env.storage().persistent().get(&DataKey::ClaimCounter).unwrap_or(0);
        let new_claim_id = claim_id + 1;
        env.storage().persistent().set(&DataKey::ClaimCounter, &new_claim_id);

        // Create claim
        let claim = Claim {
            claim_id: new_claim_id,
            contract_address: contract_address.clone(),
            failure_type,
            claim_amount,
            description,
            evidence_hash,
            submission_time: current_time,
            status: ClaimStatus::Submitted,
            review_notes: String::from_str(&env, ""),
            payout_amount: 0,
            payout_time: 0,
        };

        // Store claim
        env.storage().persistent().set(&DataKey::Claim(new_claim_id), &claim);

        // Add to contract's claims list
        Self::add_to_contract_claims(&env, contract_address.clone(), new_claim_id);

        // Update risk score
        Self::update_risk_score_on_claim(&env, contract_address.clone());

        // Increment total claims
        let total: u64 = env.storage().persistent().get(&DataKey::TotalClaims).unwrap_or(0);
        env.storage().persistent().set(&DataKey::TotalClaims, &(total + 1));

        // Update pool metrics
        Self::update_metrics_on_claim(&env);

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

        let mut claim: Claim = env.storage().persistent()
            .get(&DataKey::Claim(claim_id))
            .expect("Claim not found");

        if claim.status != ClaimStatus::Submitted && claim.status != ClaimStatus::UnderReview {
            panic!("Claim cannot be reviewed");
        }

        let config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();

        if approved {
            if payout_amount <= 0 {
                panic!("Payout amount must be positive");
            }

            // Enforce max payout ratio
            let max_payout = (claim.claim_amount * config.max_payout_ratio as i128) / BASIS_POINTS as i128;
            if payout_amount > max_payout {
                panic!("Payout amount exceeds maximum allowed");
            }

            claim.status = ClaimStatus::Approved;
            claim.payout_amount = payout_amount;
        } else {
            claim.status = ClaimStatus::Rejected;
            claim.payout_amount = 0;
        }

        claim.review_notes = review_notes;

        env.storage().persistent().set(&DataKey::Claim(claim_id), &claim);

        // Update risk score if approved
        if approved {
            Self::update_risk_score_on_approval(&env, claim.contract_address.clone(), payout_amount);
        }

        // Update pool metrics
        Self::update_metrics_on_review(&env, approved);
    }

    /// Process payout for an approved claim (admin only)
    ///
    /// # Arguments
    /// * `admin` - Admin address
    /// * `claim_id` - Claim ID to process
    /// * `recipient` - Address to receive payout
    pub fn process_payout(env: Env, admin: Address, claim_id: u64, recipient: Address) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut claim: Claim = env.storage().persistent()
            .get(&DataKey::Claim(claim_id))
            .expect("Claim not found");

        if claim.status != ClaimStatus::Approved {
            panic!("Claim is not approved");
        }

        let config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let pool: i128 = env.storage().persistent().get(&DataKey::PremiumPool).unwrap_or(0);

        // Check pool has sufficient funds (excluding reserve)
        let available_pool = pool - env.storage().persistent().get(&DataKey::ReservePool).unwrap_or(0);
        if available_pool < claim.payout_amount {
            // Try to use reserve if needed
            if pool < claim.payout_amount {
                panic!("Insufficient pool balance");
            }
        }

        // Transfer payout to recipient
        let token_client = token::Client::new(&env, &config.payment_token);
        token_client.transfer(
            &env.current_contract_address(),
            &recipient,
            &claim.payout_amount,
        );

        // Update claim
        claim.status = ClaimStatus::Paid;
        claim.payout_time = env.ledger().timestamp();
        env.storage().persistent().set(&DataKey::Claim(claim_id), &claim);

        // Update premium pool
        env.storage().persistent().set(&DataKey::PremiumPool, &(pool - claim.payout_amount));

        // Update pool metrics
        Self::update_metrics_on_payout(&env, claim.payout_amount);
    }

    // ───────────── POOL MANAGEMENT ─────────────

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

        let config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let token_client = token::Client::new(&env, &config.payment_token);

        token_client.transfer(&admin, &env.current_contract_address(), &amount);

        let pool: i128 = env.storage().persistent().get(&DataKey::PremiumPool).unwrap_or(0);
        env.storage().persistent().set(&DataKey::PremiumPool, &(pool + amount));

        // Update reserve pool proportionally
        let reserve_amount = (amount * config.reserve_ratio as i128) / BASIS_POINTS as i128;
        let reserve: i128 = env.storage().persistent().get(&DataKey::ReservePool).unwrap_or(0);
        env.storage().persistent().set(&DataKey::ReservePool, &(reserve + reserve_amount));
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

        let pool: i128 = env.storage().persistent().get(&DataKey::PremiumPool).unwrap_or(0);
        let reserve: i128 = env.storage().persistent().get(&DataKey::ReservePool).unwrap_or(0);

        // Ensure reserve is maintained
        let min_reserve = (pool * 2000) / BASIS_POINTS as i128; // 20% minimum
        let available = pool - reserve.max(min_reserve);

        if available < amount {
            panic!("Insufficient available pool balance (reserve must be maintained)");
        }

        let config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let token_client = token::Client::new(&env, &config.payment_token);

        token_client.transfer(&env.current_contract_address(), &admin, &amount);

        env.storage().persistent().set(&DataKey::PremiumPool, &(pool - amount));

        // Adjust reserve proportionally
        let reserve_reduction = (amount * config.reserve_ratio as i128) / BASIS_POINTS as i128;
        env.storage().persistent().set(&DataKey::ReservePool, &(reserve - reserve_reduction));
    }

    /// Calculate and update reserve requirements
    ///
    /// # Arguments
    /// * `admin` - Admin address
    pub fn update_reserves(env: Env, admin: Address) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let pool: i128 = env.storage().persistent().get(&DataKey::PremiumPool).unwrap_or(0);

        // Calculate required reserve
        let required_reserve = (pool * config.reserve_ratio as i128) / BASIS_POINTS as i128;
        let current_reserve: i128 = env.storage().persistent().get(&DataKey::ReservePool).unwrap_or(0);

        if current_reserve < required_reserve {
            // Need to move funds from pool to reserve
            let shortage = required_reserve - current_reserve;
            let available_pool = pool - current_reserve;
            
            if available_pool >= shortage {
                env.storage().persistent().set(&DataKey::ReservePool, &required_reserve);
            }
        } else if current_reserve > required_reserve {
            // Can release some reserve back to pool
            let _excess = current_reserve - required_reserve;
            env.storage().persistent().set(&DataKey::ReservePool, &required_reserve);
        }

        // Update pool metrics
        Self::update_pool_utilization(&env);
    }

    // ───────────── RISK ASSESSMENT ─────────────

    /// Manually set risk level for a contract (admin only)
    ///
    /// # Arguments
    /// * `admin` - Admin address
    /// * `contract_address` - Contract address
    /// * `risk_level` - New risk level
    pub fn set_risk_level(env: Env, admin: Address, contract_address: Address, risk_level: RiskLevel) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut risk_score = Self::get_risk_score(env.clone(), contract_address.clone())
            .unwrap_or(RiskScore {
                contract_address: contract_address.clone(),
                risk_level,
                score: Self::risk_level_to_score(risk_level),
                total_claims: 0,
                approved_claims: 0,
                total_payout: 0,
                last_updated: env.ledger().timestamp(),
            });

        risk_score.risk_level = risk_level;
        risk_score.score = Self::risk_level_to_score(risk_level);
        risk_score.last_updated = env.ledger().timestamp();

        env.storage().persistent().set(&DataKey::RiskScore(contract_address), &risk_score);
    }

    /// Get current risk assessment for a contract
    ///
    /// # Arguments
    /// * `contract_address` - Contract address
    ///
    /// # Returns
    /// * RiskScore struct
    pub fn get_risk_assessment(env: Env, contract_address: Address) -> RiskScore {
        Self::get_risk_score(env, contract_address.clone())
            .unwrap_or(RiskScore {
                contract_address,
                risk_level: RiskLevel::Medium,
                score: 50,
                total_claims: 0,
                approved_claims: 0,
                total_payout: 0,
                last_updated: 0,
            })
    }

    // ───────────── VIEW FUNCTIONS ─────────────

    /// Get policy information
    pub fn get_policy(env: Env, contract_address: Address) -> Option<ContractPolicy> {
        env.storage().persistent().get(&DataKey::ContractPolicy(contract_address))
    }

    /// Get claim information
    pub fn get_claim(env: Env, claim_id: u64) -> Option<Claim> {
        env.storage().persistent().get(&DataKey::Claim(claim_id))
    }

    /// Get contract's claim history
    pub fn get_contract_claims(env: Env, contract_address: Address) -> Vec<u64> {
        env.storage().persistent()
            .get(&DataKey::ContractClaims(contract_address))
            .unwrap_or(Vec::new(&env))
    }

    /// Get all covered contracts
    pub fn get_all_policies(env: Env) -> Vec<Address> {
        env.storage().persistent()
            .get(&DataKey::PolicyList)
            .unwrap_or(Vec::new(&env))
    }

    /// Get total policies count
    pub fn get_total_policies(env: Env) -> u64 {
        env.storage().persistent().get(&DataKey::TotalPolicies).unwrap_or(0)
    }

    /// Get total claims count
    pub fn get_total_claims(env: Env) -> u64 {
        env.storage().persistent().get(&DataKey::TotalClaims).unwrap_or(0)
    }

    /// Check if coverage is active
    pub fn is_coverage_active(env: Env, contract_address: Address) -> bool {
        if let Some(policy) = Self::get_policy(env.clone(), contract_address) {
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
        env.storage().persistent().get(&DataKey::PremiumPool).unwrap_or(0)
    }

    /// Get reserve pool balance
    pub fn get_reserve_pool(env: Env) -> i128 {
        env.storage().persistent().get(&DataKey::ReservePool).unwrap_or(0)
    }

    /// Get configuration
    pub fn get_config(env: Env) -> PoolConfig {
        env.storage().persistent().get(&DataKey::Config).unwrap()
    }

    /// Get pool metrics
    pub fn get_pool_metrics(env: Env) -> PoolMetrics {
        env.storage().persistent().get(&DataKey::PoolMetrics).unwrap()
    }

    /// Calculate premium for given parameters
    pub fn calculate_premium_quote(
        env: Env,
        coverage_amount: i128,
        coverage_period: u64,
        risk_level: RiskLevel,
    ) -> i128 {
        let config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let premium_rate = Self::calculate_premium_rate(&config, risk_level);
        Self::calculate_premium(&env, &config, premium_rate, coverage_amount, coverage_period)
    }

    // ───────────── ADMIN FUNCTIONS ─────────────

    /// Update premium rates (admin only)
    pub fn update_premium_rate(env: Env, admin: Address, base_rate: u32) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        config.base_premium_rate = base_rate;
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

        let mut config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        config.min_coverage_period = min_period;
        config.max_coverage_period = max_period;
        config.max_coverage_amount = max_amount;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    /// Update reserve ratio (admin only)
    pub fn update_reserve_ratio(env: Env, admin: Address, reserve_ratio: u32) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        if reserve_ratio > 10000 {
            panic!("Reserve ratio cannot exceed 100%");
        }

        let mut config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        config.reserve_ratio = reserve_ratio;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    /// Update max payout ratio (admin only)
    pub fn update_max_payout_ratio(env: Env, admin: Address, max_payout_ratio: u32) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        if max_payout_ratio > 10000 {
            panic!("Max payout ratio cannot exceed 100%");
        }

        let mut config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        config.max_payout_ratio = max_payout_ratio;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    /// Pause/unpause contract (admin only)
    pub fn set_paused(env: Env, admin: Address, paused: bool) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        config.paused = paused;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    /// Emergency withdrawal of entire pool (admin only)
    pub fn emergency_withdraw(env: Env, admin: Address) -> i128 {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let pool: i128 = env.storage().persistent().get(&DataKey::PremiumPool).unwrap_or(0);

        if pool > 0 {
            let config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
            let token_client = token::Client::new(&env, &config.payment_token);

            token_client.transfer(&env.current_contract_address(), &admin, &pool);

            env.storage().persistent().set(&DataKey::PremiumPool, &0i128);
            env.storage().persistent().set(&DataKey::ReservePool, &0i128);
        }

        pool
    }

    // ───────────── INTERNAL HELPERS ─────────────

    fn calculate_premium(
        _env: &Env,
        _config: &PoolConfig,
        premium_rate: u32,
        coverage_amount: i128,
        coverage_period: u64,
    ) -> i128 {
        // Calculate: coverage_amount * premium_rate * (period_days / 365) / BASIS_POINTS
        let coverage_days = coverage_period / SECONDS_PER_DAY;
        
        // Premium = coverage_amount * premium_rate * (coverage_days / 365) / BASIS_POINTS
        let premium = (coverage_amount * premium_rate as i128 * coverage_days as i128) 
            / (365 * BASIS_POINTS as i128);

        // Ensure minimum premium of 1
        if premium < 1 {
            1
        } else {
            premium
        }
    }

    fn calculate_premium_rate(config: &PoolConfig, risk_level: RiskLevel) -> u32 {
        // Adjust base rate based on risk level
        let risk_multiplier = match risk_level {
            RiskLevel::Low => 80,      // 0.8x
            RiskLevel::Medium => 100,  // 1.0x
            RiskLevel::High => 150,    // 1.5x
            RiskLevel::Critical => 200, // 2.0x
        };

        (config.base_premium_rate * risk_multiplier) / 100
    }

    fn assess_contract_risk(env: &Env, contract_address: &Address) -> RiskLevel {
        // Check if contract has existing risk score
        if let Some(risk_score) = Self::get_risk_score(env.clone(), contract_address.clone()) {
            return risk_score.risk_level;
        }

        // Default to Medium risk for new contracts
        RiskLevel::Medium
    }

    fn risk_level_to_score(risk_level: RiskLevel) -> u32 {
        match risk_level {
            RiskLevel::Low => 25,
            RiskLevel::Medium => 50,
            RiskLevel::High => 75,
            RiskLevel::Critical => 100,
        }
    }

    fn verify_claim(env: &Env, contract_address: &Address, _submitter: &Address) {
        // Check if contract has too many recent claims
        let claims = Self::get_contract_claims(env.clone(), contract_address.clone());
        let current_time = env.ledger().timestamp();
        let lookback_period = 30 * SECONDS_PER_DAY;

        let mut recent_claims = 0u32;
        for claim_id in claims.iter() {
            if let Some(claim) = env.storage().persistent().get::<DataKey, Claim>(&DataKey::Claim(claim_id)) {
                if current_time - claim.submission_time < lookback_period {
                    recent_claims += 1;
                }
            }
        }

        // Max 3 claims per 30 days per contract
        if recent_claims >= 3 {
            panic!("Too many recent claims for this contract");
        }

        // Check if submitter has submitted claims for multiple contracts recently
        // (Simple check - could be enhanced with more sophisticated fraud detection)
        let all_policies = env.storage().persistent()
            .get::<DataKey, Vec<Address>>(&DataKey::PolicyList)
            .unwrap_or(Vec::new(env));

        let mut submitter_claims = 0u32;
        for policy_addr in all_policies.iter() {
            let policy_claims = Self::get_contract_claims(env.clone(), policy_addr);
            for claim_id in policy_claims.iter() {
                if let Some(claim) = env.storage().persistent().get::<DataKey, Claim>(&DataKey::Claim(claim_id)) {
                    // Check if this claim was submitted by the same submitter
                    // (In a real implementation, you'd track submitter in the claim)
                    if current_time - claim.submission_time < lookback_period {
                        submitter_claims += 1;
                    }
                }
            }
        }

        if submitter_claims >= 5 {
            panic!("Submitter has too many recent claims");
        }
    }

    fn get_risk_score(env: Env, contract_address: Address) -> Option<RiskScore> {
        env.storage().persistent().get(&DataKey::RiskScore(contract_address))
    }

    fn update_risk_score_on_claim(env: &Env, contract_address: Address) {
        let mut risk_score = Self::get_risk_score(env.clone(), contract_address.clone())
            .unwrap_or(RiskScore {
                contract_address: contract_address.clone(),
                risk_level: RiskLevel::Medium,
                score: 50,
                total_claims: 0,
                approved_claims: 0,
                total_payout: 0,
                last_updated: env.ledger().timestamp(),
            });

        risk_score.total_claims += 1;
        risk_score.last_updated = env.ledger().timestamp();

        // Adjust risk level based on claim frequency
        if risk_score.total_claims >= 3 {
            risk_score.risk_level = RiskLevel::High;
            risk_score.score = 75;
        } else if risk_score.total_claims >= 5 {
            risk_score.risk_level = RiskLevel::Critical;
            risk_score.score = 100;
        }

        env.storage().persistent().set(&DataKey::RiskScore(contract_address), &risk_score);
    }

    fn update_risk_score_on_approval(env: &Env, contract_address: Address, payout_amount: i128) {
        let mut risk_score = Self::get_risk_score(env.clone(), contract_address.clone())
            .unwrap_or(RiskScore {
                contract_address: contract_address.clone(),
                risk_level: RiskLevel::Medium,
                score: 50,
                total_claims: 0,
                approved_claims: 0,
                total_payout: 0,
                last_updated: env.ledger().timestamp(),
            });

        risk_score.approved_claims += 1;
        risk_score.total_payout += payout_amount;
        risk_score.last_updated = env.ledger().timestamp();

        env.storage().persistent().set(&DataKey::RiskScore(contract_address), &risk_score);
    }

    fn update_metrics_on_purchase(env: &Env, premium: i128) {
        let mut metrics = env.storage().persistent()
            .get::<DataKey, PoolMetrics>(&DataKey::PoolMetrics)
            .unwrap_or(PoolMetrics {
                total_premiums: 0,
                total_payouts: 0,
                active_policies: 0,
                total_claims_submitted: 0,
                total_claims_approved: 0,
                pool_utilization: 0,
                reserve_ratio: 2000,
            });

        metrics.total_premiums += premium;
        metrics.active_policies += 1;

        env.storage().persistent().set(&DataKey::PoolMetrics, &metrics);
        Self::update_pool_utilization(env);
    }

    fn update_metrics_on_cancel(env: &Env) {
        let mut metrics = env.storage().persistent()
            .get::<DataKey, PoolMetrics>(&DataKey::PoolMetrics)
            .unwrap_or(PoolMetrics {
                total_premiums: 0,
                total_payouts: 0,
                active_policies: 0,
                total_claims_submitted: 0,
                total_claims_approved: 0,
                pool_utilization: 0,
                reserve_ratio: 2000,
            });

        if metrics.active_policies > 0 {
            metrics.active_policies -= 1;
        }

        env.storage().persistent().set(&DataKey::PoolMetrics, &metrics);
    }

    fn update_metrics_on_claim(env: &Env) {
        let mut metrics = env.storage().persistent()
            .get::<DataKey, PoolMetrics>(&DataKey::PoolMetrics)
            .unwrap_or(PoolMetrics {
                total_premiums: 0,
                total_payouts: 0,
                active_policies: 0,
                total_claims_submitted: 0,
                total_claims_approved: 0,
                pool_utilization: 0,
                reserve_ratio: 2000,
            });

        metrics.total_claims_submitted += 1;

        env.storage().persistent().set(&DataKey::PoolMetrics, &metrics);
    }

    fn update_metrics_on_review(env: &Env, approved: bool) {
        let mut metrics = env.storage().persistent()
            .get::<DataKey, PoolMetrics>(&DataKey::PoolMetrics)
            .unwrap_or(PoolMetrics {
                total_premiums: 0,
                total_payouts: 0,
                active_policies: 0,
                total_claims_submitted: 0,
                total_claims_approved: 0,
                pool_utilization: 0,
                reserve_ratio: 2000,
            });

        if approved {
            metrics.total_claims_approved += 1;
        }

        env.storage().persistent().set(&DataKey::PoolMetrics, &metrics);
    }

    fn update_metrics_on_payout(env: &Env, payout_amount: i128) {
        let mut metrics = env.storage().persistent()
            .get::<DataKey, PoolMetrics>(&DataKey::PoolMetrics)
            .unwrap_or(PoolMetrics {
                total_premiums: 0,
                total_payouts: 0,
                active_policies: 0,
                total_claims_submitted: 0,
                total_claims_approved: 0,
                pool_utilization: 0,
                reserve_ratio: 2000,
            });

        metrics.total_payouts += payout_amount;

        env.storage().persistent().set(&DataKey::PoolMetrics, &metrics);
        Self::update_pool_utilization(env);
    }

    fn update_pool_utilization(env: &Env) {
        let mut metrics = env.storage().persistent()
            .get::<DataKey, PoolMetrics>(&DataKey::PoolMetrics)
            .unwrap_or(PoolMetrics {
                total_premiums: 0,
                total_payouts: 0,
                active_policies: 0,
                total_claims_submitted: 0,
                total_claims_approved: 0,
                pool_utilization: 0,
                reserve_ratio: 2000,
            });

        let pool: i128 = env.storage().persistent().get(&DataKey::PremiumPool).unwrap_or(0);
        
        if pool > 0 {
            metrics.pool_utilization = ((metrics.total_payouts * BASIS_POINTS as i128) / pool) as u32;
        } else {
            metrics.pool_utilization = 0;
        }

        let reserve: i128 = env.storage().persistent().get(&DataKey::ReservePool).unwrap_or(0);
        if pool > 0 {
            metrics.reserve_ratio = ((reserve * BASIS_POINTS as i128) / pool) as u32;
        } else {
            metrics.reserve_ratio = 0;
        }

        env.storage().persistent().set(&DataKey::PoolMetrics, &metrics);
    }

    fn add_to_policy_list(env: &Env, contract_address: Address) {
        let mut policies: Vec<Address> = env.storage().persistent()
            .get(&DataKey::PolicyList)
            .unwrap_or(Vec::new(env));

        if !policies.contains(&contract_address) {
            policies.push_back(contract_address);
            env.storage().persistent().set(&DataKey::PolicyList, &policies);
        }
    }

    fn add_to_contract_claims(env: &Env, contract_address: Address, claim_id: u64) {
        let mut claims: Vec<u64> = env.storage().persistent()
            .get(&DataKey::ContractClaims(contract_address.clone()))
            .unwrap_or(Vec::new(env));

        claims.push_back(claim_id);
        env.storage().persistent().set(&DataKey::ContractClaims(contract_address), &claims);
    }

    fn assert_admin(env: &Env, user: &Address) {
        let config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        if config.admin != *user {
            panic!("Admin only");
        }
    }

    fn assert_not_paused(env: &Env) {
        let config: PoolConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        if config.paused {
            panic!("Contract is paused");
        }
    }
}

mod test;
