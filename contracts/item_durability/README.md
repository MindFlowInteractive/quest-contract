# Item Durability Module (Soroban Smart Contract)

A modular game mechanics smart contract built for Soroban on the Stellar network. It implements on-chain item durability tracking where game items experience degradation through gameplay usage.

---

## 🚀 Features

- **Item Registration**: Administrators register game items with defined maximum durability.
- **Usage & Degradation**: `use_item(user, item_id)` decrements the item's current durability by exactly 1 on every puzzle interaction.
- **Fail-Closed Zero Durability**: Rejects item usage when current durability reaches zero with `ZeroDurability`.
- **Owner Verification**: Ensures only the authenticated owner of the item can use it.
- **Durability Events**: Emits Soroban events `(use, item_id) -> (user, current_durability)` on every successful item usage.
- **Admin Governance**: Supports secure administrator role transfer with two-way cryptographic verification.

---

## 📦 Data Structures

### `Item`

```rust
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    pub owner: Address,
    pub max_durability: u32,
    pub current_durability: u32,
}
```

### Storage Keys

| Key | Storage Tier | Value Type | Description |
|:---|:---|:---|:---|
| `Symbol("ADMIN")` | Instance | `Address` | Current contract administrator |
| `(Symbol("ITEM"), u64)` | Persistent | `Item` | Item record mapped by unique `item_id` |

---

## 🛡️ Error Codes

```rust
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum DurabilityError {
    NotAuthorized = 1,          // Caller is not authorized or not the item owner
    ItemNotFound = 2,           // Item ID not found in persistent storage
    ZeroDurability = 3,         // Item has zero durability remaining and cannot be used
    InvalidMaxDurability = 4,   // Item max durability must be greater than zero
    ItemAlreadyExists = 5,      // Item ID already registered
    AdminNotFound = 6,          // Admin address has not been set
    AlreadyInitialized = 7,     // Contract has already been initialized
}
```

---

## 📡 Events

| Event Topic | Event Payload | Description |
|:---|:---|:---|
| `(Symbol("use"), u64)` | `(Address, u32)` | Emitted when an item is used: `(user, current_durability)` |

---

## 💻 Contract Interface

```rust
pub trait ItemDurabilityInterface {
    fn initialize(env: Env, admin: Address) -> Result<(), DurabilityError>;
    fn register_item(env: Env, admin: Address, item_id: u64, owner: Address, max_durability: u32) -> Result<(), DurabilityError>;
    fn use_item(env: Env, user: Address, item_id: u64) -> Result<u32, DurabilityError>;
    fn get_item(env: Env, item_id: u64) -> Option<Item>;
    fn get_durability(env: Env, item_id: u64) -> Result<u32, DurabilityError>;
    fn transfer_admin(env: Env, current_admin: Address, new_admin: Address) -> Result<(), DurabilityError>;
    fn get_admin(env: Env) -> Option<Address>;
}
```

---

## 🧪 Testing

```bash
cargo check --package item_durability
```
