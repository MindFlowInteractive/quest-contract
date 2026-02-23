#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    Symbol, Vec,
};

// ============================================================================
// ERROR DEFINITIONS
// ============================================================================

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AntiBotError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    RateLimitExceeded = 4,
    BotDetected = 5,
    InvalidWindow = 6,
    AppealNotFound = 7,
    AlreadyAppealed = 8,
    AppealPeriodExpired = 9,
    InsufficientReputation = 10,
    PenaltyActive = 11,
    VerificationFailed = 12,
    SuspiciousActivity = 13,
    TimeWindowInvalid = 14,
    ChallengeExpired = 15,
    InvalidProof = 16,
}

// ============================================================================
// DATA STRUCTURES
// ============================================================================

// #[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PlayerStatus {
    Unverified,
    Verified,
    Suspicious,
    Flagged,
    Penalized,
    AppealPending,
    Whitelisted,
    Blacklisted,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerProfile {
    pub address: Address,
    pub status: u32,
    pub trust_score: u32,
    pub total_attempts: u32,
    pub successful_attempts: u32,
    pub failed_attempts: u32,
    pub avg_solve_time_ms: u64,
    pub first_seen: u64,
    pub last_activity: u64,
    pub consecutive_fast_solves: u32,
    pub reputation_tier: u32,
    pub penalty_count: u32,
    pub appeal_count: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct ActivityRecord {
    pub timestamp: u64,
    pub puzzle_id: u32,
    pub solve_time_ms: u64,
    pub gas_used: u64,
    pub success: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct RateLimitWindow {
    pub window_start: u64,
    pub attempt_count: u32,
    pub last_attempt: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct BehavioralPattern {
    pub avg_interaction_interval_ms: u64,
    pub pattern_variance: u32,   // 0-1000, lower = more bot-like
    pub consistency_score: u32,  // 0-1000, higher = more bot-like
    pub time_distribution: Vec<u64>, // timestamps of last 10 interactions
    pub gas_pattern_variance: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct CaptchaChallenge {
    pub challenge_id: u32,
    pub difficulty: u32,          // 1-5 difficulty levels
    pub created_at: u64,
    pub expires_at: u64,
    pub target_prefix: BytesN<4>, // Hash prefix challenge
    pub min_iterations: u32,     // Proof of work iterations required
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct CaptchaProof {
    pub challenge_id: u32,
    pub nonce: u64,
    pub iterations: u32,
    pub proof_hash: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct SuspiciousActivity {
    pub activity_type: ActivityType,
    pub timestamp: u64,
    pub evidence: Symbol,
    pub severity: u32,            // 1-10 severity scale
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ActivityType {
    TooFastSolve,
    PatternMatch,
    GasAnomaly,
    RateLimitViolation,
    FailedCaptcha,
    RepeatedFailures,
    TimestampManipulation,
    SuspiciousTiming,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PenaltyRecord {
    pub penalty_id: u32,
    pub player: Address,
    pub penalty_type: PenaltyType,
    pub reason: Symbol,
    pub severity: u32,
    pub applied_at: u64,
    pub expires_at: u64,
    pub active: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PenaltyType {
    Warning,
    TemporaryBan,      // Hours
    ExtendedBan,       // Days
    PermanentBan,
    ScoreReduction,
    VerificationRequired,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Appeal {
    pub appeal_id: u32,
    pub player: Address,
    pub penalty_id: u32,
    pub reason: Symbol,
    pub evidence: Symbol,
    pub submitted_at: u64,
    pub status: AppealStatus,
    pub reviewed_by: Option<Address>,
    pub reviewed_at: Option<u64>,
    pub decision_reason: Option<Symbol>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum AppealStatus {
    Pending,
    Approved,
    Rejected,
    UnderReview,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TimeWindow {
    pub puzzle_id: u32,
    pub min_solve_time_ms: u64,
    pub max_solve_time_ms: u64,
    pub submission_start: u64,
    pub submission_end: u64,
    pub grace_period_ms: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Config {
    pub admin: Address,
    pub verifiers: Vec<Address>,
    pub max_attempts_per_window: u32,
    pub rate_limit_window_seconds: u64,
    pub min_solve_time_threshold_ms: u64,
    pub suspicious_solve_time_ms: u64,
    pub captcha_difficulty: u32,
    pub captcha_validity_seconds: u64,
    pub pattern_analysis_window: u32,
    pub max_consecutive_fast_solves: u32,
    pub appeal_period_days: u64,
    pub default_penalty_hours: u64,
    pub trust_score_threshold: u32,
    pub reputation_contract: Option<Address>,
}

// ============================================================================
// STORAGE KEYS
// ============================================================================

#[contracttype]
pub enum DataKey {
    Config,
    Initialized,
    PlayerProfile(Address),
    ActivityHistory(Address, u32), // (player, index)
    ActivityCount(Address),
    RateLimit(Address),
    BehavioralPattern(Address),
    CaptchaChallenge(u32),
    ChallengeCounter,
    SuspiciousActivity(Address, u32), // (player, index)
    SuspiciousCount(Address),
    Penalty(u32),
    PenaltyCounter,
    PlayerPenalties(Address, u32), // (player, index)
    PlayerPenaltyCount(Address),
    Appeal(u32),
    AppealCounter,
    TimeWindow(u32), // puzzle_id
    Whitelisted(Address),
    Blacklisted(Address),
    VerificationNonce(Address),
}

// ============================================================================
// CONTRACT
// ============================================================================

#[contract]
pub struct AntiBot;

#[contractimpl]
impl AntiBot {
    // ========================================================================
    // INITIALIZATION
    // ========================================================================

    pub fn initialize(env: Env, admin: Address) -> Result<(), AntiBotError> {
        if env.storage().instance().has(&DataKey::Initialized) {
            return Err(AntiBotError::AlreadyInitialized);
        }

        admin.require_auth();

        let config = Config {
            admin: admin.clone(),
            verifiers: Vec::new(&env),
            max_attempts_per_window: 10,
            rate_limit_window_seconds: 300, // 5 minutes
            min_solve_time_threshold_ms: 5000, // 5 seconds minimum
            suspicious_solve_time_ms: 2000,   // 2 seconds is suspicious
            captcha_difficulty: 2,
            captcha_validity_seconds: 300,    // 5 minutes
            pattern_analysis_window: 10,
            max_consecutive_fast_solves: 3,
            appeal_period_days: 7,
            default_penalty_hours: 24,
            trust_score_threshold: 500,
            reputation_contract: None,
        };

        env.storage().instance().set(&DataKey::Config, &config);
        env.storage().instance().set(&DataKey::Initialized, &true);
        env.storage().instance().set(&DataKey::ChallengeCounter, &0u32);
        env.storage().instance().set(&DataKey::PenaltyCounter, &0u32);
        env.storage().instance().set(&DataKey::AppealCounter, &0u32);

        env.events().publish(
            (symbol_short!("init"), admin),
            (),
        );

        Ok(())
    }

    fn require_admin(env: &Env) -> Result<Address, AntiBotError> {
        let config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(AntiBotError::NotInitialized)?;

        config.admin.require_auth();
        Ok(config.admin)
    }

    fn require_verifier(env: &Env) -> Result<(), AntiBotError> {
        let config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(AntiBotError::NotInitialized)?;

        let caller = env.current_contract_address(); // Use invoker in real scenario
        // For now, require admin or check if caller is in verifiers list
        config.admin.require_auth();
        Ok(())
    }

    // ========================================================================
    // PLAYER PROFILE MANAGEMENT
    // ========================================================================

    fn get_or_create_profile(env: &Env, player: &Address) -> PlayerProfile {
        if let Some(profile) = env
            .storage()
            .persistent()
            .get(&DataKey::PlayerProfile(player.clone()))
        {
            return profile;
        }

        let now = env.ledger().timestamp();
        let profile = PlayerProfile {
            address: player.clone(),
            status: 0,
            trust_score: 500, // Start at neutral
            total_attempts: 0,
            successful_attempts: 0,
            failed_attempts: 0,
            avg_solve_time_ms: 0,
            first_seen: now,
            last_activity: now,
            consecutive_fast_solves: 0,
            reputation_tier: 0,
            penalty_count: 0,
            appeal_count: 0,
        };

        env.storage()
            .persistent()
            .set(&DataKey::PlayerProfile(player.clone()), &profile);

        profile
    }

    pub fn get_profile(env: Env, player: Address) -> Option<PlayerProfile> {
        env.storage()
            .persistent()
            .get(&DataKey::PlayerProfile(player))
    }

    fn update_profile(env: &Env, player: &Address, profile: &PlayerProfile) {
        env.storage()
            .persistent()
            .set(&DataKey::PlayerProfile(player.clone()), profile);
    }

    // ========================================================================
    // CAPTCHA-LIKE VERIFICATION
    // ========================================================================

    pub fn generate_captcha_challenge(env: Env, player: Address) -> Result<CaptchaChallenge, AntiBotError> {
        player.require_auth();

        let config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(AntiBotError::NotInitialized)?;

        let counter: u32 = env
            .storage()
            .instance()
            .get(&DataKey::ChallengeCounter)
            .unwrap_or(0);

        let new_counter = counter + 1;
        env.storage().instance().set(&DataKey::ChallengeCounter, &new_counter);

        let now = env.ledger().timestamp();
        let difficulty = config.captcha_difficulty;
        
        // Generate pseudo-random challenge based on timestamp and player address
        let seed_bytes = Bytes::from_array(&env, &[
            (now & 0xFF) as u8,
            ((now >> 8) & 0xFF) as u8,
            ((now >> 16) & 0xFF) as u8,
            ((now >> 24) & 0xFF) as u8,
        ]);
        
        let hash: BytesN<32> = env.crypto().sha256(&seed_bytes).into();
        let mut prefix_bytes = [0u8; 4];
        for i in 0..4u32 {
            prefix_bytes[i as usize] = hash.get(i).unwrap_or(0);
        }
        let target_prefix = BytesN::from_array(&env, &prefix_bytes);

        let min_iterations = match difficulty {
            1 => 100,
            2 => 500,
            3 => 2000,
            4 => 10000,
            5 => 50000,
            _ => 500,
        };

        let challenge = CaptchaChallenge {
            challenge_id: new_counter,
            difficulty,
            created_at: now,
            expires_at: now + config.captcha_validity_seconds,
            target_prefix,
            min_iterations,
        };

        env.storage()
            .persistent()
            .set(&DataKey::CaptchaChallenge(new_counter), &challenge);

        env.events().publish(
            (symbol_short!("captcha"), symbol_short!("gen")),
            (player, new_counter),
        );

        Ok(challenge)
    }

    pub fn verify_captcha_proof(
        env: Env,
        player: Address,
        proof: CaptchaProof,
    ) -> Result<bool, AntiBotError> {
        player.require_auth();

        let challenge: CaptchaChallenge = env
            .storage()
            .persistent()
            .get(&DataKey::CaptchaChallenge(proof.challenge_id))
            .ok_or(AntiBotError::ChallengeExpired)?;

        let now = env.ledger().timestamp();
        if now > challenge.expires_at {
            return Err(AntiBotError::ChallengeExpired);
        }

        if proof.iterations < challenge.min_iterations {
            return Err(AntiBotError::VerificationFailed);
        }

        // Verify proof of work
        let mut data = Bytes::new(&env);
        // Use the player's raw bytes representation
        // In Soroban, we can convert Address to bytes using to_string or by using as a key
        // For simplicity, we'll use a hash of the address combined with the challenge_id
        let challenge_bytes = Bytes::from_array(&env, &[
            ((proof.challenge_id >> 0) & 0xFF) as u8,
            ((proof.challenge_id >> 8) & 0xFF) as u8,
            ((proof.challenge_id >> 16) & 0xFF) as u8,
            ((proof.challenge_id >> 24) & 0xFF) as u8,
        ]);
        data.append(&challenge_bytes);
        
        // Append nonce bytes
        for i in 0..8 {
            data.push_back(((proof.nonce >> (i * 8)) & 0xFF) as u8);
        }

        let computed_hash: BytesN<32> = env.crypto().sha256(&data).into();
        
        // Verify proof hash matches computed
        if computed_hash != proof.proof_hash {
            return Err(AntiBotError::VerificationFailed);
        }

        // Check prefix match (simplified: first 2 bytes should be below threshold)
        let threshold: u16 = match challenge.difficulty {
            1 => 0x4000, // ~25% chance
            2 => 0x1000, // ~6% chance
            3 => 0x0400, // ~1.5% chance
            4 => 0x0100, // ~0.4% chance
            5 => 0x0040, // ~0.06% chance
            _ => 0x1000,
        };

        let prefix_value: u16 = (computed_hash.get(0).unwrap_or(0) as u16) << 8
            | (computed_hash.get(1).unwrap_or(0) as u16);

        let verified = prefix_value < threshold;

        if verified {
            let mut profile = Self::get_or_create_profile(&env, &player);
            profile.status = 1;
            Self::update_profile(&env, &player, &profile);

            env.events().publish(
                (symbol_short!("captcha"), symbol_short!("pass")),
                (player, proof.challenge_id),
            );
        } else {
            Self::record_suspicious_activity(
                &env,
                &player,
                ActivityType::FailedCaptcha,
                symbol_short!("bad_proof"),
                5,
            )?;

            env.events().publish(
                (symbol_short!("captcha"), symbol_short!("fail")),
                (player, proof.challenge_id),
            );
        }

        Ok(verified)
    }

    // ========================================================================
    // RATE LIMITING
    // ========================================================================

    pub fn check_rate_limit(env: Env, player: Address) -> Result<(), AntiBotError> {
        let config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(AntiBotError::NotInitialized)?;

        let now = env.ledger().timestamp();
        
        let mut window: RateLimitWindow = env
            .storage()
            .persistent()
            .get(&DataKey::RateLimit(player.clone()))
            .unwrap_or(RateLimitWindow {
                window_start: now,
                attempt_count: 0,
                last_attempt: 0,
            });

        // Check if window has expired
        if now - window.window_start > config.rate_limit_window_seconds {
            window.window_start = now;
            window.attempt_count = 0;
        }

        // Check rate limit
        if window.attempt_count >= config.max_attempts_per_window {
            Self::record_suspicious_activity(
                &env,
                &player,
                ActivityType::RateLimitViolation,
                symbol_short!("rate_lim"),
                6,
            )?;
            return Err(AntiBotError::RateLimitExceeded);
        }

        window.attempt_count += 1;
        window.last_attempt = now;

        env.storage()
            .persistent()
            .set(&DataKey::RateLimit(player.clone()), &window);

        Ok(())
    }

    pub fn get_rate_limit_status(env: Env, player: Address) -> RateLimitWindow {
        let now = env.ledger().timestamp();
        env.storage()
            .persistent()
            .get(&DataKey::RateLimit(player))
            .unwrap_or(RateLimitWindow {
                window_start: now,
                attempt_count: 0,
                last_attempt: 0,
            })
    }

    // ========================================================================
    // BEHAVIORAL PATTERN ANALYSIS
    // ========================================================================

    pub fn record_activity(
        env: Env,
        player: Address,
        puzzle_id: u32,
        solve_time_ms: u64,
        gas_used: u64,
        success: bool,
    ) -> Result<(), AntiBotError> {
        player.require_auth();

        let config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(AntiBotError::NotInitialized)?;

        let now = env.ledger().timestamp();
        
        // Get or create profile
        let mut profile = Self::get_or_create_profile(&env, &player);
        
        // Update basic stats
        profile.total_attempts += 1;
        profile.last_activity = now;
        
        if success {
            profile.successful_attempts += 1;
        } else {
            profile.failed_attempts += 1;
        }

        // Check for too-fast solve
        if success && solve_time_ms < config.min_solve_time_threshold_ms {
            profile.consecutive_fast_solves += 1;
            
            if solve_time_ms < config.suspicious_solve_time_ms {
                Self::record_suspicious_activity(
                    &env,
                    &player,
                    ActivityType::TooFastSolve,
                    symbol_short!("fast_slv"),
                    7,
                )?;
            }
        } else {
            profile.consecutive_fast_solves = 0;
        }

        // Update average solve time
        if success && profile.successful_attempts > 0 {
            profile.avg_solve_time_ms = 
                (profile.avg_solve_time_ms * (profile.successful_attempts - 1) as u64 + solve_time_ms)
                    / profile.successful_attempts as u64;
        }

        // Check for bot patterns
        if profile.consecutive_fast_solves >= config.max_consecutive_fast_solves {
            Self::record_suspicious_activity(
                &env,
                &player,
                ActivityType::PatternMatch,
                symbol_short!("bot_patt"),
                8,
            )?;
        }

        // Store activity record
        let activity = ActivityRecord {
            timestamp: now,
            puzzle_id,
            solve_time_ms,
            gas_used,
            success,
        };

        let count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ActivityCount(player.clone()))
            .unwrap_or(0);
        
        env.storage()
            .persistent()
            .set(&DataKey::ActivityHistory(player.clone(), count), &activity);
        env.storage()
            .persistent()
            .set(&DataKey::ActivityCount(player.clone()), &(count + 1));

        // Update behavioral pattern
        Self::update_behavioral_pattern(&env, &player, &activity)?;

        // Update trust score based on activity
        Self::update_trust_score(&env, &mut profile)?;
        
        Self::update_profile(&env, &player, &profile);

        env.events().publish(
            (symbol_short!("activity"), player),
            (puzzle_id, solve_time_ms, success),
        );

        Ok(())
    }

    fn update_behavioral_pattern(
        env: &Env,
        player: &Address,
        new_activity: &ActivityRecord,
    ) -> Result<(), AntiBotError> {
        let config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(AntiBotError::NotInitialized)?;

        let window_size = config.pattern_analysis_window as usize;
        
        let mut pattern: BehavioralPattern = env
            .storage()
            .persistent()
            .get(&DataKey::BehavioralPattern(player.clone()))
            .unwrap_or(BehavioralPattern {
                avg_interaction_interval_ms: 0,
                pattern_variance: 500,
                consistency_score: 500,
                time_distribution: Vec::new(&env),
                gas_pattern_variance: 500,
            });

        // Add new timestamp
        pattern.time_distribution.push_back(new_activity.timestamp);
        
        // Keep only last N timestamps
        while pattern.time_distribution.len() > window_size as u32 {
            let _ = pattern.time_distribution.remove(0);
        }

        // Calculate consistency metrics if we have enough data
        if pattern.time_distribution.len() >= 3 {
            let mut intervals = Vec::new(&env);
            let len = pattern.time_distribution.len();
            
            for i in 1..len {
                let current = pattern.time_distribution.get(i).unwrap_or(0);
                let prev = pattern.time_distribution.get(i - 1).unwrap_or(0);
                intervals.push_back(current - prev);
            }

            // Calculate average interval
            let sum: u64 = intervals.iter().fold(0, |acc, x| acc + x);
            pattern.avg_interaction_interval_ms = sum / intervals.len() as u64;

            // Calculate variance (simplified)
            let mut variance_sum: u64 = 0;
            for i in 0..intervals.len() {
                let diff = if intervals.get(i).unwrap_or(0) > pattern.avg_interaction_interval_ms {
                    intervals.get(i).unwrap_or(0) - pattern.avg_interaction_interval_ms
                } else {
                    pattern.avg_interaction_interval_ms - intervals.get(i).unwrap_or(0)
                };
                variance_sum += diff * diff;
            }
            
            let variance = variance_sum / intervals.len() as u64;
            
            // High consistency (low variance) is suspicious for bots
            // Normalize to 0-1000 scale
            pattern.pattern_variance = (variance.min(1000000) / 1000) as u32;
            pattern.consistency_score = (1000 - pattern.pattern_variance).min(1000);

            // Check for suspicious gas patterns
            if new_activity.gas_used > 0 {
                // Bot might use consistent gas amounts
                let gas_threshold = 100; // threshold for gas consistency
                pattern.gas_pattern_variance = 
                    if new_activity.gas_used % 1000 == 0 {
                        900 // suspicious: round gas usage
                    } else {
                        400 // normal variance
                    };
            }
        }

        env.storage()
            .persistent()
            .set(&DataKey::BehavioralPattern(player.clone()), &pattern);

        Ok(())
    }

    pub fn get_behavioral_pattern(env: Env, player: Address) -> Option<BehavioralPattern> {
        env.storage()
            .persistent()
            .get(&DataKey::BehavioralPattern(player))
    }

    pub fn analyze_player(env: Env, player: Address) -> Result<PlayerAnalysis, AntiBotError> {
        let profile = Self::get_or_create_profile(&env, &player);
        let pattern = Self::get_behavioral_pattern(env.clone(), player.clone());
        
        let mut risk_factors: Vec<Symbol> = Vec::new(&env);
        let mut recommendation = symbol_short!("allow");
        let mut bot_probability: u32 = 0;

        // Analyze based on profile
        if profile.consecutive_fast_solves >= 3 {
            risk_factors.push_back(symbol_short!("fast_slv"));
            bot_probability += 30;
        }

        if profile.failed_attempts > profile.successful_attempts * 2 {
            risk_factors.push_back(symbol_short!("high_fail"));
            bot_probability += 20;
        }

        // Analyze behavioral pattern
        if let Some(p) = pattern {
            if p.consistency_score > 800 {
                risk_factors.push_back(symbol_short!("cons_time"));
                bot_probability += 25;
            }
            
            if p.gas_pattern_variance > 700 {
                risk_factors.push_back(symbol_short!("gas_patt"));
                bot_probability += 15;
            }

            if p.pattern_variance < 100 {
                risk_factors.push_back(symbol_short!("low_var"));
                bot_probability += 20;
            }
        }

        // Check trust score
        if profile.trust_score < 300 {
            risk_factors.push_back(symbol_short!("low_trust"));
            bot_probability += 20;
        }

        // Determine recommendation
        if bot_probability > 70 {
            recommendation = symbol_short!("block");
        } else if bot_probability > 40 {
            recommendation = symbol_short!("verify");
        }

        Ok(PlayerAnalysis {
            player,
            trust_score: profile.trust_score,
            bot_probability: bot_probability.min(100),
            risk_factors,
            recommendation,
            status: profile.status,
        })
    }

    // ========================================================================
    // SUSPICIOUS ACTIVITY FLAGGING
    // ========================================================================

    fn record_suspicious_activity(
        env: &Env,
        player: &Address,
        activity_type: ActivityType,
        evidence: Symbol,
        severity: u32,
    ) -> Result<(), AntiBotError> {
        let now = env.ledger().timestamp();
        let evidence_for_event = evidence.clone();
        
        let activity = SuspiciousActivity {
            activity_type,
            timestamp: now,
            evidence,
            severity,
        };

        let count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::SuspiciousCount(player.clone()))
            .unwrap_or(0);
        
        env.storage()
            .persistent()
            .set(&DataKey::SuspiciousActivity(player.clone(), count), &activity);
        env.storage()
            .persistent()
            .set(&DataKey::SuspiciousCount(player.clone()), &(count + 1));

        // Update player status if needed
        let mut profile = Self::get_or_create_profile(env, player);
        
        if profile.status != 3 
            && profile.status != 4
            && profile.status != 7 {
            
            // Check if we should flag the player
            let total_suspicious = count + 1;
            let high_severity_count = Self::count_high_severity_activities(env, player);
            
            if severity >= 8 || (total_suspicious >= 5 && high_severity_count >= 2) {
                profile.status = 3;
                Self::update_profile(env, player, &profile);
                
                env.events().publish(
                    (symbol_short!("flagged"), player.clone()),
                    (evidence_for_event, severity),
                );
            }
        }

        Ok(())
    }

    fn count_high_severity_activities(env: &Env, player: &Address) -> u32 {
        let count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::SuspiciousCount(player.clone()))
            .unwrap_or(0);
        
        let mut high_count = 0;
        for i in 0..count {
            if let Some(activity) = env
                .storage()
                .persistent()
                .get::<DataKey, SuspiciousActivity>(&DataKey::SuspiciousActivity(player.clone(), i))
            {
                if activity.severity >= 7 {
                    high_count += 1;
                }
            }
        }
        high_count
    }

    pub fn get_suspicious_activities(
        env: Env,
        player: Address,
        offset: u32,
        limit: u32,
    ) -> Vec<SuspiciousActivity> {
        let count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::SuspiciousCount(player.clone()))
            .unwrap_or(0);
        
        let mut result = Vec::new(&env);
        let start = offset.min(count);
        let end = (offset + limit).min(count);
        
        for i in start..end {
            if let Some(activity) = env
                .storage()
                .persistent()
                .get::<DataKey, SuspiciousActivity>(&DataKey::SuspiciousActivity(player.clone(), i))
            {
                result.push_back(activity);
            }
        }
        
        result
    }

    pub fn flag_player(
        env: Env,
        player: Address,
        reason: Symbol,
        severity: u32,
    ) -> Result<(), AntiBotError> {
        Self::require_verifier(&env)?;

        let mut profile = Self::get_or_create_profile(&env, &player);
        profile.status = 3;
        Self::update_profile(&env, &player, &profile);

        Self::record_suspicious_activity(&env, &player, ActivityType::SuspiciousTiming, reason.clone(), severity)?;

        env.events().publish(
            (symbol_short!("flagged"), player),
            (reason, severity),
        );

        Ok(())
    }

    pub fn unflag_player(env: Env, player: Address) -> Result<(), AntiBotError> {
        Self::require_admin(&env)?;

        let mut profile = Self::get_or_create_profile(&env, &player);
        
        match profile.status {
            3 | 2 => {
                profile.status = 1;
                Self::update_profile(&env, &player, &profile);
                
                env.events().publish(
                    (symbol_short!("unflagged"), player),
                    (),
                );
                Ok(())
            }
            _ => Err(AntiBotError::Unauthorized),
        }
    }

    // ========================================================================
    // TIME-BASED SUBMISSION WINDOWS
    // ========================================================================

    pub fn set_time_window(
        env: Env,
        puzzle_id: u32,
        min_solve_time_ms: u64,
        max_solve_time_ms: u64,
        submission_start: u64,
        submission_end: u64,
        grace_period_ms: u64,
    ) -> Result<(), AntiBotError> {
        Self::require_admin(&env)?;

        if submission_end <= submission_start {
            return Err(AntiBotError::InvalidWindow);
        }

        if min_solve_time_ms >= max_solve_time_ms {
            return Err(AntiBotError::InvalidWindow);
        }

        let window = TimeWindow {
            puzzle_id,
            min_solve_time_ms,
            max_solve_time_ms,
            submission_start,
            submission_end,
            grace_period_ms,
        };

        env.storage()
            .persistent()
            .set(&DataKey::TimeWindow(puzzle_id), &window);

        env.events().publish(
            (symbol_short!("window"), puzzle_id),
            (min_solve_time_ms, max_solve_time_ms, submission_start, submission_end),
        );

        Ok(())
    }

    pub fn validate_submission_time(
        env: Env,
        player: Address,
        puzzle_id: u32,
        solve_time_ms: u64,
    ) -> Result<bool, AntiBotError> {
        let window: TimeWindow = env
            .storage()
            .persistent()
            .get(&DataKey::TimeWindow(puzzle_id))
            .ok_or(AntiBotError::InvalidWindow)?;

        let now = env.ledger().timestamp();

        // Check submission window
        if now < window.submission_start || now > window.submission_end + (window.grace_period_ms / 1000) {
            Self::record_suspicious_activity(
                &env,
                &player,
                ActivityType::TimestampManipulation,
                symbol_short!("time_win"),
                6,
            )?;
            return Err(AntiBotError::TimeWindowInvalid);
        }

        // Check solve time bounds
        if solve_time_ms < window.min_solve_time_ms {
            Self::record_suspicious_activity(
                &env,
                &player,
                ActivityType::TooFastSolve,
                symbol_short!("min_time"),
                7,
            )?;
            return Ok(false);
        }

        if solve_time_ms > window.max_solve_time_ms {
            // Too slow, but not necessarily suspicious
            return Ok(false);
        }

        Ok(true)
    }

    pub fn get_time_window(env: Env, puzzle_id: u32) -> Option<TimeWindow> {
        env.storage()
            .persistent()
            .get(&DataKey::TimeWindow(puzzle_id))
    }

    // ========================================================================
    // REPUTATION-BASED TRUST SCORING
    // ========================================================================

    fn update_trust_score(env: &Env, profile: &mut PlayerProfile) -> Result<(), AntiBotError> {
        let config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(AntiBotError::NotInitialized)?;

        let pattern = env
            .storage()
            .persistent()
            .get::<DataKey, BehavioralPattern>(&DataKey::BehavioralPattern(profile.address.clone()));

        let mut score: u32 = 500; // Start neutral

        // Factor 1: Success rate (0-300 points)
        if profile.total_attempts > 0 {
            let success_rate = (profile.successful_attempts * 300) / profile.total_attempts;
            score += success_rate.min(300);
        }

        // Factor 2: Account age (0-100 points)
        let now = env.ledger().timestamp();
        let account_age_days = (now - profile.first_seen) / 86400;
        let age_score = (account_age_days * 10).min(100) as u32;
        score += age_score;

        // Factor 3: Activity pattern (0-100 points, or -100 for bad patterns)
        if let Some(p) = pattern {
            if p.consistency_score > 800 {
                score = score.saturating_sub(100); // Suspicious consistency
            } else if p.consistency_score < 300 {
                score += 50; // Good human-like variance
            }
        }

        // Factor 4: Penalty history
        let penalty_deduction = profile.penalty_count * 50;
        score = score.saturating_sub(penalty_deduction);

        // Factor 5: Consecutive fast solves penalty
        if profile.consecutive_fast_solves > 0 {
            score = score.saturating_sub(profile.consecutive_fast_solves * 20);
        }

        // Ensure score is in valid range
        profile.trust_score = score.min(1000);

        // Update status based on trust score
        if profile.trust_score >= 800 && profile.status == 0 {
            profile.status = 1;
        }

        // Update reputation tier
        profile.reputation_tier = match profile.trust_score {
            0..=199 => 0, // New/Suspicious
            200..=399 => 1, // Low trust
            400..=599 => 2, // Neutral
            600..=749 => 3, // Good
            750..=899 => 4, // High trust
            _ => 5,         // Excellent
        };

        Ok(())
    }

    pub fn get_trust_score(env: Env, player: Address) -> u32 {
        let profile = Self::get_or_create_profile(&env, &player);
        profile.trust_score
    }

    pub fn get_reputation_tier(env: Env, player: Address) -> u32 {
        let profile = Self::get_or_create_profile(&env, &player);
        profile.reputation_tier
    }

    // ========================================================================
    // PENALTY SYSTEM
    // ========================================================================

    pub fn apply_penalty(
        env: Env,
        player: Address,
        penalty_type: PenaltyType,
        reason: Symbol,
        severity: u32,
    ) -> Result<u32, AntiBotError> {
        Self::require_verifier(&env)?;

        let now = env.ledger().timestamp();
        let config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(AntiBotError::NotInitialized)?;

        let penalty_id: u32 = env
            .storage()
            .instance()
            .get(&DataKey::PenaltyCounter)
            .unwrap_or(0) + 1;
        
        env.storage().instance().set(&DataKey::PenaltyCounter, &penalty_id);

        // Calculate expiration
        let expires_at = match penalty_type {
            PenaltyType::Warning => now,
            PenaltyType::TemporaryBan => now + (config.default_penalty_hours * 3600),
            PenaltyType::ExtendedBan => now + (config.default_penalty_hours * 3600 * 7),
            PenaltyType::PermanentBan => u64::MAX,
            PenaltyType::ScoreReduction => now,
            PenaltyType::VerificationRequired => now + (config.default_penalty_hours * 3600),
        };

        let penalty = PenaltyRecord {
            penalty_id,
            player: player.clone(),
            penalty_type: penalty_type.clone(),
            reason: reason.clone(),
            severity,
            applied_at: now,
            expires_at,
            active: true,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Penalty(penalty_id), &penalty);

        // Add to player penalties list
        let player_penalty_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::PlayerPenaltyCount(player.clone()))
            .unwrap_or(0);
        
        env.storage()
            .persistent()
            .set(&DataKey::PlayerPenalties(player.clone(), player_penalty_count), &penalty_id);
        env.storage()
            .persistent()
            .set(&DataKey::PlayerPenaltyCount(player.clone()), &(player_penalty_count + 1));

        // Update player profile
        let mut profile = Self::get_or_create_profile(&env, &player);
        profile.penalty_count += 1;
        profile.status = 4;

        // Apply score reduction if needed
        if matches!(penalty_type, PenaltyType::ScoreReduction) {
            let reduction = (severity as u32 * 50).min(300);
            profile.trust_score = profile.trust_score.saturating_sub(reduction);
        }

        Self::update_profile(&env, &player, &profile);

        // Add to blacklist for permanent bans
        if matches!(penalty_type, PenaltyType::PermanentBan) {
            env.storage()
                .persistent()
                .set(&DataKey::Blacklisted(player.clone()), &true);
        }

        env.events().publish(
            (symbol_short!("penalty"), player),
            (penalty_id, reason.clone(), severity),
        );

        Ok(penalty_id)
    }

    pub fn remove_penalty(env: Env, penalty_id: u32) -> Result<(), AntiBotError> {
        Self::require_admin(&env)?;

        let mut penalty: PenaltyRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Penalty(penalty_id))
            .ok_or(AntiBotError::Unauthorized)?;

        penalty.active = false;
        
        env.storage()
            .persistent()
            .set(&DataKey::Penalty(penalty_id), &penalty);

        // Update player status if no active penalties
        let player = penalty.player.clone();
        if Self::get_active_penalties(env.clone(), player.clone()) == 0 {
            let mut profile = Self::get_or_create_profile(&env, &player);
            if profile.status == 4 {
                profile.status = 1;
                Self::update_profile(&env, &player, &profile);
            }
        }

        env.events().publish(
            (symbol_short!("pen_rem"), penalty_id),
            (),
        );

        Ok(())
    }

    pub fn get_penalty(env: Env, penalty_id: u32) -> Option<PenaltyRecord> {
        env.storage()
            .persistent()
            .get(&DataKey::Penalty(penalty_id))
    }

    pub fn get_active_penalties(env: Env, player: Address) -> u32 {
        let count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::PlayerPenaltyCount(player.clone()))
            .unwrap_or(0);
        
        let now = env.ledger().timestamp();
        let mut active = 0;
        
        for i in 0..count {
            if let Some(penalty_id) = env
                .storage()
                .persistent()
                .get::<DataKey, u32>(&DataKey::PlayerPenalties(player.clone(), i))
            {
                if let Some(penalty) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, PenaltyRecord>(&DataKey::Penalty(penalty_id))
                {
                    if penalty.active && penalty.expires_at > now {
                        active += 1;
                    }
                }
            }
        }
        
        active
    }

    pub fn check_penalty_status(env: Env, player: Address) -> Result<(), AntiBotError> {
        if Self::is_blacklisted(env.clone(), player.clone()) {
            return Err(AntiBotError::BotDetected);
        }

        if Self::get_active_penalties(env, player) > 0 {
            return Err(AntiBotError::PenaltyActive);
        }

        Ok(())
    }

    // ========================================================================
    // APPEAL MECHANISM
    // ========================================================================

    pub fn submit_appeal(
        env: Env,
        player: Address,
        penalty_id: u32,
        reason: Symbol,
        evidence: Symbol,
    ) -> Result<u32, AntiBotError> {
        player.require_auth();

        let now = env.ledger().timestamp();
        let config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(AntiBotError::NotInitialized)?;

        // Verify penalty exists and is active
        let penalty: PenaltyRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Penalty(penalty_id))
            .ok_or(AntiBotError::AppealNotFound)?;

        if !penalty.active {
            return Err(AntiBotError::AppealNotFound);
        }

        // Check appeal period
        let appeal_deadline = penalty.applied_at + (config.appeal_period_days * 86400);
        if now > appeal_deadline {
            return Err(AntiBotError::AppealPeriodExpired);
        }

        // Check if already appealed
        let appeal_count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::AppealCounter)
            .unwrap_or(0);
        
        for i in 1..=appeal_count {
            if let Some(existing_appeal) = env
                .storage()
                .persistent()
                .get::<DataKey, Appeal>(&DataKey::Appeal(i))
            {
                if existing_appeal.penalty_id == penalty_id 
                    && existing_appeal.status == AppealStatus::Pending {
                    return Err(AntiBotError::AlreadyAppealed);
                }
            }
        }

        let new_appeal_id = appeal_count + 1;
        env.storage().instance().set(&DataKey::AppealCounter, &new_appeal_id);

        let appeal = Appeal {
            appeal_id: new_appeal_id,
            player: player.clone(),
            penalty_id,
            reason,
            evidence,
            submitted_at: now,
            status: AppealStatus::Pending,
            reviewed_by: None,
            reviewed_at: None,
            decision_reason: None,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Appeal(new_appeal_id), &appeal);

        // Update player profile
        let mut profile = Self::get_or_create_profile(&env, &player);
        profile.appeal_count += 1;
        profile.status = 5;
        Self::update_profile(&env, &player, &profile);

        env.events().publish(
            (symbol_short!("appeal"), player),
            (new_appeal_id, penalty_id),
        );

        Ok(new_appeal_id)
    }

    pub fn review_appeal(
        env: Env,
        appeal_id: u32,
        approved: bool,
        decision_reason: Symbol,
    ) -> Result<(), AntiBotError> {
        Self::require_admin(&env)?;

        let now = env.ledger().timestamp();
        let reviewer = Self::require_admin(&env)?;

        let mut appeal: Appeal = env
            .storage()
            .persistent()
            .get(&DataKey::Appeal(appeal_id))
            .ok_or(AntiBotError::AppealNotFound)?;

        if appeal.status != AppealStatus::Pending {
            return Err(AntiBotError::AppealNotFound);
        }

        appeal.status = if approved {
            AppealStatus::Approved
        } else {
            AppealStatus::Rejected
        };
        appeal.reviewed_by = Some(reviewer);
        appeal.reviewed_at = Some(now);
        appeal.decision_reason = Some(decision_reason.clone());

        env.storage()
            .persistent()
            .set(&DataKey::Appeal(appeal_id), &appeal);

        // Update player status
        let player = appeal.player.clone();
        let mut profile = Self::get_or_create_profile(&env, &player);

        if approved {
            // Remove the penalty
            let _ = Self::remove_penalty(env.clone(), appeal.penalty_id);
            profile.status = 1;
            
            // Restore some trust score
            profile.trust_score = (profile.trust_score + 100).min(1000);
        } else {
            // Appeal rejected, restore penalized status if needed
            if Self::get_active_penalties(env.clone(), player.clone()) > 0 {
                profile.status = 4;
            } else {
                profile.status = 1;
            }
        }

        Self::update_profile(&env, &player, &profile);

        env.events().publish(
            (symbol_short!("app_res"), appeal_id),
            (approved, decision_reason.clone()),
        );

        Ok(())
    }

    pub fn get_appeal(env: Env, appeal_id: u32) -> Option<Appeal> {
        env.storage()
            .persistent()
            .get(&DataKey::Appeal(appeal_id))
    }

    // ========================================================================
    // WHITELIST/BLACKLIST MANAGEMENT
    // ========================================================================

    pub fn whitelist_player(env: Env, player: Address) -> Result<(), AntiBotError> {
        Self::require_admin(&env)?;

        env.storage()
            .persistent()
            .set(&DataKey::Whitelisted(player.clone()), &true);

        let mut profile = Self::get_or_create_profile(&env, &player);
        profile.status = 6;
        profile.trust_score = 1000;
        Self::update_profile(&env, &player, &profile);

        env.events().publish(
            (symbol_short!("whitelist"), player),
            (),
        );

        Ok(())
    }

    pub fn remove_whitelist(env: Env, player: Address) -> Result<(), AntiBotError> {
        Self::require_admin(&env)?;

        env.storage()
            .persistent()
            .remove(&DataKey::Whitelisted(player.clone()));

        let mut profile = Self::get_or_create_profile(&env, &player);
        profile.status = 1;
        profile.trust_score = 800;
        Self::update_profile(&env, &player, &profile);

        env.events().publish(
            (symbol_short!("unwhite"), player),
            (),
        );

        Ok(())
    }

    pub fn blacklist_player(
        env: Env,
        player: Address,
        reason: Symbol,
    ) -> Result<(), AntiBotError> {
        Self::require_admin(&env)?;

        env.storage()
            .persistent()
            .set(&DataKey::Blacklisted(player.clone()), &true);

        let mut profile = Self::get_or_create_profile(&env, &player);
        profile.status = 7;
        profile.trust_score = 0;
        Self::update_profile(&env, &player, &profile);

        // Also apply permanent penalty
        let reason_for_event = reason.clone();
        let _ = Self::apply_penalty(
            env.clone(),
            player.clone(),
            PenaltyType::PermanentBan,
            reason,
            10,
        )?;

        env.events().publish(
            (symbol_short!("blacklist"), player),
            (reason_for_event,),
        );

        Ok(())
    }

    pub fn remove_blacklist(env: Env, player: Address) -> Result<(), AntiBotError> {
        Self::require_admin(&env)?;

        env.storage()
            .persistent()
            .remove(&DataKey::Blacklisted(player.clone()));

        let mut profile = Self::get_or_create_profile(&env, &player);
        profile.status = 1;
        profile.trust_score = 300; // Low starting score
        Self::update_profile(&env, &player, &profile);

        env.events().publish(
            (symbol_short!("unblack"), player),
            (),
        );

        Ok(())
    }

    pub fn is_whitelisted(env: Env, player: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Whitelisted(player))
            .unwrap_or(false)
    }

    pub fn is_blacklisted(env: Env, player: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Blacklisted(player))
            .unwrap_or(false)
    }

    // ========================================================================
    // COMPREHENSIVE VERIFICATION
    // ========================================================================

    pub fn verify_player(
        env: Env,
        player: Address,
        puzzle_id: u32,
        solve_time_ms: u64,
    ) -> Result<VerificationResult, AntiBotError> {
        // Check basic status
        if Self::is_whitelisted(env.clone(), player.clone()) {
            return Ok(VerificationResult {
                allowed: true,
                required_action: symbol_short!("none"),
                trust_score: 1000,
                bot_probability: 0,
            });
        }

        if Self::is_blacklisted(env.clone(), player.clone()) {
            return Err(AntiBotError::BotDetected);
        }

        // Check for active penalties
        Self::check_penalty_status(env.clone(), player.clone())?;

        // Check rate limit
        Self::check_rate_limit(env.clone(), player.clone())?;

        // Validate submission time window
        let time_valid = Self::validate_submission_time(
            env.clone(),
            player.clone(),
            puzzle_id,
            solve_time_ms,
        )?;

        if !time_valid {
            return Ok(VerificationResult {
                allowed: false,
                required_action: symbol_short!("retry"),
                trust_score: Self::get_trust_score(env.clone(), player.clone()),
                bot_probability: 30,
            });
        }

        // Get player analysis
        let analysis = Self::analyze_player(env.clone(), player.clone())?;
        
        let mut allowed = true;
        let mut required_action = symbol_short!("none");

        // Determine action based on analysis
        match analysis.recommendation {
            rec if rec == symbol_short!("block") => {
                allowed = false;
                required_action = symbol_short!("blocked");
                
                // Apply penalty for high bot probability
                if analysis.bot_probability > 85 {
                    let _ = Self::apply_penalty(
                        env.clone(),
                        player.clone(),
                        PenaltyType::TemporaryBan,
                        symbol_short!("bot_det"),
                        7,
                    );
                }
            }
            rec if rec == symbol_short!("verify") => {
                // Require CAPTCHA verification
                let profile = Self::get_or_create_profile(&env, &player);
                if profile.status != 1 {
                    allowed = false;
                    required_action = symbol_short!("captcha");
                }
            }
            _ => {}
        }

        // Check trust score
        if analysis.trust_score < 200 {
            allowed = false;
            required_action = symbol_short!("verify");
        }

        Ok(VerificationResult {
            allowed,
            required_action,
            trust_score: analysis.trust_score,
            bot_probability: analysis.bot_probability,
        })
    }

    // ========================================================================
    // ADMIN FUNCTIONS
    // ========================================================================

    pub fn update_config(env: Env, new_config: Config) -> Result<(), AntiBotError> {
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::Config, &new_config);
        Ok(())
    }

    pub fn get_config(env: Env) -> Result<Config, AntiBotError> {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(AntiBotError::NotInitialized)
    }

    pub fn add_verifier(env: Env, verifier: Address) -> Result<(), AntiBotError> {
        Self::require_admin(&env)?;

        let mut config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(AntiBotError::NotInitialized)?;
        
        config.verifiers.push_back(verifier);
        env.storage().instance().set(&DataKey::Config, &config);

        Ok(())
    }

    pub fn remove_verifier(env: Env, verifier: Address) -> Result<(), AntiBotError> {
        Self::require_admin(&env)?;

        let mut config: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(AntiBotError::NotInitialized)?;
        
        let index = config.verifiers.iter().position(|v| v == verifier);
        if let Some(idx) = index {
            let _ = config.verifiers.remove(idx.try_into().unwrap());
        }
        
        env.storage().instance().set(&DataKey::Config, &config);
        Ok(())
    }
}

// ============================================================================
// ADDITIONAL TYPES
// ============================================================================

#[contracttype]
#[derive(Clone, Debug)]
pub struct PlayerAnalysis {
    pub player: Address,
    pub trust_score: u32,
    pub bot_probability: u32,
    pub risk_factors: Vec<Symbol>,
    pub recommendation: Symbol,
    pub status: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct VerificationResult {
    pub allowed: bool,
    pub required_action: Symbol,
    pub trust_score: u32,
    pub bot_probability: u32,
}

// ============================================================================
// TEST MODULE
// ============================================================================

#[cfg(test)]
mod test;
