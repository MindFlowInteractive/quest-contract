#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

fn create_token_contract<'a>(env: &Env, admin: &Address) -> (Address, TokenClient<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let address = sac.address();
    (address.clone(), TokenClient::new(env, &address))
}

struct Setup {
    client: SkillBettingContractClient<'static>,
    admin: Address,
    token_client: TokenClient<'static>,
    asset_admin: StellarAssetClient<'static>,
}

fn setup(env: &Env) -> Setup {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let token_admin = Address::generate(env);
    let (token, token_client) = create_token_contract(env, &token_admin);
    let asset_admin = StellarAssetClient::new(env, &token);

    let contract_id = env.register_contract(None, SkillBettingContract);
    let client = SkillBettingContractClient::new(env, &contract_id);
    client.initialize(&admin, &token, &DEFAULT_FEE_BPS, &None);

    Setup {
        client,
        admin,
        token_client,
        asset_admin,
    }
}

fn fund(env: &Env, s: &Setup, who: &Address, amount: i128) {
    let _ = env;
    s.asset_admin.mint(who, &amount);
}

fn outcomes(env: &Env, n: u32) -> Vec<u32> {
    let mut v = Vec::new(env);
    for i in 0..n {
        v.push_back(i);
    }
    v
}

fn create_default_market(env: &Env, s: &Setup) -> u64 {
    s.client.create_market(
        &s.admin,
        &1u32,
        &outcomes(env, 3),
        &(env.ledger().timestamp() + 1_000),
        &10i128,
        &1_000_000i128,
    )
}

// ---------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------

#[test]
fn test_initialize_sets_config() {
    let env = Env::default();
    let s = setup(&env);
    let config = s.client.get_config();
    assert_eq!(config.admin, s.admin);
    assert_eq!(config.token, s.token);
    assert_eq!(config.fee_bps, DEFAULT_FEE_BPS);
    assert_eq!(config.fee_recipient, s.admin);
}

#[test]
fn test_double_initialize_fails() {
    let env = Env::default();
    let s = setup(&env);
    let res = s.client.try_initialize(&s.admin, &s.token, &DEFAULT_FEE_BPS, &None);
    assert_eq!(res, Err(Ok(SkillBettingError::AlreadyInitialized)));
}

#[test]
fn test_initialize_rejects_excessive_fee() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token, _) = create_token_contract(&env, &token_admin);
    let contract_id = env.register_contract(None, SkillBettingContract);
    let client = SkillBettingContractClient::new(&env, &contract_id);

    let res = client.try_initialize(&admin, &token, &(MAX_FEE_BPS + 1), &None);
    assert_eq!(res, Err(Ok(SkillBettingError::InvalidFeeBps)));
}

#[test]
fn test_set_fee_bps_admin_only() {
    let env = Env::default();
    let s = setup(&env);

    s.client.set_fee_bps(&s.admin, &500);
    assert_eq!(s.client.get_config().fee_bps, 500);

    let stranger = Address::generate(&env);
    let res = s.client.try_set_fee_bps(&stranger, &100);
    assert_eq!(res, Err(Ok(SkillBettingError::Unauthorized)));

    let res = s.client.try_set_fee_bps(&s.admin, &(MAX_FEE_BPS + 1));
    assert_eq!(res, Err(Ok(SkillBettingError::InvalidFeeBps)));
}

// ---------------------------------------------------------------------
// Market creation
// ---------------------------------------------------------------------

#[test]
fn test_create_market() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);

    let market = s.client.get_market(&id);
    assert_eq!(market.id, 1);
    assert_eq!(market.puzzle_id, 1);
    assert_eq!(market.outcomes.len(), 3);
    assert_eq!(market.status, MarketStatus::Open);
    assert_eq!(market.total_pool, 0);
    assert_eq!(market.winning_outcome, None);

    let pools = s.client.get_pools(&id);
    assert_eq!(pools.len(), 3);
    for p in pools.iter() {
        assert_eq!(p.total_amount, 0);
        assert_eq!(p.bet_count, 0);
    }
}

