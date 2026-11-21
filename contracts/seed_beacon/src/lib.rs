#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contractmeta, contracttype, symbol_short,
    Address, Bytes, BytesN, Env, Symbol,
};

#[cfg(test)]
mod test;

// Contract metadata
contractmeta!(
    key = "Description",
    val = "Verifiable Puzzle Seed Beacon contract for committing and revealing unpredictable puzzle round seeds"
);

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoundCommitment {
    pub round_id: u64,
    pub target_ledger_sequence: u32,
    pub entropy_salt: BytesN<32>,
    pub is_revealed: bool,
    pub created_at_ledger: u32,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum BeaconError {
    NotAuthorized = 1,
    RoundNotFound = 2,
    RoundAlreadyExists = 3,
    TargetLedgerMustBeInFuture = 4,
    CommittedLedgerNotReached = 5,
    SeedNotRevealedYet = 6,
    AdminNotFound = 7,
    AlreadyInitialized = 8,
}

// Storage keys
const ADMIN: Symbol = symbol_short!("ADMIN");
const ROUND: Symbol = symbol_short!("ROUND");
const SEED: Symbol = symbol_short!("SEED");

#[contract]
pub struct SeedBeaconContract;

#[contractimpl]
impl SeedBeaconContract {
    /// Initialize the contract with an admin address
    pub fn initialize(env: Env, admin: Address) -> Result<(), BeaconError> {
        admin.require_auth();
        if env.storage().instance().has(&ADMIN) {
            return Err(BeaconError::AlreadyInitialized);
        }
        env.storage().instance().set(&ADMIN, &admin);
        Ok(())
    }

    /// Commit to a future ledger sequence and salt for a puzzle round.
    /// Admin only. Target ledger sequence must be strictly in the future.
    pub fn commit_round(
        env: Env,
        admin: Address,
        round_id: u64,
        target_ledger_sequence: u32,
        entropy_salt: BytesN<32>,
    ) -> Result<(), BeaconError> {
        Self::require_admin(&env, &admin)?;

        let current_ledger = env.ledger().sequence();
        if target_ledger_sequence <= current_ledger {
            return Err(BeaconError::TargetLedgerMustBeInFuture);
        }

        let key = (ROUND, round_id);
        if env.storage().persistent().has(&key) {
            return Err(BeaconError::RoundAlreadyExists);
        }

        let commitment = RoundCommitment {
            round_id,
            target_ledger_sequence,
            entropy_salt: entropy_salt.clone(),
            is_revealed: false,
            created_at_ledger: current_ledger,
        };

        env.storage().persistent().set(&key, &commitment);

        // Emit commitment event: (commit, round_id) -> (target_ledger, salt)
        env.events().publish(
            (symbol_short!("commit"), round_id),
            (target_ledger_sequence, entropy_salt),
        );

        Ok(())
    }

    /// Reveal the deterministic seed once the committed ledger sequence has been reached.
    /// Derives seed = sha256(round_id || committed_ledger_hash || entropy_salt).
    /// Idempotent: once revealed, returns the existing single seed.
    pub fn reveal_seed(
        env: Env,
        round_id: u64,
        committed_ledger_hash: BytesN<32>,
    ) -> Result<BytesN<32>, BeaconError> {
        let key = (ROUND, round_id);
        let mut round: RoundCommitment = match env.storage().persistent().get(&key) {
            Some(r) => r,
            None => return Err(BeaconError::RoundNotFound),
        };

        let seed_key = (SEED, round_id);
        if round.is_revealed {
            let existing_seed: BytesN<32> = env.storage().persistent().get(&seed_key).unwrap();
            return Ok(existing_seed);
        }

        let current_ledger = env.ledger().sequence();
        if current_ledger < round.target_ledger_sequence {
            return Err(BeaconError::CommittedLedgerNotReached);
        }

        // Derive seed deterministically
        let derived_seed = Self::compute_seed_hash(
            &env,
            round_id,
            &committed_ledger_hash,
            &round.entropy_salt,
        );

        env.storage().persistent().set(&seed_key, &derived_seed);
        round.is_revealed = true;
        env.storage().persistent().set(&key, &round);

        // Emit reveal event: (reveal, round_id) -> (derived_seed)
        env.events().publish(
            (symbol_short!("reveal"), round_id),
            derived_seed.clone(),
        );

        Ok(derived_seed)
    }

    /// Public verification function: anyone can verify that a candidate seed was
    /// faithfully generated from the published round parameters and ledger hash.
    pub fn verify_seed(
        env: Env,
        round_id: u64,
        committed_ledger_hash: BytesN<32>,
        candidate_seed: BytesN<32>,
    ) -> Result<bool, BeaconError> {
        let key = (ROUND, round_id);
        let round: RoundCommitment = match env.storage().persistent().get(&key) {
            Some(r) => r,
            None => return Err(BeaconError::RoundNotFound),
        };

        let expected_seed = Self::compute_seed_hash(
            &env,
            round_id,
            &committed_ledger_hash,
            &round.entropy_salt,
        );

        Ok(candidate_seed == expected_seed)
    }

    /// Retrieve round commitment details
    pub fn get_round(env: Env, round_id: u64) -> Option<RoundCommitment> {
        let key = (ROUND, round_id);
        env.storage().persistent().get(&key)
    }

    /// Retrieve revealed seed for a round (fails if not revealed yet)
    pub fn get_seed(env: Env, round_id: u64) -> Result<BytesN<32>, BeaconError> {
        let key = (ROUND, round_id);
        let round: RoundCommitment = match env.storage().persistent().get(&key) {
            Some(r) => r,
            None => return Err(BeaconError::RoundNotFound),
        };

        if !round.is_revealed {
            return Err(BeaconError::SeedNotRevealedYet);
        }

        let seed_key = (SEED, round_id);
        match env.storage().persistent().get(&seed_key) {
            Some(s) => Ok(s),
            None => Err(BeaconError::SeedNotRevealedYet),
        }
    }

    /// Check if a round seed has been revealed
    pub fn is_revealed(env: Env, round_id: u64) -> bool {
        let key = (ROUND, round_id);
        let round: Option<RoundCommitment> = env.storage().persistent().get(&key);
        match round {
            Some(r) => r.is_revealed,
            None => false,
        }
    }

    /// Transfer contract admin role. Admin only.
    pub fn transfer_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), BeaconError> {
        Self::require_admin(&env, &current_admin)?;
        new_admin.require_auth();

        env.storage().instance().set(&ADMIN, &new_admin);
        Ok(())
    }

    /// Get current admin
    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&ADMIN)
    }

    // Helper functions
    fn require_admin(env: &Env, admin: &Address) -> Result<(), BeaconError> {
        admin.require_auth();

        let stored_admin: Option<Address> = env.storage().instance().get(&ADMIN);

        match stored_admin {
            Some(stored) if stored == *admin => Ok(()),
            Some(_) => Err(BeaconError::NotAuthorized),
            None => Err(BeaconError::AdminNotFound),
        }
    }

    fn compute_seed_hash(
        env: &Env,
        round_id: u64,
        ledger_hash: &BytesN<32>,
        entropy_salt: &BytesN<32>,
    ) -> BytesN<32> {
        let mut preimage = Bytes::new(env);
        preimage.append(&Bytes::from_array(env, &round_id.to_be_bytes()));
        preimage.append(&Bytes::from_array(env, &ledger_hash.to_array()));
        preimage.append(&Bytes::from_array(env, &entropy_salt.to_array()));

        env.crypto().sha256(&preimage).into()
    }
}
