#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
};

const DAY: u64 = 86_400;

struct Actors {
    admin: Address,
    maker: Address,
    holder: Address,
    quote: Address,
    underlying: Address,
}

/// Deploy the contract plus the two test tokens. The returned client borrows
/// `env`, so every test keeps the `Env` in a local.
fn setup<'a>(env: &'a Env) -> (OptionsTradingClient<'a>, Actors) {
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let contract_id = env.register_contract(None, OptionsTrading);
    let client = OptionsTradingClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let maker = Address::generate(env);
    let holder = Address::generate(env);

    let quote_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let underlying_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let quote_admin = StellarAssetClient::new(env, &quote_id);
    let underlying_admin = StellarAssetClient::new(env, &underlying_id);

    for who in [maker.clone(), holder.clone()] {
        quote_admin.mint(&who, &10_000_000);
        underlying_admin.mint(&who, &10_000_000);
    }

    client.initialize(&admin, &quote_id, &100, &admin, &500);

    let actors = Actors {
        admin,
        maker,
        holder,
        quote: quote_id,
        underlying: underlying_id,
    };
    (client, actors)
}

fn q9(x: i128) -> i128 {
    x * SCALE
}

fn spot_of(client: &OptionsTradingClient, series_id: u64, price: i128) {
    client.set_spot(&series_id, &q9(price));
}

fn advance(env: &Env, seconds: u64) {
    let target = env.ledger().timestamp() + seconds;
    env.ledger().with_mut(|l| l.timestamp = target);
}

/// Register a call series struck at `strike` (Q9) expiring in `days` days.
fn call_series(
    client: &OptionsTradingClient,
    actors: &Actors,
    strike: i128,
    days: u64,
    style: OptionStyle,
) -> u64 {
    let expiry = 1_000_000 + days * DAY;
    client.create_series(
        &actors.underlying,
        &actors.quote,
        &OptionType::Call,
        &style,
        &strike,
        &expiry,
        &2000,
    )
}

// ----------------------------------------------------------------------
// Setup
// ----------------------------------------------------------------------

#[test]
fn initialize_is_one_shot() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    assert!(client
        .try_initialize(&actors.admin, &actors.quote, &100, &actors.admin, &500)
        .is_err());

    let config = client.get_config();
    assert_eq!(config.fee_bps, 100);
    assert_eq!(config.risk_free_bps, 500);
    assert_eq!(config.admin, actors.admin);
}

#[test]
fn initialize_rejects_an_oversized_fee() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, OptionsTrading);
    let client = OptionsTradingClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    assert!(client
        .try_initialize(&admin, &token, &(MAX_FEE_BPS + 1), &admin, &0)
        .is_err());
}

#[test]
fn create_series_rejects_bad_parameters() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let now = env.ledger().timestamp();

    // Zero strike.
    assert!(client
        .try_create_series(
            &actors.underlying,
            &actors.quote,
            &OptionType::Call,
            &OptionStyle::European,
            &0,
            &(now + DAY),
            &2000
        )
        .is_err());

    // Expiry in the past.
    assert!(client
        .try_create_series(
            &actors.underlying,
            &actors.quote,
            &OptionType::Call,
            &OptionStyle::European,
            &q9(100),
            &(now - 1),
            &2000
        )
        .is_err());

    assert_eq!(client.series_count(), 0);
}

// ----------------------------------------------------------------------
// Writing
// ----------------------------------------------------------------------

#[test]
fn write_collects_premium_and_opens_position() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));

    let quote = TokenClient::new(&env, &actors.quote);
    let before = quote.balance(&actors.maker);

    // 100 units at 5.00 => 500 premium, plus a 1% fee => 505.
    let pos = client.write(&id, &actors.maker, &100);
    assert_eq!(before - quote.balance(&actors.maker), 505);

    let position = client.get_position(&pos);
    assert_eq!(position.units, 100);
    assert_eq!(position.premium_paid, 500);
    assert_eq!(position.status, OptionStatus::Open);
    assert_eq!(position.owner, actors.maker);

    let series = client.get_series(&id);
    assert_eq!(series.total_supply, 100);
    assert_eq!(series.open_interest, 100);
    // Only the net premium backs the series; the fee went to the recipient.
    assert_eq!(series.quote_reserve, 500);

    assert_eq!(client.user_positions(&actors.maker).len(), 1);
    assert_eq!(client.series_positions(&id).len(), 1);
    assert_eq!(client.position_count(), 1);
}

