#![cfg(test)]
extern crate std;
use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, Symbol, TryFromVal,
};

/// Reward paid to an ancestor holding a 100% share, before the tier bonus.
const BASE_REWARD: i128 = 1_000;
/// Levels a distribution may pay with the default configuration.
const MAX_LEVELS_T: u32 = 3;
/// Payout records retained per user with the default configuration.
const HISTORY_CAP: u32 = 5;
/// Cooldown between two claims with the default configuration.
const CLAIM_INTERVAL: u64 = 100;

fn limits(per_payout: i128, per_user: i128, global: i128) -> RewardLimits {
    RewardLimits {
        per_payout_cap: per_payout,
        per_user_cap: per_user,
        global_cap: global,
    }
}

fn shares(env: &Env, values: &[u32]) -> Vec<u32> {
    let mut result = Vec::new(env);
    for value in values {
        result.push_back(*value);
    }
    result
}

/// Bronze is always satisfied, Silver needs one direct referral and 1_000
/// referred volume, Gold two referrals and 5_000 volume.
fn default_tiers(env: &Env) -> Vec<TierRule> {
    let mut tiers = Vec::new(env);
    tiers.push_back(TierRule {
        tier: Tier::Bronze,
        min_referrals: 0,
        min_volume: 0,
        bonus_bps: 0,
    });
    tiers.push_back(TierRule {
        tier: Tier::Silver,
        min_referrals: 1,
        min_volume: 1_000,
        bonus_bps: 1_000,
    });
    tiers.push_back(TierRule {
        tier: Tier::Gold,
        min_referrals: 2,
        min_volume: 5_000,
        bonus_bps: 2_500,
    });
    tiers
}

/// Initialises with an arbitrary configuration and a fresh reward token.
fn init(
    env: &Env,
    client: &ReferralRewardsClient,
    admin: &Address,
    base_reward: i128,
    level_shares_bps: Vec<u32>,
    max_levels: u32,
    limits: RewardLimits,
    tiers: Vec<TierRule>,
    max_history: u32,
    claim_interval: u64,
) {
    client.initialize(
        admin,
        &Address::generate(env),
        &base_reward,
        &level_shares_bps,
        &max_levels,
        &limits,
        &tiers,
        &max_history,
        &claim_interval,
    );
}

/// Registers the contract, initialises the protocol with uncapped rewards and
/// returns the client plus the admin address.
fn setup(env: &Env) -> (ReferralRewardsClient<'static>, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ReferralRewards);
    let client = ReferralRewardsClient::new(env, &contract_id);
    let admin = Address::generate(env);
    init(
        env,
        &client,
        &admin,
        BASE_REWARD,
        shares(env, &[10_000, 5_000, 2_500]),
        MAX_LEVELS_T,
        limits(0, 0, 0),
        default_tiers(env),
        HISTORY_CAP,
        CLAIM_INTERVAL,
    );
    (client, admin)
}

/// Mints `depth` codes and binds every user to the previous one, so `users[0]`
/// is the root of the chain and `users[depth - 1]` the deepest leaf.
fn chain(env: &Env, client: &ReferralRewardsClient, depth: u32) -> Vec<Address> {
    let mut users: Vec<Address> = Vec::new(env);
    for _ in 0..depth {
        let user = Address::generate(env);
        client.generate_code(&user);
        users.push_back(user);
    }
    for i in 1..depth {
        let code = client.get_code(&users.get(i - 1).unwrap());
        client.register_referral(&code, &users.get(i).unwrap());
    }
    users
}

fn at(users: &Vec<Address>, index: u32) -> Address {
    users.get(index).unwrap()
}

/// Every event topic that is a symbol, e.g. `code_new`, `reward`, `cap_hit`.
fn topic_symbols(env: &Env) -> Vec<Symbol> {
    let published = env.events().all();
    let mut names: Vec<Symbol> = Vec::new(env);
    for (_, topics, _) in published.iter() {
        for topic in topics.iter() {
            if let Ok(symbol) = Symbol::try_from_val(env, &topic) {
                names.push_back(symbol);
            }
        }
    }
    names
}

// ------------------------------------------------------------------- initialize

#[test]
fn test_initialize() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    let config = client.get_config();
    assert_eq!(config.admin, admin);
    assert_eq!(config.base_reward, BASE_REWARD);
    assert_eq!(config.level_shares_bps, shares(&env, &[10_000, 5_000, 2_500]));
    assert_eq!(config.max_levels, MAX_LEVELS_T);
    assert_eq!(config.max_history, HISTORY_CAP);
    assert_eq!(config.claim_interval, CLAIM_INTERVAL);
    assert_eq!(config.limits.per_payout_cap, 0);
    assert_eq!(config.limits.per_user_cap, 0);
    assert_eq!(config.limits.global_cap, 0);
    assert_eq!(config.tiers.len(), 3);

    let analytics = client.get_analytics();
    assert_eq!(analytics.users, 0);
    assert_eq!(analytics.referrals, 0);
    assert_eq!(analytics.referrers, 0);
    assert_eq!(analytics.distributions, 0);
    assert_eq!(analytics.total_volume, 0);
    assert_eq!(analytics.total_rewards, 0);
    assert_eq!(analytics.top_referrer, None);
}

#[test]
fn test_initialize_twice_is_rejected() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    let result = client.try_initialize(
        &admin,
        &Address::generate(&env),
        &BASE_REWARD,
        &shares(&env, &[10_000]),
        &1,
        &limits(0, 0, 0),
        &default_tiers(&env),
        &HISTORY_CAP,
        &CLAIM_INTERVAL,
    );
    assert_eq!(result, Err(Ok(ReferralError::AlreadyInitialized)));
}

