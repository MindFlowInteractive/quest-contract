#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, testutils::Ledger, token, Address, Env, Symbol};

fn setup_token<'a>(
    env: &'a Env,
    admin: &'a Address,
) -> (Address, token::Client<'a>, token::StellarAssetClient<'a>) {
    let token_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_client = token::Client::new(env, &token_id);
    let token_admin_client = token::StellarAssetClient::new(env, &token_id);
    (token_id, token_client, token_admin_client)
}

fn setup<'a>(env: &'a Env) -> (SwapAggregatorContractClient<'a>, Address) {
    let contract_id = env.register_contract(None, SwapAggregatorContract);
    let client = SwapAggregatorContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (client, admin)
}

#[test]
fn test_initialize_and_duplicate() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup(&env);

    // Re-initializing must fail.
    assert!(client.try_initialize(&admin).is_err());

    // Registry starts empty.
    assert_eq!(client.dex_count(), 0);
}

#[test]
fn test_registration_requires_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin) = setup(&env);
    let stranger = Address::generate(&env);
    let token_in = Address::generate(&env);
    let token_out = Address::generate(&env);
    let dex_id = Symbol::new(&env, "dex_a");

    // `stranger` passes require_auth (mocked) but is not the stored admin.
    let res = client.try_register_dex(
        &stranger,
        &dex_id,
        &token_in,
        &token_out,
        &(2 * PRICE_SCALE),
        &0u32,
    );
    assert!(res.is_err());
    assert_eq!(client.dex_count(), 0);
}

#[test]
fn test_register_update_price_and_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup(&env);
    let token_in = Address::generate(&env);
    let token_out = Address::generate(&env);
    let dex_id = Symbol::new(&env, "dex_a");

    client.register_dex(
        &admin,
        &dex_id,
        &token_in,
        &token_out,
        &(2 * PRICE_SCALE),
        &30u32,
    );

    let entry = client.get_dex(&dex_id);
    assert_eq!(entry.price, 2 * PRICE_SCALE);
    assert_eq!(entry.fee_bps, 30);
    assert!(entry.enabled);
    assert_eq!(client.dex_count(), 1);

    // Duplicate registration rejected.
    assert!(client
        .try_register_dex(
            &admin,
            &dex_id,
            &token_in,
            &token_out,
            &(2 * PRICE_SCALE),
            &30u32,
        )
        .is_err());

    // Price feed getter + admin price update.
    assert_eq!(client.get_price(&dex_id), 2 * PRICE_SCALE);
    client.update_price(&admin, &dex_id, &(3 * PRICE_SCALE));
    assert_eq!(client.get_price(&dex_id), 3 * PRICE_SCALE);

    // update_dex changes rate and fee.
    client.update_dex(&admin, &dex_id, &(4 * PRICE_SCALE), &0u32);
    let entry = client.get_dex(&dex_id);
    assert_eq!(entry.price, 4 * PRICE_SCALE);
    assert_eq!(entry.fee_bps, 0);

    // Paused venues are excluded from routing.
    client.pause_dex(&admin, &dex_id);
    assert!(client
        .try_quote_best(&1_000i128, &token_in, &token_out)
        .is_err());

    // Resuming restores the route.
    client.resume_dex(&admin, &dex_id);
    let quote = client.quote_best(&1_000i128, &token_in, &token_out);
    assert_eq!(quote.amount_out, 4_000);

    // Removal clears the registry.
    client.remove_dex(&admin, &dex_id);
    assert_eq!(client.dex_count(), 0);
    assert!(client.try_get_dex(&dex_id).is_err());
}

