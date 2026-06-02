#![cfg(test)]

use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, String, Symbol};

use crate::{
    DataKey, InsuranceConfig, InsurancePolicy, PuzzleInsuranceContract, DEFAULT_CLAIM_COOLDOWN,
};

pub struct PuzzleInsuranceContractClient<'a> {
    pub contract_id: soroban_sdk::contractclient::ContractID<'a>,
    pub env: &'a Env,
}

impl<'a> PuzzleInsuranceContractClient<'a> {
    pub fn new(env: &'a Env, contract_id: &soroban_sdk::contractclient::ContractID) -> Self {
        Self {
            contract_id: contract_id.clone(),
            env,
        }
    }

    pub fn initialize(&self, admin: &Address, payment_token: &Address, base_rate: &i128) {
        self.env.invoke_contract(
            &self.contract_id,
            &Symbol::new(self.env, "initialize"),
            soroban_sdk::vec![
                self.env,
                admin.to_val(),
                payment_token.to_val(),
                base_rate.to_val()
            ],
        );
    }

    pub fn purchase_policy(
        &self,
        holder: &Address,
        attempts: &u32,
        duration: &u64,
        coverage_percent: &u32,
    ) -> u64 {
        self.env
            .invoke_contract(
                &self.contract_id,
                &Symbol::new(self.env, "purchase_policy"),
                soroban_sdk::vec![
                    self.env,
                    holder.to_val(),
                    attempts.to_val(),
                    duration.to_val(),
                    coverage_percent.to_val()
                ],
            )
            .try_into_val(self.env)
            .unwrap()
    }

    pub fn file_claim(&self, policy_id: &u64, loss_amount: &i128) -> i128 {
        self.env
            .invoke_contract(
                &self.contract_id,
                &Symbol::new(self.env, "file_claim"),
                soroban_sdk::vec![self.env, policy_id.to_val(), loss_amount.to_val()],
            )
            .try_into_val(self.env)
            .unwrap()
    }

    pub fn get_policy(&self, policy_id: &u64) -> Option<InsurancePolicy> {
        self.env.invoke_contract::<Option<InsurancePolicy>>(
            &self.contract_id,
            &Symbol::new(self.env, "get_policy"),
            soroban_sdk::vec![self.env, policy_id.to_val()],
        )
    }

    pub fn get_user_policies(&self, holder: &Address) -> soroban_sdk::Vec<u64> {
        self.env.invoke_contract::<soroban_sdk::Vec<u64>>(
            &self.contract_id,
            &Symbol::new(self.env, "get_user_policies"),
            soroban_sdk::vec![self.env, holder.to_val()],
        )
    }

    pub fn set_base_rate(&self, admin: &Address, new_rate: &i128) {
        self.env.invoke_contract(
            &self.contract_id,
            &Symbol::new(self.env, "set_base_rate"),
            soroban_sdk::vec![self.env, admin.to_val(), new_rate.to_val()],
        );
    }

    pub fn set_max_coverage_percent(&self, admin: &Address, new_max: &u32) {
        self.env.invoke_contract(
            &self.contract_id,
            &Symbol::new(self.env, "set_max_coverage_percent"),
            soroban_sdk::vec![self.env, admin.to_val(), new_max.to_val()],
        );
    }

    pub fn get_config(&self) -> InsuranceConfig {
        self.env.invoke_contract::<InsuranceConfig>(
            &self.contract_id,
            &Symbol::new(self.env, "get_config"),
            soroban_sdk::vec![self.env],
        )
    }

    pub fn expire_policy(&self, policy_id: &u64) {
        self.env.invoke_contract(
            &self.contract_id,
            &Symbol::new(self.env, "expire_policy"),
            soroban_sdk::vec![self.env, policy_id.to_val()],
        );
    }

    pub fn set_claim_cooldown(&self, admin: &Address, cooldown_secs: &u64) {
        self.env.invoke_contract(
            &self.contract_id,
            &Symbol::new(self.env, "set_claim_cooldown"),
            soroban_sdk::vec![self.env, admin.to_val(), cooldown_secs.to_val()],
        );
    }

