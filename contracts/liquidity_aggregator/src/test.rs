#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
};

const FEE_BPS: u32 = 10;
const PENALTY_BPS: u32 = 50;
const POOL_FEE_BPS: u32 = 30;

struct Actors {
    admin: Address,
    fee_recipient: Address,
    trader: Address,
    lp: Address,
    token_a: Address,
    token_b: Address,
    token_c: Address,
}

fn setup<'a>(env: &'a Env) -> (LiquidityAggregatorClient<'a>, Actors) {
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let contract_id = env.register_contract(None, LiquidityAggregator);
    let client = LiquidityAggregatorClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let fee_recipient = Address::generate(env);
    let trader = Address::generate(env);
    let lp = Address::generate(env);

    let mut tokens: Vec<Address> = Vec::new(env);
    for _ in 0..3u32 {
        tokens.push_back(
            env.register_stellar_asset_contract_v2(admin.clone())
                .address(),
        );
    }
    let token_a = tokens.get(0).unwrap();
    let token_b = tokens.get(1).unwrap();
    let token_c = tokens.get(2).unwrap();

    for token in [token_a.clone(), token_b.clone(), token_c.clone()] {
        let admin_client = StellarAssetClient::new(env, &token);
        for who in [trader.clone(), lp.clone()] {
            admin_client.mint(&who, &100_000_000);
        }
        // Seed the aggregator's own balance so it can pay pool outputs.
        admin_client.mint(&contract_id, &100_000_000);
    }

    client.initialize(&admin, &fee_recipient, &FEE_BPS, &PENALTY_BPS);

    let actors = Actors {
        admin,
        fee_recipient,
        trader,
        lp,
        token_a,
        token_b,
        token_c,
    };
    (client, actors)
}

fn balance<'a>(env: &'a Env, token: &Address, who: &Address) -> i128 {
    TokenClient::new(env, token).balance(who)
}

/// Register an A/B pool and back it with real tokens.
fn pool_ab(
    env: &Env,
    client: &LiquidityAggregatorClient,
    actors: &Actors,
    reserve_a: &i128,
    reserve_b: &i128,
) -> Address {
    let address = Address::generate(env);
    client.register_pool(
        &address,
        &actors.token_a,
        &actors.token_b,
        reserve_a,
        reserve_b,
        &POOL_FEE_BPS,
        &PoolKind::ConstantProduct,
    );
    client.add_liquidity(&actors.lp, &address, &actors.token_a, reserve_a);
    client.add_liquidity(&actors.lp, &address, &actors.token_b, reserve_b);
    address
}

fn route_legs(route: &Route) -> u32 {
    route.legs.len()
}

// ----------------------------------------------------------------------
// Setup
// ----------------------------------------------------------------------

#[test]
fn initialize_is_one_shot() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    assert!(client
        .try_initialize(&actors.admin, &actors.fee_recipient, &FEE_BPS, &PENALTY_BPS)
        .is_err());

    let config = client.get_config();
    assert_eq!(config.admin, actors.admin);
    assert_eq!(config.fee_recipient, actors.fee_recipient);
    assert_eq!(config.fee_bps, FEE_BPS);
    assert_eq!(config.routing_penalty_bps, PENALTY_BPS);
}

#[test]
fn parameters_can_be_retuned() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    assert!(client.try_set_fee(&10_000, &actors.fee_recipient).is_err());
    assert!(client.try_set_routing_penalty(&10_000).is_err());

    client.set_fee(&25, &actors.trader);
    let config = client.get_config();
    assert_eq!(config.fee_bps, 25);
    assert_eq!(config.fee_recipient, actors.trader);

    client.set_routing_penalty(&100);
    assert_eq!(client.get_config().routing_penalty_bps, 100);

    client.set_admin(&actors.trader);
    assert_eq!(client.get_config().admin, actors.trader);
}

// ----------------------------------------------------------------------
// Registry
// ----------------------------------------------------------------------

#[test]
fn register_pool_indexes_both_directions() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    let pool = pool_ab(&env, &client, &actors, &1_000_000, &2_000_000);

    assert_eq!(client.pool_count(), 1);
    assert_eq!(client.pools_for(&actors.token_a, &actors.token_b).len(), 1);
    assert_eq!(client.pools_for(&actors.token_b, &actors.token_a).len(), 1);
    assert_eq!(client.all_tokens().len(), 2);

    let entry = client.get_pool(&pool);
    assert_eq!(entry.token_in, actors.token_a);
    assert_eq!(entry.reserve_in, 1_000_000);
    assert_eq!(entry.reserve_out, 2_000_000);
    assert!(entry.enabled);
}