#[test]
fn test_initialize_validates_config() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ReferralRewards);
    let client = ReferralRewardsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let ok_shares = shares(&env, &[10_000, 5_000]);
    let ok_tiers = default_tiers(&env);

    let negative_reward = client.try_initialize(
        &admin,
        &token,
        &-1,
        &ok_shares,
        &2,
        &limits(0, 0, 0),
        &ok_tiers,
        &5,
        &0,
    );
    assert_eq!(negative_reward, Err(Ok(ReferralError::InvalidConfig)));

    let zero_levels = client.try_initialize(
        &admin,
        &token,
        &100,
        &ok_shares,
        &0,
        &limits(0, 0, 0),
        &ok_tiers,
        &5,
        &0,
    );
    assert_eq!(zero_levels, Err(Ok(ReferralError::InvalidConfig)));

    let too_many_levels = client.try_initialize(
        &admin,
        &token,
        &100,
        &ok_shares,
        &(MAX_LEVELS + 1),
        &limits(0, 0, 0),
        &ok_tiers,
        &5,
        &0,
    );
    assert_eq!(too_many_levels, Err(Ok(ReferralError::InvalidConfig)));

    let empty_shares = client.try_initialize(
        &admin,
        &token,
        &100,
        &shares(&env, &[]),
        &2,
        &limits(0, 0, 0),
        &ok_tiers,
        &5,
        &0,
    );
    assert_eq!(empty_shares, Err(Ok(ReferralError::InvalidConfig)));

    let huge_share = client.try_initialize(
        &admin,
        &token,
        &100,
        &shares(&env, &[10_001]),
        &1,
        &limits(0, 0, 0),
        &ok_tiers,
        &5,
        &0,
    );
    assert_eq!(huge_share, Err(Ok(ReferralError::InvalidConfig)));

    let negative_cap = client.try_initialize(
        &admin,
        &token,
        &100,
        &ok_shares,
        &2,
        &limits(0, -1, 0),
        &ok_tiers,
        &5,
        &0,
    );
    assert_eq!(negative_cap, Err(Ok(ReferralError::InvalidConfig)));

    let zero_history = client.try_initialize(
        &admin,
        &token,
        &100,
        &ok_shares,
        &2,
        &limits(0, 0, 0),
        &ok_tiers,
        &0,
        &0,
    );
    assert_eq!(zero_history, Err(Ok(ReferralError::InvalidConfig)));

    let huge_history = client.try_initialize(
        &admin,
        &token,
        &100,
        &ok_shares,
        &2,
        &limits(0, 0, 0),
        &ok_tiers,
        &(MAX_HISTORY + 1),
        &0,
    );
    assert_eq!(huge_history, Err(Ok(ReferralError::InvalidConfig)));

    let huge_bonus = {
        let mut tiers = Vec::new(&env);
        tiers.push_back(TierRule {
            tier: Tier::Gold,
            min_referrals: 0,
            min_volume: 0,
            bonus_bps: 10_001,
        });
        tiers
    };
    let bad_bonus = client.try_initialize(
        &admin,
        &token,
        &100,
        &ok_shares,
        &2,
        &limits(0, 0, 0),
        &huge_bonus,
        &5,
        &0,
    );
    assert_eq!(bad_bonus, Err(Ok(ReferralError::InvalidConfig)));

    // Tiers must run from the weakest to the strongest requirement.
    let unordered = {
        let mut tiers = Vec::new(&env);
        tiers.push_back(TierRule {
            tier: Tier::Gold,
            min_referrals: 5,
            min_volume: 5_000,
            bonus_bps: 100,
        });
        tiers.push_back(TierRule {
            tier: Tier::Silver,
            min_referrals: 1,
            min_volume: 1_000,
            bonus_bps: 100,
        });
        tiers
    };
    let bad_order = client.try_initialize(
        &admin,
        &token,
        &100,
        &ok_shares,
        &2,
        &limits(0, 0, 0),
        &unordered,
        &5,
        &0,
    );
    assert_eq!(bad_order, Err(Ok(ReferralError::InvalidConfig)));

    // None of the rejected calls mutated the state.
    client.initialize(
        &admin,
        &token,
        &100,
        &ok_shares,
        &2,
        &limits(0, 0, 0),
        &ok_tiers,
        &5,
        &0,
    );
    assert_eq!(client.get_config().base_reward, 100);
}

#[test]
fn test_calls_before_initialize_are_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, ReferralRewards);
    let client = ReferralRewardsClient::new(&env, &contract_id);

    assert_eq!(
        client.try_get_analytics(),
        Err(Ok(ReferralError::NotInitialized))
    );
    assert_eq!(
        client.try_get_config(),
        Err(Ok(ReferralError::NotInitialized))
    );
    assert_eq!(
        client.try_generate_code(&Address::generate(&env)),
        Err(Ok(ReferralError::NotInitialized))
    );
    assert_eq!(
        client.try_distribute(&Address::generate(&env), &10),
        Err(Ok(ReferralError::NotInitialized))
    );
}

