#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Vec};

//
// ──────────────────────────────────────────────────────────
// TIME CONSTANTS
// ──────────────────────────────────────────────────────────
//

#[cfg(not(test))]
const MONTH_IN_SECONDS: u64 = 2_592_000; // 30 days
#[cfg(test)]
const MONTH_IN_SECONDS: u64 = 10;

#[cfg(not(test))]
const GRACE_PERIOD_SECONDS: u64 = 259_200; // 3 days
#[cfg(test)]
const GRACE_PERIOD_SECONDS: u64 = 3;

//
// ──────────────────────────────────────────────────────────
// SUBSCRIPTION TIERS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptionTier {
    Basic = 1,
    Premium = 2,
    Enterprise = 3,
}

//
// ──────────────────────────────────────────────────────────
// DATA KEYS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
pub enum DataKey {
    Config,
    Subscription(Address),
    GroupSubscription(u64),    // group_id -> GroupSubscription
    GroupMembers(u64),          // group_id -> Vec<Address>
    NextGroupId,
    UserGroup(Address),         // user -> group_id
    TotalSubscribers,
    TierPrice(SubscriptionTier),
}

//
// ──────────────────────────────────────────────────────────
// STRUCTS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
#[derive(Clone, Debug)]
pub struct Config {
    pub admin: Address,
    pub payment_token: Address,
    pub basic_price: i128,
    pub premium_price: i128,
    pub enterprise_price: i128,
    pub paused: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Subscription {
    pub tier: SubscriptionTier,
    pub start_time: u64,
    pub expiry_time: u64,
    pub auto_renew: bool,
    pub is_active: bool,
    pub total_renewals: u32,
    pub benefits_used: u32,
    pub is_gifted: bool,
    pub gifted_by: Option<Address>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct GroupSubscription {
    pub owner: Address,
    pub tier: SubscriptionTier,
    pub start_time: u64,
    pub expiry_time: u64,
    pub auto_renew: bool,
    pub is_active: bool,
    pub max_members: u32,
    pub total_renewals: u32,
}

//
// ──────────────────────────────────────────────────────────
// CONTRACT
// ──────────────────────────────────────────────────────────
//

#[contract]
pub struct SubscriptionContract;

#[contractimpl]
impl SubscriptionContract {
    // ───────────── INITIALIZATION ─────────────

    /// Initialize the subscription contract
    pub fn initialize(
        env: Env,
        admin: Address,
        payment_token: Address,
        basic_price: i128,
        premium_price: i128,
        enterprise_price: i128,
    ) {
        admin.require_auth();

        if env.storage().persistent().has(&DataKey::Config) {
            panic!("Already initialized");
        }

        let config = Config {
            admin,
            payment_token,
            basic_price,
            premium_price,
            enterprise_price,
            paused: false,
        };

        env.storage().persistent().set(&DataKey::Config, &config);
        env.storage().persistent().set(&DataKey::TotalSubscribers, &0u64);
        env.storage().persistent().set(&DataKey::NextGroupId, &1u64);
    }

    // ───────────── ADMIN FUNCTIONS ─────────────

    /// Update tier pricing (admin only)
    pub fn update_pricing(
        env: Env,
        admin: Address,
        basic_price: i128,
        premium_price: i128,
        enterprise_price: i128,
    ) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut config: Config = env.storage().persistent().get(&DataKey::Config).unwrap();
        config.basic_price = basic_price;
        config.premium_price = premium_price;
        config.enterprise_price = enterprise_price;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    /// Pause/unpause the contract (admin only)
    pub fn set_paused(env: Env, admin: Address, paused: bool) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut config: Config = env.storage().persistent().get(&DataKey::Config).unwrap();
        config.paused = paused;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    /// Withdraw accumulated payments (admin only)
    pub fn withdraw(env: Env, admin: Address, amount: i128) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let config: Config = env.storage().persistent().get(&DataKey::Config).unwrap();
        let token_client = token::Client::new(&env, &config.payment_token);
        token_client.transfer(&env.current_contract_address(), &admin, &amount);
    }

    // ───────────── SUBSCRIPTION PURCHASE ─────────────

    /// Purchase a new subscription
    pub fn purchase_subscription(
        env: Env,
        user: Address,
        tier: SubscriptionTier,
        auto_renew: bool,
    ) {
        user.require_auth();
        Self::assert_not_paused(&env);

        // Check if user already has an active subscription
        if Self::has_active_subscription(env.clone(), user.clone()) {
            panic!("Already has active subscription");
        }

        let config: Config = env.storage().persistent().get(&DataKey::Config).unwrap();
        let price = Self::get_tier_price(&tier, &config);
        
        // Transfer payment
        let token_client = token::Client::new(&env, &config.payment_token);
        token_client.transfer(&user, &env.current_contract_address(), &price);

        // Create subscription
        let current_time = env.ledger().timestamp();
        let subscription = Subscription {
            tier,
            start_time: current_time,
            expiry_time: current_time + MONTH_IN_SECONDS,
            auto_renew,
            is_active: true,
            total_renewals: 0,
            benefits_used: 0,
            is_gifted: false,
            gifted_by: None,
        };

        env.storage().persistent().set(&DataKey::Subscription(user.clone()), &subscription);

        // Update total subscribers
        let total: u64 = env.storage().persistent().get(&DataKey::TotalSubscribers).unwrap_or(0);
        env.storage().persistent().set(&DataKey::TotalSubscribers, &(total + 1));
    }

    /// Auto-renew subscription (requires user authorization)
    pub fn process_renewal(env: Env, user: Address) {
        user.require_auth();
        Self::assert_not_paused(&env);

        let config: Config = env.storage().persistent().get(&DataKey::Config).unwrap();
        let mut subscription: Subscription = env.storage().persistent()
            .get(&DataKey::Subscription(user.clone()))
            .expect("No subscription found");

        if !subscription.auto_renew {
            panic!("Auto-renew not enabled");
        }

        if !subscription.is_active {
            panic!("Subscription not active");
        }

        let current_time = env.ledger().timestamp();
        
        // Check if within renewal window (expired but within grace period)
        if current_time < subscription.expiry_time {
            panic!("Not yet time to renew");
        }

        if current_time > subscription.expiry_time + GRACE_PERIOD_SECONDS {
            // Beyond grace period, deactivate subscription
            subscription.is_active = false;
            env.storage().persistent().set(&DataKey::Subscription(user.clone()), &subscription);
            panic!("Subscription expired beyond grace period");
        }

        // Process payment
        let price = Self::get_tier_price(&subscription.tier, &config);
        let token_client = token::Client::new(&env, &config.payment_token);
        token_client.transfer(&user, &env.current_contract_address(), &price);

        // Renew subscription
        subscription.expiry_time = current_time + MONTH_IN_SECONDS;
        subscription.total_renewals += 1;
        env.storage().persistent().set(&DataKey::Subscription(user.clone()), &subscription);
    }

    // ───────────── SUBSCRIPTION MANAGEMENT ─────────────

    /// Cancel subscription (stops auto-renewal, keeps benefits until expiry)
    pub fn cancel_subscription(env: Env, user: Address) {
        user.require_auth();

        let mut subscription: Subscription = env.storage().persistent()
            .get(&DataKey::Subscription(user.clone()))
            .expect("No subscription found");

        subscription.auto_renew = false;
        env.storage().persistent().set(&DataKey::Subscription(user.clone()), &subscription);
    }

    /// Toggle auto-renewal
    pub fn set_auto_renew(env: Env, user: Address, auto_renew: bool) {
        user.require_auth();

        let mut subscription: Subscription = env.storage().persistent()
            .get(&DataKey::Subscription(user.clone()))
            .expect("No subscription found");

        subscription.auto_renew = auto_renew;
        env.storage().persistent().set(&DataKey::Subscription(user.clone()), &subscription);
    }

    /// Upgrade subscription tier
    pub fn upgrade_subscription(env: Env, user: Address, new_tier: SubscriptionTier) {
        user.require_auth();
        Self::assert_not_paused(&env);

        let mut subscription: Subscription = env.storage().persistent()
            .get(&DataKey::Subscription(user.clone()))
            .expect("No subscription found");

        if !subscription.is_active {
            panic!("Subscription not active");
        }

        let old_tier_value = subscription.tier as u32;
        let new_tier_value = new_tier as u32;

        if new_tier_value <= old_tier_value {
            panic!("Can only upgrade to higher tier");
        }

        let config: Config = env.storage().persistent().get(&DataKey::Config).unwrap();
        let old_price = Self::get_tier_price(&subscription.tier, &config);
        let new_price = Self::get_tier_price(&new_tier, &config);
        let price_diff = new_price - old_price;

        // Charge the difference
        if price_diff > 0 {
            let token_client = token::Client::new(&env, &config.payment_token);
            token_client.transfer(&user, &env.current_contract_address(), &price_diff);
        }

        subscription.tier = new_tier;
        env.storage().persistent().set(&DataKey::Subscription(user.clone()), &subscription);
    }

    // ───────────── BENEFITS TRACKING ─────────────

    /// Track benefit usage
    pub fn use_benefit(env: Env, user: Address) {
        user.require_auth();

        let mut subscription: Subscription = env.storage().persistent()
            .get(&DataKey::Subscription(user.clone()))
            .expect("No subscription found");

        if !Self::is_subscription_valid(&env, &subscription) {
            panic!("Subscription not valid");
        }

        subscription.benefits_used += 1;
        env.storage().persistent().set(&DataKey::Subscription(user.clone()), &subscription);
    }

    /// Get benefit limits based on tier
    pub fn get_benefit_limit(tier: SubscriptionTier) -> u32 {
        match tier {
            SubscriptionTier::Basic => 10,
            SubscriptionTier::Premium => 50,
            SubscriptionTier::Enterprise => 999_999,
        }
    }

    // ───────────── GROUP SUBSCRIPTIONS ─────────────

    /// Create a family/group subscription
    pub fn create_group_subscription(
        env: Env,
        owner: Address,
        tier: SubscriptionTier,
        max_members: u32,
        auto_renew: bool,
    ) -> u64 {
        owner.require_auth();
        Self::assert_not_paused(&env);

        if max_members < 2 || max_members > 10 {
            panic!("Max members must be between 2 and 10");
        }

        let config: Config = env.storage().persistent().get(&DataKey::Config).unwrap();
        let base_price = Self::get_tier_price(&tier, &config);
        // Group pricing: base price * members * 0.8 (20% discount)
        let total_price = (base_price * max_members as i128 * 80) / 100;

        // Transfer payment
        let token_client = token::Client::new(&env, &config.payment_token);
        token_client.transfer(&owner, &env.current_contract_address(), &total_price);

        // Get next group ID
        let group_id: u64 = env.storage().persistent().get(&DataKey::NextGroupId).unwrap_or(1);
        env.storage().persistent().set(&DataKey::NextGroupId, &(group_id + 1));

        // Create group subscription
        let current_time = env.ledger().timestamp();
        let group_sub = GroupSubscription {
            owner: owner.clone(),
            tier,
            start_time: current_time,
            expiry_time: current_time + MONTH_IN_SECONDS,
            auto_renew,
            is_active: true,
            max_members,
            total_renewals: 0,
        };

        env.storage().persistent().set(&DataKey::GroupSubscription(group_id), &group_sub);

        // Initialize members list with owner
        let mut members = Vec::new(&env);
        members.push_back(owner.clone());
        env.storage().persistent().set(&DataKey::GroupMembers(group_id), &members);
        env.storage().persistent().set(&DataKey::UserGroup(owner.clone()), &group_id);

        group_id
    }

    /// Add member to group subscription
    pub fn add_group_member(env: Env, owner: Address, group_id: u64, member: Address) {
        owner.require_auth();

        let group_sub: GroupSubscription = env.storage().persistent()
            .get(&DataKey::GroupSubscription(group_id))
            .expect("Group not found");

        if group_sub.owner != owner {
            panic!("Not group owner");
        }

        if !group_sub.is_active {
            panic!("Group subscription not active");
        }

        let mut members: Vec<Address> = env.storage().persistent()
            .get(&DataKey::GroupMembers(group_id))
            .unwrap_or(Vec::new(&env));

        if members.len() >= group_sub.max_members {
            panic!("Group is full");
        }

        if members.contains(&member) {
            panic!("Already a member");
        }

        members.push_back(member.clone());
        env.storage().persistent().set(&DataKey::GroupMembers(group_id), &members);
        env.storage().persistent().set(&DataKey::UserGroup(member.clone()), &group_id);
    }

    /// Remove member from group subscription
    pub fn remove_group_member(env: Env, owner: Address, group_id: u64, member: Address) {
        owner.require_auth();

        let group_sub: GroupSubscription = env.storage().persistent()
            .get(&DataKey::GroupSubscription(group_id))
            .expect("Group not found");

        if group_sub.owner != owner {
            panic!("Not group owner");
        }

        if group_sub.owner == member {
            panic!("Cannot remove owner");
        }

        let members: Vec<Address> = env.storage().persistent()
            .get(&DataKey::GroupMembers(group_id))
            .unwrap_or(Vec::new(&env));

        let mut new_members = Vec::new(&env);
        let mut found = false;

        for m in members.iter() {
            if m != member {
                new_members.push_back(m);
            } else {
                found = true;
            }
        }

        if !found {
            panic!("Member not found");
        }

        env.storage().persistent().set(&DataKey::GroupMembers(group_id), &new_members);
        env.storage().persistent().remove(&DataKey::UserGroup(member));
    }

    // ───────────── SUBSCRIPTION GIFTING ─────────────

    /// Gift a subscription to another user
    pub fn gift_subscription(
        env: Env,
        gifter: Address,
        recipient: Address,
        tier: SubscriptionTier,
    ) {
        gifter.require_auth();
        Self::assert_not_paused(&env);

        if Self::has_active_subscription(env.clone(), recipient.clone()) {
            panic!("Recipient already has active subscription");
        }

        let config: Config = env.storage().persistent().get(&DataKey::Config).unwrap();
        let price = Self::get_tier_price(&tier, &config);

        // Transfer payment from gifter
        let token_client = token::Client::new(&env, &config.payment_token);
        token_client.transfer(&gifter, &env.current_contract_address(), &price);

        // Create subscription for recipient
        let current_time = env.ledger().timestamp();
        let subscription = Subscription {
            tier,
            start_time: current_time,
            expiry_time: current_time + MONTH_IN_SECONDS,
            auto_renew: false,
            is_active: true,
            total_renewals: 0,
            benefits_used: 0,
            is_gifted: true,
            gifted_by: Some(gifter),
        };

        env.storage().persistent().set(&DataKey::Subscription(recipient.clone()), &subscription);

        // Update total subscribers
        let total: u64 = env.storage().persistent().get(&DataKey::TotalSubscribers).unwrap_or(0);
        env.storage().persistent().set(&DataKey::TotalSubscribers, &(total + 1));
    }

    // ───────────── VIEW FUNCTIONS ─────────────

    /// Check if user has active subscription (including group membership)
    pub fn has_active_subscription(env: Env, user: Address) -> bool {
        // Check individual subscription
        if let Some(sub) = Self::get_subscription(env.clone(), user.clone()) {
            if Self::is_subscription_valid(&env, &sub) {
                return true;
            }
        }

        // Check group membership
        if let Some(group_id) = env.storage().persistent().get::<DataKey, u64>(&DataKey::UserGroup(user.clone())) {
            if let Some(group_sub) = env.storage().persistent().get::<DataKey, GroupSubscription>(&DataKey::GroupSubscription(group_id)) {
                if Self::is_group_subscription_valid(&env, &group_sub) {
                    return true;
                }
            }
        }

        false
    }

    /// Get user subscription details
    pub fn get_subscription(env: Env, user: Address) -> Option<Subscription> {
        env.storage().persistent().get(&DataKey::Subscription(user))
    }

    /// Get subscription tier for user
    pub fn get_user_tier(env: Env, user: Address) -> Option<SubscriptionTier> {
        // Check individual subscription
        if let Some(sub) = Self::get_subscription(env.clone(), user.clone()) {
            if Self::is_subscription_valid(&env, &sub) {
                return Some(sub.tier);
            }
        }

        // Check group membership
        if let Some(group_id) = env.storage().persistent().get::<DataKey, u64>(&DataKey::UserGroup(user.clone())) {
            if let Some(group_sub) = env.storage().persistent().get::<DataKey, GroupSubscription>(&DataKey::GroupSubscription(group_id)) {
                if Self::is_group_subscription_valid(&env, &group_sub) {
                    return Some(group_sub.tier);
                }
            }
        }

        None
    }

    /// Get group subscription details
    pub fn get_group_subscription(env: Env, group_id: u64) -> Option<GroupSubscription> {
        env.storage().persistent().get(&DataKey::GroupSubscription(group_id))
    }

    /// Get group members
    pub fn get_group_members(env: Env, group_id: u64) -> Vec<Address> {
        env.storage().persistent().get(&DataKey::GroupMembers(group_id)).unwrap_or(Vec::new(&env))
    }

    /// Get total active subscribers
    pub fn get_total_subscribers(env: Env) -> u64 {
        env.storage().persistent().get(&DataKey::TotalSubscribers).unwrap_or(0)
    }

    /// Get configuration
    pub fn get_config(env: Env) -> Config {
        env.storage().persistent().get(&DataKey::Config).unwrap()
    }

    /// Check if subscription is in grace period
    pub fn is_in_grace_period(env: Env, user: Address) -> bool {
        if let Some(sub) = Self::get_subscription(env.clone(), user) {
            let current_time = env.ledger().timestamp();
            current_time > sub.expiry_time 
                && current_time <= sub.expiry_time + GRACE_PERIOD_SECONDS
                && sub.is_active
        } else {
            false
        }
    }

    /// Get time until expiry
    pub fn get_time_until_expiry(env: Env, user: Address) -> u64 {
        if let Some(sub) = Self::get_subscription(env.clone(), user) {
            let current_time = env.ledger().timestamp();
            if current_time >= sub.expiry_time {
                0
            } else {
                sub.expiry_time - current_time
            }
        } else {
            0
        }
    }

    // ───────────── INTERNAL HELPERS ─────────────

    fn is_subscription_valid(env: &Env, subscription: &Subscription) -> bool {
        if !subscription.is_active {
            return false;
        }

        let current_time = env.ledger().timestamp();
        
        // Valid if within expiry time or within grace period
        current_time <= subscription.expiry_time + GRACE_PERIOD_SECONDS
    }

    fn is_group_subscription_valid(env: &Env, group_sub: &GroupSubscription) -> bool {
        if !group_sub.is_active {
            return false;
        }

        let current_time = env.ledger().timestamp();
        current_time <= group_sub.expiry_time + GRACE_PERIOD_SECONDS
    }

    fn get_tier_price(tier: &SubscriptionTier, config: &Config) -> i128 {
        match tier {
            SubscriptionTier::Basic => config.basic_price,
            SubscriptionTier::Premium => config.premium_price,
            SubscriptionTier::Enterprise => config.enterprise_price,
        }
    }

    fn assert_admin(env: &Env, user: &Address) {
        let config: Config = env.storage().persistent().get(&DataKey::Config).unwrap();
        if config.admin != *user {
            panic!("Admin only");
        }
    }

    fn assert_not_paused(env: &Env) {
        let config: Config = env.storage().persistent().get(&DataKey::Config).unwrap();
        if config.paused {
            panic!("Contract is paused");
        }
    }
}

//
// ──────────────────────────────────────────────────────────
// TESTS
// ──────────────────────────────────────────────────────────
//

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::token::StellarAssetClient;

