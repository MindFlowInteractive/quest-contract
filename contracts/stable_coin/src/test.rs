#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
};

/// 110% per-user floor, 100% system floor, a 20%-of-shortfall stability fee and
/// a 5% liquidation bonus.
const MIN_COLLATERAL_BPS: u32 = 11_000;
const MIN_RESERVE_BPS: u32 = 10_000;
const STABILITY_FEE_BPS: u32 = 2_000;
const LIQ_BONUS_BPS: u32 = 500;

struct Actors {
    admin: Address,
    pool: Address,
    user: Address,
    other: Address,
    liquidator: Address,
    collateral: Address,
}

fn setup<'a>(env: &'a Env) -> (StableCoinClient<'a>, Actors) {
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let contract_id = env.register_contract(None, StableCoin);
    let client = StableCoinClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let pool = Address::generate(env);
    let user = Address::generate(env);
    let other = Address::generate(env);
    let liquidator = Address::generate(env);

    let collateral = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let admin_client = StellarAssetClient::new(env, &collateral);
    for who in [user.clone(), other.clone(), liquidator.clone(), pool.clone()] {
        admin_client.mint(&who, &10_000_000);
    }

    client.initialize(
        &admin,
        &collateral,
        &pool,
        &MIN_COLLATERAL_BPS,
        &MIN_RESERVE_BPS,
        &STABILITY_FEE_BPS,
        &LIQ_BONUS_BPS,
    );

    let actors = Actors {
        admin,
        pool,
        user,
        other,
        liquidator,
        collateral,
    };
    (client, actors)
}

fn collateral_client<'a>(env: &'a Env, actors: &Actors) -> TokenClient<'a> {
    TokenClient::new(env, &actors.collateral)
}

fn peg(x: i128) -> i128 {
    x * SCALE
}

// ----------------------------------------------------------------------
// Setup
// ----------------------------------------------------------------------

#[test]
fn initialize_is_one_shot() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    assert!(client
        .try_initialize(
            &actors.admin,
            &actors.collateral,
            &actors.pool,
            &MIN_COLLATERAL_BPS,
            &MIN_RESERVE_BPS,
            &STABILITY_FEE_BPS,
            &LIQ_BONUS_BPS
        )
        .is_err());

    let state = client.get_state();
    assert_eq!(state.peg, SCALE);
    assert_eq!(state.market_price, SCALE);
    assert_eq!(state.total_collateral, 0);
    assert_eq!(state.total_supply, 0);
    assert!(!client.is_paused());
}

#[test]
fn initialize_rejects_invalid_parameters() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StableCoin);
    let client = StableCoinClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token = Address::generate(&env);

    assert!(client
        .try_initialize(&admin, &token, &admin, &0, &MIN_RESERVE_BPS, &0, &0)
        .is_err());
    assert!(client
        .try_initialize(&admin, &token, &admin, &MIN_COLLATERAL_BPS, &0, &0, &0)
        .is_err());
    assert!(client
        .try_initialize(
            &admin,
            &token,
            &admin,
            &MIN_COLLATERAL_BPS,
            &MIN_RESERVE_BPS,
            &10_000,
            &0
        )
        .is_err());
}

#[test]
fn parameters_admin_and_pool_can_be_rotated() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    assert!(client
        .try_set_parameters(&11_000, &11_000, &STABILITY_FEE_BPS, &LIQ_BONUS_BPS)
        .is_err());

    client.set_parameters(&12_000, &12_000, &1_000, &250);
    let config = client.get_config();
    assert_eq!(config.min_collateral_ratio_bps, 12_000);
    assert_eq!(config.liquidation_bonus_bps, 250);

    client.set_admin(&actors.other);
    assert_eq!(client.get_config().admin, actors.other);

    let new_pool = Address::generate(&env);
    client.set_stability_pool(&new_pool);
    assert_eq!(client.get_config().stability_pool, new_pool);
}

// ----------------------------------------------------------------------
// Collateral
// ----------------------------------------------------------------------

