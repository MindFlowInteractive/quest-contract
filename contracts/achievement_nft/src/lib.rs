#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, String, Vec,
};
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, String, Vec};

#[contracttype]
#[derive(Clone)]
pub struct Achievement {
    pub owner: Address,
    pub puzzle_id: u32,
    pub metadata: String,
    pub timestamp: u64,
}

#[contracttype]
pub enum DataKey {
    Achievement(u32),          // Persistent: Individual NFT data
    OwnerCollection(Address),  // Persistent: List of IDs owned by an address
    NextTokenId,               // Instance: Counter for IDs
    TotalSupply,               // Instance: Current count of NFTs
    Admin,                     // Instance: Contract administrator
    PuzzleCompleted(Address, u32), // Tracks if a user has completed a puzzle
}

#[contract]
pub struct AchievementNFT;

#[contractimpl]
impl AchievementNFT {
    /// Initialize the contract and set the administrator.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::NextTokenId, &1u32);
        env.storage().instance().set(&DataKey::TotalSupply, &0u32);
    }

    /// Mark a puzzle as completed by a user.
    /// Admin function to mark a puzzle as completed for a user.
    pub fn mark_puzzle_completed(env: Env, user: Address, puzzle_id: u32) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        let key = DataKey::PuzzleCompleted(user.clone(), puzzle_id);
        env.storage()
            .persistent()
            .set(&key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&key, 100_000, 500_000);
    }

    /// Mint a new achievement NFT
    /// Mint a new achievement NFT (requires that the puzzle was previously marked completed).
    pub fn mint(env: Env, to: Address, puzzle_id: u32, metadata: String) -> u32 {
        to.require_auth();

        let completed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::PuzzleCompleted(to.clone(), puzzle_id))
            .unwrap_or(false);

        if !completed {
            panic!("Puzzle not completed");
        }

        Self::craftmint(env, to, puzzle_id, metadata)
        Self::mint_internal(env, to, puzzle_id, metadata)
    }

    /// Mint a new NFT for crafting purposes (testnet: no auth required).
    pub fn craftmint(env: Env, to: Address, puzzle_id: u32, metadata: String) -> u32 {
        // For testnet deployment, remove admin auth requirement
        // let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        // admin.require_auth();

        Self::mint_internal(env, to, puzzle_id, metadata)
    }

    fn mint_internal(env: Env, to: Address, puzzle_id: u32, metadata: String) -> u32 {
        let token_id: u32 = env.storage().instance().get(&DataKey::NextTokenId).unwrap();

        let achievement = Achievement {
            owner: to.clone(),
            puzzle_id,
            metadata,
            timestamp: env.ledger().timestamp(),
        };

        // Store Achievement
        let key = DataKey::Achievement(token_id);
        env.storage().persistent().set(&key, &achievement);
        env.storage().persistent().extend_ttl(&key, 100_000, 500_000);

        // Update Owner Collection
        let mut collection = Self::get_collection(env.clone(), to.clone());
        collection.push_back(token_id);
        let collection_key = DataKey::OwnerCollection(to.clone());
        env.storage().persistent().set(&collection_key, &collection);
        env.storage()
            .persistent()
            .extend_ttl(&collection_key, 100_000, 500_000);

        // Update Counters
        env.storage().instance().set(&DataKey::NextTokenId, &(token_id + 1));
        let total: u32 = env.storage().instance().get(&DataKey::TotalSupply).unwrap_or(0);
        env.storage().instance().set(&DataKey::TotalSupply, &(total + 1));

        // Emit Event
        env.events()
            .publish((symbol_short!("minted"), to.clone()), token_id);

        token_id
    }

    /// Transfers a token safely
    pub fn transfer(env: Env, from: Address, to: Address, token_id: u32) {
        from.require_auth();

        if from == to {
            panic!("Cannot transfer to self");
        }

        let mut achievement: Achievement = env
            .storage()
            .persistent()
            .get(&DataKey::Achievement(token_id))
            .expect("Token does not exist");

        if achievement.owner != from {
            panic!("Not the owner");
        }

        // Remove from 'from' collection
        let mut from_col = Self::get_collection(env.clone(), from.clone());
        let index = from_col.first_index_of(token_id).expect("ID not in collection");
        from_col.remove(index);
        env.storage().persistent().set(&DataKey::OwnerCollection(from.clone()), &from_col);
        env.storage().persistent().extend_ttl(&DataKey::OwnerCollection(from.clone()), 100_000, 500_000);

        // Add to 'to' collection
        let mut to_col = Self::get_collection(env.clone(), to.clone());
        to_col.push_back(token_id);
        env.storage().persistent().set(&DataKey::OwnerCollection(to.clone()), &to_col);
        env.storage().persistent().extend_ttl(&DataKey::OwnerCollection(to.clone()), 100_000, 500_000);

        // Update owner
        achievement.owner = to.clone();
        env.storage().persistent().set(&DataKey::Achievement(token_id), &achievement);
        env.storage().persistent().extend_ttl(&DataKey::Achievement(token_id), 100_000, 500_000);

        env.events().publish((symbol_short!("transfer"), from, to), token_id);
    }

    /// Returns the list of token IDs owned by an address.
    pub fn get_collection(env: Env, owner: Address) -> Vec<u32> {
        env.storage()
            .persistent()
            .get(&DataKey::OwnerCollection(owner))
            .unwrap_or(Vec::new(&env))
    }

    /// Get owner of a specific token.
    pub fn owner_of(env: Env, token_id: u32) -> Address {
        let achievement: Achievement = env
            .storage()
            .persistent()
            .get(&DataKey::Achievement(token_id))
            .expect("Token does not exist");
        achievement.owner
    }

    /// Returns the total number of NFTs minted.
    pub fn total_supply(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::TotalSupply).unwrap_or(0)
    }

    /// Destroys a token (testnet: no auth required for crafting).
    pub fn burn(env: Env, token_id: u32) {
        let achievement: Achievement = env
            .storage()
            .persistent()
            .get(&DataKey::Achievement(token_id))
            .expect("Token does not exist");

        // For testnet deployment, remove owner auth requirement
        // achievement.owner.require_auth();

        let mut collection = Self::get_collection(env.clone(), achievement.owner.clone());
        if let Some(index) = collection.first_index_of(token_id) {
            collection.remove(index);
            env.storage().persistent().set(&DataKey::OwnerCollection(achievement.owner.clone()), &collection);
        }

        env.storage().persistent().remove(&DataKey::Achievement(token_id));
        let total: u32 = env.storage().instance().get(&DataKey::TotalSupply).unwrap();
        env.storage().instance().set(&DataKey::TotalSupply, &(total - 1));

        env.events().publish((symbol_short!("burn"), achievement.owner), token_id);
    }

    /// Returns full achievement details.
    pub fn get_achievement(env: Env, token_id: u32) -> Option<Achievement> {
        env.storage().persistent().get(&DataKey::Achievement(token_id))
    }

    /// Returns unique puzzle IDs owned by `owner` (derived from achievement NFTs).
    pub fn puzzle_ids_of(env: Env, owner: Address) -> Vec<u32> {
        let token_ids = Self::get_collection(env.clone(), owner);
        let mut puzzles = Vec::new(&env);

        for tid in token_ids.iter() {
            let token_id = tid.clone();
            if let Some(a) = Self::get_achievement(env.clone(), token_id) {
                if !puzzles.contains(&a.puzzle_id) {
                    puzzles.push_back(a.puzzle_id);
                }
            }
        }

        puzzles
    }

    /// True if `owner` owns an achievement NFT whose `puzzle_id` equals `puzzle_id`.
    pub fn has_puzzle(env: Env, owner: Address, puzzle_id: u32) -> bool {
        let puzzles = Self::puzzle_ids_of(env, owner);
        puzzles.contains(&puzzle_id)
    }
}

mod test;
