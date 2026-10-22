#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, BytesN, Env, Symbol,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnchorRecord {
    pub player: Address,
    pub puzzle_id: u32,
    pub completion_hash: BytesN<32>,
    pub ledger_sequence: u32,
    pub timestamp: u64,
}

#[contracttype]
pub enum DataKey {
    Admin,
    Verifier,
    Anchor(Address, u32),
}

#[contract]
pub struct CompletionAnchor;

#[contractimpl]
impl CompletionAnchor {
    pub fn initialize(env: Env, admin: Address, verifier: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Verifier, &verifier);
    }

    pub fn set_verifier(env: Env, new_verifier: Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not set");
        admin.require_auth();
        env.storage().instance().set(&DataKey::Verifier, &new_verifier);
    }

    pub fn get_verifier(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Verifier)
            .expect("verifier not set")
    }

    pub fn anchor_completion(
        env: Env,
        verifier: Address,
        player: Address,
        puzzle_id: u32,
        completion_hash: BytesN<32>,
    ) -> AnchorRecord {
        verifier.require_auth();

        let registered_verifier: Address = env
            .storage()
            .instance()
            .get(&DataKey::Verifier)
            .expect("verifier not set");

        if verifier != registered_verifier {
            panic!("unauthorized verifier");
        }

        let key = DataKey::Anchor(player.clone(), puzzle_id);
        if env.storage().instance().has(&key) {
            panic!("duplicate anchor rejected");
        }

        let record = AnchorRecord {
            player: player.clone(),
            puzzle_id,
            completion_hash,
            ledger_sequence: env.ledger().sequence(),
            timestamp: env.ledger().timestamp(),
        };

        env.storage().instance().set(&key, &record);

        env.events().publish(
            (Symbol::new(&env, "anchor"), Symbol::new(&env, "completed")),
            (player, puzzle_id, record.ledger_sequence),
        );

        record
    }

    pub fn has_anchor(env: Env, player: Address, puzzle_id: u32) -> bool {
        let key = DataKey::Anchor(player, puzzle_id);
        env.storage().instance().has(&key)
    }

    pub fn get_anchor(env: Env, player: Address, puzzle_id: u32) -> Option<AnchorRecord> {
        let key = DataKey::Anchor(player, puzzle_id);
        env.storage().instance().get(&key)
    }

    pub fn get_completion_hash(env: Env, player: Address, puzzle_id: u32) -> Option<BytesN<32>> {
        Self::get_anchor(env, player, puzzle_id).map(|r| r.completion_hash)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger as _};

    #[test]
    fn test_initialize_and_verifier_management() {
        let env = Env::default();
        let contract_id = env.register_contract(None, CompletionAnchor);
        let client = CompletionAnchorClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let verifier = Address::generate(&env);
        let new_verifier = Address::generate(&env);

        env.mock_all_auths();
        client.initialize(&admin, &verifier);
        assert_eq!(client.get_verifier(), verifier);

        client.set_verifier(&new_verifier);
        assert_eq!(client.get_verifier(), new_verifier);
    }

    #[test]
    fn test_anchor_completion_success() {
        let env = Env::default();
        let contract_id = env.register_contract(None, CompletionAnchor);
        let client = CompletionAnchorClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let verifier = Address::generate(&env);
        let player = Address::generate(&env);

        env.mock_all_auths();
        client.initialize(&admin, &verifier);

        env.ledger().set_sequence_number(12345);
        env.ledger().set_timestamp(1_700_000_000);

        let hash = BytesN::from_array(&env, &[0xab; 32]);
        let record = client.anchor_completion(&verifier, &player, &101, &hash);

        assert_eq!(record.player, player);
        assert_eq!(record.puzzle_id, 101);
        assert_eq!(record.completion_hash, hash);
        assert_eq!(record.ledger_sequence, 12345);
        assert_eq!(record.timestamp, 1_700_000_000);

        assert_eq!(client.has_anchor(&player, &101), true);
        assert_eq!(client.get_completion_hash(&player, &101), Some(hash));

        let retrieved = client.get_anchor(&player, &101).expect("anchor record");
        assert_eq!(retrieved, record);
    }

    #[test]
    #[should_panic(expected = "duplicate anchor rejected")]
    fn test_duplicate_anchor_rejected() {
        let env = Env::default();
        let contract_id = env.register_contract(None, CompletionAnchor);
        let client = CompletionAnchorClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let verifier = Address::generate(&env);
        let player = Address::generate(&env);

        env.mock_all_auths();
        client.initialize(&admin, &verifier);

        let hash1 = BytesN::from_array(&env, &[1u8; 32]);
        let hash2 = BytesN::from_array(&env, &[2u8; 32]);

        client.anchor_completion(&verifier, &player, &1, &hash1);
        client.anchor_completion(&verifier, &player, &1, &hash2);
    }

    #[test]
    #[should_panic(expected = "unauthorized verifier")]
    fn test_unauthorized_verifier_rejected() {
        let env = Env::default();
        let contract_id = env.register_contract(None, CompletionAnchor);
        let client = CompletionAnchorClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let verifier = Address::generate(&env);
        let imposter = Address::generate(&env);
        let player = Address::generate(&env);

        env.mock_all_auths();
        client.initialize(&admin, &verifier);

        let hash = BytesN::from_array(&env, &[3u8; 32]);
        client.anchor_completion(&imposter, &player, &1, &hash);
    }

    #[test]
    fn test_nonexistent_anchor_returns_none() {
        let env = Env::default();
        let contract_id = env.register_contract(None, CompletionAnchor);
        let client = CompletionAnchorClient::new(&env, &contract_id);

        let player = Address::generate(&env);
        assert_eq!(client.has_anchor(&player, &999), false);
        assert_eq!(client.get_anchor(&player, &999), None);
        assert_eq!(client.get_completion_hash(&player, &999), None);
    }
}