#[test]
fn deposits_are_tracked_in_custody() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    let col = collateral_client(&env, &actors);
    let before = col.balance(&actors.user);

    client.deposit_collateral(&actors.user, &10_000);
    assert_eq!(before - col.balance(&actors.user), 10_000);
    assert_eq!(client.collateral_of(&actors.user), 10_000);
    assert_eq!(client.get_state().total_collateral, 10_000);

    client.deposit_collateral(&actors.user, &500);
    assert_eq!(client.collateral_of(&actors.user), 10_500);
    assert_eq!(client.get_state().total_collateral, 10_500);
}

#[test]
fn free_collateral_can_be_withdrawn() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    let col = collateral_client(&env, &actors);
    client.deposit_collateral(&actors.user, &10_000);
    let before = col.balance(&actors.user);

    client.withdraw_collateral(&actors.user, &2_000);
    assert_eq!(client.collateral_of(&actors.user), 8_000);
    assert_eq!(col.balance(&actors.user) - before, 2_000);
    assert_eq!(client.get_state().total_collateral, 8_000);
}

#[test]
fn collateral_movements_validate_amounts() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    assert!(client.try_deposit_collateral(&actors.user, &0).is_err());
    assert!(client
        .try_withdraw_collateral(&actors.user, &1)
        .is_err());

    client.deposit_collateral(&actors.user, &1_000);
    assert!(client
        .try_withdraw_collateral(&actors.user, &1_001)
        .is_err());
}

// ----------------------------------------------------------------------
// Minting
// ----------------------------------------------------------------------

#[test]
fn minting_credits_the_holder() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    client.deposit_collateral(&actors.user, &10_000);
    let balance = client.mint(&actors.user, &9_000);

    assert_eq!(balance, 9_000);
    assert_eq!(client.balance_of(&actors.user), 9_000);
    assert_eq!(client.collateral_of(&actors.user), 10_000);
    assert_eq!(client.collateral_ratio(&actors.user), 11_111);
    assert_eq!(client.get_state().total_supply, 9_000);
    assert_eq!(client.reserve_ratio(), 11_111);
}

#[test]
fn minting_respects_the_per_user_floor() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    client.deposit_collateral(&actors.user, &10_000);
    // 10_000 collateral at 110% supports 9090.
    assert!(client.try_mint(&actors.user, &9_091).is_err());
    assert_eq!(client.balance_of(&actors.user), 0);

    client.mint(&actors.user, &9_090);
    assert_eq!(client.balance_of(&actors.user), 9_090);
}

#[test]
fn the_system_wide_floor_binds_separately() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StableCoin);
    let client = StableCoinClient::new(&env, &contract_id);
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let admin = Address::generate(&env);
    let pool = Address::generate(&env);
    let user = Address::generate(&env);
    let other = Address::generate(&env);

    let collateral = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let admin_client = StellarAssetClient::new(&env, &collateral);
    admin_client.mint(&user, &10_000_000);
    admin_client.mint(&other, &10_000_000);

    // The system floor is set above the per-user floor on purpose.
    client.initialize(&admin, &collateral, &pool, &11_000, &12_000, &0, &LIQ_BONUS_BPS);

    client.deposit_collateral(&user, &10_000);
    client.deposit_collateral(&other, &10_000);
    client.mint(&user, &9_090);
    assert!(!client.try_mint(&other, &9_090).is_err());

    // Both holders are individually fine, but together they breach the 120%
    // system floor.
    assert!(client.try_mint(&other, &100).is_err());
    assert_eq!(client.balance_of(&other), 9_090);
}

#[test]
fn minting_requires_collateral() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    assert!(client.try_mint(&actors.user, &1_000).is_err());
    client.deposit_collateral(&actors.user, &1_000);
    assert!(client.try_mint(&actors.user, &1_000).is_err());
}