#[test]
fn test_set_reward_token_requires_admin() {
    let env = Env::default();
    // No auths are mocked, so the admin signature cannot be provided.
    let contract_id = env.register_contract(None, ReferralRewards);
    let client = ReferralRewardsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    client.initialize(
        &admin,
        &token,
        &BASE_REWARD,
        &shares(&env, &[10_000]),
        &1,
        &limits(0, 0, 0),
        &default_tiers(&env),
        &HISTORY_CAP,
        &0,
    );

    let replacement = Address::generate(&env);
    let result = client.try_set_reward_token(&replacement);
    assert!(result.is_err());
    assert_eq!(client.get_config().reward_token, token);

    // With the admin signature the token is replaced.
    env.mock_all_auths();
    client.set_reward_token(&replacement);
    assert_eq!(client.get_config().reward_token, replacement);
}

// ------------------------------------------------------------- code generation

#[test]
fn test_generate_code() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    let alice_code = client.generate_code(&alice);
    let bob_code = client.generate_code(&bob);

    // The code is stable, owned by its user and unique per user.
    assert_eq!(client.get_code(&alice), alice_code);
    assert_eq!(client.get_code(&bob), bob_code);
    assert_ne!(alice_code, bob_code);
    assert_eq!(client.resolve_code(&alice_code), alice);
    assert_eq!(client.resolve_code(&bob_code), bob);
    assert_eq!(client.get_analytics().users, 2);

    // A user owns at most one code.
    let duplicate = client.try_generate_code(&alice);
    assert_eq!(duplicate, Err(Ok(ReferralError::CodeAlreadyExists)));
    assert_eq!(client.get_code(&alice), alice_code);
    assert_eq!(client.get_analytics().users, 2);
}

#[test]
fn test_codes_are_unique_across_many_users() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let mut codes: Vec<Symbol> = Vec::new(&env);
    for _ in 0..8 {
        let user = Address::generate(&env);
        let code = client.generate_code(&user);
        assert!(!codes.contains(&code), "codes must be unique");
        codes.push_back(code);
    }
    assert_eq!(client.get_analytics().users, 8);
}

#[test]
fn test_unknown_codes_and_users_are_rejected() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let unknown = client.try_get_code(&Address::generate(&env));
    assert_eq!(unknown, Err(Ok(ReferralError::CodeNotFound)));

    let garbage = symbol_short!("RX");
    assert_eq!(
        client.try_resolve_code(&garbage),
        Err(Ok(ReferralError::CodeNotFound))
    );
    assert_eq!(
        client.try_register_referral(&garbage, &Address::generate(&env)),
        Err(Ok(ReferralError::CodeNotFound))
    );
}

#[test]
fn test_generate_code_requires_auth() {
    // No auths are mocked, so the user signature cannot be provided.
    let env = Env::default();
    let contract_id = env.register_contract(None, ReferralRewards);
    let client = ReferralRewardsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(
        &admin,
        &Address::generate(&env),
        &BASE_REWARD,
        &shares(&env, &[10_000]),
        &1,
        &limits(0, 0, 0),
        &default_tiers(&env),
        &HISTORY_CAP,
        &0,
    );

    let user = Address::generate(&env);
    let result = client.try_generate_code(&user);
    assert!(result.is_err());
    // The rejected call left no trace.
    assert_eq!(
        client.try_get_code(&user),
        Err(Ok(ReferralError::CodeNotFound))
    );
    assert_eq!(client.get_analytics().users, 0);
}

// ------------------------------------------------------------ referral tracking

#[test]
fn test_register_referral_tracks_the_binding() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let users = chain(&env, &client, 2);
    let root = at(&users, 0);
    let leaf = at(&users, 1);

    assert_eq!(client.get_referrer(&leaf), root);
    assert_eq!(client.get_referral_count(&root), 1);

    let referred = client.get_referral_history(&root);
    assert_eq!(referred.len(), 1);
    assert_eq!(referred.get(0).unwrap(), leaf);

    // A user without a referrer is rejected by `get_referrer`.
    assert_eq!(
        client.try_get_referrer(&root),
        Err(Ok(ReferralError::NoReferrer))
    );
    assert!(client.get_referral_history(&leaf).is_empty());

    let analytics = client.get_analytics();
    assert_eq!(analytics.referrals, 1);
    assert_eq!(analytics.referrers, 1);
    assert_eq!(analytics.users, 2);
}

#[test]
fn test_register_referral_rejects_self_and_duplicate() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let root = Address::generate(&env);
    let other = Address::generate(&env);
    let root_code = client.generate_code(&root);
    let other_code = client.generate_code(&other);

    let self_referral = client.try_register_referral(&root_code, &root);
    assert_eq!(self_referral, Err(Ok(ReferralError::SelfReferral)));
    assert_eq!(client.get_referral_count(&root), 0);

    client.register_referral(&other_code, &root);
    assert_eq!(client.get_referrer(&root), other);

    // Re-binding is rejected, and the original binding survives.
    let again = client.try_register_referral(&root_code, &root);
    assert_eq!(again, Err(Ok(ReferralError::AlreadyRegistered)));
    assert_eq!(client.get_referrer(&root), other);
    assert_eq!(client.get_referral_count(&other), 1);
    assert_eq!(client.get_analytics().referrals, 1);
}