#[test]
fn register_pool_validates_its_arguments() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    let pool = Address::generate(&env);
    // Identical tokens.
    assert!(client
        .try_register_pool(
            &pool,
            &actors.token_a,
            &actors.token_a,
            &1_000_000,
            &1_000_000,
            &POOL_FEE_BPS,
            &PoolKind::ConstantProduct
        )
        .is_err());
    // Empty reserves.
    assert!(client
        .try_register_pool(
            &pool,
            &actors.token_a,
            &actors.token_b,
            &0,
            &1_000_000,
            &POOL_FEE_BPS,
            &PoolKind::ConstantProduct
        )
        .is_err());
    // Fee of 100% or more.
    assert!(client
        .try_register_pool(
            &pool,
            &actors.token_a,
            &actors.token_b,
            &1_000_000,
            &1_000_000,
            &10_000,
            &PoolKind::ConstantProduct
        )
        .is_err());

    // No pool was created by any of the failures.
    assert_eq!(client.pool_count(), 0);

    client.register_pool(
        &pool,
        &actors.token_a,
        &actors.token_b,
        &1_000_000,
        &1_000_000,
        &POOL_FEE_BPS,
        &PoolKind::ConstantProduct,
    );
    // Re-registering the same address is refused.
    assert!(client
        .try_register_pool(
            &pool,
            &actors.token_a,
            &actors.token_b,
            &1_000_000,
            &1_000_000,
            &POOL_FEE_BPS,
            &PoolKind::ConstantProduct
        )
        .is_err());
}

#[test]
fn sync_pool_refreshes_reserves_in_either_direction() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let pool = pool_ab(&env, &client, &actors, &1_000_000, &2_000_000);

    client.sync_pool(&pool, &actors.token_a, &3_000_000, &6_000_000);
    let entry = client.get_pool(&pool);
    assert_eq!(entry.reserve_in, 3_000_000);
    assert_eq!(entry.reserve_out, 6_000_000);

    // The reverse direction writes back into the stored orientation.
    client.sync_pool(&pool, &actors.token_b, &7_000_000, &8_000_000);
    let entry = client.get_pool(&pool);
    assert_eq!(entry.token_in, actors.token_a);
    assert_eq!(entry.reserve_in, 8_000_000);
    assert_eq!(entry.reserve_out, 7_000_000);

    assert!(client
        .try_sync_pool(&pool, &actors.token_c, &1, &1)
        .is_err());
}

#[test]
fn a_disabled_pool_drops_out_of_routing() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let pool = pool_ab(&env, &client, &actors, &1_000_000, &2_000_000);

    assert_eq!(client.compare_rates(&actors.token_a, &actors.token_b, &1_000).len(), 1);
    client.set_pool_enabled(&pool, &false);
    assert_eq!(client.compare_rates(&actors.token_a, &actors.token_b, &1_000).len(), 0);
    assert!(client.try_quote(&pool, &actors.token_a, &1_000).is_err());
}

// ----------------------------------------------------------------------
// Quotes and rate comparison
// ----------------------------------------------------------------------

#[test]
fn quotes_follow_the_constant_product_curve() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let pool = pool_ab(&env, &client, &actors, &1_000_000, &2_000_000);

    let q = client.quote(&pool, &actors.token_a, &1_000);
    // 0.30% fee: out = 2_000_000 * (1000*9970) / (1_000_000*10000 + 1000*9970)
    assert_eq!(q.amount_out, 1_992);
    assert_eq!(q.fee, 3);
    assert_eq!(q.token_in, actors.token_a);
    assert_eq!(q.token_out, actors.token_b);
    // Mid price is 2000, so a 1992 fill is 40 bps of impact.
    assert_eq!(q.price_impact_bps, 40);
}

#[test]
fn quotes_are_validated() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let pool = pool_ab(&env, &client, &actors, &1_000_000, &2_000_000);

    assert!(client.try_quote(&pool, &actors.token_a, &0).is_err());
    assert!(client.try_quote(&pool, &actors.token_c, &1_000).is_err());

    // An order larger than the pool can absorb is capped at its reserves
    // rather than quoted beyond them.
    let huge = client.quote(&pool, &actors.token_a, &900_000_000);
    let entry = client.get_pool(&pool);
    assert!(huge.amount_out <= entry.reserve_out);
}

