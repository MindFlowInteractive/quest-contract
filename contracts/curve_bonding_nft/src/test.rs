#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, token, Address, Env};

const BASE: i128 = 100;
const SLOPE: i128 = 50;
const RESERVE_BPS: u32 = 4_000; // 40 % of every sale backs holders
const MAX_SUPPLY: u32 = 10;

struct Fixture {
    env: Env,
    contract: Address,
    token: Address,
    treasury: Address,
}

impl Fixture {
    fn client(&self) -> CurveBondingNFTClient<'_> {
        CurveBondingNFTClient::new(&self.env, &self.contract)
    }

    fn token_client(&self) -> token::TokenClient<'_> {
        token::TokenClient::new(&self.env, &self.token)
    }

    fn asset(&self) -> token::StellarAssetClient<'_> {
        token::StellarAssetClient::new(&self.env, &self.token)
    }

    /// Funds `to` with `amount` of the payment token.
    fn fund(&self, to: &Address, amount: i128) {
        self.asset().mint(to, &amount);
    }

    fn balance(&self, who: &Address) -> i128 {
        self.token_client().balance(who)
    }
}

fn setup_with(base_price: i128, slope: i128, reserve_bps: u32, max_supply: u32) -> Fixture {
    let env = Env::default();
    env.mock_all_auths();

    let contract = env.register_contract(None, CurveBondingNFT);
    let issuer = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(issuer);
    let token = sac.address();
    let treasury = Address::generate(&env);

    let f = Fixture {
        env,
        contract,
        token,
        treasury,
    };
    f.client().initialize(
        &f.token.clone(),
        &f.treasury.clone(),
        &base_price,
        &slope,
        &reserve_bps,
        &max_supply,
    );
    f
}

fn setup() -> Fixture {
    setup_with(BASE, SLOPE, RESERVE_BPS, MAX_SUPPLY)
}

// ── Initialization ──────────────────────────────────────────────────────────

#[test]
fn initialize_stores_curve_and_views() {
    let f = setup();
    let c = f.client();

    assert_eq!(c.price(), BASE);
    assert_eq!(c.quote(&0), BASE);
    assert_eq!(c.quote(&5), BASE + SLOPE * 5);
    assert_eq!(c.supply(), 0);
    assert_eq!(c.reserve(), 0);
    assert_eq!(c.next_token_id(), 1);
    assert_eq!(c.treasury(), f.treasury);
    assert_eq!(c.payment_token(), f.token);
    assert_eq!(c.price_history_len(), 0);

    let params = c.params();
    assert_eq!(params.base_price, BASE);
    assert_eq!(params.slope, SLOPE);
    assert_eq!(params.reserve_bps, RESERVE_BPS);
    assert_eq!(params.max_supply, MAX_SUPPLY);
}

#[test]
#[should_panic(expected = "already initialized")]
fn initialize_twice_panics() {
    let f = setup();
    f.client()
        .initialize(&f.token.clone(), &f.treasury.clone(), &100, &10, &1_000, &5);
}

#[test]
#[should_panic(expected = "base_price must be positive")]
fn initialize_rejects_non_positive_base_price() {
    setup_with(0, SLOPE, RESERVE_BPS, MAX_SUPPLY);
}

#[test]
#[should_panic(expected = "reserve_bps cannot exceed 10000")]
fn initialize_rejects_invalid_reserve_bps() {
    setup_with(BASE, SLOPE, 10_001, MAX_SUPPLY);
}

// ── Buying, pricing, and the reserve/treasury split ────────────────────────

#[test]
fn buy_mints_and_splits_payment_between_reserve_and_treasury() {
    let f = setup();
    let c = f.client();
    let alice = Address::generate(&f.env);
    let bob = Address::generate(&f.env);
    f.fund(&alice, 1_000);
    f.fund(&bob, 1_000);

    // First mint costs base_price = 100: 40 to the reserve (40 %), 60 on.
    let token_a = c.buy(&alice, &1_000);
    assert_eq!(token_a, 1);
    assert_eq!(f.balance(&alice), 900);
    assert_eq!(f.balance(&f.treasury), 60);
    assert_eq!(f.balance(&c.address), 40);
    assert_eq!(c.reserve(), 40);
    assert_eq!(c.supply(), 1);
    assert_eq!(c.owner_of(&token_a), alice);
    let nft = c.nft(&token_a);
    assert_eq!(nft.owner, alice);
    assert_eq!(nft.minted_price, 100);

    // Second mint costs 150: reserve += 60, treasury += 90.
    let token_b = c.buy(&bob, &1_000);
    assert_eq!(token_b, 2);
    assert_eq!(f.balance(&bob), 850);
    assert_eq!(f.balance(&f.treasury), 150);
    assert_eq!(f.balance(&c.address), 100);
    assert_eq!(c.reserve(), 100);
    assert_eq!(c.supply(), 2);
    assert_eq!(c.next_token_id(), 3);
    assert_eq!(c.owner_of(&token_b), bob);

    // The contract never holds more than the reserve it owes holders.
    assert_eq!(f.balance(&c.address), c.reserve());
}

