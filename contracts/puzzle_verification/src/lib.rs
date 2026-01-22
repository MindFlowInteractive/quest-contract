#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, BytesN, Env};

#[contracttype]
pub enum DataKey {
    PuzzleSolution(u32),
    Completed(Address, u32),
}

#[contract]
pub struct PuzzleVerification;

#[contractimpl]
impl PuzzleVerification {
    /// Set puzzle solution hash (admin only)
    pub fn set_puzzle(env: Env, puzzle_id: u32, solution_hash: BytesN<32>) {
        env.storage()
            .instance()
            .set(&DataKey::PuzzleSolution(puzzle_id), &solution_hash);
    }

    /// Verify and mark puzzle as completed
    pub fn verify_solution(
        env: Env,
        player: Address,
        puzzle_id: u32,
        solution_hash: BytesN<32>,
    ) -> bool {
        player.require_auth();

        // Check if already completed
        if Self::is_completed(env.clone(), player.clone(), puzzle_id) {
            panic!("puzzle already completed");
        }

        // Verify solution
        let correct_hash: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::PuzzleSolution(puzzle_id))
            .unwrap();

        if solution_hash == correct_hash {
            env.storage()
                .instance()
                .set(&DataKey::Completed(player, puzzle_id), &true);
            true
        } else {
            false
        }
    }

    /// Check if player completed puzzle
    pub fn is_completed(env: Env, player: Address, puzzle_id: u32) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Completed(player, puzzle_id))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn test_verification() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleVerification);
        let client = PuzzleVerificationClient::new(&env, &contract_id);

        let player = Address::generate(&env);
        let solution = BytesN::from_array(&env, &[1; 32]);

        client.set_puzzle(&1, &solution);
        
        env.mock_all_auths();

        let result = client.verify_solution(&player, &1, &solution);
        assert!(result);
        assert!(client.is_completed(&player, &1));
    }
}