#[test]
fn test_best_price_selection_with_fees() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup(&env);
    let token_in = Address::generate(&env);
    let token_out = Address::generate(&env);

    let dex_a = Symbol::new(&env, "dex_a");
    let dex_b = Symbol::new(&env, "dex_b");
    let dex_c = Symbol::new(&env, "dex_c");

    client.register_dex(
        &admin,
        &dex_a,
        &token_in,
        &token_out,
        &(2 * PRICE_SCALE),
        &100u32,
    );
    client.register_dex(
        &admin,
        &dex_b,
        &token_in,
        &token_out,
        &(2 * PRICE_SCALE),
        &0u32,
    );
    client.register_dex(
        &admin,
        &dex_c,
        &token_in,
        &token_out,
        &(2 * PRICE_SCALE),
        &300u32,
    );

    let best = client.quote_best(&10_000i128, &token_in, &token_out);
    // dex_b charges no fee, so it wins despite identical raw rate.
    assert!(best.dex_id == dex_b);
    assert_eq!(best.amount_out, 20_000);

    // quote_dex applies the fee: 20_000 - 1% = 19_800.
    assert_eq!(client.quote_dex(&dex_a, &10_000i128), 19_800);

    // Unknown pair has no route.
    let unknown = Address::generate(&env);
    assert!(client
        .try_quote_best(&10_000i128, &token_in, &unknown)
        .is_err());
}

#[test]
fn test_execute_swap_slippage_and_deadline() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup(&env);
    let trader = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let (token_in, token_in_client, token_in_admin) = setup_token(&env, &token_admin);
    let (token_out, token_out_client, token_out_admin) = setup_token(&env, &token_admin);

    token_in_admin.mint(&trader, &10_000);
    token_out_admin.mint(&admin, &100_000);
    client.add_liquidity(&admin, &token_out, &100_000i128);

    let dex_id = Symbol::new(&env, "dex_a");
    client.register_dex(
        &admin,
        &dex_id,
        &token_in,
        &token_out,
        &(2 * PRICE_SCALE),
        &0u32,
    );

    // MEV slippage guard: floor above achievable output is rejected.
    let res = client.try_execute_swap(
        &trader,
        &1_000i128,
        &token_in,
        &token_out,
        &2_500i128,
        &1_000u64,
    );
    assert!(res.is_err());

    // Successful swap at the quoted rate.
    let receipt = client.execute_swap(
        &trader,
        &1_000i128,
        &token_in,
        &token_out,
        &1_900i128,
        &1_000u64,
    );
    assert_eq!(receipt.amount_out, 2_000);
    assert_eq!(receipt.venues, 1);
    assert_eq!(token_in_client.balance(&trader), 9_000);
    assert_eq!(token_out_client.balance(&trader), 2_000);

    // Deadline guard: a stale swap is rejected.
    env.ledger().with_mut(|li| {
        li.timestamp = 5_000;
    });
    let stale = client.try_execute_swap(
        &trader,
        &1_000i128,
        &token_in,
        &token_out,
        &0i128,
        &1_000u64,
    );
    assert!(stale.is_err());

    // Analytics reflect exactly one settled swap.
    let analytics = client.analytics();
    assert_eq!(analytics.swap_count, 1);
    assert_eq!(analytics.total_volume_in, 1_000);
    assert_eq!(analytics.total_volume_out, 2_000);
}

#[test]
fn test_split_swap_route_and_analytics() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup(&env);
    let trader = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let (token_in, _token_in_client, token_in_admin) = setup_token(&env, &token_admin);
    let (token_out, token_out_client, token_out_admin) = setup_token(&env, &token_admin);

    token_in_admin.mint(&trader, &10_000);
    token_out_admin.mint(&admin, &100_000);
    client.add_liquidity(&admin, &token_out, &100_000i128);

    let dex_a = Symbol::new(&env, "dex_a");
    let dex_b = Symbol::new(&env, "dex_b");
    client.register_dex(
        &admin,
        &dex_a,
        &token_in,
        &token_out,
        &(2 * PRICE_SCALE),
        &0u32,
    );
    client.register_dex(
        &admin,
        &dex_b,
        &token_in,
        &token_out,
        &(2 * PRICE_SCALE),
        &0u32,
    );

    // Split across two equal venues: 500 + 500 in, 1000 + 1000 out.
    let route = client.split_swap(
        &trader,
        &1_000i128,
        &token_in,
        &token_out,
        &2u32,
        &3_500i128,
        &1_000u64,
    );
    assert_eq!(route.legs.len(), 2);
    assert_eq!(route.total_in, 1_000);
    assert_eq!(route.total_out, 2_000);

    let leg_in_sum: i128 =
        route.legs.get(0).unwrap().amount_in + route.legs.get(1).unwrap().amount_in;
    assert_eq!(leg_in_sum, 1_000);
    assert_eq!(token_out_client.balance(&trader), 2_000);

    // Analytics accumulate across split + single execution.
    let _ = client.execute_swap(
        &trader,
        &500i128,
        &token_in,
        &token_out,
        &900i128,
        &1_000u64,
    );
    let analytics = client.analytics();
    assert_eq!(analytics.swap_count, 2);
    assert_eq!(analytics.total_volume_in, 1_500);
    assert_eq!(analytics.total_volume_out, 3_000);

    // Last route is the most recent (single-venue) execution.
    let last = client.last_route();
    assert_eq!(last.legs.len(), 1);
    assert_eq!(last.total_out, 1_000);

    // A split that cannot meet its floor is rejected before any state change.
    let count_before = client.analytics().swap_count;
    let res = client.try_split_swap(
        &trader,
        &1_000i128,
        &token_in,
        &token_out,
        &2u32,
        &99_999i128,
        &1_000u64,
    );
    assert!(res.is_err());
    assert_eq!(client.analytics().swap_count, count_before);
}