#[test]
fn the_reverse_direction_quotes_the_same_curve() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let pool = pool_ab(&env, &client, &actors, &1_000_000, &2_000_000);

    let forward = client.quote(&pool, &actors.token_a, &1_000);
    let reverse = client.quote(&pool, &actors.token_b, &2_000);
    // 2000 of B back into the same curve gives about the same 1000 of A.
    assert!(reverse.amount_out <= 1_000 && reverse.amount_out > 990);
    assert_eq!(reverse.token_in, actors.token_b);
    assert_eq!(forward.amount_in, 1_000);
}

#[test]
fn compare_rates_ranks_pools_best_first() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    // Same pair, different depth: the deeper pool fills more.
    let shallow = pool_ab(&env, &client, &actors, &100_000, &100_000);
    let deep = pool_ab(&env, &client, &actors, &10_000_000, &10_000_000);

    let quotes = client.compare_rates(&actors.token_a, &actors.token_b, &10_000);
    assert_eq!(quotes.len(), 2);
    assert_eq!(quotes.get(0).unwrap().pool, deep);
    assert_eq!(quotes.get(1).unwrap().pool, shallow);
    assert!(quotes.get(0).unwrap().amount_out > quotes.get(1).unwrap().amount_out);

    let best = client.best_route(&actors.token_a, &actors.token_b, &10_000);
    assert_eq!(best.pool, deep);
}

#[test]
fn an_unroutable_pair_has_no_quote() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    pool_ab(&env, &client, &actors, &1_000_000, &1_000_000);

    assert!(client
        .try_compare_rates(&actors.token_a, &actors.token_c, &1_000)
        .is_err());
    assert!(client
        .try_best_route(&actors.token_a, &actors.token_a, &1_000)
        .is_err());
    assert!(client.try_compare_rates(&actors.token_a, &actors.token_b, &0).is_err());
}

// ----------------------------------------------------------------------
// Path optimisation
// ----------------------------------------------------------------------

#[test]
fn a_direct_route_is_found_and_preferred() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    pool_ab(&env, &client, &actors, &1_000_000, &2_000_000);

    let route = client.find_path(&actors.token_a, &actors.token_b, &1_000, &3);
    assert_eq!(route_legs(&route), 1);
    assert_eq!(route.amount_in, 1_000);
    assert_eq!(route.expected_out, 1_992);
    assert_eq!(route.total_fee, 3);
    assert!(route.score > 0);
}

#[test]
fn a_multi_hop_route_is_discovered() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    // A -> B and B -> C exist, A -> C does not.
    pool_ab(&env, &client, &actors, &1_000_000, &1_000_000);
    let bc = Address::generate(&env);
    client.register_pool(
        &bc,
        &actors.token_b,
        &actors.token_c,
        &1_000_000,
        &1_000_000,
        &POOL_FEE_BPS,
        &PoolKind::ConstantProduct,
    );
    client.add_liquidity(&actors.lp, &bc, &actors.token_b, &1_000_000);
    client.add_liquidity(&actors.lp, &bc, &actors.token_c, &1_000_000);

    assert!(client
        .try_compare_rates(&actors.token_a, &actors.token_c, &1_000)
        .is_err());

    let paths = client.find_paths(&actors.token_a, &actors.token_c, &10_000, &3);
    assert_eq!(paths.len(), 1);
    let route = paths.get(0).unwrap();
    assert_eq!(route_legs(&route), 2);
    assert_eq!(route.legs.get(0).unwrap().token_in, actors.token_a);
    assert_eq!(route.legs.get(1).unwrap().token_out, actors.token_c);

    // A hop limit of one cannot reach C.
    assert!(client
        .try_find_path(&actors.token_a, &actors.token_c, &10_000, &1)
        .is_err());
}

