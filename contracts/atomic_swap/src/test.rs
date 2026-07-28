#![cfg(test)]
use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, BytesN, Env,
};

fn create_token<'a>(env: &Env, admin: &Address) -> (Address, token::StellarAssetClient<'a>) {
    let contract_id = env.register_stellar_asset_contract_v2(admin.clone()).address();
    let client = token::StellarAssetClient::new(env, &contract_id);
    (contract_id, client)
}

fn create_id(env: &Env, num: u8) -> BytesN<32> {
    let mut arr = [0u8; 32];
    arr[31] = num;
    BytesN::from_array(env, &arr)
}

fn create_secret(env: &Env, val: u8) -> BytesN<32> {
    let mut arr = [0u8; 32];
    arr[0] = val;
    BytesN::from_array(env, &arr)
}

#[test]
fn test_happy_path() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, AtomicSwapContract);
    let client = AtomicSwapContractClient::new(&env, &contract_id);

    let initiator = Address::generate(&env);
    let participant = Address::generate(&env);
    let admin = Address::generate(&env);

    let (token_a, token_a_admin) = create_token(&env, &admin);
    let (token_b, token_b_admin) = create_token(&env, &admin);

    token_a_admin.mint(&initiator, &1000);
    token_b_admin.mint(&participant, &2000);

    let secret = create_secret(&env, 42);
    let secret_bytes = soroban_sdk::Bytes::from_slice(&env, secret.to_array().as_slice());
    let hashlock = env.crypto().sha256(&secret_bytes);

    let swap1_id = create_id(&env, 1);
    let timelock1 = 200;

    env.ledger().with_mut(|l| l.timestamp = 100);

    // Initiator creates swap leg 1
    client.create_swap(
        &swap1_id,
        &initiator,
        &participant,
        &token_a,
        &1000,
        &hashlock,
        &timelock1,
    );

    let swap1 = client.get_swap(&swap1_id);
    assert_eq!(swap1.status, SwapStatus::Initiated);

    let token_a_client = token::Client::new(&env, &token_a);
    assert_eq!(token_a_client.balance(&initiator), 0);
    assert_eq!(token_a_client.balance(&contract_id), 1000);

    // Participant creates swap leg 2
    let swap2_id = create_id(&env, 2);
    let timelock2 = 150; // must be < timelock1

    client.accept_swap(
        &swap1_id,
        &swap2_id,
        &participant,
        &token_b,
        &2000,
        &timelock2,
    );

    let swap2 = client.get_swap(&swap2_id);
    assert_eq!(swap2.status, SwapStatus::Initiated);

    // Initiator reveals secret and withdraws token_b from leg 2
    client.withdraw(&swap2_id, &secret);
    let token_b_client = token::Client::new(&env, &token_b);
    assert_eq!(token_b_client.balance(&initiator), 2000);

    // Participant sees secret on-chain and uses it to withdraw token_a from leg 1
    client.withdraw(&swap1_id, &secret);
    assert_eq!(token_a_client.balance(&participant), 1000);
}

#[test]
fn test_refund() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, AtomicSwapContract);
    let client = AtomicSwapContractClient::new(&env, &contract_id);

    let depositor = Address::generate(&env);
    let claimer = Address::generate(&env);
    let admin = Address::generate(&env);

    let (token, token_admin) = create_token(&env, &admin);
    token_admin.mint(&depositor, &1000);

    let secret = create_secret(&env, 42);
    let hashlock = env.crypto().sha256(&soroban_sdk::Bytes::from_slice(&env, secret.to_array().as_slice()));
    let swap_id = create_id(&env, 1);

    env.ledger().with_mut(|l| l.timestamp = 100);

    client.create_swap(&swap_id, &depositor, &claimer, &token, &1000, &hashlock, &200);

    // Try to refund early (fails)
    assert!(client.try_refund(&swap_id).is_err());

    // Advance time
    env.ledger().with_mut(|l| l.timestamp = 250);

    // Refund succeeds
    client.refund(&swap_id);
    let swap = client.get_swap(&swap_id);
    assert_eq!(swap.status, SwapStatus::Refunded);

    let token_client = token::Client::new(&env, &token);
    assert_eq!(token_client.balance(&depositor), 1000);
}

#[test]
fn test_hash_mismatch() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, AtomicSwapContract);
    let client = AtomicSwapContractClient::new(&env, &contract_id);

    let depositor = Address::generate(&env);
    let claimer = Address::generate(&env);
    let admin = Address::generate(&env);

    let (token, token_admin) = create_token(&env, &admin);
    token_admin.mint(&depositor, &1000);

    let secret = create_secret(&env, 42);
    let hashlock = env.crypto().sha256(&soroban_sdk::Bytes::from_slice(&env, secret.to_array().as_slice()));
    let swap_id = create_id(&env, 1);

    env.ledger().with_mut(|l| l.timestamp = 100);

    client.create_swap(&swap_id, &depositor, &claimer, &token, &1000, &hashlock, &200);

    let wrong_secret = create_secret(&env, 99);
    assert!(client.try_withdraw(&swap_id, &wrong_secret).is_err());
}