#[test]
fn test_register_referral_detects_cycles() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let a_code = client.generate_code(&a);
    let b_code = client.generate_code(&b);

    // b is referred by a ...
    client.register_referral(&a_code, &b);
    assert_eq!(client.get_referrer(&b), a);

    // ... so a cannot be referred by b without closing the loop a -> b -> a.
    let cycle = client.try_register_referral(&b_code, &a);
    assert_eq!(cycle, Err(Ok(ReferralError::CycleDetected)));
    assert_eq!(
        client.try_get_referrer(&a),
        Err(Ok(ReferralError::NoReferrer))
    );
    assert_eq!(client.get_analytics().referrals, 1);

    // A second loop is rejected as well: c is referred by b, and b may not be
    // re-bound to c without closing a -> b -> c -> a.
    let c = Address::generate(&env);
    let c_code = client.generate_code(&c);
    client.register_referral(&b_code, &c);
    let deep_cycle = client.try_register_referral(&c_code, &b);
    assert_eq!(deep_cycle, Err(Ok(ReferralError::CycleDetected)));
    assert_eq!(client.get_referrer(&b), a);
    assert_eq!(client.get_referrer(&c), b);
}

#[test]
fn test_is_referral_proves_the_binding() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let users = chain(&env, &client, 3);
    let root = at(&users, 0);
    let middle = at(&users, 1);
    let leaf = at(&users, 2);
    let root_code = client.get_code(&root);
    let leaf_code = client.get_code(&leaf);

    assert!(client.is_referral(&root_code, &middle));
    // The root is not referred by anyone.
    assert!(!client.is_referral(&root_code, &root));
    // The leaf is referred by the middle, not by the root.
    assert!(!client.is_referral(&root_code, &leaf));
    assert!(!client.is_referral(&leaf_code, &middle));

    assert_eq!(
        client.try_is_referral(&symbol_short!("RX"), &middle),
        Err(Ok(ReferralError::CodeNotFound))
    );
}

#[test]
fn test_get_ancestors() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let users = chain(&env, &client, 4);
    let leaf = at(&users, 3);

    let all = client.get_ancestors(&leaf, &0);
    assert_eq!(all.len(), 3);
    assert_eq!(all.get(0).unwrap(), at(&users, 2));
    assert_eq!(all.get(1).unwrap(), at(&users, 1));
    assert_eq!(all.get(2).unwrap(), at(&users, 0));

    // A limit truncates the walk at level 1.
    let limited = client.get_ancestors(&leaf, &1);
    assert_eq!(limited.len(), 1);
    assert_eq!(limited.get(0).unwrap(), at(&users, 2));

    // The root has no ancestors.
    assert!(client.get_ancestors(&at(&users, 0), &0).is_empty());
}

// ---------------------------------------------------------- multi level payouts

#[test]
fn test_multi_level_distribution_pays_every_level() {
    let env = Env::default();
    let (client, _admin) = setup(&env);
    env.ledger().set_timestamp(1_000);

    let users = chain(&env, &client, 3);
    let root = at(&users, 0);
    let middle = at(&users, 1);
    let leaf = at(&users, 2);

    let result = client.distribute(&leaf, &1_000);
    assert_eq!(result.source, leaf);
    assert_eq!(result.volume, 1_000);
    assert_eq!(result.levels_paid, 2);
    assert!(!result.capped);
    // Level 1 earns the full base reward, level 2 half of it.
    assert_eq!(result.total_credited, 1_500);
    assert_eq!(result.payouts.len(), 2);

    let first = result.payouts.get(0).unwrap();
    assert_eq!(first.level, 1);
    assert_eq!(first.recipient, middle);
    assert_eq!(first.source, leaf);
    assert_eq!(first.volume, 1_000);
    assert_eq!(first.base_amount, 1_000);
    assert_eq!(first.bonus_amount, 0);
    assert_eq!(first.amount, 1_000);
    assert_eq!(first.tier, Tier::Bronze);
    assert_eq!(first.timestamp, 1_000);

    let second = result.payouts.get(1).unwrap();
    assert_eq!(second.level, 2);
    assert_eq!(second.recipient, root);
    assert_eq!(second.base_amount, 500);
    assert_eq!(second.amount, 500);

    // The source never pays itself.
    assert_eq!(client.get_balance(&leaf), 0);
    assert_eq!(client.get_balance(&middle), 1_000);
    assert_eq!(client.get_balance(&root), 500);

    let middle_stats = client.get_stats(&middle);
    assert_eq!(middle_stats.user, middle);
    assert_eq!(middle_stats.direct_referrals, 1);
    assert_eq!(middle_stats.referred_volume, 1_000);
    assert_eq!(middle_stats.rewards_earned, 1_000);
    assert_eq!(middle_stats.pending, 1_000);
    assert_eq!(middle_stats.payout_count, 1);
    assert_eq!(middle_stats.claim_count, 0);

    // The middle keeps the history of its own payout.
    let history = client.get_history(&middle, &10);
    assert_eq!(history.len(), 1);
    assert_eq!(history.get(0).unwrap().level, 1);
    assert_eq!(history.get(0).unwrap().source, leaf);
    // The root was not a direct referrer of the leaf, so it referred nobody.
    assert!(client.get_referral_history(&leaf).is_empty());
    assert_eq!(client.get_referral_history(&middle).len(), 1);
}

#[test]
fn test_distribution_is_cumulative() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let users = chain(&env, &client, 2);
    let root = at(&users, 0);
    let leaf = at(&users, 1);

    // 100 + 200 + 300 stays below the Silver threshold, so every call pays the
    // flat base reward and the recomputation is identical on each call.
    client.distribute(&leaf, &100);
    client.distribute(&leaf, &200);
    client.distribute(&leaf, &300);

    assert_eq!(client.get_balance(&root), 3_000);
    assert_eq!(client.get_stats(&root).payout_count, 3);
    assert_eq!(client.get_stats(&root).referred_volume, 600);
    assert_eq!(client.get_stats(&root).rewards_earned, 3_000);
    assert_eq!(client.get_history(&root, &10).len(), 3);
    assert_eq!(client.get_analytics().distributions, 3);
    assert_eq!(client.get_analytics().total_volume, 600);
    assert_eq!(client.get_analytics().total_rewards, 3_000);
}

