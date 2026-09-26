#![cfg(test)]

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
};

const DAY: u64 = 86_400;

/// 200% minimum collateralisation, 150% liquidation trigger, 10% penalty,
/// prices older than a day are stale.
const MIN_COLLATERAL_BPS: u32 = 20_000;
const LIQ_THRESHOLD_BPS: u32 = 15_000;
const LIQ_PENALTY_BPS: u32 = 1_000;
const MAX_PRICE_AGE: u64 = DAY;

struct Actors {
    admin: Address,
    oracle: Address,
    user: Address,
    liquidator: Address,
    synth: Address,
    collateral: Address,
}

fn q9(x: i128) -> i128 {
    x * SCALE
}

fn setup<'a>(env: &'a Env) -> (SyntheticAssetsClient<'a>, Actors) {
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let contract_id = env.register_contract(None, SyntheticAssets);
    let client = SyntheticAssetsClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let oracle = Address::generate(env);
    let user = Address::generate(env);
    let liquidator = Address::generate(env);

    let synth_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let collateral_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let synth_admin = StellarAssetClient::new(env, &synth_id);
    let collateral_admin = StellarAssetClient::new(env, &collateral_id);

    for who in [user.clone(), liquidator.clone()] {
        collateral_admin.mint(&who, &10_000_000);
        synth_admin.mint(&liquidator, &1_000_000);
    }

    client.initialize(
        &admin,
        &oracle,
        &MIN_COLLATERAL_BPS,
        &LIQ_THRESHOLD_BPS,
        &LIQ_PENALTY_BPS,
        &MAX_PRICE_AGE,
    );

    let actors = Actors {
        admin,
        oracle,
        user,
        liquidator,
        synth: synth_id,
        collateral: collateral_id,
    };
    (client, actors)
}

/// Register a synth struck 1:1 with its collateral and fund its liquidity.
fn register(_env: &Env, client: &SyntheticAssetsClient, actors: &Actors) -> u64 {
    let id = client.register_synth(
        &actors.synth,
        &actors.collateral,
        &symbol_short!("sUSD"),
        &q9(1),
    );
    client.add_synth_liquidity(&actors.admin, &id, &1_000_000);
    id
}

fn collateral_client<'a>(env: &'a Env, actors: &Actors) -> TokenClient<'a> {
    TokenClient::new(env, &actors.collateral)
}

fn synth_client<'a>(env: &'a Env, actors: &Actors) -> TokenClient<'a> {
    TokenClient::new(env, &actors.synth)
}

fn advance(env: &Env, seconds: u64) {
    let target = env.ledger().timestamp() + seconds;
    env.ledger().with_mut(|l| l.timestamp = target);
}

// ----------------------------------------------------------------------
// Setup and registry
// ----------------------------------------------------------------------

#[test]
fn initialize_is_one_shot_and_validates_parameters() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    assert!(client
        .try_initialize(
            &actors.admin,
            &actors.oracle,
            &MIN_COLLATERAL_BPS,
            &LIQ_THRESHOLD_BPS,
            &LIQ_PENALTY_BPS,
            &MAX_PRICE_AGE
        )
        .is_err());

    let config = client.get_config();
    assert_eq!(config.admin, actors.admin);
    assert_eq!(config.oracle, actors.oracle);
    assert_eq!(config.min_collateral_bps, MIN_COLLATERAL_BPS);
    assert_eq!(config.liquidation_threshold_bps, LIQ_THRESHOLD_BPS);
    assert_eq!(config.liquidation_penalty_bps, LIQ_PENALTY_BPS);
    assert_eq!(config.max_price_age, MAX_PRICE_AGE);
}

#[test]
fn initialize_rejects_incoherent_risk_parameters() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SyntheticAssets);
    let client = SyntheticAssetsClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);

    // A liquidation trigger at or above the minimum leaves no buffer.
    assert!(client
        .try_initialize(&admin, &oracle, &20_000, &20_000, &1_000, &DAY)
        .is_err());
    // Zero minimum collateralisation.
    assert!(client
        .try_initialize(&admin, &oracle, &0, &15_000, &1_000, &DAY)
        .is_err());
    // A penalty of 100% or more.
    assert!(client
        .try_initialize(&admin, &oracle, &20_000, &15_000, &10_000, &DAY)
        .is_err());
}

