#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, String};

/// Default registration period in ledgers (~1 day at 5s per ledger ~ 17280).
pub const REGISTRATION_PERIOD: u32 = 17_280 * 365;

const MIN_USERNAME_LEN: u32 = 3;
const MAX_USERNAME_LEN: u32 = 32;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// username -> owner address
    Owner(String),
    /// username -> expiry ledger sequence
    Expiry(String),
    /// owner address -> username they currently hold
    Name(Address),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    InvalidLength = 1,
    InvalidCharacter = 2,
    AlreadyTaken = 3,
    NotOwner = 4,
    NotFound = 5,
    AlreadyRegistered = 6,
}

#[contract]
pub struct UsernameRegistry;

fn is_valid_byte(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'
}

fn validate_username(env: &Env, username: &String) -> Result<(), Error> {
    let len = username.len();
    if len < MIN_USERNAME_LEN || len > MAX_USERNAME_LEN {
        return Err(Error::InvalidLength);
    }
    let mut buf = [0u8; MAX_USERNAME_LEN as usize];
    let slice = &mut buf[..len as usize];
    username.copy_into_slice(slice);
    let _ = env;
    for &b in slice.iter() {
        if !is_valid_byte(b) {
            return Err(Error::InvalidCharacter);
        }
    }
    Ok(())
}

/// Returns true if the username is currently held by a non-expired registration.
fn is_active(env: &Env, username: &String) -> bool {
    if !env.storage().persistent().has(&DataKey::Owner(username.clone())) {
        return false;
    }
    let expiry: u32 = env
        .storage()
        .persistent()
        .get(&DataKey::Expiry(username.clone()))
        .unwrap_or(0);
    env.ledger().sequence() < expiry
}

/// Removes all records for a username and its owner's back-reference.
fn release(env: &Env, username: &String) {
    if let Some(owner) = env
        .storage()
        .persistent()
        .get::<_, Address>(&DataKey::Owner(username.clone()))
    {
        env.storage().persistent().remove(&DataKey::Name(owner));
    }
    env.storage().persistent().remove(&DataKey::Owner(username.clone()));
    env.storage().persistent().remove(&DataKey::Expiry(username.clone()));
}

#[contractimpl]
impl UsernameRegistry {
    /// Claim a unique username for `owner`. Fails if the name is actively held
    /// by someone else. Expired names may be claimed by anyone.
    pub fn claim(env: Env, owner: Address, username: String) -> Result<(), Error> {
        owner.require_auth();
        validate_username(&env, &username)?;

        if is_active(&env, &username) {
            return Err(Error::AlreadyTaken);
        }

        // Clean up any stale record (expired name) before reassigning.
        if env.storage().persistent().has(&DataKey::Owner(username.clone())) {
            release(&env, &username);
        }

        // An owner holds at most one active name; release their previous one.
        if let Some(prev) = env
            .storage()
            .persistent()
            .get::<_, String>(&DataKey::Name(owner.clone()))
        {
            if is_active(&env, &prev) {
                return Err(Error::AlreadyRegistered);
            }
            release(&env, &prev);
        }

        let expiry = env.ledger().sequence() + REGISTRATION_PERIOD;
        env.storage()
            .persistent()
            .set(&DataKey::Owner(username.clone()), &owner);
        env.storage()
            .persistent()
            .set(&DataKey::Expiry(username.clone()), &expiry);
        env.storage()
            .persistent()
            .set(&DataKey::Name(owner), &username);
        Ok(())
    }

    /// Extend the registration of `username` by one period. Only the owner may renew,
    /// and only while the registration has not yet expired.
    pub fn renew(env: Env, owner: Address, username: String) -> Result<u32, Error> {
        owner.require_auth();
        if !is_active(&env, &username) {
            return Err(Error::NotFound);
        }
        let current: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Owner(username.clone()))
            .ok_or(Error::NotFound)?;
        if current != owner {
            return Err(Error::NotOwner);
        }
        let expiry = env.ledger().sequence() + REGISTRATION_PERIOD;
        env.storage()
            .persistent()
            .set(&DataKey::Expiry(username.clone()), &expiry);
        Ok(expiry)
    }

    /// Returns the owner of an active username, if any.
    pub fn resolve(env: Env, username: String) -> Option<Address> {
        if is_active(&env, &username) {
            env.storage().persistent().get(&DataKey::Owner(username))
        } else {
            None
        }
    }

    /// Returns the expiry ledger for a username, if registered.
    pub fn expiry_of(env: Env, username: String) -> Option<u32> {
        env.storage().persistent().get(&DataKey::Expiry(username))
    }
}