#[test]
fn test_max_levels_truncates_the_walk() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ReferralRewards);
    let client = ReferralRewardsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    // Only the two closest ancestors are paid.
    init(
        &env,
        &client,
        &admin,
        BASE_REWARD,
        shares(&env, &[10_000, 5_000, 2_500]),
        2,
        limits(0, 0, 0),
        default_tiers(&env),
        HISTORY_CAP,
        0,
    );

    let users = chain(&env, &client, 4);
    let result = client.distribute(&at(&users, 3), &1_000);

    assert_eq!(result.levels_paid, 2);
    assert_eq!(result.total_credited, 1_500);
    assert_eq!(result.payouts.len(), 2);
    assert_eq!(result.payouts.get(0).unwrap().recipient, at(&users, 2));
    assert_eq!(result.payouts.get(1).unwrap().recipient, at(&users, 1));
    assert_eq!(client.get_balance(&at(&users, 2)), 1_000);
    assert_eq!(client.get_balance(&at(&users, 1)), 500);
    // The root three levels up is not paid at all.
    assert_eq!(client.get_balance(&at(&users, 0)), 0);
    // The volume of the truncated ancestors is still tracked.
    assert_eq!(client.get_stats(&at(&users, 0)).referred_volume, 0);
    assert_eq!(client.get_analytics().total_volume, 1_000);
}

#[test]
fn test_zero_share_level_is_not_paid() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ReferralRewards);
    let client = ReferralRewardsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    // The second level earns nothing.
    init(
        &env,
        &client,
        &admin,
        BASE_REWARD,
        shares(&env, &[10_000, 0, 2_500]),
        MAX_LEVELS_T,
        limits(0, 0, 0),
        default_tiers(&env),
        HISTORY_CAP,
        0,
    );

    let users = chain(&env, &client, 3);
    let root = at(&users, 0);
    let middle = at(&users, 1);
    let result = client.distribute(&at(&users, 2), &1_000);

    // The level is still walked and reported, it just earns nothing.
    assert_eq!(result.payouts.len(), 2);
    assert_eq!(result.levels_paid, 1);
    assert_eq!(result.total_credited, 1_000);

    let skipped = result.payouts.get(1).unwrap();
    assert_eq!(skipped.level, 2);
    assert_eq!(skipped.recipient, root);
    assert_eq!(skipped.base_amount, 0);
    assert_eq!(skipped.amount, 0);

    assert_eq!(client.get_balance(&middle), 1_000);
    assert_eq!(client.get_balance(&root), 0);
    // Zero payouts are not retained in the history.
    assert!(client.get_history(&root, &10).is_empty());
    assert_eq!(client.get_history(&middle, &10).len(), 1);
    // The volume is still attributed to the zero share level.
    assert_eq!(client.get_stats(&root).referred_volume, 1_000);
}

// ----------------------------------------------------------------- tier bonuses

#[test]
fn test_tier_bonus_applies_after_the_threshold() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let users = chain(&env, &client, 2);
    let root = at(&users, 0);
    let leaf = at(&users, 1);

    // One direct referral but no volume yet: still Bronze.
    assert_eq!(client.tier_for(&root), Tier::Bronze);
    assert_eq!(client.tier_bonus(&root), 0);

    // 600 + 400 is not enough at the start of either call, so no bonus is paid.
    let first = client.distribute(&leaf, &600);
    assert_eq!(first.total_credited, 1_000);
    assert_eq!(first.payouts.get(0).unwrap().bonus_amount, 0);
    let second = client.distribute(&leaf, &400);
    assert_eq!(second.total_credited, 1_000);
    assert_eq!(second.payouts.get(0).unwrap().bonus_amount, 0);
    assert_eq!(second.payouts.get(0).unwrap().tier, Tier::Bronze);

    // The threshold is crossed by the second call, so the third one pays the
    // Silver bonus of 10%: 1_000 + 100.
    assert_eq!(client.tier_for(&root), Tier::Silver);
    assert_eq!(client.tier_bonus(&root), 1_000);
    let third = client.distribute(&leaf, &100);
    let payout = third.payouts.get(0).unwrap();
    assert_eq!(payout.tier, Tier::Silver);
    assert_eq!(payout.base_amount, 1_000);
    assert_eq!(payout.bonus_amount, 100);
    assert_eq!(payout.amount, 1_100);
    assert_eq!(third.total_credited, 1_100);

    assert_eq!(client.get_stats(&root).rewards_earned, 3_100);
    assert_eq!(client.get_stats(&root).referred_volume, 1_100);
}

#[test]
fn test_gold_tier_bonus() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let root = Address::generate(&env);
    let first = Address::generate(&env);
    let second = Address::generate(&env);
    let root_code = client.generate_code(&root);
    client.generate_code(&first);
    client.generate_code(&second);
    client.register_referral(&root_code, &first);
    client.register_referral(&root_code, &second);
    assert_eq!(client.get_referral_count(&root), 2);

    // Gold needs 2 referrals *and* 5_000 volume, so the first call is Bronze.
    let first_call = client.distribute(&first, &5_000);
    assert_eq!(first_call.payouts.get(0).unwrap().tier, Tier::Bronze);
    assert_eq!(first_call.payouts.get(0).unwrap().amount, 1_000);

    assert_eq!(client.tier_for(&root), Tier::Gold);
    assert_eq!(client.tier_bonus(&root), 2_500);

    // 25% bonus on 1_000.
    let second_call = client.distribute(&first, &100);
    let payout = second_call.payouts.get(0).unwrap();
    assert_eq!(payout.tier, Tier::Gold);
    assert_eq!(payout.bonus_amount, 250);
    assert_eq!(payout.amount, 1_250);
    assert_eq!(client.get_stats(&root).rewards_earned, 2_250);
}