#[test]
fn register_synth_records_the_bookkeeping() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    let id = client.register_synth(
        &actors.synth,
        &actors.collateral,
        &symbol_short!("sUSD"),
        &q9(1),
    );
    assert_eq!(id, 1);
    assert_eq!(client.synth_count(), 1);
    assert_eq!(client.all_synths().len(), 1);

    let synth = client.get_synth(&id);
    assert_eq!(synth.synth, actors.synth);
    assert_eq!(synth.collateral, actors.collateral);
    assert_eq!(synth.symbol, symbol_short!("sUSD"));
    assert_eq!(synth.price, q9(1));
    assert_eq!(synth.total_debt, 0);
    assert_eq!(synth.liquidity, 0);
    assert!(synth.active);
}

#[test]
fn register_synth_rejects_bad_input() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    // A synth cannot be its own collateral.
    assert!(client
        .try_register_synth(
            &actors.collateral,
            &actors.collateral,
            &symbol_short!("sUSD"),
            &q9(1)
        )
        .is_err());
    // The price must be positive.
    assert!(client
        .try_register_synth(&actors.synth, &actors.collateral, &symbol_short!("sUSD"), &0)
        .is_err());
    assert_eq!(client.synth_count(), 0);
}

#[test]
fn admin_and_oracle_can_be_rotated() {
    let env = Env::default();
    let (client, _actors) = setup(&env);

    let new_oracle = Address::generate(&env);
    client.set_oracle(&new_oracle);
    assert_eq!(client.get_config().oracle, new_oracle);

    let new_admin = Address::generate(&env);
    client.set_admin(&new_admin);
    assert_eq!(client.get_config().admin, new_admin);
}

#[test]
fn parameters_can_be_retuned() {
    let env = Env::default();
    let (client, _) = setup(&env);

    assert!(client.try_set_parameters(&20_000, &20_000, &500, &DAY).is_err());

    client.set_parameters(&18_000, &12_000, &500, &(2 * DAY));
    let config = client.get_config();
    assert_eq!(config.min_collateral_bps, 18_000);
    assert_eq!(config.liquidation_threshold_bps, 12_000);
    assert_eq!(config.liquidation_penalty_bps, 500);
    assert_eq!(config.max_price_age, 2 * DAY);
}

// ----------------------------------------------------------------------
// Price feed
// ----------------------------------------------------------------------

#[test]
fn prices_can_be_pushed_by_the_oracle() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    assert!(client.try_update_price(&id, &0).is_err());
    assert!(client.try_update_price(&99, &q9(1)).is_err());

    client.update_price(&id, &q9(2));
    assert_eq!(client.get_price(&id), q9(2));
    assert_eq!(client.get_synth(&id).updated_at, env.ledger().timestamp());
}

#[test]
fn prices_go_stale_and_block_minting() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &10_000);
    assert!(!client.is_stale(&id));

    advance(&env, MAX_PRICE_AGE + 1);
    assert!(client.is_stale(&id));
    assert!(client.try_mint_synth(&actors.user, &id, &10).is_err());

    // A refresh clears the flag.
    client.update_price(&id, &q9(1));
    assert!(!client.is_stale(&id));
    client.mint_synth(&actors.user, &id, &10);
}

#[test]
fn a_zero_max_age_never_goes_stale() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);
    let contract_id = env.register_contract(None, SyntheticAssets);
    let client = SyntheticAssetsClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    client.initialize(
        &admin,
        &oracle,
        &MIN_COLLATERAL_BPS,
        &LIQ_THRESHOLD_BPS,
        &LIQ_PENALTY_BPS,
        &0,
    );

    let synth = Address::generate(&env);
    let collateral = Address::generate(&env);
    let id = client.register_synth(&synth, &collateral, &symbol_short!("sUSD"), &q9(1));

    advance(&env, 10 * DAY);
    assert!(!client.is_stale(&id));
}

// ----------------------------------------------------------------------
// Collateral
// ----------------------------------------------------------------------

#[test]
fn collateral_deposit_and_withdrawal_move_tokens() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    let col = collateral_client(&env, &actors);
    let before = col.balance(&actors.user);

    client.add_collateral(&actors.user, &id, &4_000);
    assert_eq!(before - col.balance(&actors.user), 4_000);
    assert_eq!(client.collateral_of(&actors.user, &id), 4_000);

    client.remove_collateral(&actors.user, &id, &1_000, &0);
    assert_eq!(client.collateral_of(&actors.user, &id), 3_000);
    assert_eq!(col.balance(&actors.user) - before, 3_000);
}

