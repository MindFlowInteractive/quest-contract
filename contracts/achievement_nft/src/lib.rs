#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, Vec};

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
    Achievement(u32),
    NextTokenId,
    TotalSupply,
}

#[contract]
pub struct AchievementNFT;

#[contractimpl]
impl AchievementNFT {
    /// Initialize the contract
    pub fn initialize(env: Env) {
        env.storage().instance().set(&DataKey::NextTokenId, &1u32);
        env.storage().instance().set(&DataKey::TotalSupply, &0u32);
    }

    /// Mint a new achievement NFT
    pub fn mint(
        env: Env,
        to: Address,
        puzzle_id: u32,
        metadata: String,
    ) -> u32 {
        to.require_auth();

        let token_id: u32 = env
            .storage()
            .instance()
            .get(&DataKey::NextTokenId)
            .unwrap_or(1);

        let achievement = Achievement {
            owner: to.clone(),
            puzzle_id,
            metadata,
            timestamp: env.ledger().timestamp(),
        };

        env.storage()
            .instance()
            .set(&DataKey::Achievement(token_id), &achievement);

        env.storage()
            .instance()
            .set(&DataKey::NextTokenId, &(token_id + 1));

        let total_supply: u32 = env
            .storage()
            .instance()
            .get(&DataKey::TotalSupply)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalSupply, &(total_supply + 1));

        token_id
    }

    /// Get achievement details
    pub fn get_achievement(env: Env, token_id: u32) -> Option<Achievement> {
        env.storage()
            .instance()
            .get(&DataKey::Achievement(token_id))
    }

    /// Get total supply
    pub fn total_supply(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::TotalSupply)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn test_mint() {
        let env = Env::default();
        let contract_id = env.register_contract(None, AchievementNFT);
        let client = AchievementNFTClient::new(&env, &contract_id);

        client.initialize();

        let user = Address::generate(&env);
        let metadata = String::from_str(&env, "First Puzzle Completed");

        env.mock_all_auths(); // <-- required for require_auth()

        let token_id = client.mint(&user, &1, &metadata);

        assert_eq!(token_id, 1);
        assert_eq!(client.total_supply(), 1);

        let achievement = client.get_achievement(&token_id).unwrap();
        assert_eq!(achievement.puzzle_id, 1);
    }
} // <-- THIS closes mod test