#[test]
fn write_rejects_bad_amounts() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));

    assert!(client.try_write(&id, &actors.maker, &0).is_err());
    assert!(client.try_write(&id, &actors.maker, &-5).is_err());
    assert!(client.try_write(&99, &actors.maker, &5).is_err());
}

#[test]
fn write_rejected_once_series_paused_or_expired() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 30, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));

    client.set_series_active(&id, &false);
    assert!(client.try_write(&id, &actors.maker, &10).is_err());
    client.set_series_active(&id, &true);

    advance(&env, 31 * DAY);
    assert!(client.try_write(&id, &actors.maker, &10).is_err());
}

// ----------------------------------------------------------------------
// Exercise
// ----------------------------------------------------------------------

#[test]
fn american_call_exercises_before_expiry() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));

    let pos = client.write(&id, &actors.maker, &100);
    advance(&env, 10 * DAY);
    spot_of(&client, id, 130);

    // Back the payout: 100 units * (130 - 100) = 3000 quote.
    client.fund_series(&actors.maker, &id, &actors.quote, &5_000);

    let before = TokenClient::new(&env, &actors.quote).balance(&actors.maker);
    assert_eq!(client.exercise(&actors.maker, &pos), 3_000);
    assert_eq!(
        TokenClient::new(&env, &actors.quote).balance(&actors.maker) - before,
        3_000
    );

    let position = client.get_position(&pos);
    assert_eq!(position.status, OptionStatus::Exercised);
    assert_eq!(position.units, 0);
    assert!(position.closed_at > 0);

    let series = client.get_series(&id);
    assert_eq!(series.open_interest, 0);
    assert_eq!(series.total_supply, 0);
    assert_eq!(series.quote_reserve, 500 + 5_000 - 3_000);
}

#[test]
fn european_call_cannot_exercise_before_expiry() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::European);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));
    let pos = client.write(&id, &actors.maker, &100);
    client.fund_series(&actors.maker, &id, &actors.quote, &5_000);

    advance(&env, 10 * DAY);
    assert!(client.try_exercise(&actors.maker, &pos).is_err());

    // Still blocked one second before expiry.
    let target = 1_000_000 + 90 * DAY - 1;
    env.ledger().with_mut(|l| l.timestamp = target);
    assert!(client.try_exercise(&actors.maker, &pos).is_err());

    // Allowed at expiry itself.
    env.ledger().with_mut(|l| l.timestamp = target + 1);
    spot_of(&client, id, 105);
    assert_eq!(client.exercise(&actors.maker, &pos), 500);
}

#[test]
fn out_of_the_money_exercise_pays_nothing() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));
    let pos = client.write(&id, &actors.maker, &100);

    // No funding at all: a zero payout must still succeed.
    spot_of(&client, id, 90);
    assert_eq!(client.exercise(&actors.maker, &pos), 0);
}

#[test]
fn exercise_fails_without_enough_series_backing() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));
    let pos = client.write(&id, &actors.maker, &100);

    // 50 of intrinsic per unit against a 495 reserve.
    spot_of(&client, id, 150);
    assert!(client.try_exercise(&actors.maker, &pos).is_err());

    client.fund_series(&actors.maker, &id, &actors.quote, &10_000);
    assert_eq!(client.exercise(&actors.maker, &pos), 5_000);
}

#[test]
fn a_position_cannot_be_exercised_twice() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));
    let pos = client.write(&id, &actors.maker, &100);
    client.fund_series(&actors.maker, &id, &actors.quote, &5_000);

    spot_of(&client, id, 120);
    assert_eq!(client.exercise(&actors.maker, &pos), 2_000);
    assert!(client.try_exercise(&actors.maker, &pos).is_err());
}

#[test]
fn put_pays_the_holder_in_underlying() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = client.create_series(
        &actors.underlying,
        &actors.quote,
        &OptionType::Put,
        &OptionStyle::American,
        &q9(100),
        &(1_000_000 + 90 * DAY),
        &2000,
    );
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(4));

    let pos = client.write(&id, &actors.maker, &50);
    client.fund_series(&actors.maker, &id, &actors.underlying, &1_000);

    spot_of(&client, id, 80);
    let before = TokenClient::new(&env, &actors.underlying).balance(&actors.maker);
    // 50 units * (100 - 80) = 1000 underlying.
    assert_eq!(client.exercise(&actors.maker, &pos), 1_000);
    assert_eq!(
        TokenClient::new(&env, &actors.underlying).balance(&actors.maker) - before,
        1_000
    );
}

