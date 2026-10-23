#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, BytesN, Env};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitWindow {
    pub start_ledger: u32,
    pub end_ledger: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Window(u32),
    Commitment(Address, u32),
}

#[contract]
pub struct CommitReveal;

#[contractimpl]
impl CommitReveal {
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

    /// Admin configures the commitment window for a puzzle round.
    pub fn set_window(env: Env, puzzle_id: u32, start_ledger: u32, end_ledger: u32) {
        Self::require_admin(&env);
        if end_ledger <= start_ledger {
            panic!("invalid window");
        }
        let window = CommitWindow {
            start_ledger,
            end_ledger,
        };
        env.storage()
            .instance()
            .set(&DataKey::Window(puzzle_id), &window);
    }

    /// Player commits a hash of their solution. Rejected if outside the window
    /// or if a commitment already exists for this player and puzzle.
    pub fn commit(env: Env, player: Address, puzzle_id: u32, hash: BytesN<32>) {
        player.require_auth();

        let window: CommitWindow = env
            .storage()
            .instance()
            .get(&DataKey::Window(puzzle_id))
            .expect("window");

        let now = env.ledger().sequence();
        if now < window.start_ledger || now > window.end_ledger {
            panic!("commit window closed");
        }

        let key = DataKey::Commitment(player.clone(), puzzle_id);
        if env.storage().instance().has(&key) {
            panic!("already committed");
        }

        env.storage().instance().set(&key, &hash);
    }

    /// Returns the stored commitment hash for a player and puzzle, if any.
    pub fn get_commitment(env: Env, player: Address, puzzle_id: u32) -> Option<BytesN<32>> {
        env.storage()
            .instance()
            .get(&DataKey::Commitment(player, puzzle_id))
    }

    /// Returns the commitment window configured for a puzzle, if any.
    pub fn get_window(env: Env, puzzle_id: u32) -> Option<CommitWindow> {
        env.storage().instance().get(&DataKey::Window(puzzle_id))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::testutils::Ledger as _;

    fn setup() -> (Env, CommitRevealClient<'static>, Address, Address) {
        let env = Env::default();
        let contract_id = env.register_contract(None, CommitReveal);
        let client = CommitRevealClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let player = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);
        (env, client, admin, player)
    }

    #[test]
    fn test_commit_and_retrieve() {
        let (env, client, _admin, player) = setup();
        env.ledger().set_sequence_number(100);
        client.set_window(&1, &100, &200);

        let hash = BytesN::from_array(&env, &[9u8; 32]);
        client.commit(&player, &1, &hash);

        assert_eq!(client.get_commitment(&player, &1), Some(hash));
        assert_eq!(client.get_window(&1).unwrap().end_ledger, 200);
    }

    #[test]
    #[should_panic(expected = "already committed")]
    fn test_duplicate_commit_rejected() {
        let (env, client, _admin, player) = setup();
        env.ledger().set_sequence_number(100);
        client.set_window(&1, &100, &200);

        let hash = BytesN::from_array(&env, &[1u8; 32]);
        client.commit(&player, &1, &hash);
        let other = BytesN::from_array(&env, &[2u8; 32]);
        client.commit(&player, &1, &other);
    }

    #[test]
    #[should_panic(expected = "commit window closed")]
    fn test_late_commit_rejected() {
        let (env, client, _admin, player) = setup();
        env.ledger().set_sequence_number(300);
        client.set_window(&1, &100, &200);

        let hash = BytesN::from_array(&env, &[3u8; 32]);
        client.commit(&player, &1, &hash);
    }

    #[test]
    #[should_panic(expected = "commit window closed")]
    fn test_early_commit_rejected() {
        let (env, client, _admin, player) = setup();
        env.ledger().set_sequence_number(50);
        client.set_window(&1, &100, &200);

        let hash = BytesN::from_array(&env, &[4u8; 32]);
        client.commit(&player, &1, &hash);
    }

    #[test]
    #[should_panic(expected = "invalid window")]
    fn test_invalid_window_rejected() {
        let (_env, client, _admin, _player) = setup();
        client.set_window(&1, &200, &100);
    }
}