    fn create_token_contract(env: &Env, admin: &Address) -> Address {
        env.register_stellar_asset_contract_v2(admin.clone()).address()
    }

    fn setup() -> (Env, Address, Address, Address, StellarAssetClient<'static>, SubscriptionContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let payment_token = create_token_contract(&env, &token_admin);

        let token_admin_client = StellarAssetClient::new(&env, &payment_token);

        let contract_id = env.register_contract(None, SubscriptionContract);
        let client = SubscriptionContractClient::new(&env, &contract_id);

        client.initialize(
            &admin,
            &payment_token,
            &1_000_000,  // Basic: 1 token
            &5_000_000,  // Premium: 5 tokens
            &20_000_000, // Enterprise: 20 tokens
        );

        let user = Address::generate(&env);
        
        // Mint tokens to user for testing
        token_admin_client.mint(&user, &100_000_000);

        (env, admin, payment_token, user, token_admin_client, client)
    }

    // ───────────── BASIC SUBSCRIPTION TESTS ─────────────

    #[test]
    fn test_initialize() {
        let (env, admin, payment_token, _user, _token_admin_client, _client) = setup();
        let contract_id = env.register_contract(None, SubscriptionContract);
        let client = SubscriptionContractClient::new(&env, &contract_id);

        client.initialize(&admin, &payment_token, &1_000_000, &5_000_000, &20_000_000);

        let config = client.get_config();
        assert_eq!(config.admin, admin);
        assert_eq!(config.basic_price, 1_000_000);
    }