#[test]
fn progressive_pricing_follows_the_curve() {
    let f = setup();
    let c = f.client();
    let buyer = Address::generate(&f.env);
    f.fund(&buyer, 10_000);

    let mut expected_prices = [BASE, BASE + SLOPE, BASE + 2 * SLOPE];
    for (i, expected) in expected_prices.iter_mut().enumerate() {
        assert_eq!(c.price(), *expected, "price before mint {}", i + 1);
        let token_id = c.buy(&buyer, &10_000);
        let nft = c.nft(&token_id);
        assert_eq!(nft.minted_price, *expected);
        *expected += SLOPE;
    }

    // The next price keeps climbing; the curve is strictly progressive.
    assert_eq!(c.price(), BASE + 3 * SLOPE);
    assert!(c.price() > BASE);
}

#[test]
#[should_panic(expected = "price exceeds slippage limit")]
fn buy_rejects_price_above_max_slippage() {
    let f = setup();
    let c = f.client();
    let buyer = Address::generate(&f.env);
    f.fund(&buyer, 1_000);

    // The next mint costs 100, but the buyer is only willing to pay 99.
    c.buy(&buyer, &99);
}

#[test]
#[should_panic(expected = "sold out")]
fn buy_rejects_mints_past_max_supply() {
    let f = setup_with(BASE, SLOPE, RESERVE_BPS, 2);
    let c = f.client();
    let buyer = Address::generate(&f.env);
    f.fund(&buyer, 10_000);

    c.buy(&buyer, &10_000);
    c.buy(&buyer, &10_000);
    c.buy(&buyer, &10_000); // supply == max_supply
}

// ── Transfers ───────────────────────────────────────────────────────────────

#[test]
fn transfer_moves_ownership_and_metadata() {
    let f = setup();
    let c = f.client();
    let alice = Address::generate(&f.env);
    let bob = Address::generate(&f.env);
    f.fund(&alice, 1_000);
    f.fund(&bob, 1_000);

    let token_id = c.buy(&alice, &1_000);
    c.transfer(&alice, &bob, &token_id);

    assert_eq!(c.owner_of(&token_id), bob);
    assert_eq!(c.nft(&token_id).owner, bob);
    // Ownership is unchanged by transfers for reserve accounting: supply
    // and price are untouched.
    assert_eq!(c.supply(), 1);
    assert_eq!(c.price(), BASE + SLOPE);
}

#[test]
#[should_panic(expected = "not the token owner")]
fn transfer_rejects_non_owner() {
    let f = setup();
    let c = f.client();
    let alice = Address::generate(&f.env);
    let mallory = Address::generate(&f.env);
    f.fund(&alice, 1_000);
    f.fund(&mallory, 1_000);

    let token_id = c.buy(&alice, &1_000);
    c.transfer(&mallory, &alice, &token_id);
}

#[test]
#[should_panic(expected = "token does not exist")]
fn views_reject_unknown_token() {
    let f = setup();
    f.client().owner_of(&42);
}

// ── Exit liquidity ──────────────────────────────────────────────────────────

#[test]
fn burn_and_exit_pays_pro_rata_reserve_share() {
    let f = setup();
    let c = f.client();
    let alice = Address::generate(&f.env);
    let bob = Address::generate(&f.env);
    f.fund(&alice, 1_000);
    f.fund(&bob, 1_000);

    let token_a = c.buy(&alice, &1_000); // reserve 40
    let token_b = c.buy(&bob, &1_000); // reserve 100
    assert_eq!(c.reserve(), 100);
    assert_eq!(c.supply(), 2);

    // Alice exits: 100 / 2 = 50 back from the reserve.
    let proceeds = c.burn_and_exit(&token_a, &0);
    assert_eq!(proceeds, 50);
    assert_eq!(f.balance(&alice), 900 + 50);
    assert_eq!(c.reserve(), 50);
    assert_eq!(c.supply(), 1);
    assert_eq!(f.balance(&c.address), c.reserve());

    // The burned token is gone; bob's remains.
    assert!(c.try_owner_of(&token_a).is_err());
    assert_eq!(c.owner_of(&token_b), bob);

    // Bob exits last: the whole remaining reserve (no division dust here).
    let proceeds_b = c.burn_and_exit(&token_b, &0);
    assert_eq!(proceeds_b, 50);
    assert_eq!(c.reserve(), 0);
    assert_eq!(c.supply(), 0);
    assert_eq!(f.balance(&c.address), 0);
}

