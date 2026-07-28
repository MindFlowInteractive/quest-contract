#![no_std]

#[cfg(test)]
mod test;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Bytes,
    BytesN, Env, Symbol,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SwapStatus {
    Initiated = 1,
    Withdrawn = 2,
    Refunded = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Swap {
    pub id: BytesN<32>,
    pub depositor: Address,
    pub claimer: Address,
    pub token: Address,
    pub amount: i128,
    pub hashlock: BytesN<32>,
    pub secret: Option<BytesN<32>>,
    pub timelock: u64,
    pub status: SwapStatus,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyExists = 1,
    NotFound = 2,
    InvalidStatus = 3,
    HashMismatch = 4,
    TimelockNotExpired = 5,
    TimelockExpired = 6,
    Unauthorized = 7,
    InvalidAmount = 8,
    InvalidTimelock = 9,
}

#[contracttype]
pub enum DataKey {
    Swap(BytesN<32>),
}

const SWAP_CREATED: Symbol = symbol_short!("created");
const SWAP_WITHDRAWN: Symbol = symbol_short!("withdrawn");
const SWAP_REFUNDED: Symbol = symbol_short!("refunded");

#[contract]
pub struct AtomicSwapContract;

// Helper to calculate hash
fn verify_hashlock(env: &Env, secret: &BytesN<32>, hashlock: &BytesN<32>) -> bool {
    let secret_bytes = Bytes::from_slice(env, secret.to_array().as_slice());
    let hash = env.crypto().sha256(&secret_bytes);
    &hash == hashlock
}

#[contractimpl]
impl AtomicSwapContract {
    /// Phase 3 - Initiator: Swap Creation
    pub fn create_swap(
        env: Env,
        id: BytesN<32>,
        depositor: Address,
        claimer: Address,
        token: Address,
        amount: i128,
        hashlock: BytesN<32>,
        timelock: u64,
    ) -> Result<(), Error> {
        depositor.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        if timelock <= env.ledger().timestamp() {
            return Err(Error::InvalidTimelock);
        }

        if env.storage().persistent().has(&DataKey::Swap(id.clone())) {
            return Err(Error::AlreadyExists);
        }

        let swap = Swap {
            id: id.clone(),
            depositor: depositor.clone(),
            claimer: claimer.clone(),
            token: token.clone(),
            amount,
            hashlock: hashlock.clone(),
            secret: None,
            timelock,
            status: SwapStatus::Initiated,
        };

        // Transfer tokens into contract custody
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&depositor, &env.current_contract_address(), &amount);

        // Save swap
        env.storage().persistent().set(&DataKey::Swap(id.clone()), &swap);

        // Emit event
        env.events().publish((SWAP_CREATED, id), swap);

        Ok(())
    }

    /// Phase 4 - Participant Acceptance (Linked HTLC Creation)
    pub fn accept_swap(
        env: Env,
        linked_id: BytesN<32>,
        new_id: BytesN<32>,
        depositor: Address,
        token: Address,
        amount: i128,
        timelock: u64,
    ) -> Result<(), Error> {
        depositor.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        if timelock <= env.ledger().timestamp() {
            return Err(Error::InvalidTimelock);
        }

        if env.storage().persistent().has(&DataKey::Swap(new_id.clone())) {
            return Err(Error::AlreadyExists);
        }

        let linked_swap: Swap = env
            .storage()
            .persistent()
            .get(&DataKey::Swap(linked_id.clone()))
            .ok_or(Error::NotFound)?;

        if linked_swap.status != SwapStatus::Initiated {
            return Err(Error::InvalidStatus);
        }

        if timelock >= linked_swap.timelock {
            return Err(Error::InvalidTimelock);
        }

        if depositor != linked_swap.claimer {
            return Err(Error::Unauthorized);
        }

        let swap = Swap {
            id: new_id.clone(),
            depositor: depositor.clone(),
            claimer: linked_swap.depositor.clone(),
            token: token.clone(),
            amount,
            hashlock: linked_swap.hashlock.clone(),
            secret: None,
            timelock,
            status: SwapStatus::Initiated,
        };

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&depositor, &env.current_contract_address(), &amount);

        env.storage().persistent().set(&DataKey::Swap(new_id.clone()), &swap);
        env.events().publish((SWAP_CREATED, new_id), swap);

        Ok(())
    }

    /// Phase 6 - Secret Reveal / Withdraw
    pub fn withdraw(env: Env, id: BytesN<32>, secret: BytesN<32>) -> Result<(), Error> {
        let mut swap: Swap = env
            .storage()
            .persistent()
            .get(&DataKey::Swap(id.clone()))
            .ok_or(Error::NotFound)?;

        if swap.status != SwapStatus::Initiated {
            return Err(Error::InvalidStatus);
        }

        if env.ledger().timestamp() >= swap.timelock {
            return Err(Error::TimelockExpired);
        }

        if !verify_hashlock(&env, &secret, &swap.hashlock) {
            return Err(Error::HashMismatch);
        }

        swap.status = SwapStatus::Withdrawn;
        swap.secret = Some(secret.clone());
        env.storage().persistent().set(&DataKey::Swap(id.clone()), &swap);

        let token_client = token::Client::new(&env, &swap.token);
        token_client.transfer(
            &env.current_contract_address(),
            &swap.claimer,
            &swap.amount,
        );

        env.events().publish((SWAP_WITHDRAWN, id), secret);
        Ok(())
    }

    /// Phase 7 - Timeout Refund Logic
    pub fn refund(env: Env, id: BytesN<32>) -> Result<(), Error> {
        let mut swap: Swap = env
            .storage()
            .persistent()
            .get(&DataKey::Swap(id.clone()))
            .ok_or(Error::NotFound)?;

        if swap.status != SwapStatus::Initiated {
            return Err(Error::InvalidStatus);
        }

        if env.ledger().timestamp() < swap.timelock {
            return Err(Error::TimelockNotExpired);
        }

        swap.status = SwapStatus::Refunded;
        env.storage().persistent().set(&DataKey::Swap(id.clone()), &swap);

        let token_client = token::Client::new(&env, &swap.token);
        token_client.transfer(
            &env.current_contract_address(),
            &swap.depositor,
            &swap.amount,
        );

        env.events().publish((SWAP_REFUNDED, id), ());
        Ok(())
    }

    /// Phase 8 - Status Queries & Swap History
    pub fn get_swap(env: Env, id: BytesN<32>) -> Result<Swap, Error> {
        env.storage().persistent().get(&DataKey::Swap(id)).ok_or(Error::NotFound)
    }

    pub fn get_status(env: Env, id: BytesN<32>) -> Result<SwapStatus, Error> {
        let swap = Self::get_swap(env, id)?;
        Ok(swap.status)
    }
}