#[test]
fn collateral_movements_validate_amounts() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    assert!(client.try_add_collateral(&actors.user, &id, &0).is_err());
    assert!(client.try_remove_collateral(&actors.user, &id, &1, &0).is_err());
    assert!(client.try_add_collateral(&actors.user, &99, &1).is_err());
}

#[test]
fn collateral_cannot_be_withdrawn_from_below_the_floor() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    // 2000 collateral supports 1000 debt at 200%.
    client.add_collateral(&actors.user, &id, &2_000);
    client.mint_synth(&actors.user, &id, &1_000);
    assert_eq!(client.health_factor(&actors.user, &id), 20_000);

    // Dropping under 200% must fail, and so must an explicit floor.
    assert!(client
        .try_remove_collateral(&actors.user, &id, &1, &20_000)
        .is_err());
    assert!(client
        .try_remove_collateral(&actors.user, &id, &500, &0)
        .is_err());
    assert_eq!(client.collateral_of(&actors.user, &id), 2_000);

    // Free collateral can leave.
    client.remove_collateral(&actors.user, &id, &200, &0);
    assert_eq!(client.collateral_of(&actors.user, &id), 1_800);
}

// ----------------------------------------------------------------------
// Minting and burning
// ----------------------------------------------------------------------

#[test]
fn minting_moves_synth_and_records_debt() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &4_000);
    client.mint_synth(&actors.user, &id, &2_000);

    assert_eq!(client.debt_of(&actors.user, &id), 2_000);
    assert_eq!(client.get_synth(&id).total_debt, 2_000);
    assert_eq!(client.get_synth(&id).liquidity, 998_000);
    assert_eq!(synth_client(&env, &actors).balance(&actors.user), 2_000);

    let position = client.position(&actors.user, &id);
    assert_eq!(position.collateral, 4_000);
    assert_eq!(position.debt, 2_000);
    assert!(position.opened_at > 0);
}

#[test]
fn minting_cannot_exceed_borrowing_power() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &2_000);
    // 2000 collateral at 200% backs 1000 debt.
    assert!(client.try_mint_synth(&actors.user, &id, &1_001).is_err());
    assert_eq!(client.debt_of(&actors.user, &id), 0);

    client.mint_synth(&actors.user, &id, &1_000);
    assert_eq!(client.health_factor(&actors.user, &id), 20_000);
}

#[test]
fn minting_is_capped_by_contract_liquidity() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    // 10_000_000 of collateral could support 5_000_000 of debt, but the
    // contract only holds 1_000_000 synth.
    client.add_collateral(&actors.user, &id, &10_000_000);
    assert!(client.try_mint_synth(&actors.user, &id, &2_000_000).is_err());
    assert_eq!(client.debt_of(&actors.user, &id), 0);

    client.mint_synth(&actors.user, &id, &1_000_000);
    assert_eq!(client.get_synth(&id).liquidity, 0);
}

#[test]
fn minting_respects_the_active_flag() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &4_000);
    client.set_synth_active(&id, &false);
    assert!(client.try_mint_synth(&actors.user, &id, &1_000).is_err());

    client.set_synth_active(&id, &true);
    client.mint_synth(&actors.user, &id, &1_000);
}

#[test]
fn burning_returns_collateral_proportionally() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &4_000);
    client.mint_synth(&actors.user, &id, &2_000);

    let col = collateral_client(&env, &actors);
    let syn = synth_client(&env, &actors);
    let col_before = col.balance(&actors.user);
    let syn_before = syn.balance(&actors.user);

    // Half the debt back releases half the collateral.
    let released = client.burn_synth(&actors.user, &id, &1_000);
    assert_eq!(released, 2_000);
    assert_eq!(col.balance(&actors.user) - col_before, 2_000);
    assert_eq!(syn_before - syn.balance(&actors.user), 1_000);

    assert_eq!(client.debt_of(&actors.user, &id), 1_000);
    assert_eq!(client.collateral_of(&actors.user, &id), 2_000);
    assert_eq!(client.get_synth(&id).total_debt, 1_000);
    // A proportional burn preserves the ratio.
    assert_eq!(client.health_factor(&actors.user, &id), 20_000);
}

#[test]
fn burning_cannot_exceed_the_debt() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &4_000);
    client.mint_synth(&actors.user, &id, &2_000);

    assert!(client.try_burn_synth(&actors.user, &id, &2_001).is_err());
    assert!(client.try_burn_synth(&actors.user, &id, &0).is_err());
}

// ----------------------------------------------------------------------
// Price moves and health
// ----------------------------------------------------------------------