    pub fn get_last_claim_time(&self, policy_id: &u64, holder: &Address) -> Option<u64> {
        self.env.invoke_contract::<Option<u64>>(
            &self.contract_id,
            &Symbol::new(self.env, "get_last_claim_time"),
            soroban_sdk::vec![self.env, policy_id.to_val(), holder.to_val()],
        )
    }
}

fn setup() -> (
    Env,
    Address,
    Address,
    Address,
    PuzzleInsuranceContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, PuzzleInsuranceContract);
    let client = PuzzleInsuranceContractClient::new(&env, &contract_id);

    let admin = Address::random(&env);
    let payment_token = Address::random(&env);
    let user = Address::random(&env);

    client.initialize(&admin, &payment_token, &1000i128);

    (env, admin, payment_token, user, client)
}

#[test]
fn test_initialize() {
    let (env, admin, payment_token, _user, client) = setup();

    let config = client.get_config();
    assert_eq!(config.admin, admin);
    assert_eq!(config.payment_token, payment_token);
    assert_eq!(config.base_rate, 1000i128);
    assert_eq!(config.max_coverage_percent, 8000); // 80%
}

#[test]
fn test_purchase_policy() {
    let (env, _admin, _payment_token, user, client) = setup();

    let policy_id = client.purchase_policy(&user, &5, &86400, &5000); // 50% coverage
    assert_eq!(policy_id, 1);

    let policy = client.get_policy(&policy_id).unwrap();
    assert_eq!(policy.holder, user);
    assert_eq!(policy.attempts_covered, 5);
    assert_eq!(policy.attempts_used, 0);
    assert_eq!(policy.coverage_percent, 5000);
    assert_eq!(policy.premium_paid, 2500i128); // 5 * 1000 * 5000 / 10000
    assert!(policy.active);
}

#[test]
#[should_panic(expected = "Coverage percent exceeds maximum")]
fn test_purchase_policy_exceeds_max_coverage() {
    let (env, _admin, _payment_token, user, client) = setup();

    // Try to purchase with 90% coverage (exceeds 80% max)
    client.purchase_policy(&user, &5, &86400, &9000);
}

#[test]
#[should_panic(expected = "Invalid attempts count")]
fn test_purchase_policy_invalid_attempts() {
    let (env, _admin, _payment_token, user, client) = setup();

    // Try to purchase with 0 attempts
    client.purchase_policy(&user, &0, &86400, &5000);
}

#[test]
fn test_file_claim_valid() {
    let (env, _admin, _payment_token, user, client) = setup();

    let policy_id = client.purchase_policy(&user, &3, &86400, &5000);

    // File a claim for 1000 loss (should pay 500)
    let payout = client.file_claim(&policy_id, &1000i128);
    assert_eq!(payout, 500i128); // 1000 * 5000 / 10000

    let policy = client.get_policy(&policy_id).unwrap();
    assert_eq!(policy.attempts_used, 1);
    assert!(policy.active);
}

#[test]
#[should_panic(expected = "Policy is not active")]
fn test_file_claim_inactive_policy() {
    let (env, _admin, _payment_token, user, client) = setup();

    let policy_id = client.purchase_policy(&user, &1, &86400, &5000);

    // Manually expire the policy
    client.expire_policy(&policy_id);

    // Try to file a claim
    client.file_claim(&policy_id, &1000i128);
}

#[test]
#[should_panic(expected = "No attempts remaining")]
fn test_file_claim_exhausted_attempts() {
    let (env, admin, _payment_token, user, client) = setup();

    // Disable cooldown so we can exhaust attempts without time-advancing.
    client.set_claim_cooldown(&admin, &0u64);

    let policy_id = client.purchase_policy(&user, &2, &86400, &5000);

    // Use all attempts
    client.file_claim(&policy_id, &1000i128);
    client.file_claim(&policy_id, &1000i128);

    // Try to file another claim
    client.file_claim(&policy_id, &1000i128);
}

#[test]
fn test_policy_expires_by_time() {
    let (env, _admin, _payment_token, user, client) = setup();

    let policy_id = client.purchase_policy(&user, &3, &86400, &5000);

    // Fast forward time past expiration
    env.ledger().set_timestamp(env.ledger().timestamp() + 86401);

    // Policy should be marked as expired
    let policy = client.get_policy(&policy_id).unwrap();
    assert!(!policy.active);
}

