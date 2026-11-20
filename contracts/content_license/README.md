# Content License Registry (Soroban Smart Contract)

A decentralized puzzle and quest content licensing smart contract built for Soroban on the Stellar network. It allows creators to register, version, and manage licensing terms for game content on-chain.

---

## 🚀 Features

- **Creator-Gated Registration**: Only the authenticated creator of a puzzle can publish or update its license terms.
- **Immutable Term Versions**: Published terms cannot be modified in place. Any update requires publishing a new version, strictly preserving historical license records.
- **Flexible License Types**: Supports `FreeToPlay`, `PaidPerPlay` (with customizable fees), and `RemixAllowed`.
- **Fast Lookups**: Exposes `get_current_terms` for the latest active terms, as well as `get_terms_by_version` for auditability.
- **Event Emission**: Emits standard Soroban events `(license, puzzle_id) -> (creator, version)` on every published terms release.
- **Admin Governance**: Supports secure role transfer with two-way cryptographic verification.

---

## 📦 Data Structures

### `LicenseType`

```rust
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum LicenseType {
    FreeToPlay = 1,
    PaidPerPlay = 2,
    RemixAllowed = 3,
}
```

### `LicenseTerms`

```rust
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
```

### Storage Keys

| Key | Storage Tier | Value Type | Description |
|:---|:---|:---|:---|
| `Symbol("ADMIN")` | Instance | `Address` | Current contract administrator |
| `(Symbol("CREATOR"), u64)` | Persistent | `Address` | Registered creator of a puzzle |
| `(Symbol("LATEST"), u64)` | Persistent | `u32` | Latest published version number for a puzzle |
| `(Symbol("TERMS"), u64, u32)` | Persistent | `LicenseTerms` | Immutable terms record for a puzzle version |

---

## 🛡️ Error Codes

```rust
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum LicenseError {
    NotAuthorized = 1,          // Caller is not the registered creator or admin
    PuzzleNotFound = 2,         // Puzzle ID has not been registered
    VersionNotFound = 3,        // Requested version number does not exist
    AdminNotFound = 4,          // Admin address has not been set
    AlreadyInitialized = 5,     // Contract has already been initialized
}
```

---

## 📡 Events

| Event Topic | Event Payload | Description |
|:---|:---|:---|
| `(Symbol("license"), u64)` | `(Address, u32)` | Emitted when terms are published: `(creator, new_version)` |

---

## 💻 Contract Interface

```rust
pub trait ContentLicenseInterface {
    fn initialize(env: Env, admin: Address) -> Result<(), LicenseError>;
    fn register_terms(env: Env, creator: Address, puzzle_id: u64, license_type: LicenseType, fee_amount: u64, terms_uri: Symbol) -> Result<u32, LicenseError>;
    fn get_current_terms(env: Env, puzzle_id: u64) -> Option<LicenseTerms>;
    fn get_terms_by_version(env: Env, puzzle_id: u64, version: u32) -> Option<LicenseTerms>;
    fn get_creator(env: Env, puzzle_id: u64) -> Option<Address>;
    fn get_latest_version(env: Env, puzzle_id: u64) -> u32;
    fn transfer_admin(env: Env, current_admin: Address, new_admin: Address) -> Result<(), LicenseError>;
    fn get_admin(env: Env) -> Option<Address>;
}
```

---

## 🧪 Testing

```bash
cargo check --package content_license
```
