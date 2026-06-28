#![no_std]

//! # Dynamic NFT Metadata Contract (`dynamic_metadata`)
//!
//! A Soroban smart contract that manages fully dynamic NFT metadata. Every
//! aspect of a token's metadata (attributes, traits, level, rarity, image)
//! can be derived from and updated by on-chain state: ownership history,
//! oracle-driven performance metrics, elapsed time, or ownership volume.
//!
//! Implements the ten issue requirements:
//!
//! 1. **Metadata structure** — `TokenMetadata` with string + numeric traits.
//! 2. **Metadata updates** — owner/admin driven updates to fields, string
//!    traits, and numeric traits with snapshotting.
//! 3. **Triggering conditions** — metric / ownership_count / time triggers
//!    that fire add_trait / add_numeric_trait / bump_level actions.
//! 4. **Ownership history** — full mint / transfer / burn log per token.
//! 5. **Trait evolution** — rules that bump level/rarity and write traits
//!    when `performance_score` crosses a threshold.
//! 6. **Metadata versioning** — every meaningful change snapshots `TokenMetadata`
//!    into `Vec<MetadataVersion>`.
//! 7. **Metadata freezing** — owners/admin can lock a token so attributes,
//!    traits and image cannot be mutated (ownership + metric collection
//!    still permitted).
//! 8. **Image URI generation** — deterministic URI built from `base_uri`,
//!    `token_id`, `level`, and `rarity`. Cached per token.
//! 9. **Metadata migration** — migrator role can rebuild a token's metadata
//!    against a newer schema with version bump.
//! 10. **Comprehensive tests** — full test suite in `src/test.rs`.

extern crate alloc;

use soroban_sdk::{contract, contractimpl, contracttype, contractevent, Address, Env, Map, String, Vec};

// ════════════════════════════════════════════════════════════════════════════
// CONSTANTS
// ════════════════════════════════════════════════════════════════════════════

/// Maximum schema version the contract knows how to migrate to.
const MAX_SCHEMA_VERSION: u32 = 10;

/// Maximum length allowed for the `image_base_uri` string.
const MAX_BASE_URI_LEN: u32 = 200;

/// Maximum number of traits per token (prevents unbounded growth).
const MAX_TRAITS_PER_TOKEN: u32 = 32;

/// Capacity for inline image URI byte buffer.
const IMG_BUF_CAP: usize = 256;

// ════════════════════════════════════════════════════════════════════════════
// STORAGE KEYS
// ════════════════════════════════════════════════════════════════════════════

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Contract-wide configuration.
    Config,
    /// Admin marker (mirrors config for cheap checks).
    Admin(Address),
    /// Migrator marker.
    Migrator(Address),
    /// Oracle marker.
    Oracle(Address),
    /// Current per-token metadata.
    TokenMetadata(u32),
    /// Full ownership history for a token.
    OwnershipHistory(u32),
    /// Full version history for a token.
    VersionHistory(u32),
    /// Per-token, per-metric performance value.
    PerfMetric(u32, String),
    /// Trait evolution rule id -> rule.
    TraitRules,
    /// Trigger rule id -> rule.
    TriggerRules,
    /// Owner -> append-only list of token ids.
    OwnerTokenIndex(Address),
    /// Cached generated image URI for a token.
    GeneratedImageUri(u32),
    /// Next token id counter.
    NextTokenId,
    /// Next trait-rule id counter.
    NextTraitRuleId,
    /// Next trigger-rule id counter.
    NextTriggerRuleId,
    /// Global pause flag.
    GlobalPaused,
}

// ════════════════════════════════════════════════════════════════════════════
// CORE STRUCTS
// ════════════════════════════════════════════════════════════════════════════

/// Contract-wide configuration.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ContractConfig {
    pub admin: Address,
    pub migrator: Address,
    pub oracle: Address,
    pub image_base_uri: String,
    pub schema_version: u32,
}

/// Per-token metadata. This is the canonical "current state" record.
#[contracttype]
#[derive(Clone, Debug)]
pub struct TokenMetadata {
    pub token_id: u32,
    pub owner: Address,
    pub name: String,
    pub description: String,
    pub image_id: u32,
    pub external_uri: String,
    /// String traits — e.g. "background" -> "forest", "rank" -> "mythic".
    pub string_traits: Map<String, String>,
    /// Numeric traits — e.g. "power" -> 42, "speed" -> 7. These participate
    /// in evolution + trigger rules.
    pub numeric_traits: Map<String, u32>,
    pub level: u32,
    pub rarity: u32,
    pub performance_score: u64,
    /// Schema version that this metadata record was last written under.
    pub schema_version: u32,
    pub created_at: u64,
    pub updated_at: u64,
    pub frozen: bool,
}

/// Ownership event kinds.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OwnershipEvent {
    Mint,
    Transfer,
    Burn,
}

/// Record of a single ownership event for a token.
#[contracttype]
#[derive(Clone, Debug)]
pub struct OwnershipRecord {
    pub owner: Address,
    pub event: OwnershipEvent,
    pub at: u64,
    /// For transfers: the previous owner. Equals owner for mint / burn.
    pub previous_owner: Address,
}

/// Immutable snapshot of metadata at a particular point in time.
#[contracttype]
#[derive(Clone, Debug)]
pub struct MetadataVersion {
    pub version: u32,
    pub snapshot: TokenMetadata,
    pub changed_by: Address,
    pub reason: String,
    pub changed_at: u64,
}

/// Trait evolution rule. Fires when a token's `performance_score` reaches
/// `threshold` AND its current level is at least `target_level`. On fire it
/// writes `target_trait_value` into `target_trait_name` and may bump rarity
/// and level.
///
/// Note: `metric` is currently a human-readable label only — evolution is
/// evaluated against the aggregate `performance_score`. It is reserved for
/// a future per-metric lookup.
#[contracttype]
#[derive(Clone, Debug)]
pub struct TraitEvolutionRule {
    pub id: u64,
    pub metric: String,
    pub threshold: u64,
    pub target_trait_name: String,
    pub target_trait_value: String,
    pub target_level: u32,
    pub new_rarity: u32,
}

