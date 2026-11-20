#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, String,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    AlreadyRegistered = 3,
    NotRegistered = 4,
    InvalidUri = 5,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatorProfile {
    pub creator: Address,
    pub profile_uri: String,
    pub is_verified: bool,
    pub registered_at: u64,
    pub verified_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Creator(Address),
    TotalCreators,
}

#[contract]
pub struct CreatorRegistryContract;

#[contractimpl]
impl CreatorRegistryContract {
    /// Initialize the contract with an admin
    pub fn init(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TotalCreators, &0u32);
        Ok(())
    }

    /// Read the current admin
    pub fn get_admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    /// Set a new admin (admin only)
    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.events().publish(
            (symbol_short!("admin"), symbol_short!("transfer")),
            new_admin,
        );
        Ok(())
    }

    /// Register a new creator profile
    pub fn register_creator(
        env: Env,
        creator: Address,
        profile_uri: String,
    ) -> Result<(), Error> {
        creator.require_auth();

        if profile_uri.len() == 0 {
            return Err(Error::InvalidUri);
        }

        let key = DataKey::Creator(creator.clone());
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyRegistered);
        }

        let timestamp = env.ledger().timestamp();
        let profile = CreatorProfile {
            creator: creator.clone(),
            profile_uri: profile_uri.clone(),
            is_verified: false,
            registered_at: timestamp,
            verified_at: 0,
        };

        env.storage().persistent().set(&key, &profile);

        let total: u32 = env
            .storage()
            .instance()
            .get(&DataKey::TotalCreators)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalCreators, &(total + 1));

        env.events().publish(
            (symbol_short!("creator"), symbol_short!("register")),
            (creator, profile_uri),
        );

        Ok(())
    }

    /// Update creator profile URI
    pub fn update_profile(
        env: Env,
        creator: Address,
        new_profile_uri: String,
    ) -> Result<(), Error> {
        creator.require_auth();

        if new_profile_uri.len() == 0 {
            return Err(Error::InvalidUri);
        }

        let key = DataKey::Creator(creator.clone());
        let mut profile: CreatorProfile = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::NotRegistered)?;

        profile.profile_uri = new_profile_uri.clone();
        env.storage().persistent().set(&key, &profile);

        env.events().publish(
            (symbol_short!("creator"), symbol_short!("update")),
            (creator, new_profile_uri),
        );

        Ok(())
    }

    /// Grant or revoke verified status for a creator (admin only)
    pub fn set_verified(
        env: Env,
        creator: Address,
        verified: bool,
    ) -> Result<(), Error> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        let key = DataKey::Creator(creator.clone());
        let mut profile: CreatorProfile = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::NotRegistered)?;

        profile.is_verified = verified;
        profile.verified_at = if verified {
            env.ledger().timestamp()
        } else {
            0
        };

        env.storage().persistent().set(&key, &profile);

        env.events().publish(
            (symbol_short!("creator"), symbol_short!("verified")),
            (creator, verified),
        );

        Ok(())
    }

    /// Check if a creator is registered
    pub fn is_registered(env: Env, creator: Address) -> bool {
        let key = DataKey::Creator(creator);
        env.storage().persistent().has(&key)
    }

    /// Check if a creator is verified
    pub fn is_verified(env: Env, creator: Address) -> bool {
        let key = DataKey::Creator(creator);
        match env.storage().persistent().get::<DataKey, CreatorProfile>(&key) {
            Some(profile) => profile.is_verified,
            None => false,
        }
    }

    /// Retrieve full creator profile
    pub fn get_creator(env: Env, creator: Address) -> Option<CreatorProfile> {
        let key = DataKey::Creator(creator);
        env.storage().persistent().get(&key)
    }

    /// Get total number of registered creators
    pub fn get_total_creators(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::TotalCreators)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod test;
