#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

struct Ctx {
    env: Env,
    contract: Address,
    token: Address,
    sender: Address,
    recipient: Address,
    new_recipient: Address,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let new_recipient = Address::generate(&env);

    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let contract = env.register_contract(None, TokenStreamingContract);

    StellarAssetClient::new(&env, &token).mint(&sender, &10_000);

    Ctx {
        env,
        contract,
        token,
        sender,
        recipient,
        new_recipient,
    }
}

fn client(ctx: &Ctx) -> TokenStreamingContractClient<'_> {
    TokenStreamingContractClient::new(&ctx.env, &ctx.contract)
}

fn create_stream(ctx: &Ctx) -> u64 {
    client(ctx)
        .create_stream(&ctx.sender, &ctx.recipient, &ctx.token, &10, &0, &100)
        .unwrap()
}

#[test]
fn test_accrual_matches_elapsed_time() {
    let ctx = setup();
    let c = client(&ctx);
    let id = create_stream(&ctx);

    assert_eq!(TokenClient::new(&ctx.env, &ctx.token).balance(&ctx.contract), 1000);

    ctx.env.ledger().with_mut(|li| li.timestamp = 40);
    assert_eq!(c.streamed(&id), Ok(400));
    assert_eq!(c.accrued(&id), Ok(400));

    c.withdraw(&ctx.recipient, &id, &200).unwrap();
    assert_eq!(c.accrued(&id), Ok(200));

    ctx.env.ledger().with_mut(|li| li.timestamp = 100);
    assert_eq!(c.streamed(&id), Ok(1000));
    assert_eq!(c.accrued(&id), Ok(800));

    ctx.env.ledger().with_mut(|li| li.timestamp = 500);
    assert_eq!(c.accrued(&id), Ok(800));
}

#[test]
fn test_withdraw_never_exceeds_accrued() {
    let ctx = setup();
    let c = client(&ctx);
    let id = create_stream(&ctx);

    ctx.env.ledger().with_mut(|li| li.timestamp = 10);
    assert_eq!(c.accrued(&id), Ok(100));

    assert_eq!(
        c.try_withdraw(&ctx.recipient, &id, &101),
        Err(Ok(StreamingError::WithdrawalExceedsAccrued))
    );
    assert_eq!(
        c.try_withdraw(&ctx.sender, &id, &10),
        Err(Ok(StreamingError::Unauthorized))
    );
    c.withdraw(&ctx.recipient, &id, &100).unwrap();
    assert_eq!(
        c.try_withdraw(&ctx.recipient, &id, &1),
        Err(Ok(StreamingError::NothingAccrued))
    );
}

#[test]
fn test_cancel_splits_remainder_and_settles_both_parties() {
    let ctx = setup();
    let c = client(&ctx);
    let id = create_stream(&ctx);

    ctx.env.ledger().with_mut(|li| li.timestamp = 40);
    let (sender_share, recipient_share) = c.cancel(&ctx.sender, &id, &2_500).unwrap();
    assert_eq!(sender_share, 450);
    assert_eq!(recipient_share, 150);

    let token = TokenClient::new(&ctx.env, &ctx.token);
    assert_eq!(token.balance(&ctx.sender), 9_450);
    assert_eq!(token.balance(&ctx.recipient), 150);

    assert_eq!(c.accrued(&id), Ok(400));
    c.withdraw_all(&ctx.recipient, &id).unwrap();
    assert_eq!(token.balance(&ctx.recipient), 550);

    assert_eq!(
        c.try_cancel(&ctx.sender, &id, &0),
        Err(Ok(StreamingError::AlreadyCancelled))
    );
}

#[test]
fn test_recipient_can_change_recipient() {
    let ctx = setup();
    let c = client(&ctx);
    let id = create_stream(&ctx);

    c.change_recipient(&ctx.recipient, &id, &ctx.new_recipient)
        .unwrap();
    assert_eq!(c.get_stream(&id).unwrap().recipient, ctx.new_recipient);

    ctx.env.ledger().with_mut(|li| li.timestamp = 10);
    assert_eq!(
        c.try_withdraw(&ctx.recipient, &id, &10),
        Err(Ok(StreamingError::Unauthorized))
    );
    c.withdraw_all(&ctx.new_recipient, &id).unwrap();
    assert_eq!(
        TokenClient::new(&ctx.env, &ctx.token).balance(&ctx.new_recipient),
        100
    );
}