    #[test]
    fn test_purchase_basic_subscription() {
        let (_env, _admin, _payment_token, user, _token_admin_client, client) = setup();

        client.purchase_subscription(&user, &SubscriptionTier::Basic, &true);

        assert!(client.has_active_subscription(&user));
        
        let sub = client.get_subscription(&user).unwrap();
        assert_eq!(sub.tier, SubscriptionTier::Basic);
        assert!(sub.is_active);
        assert!(sub.auto_renew);
        assert_eq!(sub.total_renewals, 0);
    }

    #[test]
    fn test_purchase_premium_subscription() {
        let (_env, _admin, _payment_token, user, _token_admin_client, client) = setup();

        client.purchase_subscription(&user, &SubscriptionTier::Premium, &false);

        let sub = client.get_subscription(&user).unwrap();
        assert_eq!(sub.tier, SubscriptionTier::Premium);
        assert!(!sub.auto_renew);
    }

    #[test]
    #[should_panic(expected = "Already has active subscription")]
    fn test_cannot_purchase_duplicate_subscription() {
        let (_env, _admin, _payment_token, user, _token_admin_client, client) = setup();

        client.purchase_subscription(&user, &SubscriptionTier::Basic, &true);
        client.purchase_subscription(&user, &SubscriptionTier::Premium, &false);
    }

