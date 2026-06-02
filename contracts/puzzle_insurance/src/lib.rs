#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Symbol, Vec};

/// Default cooldown between claims per player per puzzle (in seconds).
/// Configurable by admin via `set_claim_cooldown`.
const DEFAULT_CLAIM_COOLDOWN: u64 = 3_600; // 1 hour

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsurancePolicy {
    pub holder: Address,
    pub premium_paid: i128,
    pub coverage_percent: u32,
    pub attempts_covered: u32,
    pub attempts_used: u32,
    pub expires_at: u64,
    pub active: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsuranceConfig {
    pub admin: Address,
    pub payment_token: Address,
    pub base_rate: i128,
    pub max_coverage_percent: u32,
    /// Minimum seconds that must elapse between two claims by the same
    /// player for the same puzzle (issue #244).
    pub claim_cooldown: u64,
}

#[contracttype]
pub enum DataKey {
    Config,
    Policy(u64),
    PolicyCounter,
    UserPolicies(Address),
    /// Tracks the timestamp of the last claim filed by a given holder for a
    /// given policy id.  Key: (policy_id, holder) → u64 timestamp.
    /// (issue #244)
    LastClaim(u64, Address),
}

#[contract]
pub struct PuzzleInsuranceContract;

#[contractimpl]
impl PuzzleInsuranceContract {
    pub fn initialize(env: Env, admin: Address, payment_token: Address, base_rate: i128) {
        admin.require_auth();

        if env.storage().persistent().has(&DataKey::Config) {
            panic!("Already initialized");
        }

        if base_rate <= 0 {
            panic!("Base rate must be positive");
        }

        let config = InsuranceConfig {
            admin: admin.clone(),
            payment_token,
            base_rate,
            max_coverage_percent: 8000,
            claim_cooldown: DEFAULT_CLAIM_COOLDOWN,
        };

        env.storage().persistent().set(&DataKey::Config, &config);
        env.storage()
            .persistent()
            .set(&DataKey::PolicyCounter, &0u64);
    }

    pub fn purchase_policy(
        env: Env,
        holder: Address,
        attempts: u32,
        duration: u64,
        coverage_percent: u32,
    ) -> u64 {
        holder.require_auth();

        let config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();

        if coverage_percent > config.max_coverage_percent {
            panic!("Coverage percent exceeds maximum");
        }

        if attempts == 0 || attempts > 100 {
            panic!("Invalid attempts count");
        }

        if duration == 0 || duration > 365 * 24 * 60 * 60 {
            panic!("Invalid duration");
        }

        let premium = (attempts as i128) * config.base_rate * (coverage_percent as i128) / 10000;

        if premium <= 0 {
            panic!("Premium must be positive");
        }

        let token_client = token::Client::new(&env, &config.payment_token);
        token_client.transfer(&holder, &env.current_contract_address(), &premium);

        let policy_id: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::PolicyCounter)
            .unwrap_or(0);
        let new_policy_id = policy_id + 1;
        env.storage()
            .persistent()
            .set(&DataKey::PolicyCounter, &new_policy_id);

        let current_time = env.ledger().timestamp();

        let policy = InsurancePolicy {
            holder: holder.clone(),
            premium_paid: premium,
            coverage_percent,
            attempts_covered: attempts,
            attempts_used: 0,
            expires_at: current_time + duration,
            active: true,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Policy(new_policy_id), &policy);

        let mut user_policies: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::UserPolicies(holder.clone()))
            .unwrap_or(Vec::new(&env));

        user_policies.push_back(new_policy_id);
        env.storage()
            .persistent()
            .set(&DataKey::UserPolicies(holder.clone()), &user_policies);

        env.events().publish(
            (Symbol::new(&env, "policy_purchased"), new_policy_id),
            (holder, attempts, coverage_percent, current_time + duration),
        );

        new_policy_id
    }

    /// File a claim against a policy.
    ///
    /// Enforces a per-player per-policy cooldown between successive claims
    /// (issue #244).  The cooldown duration is configurable by the admin via
    /// `set_claim_cooldown`.
    pub fn file_claim(env: Env, policy_id: u64, loss_amount: i128) -> i128 {
        let mut policy: InsurancePolicy = env
            .storage()
            .persistent()
            .get(&DataKey::Policy(policy_id))
            .expect("Policy not found");

        if !policy.active {
            panic!("Policy is not active");
        }

        let current_time = env.ledger().timestamp();
        if current_time > policy.expires_at {
            panic!("Policy has expired");
        }

        if policy.attempts_used >= policy.attempts_covered {
            panic!("No attempts remaining");
        }

        if loss_amount <= 0 {
            panic!("Loss amount must be positive");
        }

        // ── Cooldown enforcement (issue #244) ──────────────────────────────
        let config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let cooldown_key = DataKey::LastClaim(policy_id, policy.holder.clone());

        if let Some(last_claim_ts) = env
            .storage()
            .persistent()
            .get::<DataKey, u64>(&cooldown_key)
        {
            let elapsed = current_time.saturating_sub(last_claim_ts);
            if elapsed < config.claim_cooldown {
                panic!("Claim submitted within cooldown period");
            }
        }

        // Record this claim timestamp.
        env.storage().persistent().set(&cooldown_key, &current_time);
        // ──────────────────────────────────────────────────────────────────

        let payout = loss_amount * (policy.coverage_percent as i128) / 10000;

        if payout <= 0 {
            panic!("Payout must be positive");
        }

        policy.attempts_used += 1;

        if policy.attempts_used >= policy.attempts_covered {
            policy.active = false;

            env.events().publish(
                (Symbol::new(&env, "policy_expired"), policy_id),
                policy.holder.clone(),
            );
        }

        env.storage()
            .persistent()
            .set(&DataKey::Policy(policy_id), &policy);

        let token_client = token::Client::new(&env, &config.payment_token);
        token_client.transfer(&env.current_contract_address(), &policy.holder, &payout);

        env.events().publish(
            (Symbol::new(&env, "claim_paid"), policy_id),
            (policy.holder, payout),
        );

        payout
    }

    pub fn get_policy(env: Env, policy_id: u64) -> Option<InsurancePolicy> {
        let mut policy: Option<InsurancePolicy> =
            env.storage().persistent().get(&DataKey::Policy(policy_id));

        if let Some(ref mut p) = policy {
            if p.active && env.ledger().timestamp() > p.expires_at {
                p.active = false;
                env.storage()
                    .persistent()
                    .set(&DataKey::Policy(policy_id), p);
            }
        }

        policy
    }

    pub fn get_user_policies(env: Env, holder: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::UserPolicies(holder))
            .unwrap_or(Vec::new(&env))
    }

    pub fn expire_policy(env: Env, policy_id: u64) {
        let mut policy: InsurancePolicy = env
            .storage()
            .persistent()
            .get(&DataKey::Policy(policy_id))
            .expect("Policy not found");

        if !policy.active {
            panic!("Policy already inactive");
        }

        policy.active = false;
        env.storage()
            .persistent()
            .set(&DataKey::Policy(policy_id), &policy);

        env.events().publish(
            (Symbol::new(&env, "policy_expired"), policy_id),
            policy.holder,
        );
    }

    pub fn get_config(env: Env) -> InsuranceConfig {
        env.storage().persistent().get(&DataKey::Config).unwrap()
    }

    // ── Admin functions ────────────────────────────────────────────────────

    pub fn set_base_rate(env: Env, admin: Address, new_rate: i128) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        if new_rate <= 0 {
            panic!("Base rate must be positive");
        }

        let mut config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        config.base_rate = new_rate;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    pub fn set_max_coverage_percent(env: Env, admin: Address, new_max: u32) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        if new_max > 10000 {
            panic!("Invalid max coverage percent");
        }

        let mut config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        config.max_coverage_percent = new_max;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    /// Update the cooldown duration between claims (issue #244).
    /// Only the admin may call this.
    pub fn set_claim_cooldown(env: Env, admin: Address, cooldown_secs: u64) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        let mut config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        config.claim_cooldown = cooldown_secs;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    /// Returns the timestamp of the last claim filed for a given policy by its
    /// holder, or `None` if no claim has been filed yet.
    pub fn get_last_claim_time(env: Env, policy_id: u64, holder: Address) -> Option<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::LastClaim(policy_id, holder))
    }

    // ── Internal helpers ───────────────────────────────────────────────────

    fn require_admin(env: &Env, caller: &Address) {
        let config: InsuranceConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        if config.admin != *caller {
            panic!("Not admin");
        }
    }
}

#[cfg(test)]
mod test;
