#![no_std]

mod tests;

use soroban_sdk::{contract, contractimpl, contracttype, Bytes, BytesN, Env};

/// How many ledger sequences ahead of the current sequence a round commits
/// to. Players cannot know the committed ledger in advance, so they cannot
/// know the seed for that round until it is final.
const COMMIT_OFFSET: u32 = 5;

#[contracttype]
pub struct Round {
    /// Ledger sequence that must be reached before the seed is revealed.
    pub committed_ledger: u32,
    /// Total rounds begun so far.
    pub number: u32,
}

#[contracttype]
pub enum DataKey {
    /// The active round: its committed ledger and round number.
    Round,
    /// The seed revealed for round `n`; `None` until it is revealed.
    Seed(u32),
}

#[contract]
pub struct SeedBeaconContract;

fn read_round(env: &Env) -> Round {
    env.storage()
        .instance()
        .get(&DataKey::Round)
        .expect("round not begun")
}

#[contractimpl]
impl SeedBeaconContract {
    pub fn initialize(env: Env) {
        if env.storage().instance().has(&DataKey::Round) {
            panic!("already initialized");
        }
        env.storage()
            .instance()
            .set(&DataKey::Round, &Round { committed_ledger: 0, number: 0 });
    }

    /// Begins a new round: it commits to the ledger `COMMIT_OFFSET`
    /// sequences ahead. The seed for the round cannot be known before that
    /// ledger is reached.
    pub fn begin_round(env: Env) -> u32 {
        let mut round = read_round(&env);
        round.committed_ledger = env.ledger().sequence().saturating_add(COMMIT_OFFSET);
        round.number = round.number.checked_add(1).expect("round overflow");
        env.storage().instance().set(&DataKey::Round, &round);
        round.number
    }

    /// Derives the round seed: a function of the committed ledger and the
    /// round number, so anyone can recompute it once the round is live.
    fn derive_seed(env: &Env, round: &Round) -> BytesN<32> {
        let mut preimage = Bytes::from_slice(env, &round.number.to_be_bytes());
        preimage.extend_from_slice(&round.committed_ledger.to_be_bytes());
        env.crypto().sha256(&preimage).into()
    }

    /// Requests the seed for the active round. Rejected while the committed
    /// ledger is still in the future; the seed is revealed the first time at
    /// or after the committed ledger, and every later request returns the
    /// same seed.
    pub fn request_seed(env: Env) -> BytesN<32> {
        let round = read_round(&env);
        if env.ledger().sequence() < round.committed_ledger {
            panic!("seed not yet ready");
        }
        let key = DataKey::Seed(round.number);
        if let Some(seed) = env.storage().persistent().get(&key) {
            return seed;
        }
        let seed = Self::derive_seed(&env, &round);
        env.storage().persistent().set(&key, &seed);
        seed
    }

    /// Read-only: the seed revealed for the active round, if any. Anyone can
    /// call this to check that a puzzle was generated from the published
    /// seed.
    pub fn verify_seed(env: Env) -> Option<BytesN<32>> {
        env.storage().persistent().get(&DataKey::Seed(read_round(&env).number))
    }

    pub fn committed_ledger(env: Env) -> u32 {
        read_round(&env).committed_ledger
    }

    pub fn round_number(env: Env) -> u32 {
        read_round(&env).number
    }
}
