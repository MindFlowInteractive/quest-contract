#![no_std]
// Closes #304: Merkle tree proof verification (starter: root storage + leaf
// verification against a single active root). Batch verification, multiple
// historical roots, and replay protection are follow-ups.

use soroban_sdk::{contract, contractimpl, contracttype, Bytes, BytesN, Env, Vec};

#[contracttype]
pub enum DataKey {
    Root,
}

#[contract]
pub struct MerkleTreeContract;

#[contractimpl]
impl MerkleTreeContract {
    /// Sets the active Merkle root. Admin auth is intentionally out of scope
    /// for this starter; callers should gate this behind an admin check.
    pub fn set_root(env: Env, root: BytesN<32>) {
        env.storage().instance().set(&DataKey::Root, &root);
    }

    /// Verifies `leaf` against the stored root using a sibling-hash `proof`.
    pub fn verify_proof(env: Env, leaf: BytesN<32>, proof: Vec<BytesN<32>>) -> bool {
        let root: BytesN<32> = match env.storage().instance().get(&DataKey::Root) {
            Some(r) => r,
            None => return false,
        };

        let mut computed = leaf;
        for sibling in proof.iter() {
            let mut combined = Bytes::new(&env);
            if computed.to_array() <= sibling.to_array() {
                combined.append(&Bytes::from_array(&env, &computed.to_array()));
                combined.append(&Bytes::from_array(&env, &sibling.to_array()));
            } else {
                combined.append(&Bytes::from_array(&env, &sibling.to_array()));
                combined.append(&Bytes::from_array(&env, &computed.to_array()));
            }
            computed = env.crypto().sha256(&combined).into();
        }

        computed == root
    }
}