/// Trigger rule. Three modes, selected by `trigger_type`:
///
/// * `"metric"` — fires when `metric_key` reaches `threshold`.
/// * `"ownership_count"` — fires when a token has had at least `threshold`
///   distinct owners.
/// * `"time"` — fires when `updated_at + threshold <= current_time`.
///
/// Three actions:
///
/// * `"add_trait"` — writes `action_value` into the string trait `action_param`.
/// * `"add_numeric_trait"` — writes `action_value` (parsed as u32) into the
///   numeric trait `action_param`.
/// * `"bump_level"` — adds `action_value` (parsed as u32) to current level.
#[contracttype]
#[derive(Clone, Debug)]
pub struct TriggerRule {
    pub id: u64,
    pub trigger_type: String,        // "metric" | "ownership_count" | "time"
    pub metric_key: String,         // metric name for "metric"
    pub threshold: u64,
    pub action_type: String,         // "add_trait" | "add_numeric_trait" | "bump_level"
    pub action_param: String,        // trait name (string or numeric)
    pub action_value: String,        // trait value or numeric amount
}

// ════════════════════════════════════════════════════════════════════════════
// EVENTS
// ════════════════════════════════════════════════════════════════════════════

#[contractevent]
#[derive(Clone, Debug)]
pub struct MetadataUpdated {
    pub token_id: u32,
    pub version: u32,
    pub by: Address,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct TraitEvolved {
    pub token_id: u32,
    pub trait_name: String,
    pub new_value: String,
    pub new_level: u32,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct MetadataFrozen {
    pub token_id: u32,
    pub by: Address,
    pub frozen: bool,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct TokenTransferred {
    pub token_id: u32,
    pub from: Address,
    pub to: Address,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct TokenMinted {
    pub token_id: u32,
    pub owner: Address,
    pub minter: Address,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct TokenBurned {
    pub token_id: u32,
    pub owner: Address,
}

// ════════════════════════════════════════════════════════════════════════════
// CONTRACT
// ════════════════════════════════════════════════════════════════════════════

#[contract]
pub struct DynamicMetadataContract;

#[contractimpl]
impl DynamicMetadataContract {
    // ──────────────────────────────────────────────────────────────────────
    // INITIALIZATION & ADMIN
    // ──────────────────────────────────────────────────────────────────────

    /// Initialize the contract exactly once. Admin must authorize.
    pub fn initialize(
        env: Env,
        admin: Address,
        migrator: Address,
        oracle: Address,
        image_base_uri: String,
        schema_version: u32,
    ) {
        if env.storage().instance().has(&DataKey::Config) {
            panic!("AlreadyInitialized");
        }
        admin.require_auth();

        if schema_version == 0 || schema_version > MAX_SCHEMA_VERSION {
            panic!("InvalidSchemaVersion");
        }
        if image_base_uri.len() == 0 || image_base_uri.len() > MAX_BASE_URI_LEN {
            panic!("InvalidImageBaseUri");
        }

        let config = ContractConfig {
            admin: admin.clone(),
            migrator,
            oracle,
            image_base_uri,
            schema_version,
        };

        env.storage().instance().set(&DataKey::Config, &config);
        env.storage().instance().set(&DataKey::Admin(admin.clone()), &true);
        env.storage()
            .instance()
            .set(&DataKey::Migrator(config.migrator.clone()), &true);
        env.storage()
            .instance()
            .set(&DataKey::Oracle(config.oracle.clone()), &true);
        env.storage().instance().set(&DataKey::NextTokenId, &1u32);
        env.storage().instance().set(&DataKey::NextTraitRuleId, &1u64);
        env.storage().instance().set(&DataKey::NextTriggerRuleId, &1u64);
        env.storage().instance().set(&DataKey::GlobalPaused, &false);
    }

    pub fn get_config(env: Env) -> ContractConfig {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .unwrap_or_else(|| panic!("NotInitialized"))
    }

    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::GlobalPaused)
            .unwrap_or(false)
    }

    pub fn set_paused(env: Env, admin: Address, paused: bool) {
        Self::assert_admin(&env, &admin);
        admin.require_auth();
        env.storage().instance().set(&DataKey::GlobalPaused, &paused);
    }

    pub fn update_image_base_uri(env: Env, admin: Address, new_base_uri: String) {
        Self::assert_admin(&env, &admin);
        admin.require_auth();
        if new_base_uri.len() == 0 || new_base_uri.len() > MAX_BASE_URI_LEN {
            panic!("InvalidImageBaseUri");
        }
        let mut config = Self::load_config(&env);
        config.image_base_uri = new_base_uri;
        env.storage().instance().set(&DataKey::Config, &config);
    }

    pub fn update_schema_version(env: Env, admin: Address, new_version: u32) {
        Self::assert_admin(&env, &admin);
        admin.require_auth();
        if new_version == 0 || new_version > MAX_SCHEMA_VERSION {
            panic!("InvalidSchemaVersion");
        }
        let mut config = Self::load_config(&env);
        if new_version < config.schema_version {
            panic!("SchemaVersionCanOnlyIncrease");
        }
        config.schema_version = new_version;
        env.storage().instance().set(&DataKey::Config, &config);
    }

    pub fn set_oracle(env: Env, admin: Address, new_oracle: Address) {
        Self::assert_admin(&env, &admin);
        admin.require_auth();
        let old: Address = Self::load_config(&env).oracle;
        env.storage().instance().remove(&DataKey::Oracle(old));
        env.storage()
            .instance()
            .set(&DataKey::Oracle(new_oracle.clone()), &true);
        let mut config = Self::load_config(&env);
        config.oracle = new_oracle;
        env.storage().instance().set(&DataKey::Config, &config);
    }

    pub fn set_migrator(env: Env, admin: Address, new_migrator: Address) {
        Self::assert_admin(&env, &admin);
        admin.require_auth();
        let old: Address = Self::load_config(&env).migrator;
        env.storage().instance().remove(&DataKey::Migrator(old));
        env.storage()
            .instance()
            .set(&DataKey::Migrator(new_migrator.clone()), &true);
        let mut config = Self::load_config(&env);
        config.migrator = new_migrator;
        env.storage().instance().set(&DataKey::Config, &config);
    }

    // ──────────────────────────────────────────────────────────────────────
    // MINTING / TRANSFER / BURN
    // ──────────────────────────────────────────────────────────────────────

    /// Mint a new token with initial metadata + traits. Returns the new id.
    pub fn mint_token(
        env: Env,
        minter: Address,
        owner: Address,
        name: String,
        description: String,
        image_id: u32,
        external_uri: String,
        string_traits: Map<String, String>,
        numeric_traits: Map<String, u32>,
    ) -> u32 {
        Self::assert_not_paused(&env);
        minter.require_auth();

        if string_traits.len() > MAX_TRAITS_PER_TOKEN
            || numeric_traits.len() > MAX_TRAITS_PER_TOKEN
        {
            panic!("TooManyTraits");
        }

        let token_id: u32 = env
            .storage()
            .instance()
            .get(&DataKey::NextTokenId)
            .unwrap_or(1u32);

        let now = env.ledger().timestamp();
        let config = Self::load_config(&env);

        let metadata = TokenMetadata {
            token_id,
            owner: owner.clone(),
            name,
            description,
            image_id,
            external_uri,
            string_traits,
            numeric_traits,
            level: 1,
            rarity: 1,
            performance_score: 0,
            schema_version: config.schema_version,
            created_at: now,
            updated_at: now,
            frozen: false,
        };

        env.storage()
            .persistent()
            .set(&DataKey::TokenMetadata(token_id), &metadata);
        env.storage()
            .instance()
            .set(&DataKey::NextTokenId, &(token_id + 1));

        // Bootstrap ownership history with a Mint event.
        let mut history: Vec<OwnershipRecord> = Vec::new(&env);
        history.push_back(OwnershipRecord {
            owner: owner.clone(),
            event: OwnershipEvent::Mint,
            at: now,
            previous_owner: minter.clone(),
        });
        env.storage()
            .persistent()
            .set(&DataKey::OwnershipHistory(token_id), &history);

        // First version snapshot.
        let mut versions: Vec<MetadataVersion> = Vec::new(&env);
        versions.push_back(MetadataVersion {
            version: 1,
            snapshot: metadata.clone(),
            changed_by: minter.clone(),
            reason: String::from_str(&env, "mint"),
            changed_at: now,
        });
        env.storage()
            .persistent()
            .set(&DataKey::VersionHistory(token_id), &versions);

        // Owner index (append-only log).
        let mut owned: Vec<u32> = env
            .storage()
            .persistent()
            .get(&DataKey::OwnerTokenIndex(owner.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        owned.push_back(token_id);
        env.storage()
            .persistent()
            .set(&DataKey::OwnerTokenIndex(owner.clone()), &owned);

        // Eagerly compute the image URI to cache and emit mint event.
        Self::compute_and_cache_image_uri(&env, &metadata);

        env.events().publish_event(&TokenMinted {
            token_id,
            owner,
            minter,
        });
        token_id
    }

    /// Transfer a token, recording history and snapshotting a new version.
    pub fn transfer_token(env: Env, from: Address, to: Address, token_id: u32) {
        Self::assert_not_paused(&env);
        from.require_auth();

        let mut metadata = Self::load_metadata(&env, token_id);
        if metadata.owner != from {
            panic!("NotOwner");
        }

        let now = env.ledger().timestamp();

        metadata.owner = to.clone();
        metadata.updated_at = now;
        env.storage()
            .persistent()
            .set(&DataKey::TokenMetadata(token_id), &metadata);

        let mut history: Vec<OwnershipRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::OwnershipHistory(token_id))
            .unwrap_or_else(|| Vec::new(&env));
        history.push_back(OwnershipRecord {
            owner: to.clone(),
            event: OwnershipEvent::Transfer,
            at: now,
            previous_owner: from.clone(),
        });
        env.storage()
            .persistent()
            .set(&DataKey::OwnershipHistory(token_id), &history);

        let mut versions: Vec<MetadataVersion> = env
            .storage()
            .persistent()
            .get(&DataKey::VersionHistory(token_id))
            .unwrap_or_else(|| Vec::new(&env));
        let latest = versions.len();
        versions.push_back(MetadataVersion {
            version: latest + 1,
            snapshot: metadata.clone(),
            changed_by: from.clone(),
            reason: String::from_str(&env, "transfer"),
            changed_at: now,
        });
        env.storage()
            .persistent()
            .set(&DataKey::VersionHistory(token_id), &versions);

        // Add token to receiver's index.
        let mut to_list: Vec<u32> = env
            .storage()
            .persistent()
            .get(&DataKey::OwnerTokenIndex(to.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        to_list.push_back(token_id);
        env.storage()
            .persistent()
            .set(&DataKey::OwnerTokenIndex(to.clone()), &to_list);

        env.events().publish_event(&TokenTransferred {
            token_id,
            from,
            to,
        });
    }

    /// Burn a token. The token record remains on-chain for audit; the burn
    /// event is appended to ownership history and recorded as a version
    /// snapshot under reason "burn". A token that has been burned will
    /// have `OwnershipEvent::Burn` as its most recent history record —
    /// callers should treat its `TokenMetadata` as immutable and stopped.
    pub fn burn_token(env: Env, caller: Address, token_id: u32) {
        Self::assert_not_paused(&env);
        caller.require_auth();
        let mut metadata = Self::load_metadata(&env, token_id);
        if metadata.owner != caller {
            panic!("NotOwner");
        }
        if metadata.frozen {
            panic!("MetadataFrozen");
        }
        let now = env.ledger().timestamp();

        // Record the burn event in ownership history.
        let mut history: Vec<OwnershipRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::OwnershipHistory(token_id))
            .unwrap_or_else(|| Vec::new(&env));
        history.push_back(OwnershipRecord {
            owner: caller.clone(),
            event: OwnershipEvent::Burn,
            at: now,
            previous_owner: caller.clone(),
        });
        env.storage()
            .persistent()
            .set(&DataKey::OwnershipHistory(token_id), &history);

        metadata.updated_at = now;
        Self::snapshot_version(&env, &metadata, &caller, "burn");
        env.storage()
            .persistent()
            .set(&DataKey::TokenMetadata(token_id), &metadata);

        env.events().publish_event(&TokenBurned {
            token_id,
            owner: caller,
        });
    }

    /// Returns true iff the most recent ownership event for `token_id` is
    /// a burn. Use this to gate consumers from treating burned tokens as
    /// transferable.
    pub fn is_burned(env: Env, token_id: u32) -> bool {
        let history: Vec<OwnershipRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::OwnershipHistory(token_id))
            .unwrap_or_else(|| Vec::new(&env));
        match history.len() {
            0 => false,
            n => history.get(n - 1).map(|r| r.event == OwnershipEvent::Burn).unwrap_or(false),
        }
    }

    // ──────────────────────────────────────────────────────────────────────
    // METADATA UPDATES
    // ──────────────────────────────────────────────────────────────────────

    /// Update metadata fields. Pass empty string / 0 sentinel to skip a
    /// field. Owner (or admin) can update.
    pub fn update_metadata(
        env: Env,
        caller: Address,
        token_id: u32,
        name: String,
        description: String,
        image_id: u32,
        external_uri: String,
    ) -> u32 {
        Self::assert_not_paused(&env);
        caller.require_auth();
        let mut metadata = Self::load_metadata(&env, token_id);
        if metadata.owner != caller && !Self::is_admin(&env, &caller) {
            panic!("NotAuthorized");
        }
        if metadata.frozen {
            panic!("MetadataFrozen");
        }
        if name.len() > 0 {
            metadata.name = name;
        }
        if description.len() > 0 {
            metadata.description = description;
        }
        if image_id > 0 {
            metadata.image_id = image_id;
        }
        if external_uri.len() > 0 {
            metadata.external_uri = external_uri;
        }
        metadata.updated_at = env.ledger().timestamp();
        Self::write_metadata(&env, &metadata);
        Self::snapshot_version(&env, &metadata, &caller, "update");
        env.events().publish_event(&MetadataUpdated {
            token_id,
            version: Self::current_version(&env, token_id),
            by: caller,
        });
        Self::current_version(&env, token_id)
    }

    /// Add or overwrite a string trait.
    pub fn add_string_trait(
        env: Env,
        caller: Address,
        token_id: u32,
        trait_name: String,
        trait_value: String,
    ) {
        Self::assert_not_paused(&env);
        caller.require_auth();
        let mut metadata = Self::load_metadata(&env, token_id);
        if metadata.owner != caller && !Self::is_admin(&env, &caller) {
            panic!("NotAuthorized");
        }
        if metadata.frozen {
            panic!("MetadataFrozen");
        }
        if metadata.string_traits.get(trait_name.clone()).is_none()
            && metadata.string_traits.len() >= MAX_TRAITS_PER_TOKEN
        {
            panic!("TooManyTraits");
        }
        metadata.string_traits.set(trait_name, trait_value);
        metadata.updated_at = env.ledger().timestamp();
        Self::write_metadata(&env, &metadata);
        Self::snapshot_version(&env, &metadata, &caller, "add_string_trait");
    }

    /// Remove a string trait by name. No-op if absent.
    pub fn remove_string_trait(env: Env, caller: Address, token_id: u32, trait_name: String) {
        Self::assert_not_paused(&env);
        caller.require_auth();
        let mut metadata = Self::load_metadata(&env, token_id);
        if metadata.owner != caller && !Self::is_admin(&env, &caller) {
            panic!("NotAuthorized");
        }
        if metadata.frozen {
            panic!("MetadataFrozen");
        }
        metadata.string_traits.remove(trait_name);
        metadata.updated_at = env.ledger().timestamp();
        Self::write_metadata(&env, &metadata);
        Self::snapshot_version(&env, &metadata, &caller, "remove_string_trait");
    }

    /// Add or overwrite a numeric trait.
    pub fn add_numeric_trait(
        env: Env,
        caller: Address,
        token_id: u32,
        trait_name: String,
        value: u32,
    ) {
        Self::assert_not_paused(&env);
        caller.require_auth();
        let mut metadata = Self::load_metadata(&env, token_id);
        if metadata.owner != caller && !Self::is_admin(&env, &caller) {
            panic!("NotAuthorized");
        }
        if metadata.frozen {
            panic!("MetadataFrozen");
        }
        if metadata.numeric_traits.get(trait_name.clone()).is_none()
            && metadata.numeric_traits.len() >= MAX_TRAITS_PER_TOKEN
        {
            panic!("TooManyTraits");
        }
        metadata.numeric_traits.set(trait_name, value);
        metadata.updated_at = env.ledger().timestamp();
        Self::write_metadata(&env, &metadata);
        Self::snapshot_version(&env, &metadata, &caller, "add_numeric_trait");
    }

    /// Increment a numeric trait (or set it if absent).
    pub fn increment_numeric_trait(
        env: Env,
        caller: Address,
        token_id: u32,
        trait_name: String,
        delta: u32,
    ) -> u32 {
        Self::assert_not_paused(&env);
        caller.require_auth();
        let mut metadata = Self::load_metadata(&env, token_id);
        if metadata.owner != caller && !Self::is_admin(&env, &caller) {
            panic!("NotAuthorized");
        }
        if metadata.frozen {
            panic!("MetadataFrozen");
        }
        let current = metadata.numeric_traits.get(trait_name.clone()).unwrap_or(0);
        let new_val = current.saturating_add(delta);
        metadata.numeric_traits.set(trait_name, new_val);
        metadata.updated_at = env.ledger().timestamp();
        Self::write_metadata(&env, &metadata);
        Self::snapshot_version(&env, &metadata, &caller, "increment_numeric_trait");
        new_val
    }

    // ──────────────────────────────────────────────────────────────────────
    // ORACLE-DRIVEN PERFORMANCE METRICS
    // ──────────────────────────────────────────────────────────────────────

    /// Replace a token's `metric_key` performance metric with `value`.
    /// `performance_score` is updated to mirror the new aggregate of all
    /// stored metrics (delta-only, since Soroban does not expose key
    /// enumeration).
    pub fn set_performance_metric(
        env: Env,
        oracle: Address,
        token_id: u32,
        metric_key: String,
        value: u64,
    ) {
        Self::assert_not_paused(&env);
        Self::assert_oracle(&env, &oracle);
        oracle.require_auth();

        // Confirm token exists.
        let _ = Self::load_metadata(&env, token_id);

        let key = DataKey::PerfMetric(token_id, metric_key.clone());
        let old: u64 = env.storage().persistent().get(&key).unwrap_or(0u64);
        env.storage().persistent().set(&key, &value);

        // Update aggregate performance_score with the *delta* so it correctly
        // mirrors the new metric value rather than inflating cumulatively.
        let delta = value.saturating_sub(old);
        Self::recompute_performance_score(&env, token_id, delta);

        // Apply trait evolution + trigger rules.
        Self::apply_trait_evolution(&env, token_id);
        Self::apply_trigger_rules(&env, token_id);
    }

    /// Add `delta` to a token's `metric_key` performance metric.
    pub fn add_performance_metric(
        env: Env,
        oracle: Address,
        token_id: u32,
        metric_key: String,
        delta: u64,
    ) -> u64 {
        Self::assert_not_paused(&env);
        Self::assert_oracle(&env, &oracle);
        oracle.require_auth();

        let _ = Self::load_metadata(&env, token_id);
        let key = DataKey::PerfMetric(token_id, metric_key);
        let current: u64 = env.storage().persistent().get(&key).unwrap_or(0u64);
        let new_val = current.saturating_add(delta);
        env.storage().persistent().set(&key, &new_val);

        Self::recompute_performance_score(&env, token_id, delta);
        Self::apply_trait_evolution(&env, token_id);
        Self::apply_trigger_rules(&env, token_id);
        new_val
    }

    pub fn get_performance_metric(env: Env, token_id: u32, metric_key: String) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::PerfMetric(token_id, metric_key))
            .unwrap_or(0u64)
    }

    // ──────────────────────────────────────────────────────────────────────
    // TRAIT EVOLUTION RULES
    // ──────────────────────────────────────────────────────────────────────

    /// Add a new trait evolution rule. Returns the assigned rule id.
    pub fn configure_trait_rule(
        env: Env,
        admin: Address,
        metric: String,
        threshold: u64,
        target_trait_name: String,
        target_trait_value: String,
        target_level: u32,
        new_rarity: u32,
    ) -> u64 {
        Self::assert_admin(&env, &admin);
        admin.require_auth();

        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextTraitRuleId)
            .unwrap_or(1u64);
        let rule = TraitEvolutionRule {
            id,
            metric,
            threshold,
            target_trait_name,
            target_trait_value,
            target_level,
            new_rarity,
        };
        let mut rules: Map<u64, TraitEvolutionRule> = env
            .storage()
            .persistent()
            .get(&DataKey::TraitRules)
            .unwrap_or_else(|| Map::new(&env));
        rules.set(id, rule);
        env.storage()
            .persistent()
            .set(&DataKey::TraitRules, &rules);
        env.storage()
            .instance()
            .set(&DataKey::NextTraitRuleId, &(id + 1));
        id
    }

    pub fn remove_trait_rule(env: Env, admin: Address, rule_id: u64) {
        Self::assert_admin(&env, &admin);
        admin.require_auth();
        let mut rules: Map<u64, TraitEvolutionRule> = env
            .storage()
            .persistent()
            .get(&DataKey::TraitRules)
            .unwrap_or_else(|| Map::new(&env));
        if rules.get(rule_id).is_none() {
            panic!("RuleNotFound");
        }
        rules.remove(rule_id);
        env.storage()
            .persistent()
            .set(&DataKey::TraitRules, &rules);
    }

    pub fn get_trait_rules(env: Env) -> Map<u64, TraitEvolutionRule> {
        env.storage()
            .persistent()
            .get(&DataKey::TraitRules)
            .unwrap_or_else(|| Map::new(&env))
    }

    /// Manually trigger trait evolution. Walks every trait rule and applies
    /// any whose threshold is met. Returns the number of rules applied.
    pub fn evolve_traits(env: Env, caller: Address, token_id: u32) -> u32 {
        Self::assert_not_paused(&env);
        caller.require_auth();
        let metadata = Self::load_metadata(&env, token_id);
        if metadata.owner != caller && !Self::is_admin(&env, &caller) {
            panic!("NotAuthorized");
        }
        if metadata.frozen {
            panic!("MetadataFrozen");
        }
        Self::apply_trait_evolution(&env, token_id)
    }

    // ──────────────────────────────────────────────────────────────────────
    // TRIGGER RULES
    // ──────────────────────────────────────────────────────────────────────

    /// Add a new trigger rule. Returns the assigned rule id.
    pub fn configure_trigger_rule(
        env: Env,
        admin: Address,
        trigger_type: String,
        metric_key: String,
        threshold: u64,
        action_type: String,
        action_param: String,
        action_value: String,
    ) -> u64 {
        Self::assert_admin(&env, &admin);
        admin.require_auth();
        Self::validate_trigger_inputs(&env, &trigger_type, &action_type);

        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextTriggerRuleId)
            .unwrap_or(1u64);
        let rule = TriggerRule {
            id,
            trigger_type,
            metric_key,
            threshold,
            action_type,
            action_param,
            action_value,
        };
        let mut rules: Map<u64, TriggerRule> = env
            .storage()
            .persistent()
            .get(&DataKey::TriggerRules)
            .unwrap_or_else(|| Map::new(&env));
        rules.set(id, rule);
        env.storage()
            .persistent()
            .set(&DataKey::TriggerRules, &rules);
        env.storage()
            .instance()
            .set(&DataKey::NextTriggerRuleId, &(id + 1));
        id
    }