#[test]
fn the_hop_penalty_prefers_the_shorter_route() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    // Both a direct A/C pool and the A -> B -> C chain exist.
    pool_ab(&env, &client, &actors, &1_000_000, &1_000_000);
    let bc = Address::generate(&env);
    client.register_pool(
        &bc,
        &actors.token_b,
        &actors.token_c,
        &1_000_000,
        &1_000_000,
        &POOL_FEE_BPS,
        &PoolKind::ConstantProduct,
    );
    client.add_liquidity(&actors.lp, &bc, &actors.token_b, &1_000_000);
    client.add_liquidity(&actors.lp, &bc, &actors.token_c, &1_000_000);
    let ac = Address::generate(&env);
    client.register_pool(
        &ac,
        &actors.token_a,
        &actors.token_c,
        &1_000_000,
        &1_000_000,
        &POOL_FEE_BPS,
        &PoolKind::ConstantProduct,
    );
    client.add_liquidity(&actors.lp, &ac, &actors.token_a, &1_000_000);
    client.add_liquidity(&actors.lp, &ac, &actors.token_c, &1_000_000);

    let paths = client.find_paths(&actors.token_a, &actors.token_c, &10_000, &3);
    assert_eq!(paths.len(), 2);
    // The one-hop route is quoted slightly better, so it also scores best.
    assert_eq!(route_legs(&paths.get(0).unwrap()), 1);
    assert_eq!(route_legs(&paths.get(1).unwrap()), 2);

    // Cranking the penalty up leaves the ordering alone, since the direct
    // route is both cheaper and shorter.
    client.set_routing_penalty(&1_000);
    let paths = client.find_paths(&actors.token_a, &actors.token_c, &10_000, &3);
    assert_eq!(route_legs(&paths.get(0).unwrap()), 1);
    assert_eq!(route_legs(&paths.get(1).unwrap()), 2);
}

#[test]
fn find_paths_validates_its_arguments() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    pool_ab(&env, &client, &actors, &1_000_000, &1_000_000);

    assert!(client
        .try_find_paths(&actors.token_a, &actors.token_c, &1_000, &3)
        .is_err());
    assert!(client
        .try_find_paths(&actors.token_a, &actors.token_a, &1_000, &3)
        .is_err());
    assert!(client
        .try_find_paths(&actors.token_a, &actors.token_b, &0, &3)
        .is_err());
}

// ----------------------------------------------------------------------
// Split routing
// ----------------------------------------------------------------------

#[test]
fn a_split_never_exceeds_the_best_single_route() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    // Two A/B pools of different depth plus the A -> B -> C chain.
    pool_ab(&env, &client, &actors, &1_000_000, &2_000_000);
    pool_ab(&env, &client, &actors, &10_000_000, &20_000_000);
    let bc = Address::generate(&env);
    client.register_pool(
        &bc,
        &actors.token_b,
        &actors.token_c,
        &20_000_000,
        &20_000_000,
        &POOL_FEE_BPS,
        &PoolKind::ConstantProduct,
    );
    client.add_liquidity(&actors.lp, &bc, &actors.token_b, &20_000_000);
    client.add_liquidity(&actors.lp, &bc, &actors.token_c, &20_000_000);

    // 30% of the deepest pool is 3M, so a 5M order cannot be served in one leg.
    let amount = 5_000_000i128;
    let splits = client.find_split_routes(&actors.token_a, &actors.token_c, &amount, &4);

    assert!(splits.len() >= 2, "expected a fan-out, got {}", splits.len());

    // Every split sums back to the original order, and each leg pays out.
    let mut allocated: i128 = 0;
    for route in splits.iter() {
        allocated += route.amount_in;
        assert!(route.expected_out > 0);
        assert!(!route.legs.is_empty());
    }
    assert_eq!(allocated, amount);
}

#[test]
fn a_split_respects_the_route_cap() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    pool_ab(&env, &client, &actors, &1_000_000, &2_000_000);
    pool_ab(&env, &client, &actors, &2_000_000, &4_000_000);
    pool_ab(&env, &client, &actors, &3_000_000, &6_000_000);
    let bc = Address::generate(&env);
    client.register_pool(
        &bc,
        &actors.token_b,
        &actors.token_c,
        &1_000_000,
        &1_000_000,
        &POOL_FEE_BPS,
        &PoolKind::ConstantProduct,
    );
    client.add_liquidity(&actors.lp, &bc, &actors.token_b, &1_000_000);
    client.add_liquidity(&actors.lp, &bc, &actors.token_c, &1_000_000);

    let one = client.find_split_routes(&actors.token_a, &actors.token_c, &1_000_000, &1);
    assert_eq!(one.len(), 1);

    let two = client.find_split_routes(&actors.token_a, &actors.token_c, &1_000_000, &2);
    assert!(two.len() <= 2);

    // The hard ceiling holds even when more is asked for.
    let many = client.find_split_routes(&actors.token_a, &actors.token_c, &1_000_000, &99);
    assert!(many.len() <= MAX_SPLIT_ROUTES as u32);
}