    // ───────────── SUBSCRIPTION VALIDITY TESTS ─────────────

    #[test]
    fn test_subscription_validity() {
        let (env, _admin, _payment_token, user, token_admin_client, client) = setup();

        client.purchase_subscription(&user, &SubscriptionTier::Basic, &true);
        assert!(client.has_active_subscription(&user));

        // Still valid before expiry
        env.ledger().with_mut(|li| li.timestamp += MONTH_IN_SECONDS - 1);
        assert!(client.has_active_subscription(&user));

        // Still valid at exact expiry
        env.ledger().with_mut(|li| li.timestamp += 1);
        assert!(client.has_active_subscription(&user));
    }

    #[test]
    fn test_grace_period() {
        let (env, _admin, _payment_token, user, token_admin_client, client) = setup();

        client.purchase_subscription(&user, &SubscriptionTier::Basic, &true);

        // Advance past expiry but within grace period
        env.ledger().with_mut(|li| li.timestamp += MONTH_IN_SECONDS + 1);
        
        assert!(client.is_in_grace_period(&user));
        assert!(client.has_active_subscription(&user));

        // Advance to end of grace period
        env.ledger().with_mut(|li| li.timestamp += GRACE_PERIOD_SECONDS - 1);
        assert!(client.has_active_subscription(&user));

        // Beyond grace period
        env.ledger().with_mut(|li| li.timestamp += 1);
        assert!(!client.has_active_subscription(&user));
    }