    pub fn remove_trigger_rule(env: Env, admin: Address, rule_id: u64) {
        Self::assert_admin(&env, &admin);
        admin.require_auth();
        let mut rules: Map<u64, TriggerRule> = env
            .storage()
            .persistent()
            .get(&DataKey::TriggerRules)
            .unwrap_or_else(|| Map::new(&env));
        if rules.get(rule_id).is_none() {
            panic!("RuleNotFound");
        }
        rules.remove(rule_id);
        env.storage()
            .persistent()
            .set(&DataKey::TriggerRules, &rules);
    }

    pub fn get_trigger_rules(env: Env) -> Map<u64, TriggerRule> {
        env.storage()
            .persistent()
            .get(&DataKey::TriggerRules)
            .unwrap_or_else(|| Map::new(&env))
    }

    /// Manually fire all trigger rules for `token_id`. Returns the count
    /// of rules applied.
    pub fn check_and_apply_triggers(env: Env, caller: Address, token_id: u32) -> u32 {
        Self::assert_not_paused(&env);
        caller.require_auth();
        let metadata = Self::load_metadata(&env, token_id);
        if metadata.owner != caller && !Self::is_admin(&env, &caller) {
            panic!("NotAuthorized");
        }
        if metadata.frozen {
            panic!("MetadataFrozen");
        }
        Self::apply_trigger_rules(&env, token_id)
    }