#[test]
fn test_create_market_rejects_bad_inputs() {
    let env = Env::default();
    let s = setup(&env);
    let close = env.ledger().timestamp() + 1_000;

    let res = s
        .client
        .try_create_market(&s.admin, &1u32, &outcomes(&env, 1), &close, &10i128, &100i128);
    assert_eq!(res, Err(Ok(SkillBettingError::InvalidOutcomeCount)));

    let res = s.client.try_create_market(
        &s.admin,
        &1u32,
        &outcomes(&env, 3),
        &env.ledger().timestamp(),
        &10i128,
        &100i128,
    );
    assert_eq!(res, Err(Ok(SkillBettingError::InvalidCloseTime)));

    let res = s.client.try_create_market(
        &s.admin,
        &1u32,
        &outcomes(&env, 3),
        &close,
        &100i128,
        &10i128,
    );
    assert_eq!(res, Err(Ok(SkillBettingError::InvalidBetBounds)));
}

// ---------------------------------------------------------------------
// Betting + odds
// ---------------------------------------------------------------------

#[test]
fn test_place_bet_escrows_and_tracks() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);

    let alice = Address::generate(&env);
    fund(&env, &s, &alice, 1_000_000);

    let bet = s.client.place_bet(&alice, &id, &0u32, &100_000i128);
    assert_eq!(bet.amount, 100_000);
    assert_eq!(bet.outcome_index, 0);
    assert!(!bet.claimed);
    assert_eq!(s.token_client.balance(&alice), 900_000);
    assert_eq!(s.token_client.balance(&s.client.address), 100_000);

    let stored = s.client.get_bet(&id, &alice);
    assert_eq!(stored, bet);

    let market = s.client.get_market(&id);
    assert_eq!(market.total_pool, 100_000);

    let pools = s.client.get_pools(&id);
    assert_eq!(pools.get(0).unwrap().total_amount, 100_000);
    assert_eq!(pools.get(0).unwrap().bet_count, 1);

    let analytics = s.client.get_analytics(&id);
    assert_eq!(analytics.total_pool, 100_000);
    assert_eq!(analytics.total_bets, 1);
    assert_eq!(analytics.unique_bettors, 1);
    assert_eq!(analytics.largest_bet, 100_000);
}

#[test]
fn test_place_bet_validates_bounds_and_status() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);
    let alice = Address::generate(&env);
    fund(&env, &s, &alice, 10_000_000);

    let res = s.client.try_place_bet(&alice, &id, &0u32, &5i128);
    assert_eq!(res, Err(Ok(SkillBettingError::BetTooSmall)));

    let res = s.client.try_place_bet(&alice, &id, &0u32, &2_000_000i128);
    assert_eq!(res, Err(Ok(SkillBettingError::BetTooLarge)));

    let res = s.client.try_place_bet(&alice, &id, &9u32, &100_000i128);
    assert_eq!(res, Err(Ok(SkillBettingError::InvalidOutcome)));

    let res = s.client.try_place_bet(&alice, &999u64, &0u32, &100_000i128);
    assert_eq!(res, Err(Ok(SkillBettingError::MarketNotFound)));
}

#[test]
fn test_betting_closed_after_close_time() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);
    let alice = Address::generate(&env);
    fund(&env, &s, &alice, 1_000_000);

    env.ledger().set_timestamp(env.ledger().timestamp() + 2_000);
    let res = s.client.try_place_bet(&alice, &id, &0u32, &100_000i128);
    assert_eq!(res, Err(Ok(SkillBettingError::BettingClosed)));
}