    // ───────────── AUTO-RENEWAL TESTS ─────────────

    #[test]
    fn test_auto_renewal() {
        let (env, _admin, _payment_token, user, token_admin_client, client) = setup();

        client.purchase_subscription(&user, &SubscriptionTier::Basic, &true);
        
        let sub_before = client.get_subscription(&user).unwrap();
        assert_eq!(sub_before.total_renewals, 0);

        // Advance to renewal time
        env.ledger().with_mut(|li| li.timestamp += MONTH_IN_SECONDS + 1);

        client.process_renewal(&user);

        let sub_after = client.get_subscription(&user).unwrap();
        assert_eq!(sub_after.total_renewals, 1);
        assert!(sub_after.is_active);
        assert!(client.has_active_subscription(&user));
    }

    #[test]
    #[should_panic(expected = "Not yet time to renew")]
    fn test_cannot_renew_before_expiry() {
        let (env, _admin, _payment_token, user, token_admin_client, client) = setup();

        client.purchase_subscription(&user, &SubscriptionTier::Basic, &true);
        
        env.ledger().with_mut(|li| li.timestamp += MONTH_IN_SECONDS - 1);
        client.process_renewal(&user);
    }

    #[test]
    #[should_panic(expected = "Auto-renew not enabled")]
    fn test_cannot_auto_renew_when_disabled() {
        let (env, _admin, _payment_token, user, token_admin_client, client) = setup();

        client.purchase_subscription(&user, &SubscriptionTier::Basic, &false);
        
        env.ledger().with_mut(|li| li.timestamp += MONTH_IN_SECONDS + 1);
        client.process_renewal(&user);
    }