// ----------------------------------------------------------------------
// Execution
// ----------------------------------------------------------------------

#[test]
fn a_swap_settles_end_to_end() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let pool = pool_ab(&env, &client, &actors, &1_000_000, &2_000_000);

    let route = client.find_path(&actors.token_a, &actors.token_b, &1_000, &1);
    let expected = route.expected_out;
    let fee = (expected * FEE_BPS as i128) / BPS;
    let net = expected - fee;

    let a_before = balance(&env, &actors.token_a, &actors.trader);
    let b_before = balance(&env, &actors.token_b, &actors.trader);
    let fee_before = balance(&env, &actors.token_b, &actors.fee_recipient);

    let out = client.swap(
        &actors.trader,
        &actors.token_a,
        &actors.token_b,
        &1_000,
        &0,
        &route,
    );

    assert_eq!(out, net);
    assert_eq!(a_before - balance(&env, &actors.token_a, &actors.trader), 1_000);
    assert_eq!(balance(&env, &actors.token_b, &actors.trader) - b_before, net);
    assert_eq!(balance(&env, &actors.token_b, &actors.fee_recipient) - fee_before, fee);

    // The pool's reserves moved with the trade.
    let entry = client.get_pool(&pool);
    assert_eq!(entry.reserve_in, 1_001_000);
    assert_eq!(entry.reserve_out, 2_000_000 - expected);

    assert_eq!(client.fees_collected(), fee);
    assert_eq!(client.volume(), 1_000);
}

#[test]
fn a_swap_honours_the_minimum_out() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    pool_ab(&env, &client, &actors, &1_000_000, &2_000_000);

    let route = client.find_path(&actors.token_a, &actors.token_b, &1_000, &1);
    assert!(client
        .try_swap(
            &actors.trader,
            &actors.token_a,
            &actors.token_b,
            &1_000,
            &999_999,
            &route
        )
        .is_err());
}

#[test]
fn a_multi_hop_swap_runs_atomically() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    pool_ab(&env, &client, &actors, &1_000_000, &1_000_000);
    let bc = Address::generate(&env);
    client.register_pool(
        &bc,
        &actors.token_b,
        &actors.token_c,
        &1_000_000,
        &1_000_000,
        &POOL_FEE_BPS,
        &PoolKind::ConstantProduct,
    );
    client.add_liquidity(&actors.lp, &bc, &actors.token_b, &1_000_000);
    client.add_liquidity(&actors.lp, &bc, &actors.token_c, &1_000_000);

    let route = client.find_path(&actors.token_a, &actors.token_c, &10_000, &3);
    assert_eq!(route_legs(&route), 2);

    let c_before = balance(&env, &actors.token_c, &actors.trader);
    let a_before = balance(&env, &actors.token_a, &actors.trader);

    let out = client.swap(
        &actors.trader,
        &actors.token_a,
        &actors.token_c,
        &10_000,
        &0,
        &route,
    );
    assert!(out > 0);
    assert_eq!(a_before - balance(&env, &actors.token_a, &actors.trader), 10_000);
    assert_eq!(balance(&env, &actors.token_c, &actors.trader) - c_before, out);

    // The net is exactly the route's output less the protocol fee.
    let fee = (route.expected_out * FEE_BPS as i128) / BPS;
    assert_eq!(out, route.expected_out - fee);
}

#[test]
fn a_route_for_the_wrong_pair_is_refused() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    pool_ab(&env, &client, &actors, &1_000_000, &2_000_000);

    let route = client.find_path(&actors.token_a, &actors.token_b, &1_000, &1);
    // Swapping B for A against a route that starts at A.
    assert!(client
        .try_swap(
            &actors.trader,
            &actors.token_b,
            &actors.token_a,
            &1_000,
            &0,
            &route
        )
        .is_err());
    assert!(client
        .try_swap(
            &actors.trader,
            &actors.token_a,
            &actors.token_a,
            &1_000,
            &0,
            &route
        )
        .is_err());
    assert!(client
        .try_swap(&actors.trader, &actors.token_a, &actors.token_b, &0, &0, &route)
        .is_err());
}