#[test]
fn test_odds_are_fair_and_sum_to_one() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let carol = Address::generate(&env);
    fund(&env, &s, &alice, 1_000_000);
    fund(&env, &s, &bob, 1_000_000);
    fund(&env, &s, &carol, 1_000_000);

    // 100 on outcome 0, 300 on outcome 1, 600 on outcome 2 => total 1000.
    s.client.place_bet(&alice, &id, &0u32, &100i128);
    s.client.place_bet(&bob, &id, &1u32, &300i128);
    s.client.place_bet(&carol, &id, &2u32, &600i128);

    let odds = s.client.get_odds(&id);
    assert_eq!(odds.len(), 3);

    // Decimal odds = total / pool.
    assert_eq!(odds.get(0).unwrap().odds, 100_000); // 10.0x
    assert_eq!(odds.get(1).unwrap().odds, 33_333); // ~3.33x
    assert_eq!(odds.get(2).unwrap().odds, 16_666); // ~1.66x

    // Implied probabilities sum to 10_000 bps.
    let sum: u32 = odds.iter().map(|o| o.implied_probability_bps).sum();
    assert_eq!(sum, 10_000);
    assert_eq!(odds.get(0).unwrap().implied_probability_bps, 1_000);
    assert_eq!(odds.get(1).unwrap().implied_probability_bps, 3_000);
    assert_eq!(odds.get(2).unwrap().implied_probability_bps, 6_000);
}

#[test]
fn test_odds_update_after_each_bet() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    fund(&env, &s, &alice, 1_000_000);
    fund(&env, &s, &bob, 1_000_000);

    // Empty market: even money for everyone.
    let odds = s.client.get_odds(&id);
    assert_eq!(odds.get(0).unwrap().odds, ODDS_SCALE);

    s.client.place_bet(&alice, &id, &0u32, &100i128);
    let odds = s.client.get_odds(&id);
    assert_eq!(odds.get(0).unwrap().odds, ODDS_SCALE); // only pool => 1.0x
    assert_eq!(odds.get(1).unwrap().odds, ODDS_SCALE); // no pool => even money

    s.client.place_bet(&bob, &id, &1u32, &100i128);
    let odds = s.client.get_odds(&id);
    assert_eq!(odds.get(0).unwrap().odds, 20_000); // 2.0x
    assert_eq!(odds.get(1).unwrap().odds, 20_000); // 2.0x
}

#[test]
fn test_odds_at_placement_recorded() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    fund(&env, &s, &alice, 1_000_000);
    fund(&env, &s, &bob, 1_000_000);

    let bet = s.client.place_bet(&alice, &id, &0u32, &100i128);
    assert_eq!(bet.odds_at_placement, ODDS_SCALE);

    s.client.place_bet(&bob, &id, &1u32, &300i128);
    // Alice's recorded odds are unchanged even though market odds moved.
    assert_eq!(s.client.get_bet(&id, &alice).odds_at_placement, ODDS_SCALE);
}

// ---------------------------------------------------------------------
// Arbitrage prevention
// ---------------------------------------------------------------------

#[test]
fn test_arbitrage_prevented_single_outcome_per_bettor() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);

    let alice = Address::generate(&env);
    fund(&env, &s, &alice, 10_000_000);

    s.client.place_bet(&alice, &id, &0u32, &100_000i128);

    // Backing a second outcome in the same market is rejected.
    let res = s.client.try_place_bet(&alice, &id, &1u32, &100_000i128);
    assert_eq!(res, Err(Ok(SkillBettingError::AlreadyBetOnOutcome)));

    // Even re-betting the same outcome is rejected (no double exposure).
    let res = s.client.try_place_bet(&alice, &id, &0u32, &100_000i128);
    assert_eq!(res, Err(Ok(SkillBettingError::AlreadyBetOnOutcome)));

    // Only one bet recorded.
    assert_eq!(s.client.get_bets(&id).len(), 1);
}

#[test]
fn test_arbitrage_prevented_by_max_bet_cap() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);
    let alice = Address::generate(&env);
    fund(&env, &s, &alice, 10_000_000);

    let res = s.client.try_place_bet(&alice, &id, &0u32, &1_000_001i128);
    assert_eq!(res, Err(Ok(SkillBettingError::BetTooLarge)));
}

