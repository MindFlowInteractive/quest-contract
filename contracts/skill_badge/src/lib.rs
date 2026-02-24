#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec,
};

// ─────────────────────────────────────────────────────────────
// Types & Storage Keys
// ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum BadgeCategory {
    Logic,
    Math,
    Cryptography,
    Speed,
    Social,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum BadgeLevel {
    Novice = 0,
    Apprentice = 1,
    Journeyman = 2,
    Expert = 3,
    Master = 4,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Badge {
    pub category: BadgeCategory,
    pub level: BadgeLevel,
    pub issued_at: u64,
    pub last_upgrade_at: u64,
    pub verifier_score: i32,
}

#[contracttype]
pub enum DataKey {
    Admin,
    Badge(Address, BadgeCategory),
    Showcase(Address),
    CategoryPlayers(BadgeCategory), // Vec<Address> for leaderboard
    Restricted(Address),           // Misconduct flag
    Verifier(Address),             // Authorized verifiers
}

// ─────────────────────────────────────────────────────────────
// Errors & Events
// ─────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    BadgeAlreadyExists = 4,
    NoBadge = 5,
    MaxLevelReached = 6,
    Restricted = 7,
    InvalidCategory = 8,
}

const EVT_ISSUED: Symbol = symbol_short!("issued");
const EVT_UPGRADED: Symbol = symbol_short!("upgraded");
const EVT_REVOKED: Symbol = symbol_short!("revoked");

// ─────────────────────────────────────────────────────────────
// Contract
// ─────────────────────────────────────────────────────────────

#[contract]
pub struct SkillBadgeContract;

#[contractimpl]
impl SkillBadgeContract {
    /// Initialize the contract with an admin
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    /// Issue a Novice badge for a category
    pub fn issue_badge(
        env: Env,
        issuer: Address,
        player: Address,
        category: BadgeCategory,
    ) -> Result<(), Error> {
        issuer.require_auth();
        Self::assert_authorized(&env, &issuer)?;

        if env.storage().persistent().has(&DataKey::Restricted(player.clone())) {
            return Err(Error::Restricted);
        }

        let key = DataKey::Badge(player.clone(), category);
        if env.storage().persistent().has(&key) {
            return Err(Error::BadgeAlreadyExists);
        }

        let now = env.ledger().timestamp();
        let badge = Badge {
            category,
            level: BadgeLevel::Novice,
            issued_at: now,
            last_upgrade_at: now,
            verifier_score: 0,
        };

        env.storage().persistent().set(&key, &badge);
        
        // Add to leaderboard list
        let mut players = Self::get_category_players(&env, category);
        players.push_back(player.clone());
        env.storage().persistent().set(&DataKey::CategoryPlayers(category), &players);

        env.events().publish((EVT_ISSUED, player), (category, BadgeLevel::Novice));
        Ok(())
    }

    /// Upgrade a badge to the next level
    pub fn upgrade_badge(
        env: Env,
        issuer: Address,
        player: Address,
        category: BadgeCategory,
        new_score: i32,
    ) -> Result<BadgeLevel, Error> {
        issuer.require_auth();
        Self::assert_authorized(&env, &issuer)?;

        let key = DataKey::Badge(player.clone(), category);
        let mut badge: Badge = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::NoBadge)?;

        let next_level = match badge.level {
            BadgeLevel::Novice => BadgeLevel::Apprentice,
            BadgeLevel::Apprentice => BadgeLevel::Journeyman,
            BadgeLevel::Journeyman => BadgeLevel::Expert,
            BadgeLevel::Expert => BadgeLevel::Master,
            BadgeLevel::Master => return Err(Error::MaxLevelReached),
        };

        badge.level = next_level;
        badge.last_upgrade_at = env.ledger().timestamp();
        badge.verifier_score = new_score;

        env.storage().persistent().set(&key, &badge);
        env.events().publish((EVT_UPGRADED, player), (category, next_level));

