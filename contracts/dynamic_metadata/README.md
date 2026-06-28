# Dynamic NFT Metadata Contract

A Soroban smart contract that manages **fully dynamic NFT metadata** with versioning,
ownership history, trait evolution rules, triggering conditions, freezing, automatic
image URI generation, and a metadata migration path.

This contract closes **issue #268** (Dynamic NFT Metadata Contract). It models a token's
metadata as a struct that mutates in response to in-game events, metrics, time-based
counters, and ownership activity — every meaningful change is snapshotted into a
queryable version history.

## Overview

```
┌────────────────────────────────────────────────────────────────────┐
│                       DynamicMetadataContract                      │
├────────────────────────────────────────────────────────────────────┤
│  Metadata Structure      │ TokenMetadata (Name / Desc / Traits ..) │
│  Updates                 │ Owner / Admin mutate any field          │
│  Triggers                │ metric / ownership_count / time         │
│  History                 │ Mint / Transfer / Burn log per token    │
│  Trait Evolution         │ Performance-score rules w/ auto-apply   │
│  Versioning              │ Per-token Vec<MetadataVersion>          │
│  Freezing                │ Owner / Admin can lock a token          │
│  Image URI Generation    │ Deterministic from base + metadata      │
│  Migration               │ Migrator rebuilds under newer schema    │
└────────────────────────────────────────────────────────────────────┘
```

## Features

### 1. Metadata Structure (`TokenMetadata`)

A canonical, on-chain record that captures everything about a token, including
its current dynamic state:

```rust
pub struct TokenMetadata {
    pub token_id: u32,
    pub owner: Address,
    pub name: String,
    pub description: String,
    pub image_id: u32,
    pub external_uri: String,
    pub string_traits: Map<String, String>,    // e.g. "rank" -> "legendary"
    pub numeric_traits: Map<String, u32>,      // e.g. "power" -> 42
    pub level: u32,
    pub rarity: u32,
    pub performance_score: u64,
    pub schema_version: u32,
    pub created_at: u64,
    pub updated_at: u64,
    pub frozen: bool,
}
```

### 2. Metadata Updates

| Function                           | Authorization |
|------------------------------------|----------------|
| `mint_token`                       | Minter         |
| `update_metadata`                  | Owner or Admin |
| `add_string_trait`                 | Owner or Admin |
| `remove_string_trait`              | Owner or Admin |
| `add_numeric_trait`                | Owner or Admin |
| `increment_numeric_trait`          | Owner or Admin |
| `transfer_token`                   | Owner          |
| `burn_token`                       | Owner          |

Every update appends a snapshot to `Vec<MetadataVersion>` before the change takes
effect, so consumers can roll back to any prior version.

### 3. Triggering Conditions

Trigger rules wake up after every oracle update and walk three modes:

```rust
pub struct TriggerRule {
    pub trigger_type: String,        // "metric" | "ownership_count" | "time"
    pub metric_key: String,
    pub threshold: u64,
    pub action_type: String,          // "add_trait" | "add_numeric_trait" | "bump_level"
    pub action_param: String,
    pub action_value: String,
}
```

* **`metric`** — fires when a token's `metric_key` performance metric reaches
  `threshold`. Update the metric via `set_performance_metric` /
  `add_performance_metric` (oracle-only).
* **`ownership_count`** — fires once the token has had at least `threshold`
  distinct owners (read out of the ownership history).
* **`time`** — fires when `now - metadata.updated_at >= threshold`.

### 4. Ownership History Tracking

Every mint, transfer, and burn is recorded in `Vec<OwnershipRecord>`. Burns are
not counted toward `distinct_owners` for trigger evaluation:

```rust
pub struct OwnershipRecord {
    pub owner: Address,
    pub event: OwnershipEvent,         // Mint | Transfer | Burn
    pub at: u64,
    pub previous_owner: Address,
}
```

### 5. Trait Evolution Rules

```rust
pub struct TraitEvolutionRule {
    pub id: u64,
    pub metric: String,
    pub threshold: u64,
    pub target_trait_name: String,
    pub target_trait_value: String,
    pub target_level: u32,
    pub new_rarity: u32,
}
```

When the token's `performance_score >= threshold` AND
`level >= target_level`, the contract applies the highest-thresholded matching
rule. The applied rule:

* Writes `target_trait_value` into the string trait `target_trait_name`.
* Sets `rarity = max(rarity, new_rarity)`.
* Sets `level = max(level, target_level)`.

### 6. Metadata Versioning

Every meaningful change (`mint`, `update`, `transfer`, `burn`, trait rule fires,
trigger fires, freeze, unfreeze, migrate) appends a snapshot:

```rust
pub struct MetadataVersion {
    pub version: u32,
    pub snapshot: TokenMetadata,
    pub changed_by: Address,
    pub reason: String,
    pub changed_at: u64,
}
```

`get_version_history(token_id)` returns the entire chain — useful for off-chain
indexers, audit, or rollback paths.

### 7. Metadata Freezing

`freeze_metadata` / `unfreeze_metadata` lock all trait / metadata mutation. The
contract still:

* Allows transfers and oracle-driven metric collection.
* Records all events.
* Rejects `update_metadata`, `add_*_trait`, `update_*`, `burn_token`, and
  manual `evolve_traits` / `check_and_apply_triggers` invocations.

### 8. Image URI Generation

The contract builds a deterministic URI on read:

```
{image_base_uri}/nft/{token_id}/lvl/{level}/rar/{rarity}.png
```

The URI is cached per token (`GeneratedImageUri(token_id)`) and invalidated on
every metadata write. Changing `image_base_uri` invalidates and recomputes all
URIs lazily on the next read.

### 9. Metadata Migration

A migrator role can rebuild a token against a newer schema version:

```rust
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
) -> u32;
```

The pre-migration metadata is snapshotted into version history, then the new
state is written and re-snapshotted under reason `"migrated:{old}->{new}"`.

### 10. Authorization Model

| Role     | Can do                                                |
|----------|--------------------------------------------------------|
| Admin    | Configure rules, swap oracle / migrator, pause, set schema version / base URI, freeze any token |
| Migrator | Migrate token metadata to a newer schema                |
| Oracle   | Submit performance metrics that drive evolution / triggers |
| Owner    | Update metadata, traits, freeze/unfreeze, transfer, burn |

Authorization checks are explicit in every entrypoint — calling a role-restricted
function with the wrong principal panics with the corresponding sentinel.

## Events

| Event              | Emitted on                                  |
|--------------------|----------------------------------------------|
| `TokenMinted`      | New token created                            |
| `TokenTransferred` | Ownership change                             |
| `TokenBurned`      | Burn                                         |
| `MetadataUpdated`  | Any successful metadata update / migration  |
| `TraitEvolved`     | Trait evolution rule fired                   |
| `MetadataFrozen`   | Freeze / Unfreeze                            |

## Usage

```rust
// ---- Setup ----
client.initialize(&admin, &migrator, &oracle, &image_base_uri, &1u32);

// ---- Mint ----
let token_id = client.mint_token(
    &minter, &owner, &name, &description, &1u32, &external_uri,
    &string_traits, &numeric_traits,
);

// ---- Configure a trait-evolution rule ----
client.configure_trait_rule(
    &admin, "perf", &100u64, "rank", "legendary", &2u32, &3u32,
);

// ---- Configure a trigger rule ----
client.configure_trigger_rule(
    &admin,
    "metric", "wins", &10u64,
    "add_trait", "title", "veteran",
);

// ---- Oracle updates metric, which applies both rules automatically ----
client.set_performance_metric(&oracle, &token_id, "wins", &120u64);

// ---- Read end state ----
let m = client.get_metadata(&token_id);
let uri = client.get_image_uri(&token_id);
let versions = client.get_version_history(&token_id);
let history = client.get_ownership_history(&token_id);
```

## Deployment

The contract is deployable via standard Soroban CLI:

```bash
cargo build --manifest-path contracts/dynamic_metadata/Cargo.toml \
    --target wasm32-unknown-unknown --release
soroban contract deploy \
    --wasm target/wasm32-unknown-unknown/release/dynamic_metadata.wasm \
    --source deployer \
    --network testnet
```

## Testing

Run the full test suite:

```bash
cargo test --manifest-path contracts/dynamic_metadata/Cargo.toml
```

The suite covers:

* Initialization (success, double-init panics, schema validation, base URI)
* Minting (initial metadata, traits, snapshot, owner index)
* Updates (string / numeric traits, partial update, owner / admin / stranger auth)
* Ownership history (Mint, Transfer, Burn records)
* Trigger rules (metric, ownership_count, time, all action types)
* Trait evolution (threshold firing, manual invocation, removal)
* Versioning (snapshot chain, rollback shape, transfer version, evolution versions)
* Freezing (blocks updates, blocks trait adds, allows reads, unfreeze roundtrip)
* Image URI (format match, recompute on level change, base URI update)
* Migration (schema bump, role check, version direction)
* Role switching (oracle, migrator, pause flag)

## Future Enhancements

* Cross-contract evolution triggers (oracle hook)
* Batch evolution (per-game / per-collection)
* Hash-chained version history for cryptographic audit
* Composable traits (`cosmetic + utility + cosmetic` ordering)
* Off-chain JSON metadata via deterministic URI scheme
* Marketplace / auction metadata enrichment hooks

## License

This contract is part of the `quest-contract` project.
