#![cfg(test)]
extern crate std;
use super::*;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Env, Symbol, Vec,
};

#[test]
fn test_oracle_flow() {
    let env = Env::default();
    env.mock_all_auths();

    // Set time
    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
    });

    let contract_id = env.register_contract(None, OracleContract);
    let client = OracleContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    // Generate signers
    let mut csprng = OsRng;
    let signer1_key = SigningKey::generate(&mut csprng);
    let signer2_key = SigningKey::generate(&mut csprng);
    let signer3_key = SigningKey::generate(&mut csprng);

    let signer1_pub = VerifyingKey::from(&signer1_key);
    let signer2_pub = VerifyingKey::from(&signer2_key);
    let signer3_pub = VerifyingKey::from(&signer3_key);

    let mut signers_vec = Vec::new(&env);
    signers_vec.push_back(BytesN::from_array(&env, &signer1_pub.to_bytes()));
    signers_vec.push_back(BytesN::from_array(&env, &signer2_pub.to_bytes()));
    signers_vec.push_back(BytesN::from_array(&env, &signer3_pub.to_bytes()));

    // Initialize with threshold 2
    client.initialize(&admin, &signers_vec, &2);

    // Prepare data
    // env.ledger().timestamp() is 1000.
    let asset = Symbol::new(&env, "XLM");
    let price: i128 = 1000000;
    let timestamp: u64 = 1000;
    let round_id: u64 = 1;

    // Construct payload to sign
    let payload_tuple = (
        asset.clone(),
        price,
        timestamp,
        round_id,
        contract_id.clone(),
    );
    let payload_bytes = payload_tuple.to_xdr(&env);

    // Convert soroban Bytes to slice using std::vec
    let len = payload_bytes.len() as usize;
    let mut vec_bytes = std::vec![0u8; len];
    payload_bytes.copy_into_slice(&mut vec_bytes);

    // Sign with signer 1 and 2
    let sig1 = signer1_key.sign(&vec_bytes);
    let sig2 = signer2_key.sign(&vec_bytes);

    // Prepare signatures arg
    let mut signatures = Vec::new(&env);
    signatures.push_back((
        BytesN::from_array(&env, &signer1_pub.to_bytes()),
        BytesN::from_array(&env, &sig1.to_bytes()),
    ));
    signatures.push_back((
        BytesN::from_array(&env, &signer2_pub.to_bytes()),
        BytesN::from_array(&env, &sig2.to_bytes()),
    ));

    // Submit
    client.submit_price(&asset, &price, &timestamp, &round_id, &signatures);

    // Verify
    let data = client.get_price(&asset);
    assert_eq!(data.price, price);
    assert_eq!(data.timestamp, timestamp);

    // Test Stale Price
    env.ledger().with_mut(|li| {
        li.timestamp = 5000; // > 1000 + 3600
    });

    let res = client.try_get_price(&asset);
    assert!(res.is_err()); // Should be StalePrice

    // Test Insufficient Signatures
    let round_id_2 = 2;
    let timestamp_2 = 5000;
    let payload_tuple_2 = (
        asset.clone(),
        price,
        timestamp_2,
        round_id_2,
        contract_id.clone(),
    );
    let payload_bytes_2 = payload_tuple_2.to_xdr(&env);
    let len2 = payload_bytes_2.len() as usize;
    let mut vec_bytes_2 = std::vec![0u8; len2];
    payload_bytes_2.copy_into_slice(&mut vec_bytes_2);

    let sig1_2 = signer1_key.sign(&vec_bytes_2);

    let mut signatures_insufficient = Vec::new(&env);
    signatures_insufficient.push_back((
        BytesN::from_array(&env, &signer1_pub.to_bytes()),
        BytesN::from_array(&env, &sig1_2.to_bytes()),
    ));

    let res_insufficient = client.try_submit_price(
        &asset,
        &price,
        &timestamp_2,
        &round_id_2,
        &signatures_insufficient,
    );
    assert!(res_insufficient.is_err());
}