    // ───────────── SUBSCRIPTION MANAGEMENT TESTS ─────────────

    #[test]
    fn test_cancel_subscription() {
        let (env, _admin, _payment_token, user, token_admin_client, client) = setup();

        client.purchase_subscription(&user, &SubscriptionTier::Basic, &true);
        
        let sub = client.get_subscription(&user).unwrap();
        assert!(sub.auto_renew);

        client.cancel_subscription(&user);

        let sub = client.get_subscription(&user).unwrap();
        assert!(!sub.auto_renew);
        assert!(sub.is_active); // Still active until expiry
        
        // Still has access until expiry
        env.ledger().with_mut(|li| li.timestamp += MONTH_IN_SECONDS - 1);
        assert!(client.has_active_subscription(&user));
    }

    #[test]
    fn test_toggle_auto_renew() {
        let (_env, _admin, _payment_token, user, _token_admin_client, client) = setup();

        client.purchase_subscription(&user, &SubscriptionTier::Basic, &false);
        
        let sub = client.get_subscription(&user).unwrap();
        assert!(!sub.auto_renew);

        client.set_auto_renew(&user, &true);

        let sub = client.get_subscription(&user).unwrap();
        assert!(sub.auto_renew);
    }

    #[test]
    fn test_upgrade_subscription() {
        let (_env, _admin, _payment_token, user, _token_admin_client, client) = setup();

        client.purchase_subscription(&user, &SubscriptionTier::Basic, &true);
        
        let sub = client.get_subscription(&user).unwrap();
        assert_eq!(sub.tier, SubscriptionTier::Basic);

        client.upgrade_subscription(&user, &SubscriptionTier::Premium);

        let sub = client.get_subscription(&user).unwrap();
        assert_eq!(sub.tier, SubscriptionTier::Premium);
    }