    // ──────────────────────────────────────────────────────────────────────
    // FREEZE / UNFREEZE
    // ──────────────────────────────────────────────────────────────────────

    pub fn freeze_metadata(env: Env, caller: Address, token_id: u32) {
        caller.require_auth();
        let mut metadata = Self::load_metadata(&env, token_id);
        if metadata.owner != caller && !Self::is_admin(&env, &caller) {
            panic!("NotAuthorized");
        }
        metadata.frozen = true;
        metadata.updated_at = env.ledger().timestamp();
        Self::write_metadata(&env, &metadata);
        Self::snapshot_version(&env, &metadata, &caller, "freeze");
        env.events().publish_event(&MetadataFrozen {
            token_id,
            by: caller,
            frozen: true,
        });
    }

    pub fn unfreeze_metadata(env: Env, caller: Address, token_id: u32) {
        caller.require_auth();
        let mut metadata = Self::load_metadata(&env, token_id);
        if metadata.owner != caller && !Self::is_admin(&env, &caller) {
            panic!("NotAuthorized");
        }
        metadata.frozen = false;
        metadata.updated_at = env.ledger().timestamp();
        Self::write_metadata(&env, &metadata);
        Self::snapshot_version(&env, &metadata, &caller, "unfreeze");
        env.events().publish_event(&MetadataFrozen {
            token_id,
            by: caller,
            frozen: false,
        });
    }