// ---------------------------------------------------------------------
// Settlement + payouts
// ---------------------------------------------------------------------

#[test]
fn test_resolve_and_payout_pari_mutuel() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let carol = Address::generate(&env);
    fund(&env, &s, &alice, 1_000_000);
    fund(&env, &s, &bob, 1_000_000);
    fund(&env, &s, &carol, 1_000_000);

    s.client.place_bet(&alice, &id, &0u32, &100i128);
    s.client.place_bet(&bob, &id, &1u32, &300i128);
    s.client.place_bet(&carol, &id, &2u32, &600i128);

    s.client.resolve_market(&s.admin, &id, &0u32);
    let market = s.client.get_market(&id);
    assert_eq!(market.status, MarketStatus::Resolved);
    assert_eq!(market.winning_outcome, Some(0));

    // Alice staked 100 into a 1000 pool; losing pool is 900.
    // gross = 100 + 100*900/100 = 1000; fee 2% => 980.
    let preview = s.client.preview_payout(&id, &alice);
    assert_eq!(preview, 980);

    let payout = s.client.claim_payout(&alice, &id);
    assert_eq!(payout, 980);
    assert_eq!(s.token_client.balance(&alice), 900_000 + 980);

    // Double claim rejected.
    let res = s.client.try_claim_payout(&alice, &id);
    assert_eq!(res, Err(Ok(SkillBettingError::AlreadyClaimed)));

    // Losers cannot claim.
    let res = s.client.try_claim_payout(&bob, &id);
    assert_eq!(res, Err(Ok(SkillBettingError::NothingToClaim)));
}

#[test]
fn test_payout_splits_proportionally_between_winners() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let carol = Address::generate(&env);
    fund(&env, &s, &alice, 1_000_000);
    fund(&env, &s, &bob, 1_000_000);
    fund(&env, &s, &carol, 1_000_000);

    // Winning pool 300 (alice 100, bob 200), losing pool 200 (carol).
    s.client.place_bet(&alice, &id, &0u32, &100i128);
    s.client.place_bet(&bob, &id, &0u32, &200i128);
    s.client.place_bet(&carol, &id, &1u32, &200i128);

    s.client.resolve_market(&s.admin, &id, &0u32);

    // alice: 100 + 100*200/300 = 166 (floor) => fee 2% (3) => 163
    // bob:   200 + 200*200/300 = 333 (floor) => fee 2% (6) => 327
    assert_eq!(s.client.claim_payout(&alice, &id), 163);
    assert_eq!(s.client.claim_payout(&bob, &id), 327);

    let analytics = s.client.get_analytics(&id);
    assert_eq!(analytics.total_paid_out, 490);
    assert!(analytics.settled);
}

#[test]
fn test_claim_before_resolution_fails() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);
    let alice = Address::generate(&env);
    fund(&env, &s, &alice, 1_000_000);
    s.client.place_bet(&alice, &id, &0u32, &100i128);

    let res = s.client.try_claim_payout(&alice, &id);
    assert_eq!(res, Err(Ok(SkillBettingError::NotResolved)));
}

#[test]
fn test_resolve_requires_admin_and_valid_outcome() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);

    let stranger = Address::generate(&env);
    let res = s.client.try_resolve_market(&stranger, &id, &0u32);
    assert_eq!(res, Err(Ok(SkillBettingError::Unauthorized)));

    let res = s.client.try_resolve_market(&s.admin, &id, &9u32);
    assert_eq!(res, Err(Ok(SkillBettingError::InvalidWinner)));

    s.client.resolve_market(&s.admin, &id, &0u32);
    let res = s.client.try_resolve_market(&s.admin, &id, &1u32);
    assert_eq!(res, Err(Ok(SkillBettingError::InvalidStatus)));
}