#[test]
fn test_tier_requires_both_conditions() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let root = Address::generate(&env);
    let leaf = Address::generate(&env);
    let root_code = client.generate_code(&root);
    client.generate_code(&leaf);
    client.register_referral(&root_code, &leaf);

    // Enough referrals, not enough volume.
    client.distribute(&leaf, &999);
    assert_eq!(client.tier_for(&root), Tier::Bronze);

    // Enough volume now, so Silver is reached.
    client.distribute(&leaf, &1);
    assert_eq!(client.tier_for(&root), Tier::Silver);
    assert_eq!(client.tier_bonus(&root), 1_000);
}

// ----------------------------------------------------------------------- caps

#[test]
fn test_per_payout_cap_clamps_a_single_payout() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ReferralRewards);
    let client = ReferralRewardsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    init(
        &env,
        &client,
        &admin,
        BASE_REWARD,
        shares(&env, &[10_000]),
        1,
        limits(300, 0, 0),
        default_tiers(&env),
        HISTORY_CAP,
        0,
    );

    let users = chain(&env, &client, 2);
    let result = client.distribute(&at(&users, 1), &1_000);

    assert!(result.capped);
    assert_eq!(result.levels_paid, 1);
    assert_eq!(result.payouts.get(0).unwrap().amount, 300);
    // The unclamped price is still reported for transparency.
    assert_eq!(result.payouts.get(0).unwrap().base_amount, 1_000);
    assert_eq!(result.total_credited, 300);
    assert_eq!(client.get_balance(&at(&users, 0)), 300);
    assert_eq!(client.get_analytics().total_rewards, 300);
}

#[test]
fn test_per_user_cap_clamps_the_lifetime_total() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ReferralRewards);
    let client = ReferralRewardsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    init(
        &env,
        &client,
        &admin,
        BASE_REWARD,
        shares(&env, &[10_000]),
        1,
        limits(0, 500, 0),
        default_tiers(&env),
        HISTORY_CAP,
        0,
    );

    let users = chain(&env, &client, 2);
    let root = at(&users, 0);
    let leaf = at(&users, 1);

    let first = client.distribute(&leaf, &1_000);
    assert!(first.capped);
    assert_eq!(first.total_credited, 500);
    assert_eq!(client.get_balance(&root), 500);

    // The lifetime budget is exhausted, the call still succeeds and pays 0.
    let second = client.distribute(&leaf, &1_000);
    assert!(second.capped);
    assert_eq!(second.levels_paid, 0);
    assert_eq!(second.total_credited, 0);
    assert_eq!(second.payouts.get(0).unwrap().amount, 0);
    assert_eq!(client.get_balance(&root), 500);
    assert_eq!(client.get_stats(&root).rewards_earned, 500);
    // The volume is still tracked even when the credit is capped away.
    assert_eq!(client.get_stats(&root).referred_volume, 2_000);
    assert_eq!(client.get_analytics().total_volume, 2_000);
    // Zero payouts are not retained in the history.
    assert_eq!(client.get_history(&root, &10).len(), 1);
}

#[test]
fn test_global_cap_is_shared_by_every_level() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ReferralRewards);
    let client = ReferralRewardsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    init(
        &env,
        &client,
        &admin,
        BASE_REWARD,
        shares(&env, &[10_000, 5_000]),
        2,
        limits(0, 0, 700),
        default_tiers(&env),
        HISTORY_CAP,
        0,
    );

    let users = chain(&env, &client, 3);
    let root = at(&users, 0);
    let middle = at(&users, 1);
    let leaf = at(&users, 2);

    // Level 1 would take 1_000 but only 700 of global budget is left.
    let result = client.distribute(&leaf, &1_000);
    assert!(result.capped);
    assert_eq!(result.levels_paid, 1);
    assert_eq!(result.total_credited, 700);
    assert_eq!(result.payouts.get(0).unwrap().amount, 700);
    // The second level is starved by the global budget.
    assert_eq!(result.payouts.get(1).unwrap().amount, 0);
    assert_eq!(client.get_balance(&middle), 700);
    assert_eq!(client.get_balance(&root), 0);
    assert_eq!(client.get_analytics().total_rewards, 700);

    // Nothing more can be paid out.
    let after = client.distribute(&leaf, &1_000);
    assert!(after.capped);
    assert_eq!(after.total_credited, 0);
    assert_eq!(after.levels_paid, 0);
    assert_eq!(client.get_analytics().total_rewards, 700);
}

#[test]
fn test_invalid_amounts_are_rejected() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let users = chain(&env, &client, 2);
    let leaf = at(&users, 1);

    assert_eq!(
        client.try_distribute(&leaf, &0),
        Err(Ok(ReferralError::InvalidAmount))
    );
    assert_eq!(
        client.try_distribute(&leaf, &-5),
        Err(Ok(ReferralError::InvalidAmount))
    );
    assert_eq!(
        client.try_quote(&leaf, &0),
        Err(Ok(ReferralError::InvalidAmount))
    );
    assert_eq!(client.get_analytics().distributions, 0);
}