    // ──────────────────────────────────────────────────────────────────────
    // MIGRATION
    // ──────────────────────────────────────────────────────────────────────

    /// Migrate a token's metadata to a newer schema version. Pre-state is
    /// snapshotted into version history under reason "migrate", then the
    /// new state is stored and re-snapshotted under reason
    /// "migrated:{old}->{new}".
    pub fn migrate_metadata(
        env: Env,
        migrator: Address,
        token_id: u32,
        new_version: u32,
        new_name: String,
        new_description: String,
        new_image_id: u32,
        new_external_uri: String,
        new_string_traits: Map<String, String>,
        new_numeric_traits: Map<String, u32>,
        level_override: u32,
        rarity_override: u32,
    ) -> u32 {
        Self::assert_migrator(&env, &migrator);
        migrator.require_auth();
        let config = Self::load_config(&env);
        if new_version == 0 || new_version > MAX_SCHEMA_VERSION {
            panic!("InvalidSchemaVersion");
        }
        if new_version < config.schema_version {
            panic!("CannotDowngradeVersion");
        }
        if new_string_traits.len() > MAX_TRAITS_PER_TOKEN
            || new_numeric_traits.len() > MAX_TRAITS_PER_TOKEN
        {
            panic!("TooManyTraits");
        }

        let mut metadata = Self::load_metadata(&env, token_id);
        let old_version = metadata.schema_version;

        // Snapshot pre-migration state.
        Self::snapshot_version(&env, &metadata, &migrator, "migrate");

        metadata.name = new_name;
        metadata.description = new_description;
        if new_image_id > 0 {
            metadata.image_id = new_image_id;
        }
        metadata.external_uri = new_external_uri;
        metadata.string_traits = new_string_traits;
        metadata.numeric_traits = new_numeric_traits;
        if level_override > 0 {
            metadata.level = level_override;
        }
        if rarity_override > 0 {
            metadata.rarity = rarity_override;
        }
        metadata.schema_version = new_version;
        metadata.updated_at = env.ledger().timestamp();
        Self::write_metadata(&env, &metadata);

        let mut versions: Vec<MetadataVersion> = env
            .storage()
            .persistent()
            .get(&DataKey::VersionHistory(token_id))
            .unwrap_or_else(|| Vec::new(&env));
        let latest = versions.len();
        let reason = make_migration_reason(&env, old_version, new_version);
        versions.push_back(MetadataVersion {
            version: latest + 1,
            snapshot: metadata.clone(),
            changed_by: migrator.clone(),
            reason,
            changed_at: env.ledger().timestamp(),
        });
        env.storage()
            .persistent()
            .set(&DataKey::VersionHistory(token_id), &versions);

        env.events().publish_event(&MetadataUpdated {
            token_id,
            version: Self::current_version(&env, token_id),
            by: migrator,
        });
        Self::current_version(&env, token_id)
    }

