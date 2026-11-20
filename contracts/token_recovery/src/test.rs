#![cfg(test)]

use super::{Error, TokenRecoveryContract, TokenRecoveryContractClient};
use soroban_sdk::{testutils::Address as _, token, Address, Env};

fn setup() -> (
    Env,
    TokenRecoveryContractClient<'static>,
    Address,
    Address,
    token::Client<'static>,
    token::StellarAssetClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register(TokenRecoveryContract, ());
    let client = TokenRecoveryContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_client = token::Client::new(&env, &token_id);
    let token_asset = token::StellarAssetClient::new(&env, &token_id);

    (env, client, admin, token_id, token_client, token_asset)
}

#[test]
fn test_recover_surplus_only() {
    let (env, client, _admin, token_id, token_client, token_asset) = setup();
    let contract_id = client.address.clone();
    let to = Address::generate(&env);

    token_asset.mint(&contract_id, &1_000);
    client.add_owed(&token_id, &600);

    assert_eq!(client.recoverable(&token_id), 400);
    client.recover(&token_id, &to, &400);
    assert_eq!(token_client.balance(&to), 400);
    assert_eq!(token_client.balance(&contract_id), 600);
    assert_eq!(client.owed(&token_id), 600);
}

#[test]
fn test_cannot_recover_owed() {
    let (env, client, _admin, token_id, _token_client, token_asset) = setup();
    let contract_id = client.address.clone();
    let to = Address::generate(&env);

    token_asset.mint(&contract_id, &1_000);
    client.add_owed(&token_id, &600);

    let res = client.try_recover(&token_id, &to, &401);
    assert_eq!(res, Err(Ok(Error::ExceedsSurplus)));
}

#[test]
fn test_no_surplus() {
    let (env, client, _admin, token_id, _token_client, token_asset) = setup();
    let contract_id = client.address.clone();
    let to = Address::generate(&env);

    token_asset.mint(&contract_id, &500);
    client.add_owed(&token_id, &500);

    let res = client.try_recover(&token_id, &to, &1);
    assert_eq!(res, Err(Ok(Error::NoSurplus)));
}

#[test]
fn test_invalid_amount() {
    let (env, client, _admin, token_id, _token_client, token_asset) = setup();
    let contract_id = client.address.clone();
    let to = Address::generate(&env);

    token_asset.mint(&contract_id, &100);

    let res = client.try_recover(&token_id, &to, &0);
    assert_eq!(res, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn test_settle_owed_then_recover() {
    let (env, client, _admin, token_id, token_client, token_asset) = setup();
    let contract_id = client.address.clone();
    let to = Address::generate(&env);

    token_asset.mint(&contract_id, &1_000);
    client.add_owed(&token_id, &600);
    client.settle_owed(&token_id, &600);

    assert_eq!(client.owed(&token_id), 0);
    assert_eq!(client.recoverable(&token_id), 1_000);
    client.recover(&token_id, &to, &1_000);
    assert_eq!(token_client.balance(&to), 1_000);
}

#[test]
fn test_initialize_twice_fails() {
    let (env, client, _admin, _token_id, _token_client, _token_asset) = setup();
    let other = Address::generate(&env);
    let res = client.try_initialize(&other);
    assert_eq!(res, Err(Ok(Error::AlreadyInitialized)));
}
