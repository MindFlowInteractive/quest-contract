#![no_std]
// Closes #301: reward distribution with proof-based claims (starter: single
// root + claimed tracking). Batch distributions, expiration, and unclaimed
// recovery are follow-ups.

use soroban_sdk::{contract, contractimpl, contracttype, Address, Bytes, BytesN, Env, Vec};

#[contracttype]
pub enum DataKey {
    Root,
    Claimed(Address),
}

#[contract]
pub struct RewardDistributionContract;

#[contractimpl]
impl RewardDistributionContract {
    /// Sets the Merkle root describing the eligible (claimant, amount) set.
    pub fn set_root(env: Env, root: BytesN<32>) {
        env.storage().instance().set(&DataKey::Root, &root);
    }

    /// Claims `amount` for `claimant` given a sibling-hash `proof` for their leaf.
    pub fn claim(env: Env, claimant: Address, amount: i128, leaf: BytesN<32>, proof: Vec<BytesN<32>>) -> bool {
        if env.storage().instance().has(&DataKey::Claimed(claimant.clone())) {
            return false;
        }
        let root: BytesN<32> = match env.storage().instance().get(&DataKey::Root) {
            Some(r) => r,
            None => return false,
        };
        let mut computed = leaf;
        for sibling in proof.iter() {
            let mut combined = Bytes::new(&env);
            combined.append(&Bytes::from_array(&env, &computed.to_array()));
            combined.append(&Bytes::from_array(&env, &sibling.to_array()));
            computed = env.crypto().sha256(&combined).into();
        }
        if computed != root {
            return false;
        }
        env.storage().instance().set(&DataKey::Claimed(claimant), &amount);
        true
    }
}
