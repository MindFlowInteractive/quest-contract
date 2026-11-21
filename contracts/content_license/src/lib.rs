#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contractmeta, contracttype, symbol_short, token,
    Address, Env, String, Symbol,
};

#[cfg(test)]
mod test;

// Contract metadata
contractmeta!(
    key = "Description",
    val = "Content License Registry contract supporting versioned terms, purchases with creator fees, and policy-based revocation"
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
    pub fee_token: Address,
    pub fee_amount: i128,
    pub validity_duration: u64,
    pub allows_revocation: bool,
    pub refund_on_revocation: bool,
    pub terms_uri: String,
    pub published_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LicenseGrant {
    pub holder: Address,
    pub puzzle_id: u64,
    pub version_bought: u32,
    pub purchased_at: u64,
    pub expires_at: u64,
    pub price_paid: i128,
    pub is_revoked: bool,
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
    LicenseNotFound = 6,
    LicenseExpired = 7,
    RevocationNotAllowed = 8,
    AlreadyRevoked = 9,
    InvalidFee = 10,
    TermsNotFound = 11,
}

// Storage keys
const ADMIN: Symbol = symbol_short!("ADMIN");
const CREATOR: Symbol = symbol_short!("CREATOR");
const LATEST_VER: Symbol = symbol_short!("LATEST");
const TERMS: Symbol = symbol_short!("TERMS");
const GRANT: Symbol = symbol_short!("GRANT");

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
        fee_token: Address,
        fee_amount: i128,
        validity_duration: u64,
        allows_revocation: bool,
        refund_on_revocation: bool,
        terms_uri: String,
    ) -> Result<u32, LicenseError> {
        creator.require_auth();

        if fee_amount < 0 {
            return Err(LicenseError::InvalidFee);
        }

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
            fee_token,
            fee_amount,
            validity_duration,
            allows_revocation,
            refund_on_revocation,
            terms_uri,
            published_at: env.ledger().timestamp(),
        };

        let terms_key = (TERMS, puzzle_id, new_version);
        env.storage().persistent().set(&terms_key, &terms);
        env.storage().persistent().set(&ver_key, &new_version);

        // Emit registration event: (terms, puzzle_id) -> (creator, new_version)
        env.events().publish(
            (symbol_short!("terms"), puzzle_id),
            (creator, new_version),
        );

        Ok(new_version)
    }

    /// Purchase a license for a puzzle under the current terms.
    /// Pays creator the stated fee amount via token client.
    /// License is immutably bound to the specific terms version bought.
    pub fn purchase_license(
        env: Env,
        player: Address,
        puzzle_id: u64,
    ) -> Result<u32, LicenseError> {
        player.require_auth();

        let ver_key = (LATEST_VER, puzzle_id);
        let latest_version: u32 = match env.storage().persistent().get(&ver_key) {
            Some(v) => v,
            None => return Err(LicenseError::TermsNotFound),
        };

        let terms_key = (TERMS, puzzle_id, latest_version);
        let terms: LicenseTerms = match env.storage().persistent().get(&terms_key) {
            Some(t) => t,
            None => return Err(LicenseError::TermsNotFound),
        };

        // Pay creator if fee is required
        if terms.fee_amount > 0 {
            token::Client::new(&env, &terms.fee_token).transfer(
                &player,
                &terms.creator,
                &terms.fee_amount,
            );
        }

        let current_time = env.ledger().timestamp();
        let expires_at = if terms.validity_duration > 0 {
            current_time.saturating_add(terms.validity_duration)
        } else {
            0
        };

        let grant = LicenseGrant {
            holder: player.clone(),
            puzzle_id,
            version_bought: latest_version,
            purchased_at: current_time,
            expires_at,
            price_paid: terms.fee_amount,
            is_revoked: false,
        };

        let grant_key = (GRANT, puzzle_id, player.clone());
        env.storage().persistent().set(&grant_key, &grant);

        // Emit purchase event: (buy_lic, puzzle_id) -> (player, version_bought)
        env.events().publish(
            (symbol_short!("buy_lic"), puzzle_id),
            (player, latest_version),
        );

        Ok(latest_version)
    }

    /// Revoke an existing license grant. Creator only.
    /// Enforces that revocation is allowed under the specific terms version bought.
    /// Issues refund to player if the terms required refund on revocation.
    pub fn revoke_license(
        env: Env,
        caller: Address,
        puzzle_id: u64,
        player: Address,
    ) -> Result<(), LicenseError> {
        caller.require_auth();

        let creator_key = (CREATOR, puzzle_id);
        let creator: Address = match env.storage().persistent().get(&creator_key) {
            Some(c) => c,
            None => return Err(LicenseError::PuzzleNotFound),
        };

        if caller != creator {
            return Err(LicenseError::NotAuthorized);
        }

        let grant_key = (GRANT, puzzle_id, player.clone());
        let mut grant: LicenseGrant = match env.storage().persistent().get(&grant_key) {
            Some(g) => g,
            None => return Err(LicenseError::LicenseNotFound),
        };

        if grant.is_revoked {
            return Err(LicenseError::AlreadyRevoked);
        }

        // Fetch terms under which license was purchased
        let terms_key = (TERMS, puzzle_id, grant.version_bought);
        let terms: LicenseTerms = match env.storage().persistent().get(&terms_key) {
            Some(t) => t,
            None => return Err(LicenseError::TermsNotFound),
        };

        // Enforce policy: only revoke if terms allow it
        if !terms.allows_revocation {
            return Err(LicenseError::RevocationNotAllowed);
        }

        // Refund player if required by terms
        if terms.refund_on_revocation && grant.price_paid > 0 {
            token::Client::new(&env, &terms.fee_token).transfer(
                &caller,
                &player,
                &grant.price_paid,
            );
        }

        grant.is_revoked = true;
        env.storage().persistent().set(&grant_key, &grant);

        // Emit revocation event: (revoked, puzzle_id) -> (creator, player)
        env.events().publish(
            (symbol_short!("revoked"), puzzle_id),
            (caller, player),
        );

        Ok(())
    }

    /// Retrieve active license status for a player
    pub fn is_license_active(env: Env, puzzle_id: u64, player: Address) -> bool {
        let grant_key = (GRANT, puzzle_id, player);
        let grant: Option<LicenseGrant> = env.storage().persistent().get(&grant_key);

        match grant {
            Some(g) => {
                if g.is_revoked {
                    false
                } else if g.expires_at == 0 {
                    true
                } else {
                    env.ledger().timestamp() <= g.expires_at
                }
            }
            None => false,
        }
    }

    /// Retrieve license grant details
    pub fn get_license(env: Env, puzzle_id: u64, player: Address) -> Option<LicenseGrant> {
        let grant_key = (GRANT, puzzle_id, player);
        env.storage().persistent().get(&grant_key)
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