#[test]
fn a_rising_price_erodes_the_position() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &2_000);
    client.mint_synth(&actors.user, &id, &1_000);
    assert!(!client.is_liquidatable(&actors.user, &id));

    // The synth doubles, so the same collateral now backs half as much.
    client.update_price(&id, &q9(2));
    assert_eq!(client.health_factor(&actors.user, &id), 10_000);
    assert!(client.is_liquidatable(&actors.user, &id));
}

#[test]
fn a_falling_price_improves_the_position() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &2_000);
    client.mint_synth(&actors.user, &id, &1_000);

    client.update_price(&id, &(q9(1) / 2));
    // ratio = 2000 * SCALE * BPS / (1000 * 0.5) = 40000
    assert_eq!(client.health_factor(&actors.user, &id), 40_000);
    assert!(!client.is_liquidatable(&actors.user, &id));
}

#[test]
fn a_debt_free_position_is_always_healthy() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &1);
    assert_eq!(client.health_factor(&actors.user, &id), u32::MAX);
    assert!(!client.is_liquidatable(&actors.user, &id));
}

// ----------------------------------------------------------------------
// Rebalancing
// ----------------------------------------------------------------------

#[test]
fn target_ratio_must_respect_the_minimum() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    assert!(client
        .try_set_target_ratio(&actors.user, &id, &(MIN_COLLATERAL_BPS - 1))
        .is_err());

    client.set_target_ratio(&actors.user, &id, &30_000);
    assert_eq!(client.position(&actors.user, &id).target_bps, 30_000);
}

#[test]
fn rebalance_mints_against_idle_collateral() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    // 300% collateral, target 200%: 10000 collateral should end at 5000 debt.
    client.add_collateral(&actors.user, &id, &10_000);
    client.mint_synth(&actors.user, &id, &3_000);
    client.set_target_ratio(&actors.user, &id, &20_000);

    let moved = client.rebalance(&actors.user, &id);
    assert_eq!(moved, 2_000);
    assert_eq!(client.debt_of(&actors.user, &id), 5_000);
    assert_eq!(client.collateral_of(&actors.user, &id), 10_000);
    assert_eq!(client.health_factor(&actors.user, &id), 20_000);
    assert_eq!(synth_client(&env, &actors).balance(&actors.user), 5_000);
}

#[test]
fn rebalance_burns_to_restore_an_under_target_ratio() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &10_000);
    client.mint_synth(&actors.user, &id, &5_000);
    client.set_target_ratio(&actors.user, &id, &20_000);
    assert_eq!(client.rebalance(&actors.user, &id), 0);

    // The price rises 40%, dropping the ratio to about 142%.
    client.update_price(&id, &(q9(14) / 10));
    let before = client.health_factor(&actors.user, &id);
    assert!(before < 20_000);

    let col_before = collateral_client(&env, &actors).balance(&actors.user);
    let syn_before = synth_client(&env, &actors).balance(&actors.user);

    let moved = client.rebalance(&actors.user, &id);
    assert!(moved < 0, "expected a burn, got {}", moved);

    // The holder pays synth in and gets collateral back.
    assert_eq!(client.debt_of(&actors.user, &id), 5_000 + moved);
    assert_eq!(syn_before - synth_client(&env, &actors).balance(&actors.user), -moved);
    assert!(collateral_client(&env, &actors).balance(&actors.user) > col_before);

    // Collateral is released at integer precision, so the ratio lands just
    // short of the target rather than exactly on it.
    let after = client.health_factor(&actors.user, &id);
    assert!(after > before, "{} -> {}", before, after);
    assert!(after <= 20_000, "{}", after);
    assert!(after > 15_000, "{}", after);
}

#[test]
fn rebalance_is_idempotent_at_target() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &10_000);
    client.mint_synth(&actors.user, &id, &5_000);
    client.set_target_ratio(&actors.user, &id, &20_000);

    assert_eq!(client.rebalance(&actors.user, &id), 0);
    assert_eq!(client.rebalance(&actors.user, &id), 0);
}

#[test]
fn rebalance_requires_a_target_and_a_position() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &10_000);
    client.mint_synth(&actors.user, &id, &5_000);
    assert!(client.try_rebalance(&actors.user, &id).is_err());

    // An empty position cannot be rebalanced.
    client.set_target_ratio(&actors.liquidator, &id, &20_000);
    assert!(client.try_rebalance(&actors.liquidator, &id).is_err());
}