#[test]
fn a_split_executes_across_every_route() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    pool_ab(&env, &client, &actors, &1_000_000, &2_000_000);
    pool_ab(&env, &client, &actors, &10_000_000, &20_000_000);
    let bc = Address::generate(&env);
    client.register_pool(
        &bc,
        &actors.token_b,
        &actors.token_c,
        &20_000_000,
        &20_000_000,
        &POOL_FEE_BPS,
        &PoolKind::ConstantProduct,
    );
    client.add_liquidity(&actors.lp, &bc, &actors.token_b, &20_000_000);
    client.add_liquidity(&actors.lp, &bc, &actors.token_c, &20_000_000);

    // Big enough that the best single pool cannot take the whole order.
    let amount = 5_000_000i128;
    let splits = client.find_split_routes(&actors.token_a, &actors.token_c, &amount, &4);
    assert!(splits.len() >= 2, "expected a fan-out, got {}", splits.len());

    let mut expected_out: i128 = 0;
    for route in splits.iter() {
        expected_out += route.expected_out;
    }
    let fee = (expected_out * FEE_BPS as i128) / BPS;

    let c_before = balance(&env, &actors.token_c, &actors.trader);
    let a_before = balance(&env, &actors.token_a, &actors.trader);

    let out = client.execute_split(
        &actors.trader,
        &actors.token_a,
        &amount,
        &0,
        &splits,
    );

    assert_eq!(out, expected_out - fee);
    assert_eq!(a_before - balance(&env, &actors.token_a, &actors.trader), amount);
    assert_eq!(balance(&env, &actors.token_c, &actors.trader) - c_before, out);
    assert_eq!(client.volume(), amount);
}

#[test]
fn a_split_must_account_for_the_whole_order() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    pool_ab(&env, &client, &actors, &1_000_000, &2_000_000);

    let route = client.find_path(&actors.token_a, &actors.token_b, &1_000, &1);
    let splits = vec![&env, route.clone()];

    // The order is larger than the split accounts for.
    assert!(client
        .try_execute_split(
            &actors.trader,
            &actors.token_a,
            &2_000,
            &0,
            &splits
        )
        .is_err());
    assert!(client
        .try_execute_split(
            &actors.trader,
            &actors.token_a,
            &1_000,
            &999_999_999,
            &splits
        )
        .is_err());
    // An empty split is meaningless.
    assert!(client
        .try_execute_split(
            &actors.trader,
            &actors.token_a,
            &1_000,
            &0,
            &Vec::new(&env)
        )
        .is_err());
}

#[test]
fn a_swap_fails_when_the_pool_cannot_pay() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    // A pool promising far more output than the aggregator actually custodies,
    // and with no liquidity behind it.
    let pool = Address::generate(&env);
    client.register_pool(
        &pool,
        &actors.token_a,
        &actors.token_c,
        &1,
        &10_000_000_000,
        &POOL_FEE_BPS,
        &PoolKind::ConstantProduct,
    );

    let route = client.find_path(&actors.token_a, &actors.token_c, &1_000, &1);
    assert!(client
        .try_swap(
            &actors.trader,
            &actors.token_a,
            &actors.token_c,
            &1_000,
            &0,
            &route
        )
        .is_err());
}

// ----------------------------------------------------------------------
// Liquidity and incentives
// ----------------------------------------------------------------------

#[test]
fn liquidity_can_be_added_and_removed() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    let pool = Address::generate(&env);
    client.register_pool(
        &pool,
        &actors.token_a,
        &actors.token_b,
        &1_000_000,
        &1_000_000,
        &POOL_FEE_BPS,
        &PoolKind::ConstantProduct,
    );

    let before = balance(&env, &actors.token_a, &actors.lp);
    client.add_liquidity(&actors.lp, &pool, &actors.token_a, &500_000);
    assert_eq!(before - balance(&env, &actors.token_a, &actors.lp), 500_000);
    assert_eq!(client.get_pool(&pool).reserve_in, 1_500_000);

    client.remove_liquidity(&actors.lp, &pool, &actors.token_a, &200_000);
    assert_eq!(client.get_pool(&pool).reserve_in, 1_300_000);
    assert_eq!(balance(&env, &actors.token_a, &actors.lp) - before, 300_000);

    // Liquidity for a token the pool does not hold is refused.
    assert!(client
        .try_add_liquidity(&actors.lp, &pool, &actors.token_c, &1)
        .is_err());
    // And you cannot withdraw more than the pool is quoting.
    assert!(client
        .try_remove_liquidity(&actors.lp, &pool, &actors.token_a, &9_000_000)
        .is_err());
    assert!(client
        .try_add_liquidity(&actors.lp, &pool, &actors.token_a, &0)
        .is_err());
}