#[test]
fn withdrawal_cannot_breach_the_floors() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    client.deposit_collateral(&actors.user, &10_000);
    client.mint(&actors.user, &9_000);

    // Anything past 100 units of slack drops under 110%.
    assert!(client.try_withdraw_collateral(&actors.user, &1_000).is_err());
    assert!(client.try_withdraw_collateral(&actors.user, &500).is_err());
    assert_eq!(client.collateral_of(&actors.user), 10_000);

    client.withdraw_collateral(&actors.user, &100);
    assert_eq!(client.collateral_of(&actors.user), 9_900);
    assert_eq!(client.collateral_ratio(&actors.user), 11_000);
}

// ----------------------------------------------------------------------
// Burning and redemption
// ----------------------------------------------------------------------

#[test]
fn burning_shrinks_the_supply_without_moving_collateral() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    client.deposit_collateral(&actors.user, &10_000);
    client.mint(&actors.user, &9_000);

    let remaining = client.burn(&actors.user, &1_000);
    assert_eq!(remaining, 8_000);
    assert_eq!(client.balance_of(&actors.user), 8_000);
    assert_eq!(client.collateral_of(&actors.user), 10_000);
    assert_eq!(client.get_state().total_supply, 8_000);
    assert_eq!(client.collateral_ratio(&actors.user), 12_500);
}

#[test]
fn burning_cannot_exceed_the_balance() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    client.deposit_collateral(&actors.user, &10_000);
    client.mint(&actors.user, &9_000);

    assert!(client.try_burn(&actors.user, &9_001).is_err());
    assert!(client.try_burn(&actors.user, &0).is_err());
    assert!(client.try_burn(&actors.other, &1).is_err());
}

#[test]
fn redeeming_returns_collateral_proportionally() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    client.deposit_collateral(&actors.user, &10_000);
    client.mint(&actors.user, &9_000);

    let col = collateral_client(&env, &actors);
    let before = col.balance(&actors.user);

    // Half the position comes back, along with half the collateral.
    let out = client.redeem(&actors.user, &4_500);
    assert_eq!(out, 5_000);
    assert_eq!(col.balance(&actors.user) - before, 5_000);

    assert_eq!(client.balance_of(&actors.user), 4_500);
    assert_eq!(client.collateral_of(&actors.user), 5_000);
    assert_eq!(client.get_state().total_supply, 4_500);
    assert_eq!(client.get_state().total_collateral, 5_000);
    // Redeeming pro rata leaves the ratio untouched.
    assert_eq!(client.collateral_ratio(&actors.user), 11_111);
}

#[test]
fn redeeming_the_whole_position_empties_it() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    client.deposit_collateral(&actors.user, &10_000);
    client.mint(&actors.user, &9_000);

    assert_eq!(client.redeem(&actors.user, &9_000), 10_000);
    assert_eq!(client.balance_of(&actors.user), 0);
    assert_eq!(client.collateral_of(&actors.user), 0);
    assert_eq!(client.get_state().total_supply, 0);
    assert_eq!(client.reserve_ratio(), u32::MAX);
}

#[test]
fn redeeming_cannot_exceed_the_balance() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    client.deposit_collateral(&actors.user, &10_000);
    client.mint(&actors.user, &9_000);

    assert!(client.try_redeem(&actors.user, &9_001).is_err());
    assert!(client.try_redeem(&actors.user, &0).is_err());
}

// ----------------------------------------------------------------------
// Peg protection
// ----------------------------------------------------------------------

#[test]
fn a_market_at_par_charges_no_fee() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    assert!(!client.is_peg_broken());
    assert_eq!(client.stability_fee_for(&1_000), 0);

    client.deposit_collateral(&actors.user, &10_000);
    client.mint(&actors.user, &1_000);
    assert_eq!(client.pool_collateral(), 0);
}

#[test]
fn a_depeg_charges_a_stability_fee_into_the_pool() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    // Market at 0.90: a 10% shortfall on 1000 mints a 20% fee, so 20.
    client.set_market_price(&(peg(9) / 10));
    assert!(client.is_peg_broken());
    assert_eq!(client.stability_fee_for(&1_000), 20);

    client.deposit_collateral(&actors.user, &10_000);
    client.mint(&actors.user, &1_000);

    // The fee leaves the holder and lands in the stability pool.
    assert_eq!(client.collateral_of(&actors.user), 9_980);
    assert_eq!(client.balance_of(&actors.user), 1_000);
    assert_eq!(client.pool_collateral(), 20);
    assert_eq!(client.get_state().total_collateral, 9_980);
}