#[test]
fn test_distributing_without_a_referrer_is_rejected() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let lonely = Address::generate(&env);
    client.generate_code(&lonely);

    assert_eq!(
        client.try_distribute(&lonely, &1_000),
        Err(Ok(ReferralError::NoChain))
    );
    assert_eq!(
        client.try_quote(&lonely, &1_000),
        Err(Ok(ReferralError::NoChain))
    );
    assert_eq!(client.get_analytics().distributions, 0);
}

#[test]
fn test_distribute_requires_auth() {
    // No auths are mocked, so the source signature cannot be provided.
    let env = Env::default();
    let contract_id = env.register_contract(None, ReferralRewards);
    let client = ReferralRewardsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(
        &admin,
        &Address::generate(&env),
        &BASE_REWARD,
        &shares(&env, &[10_000]),
        &1,
        &limits(0, 0, 0),
        &default_tiers(&env),
        &HISTORY_CAP,
        &0,
    );

    let root = Address::generate(&env);
    let leaf = Address::generate(&env);
    let root_code = client.generate_code(&root);
    client.generate_code(&leaf);
    client.register_referral(&root_code, &leaf);

    let result = client.try_distribute(&leaf, &1_000);
    assert!(result.is_err());
    assert_eq!(client.get_balance(&root), 0);
    assert_eq!(client.get_analytics().distributions, 0);
}

// ----------------------------------------------------------------------- quote

#[test]
fn test_quote_matches_the_distribution() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let users = chain(&env, &client, 3);
    let leaf = at(&users, 2);

    let quoted = client.quote(&leaf, &1_000);
    assert_eq!(quoted.total_credited, 1_500);
    assert_eq!(quoted.levels_paid, 2);
    assert!(!quoted.capped);
    // Quoting must not mutate anything.
    assert_eq!(client.get_balance(&at(&users, 1)), 0);
    assert_eq!(client.get_analytics().distributions, 0);

    let distributed = client.distribute(&leaf, &1_000);
    assert_eq!(distributed, quoted);

    // The second round pays the Silver bonus on both levels: 1_100 + 550.
    let quoted_again = client.quote(&leaf, &1_000);
    assert_eq!(quoted_again.total_credited, 1_650);
    assert_eq!(quoted_again.payouts.get(0).unwrap().amount, 1_100);
    assert_eq!(quoted_again.payouts.get(1).unwrap().amount, 550);
    assert_eq!(client.distribute(&leaf, &1_000), quoted_again);
}

// -------------------------------------------------------------------- history

#[test]
fn test_history_is_bounded_and_paginated() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let users = chain(&env, &client, 2);
    let root = at(&users, 0);
    let leaf = at(&users, 1);

    // 7 rounds of 100 volume stay below the Silver threshold, so every round
    // pays the flat base reward of 1_000.
    for round in 1..=7u64 {
        env.ledger().set_timestamp(round * 1_000);
        client.distribute(&leaf, &100);
    }

    // At most `max_history` records are retained, the oldest dropped first.
    let history = client.get_history(&root, &10);
    assert_eq!(history.len(), HISTORY_CAP);
    assert_eq!(history.get(0).unwrap().timestamp, 3_000);
    assert_eq!(history.get(4).unwrap().timestamp, 7_000);

    // A smaller limit returns the most recent entries, oldest first.
    let tail = client.get_history(&root, &2);
    assert_eq!(tail.len(), 2);
    assert_eq!(tail.get(0).unwrap().timestamp, 6_000);
    assert_eq!(tail.get(1).unwrap().timestamp, 7_000);

    // A zero limit returns nothing.
    assert!(client.get_history(&root, &0).is_empty());
    // The full count survives the truncation of the list.
    assert_eq!(client.get_stats(&root).payout_count, 7);
    assert_eq!(client.get_stats(&root).rewards_earned, 7_000);
    assert_eq!(client.get_stats(&root).referred_volume, 700);
}

// --------------------------------------------------------------------- claims

#[test]
fn test_claim_reward_pays_out_and_is_idempotent() {
    let env = Env::default();
    let (client, _admin) = setup(&env);
    env.ledger().set_timestamp(5_000);

    let users = chain(&env, &client, 2);
    let root = at(&users, 0);
    let leaf = at(&users, 1);
    client.distribute(&leaf, &1_000);
    assert_eq!(client.get_balance(&root), 1_000);

    let claimed = client.claim_reward(&root);
    assert_eq!(claimed, 1_000);
    assert_eq!(client.get_balance(&root), 0);

    let stats = client.get_stats(&root);
    assert_eq!(stats.rewards_claimed, 1_000);
    assert_eq!(stats.pending, 0);
    assert_eq!(stats.claim_count, 1);
    assert_eq!(stats.last_claim_at, 5_000);
    assert_eq!(client.get_analytics().claims, 1);
    assert_eq!(client.get_analytics().total_claimed, 1_000);

    // A second claim without new credits is rejected.
    let again = client.try_claim_reward(&root);
    assert_eq!(again, Err(Ok(ReferralError::NothingToClaim)));
    assert_eq!(client.get_stats(&root).rewards_claimed, 1_000);
    assert_eq!(client.get_analytics().claims, 1);
}

#[test]
fn test_claim_without_rewards_is_rejected() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let user = Address::generate(&env);
    client.generate_code(&user);
    assert_eq!(client.get_balance(&user), 0);
    assert_eq!(
        client.try_claim_reward(&user),
        Err(Ok(ReferralError::NothingToClaim))
    );
}