#[test]
fn test_policy_expires_by_attempts() {
    let (env, admin, _payment_token, user, client) = setup();

    // Disable cooldown so we can exhaust attempts without time-advancing.
    client.set_claim_cooldown(&admin, &0u64);

    let policy_id = client.purchase_policy(&user, &2, &86400, &5000);

    // Use all attempts
    client.file_claim(&policy_id, &1000i128);
    client.file_claim(&policy_id, &1000i128);

    // Policy should be inactive
    let policy = client.get_policy(&policy_id).unwrap();
    assert!(!policy.active);
}

#[test]
fn test_set_base_rate() {
    let (env, admin, _payment_token, _user, client) = setup();

    client.set_base_rate(&admin, &2000i128);

    let config = client.get_config();
    assert_eq!(config.base_rate, 2000i128);
}

#[test]
#[should_panic(expected = "Not admin")]
fn test_set_base_rate_unauthorized() {
    let (env, _admin, _payment_token, user, client) = setup();

    client.set_base_rate(&user, &2000i128);
}

#[test]
fn test_set_max_coverage_percent() {
    let (env, admin, _payment_token, _user, client) = setup();

    client.set_max_coverage_percent(&admin, &9000); // 90%

    let config = client.get_config();
    assert_eq!(config.max_coverage_percent, 9000);
}

#[test]
#[should_panic(expected = "Invalid max coverage percent")]
fn test_set_max_coverage_percent_invalid() {
    let (env, admin, _payment_token, _user, client) = setup();

    client.set_max_coverage_percent(&admin, &11000); // 110% invalid
}

#[test]
fn test_get_user_policies() {
    let (env, _admin, _payment_token, user, client) = setup();

    let policy1 = client.purchase_policy(&user, &3, &86400, &5000);
    let policy2 = client.purchase_policy(&user, &2, &86400, &7000);

    let user_policies = client.get_user_policies(&user);
    assert_eq!(user_policies.len(), 2);
    assert!(user_policies.contains(&policy1));
    assert!(user_policies.contains(&policy2));
}

#[test]
fn test_premium_calculation() {
    let (env, _admin, _payment_token, user, client) = setup();

    // Test different coverage percentages
    let policy1 = client.purchase_policy(&user, &1, &86400, &2500); // 25%
    assert_eq!(policy1, 1);
    let policy1_data = client.get_policy(&policy1).unwrap();
    assert_eq!(policy1_data.premium_paid, 250i128); // 1 * 1000 * 2500 / 10000

    let policy2 = client.purchase_policy(&user, &2, &86400, &7500); // 75%
    assert_eq!(policy2, 2);
    let policy2_data = client.get_policy(&policy2).unwrap();
    assert_eq!(policy2_data.premium_paid, 1500i128); // 2 * 1000 * 7500 / 10000
}

#[test]
#[should_panic(expected = "Loss amount must be positive")]
fn test_file_claim_invalid_amount() {
    let (env, _admin, _payment_token, user, client) = setup();

    let policy_id = client.purchase_policy(&user, &3, &86400, &5000);

    // Try to file claim with negative amount
    client.file_claim(&policy_id, &-100i128);
}

#[test]
#[should_panic(expected = "Policy not found")]
fn test_file_claim_nonexistent_policy() {
    let (env, _admin, _payment_token, _user, client) = setup();

    // Try to file claim for non-existent policy
    client.file_claim(&999, &1000i128);
}

#[test]
fn test_manual_policy_expiry() {
    let (env, _admin, _payment_token, user, client) = setup();

    let policy_id = client.purchase_policy(&user, &3, &86400, &5000);

    // Manually expire policy
    client.expire_policy(&policy_id);

    let policy = client.get_policy(&policy_id).unwrap();
    assert!(!policy.active);
}

#[test]
#[should_panic(expected = "Policy already inactive")]
fn test_expire_already_inactive_policy() {
    let (env, _admin, _payment_token, user, client) = setup();

    let policy_id = client.purchase_policy(&user, &1, &86400, &5000);

    // Expire policy twice
    client.expire_policy(&policy_id);
    client.expire_policy(&policy_id);
}

