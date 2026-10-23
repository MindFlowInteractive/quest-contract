#![no_std]

use soroban_sdk::{contract, contractimpl, contracterror, contracttype, symbol_short, Address, Bytes, BytesN, Env};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyCommitted = 1,
    NotCommitted = 2,
    AlreadyRevealed = 3,
    RevealWindowNotOpen = 4,
    RevealWindowClosed = 5,
    HashMismatch = 6,
    NotExpired = 7,
    AlreadyForfeited = 8,
    InvalidWindow = 9,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Commitment(Address),
    Score(Address),
    Window,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Window {
    pub reveal_start: u64,
    pub reveal_end: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Commitment {
    pub hash: BytesN<32>,
    pub revealed: bool,
    pub forfeited: bool,
}

#[contract]
pub struct CommitRevealContract;

/// Computes the commitment hash: sha256(solution || salt).
pub fn compute_hash(env: &Env, solution: &Bytes, salt: &BytesN<32>) -> BytesN<32> {
    let mut preimage = Bytes::new(env);
    preimage.append(solution);
    preimage.append(&Bytes::from_slice(env, &salt.to_array()));
    env.crypto().sha256(&preimage).into()
}

#[contractimpl]
impl CommitRevealContract {
    /// Sets the reveal window (admin-free for simplicity; intended to be called once at setup).
    pub fn set_window(env: Env, reveal_start: u64, reveal_end: u64) -> Result<(), Error> {
        if reveal_end <= reveal_start {
            return Err(Error::InvalidWindow);
        }
        env.storage().instance().set(
            &DataKey::Window,
            &Window {
                reveal_start,
                reveal_end,
            },
        );
        Ok(())
    }

    /// Stores a commitment hash for `player`.
    pub fn commit(env: Env, player: Address, hash: BytesN<32>) -> Result<(), Error> {
        player.require_auth();
        let key = DataKey::Commitment(player.clone());
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyCommitted);
        }
        env.storage().persistent().set(
            &key,
            &Commitment {
                hash,
                revealed: false,
                forfeited: false,
            },
        );
        env.events()
            .publish((symbol_short!("commit"), player), ());
        Ok(())
    }

    /// Reveals `solution` and `salt`; verifies against the stored commitment and records a score.
    pub fn reveal(
        env: Env,
        player: Address,
        solution: Bytes,
        salt: BytesN<32>,
    ) -> Result<u32, Error> {
        player.require_auth();
        let window = Self::get_window(&env)?;
        let now = env.ledger().timestamp();
        if now < window.reveal_start {
            return Err(Error::RevealWindowNotOpen);
        }
        if now > window.reveal_end {
            return Err(Error::RevealWindowClosed);
        }

        let key = DataKey::Commitment(player.clone());
        let mut commitment: Commitment = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::NotCommitted)?;
        if commitment.revealed {
            return Err(Error::AlreadyRevealed);
        }
        if commitment.forfeited {
            return Err(Error::AlreadyForfeited);
        }

        let computed = compute_hash(&env, &solution, &salt);
        if computed != commitment.hash {
            return Err(Error::HashMismatch);
        }

        let score = Self::score_solution(&solution);
        commitment.revealed = true;
        env.storage().persistent().set(&key, &commitment);
        env.storage()
            .persistent()
            .set(&DataKey::Score(player.clone()), &score);
        env.events()
            .publish((symbol_short!("reveal"), player), score);
        Ok(score)
    }

    /// Marks a player's unrevealed commitment as forfeited once the reveal window has closed.
    pub fn forfeit(env: Env, player: Address) -> Result<(), Error> {
        let window = Self::get_window(&env)?;
        if env.ledger().timestamp() <= window.reveal_end {
            return Err(Error::NotExpired);
        }
        let key = DataKey::Commitment(player.clone());
        let mut commitment: Commitment = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::NotCommitted)?;
        if commitment.revealed {
            return Err(Error::AlreadyRevealed);
        }
        if commitment.forfeited {
            return Err(Error::AlreadyForfeited);
        }
        commitment.forfeited = true;
        env.storage().persistent().set(&key, &commitment);
        env.events()
            .publish((symbol_short!("forfeit"), player), ());
        Ok(())
    }

    /// Returns the recorded score for `player`, or 0 if none.
    pub fn get_score(env: Env, player: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::Score(player))
            .unwrap_or(0)
    }

    /// Returns the stored commitment for `player`, if any.
    pub fn get_commitment(env: Env, player: Address) -> Option<Commitment> {
        env.storage().persistent().get(&DataKey::Commitment(player))
    }

    fn get_window(env: &Env) -> Result<Window, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Window)
            .ok_or(Error::InvalidWindow)
    }

    /// Scoring rule: number of bytes in the solution (deterministic, simple).
    fn score_solution(solution: &Bytes) -> u32 {
        solution.len()
    }
}

mod test;