#[test]
fn only_the_owner_can_exercise() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));
    let pos = client.write(&id, &actors.maker, &10);
    client.fund_series(&actors.maker, &id, &actors.quote, &1_000);

    assert!(client.try_exercise(&actors.holder, &pos).is_err());
    assert!(client.try_exercise(&actors.maker, &99).is_err());
}

// ----------------------------------------------------------------------
// Secondary market
// ----------------------------------------------------------------------

#[test]
fn buy_moves_units_and_cost_basis() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));

    let pos = client.write(&id, &actors.maker, &100);

    let quote = TokenClient::new(&env, &actors.quote);
    let taker_before = quote.balance(&actors.holder);
    let maker_before = quote.balance(&actors.maker);

    let bought = client.buy(&actors.holder, &pos, &40);

    // 40 units at 5.00 => 200 premium plus a 1% fee charged to the taker.
    assert_eq!(taker_before - quote.balance(&actors.holder), 202);
    // The maker keeps the 200 net premium but pays the 2 fee.
    assert_eq!(quote.balance(&actors.maker) - maker_before, 198);

    let original = client.get_position(&pos);
    assert_eq!(original.units, 60);
    assert_eq!(original.premium_paid, 300);

    let taken = client.get_position(&bought);
    assert_eq!(taken.owner, actors.holder);
    assert_eq!(taken.units, 40);
    assert_eq!(taken.premium_paid, 200);
    assert_eq!(client.series_positions(&id).len(), 2);
}

#[test]
fn buying_the_whole_position_flattens_it() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));

    let pos = client.write(&id, &actors.maker, &100);
    client.buy(&actors.holder, &pos, &100);

    let original = client.get_position(&pos);
    assert_eq!(original.units, 0);
    assert_eq!(original.premium_paid, 0);
}

#[test]
fn buy_rejects_more_units_than_held() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));
    let pos = client.write(&id, &actors.maker, &10);

    assert!(client.try_buy(&actors.holder, &pos, &11).is_err());
    assert!(client.try_buy(&actors.holder, &pos, &0).is_err());
    assert!(client.try_buy(&actors.holder, &pos, &-1).is_err());
}

#[test]
fn sell_returns_the_model_value() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));
    let pos = client.write(&id, &actors.maker, &100);
    client.fund_series(&actors.maker, &id, &actors.quote, &5_000);

    // Half a year out, 20% vol, ATM: roughly 5.5 per unit.
    advance(&env, 182 * DAY);

    let before = TokenClient::new(&env, &actors.quote).balance(&actors.maker);
    let value = client.sell(&actors.maker, &pos, &100);
    assert!(value > 400 && value < 700, "unexpected model value {}", value);
    assert_eq!(
        TokenClient::new(&env, &actors.quote).balance(&actors.maker) - before,
        value
    );

    let position = client.get_position(&pos);
    assert_eq!(position.status, OptionStatus::Closed);
    assert_eq!(position.units, 0);
    assert_eq!(client.get_series(&id).open_interest, 0);
}

#[test]
fn sell_partially_reduces_the_position() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));
    let pos = client.write(&id, &actors.maker, &100);
    client.fund_series(&actors.maker, &id, &actors.quote, &5_000);

    client.sell(&actors.maker, &pos, &40);
    let position = client.get_position(&pos);
    assert_eq!(position.status, OptionStatus::Open);
    assert_eq!(position.units, 60);
    assert_eq!(position.premium_paid, 300);
}

#[test]
fn only_the_owner_can_sell() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));
    let pos = client.write(&id, &actors.maker, &100);

    assert!(client.try_sell(&actors.holder, &pos, &10).is_err());
}

// ----------------------------------------------------------------------
// Settlement
// ----------------------------------------------------------------------

#[test]
fn settle_pays_intrinsic_and_skips_worthless() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 30, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(2));

    let winner = client.write(&id, &actors.maker, &100);
    let loser = client.write(&id, &actors.holder, &100);
    client.fund_series(&actors.maker, &id, &actors.quote, &1_000);

    advance(&env, 31 * DAY);
    spot_of(&client, id, 110);

    let quote = TokenClient::new(&env, &actors.quote);
    let winner_before = quote.balance(&actors.maker);
    let loser_before = quote.balance(&actors.holder);

    assert_eq!(client.settle(&id), 2);

    // (110 - 100) * 100 = 1000, to the in-the-money holder only.
    assert_eq!(quote.balance(&actors.maker) - winner_before, 1_000);
    assert_eq!(quote.balance(&actors.holder), loser_before);

    assert_eq!(client.get_position(&winner).status, OptionStatus::Settled);
    assert_eq!(client.get_position(&loser).status, OptionStatus::Settled);
    assert_eq!(client.get_series(&id).open_interest, 0);
}