#[test]
fn test_payout_calculation() {
    let (env, admin, _payment_token, user, client) = setup();

    // Disable cooldown so we can file two claims back-to-back in this test.
    client.set_claim_cooldown(&admin, &0u64);

    let policy_id = client.purchase_policy(&user, &3, &86400, &3000); // 30% coverage

    // File claim for different amounts
    let payout1 = client.file_claim(&policy_id, &10000i128);
    assert_eq!(payout1, 3000i128); // 10000 * 3000 / 10000

    let payout2 = client.file_claim(&policy_id, &5000i128);
    assert_eq!(payout2, 1500i128); // 5000 * 3000 / 10000
}

#[test]
#[should_panic(expected = "Base rate must be positive")]
fn test_set_base_rate_invalid() {
    let (env, admin, _payment_token, _user, client) = setup();

    client.set_base_rate(&admin, &0i128);
}

#[test]
fn test_maximum_duration() {
    let (env, _admin, _payment_token, user, client) = setup();

    // Test maximum duration (1 year in seconds)
    let max_duration = 365 * 24 * 60 * 60;
    let policy_id = client.purchase_policy(&user, &1, &max_duration, &5000);

    let policy = client.get_policy(&policy_id).unwrap();
    assert!(policy.active);
    assert_eq!(policy.expires_at, env.ledger().timestamp() + max_duration);
}

#[test]
#[should_panic(expected = "Invalid duration")]
fn test_duration_too_long() {
    let (env, _admin, _payment_token, user, client) = setup();

    // Try duration longer than 1 year
    let too_long = 366 * 24 * 60 * 60;
    client.purchase_policy(&user, &1, &too_long, &5000);
}

// ──────────────────────────────────────────────────────────
// Issue #244 — Insurance claim cooldown enforcement
// ──────────────────────────────────────────────────────────

#[test]
fn test_default_cooldown_set_on_initialize() {
    let (env, _admin, _payment_token, _user, client) = setup();
    let config = client.get_config();
    assert_eq!(config.claim_cooldown, DEFAULT_CLAIM_COOLDOWN);
}

#[test]
fn test_first_claim_succeeds_no_prior_history() {
    let (env, _admin, _payment_token, user, client) = setup();

    let policy_id = client.purchase_policy(&user, &3, &(86400 * 7), &5000);

    // First claim should always succeed regardless of cooldown.
    let payout = client.file_claim(&policy_id, &1000i128);
    assert_eq!(payout, 500i128); // 1000 * 5000 / 10000

    // Last claim time should now be recorded.
    let last = client.get_last_claim_time(&policy_id, &user);
    assert!(last.is_some());
}

#[test]
#[should_panic(expected = "Claim submitted within cooldown period")]
fn test_claim_within_cooldown_reverts() {
    let (env, _admin, _payment_token, user, client) = setup();

    // Policy with 3 attempts and 7-day duration.
    let policy_id = client.purchase_policy(&user, &3, &(86400 * 7), &5000);

    // First claim at t=0.
    client.file_claim(&policy_id, &500i128);

    // Advance time by less than the default cooldown (1 hour = 3600s).
    env.ledger().set_timestamp(env.ledger().timestamp() + 1800); // 30 minutes

    // Second claim within cooldown window — must revert.
    client.file_claim(&policy_id, &500i128);
}

#[test]
fn test_claim_after_cooldown_succeeds() {
    let (env, _admin, _payment_token, user, client) = setup();

    let policy_id = client.purchase_policy(&user, &3, &(86400 * 7), &5000);

    let t0 = env.ledger().timestamp();

    // First claim.
    client.file_claim(&policy_id, &500i128);

    // Advance time past the default cooldown (3600s).
    env.ledger().set_timestamp(t0 + DEFAULT_CLAIM_COOLDOWN + 1);

    // Second claim after cooldown — must succeed.
    let payout = client.file_claim(&policy_id, &500i128);
    assert_eq!(payout, 250i128);
}

#[test]
fn test_admin_can_update_cooldown_duration() {
    let (env, admin, _payment_token, _user, client) = setup();

    // Change cooldown to 2 hours.
    let new_cooldown: u64 = 7200;
    client.set_claim_cooldown(&admin, &new_cooldown);

    let config = client.get_config();
    assert_eq!(config.claim_cooldown, new_cooldown);
}