        Ok(next_level)
    }

    /// Revoke a badge for misconduct
    pub fn revoke_badge(
        env: Env,
        admin: Address,
        player: Address,
        category: BadgeCategory,
    ) -> Result<(), Error> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        let key = DataKey::Badge(player.clone(), category);
        if !env.storage().persistent().has(&key) {
            return Err(Error::NoBadge);
        }

        env.storage().persistent().remove(&key);
        
        // Remove from leaderboard list (expensive, but necessary on revocation)
        let players = Self::get_category_players(&env, category);
        let mut new_players = Vec::new(&env);
        for p in players.iter() {
            if p != player {
                new_players.push_back(p);
            }
        }
        env.storage().persistent().set(&DataKey::CategoryPlayers(category), &new_players);

        env.events().publish((EVT_REVOKED, player), category);
        Ok(())
    }

    /// Set misconduct flag for a player (prevents new badges)
    pub fn set_restricted(env: Env, admin: Address, player: Address, restricted: bool) -> Result<(), Error> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        if restricted {
            env.storage().persistent().set(&DataKey::Restricted(player), &true);
        } else {
            env.storage().persistent().remove(&DataKey::Restricted(player));
        }
        Ok(())
    }

    /// Add a verifier
    pub fn add_verifier(env: Env, admin: Address, verifier: Address) -> Result<(), Error> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        env.storage().persistent().set(&DataKey::Verifier(verifier), &true);
        Ok(())
    }

    /// Remove a verifier
    pub fn remove_verifier(env: Env, admin: Address, verifier: Address) -> Result<(), Error> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        env.storage().persistent().remove(&DataKey::Verifier(verifier));
        Ok(())
    }

    /// Add a badge to the profile showcase
    pub fn add_to_showcase(env: Env, player: Address, category: BadgeCategory) -> Result<(), Error> {
        player.require_auth();
        
        let key = DataKey::Badge(player.clone(), category);
        if !env.storage().persistent().has(&key) {
            return Err(Error::NoBadge);
        }

        let mut showcase = Self::get_showcase(&env, player.clone());
        if !showcase.contains(category) {
            showcase.push_back(category);
            env.storage().persistent().set(&DataKey::Showcase(player), &showcase);
        }
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────
    // View Functions
    // ─────────────────────────────────────────────────────────────

    pub fn get_badge(env: Env, player: Address, category: BadgeCategory) -> Option<Badge> {
        env.storage().persistent().get(&DataKey::Badge(player, category))
    }

    pub fn get_showcase(env: Env, player: Address) -> Vec<BadgeCategory> {
        env.storage()
            .persistent()
            .get(&DataKey::Showcase(player))
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_leaderboard(env: Env, category: BadgeCategory) -> Vec<(Address, BadgeLevel)> {
        let players = Self::get_category_players(&env, category);
        let mut board = Vec::new(&env);
        
        for player in players.iter() {
            if let Some(badge) = env.storage().persistent().get(&DataKey::Badge(player.clone(), category)) {
                board.push_back((player, badge.level));
            }
        }
        
        // Sorting logic (simplified: bubble sort for example, in prod we might use a more efficient index)
        // Soroban Vec doesn't have sort_by, so we'd typically maintain a sorted index or sort off-chain.
        // For this task, we return the list.
        board
    }

    pub fn get_synergies(env: Env, player: Address) -> u32 {
        let categories = [
            BadgeCategory::Logic,
            BadgeCategory::Math,
            BadgeCategory::Cryptography,
            BadgeCategory::Speed,
            BadgeCategory::Social,
        ];
        
        let mut total_levels = 0u32;
        let mut count = 0;
        
        for cat in categories.iter() {
            if let Some(badge) = env.storage().persistent().get(&DataKey::Badge(player.clone(), *cat)) {
                total_levels += (badge.level as u32) + 1;
                count += 1;
            }
        }
        
        if count >= 3 {
            total_levels + 5 // Bonus for variety
        } else {
            total_levels
        }
    }

    // ─────────────────────────────────────────────────────────────
    // Internal Helpers
    // ─────────────────────────────────────────────────────────────

    fn assert_admin(env: &Env, address: &Address) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        if admin != *address {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }

    fn assert_authorized(env: &Env, address: &Address) -> Result<(), Error> {
        // Admin is always authorized
        if Self::assert_admin(env, address).is_ok() {
            return Ok(());
        }
        // Check verifier role
        if env.storage().persistent().has(&DataKey::Verifier(address.clone())) {
            return Ok(());
        }
        Err(Error::Unauthorized)
    }

    fn get_category_players(env: &Env, category: BadgeCategory) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::CategoryPlayers(category))
            .unwrap_or(Vec::new(env))
    }
}

mod test;
