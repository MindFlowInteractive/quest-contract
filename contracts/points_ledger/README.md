# Achievement Points Ledger (Soroban Smart Contract)

A modular, auditable points accounting smart contract built for Soroban on the Stellar network. It provides on-chain tracking of achievement points earned by players across quests, puzzles, and milestones, with a full event history.

Only authorized contracts (such as verified puzzle engines, quest contracts, and administrators) can award points to players.

---

## 🚀 Features

- **Access-Controlled Point Earning**: Only designated administrators or explicitly authorized contracts can award points.
- **Complete Audit History**: Every point award records the player, amount, reason code (Soroban `Symbol`), timestamp, and caller address.
- **Strict Invariant**: Player balance strictly equals the sum of all historical earn entries.
- **Public Balance & History Queries**: Exposes gas-efficient read methods for querying player balances and complete award histories.
- **Audit Logging via Events**: Emits standard Soroban events `(earn, player) -> (amount, reason_code, caller)` upon each award.
- **Secure Governance**: Supports transferring the administrative role with two-way cryptographic verification.

---

## 📦 Data Structures

### `PointsEntry`

```rust
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PointsEntry {
    pub player: Address,
    pub amount: u64,
    pub reason_code: Symbol,
    pub timestamp: u64,
    pub caller: Address,
}
```

### `PlayerLedger`

```rust
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerLedger {
    pub balance: u64,
    pub history: Vec<PointsEntry>,
}
```

### Storage Keys

| Key | Storage Tier | Value Type | Description |
|:---|:---|:---|:---|
| `Symbol("ADMIN")` | Instance | `Address` | Current contract administrator |
| `(Symbol("AUTH"), Address)` | Persistent | `bool` | Authorized caller / game contract flag |
| `(Symbol("LEDGER"), Address)` | Persistent | `PlayerLedger` | Points balance and history entries for a player |

---

## 🛡️ Error Codes

```rust
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum PointsLedgerError {
    NotAuthorized = 1,       // Caller is not authorized to perform the operation
    AdminNotFound = 2,       // Admin address has not been set
    AlreadyInitialized = 3,  // Contract has already been initialized
    InvalidAmount = 4,       // Point amount is invalid (e.g., 0)
}
```

---

## 📡 Events

| Event Topic | Event Payload | Description |
|:---|:---|:---|
| `(Symbol("earn"), Address)` | `(u64, Symbol, Address)` | Emitted when points are awarded: `(amount, reason_code, caller)` |

---

## 💻 Contract Interface

```rust
pub trait PointsLedgerInterface {
    fn initialize(env: Env, admin: Address) -> Result<(), PointsLedgerError>;
    fn authorize_caller(env: Env, admin: Address, caller: Address) -> Result<(), PointsLedgerError>;
    fn deauthorize_caller(env: Env, admin: Address, caller: Address) -> Result<(), PointsLedgerError>;
    fn is_authorized(env: Env, caller: Address) -> bool;
    fn earn(env: Env, caller: Address, player: Address, amount: u64, reason_code: Symbol) -> Result<(), PointsLedgerError>;
    fn get_balance(env: Env, player: Address) -> u64;
    fn get_history(env: Env, player: Address) -> Vec<PointsEntry>;
    fn transfer_admin(env: Env, current_admin: Address, new_admin: Address) -> Result<(), PointsLedgerError>;
    fn get_admin(env: Env) -> Option<Address>;
}
```

---

## 🔗 Integration Example

A puzzle or achievement contract can award points to a player upon solving a challenge:

```rust
pub fn complete_puzzle(env: Env, player: Address, points_ledger_id: Address) {
    player.require_auth();

    // Game validation logic...

    let client = PointsLedgerContractClient::new(&env, &points_ledger_id);
    let contract_address = env.current_contract_address();
    client.earn(
        &contract_address,
        &player,
        &50,
        &symbol_short!("PUZZLE"),
    );
}
```

---

## 🧪 Testing

```bash
cargo check --package points_ledger
```