#[test]
fn test_claim_respects_updated_cooldown() {
    let (env, admin, _payment_token, user, client) = setup();

    // Set a short cooldown of 60 seconds for this test.
    client.set_claim_cooldown(&admin, &60u64);

    let policy_id = client.purchase_policy(&user, &3, &(86400 * 7), &5000);
    let t0 = env.ledger().timestamp();

    // First claim.
    client.file_claim(&policy_id, &500i128);

    // Advance 61 seconds — past the 60s cooldown.
    env.ledger().set_timestamp(t0 + 61);

    // Should succeed with the shorter cooldown.
    let payout = client.file_claim(&policy_id, &500i128);
    assert_eq!(payout, 250i128);
}

#[test]
#[should_panic(expected = "Claim submitted within cooldown period")]
fn test_claim_fails_with_updated_longer_cooldown() {
    let (env, admin, _payment_token, user, client) = setup();

    // Set a long cooldown of 2 days.
    client.set_claim_cooldown(&admin, &(86400 * 2));

    let policy_id = client.purchase_policy(&user, &3, &(86400 * 7), &5000);
    let t0 = env.ledger().timestamp();

    // First claim.
    client.file_claim(&policy_id, &500i128);

    // Advance only 1 day — still within the 2-day cooldown.
    env.ledger().set_timestamp(t0 + 86400);

    // Must revert.
    client.file_claim(&policy_id, &500i128);
}

#[test]
fn test_cooldown_is_per_policy_independent() {
    let (env, _admin, _payment_token, user, client) = setup();

    // Two separate policies for the same user.
    let policy_a = client.purchase_policy(&user, &3, &(86400 * 7), &5000);
    let policy_b = client.purchase_policy(&user, &3, &(86400 * 7), &5000);

    let t0 = env.ledger().timestamp();

    // Claim on policy A.
    client.file_claim(&policy_a, &500i128);

    // Claiming on policy B immediately should succeed — cooldowns are independent.
    let payout_b = client.file_claim(&policy_b, &500i128);
    assert_eq!(payout_b, 250i128);

    // Advance past cooldown and claim on A again.
    env.ledger().set_timestamp(t0 + DEFAULT_CLAIM_COOLDOWN + 1);
    let payout_a2 = client.file_claim(&policy_a, &500i128);
    assert_eq!(payout_a2, 250i128);
}

#[test]
fn test_get_last_claim_time_returns_none_before_any_claim() {
    let (env, _admin, _payment_token, user, client) = setup();

    let policy_id = client.purchase_policy(&user, &3, &(86400 * 7), &5000);

    let last = client.get_last_claim_time(&policy_id, &user);
    assert!(last.is_none());
}

#[test]
fn test_get_last_claim_time_updates_after_each_claim() {
    let (env, _admin, _payment_token, user, client) = setup();

    let policy_id = client.purchase_policy(&user, &3, &(86400 * 7), &5000);
    let t0 = env.ledger().timestamp();

    client.file_claim(&policy_id, &500i128);
    let last1 = client.get_last_claim_time(&policy_id, &user).unwrap();
    assert_eq!(last1, t0);

    // Advance past cooldown and claim again.
    let t1 = t0 + DEFAULT_CLAIM_COOLDOWN + 1;
    env.ledger().set_timestamp(t1);
    client.file_claim(&policy_id, &500i128);

    let last2 = client.get_last_claim_time(&policy_id, &user).unwrap();
    assert_eq!(last2, t1);
    assert!(last2 > last1);
}

#[test]
fn test_zero_cooldown_allows_immediate_reclaim() {
    let (env, admin, _payment_token, user, client) = setup();

    // Admin sets cooldown to 0 — no restriction.
    client.set_claim_cooldown(&admin, &0u64);

    let policy_id = client.purchase_policy(&user, &3, &(86400 * 7), &5000);

    // Two claims at the same timestamp should both succeed.
    client.file_claim(&policy_id, &500i128);
    let payout = client.file_claim(&policy_id, &500i128);
    assert_eq!(payout, 250i128);
}