#[test]
#[should_panic(expected = "proceeds below slippage limit")]
fn exit_rejects_proceeds_below_min() {
    let f = setup();
    let c = f.client();
    let alice = Address::generate(&f.env);
    f.fund(&alice, 1_000);

    let token_id = c.buy(&alice, &1_000);
    // Reserve 40, supply 1 → payout would be 40; the holder demands 41.
    c.burn_and_exit(&token_id, &41);
}

#[test]
fn exit_requires_token_auth() {
    let f = setup();
    let c = f.client();
    let alice = Address::generate(&f.env);
    f.fund(&alice, 1_000);
    let token_id = c.buy(&alice, &1_000);

    // Drop auth mocking: the burn must fail because the owner's
    // require_auth() is not satisfied.
    f.env.set_auths(&[]);
    assert!(c.try_burn_and_exit(&token_id, &0).is_err());
    assert_eq!(c.supply(), 1);
}

#[test]
fn plain_burn_forfeits_claim_to_remaining_holders() {
    let f = setup();
    let c = f.client();
    let alice = Address::generate(&f.env);
    let bob = Address::generate(&f.env);
    f.fund(&alice, 1_000);
    f.fund(&bob, 1_000);

    let token_a = c.buy(&alice, &1_000); // reserve 40
    let token_b = c.buy(&bob, &1_000); // reserve 100
    c.burn(&token_a);

    // Alice's share stays in the reserve; bob's exit claims it all.
    assert_eq!(c.reserve(), 100);
    assert_eq!(c.supply(), 1);
    let proceeds = c.burn_and_exit(&token_b, &0);
    assert_eq!(proceeds, 100);
    assert_eq!(c.reserve(), 0);
}

// ── Price history ───────────────────────────────────────────────────────────

#[test]
fn price_history_records_every_mint() {
    let f = setup();
    let c = f.client();
    let buyer = Address::generate(&f.env);
    f.fund(&buyer, 10_000);

    c.buy(&buyer, &10_000);
    c.buy(&buyer, &10_000);
    c.buy(&buyer, &10_000);

    assert_eq!(c.price_history_len(), 3);
    let p0 = c.price_history(&0);
    assert_eq!(p0.supply_before, 0);
    assert_eq!(p0.price, BASE);
    let p1 = c.price_history(&1);
    assert_eq!(p1.supply_before, 1);
    assert_eq!(p1.price, BASE + SLOPE);
    let p2 = c.price_history(&2);
    assert_eq!(p2.supply_before, 2);
    assert_eq!(p2.price, BASE + 2 * SLOPE);
}

#[test]
#[should_panic(expected = "price history index out of bounds")]
fn price_history_rejects_out_of_range_index() {
    let f = setup();
    f.client().price_history(&0);
}

#[test]
fn price_history_ring_is_bounded() {
    // Tiny curve so 266 mints stay cheap: price(s) = 1 + s.
    let f = setup_with(1, 1, 10_000, 300);
    // 266 mints with growing storage exceed the default host budget.
    f.env.budget().reset_unlimited();
    let c = f.client();
    let buyer = Address::generate(&f.env);
    f.fund(&buyer, 1_000_000);

    let mints = MAX_HISTORY + 10; // 266 mints > the 256-point ring
    for _ in 0..mints {
        c.buy(&buyer, &1_000_000);
    }

    assert_eq!(c.price_history_len(), MAX_HISTORY);
    // The oldest 10 points were evicted: the ring now starts at
    // supply_before = 10 with price = 1 + 10.
    let first = c.price_history(&0);
    assert_eq!(first.supply_before, 10);
    assert_eq!(first.price, 11);
    let last = c.price_history(&(MAX_HISTORY - 1));
    assert_eq!(last.supply_before, mints - 1);
    assert_eq!(last.price, 1 + (mints - 1) as i128);
}
