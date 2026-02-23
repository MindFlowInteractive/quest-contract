#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, contractevent, Address, Env, String, Vec};

#[contracttype]
#[derive(Clone)]
pub struct DynamicNft {
    pub owner: Address,
    pub level: u32,
    pub rarity: u32,
    pub traits: String,
    pub metadata: String,
    pub history: Vec<String>,
    pub minted_at: u64,
}

#[contracttype]
pub enum DataKey {
    Admin(Address),
    Verifier(Address),
    DynamicNft(u32),
    NextNftId,
}

#[contract]
pub struct DynamicNftContract;

#[contractevent]
#[derive(Clone)]
pub struct MintEvent {
    pub owner: Address,
    pub token_id: u32,
}

#[contractevent]
#[derive(Clone)]
pub struct EvolveMilestoneEvent {
    pub token_id: u32,
}

#[contractevent]
#[derive(Clone)]
pub struct EvolveTimeEvent {
    pub token_id: u32,
}

#[contractevent]
#[derive(Clone)]
pub struct DowngradeEvent {
    pub token_id: u32,
}

#[contractevent]
#[derive(Clone)]
pub struct FuseEvent {
    pub owner: Address,
    pub token_id: u32,
}

#[contractimpl]
impl DynamicNftContract {
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        if env.storage().persistent().has(&DataKey::Admin(admin.clone())) {
            panic!("Already initialized");
        }
        env.storage()
            .persistent()
            .set(&DataKey::Admin(admin.clone()), &true);
        env.storage().persistent().set(&DataKey::NextNftId, &1u32);
    }

    pub fn add_verifier(env: Env, admin: Address, verifier: Address) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);
        env.storage().persistent().set(&DataKey::Verifier(verifier), &true);
    }

    pub fn remove_verifier(env: Env, admin: Address, verifier: Address) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);
        env.storage().persistent().remove(&DataKey::Verifier(verifier));
    }

    pub fn mint(env: Env, minter: Address, owner: Address, metadata: String, traits: String) -> u32 {
        minter.require_auth();

        let next: u32 = env.storage().persistent().get(&DataKey::NextNftId).unwrap();
        let nft = DynamicNft {
            owner: owner.clone(),
            level: 1,
            rarity: 1,
            traits: traits.clone(),
            metadata: metadata.clone(),
            history: Vec::new(&env),
            minted_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&DataKey::DynamicNft(next), &nft);
        env.storage().persistent().set(&DataKey::NextNftId, &(next + 1));
        env.events().publish_event(&MintEvent { owner: owner.clone(), token_id: next });
        next
    }

    // evolve by milestone (admin or verifier in governance)
    pub fn evolve_milestone(env: Env, submitter: Address, token_id: u32, level_inc: u32, rarity_inc: u32, new_traits: Option<String>) {
        submitter.require_auth();
        Self::assert_admin_or_verifier(&env, &submitter);
        let mut nft: DynamicNft = env.storage().persistent().get(&DataKey::DynamicNft(token_id)).unwrap();
        nft.level = nft.level.saturating_add(level_inc);
        nft.rarity = nft.rarity.saturating_add(rarity_inc);
        if let Some(t) = new_traits {
            nft.traits = t;
        }
        // record evolution event in history
        let mut hist = nft.history.clone();
        hist.push_back(String::from_str(&env, "evolved_milestone"));
        nft.history = hist;
        env.storage().persistent().set(&DataKey::DynamicNft(token_id), &nft);
        env.events().publish_event(&EvolveMilestoneEvent { token_id });
    }

    // time-based evolution callable by anyone; checks elapsed time
    pub fn evolve_time(env: Env, caller: Address, token_id: u32, required_secs: u64) {
        caller.require_auth();
        let mut nft: DynamicNft = env.storage().persistent().get(&DataKey::DynamicNft(token_id)).unwrap();
        let now = env.ledger().timestamp();
        if now < nft.minted_at + required_secs {
            panic!("Not ready for time evolution");
        }
        nft.level = nft.level.saturating_add(1);
        // record time evolution in history
        let mut hist = nft.history.clone();
        hist.push_back(String::from_str(&env, "time_evolved"));
        nft.history = hist;
        nft.minted_at = now; // reset timer for further evolutions
        env.storage().persistent().set(&DataKey::DynamicNft(token_id), &nft);
        env.events().publish_event(&EvolveTimeEvent { token_id });
    }

    pub fn downgrade(env: Env, submitter: Address, token_id: u32, level_dec: u32) {
        submitter.require_auth();
        Self::assert_admin_or_verifier(&env, &submitter);
        let mut nft: DynamicNft = env.storage().persistent().get(&DataKey::DynamicNft(token_id)).unwrap();
        nft.level = nft.level.saturating_sub(level_dec);
        let mut hist = nft.history.clone();
        hist.push_back(String::from_str(&env, "downgraded"));
        nft.history = hist;
        env.storage().persistent().set(&DataKey::DynamicNft(token_id), &nft);
        env.events().publish_event(&DowngradeEvent { token_id });
    }

    // fuse two NFTs into a new one; owner must be same for both
    pub fn fuse(env: Env, submitter: Address, token_a: u32, token_b: u32) -> u32 {
        submitter.require_auth();
        let nft_a: DynamicNft = env.storage().persistent().get(&DataKey::DynamicNft(token_a)).unwrap();
        let nft_b: DynamicNft = env.storage().persistent().get(&DataKey::DynamicNft(token_b)).unwrap();
        if nft_a.owner != nft_b.owner {
            panic!("Owners must match to fuse");
        }
        // only owner can fuse
        if submitter != nft_a.owner {
            panic!("Only owner can fuse tokens");
        }
        // create fused NFT: summed level, higher rarity, combined traits
        let owner = nft_a.owner.clone();
        let fused_level = nft_a.level.saturating_add(nft_b.level);
        let fused_rarity = if nft_a.rarity > nft_b.rarity { nft_a.rarity } else { nft_b.rarity } + 1u32;
        let combined_traits = nft_a.traits.clone();
        let combined_metadata = nft_a.metadata.clone();

        // simple burn: remove old entries
        env.storage().persistent().remove(&DataKey::DynamicNft(token_a));
        env.storage().persistent().remove(&DataKey::DynamicNft(token_b));

        let next: u32 = env.storage().persistent().get(&DataKey::NextNftId).unwrap();
        let mut new_hist = Vec::new(&env);
        new_hist.push_back(String::from_str(&env, "fused"));

        let nft = DynamicNft {
            owner: owner.clone(),
            level: fused_level,
            rarity: fused_rarity,
            traits: combined_traits,
            metadata: combined_metadata,
            history: new_hist,
            minted_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&DataKey::DynamicNft(next), &nft);
        env.storage().persistent().set(&DataKey::NextNftId, &(next + 1));
        env.events().publish_event(&FuseEvent { owner: owner.clone(), token_id: next });
        next
    }

    pub fn get_nft(env: Env, token_id: u32) -> Option<DynamicNft> {
        env.storage().persistent().get(&DataKey::DynamicNft(token_id))
    }

    pub fn get_history(env: Env, token_id: u32) -> Option<Vec<String>> {
        let nft: Option<DynamicNft> = env.storage().persistent().get(&DataKey::DynamicNft(token_id));
        match nft {
            Some(n) => Some(n.history),
            None => None,
        }
    }

    // helpers
    fn assert_admin(env: &Env, admin: &Address) {
        let is_admin: bool = env.storage().persistent().get(&DataKey::Admin(admin.clone())).unwrap_or(false);
        if !is_admin {
            panic!("Unauthorized");
        }
    }

    fn assert_admin_or_verifier(env: &Env, submitter: &Address) {
        // check admin
        let is_admin: bool = env.storage().persistent().get(&DataKey::Admin(submitter.clone())).unwrap_or(false);
        if is_admin {
            return;
        }
        let is_verifier: bool = env.storage().persistent().get(&DataKey::Verifier(submitter.clone())).unwrap_or(false);
        if !is_verifier {
            panic!("Unauthorized");
        }
    }
}