#[test]
fn settle_is_blocked_before_expiry_and_is_idempotent() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 30, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(2));
    client.write(&id, &actors.maker, &10);

    assert!(client.try_settle(&id).is_err());

    advance(&env, 31 * DAY);
    spot_of(&client, id, 90);

    assert_eq!(client.settle(&id), 1);
    // Nothing left open the second time round.
    assert_eq!(client.settle(&id), 0);
}

#[test]
fn settle_skips_positions_the_series_cannot_cover() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 30, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(2));
    client.write(&id, &actors.maker, &100);

    advance(&env, 31 * DAY);
    spot_of(&client, id, 140);

    // No backing: the position stays open instead of settling short.
    assert_eq!(client.settle(&id), 0);
    assert_eq!(client.get_series(&id).open_interest, 100);

    client.fund_series(&actors.maker, &id, &actors.quote, &10_000);
    assert_eq!(client.settle(&id), 1);
    assert_eq!(client.get_series(&id).open_interest, 0);
}

#[test]
fn expire_marks_positions_worthless() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 10, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(2));
    let pos = client.write(&id, &actors.maker, &100);

    assert!(client.try_expire(&id).is_err());
    advance(&env, 11 * DAY);
    assert_eq!(client.expire(&id), 1);
    assert_eq!(client.get_position(&pos).status, OptionStatus::Expired);
}

// ----------------------------------------------------------------------
// Pricing and greeks
// ----------------------------------------------------------------------

#[test]
fn greeks_match_the_analytical_expectations() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 365, OptionStyle::European);
    spot_of(&client, id, 100);

    let g = client.greeks(&id);

    // ATM one-year call, 20% vol, 5% rate: delta ~0.64, price ~10.45.
    assert!(g.delta > SCALE / 2 && g.delta < SCALE, "delta {}", g.delta);
    assert!(g.gamma > 0, "gamma {}", g.gamma);
    assert!(g.vega > 0, "vega {}", g.vega);
    assert!(g.theta < 0, "theta {} should be negative for a call", g.theta);
    assert!(g.rho > 0, "rho {}", g.rho);
    assert!(g.price > q9(9) && g.price < q9(12), "price {}", g.price);
    assert_eq!(g.intrinsic, 0);
    assert_eq!(g.time_to_expiry, SCALE);
}

#[test]
fn deep_in_the_money_call_grows_with_the_forward() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 730, OptionStyle::European);
    spot_of(&client, id, 200);

    let g = client.greeks(&id);
    assert_eq!(g.intrinsic, q9(100));
    // Delta saturates once deep in the money.
    assert!(g.delta > 99 * SCALE / 100, "delta {}", g.delta);
    // Black-Scholes puts it at ~109.6: intrinsic plus the forward carry.
    assert!(g.price > q9(100) && g.price < q9(120), "price {}", g.price);
}

#[test]
fn deep_out_of_the_money_call_is_worthless() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 30, OptionStyle::European);
    spot_of(&client, id, 50);

    let g = client.greeks(&id);
    assert_eq!(g.intrinsic, 0);
    assert_eq!(g.delta, 0);
    assert!(g.price < SCALE, "price {} should round to zero", g.price);
}

#[test]
fn put_greeks_mirror_the_call() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = client.create_series(
        &actors.underlying,
        &actors.quote,
        &OptionType::Put,
        &OptionStyle::European,
        &q9(100),
        &(1_000_000 + 365 * DAY),
        &2000,
    );
    spot_of(&client, id, 100);

    let g = client.greeks(&id);
    // Put-call parity: delta_put = delta_call - 1, so the put delta is negative.
    assert!(g.delta < 0, "put delta {} should be negative", g.delta);
    assert!(g.gamma > 0, "gamma {}", g.gamma);
    assert!(g.rho < 0, "put rho {} should be negative", g.rho);
    // A long put bleeds time value too, so theta stays negative here.
    assert!(g.theta < 0, "put theta {} should be negative", g.theta);

    // The call on the same inputs has the mirrored delta and an identical gamma.
    let call_id = call_series(&client, &actors, q9(100), 365, OptionStyle::European);
    spot_of(&client, call_id, 100);
    let c = client.greeks(&call_id);
    assert_eq!(c.delta, g.delta + SCALE);
    assert_eq!(c.gamma, g.gamma);
    assert!(c.rho > 0);
}

