#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol,
};

// ─── Errors ───────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    AlreadyBlocked = 4,
    NotBlocked = 5,
    AnchorAlreadyExists = 6,
    AnchorNotFound = 7,
}

// ─── Storage Keys / Types ──────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    Blocked(Address),
    Anchor(Address, u64), // (player, puzzle_id)
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnchorRecord {
    pub player: Address,
    pub puzzle_id: u64,
    pub completion_hash: BytesN<32>,
    pub ledger: u32,
}

// ─── Contract ─────────────────────────────────────────────

#[contract]
pub struct BlocklistAnchorContract;

#[contractimpl]
impl BlocklistAnchorContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    // ─── Blocklist (#395) ─────────────────────────────────

    /// Admin-only: block an address from interacting with integrating contracts.
    pub fn block(env: Env, admin: Address, user: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Blocked(user.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyBlocked);
        }
        env.storage().persistent().set(&key, &true);
        env.events().publish((symbol_short!("blocked"), user), admin);
        Ok(())
    }

    /// Admin-only: unblock an address and restore access.
    pub fn unblock(env: Env, admin: Address, user: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Blocked(user.clone());
        if !env.storage().persistent().has(&key) {
            return Err(ContractError::NotBlocked);
        }
        env.storage().persistent().remove(&key);
        env.events().publish((symbol_short!("unblock"), user), admin);
        Ok(())
    }

    /// Read function for integrating contracts: returns true if address is blocked.
    pub fn is_blocked(env: Env, user: Address) -> bool {
        env.storage().persistent().has(&DataKey::Blocked(user))
    }

    // ─── Completion Hash Anchor (#371) ────────────────────

    /// Stores a completion proof hash for (player, puzzle) with ledger reference.
    /// Callable by the verifier/admin in this lean implementation.
    pub fn anchor_completion(
        env: Env,
        verifier: Address,
        player: Address,
        puzzle_id: u64,
        completion_hash: BytesN<32>,
    ) -> Result<AnchorRecord, ContractError> {
        Self::require_admin(&env, &verifier)?;
        let key = DataKey::Anchor(player.clone(), puzzle_id);
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AnchorAlreadyExists);
        }
        let record = AnchorRecord {
            player: player.clone(),
            puzzle_id,
            completion_hash,
            ledger: env.ledger().sequence(),
        };
        env.storage().persistent().set(&key, &record);
        env.events()
            .publish((symbol_short!("anchored"), player, puzzle_id), verifier);
        Ok(record)
    }

    /// Lookup anchored completion hash/record by player and puzzle.
    pub fn get_anchor(env: Env, player: Address, puzzle_id: u64) -> Result<AnchorRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Anchor(player, puzzle_id))
            .ok_or(ContractError::AnchorNotFound)
    }

    // ─── Internal ─────────────────────────────────────────

    fn require_initialized(env: &Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin = Self::require_initialized(env)?;
        caller.require_auth();
        if *caller != admin {
            return Err(ContractError::Unauthorized);
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, BytesN, Env};

    #[test]
    fn blocklist_and_anchor_flow() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(BlocklistAnchorContract, ());
        let client = BlocklistAnchorContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        client.initialize(&admin);

        assert!(!client.is_blocked(&user));
        client.block(&admin, &user);
        assert!(client.is_blocked(&user));
        client.unblock(&admin, &user);
        assert!(!client.is_blocked(&user));

        let hash = BytesN::from_array(&env, &[1u8; 32]);
        let rec = client.anchor_completion(&admin, &user, &7u64, &hash);
        assert_eq!(rec.player, user);
        assert_eq!(rec.puzzle_id, 7u64);
        let fetched = client.get_anchor(&user, &7u64);
        assert_eq!(fetched.completion_hash, hash);
    }
}