    #[test]
    #[should_panic(expected = "Can only upgrade to higher tier")]
    fn test_cannot_downgrade_subscription() {
        let (_env, _admin, _payment_token, user, _token_admin_client, client) = setup();

        client.purchase_subscription(&user, &SubscriptionTier::Premium, &true);
        client.upgrade_subscription(&user, &SubscriptionTier::Basic);
    }

    // ───────────── BENEFITS TRACKING TESTS ─────────────

    #[test]
    fn test_benefit_usage() {
        let (_env, _admin, _payment_token, user, _token_admin_client, client) = setup();

        client.purchase_subscription(&user, &SubscriptionTier::Basic, &true);
        
        let sub = client.get_subscription(&user).unwrap();
        assert_eq!(sub.benefits_used, 0);

        client.use_benefit(&user);
        client.use_benefit(&user);

        let sub = client.get_subscription(&user).unwrap();
        assert_eq!(sub.benefits_used, 2);
    }

    #[test]
    fn test_benefit_limits() {
        let basic_limit = SubscriptionContract::get_benefit_limit(SubscriptionTier::Basic);
        let premium_limit = SubscriptionContract::get_benefit_limit(SubscriptionTier::Premium);
        let enterprise_limit = SubscriptionContract::get_benefit_limit(SubscriptionTier::Enterprise);

        assert_eq!(basic_limit, 10);
        assert_eq!(premium_limit, 50);
        assert_eq!(enterprise_limit, 999_999);
    }

    // ───────────── GROUP SUBSCRIPTION TESTS ─────────────

    #[test]
    fn test_create_group_subscription() {
        let (_env, _admin, _payment_token, user, _token_admin_client, client) = setup();

        let group_id = client.create_group_subscription(
            &user,
            &SubscriptionTier::Premium,
            &5,
            &true,
        );

        assert_eq!(group_id, 1);

        let group = client.get_group_subscription(&group_id).unwrap();
        assert_eq!(group.owner, user);
        assert_eq!(group.tier, SubscriptionTier::Premium);
        assert_eq!(group.max_members, 5);
        assert!(group.is_active);

        let members = client.get_group_members(&group_id);
        assert_eq!(members.len(), 1);
        assert!(client.has_active_subscription(&user));
    }

    #[test]
    fn test_add_group_member() {
        let (env, _admin, _payment_token, owner, token_admin_client, client) = setup();

        let group_id = client.create_group_subscription(
            &owner,
            &SubscriptionTier::Premium,
            &5,
            &true,
        );

        let member = Address::generate(&env);
        token_admin_client.mint(&member, &100_000_000);
        client.add_group_member(&owner, &group_id, &member);

        let members = client.get_group_members(&group_id);
        assert_eq!(members.len(), 2);
        assert!(client.has_active_subscription(&member));

        let tier = client.get_user_tier(&member).unwrap();
        assert_eq!(tier, SubscriptionTier::Premium);
    }

    #[test]
    fn test_remove_group_member() {
        let (env, _admin, _payment_token, owner, token_admin_client, client) = setup();

        let group_id = client.create_group_subscription(
            &owner,
            &SubscriptionTier::Premium,
            &5,
            &true,
        );

        let member = Address::generate(&env);
        token_admin_client.mint(&member, &100_000_000);
        client.add_group_member(&owner, &group_id, &member);
        
        assert!(client.has_active_subscription(&member));

        client.remove_group_member(&owner, &group_id, &member);

        let members = client.get_group_members(&group_id);
        assert_eq!(members.len(), 1);
        assert!(!client.has_active_subscription(&member));
    }

    #[test]
    #[should_panic(expected = "Group is full")]
    fn test_group_max_members_limit() {
        let (env, _admin, _payment_token, owner, token_admin_client, client) = setup();

        let group_id = client.create_group_subscription(
            &owner,
            &SubscriptionTier::Premium,
            &3,
            &true,
        );

        // Add 2 more members (owner is already member 1)
        let member1 = Address::generate(&env);
        let member2 = Address::generate(&env);
        let member3 = Address::generate(&env);
        
        token_admin_client.mint(&member1, &100_000_000);
        token_admin_client.mint(&member2, &100_000_000);
        token_admin_client.mint(&member3, &100_000_000);

        client.add_group_member(&owner, &group_id, &member1);
        client.add_group_member(&owner, &group_id, &member2);
        client.add_group_member(&owner, &group_id, &member3); // Should panic
    }