#[test]
fn greeks_require_a_spot_price() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::European);

    assert!(client.try_greeks(&id).is_err());
    assert!(client.try_theoretical_price(&id, &1).is_err());
    assert!(client.try_intrinsic_value(&id, &1).is_err());

    assert!(client.try_set_spot(&id, &0).is_err());
    spot_of(&client, id, 100);
    assert!(client.try_greeks(&id).is_ok());
}

#[test]
fn pricing_extends_linearly_over_units() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 365, OptionStyle::European);
    spot_of(&client, id, 100);

    let one = client.theoretical_price(&id, &1);
    let hundred = client.theoretical_price(&id, &100);
    assert_eq!(hundred / 100, one);

    // ATM at 100 means no intrinsic value.
    assert_eq!(client.intrinsic_value(&id, &50), 0);
    spot_of(&client, id, 130);
    assert_eq!(client.intrinsic_value(&id, &50), 1_500);
}

#[test]
fn longer_dated_options_are_worth_more() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let short = call_series(&client, &actors, q9(100), 30, OptionStyle::European);
    let long = call_series(&client, &actors, q9(100), 730, OptionStyle::European);
    spot_of(&client, short, 100);
    spot_of(&client, long, 100);

    assert!(client.theoretical_price(&long, &1) > client.theoretical_price(&short, &1));
}

// ----------------------------------------------------------------------
// Close
// ----------------------------------------------------------------------

#[test]
fn close_honours_the_minimum_out() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));
    let pos = client.write(&id, &actors.maker, &100);
    client.fund_series(&actors.maker, &id, &actors.quote, &5_000);

    // An unreachable floor must abort.
    assert!(client.try_close(&actors.maker, &pos, &1_000_000).is_err());

    let value = client.close(&actors.maker, &pos, &0);
    assert!(value > 0);
    assert_eq!(client.get_position(&pos).status, OptionStatus::Closed);
    assert_eq!(client.get_series(&id).open_interest, 0);
}

#[test]
fn close_is_blocked_after_expiry() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 10, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(5));
    let pos = client.write(&id, &actors.maker, &100);
    client.fund_series(&actors.maker, &id, &actors.quote, &5_000);

    advance(&env, 11 * DAY);
    assert!(client.try_close(&actors.maker, &pos, &0).is_err());
    // Settling is the correct route from here.
    assert_eq!(client.settle(&id), 1);
}

// ----------------------------------------------------------------------
// Fees and administration
// ----------------------------------------------------------------------

#[test]
fn fees_are_configurable_and_capped() {
    let env = Env::default();
    let (client, actors) = setup(&env);

    assert!(client.try_set_fee(&(MAX_FEE_BPS + 1), &actors.admin).is_err());

    client.set_fee(&200, &actors.holder);
    let config = client.get_config();
    assert_eq!(config.fee_bps, 200);
    assert_eq!(config.fee_recipient, actors.holder);

    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::American);
    spot_of(&client, id, 100);
    client.set_premium(&id, &q9(10));

    let before = TokenClient::new(&env, &actors.quote).balance(&actors.holder);
    client.write(&id, &actors.maker, &100); // 1000 premium, 2% fee
    assert_eq!(
        TokenClient::new(&env, &actors.quote).balance(&actors.holder) - before,
        20
    );
    assert_eq!(client.quote_premium(&id, &100), 1_020);
}

#[test]
fn admin_can_be_rotated() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    client.set_admin(&actors.holder);
    assert_eq!(client.get_config().admin, actors.holder);
}

#[test]
fn funding_rejects_an_unrelated_token() {
    let env = Env::default();
    let (client, actors) = setup(&env);
    let id = call_series(&client, &actors, q9(100), 90, OptionStyle::American);

    let stranger = env
        .register_stellar_asset_contract_v2(actors.admin.clone())
        .address();
    assert!(client
        .try_fund_series(&actors.maker, &id, &stranger, &100)
        .is_err());
    assert!(client
        .try_fund_series(&actors.maker, &id, &actors.quote, &0)
        .is_err());

    client.fund_series(&actors.maker, &id, &actors.quote, &100);
    assert_eq!(client.get_series(&id).quote_reserve, 100);
}