#[test]
fn test_close_market_blocks_betting() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);
    let alice = Address::generate(&env);
    fund(&env, &s, &alice, 1_000_000);

    let market = s.client.close_market(&s.admin, &id);
    assert_eq!(market.status, MarketStatus::Closed);

    let res = s.client.try_place_bet(&alice, &id, &0u32, &100_000i128);
    assert_eq!(res, Err(Ok(SkillBettingError::InvalidStatus)));

    // A closed market can still be resolved.
    s.client.resolve_market(&s.admin, &id, &0u32);
    assert_eq!(s.client.get_market(&id).status, MarketStatus::Resolved);
}

#[test]
fn test_cancel_market_refunds_all_bettors() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    fund(&env, &s, &alice, 1_000_000);
    fund(&env, &s, &bob, 1_000_000);

    s.client.place_bet(&alice, &id, &0u32, &100i128);
    s.client.place_bet(&bob, &id, &1u32, &200i128);

    s.client.cancel_market(&s.admin, &id);
    assert_eq!(s.client.get_market(&id).status, MarketStatus::Cancelled);
    assert_eq!(s.token_client.balance(&alice), 1_000_000);
    assert_eq!(s.token_client.balance(&bob), 1_000_000);
    assert_eq!(s.token_client.balance(&s.client.address), 0);

    // Cancelled markets cannot be resolved.
    let res = s.client.try_resolve_market(&s.admin, &id, &0u32);
    assert_eq!(res, Err(Ok(SkillBettingError::InvalidStatus)));
}

// ---------------------------------------------------------------------
// Analytics + tracking
// ---------------------------------------------------------------------

#[test]
fn test_analytics_tracks_unique_bettors_and_largest_bet() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    fund(&env, &s, &alice, 1_000_000);
    fund(&env, &s, &bob, 1_000_000);

    s.client.place_bet(&alice, &id, &0u32, &100i128);
    s.client.place_bet(&bob, &id, &1u32, &500i128);

    let analytics = s.client.get_analytics(&id);
    assert_eq!(analytics.total_pool, 600);
    assert_eq!(analytics.total_bets, 2);
    assert_eq!(analytics.unique_bettors, 2);
    assert_eq!(analytics.largest_bet, 500);
    assert!(!analytics.settled);
    assert_eq!(analytics.total_paid_out, 0);
}

#[test]
fn test_get_bets_returns_all_bets() {
    let env = Env::default();
    let s = setup(&env);
    let id = create_default_market(&env, &s);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    fund(&env, &s, &alice, 1_000_000);
    fund(&env, &s, &bob, 1_000_000);

    s.client.place_bet(&alice, &id, &0u32, &100i128);
    s.client.place_bet(&bob, &id, &2u32, &200i128);

    let bets = s.client.get_bets(&id);
    assert_eq!(bets.len(), 2);
    assert_eq!(bets.get(0).unwrap().bettor, alice);
    assert_eq!(bets.get(1).unwrap().bettor, bob);
}

#[test]
fn test_market_ids_increment() {
    let env = Env::default();
    let s = setup(&env);
    let first = create_default_market(&env, &s);
    let second = create_default_market(&env, &s);
    assert_eq!(first, 1);
    assert_eq!(second, 2);
}

#[test]
fn test_bettor_can_bet_on_multiple_markets() {
    let env = Env::default();
    let s = setup(&env);
    let m1 = create_default_market(&env, &s);
    let m2 = create_default_market(&env, &s);

    let alice = Address::generate(&env);
    fund(&env, &s, &alice, 1_000_000);

    s.client.place_bet(&alice, &m1, &0u32, &100i128);
    s.client.place_bet(&alice, &m2, &1u32, &100i128);

    assert_eq!(s.client.get_bet(&m1, &alice).outcome_index, 0);
    assert_eq!(s.client.get_bet(&m2, &alice).outcome_index, 1);
}