#[test]
fn test_claim_cooldown() {
    let env = Env::default();
    let (client, _admin) = setup(&env);
    env.ledger().set_timestamp(1_000);

    let users = chain(&env, &client, 2);
    let root = at(&users, 0);
    let leaf = at(&users, 1);

    client.distribute(&leaf, &1_000);
    assert_eq!(client.claim_reward(&root), 1_000);

    // A new credit arrives inside the cooldown window. The referrer crossed the
    // Silver threshold in the meantime, so the bonus is paid as well.
    client.distribute(&leaf, &1_000);
    assert_eq!(client.get_balance(&root), 1_100);

    env.ledger().set_timestamp(1_050);
    let too_soon = client.try_claim_reward(&root);
    assert_eq!(too_soon, Err(Ok(ReferralError::ClaimCooldown)));
    assert_eq!(client.get_balance(&root), 1_100);

    // After the window the balance is claimable again.
    env.ledger().set_timestamp(1_100);
    assert_eq!(client.claim_reward(&root), 1_100);
    assert_eq!(client.get_stats(&root).rewards_claimed, 2_100);
    assert_eq!(client.get_stats(&root).claim_count, 2);
    assert_eq!(client.get_analytics().total_claimed, 2_100);
}

#[test]
fn test_claim_requires_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ReferralRewards);
    let client = ReferralRewardsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(
        &admin,
        &Address::generate(&env),
        &BASE_REWARD,
        &shares(&env, &[10_000]),
        &1,
        &limits(0, 0, 0),
        &default_tiers(&env),
        &HISTORY_CAP,
        &0,
    );

    let root = Address::generate(&env);
    let leaf = Address::generate(&env);
    let root_code = client.generate_code(&root);
    client.generate_code(&leaf);
    client.register_referral(&root_code, &leaf);
    client.distribute(&leaf, &1_000);

    // Disabling the mocked auths means the user signature cannot be provided.
    env.set_auths(&[]);
    let result = client.try_claim_reward(&root);
    assert!(result.is_err());
    assert_eq!(client.get_balance(&root), 1_000);
    assert_eq!(client.get_stats(&root).rewards_claimed, 0);
}

// ------------------------------------------------------------------ analytics

#[test]
fn test_analytics_counters() {
    let env = Env::default();
    let (client, _admin) = setup(&env);
    env.ledger().set_timestamp(1_000);

    // Fresh state: no code owner, so no top referrer.
    let empty = client.get_analytics();
    assert_eq!(empty.users, 0);
    assert_eq!(empty.top_referrer, None);
    assert_eq!(empty.top_referrer_rewards, 0);

    let users = chain(&env, &client, 3);
    let root = at(&users, 0);
    let middle = at(&users, 1);
    let leaf = at(&users, 2);

    let analytics = client.get_analytics();
    assert_eq!(analytics.users, 3);
    assert_eq!(analytics.referrals, 2);
    assert_eq!(analytics.referrers, 2);
    // Without any volume nobody crossed the Silver threshold.
    assert_eq!(analytics.bronze, 3);
    assert_eq!(analytics.silver, 0);
    assert_eq!(analytics.gold, 0);
    assert_eq!(analytics.top_referrer, None);

    client.distribute(&leaf, &1_000);
    let analytics = client.get_analytics();
    assert_eq!(analytics.distributions, 1);
    assert_eq!(analytics.total_volume, 1_000);
    assert_eq!(analytics.total_rewards, 1_500);
    // Root and middle crossed the Silver threshold, the leaf is still Bronze.
    assert_eq!(analytics.bronze, 1);
    assert_eq!(analytics.silver, 2);
    assert_eq!(analytics.gold, 0);
    assert_eq!(analytics.top_referrer, Some(middle.clone()));
    assert_eq!(analytics.top_referrer_rewards, 1_000);

    client.claim_reward(&root);
    let analytics = client.get_analytics();
    assert_eq!(analytics.claims, 1);
    assert_eq!(analytics.total_claimed, 500);
    // Crediting is not affected by claiming.
    assert_eq!(analytics.total_rewards, 1_500);
    assert_eq!(analytics.top_referrer, Some(middle));
}

// --------------------------------------------------------------------- events

#[test]
fn test_events_are_published() {
    let env = Env::default();
    let (client, _admin) = setup(&env);
    env.ledger().set_timestamp(1_000);

    let users = chain(&env, &client, 2);
    let root = at(&users, 0);
    let leaf = at(&users, 1);
    client.distribute(&leaf, &1_000);
    client.claim_reward(&root);

    let topics = topic_symbols(&env);
    assert!(topics.contains(&symbol_short!("init")));
    assert!(topics.contains(&symbol_short!("code_new")));
    assert!(topics.contains(&symbol_short!("ref_reg")));
    assert!(topics.contains(&symbol_short!("reward")));
    // The distribution pushed the referrer from Bronze to Silver.
    assert!(topics.contains(&symbol_short!("tier_up")));
    assert!(topics.contains(&symbol_short!("claim")));
}

#[test]
fn test_cap_reached_event_is_published() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ReferralRewards);
    let client = ReferralRewardsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    init(
        &env,
        &client,
        &admin,
        BASE_REWARD,
        shares(&env, &[10_000]),
        1,
        limits(100, 0, 0),
        default_tiers(&env),
        HISTORY_CAP,
        0,
    );

    let users = chain(&env, &client, 2);
    let result = client.distribute(&at(&users, 1), &1_000);
    assert!(result.capped);

    assert!(topic_symbols(&env).contains(&symbol_short!("cap_hit")));
}