#[test]
fn recovery_clears_the_depeg() {
    let env = Env::default();
    let (client, _actors) = setup(&env);

    client.set_market_price(&(peg(9) / 10));
    assert!(client.is_peg_broken());

    client.set_market_price(&peg(1));
    assert!(!client.is_peg_broken());
    assert_eq!(client.stability_fee_for(&1_000), 0);
}

#[test]
fn the_price_feed_is_range_checked() {
    let env = Env::default();
    let (client, _actors) = setup(&env);

    assert!(client.try_set_market_price(&0).is_err());
    // Beyond a 4x band around the peg is treated as a bad feed reading.
    assert!(client.try_set_market_price(&peg(5)).is_err());
    assert!(client.try_set_market_price(&(peg(1) / 5)).is_err());

    client.set_market_price(&peg(2));
    assert_eq!(client.get_state().market_price, peg(2));
}

#[test]
fn lowering_the_peg_clamps_the_market_price() {
    let env = Env::default();
    let (client, _) = setup(&env);

    assert!(client.try_set_peg(&0).is_err());

    client.set_peg(&(peg(1) / 2));
    let state = client.get_state();
    assert_eq!(state.peg, peg(1) / 2);
    // The market can never be marked above the peg.
    assert_eq!(state.market_price, peg(1) / 2);
    assert!(!client.is_peg_broken());
}

// ----------------------------------------------------------------------
// Stability pool
// ----------------------------------------------------------------------

#[test]
fn the_pool_issues_and_burns_shares() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    let col = collateral_client(&env, &actors);
    let first = client.deposit_to_stability_pool(&actors.user, &1_000);
    assert_eq!(first, 1_000);
    assert_eq!(col.balance(&actors.user), 10_000_000 - 1_000);
    assert_eq!(client.pool_shares(&actors.user), 1_000);
    assert_eq!(client.pool_share_supply(), 1_000);
    assert_eq!(client.pool_collateral(), 1_000);

    // A second depositor is priced against the existing supply.
    let second = client.deposit_to_stability_pool(&actors.other, &2_000);
    assert_eq!(second, 2_000);
    assert_eq!(client.pool_collateral(), 3_000);
    assert_eq!(client.pool_share_supply(), 3_000);

    let before = col.balance(&actors.user);
    let out = client.withdraw_from_stability_pool(&actors.user, &1_000);
    assert_eq!(out, 1_000);
    assert_eq!(col.balance(&actors.user) - before, 1_000);
    assert_eq!(client.pool_shares(&actors.user), 0);
    assert_eq!(client.pool_collateral(), 2_000);
    assert_eq!(client.pool_share_supply(), 2_000);
}

#[test]
fn pool_withdrawals_validate_shares() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    client.deposit_to_stability_pool(&actors.user, &1_000);

    assert!(client.try_withdraw_from_stability_pool(&actors.user, &0).is_err());
    assert!(client
        .try_withdraw_from_stability_pool(&actors.user, &1_001)
        .is_err());
    assert!(client
        .try_withdraw_from_stability_pool(&actors.other, &1)
        .is_err());
}

// ----------------------------------------------------------------------
// Emergency pause
// ----------------------------------------------------------------------

