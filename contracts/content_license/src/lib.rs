#![no_std]

mod tests;
mod types;

use soroban_sdk::{
    contract, contractimpl, contracttype, token, vec, Address, Env, Symbol, Vec,
};
pub use types::{License, LicenseKind, LicenseTerms};

const EVT_TERMS: &str = "terms_registered";
const EVT_PURCHASED: &str = "license_purchased";
const EVT_REVOKED: &str = "license_revoked";

#[contracttype]
pub enum DataKey {
    Admin,
    PaymentToken,
    /// Terms keyed by puzzle and immutable version.
    Terms(u64, u32),
    /// Latest published version for a puzzle.
    CurrentVersion(u64),
    /// All published versions for a puzzle.
    Versions(u64),
    /// One active licence per (holder, puzzle).
    License(Address, u64),
}

fn read_terms(env: &Env, puzzle_id: u64, version: u32) -> LicenseTerms {
    env.storage()
        .persistent()
        .get(&DataKey::Terms(puzzle_id, version))
        .expect("license terms not found")
}

fn read_license(env: &Env, holder: &Address, puzzle_id: u64) -> Option<License> {
    env.storage()
        .persistent()
        .get(&DataKey::License(holder.clone(), puzzle_id))
}

#[contract]
pub struct ContentLicenseContract;

#[contractimpl]
impl ContentLicenseContract {
    /// One-time setup. `payment_token` is the token used for paid licences.
    pub fn initialize(env: Env, admin: Address, payment_token: Address) {
        admin.require_auth();
        if env.storage().persistent().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().persistent().set(&DataKey::Admin, &admin);
        env.storage()
            .persistent()
            .set(&DataKey::PaymentToken, &payment_token);
    }

    // ─── #397: CONTENT LICENSE REGISTRY ─────────────────────────────────

    /// Publish a new, immutable version of the terms for `puzzle_id`.
    /// Only the creator of the previous version may publish a new version,
    /// and the first publisher becomes the puzzle's creator (#397).
    pub fn register_terms(
        env: Env,
        creator: Address,
        puzzle_id: u64,
        kind: LicenseKind,
        price: i128,
        duration_secs: u64,
        revocable: bool,
        refund_on_revoke: bool,
    ) -> u32 {
        creator.require_auth();
        if price < 0 {
            panic!("price must be non-negative");
        }

        let version = match env
            .storage()
            .persistent()
            .get::<_, u32>(&DataKey::CurrentVersion(puzzle_id))
        {
            Some(current) => {
                let existing = read_terms(&env, puzzle_id, current);
                if existing.creator != creator {
                    panic!("only the puzzle creator can publish new terms");
                }
                current + 1
            }
            None => 1,
        };

        let terms = LicenseTerms {
            puzzle_id,
            version,
            creator: creator.clone(),
            kind,
            price,
            duration_secs,
            revocable,
            refund_on_revoke,
            published_at: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Terms(puzzle_id, version), &terms);
        env.storage()
            .persistent()
            .set(&DataKey::CurrentVersion(puzzle_id), &version);

        let mut versions: Vec<u32> = env
            .storage()
            .persistent()
            .get(&DataKey::Versions(puzzle_id))
            .unwrap_or_else(|| vec![&env]);
        versions.push_back(version);
        env.storage()
            .persistent()
            .set(&DataKey::Versions(puzzle_id), &versions);

        env.events()
            .publish((Symbol::new(&env, EVT_TERMS), puzzle_id), (version, creator));

        version
    }

    /// Read an exact published version.
    pub fn get_terms(env: Env, puzzle_id: u64, version: u32) -> LicenseTerms {
        read_terms(&env, puzzle_id, version)
    }

    /// Read the newest published version for a puzzle.
    pub fn get_current_terms(env: Env, puzzle_id: u64) -> LicenseTerms {
        let version = env
            .storage()
            .persistent()
            .get::<_, u32>(&DataKey::CurrentVersion(puzzle_id))
            .expect("no terms published for this puzzle");
        read_terms(&env, puzzle_id, version)
    }

    /// All versions ever published for a puzzle.
    pub fn get_versions(env: Env, puzzle_id: u64) -> Vec<u32> {
        env.storage()
            .persistent()
            .get(&DataKey::Versions(puzzle_id))
            .unwrap_or_else(|| vec![&env])
    }

    // ─── #398: PURCHASE AND REVOCATION ──────────────────────────────────

    /// Buy a licence under a specific published version. Payment goes
    /// directly to the creator named in those terms (#398).
    pub fn purchase(env: Env, buyer: Address, puzzle_id: u64, version: u32) -> License {
        buyer.require_auth();
        let terms = read_terms(&env, puzzle_id, version);

        if let Some(existing) = read_license(&env, &buyer, puzzle_id) {
            if !existing.revoked
                && (existing.expires_at == 0 || existing.expires_at > env.ledger().timestamp())
            {
                panic!("an active licence already exists for this puzzle");
            }
        }

        if terms.price > 0 {
            let token_address: Address = env
                .storage()
                .persistent()
                .get(&DataKey::PaymentToken)
                .expect("payment token not configured");
            token::Client::new(&env, &token_address).transfer(
                &buyer,
                &terms.creator,
                &terms.price,
            );
        }

        let now = env.ledger().timestamp();
        let license = License {
            holder: buyer.clone(),
            puzzle_id,
            version,
            purchased_at: now,
            expires_at: if terms.duration_secs == 0 {
                0
            } else {
                now + terms.duration_secs
            },
            revoked: false,
        };
        env.storage()
            .persistent()
            .set(&DataKey::License(buyer.clone(), puzzle_id), &license);

        env.events().publish(
            (Symbol::new(&env, EVT_PURCHASED), buyer),
            (puzzle_id, version),
        );
        license
    }

    /// Revoke a licence only under the terms version that was purchased.
    /// Refunds the purchase price when those terms require it (#398).
    pub fn revoke(env: Env, creator: Address, holder: Address, puzzle_id: u64) -> License {
        creator.require_auth();

        let mut license = read_license(&env, &holder, puzzle_id).expect("license not found");
        if license.revoked {
            panic!("license already revoked");
        }
        let terms = read_terms(&env, puzzle_id, license.version);
        if terms.creator != creator {
            panic!("only the terms creator may revoke this license");
        }
        if !terms.revocable {
            panic!("these terms do not allow revocation");
        }

        if terms.refund_on_revoke && terms.price > 0 {
            let token_address: Address = env
                .storage()
                .persistent()
                .get(&DataKey::PaymentToken)
                .expect("payment token not configured");
            token::Client::new(&env, &token_address).transfer(
                &env.current_contract_address(),
                &holder,
                &terms.price,
            );
        }

        license.revoked = true;
        env.storage()
            .persistent()
            .set(&DataKey::License(holder.clone(), puzzle_id), &license);

        env.events().publish(
            (Symbol::new(&env, EVT_REVOKED), holder),
            (puzzle_id, license.version),
        );
        license
    }

    /// Whether the holder currently has a usable licence for the puzzle.
    pub fn is_license_active(env: Env, holder: Address, puzzle_id: u64) -> bool {
        match read_license(&env, &holder, puzzle_id) {
            Some(license) => {
                !license.revoked
                    && (license.expires_at == 0
                        || license.expires_at > env.ledger().timestamp())
            }
            None => false,
        }
    }

    /// Read the holder's licence record.
    pub fn get_license(env: Env, holder: Address, puzzle_id: u64) -> License {
        read_license(&env, &holder, puzzle_id).expect("license not found")
    }
}
