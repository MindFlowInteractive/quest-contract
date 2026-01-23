#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Bytes, BytesN, Env, Symbol,
};

#[contracttype]
#[derive(Clone)]
pub struct PuzzleMeta {
    pub id: u32,
    pub solution_hash: BytesN<32>,
    pub start_ts: u64,
    pub end_ts: u64,
    pub difficulty: u32,
    pub reward_points: i128,
}

#[cfg(test)]
mod double_claim_test {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::testutils::Ledger as _;

    #[test]
    #[should_panic(expected = "puzzle already completed")]
    fn test_double_claim_panics() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleVerification);
        let client = PuzzleVerificationClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let player = Address::generate(&env);

        env.mock_all_auths();
        client.initialize(&admin);

        env.ledger().set_timestamp(1_000);

        let preimage = Bytes::from_array(&env, &[9u8; 4]);
        let hash: BytesN<32> = env.crypto().sha256(&preimage).into();
        let now = env.ledger().timestamp();
        client.set_puzzle(&7, &hash, &(now - 1), &(now + 1000), &3, &5);

        assert!(client.verify_solution(&player, &7, &preimage));
        // Second attempt should panic
        let _ = client.verify_solution(&player, &7, &preimage);
    }
}

#[contracttype]
pub enum DataKey {
    Admin,
    Puzzle(u32),
    Completed(Address, u32),
    Rewards(Address),
}

#[contract]
pub struct PuzzleVerification;

#[contractimpl]
impl PuzzleVerification {
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    fn require_admin(env: &Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin");
        admin.require_auth();
    }

    /// Admin: define or update a puzzle
    pub fn set_puzzle(
        env: Env,
        puzzle_id: u32,
        solution_hash: BytesN<32>,
        start_ts: u64,
        end_ts: u64,
        difficulty: u32,
        reward_points: i128,
    ) {
        Self::require_admin(&env);
        if end_ts <= start_ts {
            panic!("invalid time window");
        }
        let meta = PuzzleMeta {
            id: puzzle_id,
            solution_hash,
            start_ts,
            end_ts,
            difficulty,
            reward_points,
        };
        env.storage()
            .instance()
            .set(&DataKey::Puzzle(puzzle_id), &meta);
    }

    /// Verify solution preimage by hashing on-chain and credit rewards once
    pub fn verify_solution(
        env: Env,
        player: Address,
        puzzle_id: u32,
        solution_preimage: Bytes,
    ) -> bool {
        player.require_auth();

        // Prevent double-claim
        if Self::is_completed(env.clone(), player.clone(), puzzle_id) {
            panic!("puzzle already completed");
        }

        // Load puzzle and enforce time window
        let meta: PuzzleMeta = env
            .storage()
            .instance()
            .get(&DataKey::Puzzle(puzzle_id))
            .expect("puzzle");
        let now = env.ledger().timestamp();
        if now < meta.start_ts || now > meta.end_ts {
            panic!("puzzle not active");
        }

        // Hash the provided solution and compare
        let computed: BytesN<32> = env.crypto().sha256(&solution_preimage).into();
        if computed != meta.solution_hash {
            return false;
        }

        // Mark completed
        env.storage()
            .instance()
            .set(&DataKey::Completed(player.clone(), puzzle_id), &true);

        // Difficulty-based reward scaling: scale reward_points by difficulty factor (>=1)
        let scaled = meta.reward_points * (meta.difficulty as i128).max(1);
        let mut rewards: i128 = env
            .storage()
            .instance()
            .get(&DataKey::Rewards(player.clone()))
            .unwrap_or(0);
        rewards += scaled;
        env.storage()
            .instance()
            .set(&DataKey::Rewards(player.clone()), &rewards);

        // Emit completion event
        env.events().publish(
            (Symbol::new(&env, "puzzle"), Symbol::new(&env, "completed")),
            (player, puzzle_id, scaled),
        );

        true
    }

    pub fn is_completed(env: Env, player: Address, puzzle_id: u32) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Completed(player, puzzle_id))
            .unwrap_or(false)
    }

    pub fn rewards_of(env: Env, player: Address) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::Rewards(player))
            .unwrap_or(0)
    }

    pub fn get_puzzle(env: Env, puzzle_id: u32) -> Option<PuzzleMeta> {
        env.storage().instance().get(&DataKey::Puzzle(puzzle_id))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::testutils::Ledger as _;

    #[test]
    fn test_verification_flow() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleVerification);
        let client = PuzzleVerificationClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let player = Address::generate(&env);

        env.mock_all_auths();
        client.initialize(&admin);

        // Set a safe ledger timestamp to avoid underflow on subtraction
        env.ledger().set_timestamp(1_000);

        // Prepare puzzle meta
        let preimage = Bytes::from_array(&env, &[7u8; 5]);
        let hash: BytesN<32> = env.crypto().sha256(&preimage).into();
        let now = env.ledger().timestamp();
        client.set_puzzle(&1, &hash, &(now - 1), &(now + 1000), &2, &50);

        // Wrong preimage
        let wrong = Bytes::from_array(&env, &[8u8; 5]);
        assert_eq!(client.verify_solution(&player, &1, &wrong), false);
        assert_eq!(client.is_completed(&player, &1), false);

        // Correct preimage
        assert_eq!(client.verify_solution(&player, &1, &preimage), true);
        assert_eq!(client.is_completed(&player, &1), true);
        // Reward scaled: 50 * difficulty(2) = 100
        assert_eq!(client.rewards_of(&player), 100);

        // No double-claim: verified above; dedicated panic test below
    }

    #[test]
    #[should_panic(expected = "puzzle not active")]
    fn test_expiration_enforced() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleVerification);
        let client = PuzzleVerificationClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let player = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);

        env.ledger().set_timestamp(1_000);

        let preimage = Bytes::from_array(&env, &[1u8; 3]);
        let hash: BytesN<32> = env.crypto().sha256(&preimage).into();
        let now = env.ledger().timestamp();
        // Expired window
        client.set_puzzle(&42, &hash, &(now - 100), &(now - 50), &1, &10);

        // Attempt verify should panic
        let _ = client.verify_solution(&player, &42, &preimage);
    }
}