#[test]
fn pause_stops_every_state_change() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    client.deposit_collateral(&actors.user, &10_000);
    client.mint(&actors.user, &9_000);
    client.deposit_to_stability_pool(&actors.other, &1_000);

    client.pause();
    assert!(client.is_paused());

    assert!(client.try_deposit_collateral(&actors.user, &1).is_err());
    assert!(client.try_withdraw_collateral(&actors.user, &1).is_err());
    assert!(client.try_mint(&actors.user, &1).is_err());
    assert!(client.try_burn(&actors.user, &1).is_err());
    assert!(client.try_redeem(&actors.user, &1).is_err());
    assert!(client
        .try_deposit_to_stability_pool(&actors.user, &1)
        .is_err());
    assert!(client
        .try_withdraw_from_stability_pool(&actors.user, &1)
        .is_err());
    assert!(client
        .try_liquidate(&actors.liquidator, &actors.user, &1_000)
        .is_err());

    // Reads keep working while paused.
    assert_eq!(client.balance_of(&actors.user), 9_000);
    assert_eq!(client.collateral_ratio(&actors.user), 11_111);

    client.unpause();
    assert!(!client.is_paused());
    client.burn(&actors.user, &1_000);
    assert_eq!(client.balance_of(&actors.user), 8_000);
}

// ----------------------------------------------------------------------
// Liquidation
// ----------------------------------------------------------------------

/// Open a position and then tighten the risk parameters until it is unhealthy.
fn open_underwater(_env: &Env, client: &StableCoinClient, actors: &Actors) {
    client.deposit_collateral(&actors.user, &10_000);
    client.mint(&actors.user, &9_000);
    // 10_000 / 9_000 is 111%, healthy at 110% but not at 120%.
    client.set_parameters(&12_000, &MIN_RESERVE_BPS, &STABILITY_FEE_BPS, &LIQ_BONUS_BPS);
}

#[test]
fn a_healthy_position_cannot_be_liquidated() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    client.deposit_collateral(&actors.user, &10_000);
    client.mint(&actors.user, &9_000);
    assert!(!client.is_liquidatable(&actors.user));
    assert!(client
        .try_liquidate(&actors.liquidator, &actors.user, &1_000)
        .is_err());
}

#[test]
fn liquidation_pays_a_bonus_out_of_the_borrower() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    open_underwater(&env, &client, &actors);

    assert!(client.is_liquidatable(&actors.user));

    let col = collateral_client(&env, &actors);
    let before = col.balance(&actors.liquidator);

    // Repaying 1000 of 9000 is worth 10000 * 1000 / 9000 = 1111 of collateral,
    // grossed up by the 5% bonus to 1166.
    let seized = client.liquidate(&actors.liquidator, &actors.user, &1_000);
    assert_eq!(seized, 1_166);
    assert_eq!(col.balance(&actors.liquidator) - before, 1_166);

    assert_eq!(client.balance_of(&actors.user), 8_000);
    assert_eq!(client.collateral_of(&actors.user), 8_834);
    assert_eq!(client.get_state().total_supply, 8_000);
    assert_eq!(client.get_state().total_collateral, 8_834);

    // The 55 bonus is skimmed to the pool, and nothing was left uncovered.
    assert_eq!(client.pool_collateral(), 55);
    assert_eq!(client.pool_bad_debt(), 0);
}

#[test]
fn a_full_liquidation_clears_the_position() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    open_underwater(&env, &client, &actors);

    let seized = client.liquidate(&actors.liquidator, &actors.user, &99_999);
    assert_eq!(client.balance_of(&actors.user), 0);
    assert!(seized > 0 && seized <= 10_000, "seized {}", seized);
    assert_eq!(client.get_state().total_supply, 0);
}

#[test]
fn liquidation_validates_its_arguments() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    open_underwater(&env, &client, &actors);

    assert!(client
        .try_liquidate(&actors.liquidator, &actors.user, &0)
        .is_err());
    // Self-liquidation is pointless.
    assert!(client
        .try_liquidate(&actors.user, &actors.user, &1_000)
        .is_err());
    // A position with no debt has nothing to repay.
    assert!(client
        .try_liquidate(&actors.liquidator, &actors.other, &1_000)
        .is_err());
}

#[test]
fn a_fully_collateralised_user_is_not_liquidatable() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    client.deposit_collateral(&actors.user, &10_000);
    assert_eq!(client.balance_of(&actors.user), 0);
    assert!(!client.is_liquidatable(&actors.user));
    assert_eq!(client.collateral_ratio(&actors.user), u32::MAX);
}
