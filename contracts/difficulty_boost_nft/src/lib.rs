use soroban_sdk::{contractimpl, Address, Env, Symbol};

#[derive(Clone)]
pub struct BoostNft {
    pub id: u64,
    pub owner: Address,
    pub multiplier_bps: u32, // e.g. 15000 = 1.5x
    pub expires_at: u64,
    pub active: bool,
}

pub struct DifficultyBoostNftContract;

const MAX_ACTIVE_STACK: u32 = 3;

#[contractimpl]
impl DifficultyBoostNftContract {
    pub fn mint_boost(env: Env, owner: Address, multiplier_bps: u32, duration: u64) -> u64 {
        owner.require_auth();
        assert!(multiplier_bps > 10_000, "multiplier must exceed 1.0x");
        let now = env.ledger().timestamp();
        let id: u64 = env.storage().instance().get(&Symbol::new(&env, "next_id")).unwrap_or(0);
        env.storage().instance().set(&Symbol::new(&env, "next_id"), &(id + 1));

        env.storage().persistent().set(&id, &BoostNft {
            id,
            owner,
            multiplier_bps,
            expires_at: now + duration,
            active: false,
        });
        id
    }

    /// Activates a boost, enforcing the stacking cap for the caller.
    pub fn activate(env: Env, owner: Address, boost_id: u64, currently_active: u32) {
        owner.require_auth();
        assert!(currently_active < MAX_ACTIVE_STACK, "stacking limit reached");
        let mut b: BoostNft = env.storage().persistent().get(&boost_id).unwrap();
        assert!(env.ledger().timestamp() < b.expires_at, "boost expired");
        b.active = true;
        env.storage().persistent().set(&boost_id, &b);
    }
}