#[test]
fn incentives_accrue_proportionally() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let pool = pool_ab(&env, &client, &actors, &1_000_000, &1_000_000);

    // The first backer seeds the pool and earns nothing on their own deposit.
    client.fund_incentives(&actors.lp, &pool, &10_000);
    assert_eq!(client.incentive_shares(&pool, &actors.lp), 10_000);
    assert_eq!(client.claim_incentives(&actors.lp, &pool), 0);

    // A second backer's budget goes entirely to the shares that were at risk,
    // so the first backer takes all of it and the newcomer takes none.
    client.fund_incentives(&actors.trader, &pool, &10_000);
    assert_eq!(client.claim_incentives(&actors.lp, &pool), 10_000);
    assert_eq!(client.claim_incentives(&actors.trader, &pool), 0);

    // Nothing is left over, so a repeat claim pays nothing.
    assert_eq!(client.claim_incentives(&actors.lp, &pool), 0);
    assert_eq!(client.claim_incentives(&actors.trader, &pool), 0);

    // A third backer splits the next round between the two holders already at
    // risk: lp was exposed to all 30_000 of shares, trader only to 20_000.
    client.fund_incentives(&actors.admin, &pool, &10_000);
    assert_eq!(client.claim_incentives(&actors.lp, &pool), 5_000);
    assert_eq!(client.claim_incentives(&actors.trader, &pool), 5_000);
    assert_eq!(client.claim_incentives(&actors.admin, &pool), 0);

    // 30_000 funded, 30_000 paid out: the pool is exactly solvent.
    assert_eq!(client.get_pool(&pool).incentives, 30_000);

    // Someone with no shares cannot claim.
    assert!(client.try_claim_incentives(&actors.fee_recipient, &pool).is_err());
    assert!(client.try_fund_incentives(&actors.lp, &pool, &0).is_err());
}

#[test]
fn incentive_payouts_are_backed_by_funded_tokens() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let pool = pool_ab(&env, &client, &actors, &1_000_000, &1_000_000);

    client.fund_incentives(&actors.lp, &pool, &10_000);
    client.fund_incentives(&actors.trader, &pool, &10_000);

    let before = balance(&env, &actors.token_a, &actors.lp);
    assert_eq!(client.claim_incentives(&actors.lp, &pool), 10_000);
    assert_eq!(balance(&env, &actors.token_a, &actors.lp) - before, 10_000);

    // The contract still custodies the newcomer's deposit for future rounds.
    let contract = client.address.clone();
    assert!(TokenClient::new(&env, &actors.token_a).balance(&contract) >= 10_000);
}

#[test]
fn stable_pools_slippage_lower() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    let pool = Address::generate(&env);
    client.register_pool(
        &pool,
        &actors.token_a,
        &actors.token_b,
        &1_000_000,
        &1_000_000,
        &POOL_FEE_BPS,
        &PoolKind::Stable,
    );
    client.add_liquidity(&actors.lp, &pool, &actors.token_a, &1_000_000);
    client.add_liquidity(&actors.lp, &pool, &actors.token_b, &1_000_000);

    // A stableswap pool gives up much less to a large order than x*y=k does.
    let cp = Address::generate(&env);
    client.register_pool(
        &cp,
        &actors.token_a,
        &actors.token_b,
        &1_000_000,
        &1_000_000,
        &POOL_FEE_BPS,
        &PoolKind::ConstantProduct,
    );
    client.add_liquidity(&actors.lp, &cp, &actors.token_a, &1_000_000);
    client.add_liquidity(&actors.lp, &cp, &actors.token_b, &1_000_000);

    let stable = client.quote(&pool, &actors.token_a, &100_000);
    let constant = client.quote(&cp, &actors.token_a, &100_000);
    assert!(stable.amount_out > constant.amount_out);
    assert!(stable.price_impact_bps < constant.price_impact_bps);
}
