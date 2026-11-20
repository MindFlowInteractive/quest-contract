# Address Blocklist Module (Soroban Smart Contract)

A modular, reusable address blocklist smart contract built for Soroban on the Stellar network. It provides on-chain access control and compliance capabilities by enabling an authorized administrator to block and unblock addresses (e.g., following confirmed cheating, exploits, or regulatory sanctions).

Other contracts across the ecosystem can query this contract synchronously via `is_blocked` to reject unwanted interactions before executing game or DeFi actions.

---

## 🚀 Features

- **Admin-Gated Modification**: Only the designated administrator can add or remove addresses from the blocklist.
- **Keyed Set Storage**: Stores blocked entries persistently under `(Symbol("BLIST"), Address)` for $O(1)$ lookups.
- **Contextual Metadata**: Stores `reason` (as a Soroban `Symbol`) and `blocked_at` ledger timestamp for every blocked address.
- **Public Read Interface**: Exposes a zero-cost read function `is_blocked(address: Address) -> bool` for easy integration.
- **Detailed Inquiry**: Exposes `get_block_entry(address: Address) -> Option<BlocklistEntry>` returning full block context.
- **Batch Operations**: Supports atomic batch operations (`batch_block` and `batch_unblock`) for administrative efficiency.
- **Audit Logging via Events**: Emits Soroban events on every single block and unblock action.
- **Admin Management**: Supports secure role transfer to a new admin via two-way authorization (`transfer_admin`).

---

## 📦 Data Structures

### `BlocklistEntry`

```rust
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlocklistEntry {
    pub address: Address,
    pub reason: Symbol,
    pub blocked_at: u64,
}
```

### Storage Keys

| Key | Storage Tier | Value Type | Description |
|:---|:---|:---|:---|
| `Symbol("ADMIN")` | Instance | `Address` | Current contract administrator |
| `(Symbol("BLIST"), Address)` | Persistent | `BlocklistEntry` | Block record for a specific address |

---

## 🛡️ Error Codes

```rust
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum BlocklistError {
    NotAuthorized = 1,       // Caller is not the authorized administrator
    AlreadyBlocked = 2,      // Address is already present in the blocklist
    NotBlocked = 3,          // Address was not found in the blocklist
    AdminNotFound = 4,       // Admin address has not been configured
    AlreadyInitialized = 5,  // Contract has already been initialized
}
```

---

## 📡 Events

| Event Topic | Event Payload | Description |
|:---|:---|:---|
| `(Symbol("block"), Address)` | `Symbol` (Reason) | Emitted when an address is added to the blocklist |
| `(Symbol("unblock"), Address)` | `()` | Emitted when an address is removed from the blocklist |

---

## 💻 Contract Interface

```rust
pub trait BlocklistInterface {
    fn initialize(env: Env, admin: Address) -> Result<(), BlocklistError>;
    fn block_address(env: Env, admin: Address, address: Address, reason: Symbol) -> Result<(), BlocklistError>;
    fn unblock_address(env: Env, admin: Address, address: Address) -> Result<(), BlocklistError>;
    fn is_blocked(env: Env, address: Address) -> bool;
    fn get_block_entry(env: Env, address: Address) -> Option<BlocklistEntry>;
    fn batch_block(env: Env, admin: Address, addresses: Vec<Address>, reason: Symbol) -> Result<(), BlocklistError>;
    fn batch_unblock(env: Env, admin: Address, addresses: Vec<Address>) -> Result<(), BlocklistError>;
    fn transfer_admin(env: Env, current_admin: Address, new_admin: Address) -> Result<(), BlocklistError>;
    fn get_admin(env: Env) -> Option<Address>;
}
```

---

## 🔗 Integration Example

Integrating contracts can check if a player or caller is blocked before executing gameplay actions:

```rust
// Example integration in a game or quest contract:
pub fn start_quest(env: Env, player: Address, blocklist_id: Address) {
    player.require_auth();

    let client = BlocklistContractClient::new(&env, &blocklist_id);
    if client.is_blocked(&player) {
        panic!("Player address is blocked from participating");
    }

    // Proceed with game logic...
}
```

---

## 🧪 Testing

Run unit tests covering single block/unblock, unauthorized callers, duplicate block prevention, batch operations, and admin transfers:

```bash
cargo test --package blocklist
```