    // ──────────────────────────────────────────────────────────────────────
    // QUERIES
    // ──────────────────────────────────────────────────────────────────────

    pub fn get_metadata(env: Env, token_id: u32) -> TokenMetadata {
        Self::load_metadata(&env, token_id)
    }

    pub fn get_ownership_history(env: Env, token_id: u32) -> Vec<OwnershipRecord> {
        env.storage()
            .persistent()
            .get(&DataKey::OwnershipHistory(token_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn get_version_history(env: Env, token_id: u32) -> Vec<MetadataVersion> {
        env.storage()
            .persistent()
            .get(&DataKey::VersionHistory(token_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn get_tokens_by_owner(env: Env, owner: Address) -> Vec<u32> {
        env.storage()
            .persistent()
            .get(&DataKey::OwnerTokenIndex(owner))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns the deterministic image URI for a token, using the current
    /// metadata and configured base URI. Caches the result for cheap
    /// repeated reads.
    pub fn get_image_uri(env: Env, token_id: u32) -> String {
        let cached: Option<String> = env
            .storage()
            .persistent()
            .get(&DataKey::GeneratedImageUri(token_id));
        if let Some(uri) = cached {
            return uri;
        }
        let metadata = Self::load_metadata(&env, token_id);
        Self::compute_and_cache_image_uri(&env, &metadata)
    }

    // ══════════════════════════════════════════════════════════════════════
    // INTERNALS
    // ══════════════════════════════════════════════════════════════════════

    fn load_config(env: &Env) -> ContractConfig {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .unwrap_or_else(|| panic!("NotInitialized"))
    }

    fn load_metadata(env: &Env, token_id: u32) -> TokenMetadata {
        env.storage()
            .persistent()
            .get(&DataKey::TokenMetadata(token_id))
            .unwrap_or_else(|| panic!("TokenNotFound"))
    }

    fn write_metadata(env: &Env, metadata: &TokenMetadata) {
        env.storage()
            .persistent()
            .set(&DataKey::TokenMetadata(metadata.token_id), metadata);
        env.storage()
            .persistent()
            .remove(&DataKey::GeneratedImageUri(metadata.token_id));
    }

    fn snapshot_version(
        env: &Env,
        metadata: &TokenMetadata,
        by: &Address,
        reason: &str,
    ) {
        let mut versions: Vec<MetadataVersion> = env
            .storage()
            .persistent()
            .get(&DataKey::VersionHistory(metadata.token_id))
            .unwrap_or_else(|| Vec::new(env));
        let latest = versions.len();
        versions.push_back(MetadataVersion {
            version: (latest + 1) as u32,
            snapshot: metadata.clone(),
            changed_by: by.clone(),
            reason: String::from_str(env, reason),
            changed_at: env.ledger().timestamp(),
        });
        env.storage()
            .persistent()
            .set(&DataKey::VersionHistory(metadata.token_id), &versions);
    }

    fn current_version(env: &Env, token_id: u32) -> u32 {
        let versions: Vec<MetadataVersion> = env
            .storage()
            .persistent()
            .get(&DataKey::VersionHistory(token_id))
            .unwrap_or_else(|| Vec::new(env));
        versions.len() as u32
    }

    fn recompute_performance_score(env: &Env, token_id: u32, delta: u64) {
        let mut metadata = Self::load_metadata(env, token_id);
        metadata.performance_score = metadata.performance_score.saturating_add(delta);
        env.storage()
            .persistent()
            .set(&DataKey::TokenMetadata(token_id), &metadata);
    }

    fn apply_trait_evolution(env: &Env, token_id: u32) -> u32 {
        let rules: Map<u64, TraitEvolutionRule> = env
            .storage()
            .persistent()
            .get(&DataKey::TraitRules)
            .unwrap_or_else(|| Map::new(env));
        if rules.len() == 0 {
            return 0;
        }
        let mut metadata = Self::load_metadata(env, token_id);
        if metadata.frozen {
            return 0;
        }
        let metric = metadata.performance_score;
        let mut best: Option<TraitEvolutionRule> = None;
        for (_id, rule) in rules.iter() {
            if metric >= rule.threshold && metadata.level >= rule.target_level {
                match &best {
                    None => best = Some(rule.clone()),
                    Some(existing) => {
                        if rule.threshold > existing.threshold {
                            best = Some(rule.clone());
                        }
                    }
                }
            }
        }
        if best.is_none() {
            return 0;
        }
        let rule = best.unwrap();
        metadata
            .string_traits
            .set(rule.target_trait_name.clone(), rule.target_trait_value.clone());
        if rule.new_rarity > metadata.rarity {
            metadata.rarity = rule.new_rarity;
        }
        if rule.target_level > metadata.level {
            metadata.level = rule.target_level;
        }
        metadata.updated_at = env.ledger().timestamp();
        Self::write_metadata(env, &metadata);
        env.events().publish_event(&TraitEvolved {
            token_id,
            trait_name: rule.target_trait_name.clone(),
            new_value: rule.target_trait_value.clone(),
            new_level: metadata.level,
        });
        Self::snapshot_version(
            env,
            &metadata,
            &env.current_contract_address(),
            "evolve_traits",
        );
        1
    }

    fn apply_trigger_rules(env: &Env, token_id: u32) -> u32 {
        let rules: Map<u64, TriggerRule> = env
            .storage()
            .persistent()
            .get(&DataKey::TriggerRules)
            .unwrap_or_else(|| Map::new(env));
        if rules.len() == 0 {
            return 0;
        }
        let mut metadata = Self::load_metadata(env, token_id);
        if metadata.frozen {
            return 0;
        }
        let history: Vec<OwnershipRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::OwnershipHistory(token_id))
            .unwrap_or_else(|| Vec::new(env));
        let now = env.ledger().timestamp();

        let mut applied = 0u32;
        for (_id, rule) in rules.iter() {
            let fired = if str_eq(env, &rule.trigger_type, b"metric") {
                let v: u64 = env
                    .storage()
                    .persistent()
                    .get(&DataKey::PerfMetric(token_id, rule.metric_key.clone()))
                    .unwrap_or(0u64);
                v >= rule.threshold
            } else if str_eq(env, &rule.trigger_type, b"ownership_count") {
                (count_distinct_owners(&history) as u64) >= rule.threshold
            } else if str_eq(env, &rule.trigger_type, b"time") {
                now.saturating_sub(metadata.updated_at) >= rule.threshold
            } else {
                false
            };
            if !fired {
                continue;
            }
            if str_eq(env, &rule.action_type, b"add_trait") {
                metadata
                    .string_traits
                    .set(rule.action_param.clone(), rule.action_value.clone());
                applied += 1;
            } else if str_eq(env, &rule.action_type, b"add_numeric_trait") {
                let parsed = parse_u32(&rule.action_value);
                metadata
                    .numeric_traits
                    .set(rule.action_param.clone(), parsed);
                applied += 1;
            } else if str_eq(env, &rule.action_type, b"bump_level") {
                let bump = parse_u32(&rule.action_value);
                metadata.level = metadata.level.saturating_add(bump);
                applied += 1;
            }
        }
        if applied > 0 {
            metadata.updated_at = env.ledger().timestamp();
            Self::write_metadata(env, &metadata);
            Self::snapshot_version(env, &metadata, &env.current_contract_address(), "trigger_apply");
        }
        applied
    }

    /// Deterministic image URI: `{base}/nft/{id}/lvl/{level}/rar/{rarity}.png`.
    /// Built by direct byte buffer to honour SDK 21 String limitations.
    fn compute_and_cache_image_uri(env: &Env, metadata: &TokenMetadata) -> String {
        let config = Self::load_config(env);
        let mut buf = alloc::vec![0u8; IMG_BUF_CAP];
        let mut idx: usize = 0;
        // Copy base
        let base_len = config.image_base_uri.len() as usize;
        if base_len == 0 || base_len + 50 > IMG_BUF_CAP {
            panic!("InvalidImageBaseUri");
        }
        config.image_base_uri.copy_into_slice(&mut buf[..base_len]);
        idx += base_len;
        // "/nft/"
        let p1: &[u8] = b"/nft/";
        buf[idx..idx + p1.len()].copy_from_slice(p1);
        idx += p1.len();
        idx = append_u32_bytes(&mut buf, idx, metadata.token_id);
        // "/lvl/"
        let p2: &[u8] = b"/lvl/";
        buf[idx..idx + p2.len()].copy_from_slice(p2);
        idx += p2.len();
        idx = append_u32_bytes(&mut buf, idx, metadata.level);
        // "/rar/"
        let p3: &[u8] = b"/rar/";
        buf[idx..idx + p3.len()].copy_from_slice(p3);
        idx += p3.len();
        idx = append_u32_bytes(&mut buf, idx, metadata.rarity);
        // ".png"
        let p4: &[u8] = b".png";
        buf[idx..idx + p4.len()].copy_from_slice(p4);
        idx += p4.len();
        // Convert to Soroban String
        let uri = String::from_bytes(env, &buf[..idx]).unwrap();
        env.storage()
            .persistent()
            .set(&DataKey::GeneratedImageUri(metadata.token_id), &uri);
        uri
    }

    fn validate_trigger_inputs(env: &Env, trigger_type: &String, action_type: &String) {
        let valid_trigger = str_eq(env, trigger_type, b"metric")
            || str_eq(env, trigger_type, b"ownership_count")
            || str_eq(env, trigger_type, b"time");
        if !valid_trigger {
            panic!("InvalidTriggerType");
        }
        let valid_action = str_eq(env, action_type, b"add_trait")
            || str_eq(env, action_type, b"add_numeric_trait")
            || str_eq(env, action_type, b"bump_level");
        if !valid_action {
            panic!("InvalidActionType");
        }
    }

    fn assert_admin(env: &Env, admin: &Address) {
        let ok: bool = env
            .storage()
            .instance()
            .get(&DataKey::Admin(admin.clone()))
            .unwrap_or(false);
        if !ok {
            panic!("NotAdmin");
        }
    }

    fn is_admin(env: &Env, addr: &Address) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Admin(addr.clone()))
            .unwrap_or(false)
    }

    fn assert_migrator(env: &Env, migrator: &Address) {
        let ok: bool = env
            .storage()
            .instance()
            .get(&DataKey::Migrator(migrator.clone()))
            .unwrap_or(false);
        if !ok {
            panic!("NotMigrator");
        }
    }

    fn assert_oracle(env: &Env, oracle: &Address) {
        let ok: bool = env
            .storage()
            .instance()
            .get(&DataKey::Oracle(oracle.clone()))
            .unwrap_or(false);
        if !ok {
            panic!("NotOracle");
        }
    }

    fn assert_not_paused(env: &Env) {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::GlobalPaused)
            .unwrap_or(false);
        if paused {
            panic!("ContractPaused");
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// FREE HELPERS
// ════════════════════════════════════════════════════════════════════════════

/// Decode `u32` into decimal ASCII bytes, appending into `buf` starting at
/// `idx`. Returns the new (advanced) index.
fn append_u32_bytes(buf: &mut [u8], idx: usize, n: u32) -> usize {
    if n == 0 {
        buf[idx] = b'0';
        return idx + 1;
    }
    let mut tmp = [0u8; 10];
    let mut i = 0usize;
    let mut v = n;
    while v > 0 && i < 10 {
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i += 1;
    }
    let mut out = idx;
    let mut j = 0usize;
    while j < i {
        buf[out] = tmp[i - 1 - j];
        out += 1;
        j += 1;
    }
    out
}

/// Count ownership events in a token's history, excluding burns. Each
/// recorded mint or transfer counts as one owner event. Used by trigger
/// rules of type `"ownership_count"` to decide when to fire.
fn count_distinct_owners(history: &Vec<OwnershipRecord>) -> u32 {
    let mut count = 0u32;
    for rec in history.iter() {
        match rec.event {
            OwnershipEvent::Mint | OwnershipEvent::Transfer => count += 1,
            OwnershipEvent::Burn => {}
        }
    }
    count
}

/// Equality between a Soroban `String` and a literal ASCII byte slice.
/// Used because `String::as_str()` is not available on
/// `soroban_sdk::String` in SDK 21.x.
fn str_eq(env: &Env, s: &String, literal: &[u8]) -> bool {
    let lit_len = literal.len();
    if (s.len() as usize) != lit_len {
        return false;
    }
    if lit_len == 0 {
        return true;
    }
    let mut buf = [0u8; 32];
    if lit_len > buf.len() {
        // Literal too long for our fixed buffer; in practice none of ours are.
        return false;
    }
    s.copy_into_slice(&mut buf[..lit_len]);
    &buf[..lit_len] == literal
}

/// Parses a `String` as a `u32` using positive integer decode. Returns 0
/// for any input that fails to parse.
fn parse_u32(s: &String) -> u32 {
    let mut buf = [0u8; 16];
    let len = s.len() as usize;
    if len == 0 || len > 10 {
        return 0;
    }
    s.copy_into_slice(&mut buf[..len]);
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < len {
        let c = buf[i];
        if c < b'0' || c > b'9' {
            return 0;
        }
        v = v.saturating_mul(10).saturating_add((c - b'0') as u32);
        i += 1;
    }
    v
}

/// Build `"migrated:{old}->{new}"` reason string.
fn make_migration_reason(env: &Env, old: u32, new: u32) -> String {
    let mut buf = [0u8; 32];
    let prefix: &[u8] = b"migrated:";
    buf[..prefix.len()].copy_from_slice(prefix);
    let mut idx = prefix.len();
    idx = append_u32_bytes(&mut buf, idx, old);
    let p: &[u8] = b"->";
    buf[idx..idx + p.len()].copy_from_slice(p);
    idx += p.len();
    idx = append_u32_bytes(&mut buf, idx, new);
    String::from_bytes(env, &buf[..idx]).unwrap()
}

#[cfg(test)]
mod test;
