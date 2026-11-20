#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror, contractmeta,
    Address, Env, Symbol, symbol_short,
};

mod test;

// Contract metadata
contractmeta!(
    key = "Description",
    val = "Content License Registry contract for managing immutable versioned puzzle licenses"
);

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum LicenseType {
    FreeToPlay = 1,
    PaidPerPlay = 2,
    RemixAllowed = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LicenseTerms {
    pub creator: Address,
    pub puzzle_id: u64,
    pub version: u32,
    pub license_type: LicenseType,
    pub fee_amount: u64,
    pub terms_uri: Symbol,
    pub published_at: u64,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum LicenseError {
    NotAuthorized = 1,
    PuzzleNotFound = 2,
    VersionNotFound = 3,
    AdminNotFound = 4,
    AlreadyInitialized = 5,
}

// Storage keys
const ADMIN: Symbol = symbol_short!("ADMIN");
const CREATOR: Symbol = symbol_short!("CREATOR");
const LATEST_VER: Symbol = symbol_short!("LATEST");
const TERMS: Symbol = symbol_short!("TERMS");

#[contract]
pub struct ContentLicenseContract;

#[contractimpl]
impl ContentLicenseContract {
    /// Initialize the contract with an admin address
    pub fn initialize(env: Env, admin: Address) -> Result<(), LicenseError> {
        admin.require_auth();
        if env.storage().instance().has(&ADMIN) {
            return Err(LicenseError::AlreadyInitialized);
        }
        env.storage().instance().set(&ADMIN, &admin);
        Ok(())
    }

    /// Register license terms for a puzzle. Only the puzzle creator can register terms.
    /// If registering for the first time, the caller becomes the registered creator.
    /// Each publication increments the version. Terms once published are strictly immutable.
    pub fn register_terms(
        env: Env,
        creator: Address,
        puzzle_id: u64,
        license_type: LicenseType,
        fee_amount: u64,
        terms_uri: Symbol,
    ) -> Result<u32, LicenseError> {
        creator.require_auth();

        let creator_key = (CREATOR, puzzle_id);
        let stored_creator: Option<Address> = env.storage().persistent().get(&creator_key);

        match stored_creator {
            Some(existing_creator) => {
                if existing_creator != creator {
                    return Err(LicenseError::NotAuthorized);
                }
            }
            None => {
                env.storage().persistent().set(&creator_key, &creator);
            }
        }

        let ver_key = (LATEST_VER, puzzle_id);
        let current_version: u32 = env.storage().persistent().get(&ver_key).unwrap_or(0);
        let new_version = current_version.checked_add(1).unwrap();

        let terms = LicenseTerms {
            creator: creator.clone(),
            puzzle_id,
            version: new_version,
            license_type,
            fee_amount,
            terms_uri: terms_uri.clone(),
            published_at: env.ledger().timestamp(),
        };

        let terms_key = (TERMS, puzzle_id, new_version);
        env.storage().persistent().set(&terms_key, &terms);
        env.storage().persistent().set(&ver_key, &new_version);

        // Emit registration event: (license, puzzle_id) -> (creator, new_version)
        env.events().publish(
            (symbol_short!("license"), puzzle_id),
            (creator, new_version),
        );

        Ok(new_version)
    }

    /// Retrieve the current (latest version) terms for a puzzle
    pub fn get_current_terms(env: Env, puzzle_id: u64) -> Option<LicenseTerms> {
        let ver_key = (LATEST_VER, puzzle_id);
        let latest_version: u32 = env.storage().persistent().get(&ver_key)?;
        let terms_key = (TERMS, puzzle_id, latest_version);
        env.storage().persistent().get(&terms_key)
    }

    /// Retrieve specific version terms for a puzzle
    pub fn get_terms_by_version(
        env: Env,
        puzzle_id: u64,
        version: u32,
    ) -> Option<LicenseTerms> {
        let terms_key = (TERMS, puzzle_id, version);
        env.storage().persistent().get(&terms_key)
    }

    /// Get registered creator for a puzzle
    pub fn get_creator(env: Env, puzzle_id: u64) -> Option<Address> {
        let creator_key = (CREATOR, puzzle_id);
        env.storage().persistent().get(&creator_key)
    }

    /// Get latest version number for a puzzle
    pub fn get_latest_version(env: Env, puzzle_id: u64) -> u32 {
        let ver_key = (LATEST_VER, puzzle_id);
        env.storage().persistent().get(&ver_key).unwrap_or(0)
    }

    /// Transfer contract admin role. Admin only.
    pub fn transfer_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), LicenseError> {
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
    fn require_admin(env: &Env, admin: &Address) -> Result<(), LicenseError> {
        admin.require_auth();

        let stored_admin: Option<Address> = env.storage().instance().get(&ADMIN);

        match stored_admin {
            Some(stored) if stored == *admin => Ok(()),
            Some(_) => Err(LicenseError::NotAuthorized),
            None => Err(LicenseError::AdminNotFound),
        }
    }
}