    #[test]
    #[should_panic(expected = "Cannot remove owner")]
    fn test_cannot_remove_group_owner() {
        let (_env, _admin, _payment_token, owner, _token_admin_client, client) = setup();

        let group_id = client.create_group_subscription(
            &owner,
            &SubscriptionTier::Premium,
            &5,
            &true,
        );

        client.remove_group_member(&owner, &group_id, &owner);
    }

    // ───────────── GIFTING TESTS ─────────────

    #[test]
    fn test_gift_subscription() {
        let (env, _admin, _payment_token, gifter, token_admin_client, client) = setup();

        let recipient = Address::generate(&env);
        token_admin_client.mint(&recipient, &100_000_000);

        client.gift_subscription(&gifter, &recipient, &SubscriptionTier::Premium);

        assert!(client.has_active_subscription(&recipient));
        
        let sub = client.get_subscription(&recipient).unwrap();
        assert_eq!(sub.tier, SubscriptionTier::Premium);
        assert!(sub.is_gifted);
        assert_eq!(sub.gifted_by.unwrap(), gifter);
        assert!(!sub.auto_renew); // Gifts don't auto-renew
    }

    #[test]
    #[should_panic(expected = "Recipient already has active subscription")]
    fn test_cannot_gift_to_existing_subscriber() {
        let (env, _admin, _payment_token, gifter, token_admin_client, client) = setup();

        let recipient = Address::generate(&env);
        token_admin_client.mint(&recipient, &100_000_000);
        
        client.purchase_subscription(&recipient, &SubscriptionTier::Basic, &true);
        client.gift_subscription(&gifter, &recipient, &SubscriptionTier::Premium);
    }

    // ───────────── ADMIN TESTS ─────────────

    #[test]
    fn test_update_pricing() {
        let (_env, admin, _payment_token, _user, _token_admin_client, client) = setup();

        client.update_pricing(&admin, &2_000_000, &10_000_000, &40_000_000);

        let config = client.get_config();
        assert_eq!(config.basic_price, 2_000_000);
        assert_eq!(config.premium_price, 10_000_000);
        assert_eq!(config.enterprise_price, 40_000_000);
    }

    #[test]
    fn test_pause_contract() {
        let (_env, admin, _payment_token, _user, _token_admin_client, client) = setup();

        client.set_paused(&admin, &true);

        let config = client.get_config();
        assert!(config.paused);
    }

    #[test]
    #[should_panic(expected = "Contract is paused")]
    fn test_cannot_purchase_when_paused() {
        let (_env, admin, _payment_token, user, _token_admin_client, client) = setup();

        client.set_paused(&admin, &true);
        client.purchase_subscription(&user, &SubscriptionTier::Basic, &true);
    }

    // ───────────── VIEW FUNCTION TESTS ─────────────

    #[test]
    fn test_get_time_until_expiry() {
        let (env, _admin, _payment_token, user, token_admin_client, client) = setup();

        client.purchase_subscription(&user, &SubscriptionTier::Basic, &true);

        let time_left = client.get_time_until_expiry(&user);
        assert_eq!(time_left, MONTH_IN_SECONDS);

        env.ledger().with_mut(|li| li.timestamp += 5);

        let time_left = client.get_time_until_expiry(&user);
        assert_eq!(time_left, MONTH_IN_SECONDS - 5);
    }

    #[test]
    fn test_get_total_subscribers() {
        let (env, _admin, _payment_token, user1, token_admin_client, client) = setup();

        assert_eq!(client.get_total_subscribers(), 0);

        client.purchase_subscription(&user1, &SubscriptionTier::Basic, &true);
        assert_eq!(client.get_total_subscribers(), 1);

        let user2 = Address::generate(&env);
        token_admin_client.mint(&user2, &100_000_000);
        client.purchase_subscription(&user2, &SubscriptionTier::Premium, &true);
        assert_eq!(client.get_total_subscribers(), 2);
    }

    #[test]
    fn test_get_user_tier() {
        let (_env, _admin, _payment_token, user, _token_admin_client, client) = setup();

        assert!(client.get_user_tier(&user).is_none());

        client.purchase_subscription(&user, &SubscriptionTier::Enterprise, &true);

        let tier = client.get_user_tier(&user).unwrap();
        assert_eq!(tier, SubscriptionTier::Enterprise);
    }
}
