#![no_std]
// Closes #307: puzzle subscription NFT (starter: subscribe + renew + gate).
// Tiers, grace periods, and refunds are follow-ups.

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

#[contracttype]
#[derive(Clone)]
pub struct Subscription {
    pub owner: Address,
    pub expires_at: u64,
}

#[contracttype]
pub enum DataKey {
    Sub(Address),
}

#[contract]
pub struct SubscriptionNftContract;

#[contractimpl]
impl SubscriptionNftContract {
    /// Creates or renews `owner`'s subscription for `duration_secs` from now.
    pub fn subscribe(env: Env, owner: Address, duration_secs: u64) {
        owner.require_auth();
        let expires_at = env.ledger().timestamp() + duration_secs;
        env.storage()
            .instance()
            .set(&DataKey::Sub(owner.clone()), &Subscription { owner, expires_at });
    }

    /// Returns true if `owner` currently has an unexpired subscription.
    pub fn is_active(env: Env, owner: Address) -> bool {
        match env.storage().instance().get::<_, Subscription>(&DataKey::Sub(owner)) {
            Some(sub) => sub.expires_at > env.ledger().timestamp(),
            None => false,
        }
    }
}
