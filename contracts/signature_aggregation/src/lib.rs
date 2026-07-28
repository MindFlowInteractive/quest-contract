#![no_std]
// Closes #303: signature aggregation and batch verification (starter: batch
// verifies individual ed25519 signatures). True BLS aggregation (single
// aggregated signature via pairing) is a larger follow-up requiring the
// host's BLS12-381 crypto functions.

use soroban_sdk::{contract, contractimpl, crypto::Hash, BytesN, Bytes, Env, Vec};

#[contract]
pub struct SignatureAggregationContract;

#[contractimpl]
impl SignatureAggregationContract {
    /// Verifies that every (message, signature) pair is valid under its
    /// corresponding public key. Returns false on a length mismatch; panics
    /// (host trap) on the first invalid signature, per `ed25519_verify`.
    pub fn verify_batch(
        env: Env,
        public_keys: Vec<BytesN<32>>,
        messages: Vec<Bytes>,
        signatures: Vec<BytesN<64>>,
    ) -> bool {
        if public_keys.len() != messages.len() || messages.len() != signatures.len() {
            return false;
        }
        for i in 0..public_keys.len() {
            let pk = public_keys.get(i).unwrap();
            let msg = messages.get(i).unwrap();
            let sig = signatures.get(i).unwrap();
            let digest: Hash<32> = env.crypto().sha256(&msg);
            env.crypto().ed25519_verify(&pk, &Bytes::from_array(&env, &digest.to_array()), &sig);
        }
        true
    }
}
