#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

struct Ctx {
    env: Env,
    contract: Address,
    token: Address,
    owner: Address,
    recipient: Address,
}

fn setup(delay: u64) -> Ctx {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let recipient = Address::generate(&env);

    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let contract = env.register_contract(None, SpendingLimitContract);

    let client = SpendingLimitContractClient::new(&env, &contract);
    client.initialize(&admin, &token, &delay).unwrap();

    Ctx {
        env,
        contract,
        token,
        owner,
        recipient,
    }
}

fn client(ctx: &Ctx) -> SpendingLimitContractClient<'_> {
    SpendingLimitContractClient::new(&ctx.env, &ctx.contract)
}

#[test]
fn test_initialize() {
    let ctx = setup(100);
    let c = client(&ctx);
    assert_eq!(c.get_delay(), 100);
    assert_eq!(c.get_balance(&ctx.owner), 0);
}

#[test]
fn test_increase_is_delayed_until_applied() {
    let ctx = setup(100);
    let c = client(&ctx);

    c.set_limits(&ctx.owner, &1000, &500).unwrap();
    let cfg = c.get_config(&ctx.owner);
    assert_eq!(cfg.daily_limit, 1000);
    assert_eq!(cfg.per_tx_limit, 500);

    c.set_limits(&ctx.owner, &2000, &800).unwrap();
    let cfg = c.get_config(&ctx.owner);
    assert_eq!(cfg.daily_limit, 1000);
    assert_eq!(cfg.per_tx_limit, 500);

    let pending = c.get_pending(&ctx.owner).unwrap();
    assert_eq!(pending.daily_limit, 2000);
    assert_eq!(pending.per_tx_limit, 800);
    assert_eq!(pending.effective_at, 100);

    assert_eq!(
        c.try_apply_pending(&ctx.owner),
        Err(Ok(SpendingLimitError::DelayNotElapsed))
    );

    ctx.env.ledger().with_mut(|li| li.timestamp = 100);
    c.apply_pending(&ctx.owner).unwrap();

    let cfg = c.get_config(&ctx.owner);
    assert_eq!(cfg.daily_limit, 2000);
    assert_eq!(cfg.per_tx_limit, 800);
    assert_eq!(c.get_pending(&ctx.owner), None);
}

#[test]
fn test_lower_is_immediate_and_cancels_pending() {
    let ctx = setup(100);
    let c = client(&ctx);

    c.set_limits(&ctx.owner, &1000, &500).unwrap();
    c.set_limits(&ctx.owner, &2000, &800).unwrap();
    assert!(c.get_pending(&ctx.owner).is_some());

    c.set_limits(&ctx.owner, &300, &200).unwrap();
    let cfg = c.get_config(&ctx.owner);
    assert_eq!(cfg.daily_limit, 300);
    assert_eq!(cfg.per_tx_limit, 200);
    assert_eq!(c.get_pending(&ctx.owner), None);
}

#[test]
fn test_per_transaction_and_daily_caps_are_enforced() {
    let ctx = setup(0);
    let c = client(&ctx);

    c.set_limits(&ctx.owner, &1000, &400).unwrap();

    StellarAssetClient::new(&ctx.env, &ctx.token).mint(&ctx.owner, &5000);
    c.deposit(&ctx.owner, &5000).unwrap();
    assert_eq!(c.get_balance(&ctx.owner), 5000);

    assert_eq!(
        c.try_spend(&ctx.owner, &ctx.recipient, &500),
        Err(Ok(SpendingLimitError::PerTransactionLimitExceeded))
    );

    c.spend(&ctx.owner, &ctx.recipient, &400).unwrap();
    c.spend(&ctx.owner, &ctx.recipient, &400).unwrap();

    assert_eq!(
        c.try_spend(&ctx.owner, &ctx.recipient, &300),
        Err(Ok(SpendingLimitError::DailyLimitExceeded))
    );

    c.spend(&ctx.owner, &ctx.recipient, &200).unwrap();
    assert_eq!(c.daily_spend(&ctx.owner), 1000);
    assert_eq!(c.get_balance(&ctx.owner), 4000);
    assert_eq!(
        TokenClient::new(&ctx.env, &ctx.token).balance(&ctx.recipient),
        1000
    );
}

#[test]
fn test_daily_cap_resets_on_schedule() {
    let ctx = setup(0);
    let c = client(&ctx);

    c.set_limits(&ctx.owner, &1000, &1000).unwrap();
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(&ctx.owner, &5000);
    c.deposit(&ctx.owner, &5000).unwrap();

    c.spend(&ctx.owner, &ctx.recipient, &1000).unwrap();
    assert_eq!(
        c.try_spend(&ctx.owner, &ctx.recipient, &1),
        Err(Ok(SpendingLimitError::DailyLimitExceeded))
    );

    ctx.env
        .ledger()
        .with_mut(|li| li.timestamp = DAY_SECONDS);
    c.spend(&ctx.owner, &ctx.recipient, &1).unwrap();
    assert_eq!(c.daily_spend(&ctx.owner), 1);
}