#[test]
fn rebalance_is_capped_by_contract_liquidity() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SyntheticAssets);
    let client = SyntheticAssetsClient::new(&env, &contract_id);
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(
        &admin,
        &oracle,
        &MIN_COLLATERAL_BPS,
        &LIQ_THRESHOLD_BPS,
        &LIQ_PENALTY_BPS,
        &MAX_PRICE_AGE,
    );

    let synth = Address::generate(&env);
    let collateral = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    StellarAssetClient::new(&env, &collateral).mint(&user, &10_000_000);

    let id = client.register_synth(&synth, &collateral, &symbol_short!("sUSD"), &q9(1));
    client.add_synth_liquidity(&admin, &id, &100);

    client.add_collateral(&user, &id, &10_000);
    client.set_target_ratio(&user, &id, &20_000);
    // It wants to mint 5000 but the contract only holds 100 synth.
    assert!(client.try_rebalance(&user, &id).is_err());
}

// ----------------------------------------------------------------------
// Liquidation
// ----------------------------------------------------------------------

#[test]
fn a_healthy_position_cannot_be_liquidated() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &2_000);
    client.mint_synth(&actors.user, &id, &1_000);

    assert!(client
        .try_liquidate(&actors.liquidator, &actors.user, &id, &500)
        .is_err());
}

#[test]
fn liquidation_takes_collateral_plus_the_penalty() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &2_000);
    client.mint_synth(&actors.user, &id, &1_000);
    // The synth doubles: 2000 of collateral now backs 2000 of debt, 100%.
    client.update_price(&id, &q9(2));
    assert_eq!(client.health_factor(&actors.user, &id), 10_000);

    let syn = synth_client(&env, &actors);
    let col = collateral_client(&env, &actors);
    let syn_before = syn.balance(&actors.liquidator);
    let col_before = col.balance(&actors.liquidator);

    // Repay 500 of debt worth 1000 of collateral, plus the 10% penalty.
    let seized = client.liquidate(&actors.liquidator, &actors.user, &id, &500);
    assert_eq!(seized, 1_100);

    assert_eq!(syn_before - syn.balance(&actors.liquidator), 500);
    assert_eq!(col.balance(&actors.liquidator) - col_before, 1_100);

    assert_eq!(client.debt_of(&actors.user, &id), 500);
    assert_eq!(client.collateral_of(&actors.user, &id), 900);
    assert_eq!(client.get_synth(&id).total_debt, 500);
}

#[test]
fn liquidation_caps_at_the_debt_and_the_collateral() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &2_000);
    client.mint_synth(&actors.user, &id, &1_000);
    client.update_price(&id, &q9(4)); // 50% ratio

    // Repaying more than is owed only clears the debt, and the seizure is
    // capped by the collateral actually held.
    let seized = client.liquidate(&actors.liquidator, &actors.user, &id, &99_999);
    assert_eq!(client.debt_of(&actors.user, &id), 0);
    assert_eq!(client.collateral_of(&actors.user, &id), 0);
    assert_eq!(seized, 2_000);
}

#[test]
fn liquidation_works_on_a_stale_price() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &2_000);
    client.mint_synth(&actors.user, &id, &1_000);
    client.update_price(&id, &q9(2));

    // The oracle goes quiet: minting freezes but liquidations must not.
    advance(&env, MAX_PRICE_AGE + 1);
    assert!(client.is_stale(&id));
    assert!(client.try_mint_synth(&actors.user, &id, &1).is_err());

    let seized = client.liquidate(&actors.liquidator, &actors.user, &id, &500);
    assert_eq!(seized, 1_100);
}

#[test]
fn liquidation_validates_its_arguments() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &2_000);
    client.mint_synth(&actors.user, &id, &1_000);
    client.update_price(&id, &q9(2));

    assert!(client
        .try_liquidate(&actors.liquidator, &actors.user, &id, &0)
        .is_err());
    // Self-liquidation is pointless.
    assert!(client
        .try_liquidate(&actors.user, &actors.user, &id, &100)
        .is_err());
    // Nothing to take from.
    assert!(client
        .try_liquidate(&actors.liquidator, &actors.liquidator, &id, &100)
        .is_err());
}

#[test]
fn a_fully_collateralised_user_cannot_be_liquidated() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = register(&env, &client, &actors);

    client.add_collateral(&actors.user, &id, &2_000);
    assert_eq!(client.debt_of(&actors.user, &id), 0);
    assert!(client
        .try_liquidate(&actors.liquidator, &actors.user, &id, &100)
        .is_err());
}