#[test]
fn test_validation_and_no_route() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup(&env);
    let token_in = Address::generate(&env);
    let token_out = Address::generate(&env);

    // Nothing registered yet.
    assert!(client
        .try_quote_best(&1_000i128, &token_in, &token_out)
        .is_err());

    // Identical tokens rejected.
    assert!(client
        .try_register_dex(
            &admin,
            &Symbol::new(&env, "bad1"),
            &token_in,
            &token_in,
            &PRICE_SCALE,
            &0u32,
        )
        .is_err());

    // Non-positive price rejected.
    assert!(client
        .try_register_dex(
            &admin,
            &Symbol::new(&env, "bad2"),
            &token_in,
            &token_out,
            &0i128,
            &0u32,
        )
        .is_err());

    // Fee above 100% rejected.
    assert!(client
        .try_register_dex(
            &admin,
            &Symbol::new(&env, "bad3"),
            &token_in,
            &token_out,
            &PRICE_SCALE,
            &10_001u32,
        )
        .is_err());

    // Non-positive swap amount rejected.
    let dex_id = Symbol::new(&env, "dex_a");
    client.register_dex(
        &admin,
        &dex_id,
        &token_in,
        &token_out,
        &PRICE_SCALE,
        &0u32,
    );
    assert!(client
        .try_quote_best(&0i128, &token_in, &token_out)
        .is_err());
}

#[test]
fn test_unauthorized_admin_operations() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup(&env);
    let stranger = Address::generate(&env);
    let token_in = Address::generate(&env);
    let token_out = Address::generate(&env);
    let dex_id = Symbol::new(&env, "dex_a");

    client.register_dex(
        &admin,
        &dex_id,
        &token_in,
        &token_out,
        &(2 * PRICE_SCALE),
        &30u32,
    );

    assert!(client
        .try_update_price(&stranger, &dex_id, &PRICE_SCALE)
        .is_err());
    assert!(client
        .try_update_dex(&stranger, &dex_id, &PRICE_SCALE, &0u32)
        .is_err());
    assert!(client.try_pause_dex(&stranger, &dex_id).is_err());
    assert!(client
        .try_set_dex_enabled(&stranger, &dex_id, &false)
        .is_err());
    assert!(client.try_remove_dex(&stranger, &dex_id).is_err());
}

#[test]
#[should_panic]
fn test_swap_without_liquidity_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup(&env);
    let trader = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let (token_in, _token_in_client, token_in_admin) = setup_token(&env, &token_admin);
    let (token_out, _token_out_client, token_out_admin) = setup_token(&env, &token_admin);

    token_in_admin.mint(&trader, &10_000);
    token_out_admin.mint(&admin, &100_000);
    // No liquidity funded for token_out.

    let dex_id = Symbol::new(&env, "dex_a");
    client.register_dex(
        &admin,
        &dex_id,
        &token_in,
        &token_out,
        &(2 * PRICE_SCALE),
        &0u32,
    );

    // Client unwraps the contract error and panics.
    client.execute_swap(
        &trader,
        &1_000i128,
        &token_in,
        &token_out,
        &0i128,
        &1_000u64,
    );
}
