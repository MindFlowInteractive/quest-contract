# Whitelist Contract

A Soroban smart contract for managing access control with tiered permissions, designed for exclusive puzzles, events, and features in the quest ecosystem.

## Features

### Core Functionality
- **Address Whitelisting**: Add/remove addresses with tier-based permissions
- **Tiered Access Control**: Multi-level permission system (tier 1, 2, 3, etc.)
- **Expiration Management**: Time-based whitelist entries with automatic expiration
- **Permission System**: Granular permission control per address and tier
- **Admin Management**: Secure admin role with transfer capabilities

### Advanced Features
- **Batch Operations**: Efficient bulk add/remove operations
- **Merkle Tree Verification**: Gas-optimized whitelist verification using merkle proofs
- **Snapshot System**: Create immutable snapshots of whitelist state
- **Tier-based Permissions**: Define permissions that apply to entire tiers

## Contract Interface

### Initialization
```rust
pub fn initialize(env: Env, admin: Address)
```

### Whitelist Management
```rust
pub fn add_to_whitelist(env: Env, admin: Address, address: Address, tier: u32, expiration: Option<u64>, permissions: Vec<Symbol>) -> Result<(), WhitelistError>
pub fn remove_from_whitelist(env: Env, admin: Address, address: Address) -> Result<(), WhitelistError>
pub fn batch_add_to_whitelist(env: Env, admin: Address, entries: Vec<WhitelistEntry>) -> Result<(), WhitelistError>
pub fn batch_remove_from_whitelist(env: Env, admin: Address, addresses: Vec<Address>) -> Result<(), WhitelistError>
```

### Access Verification
```rust
pub fn is_whitelisted(env: Env, address: Address, required_tier: Option<u32>) -> bool
pub fn has_permission(env: Env, address: Address, permission: Symbol) -> bool
pub fn get_whitelist_entry(env: Env, address: Address) -> Option<WhitelistEntry>
```

### Merkle Tree Operations
```rust
pub fn set_merkle_root(env: Env, admin: Address, merkle_root: BytesN<32>) -> Result<(), WhitelistError>
pub fn verify_merkle_proof(env: Env, address: Address, tier: u32, proof: Vec<BytesN<32>>) -> Result<bool, WhitelistError>
```

### Snapshot Management
```rust
pub fn create_snapshot(env: Env, admin: Address, merkle_root: BytesN<32>, total_entries: u32) -> Result<(), WhitelistError>
pub fn get_snapshot(env: Env) -> Option<WhitelistSnapshot>
```

### Admin Controls
```rust
pub fn transfer_admin(env: Env, current_admin: Address, new_admin: Address) -> Result<(), WhitelistError>
pub fn get_admin(env: Env) -> Option<Address>
```

## Data Structures

### WhitelistEntry
```rust
pub struct WhitelistEntry {
    pub address: Address,
    pub tier: u32,
    pub expiration: Option<u64>, // Block number for expiration
    pub permissions: Vec<Symbol>,
}
```

### WhitelistSnapshot
```rust
pub struct WhitelistSnapshot {
    pub block_number: u64,
    pub merkle_root: BytesN<32>,
    pub total_entries: u32,
}
```

## Usage Examples

### Basic Whitelisting
```rust
// Initialize contract
client.initialize(&admin);

// Add user with tier 2 access and specific permissions
let permissions = vec![symbol_short!("PUZZLE"), symbol_short!("EVENT")];
client.add_to_whitelist(&admin, &user, &2, &None, &permissions);

// Check if user is whitelisted for tier 1 access
let is_whitelisted = client.is_whitelisted(&user, &Some(1)); // true (tier 2 >= tier 1)
```

### Expiration-based Access
```rust
// Add user with expiration at block 1000
let expiration = Some(1000u64);
client.add_to_whitelist(&admin, &user, &1, &expiration, &permissions);

// Access automatically expires after block 1000
```

### Batch Operations
```rust
let entries = vec![
    WhitelistEntry { address: user1, tier: 1, expiration: None, permissions: perms1 },
    WhitelistEntry { address: user2, tier: 2, expiration: None, permissions: perms2 },
];

client.batch_add_to_whitelist(&admin, &entries);
```

### Merkle Proof Verification
```rust
// Set merkle root (computed off-chain)
client.set_merkle_root(&admin, &merkle_root);

// Verify user inclusion with proof
let is_valid = client.verify_merkle_proof(&user, &tier, &proof);
```

## Error Handling

The contract defines comprehensive error types:
- `NotAuthorized`: Caller is not authorized for the operation
- `AddressNotWhitelisted`: Address is not in the whitelist
- `InvalidTier`: Tier value is invalid (must be > 0)
- `ExpiredEntry`: Whitelist entry has expired
- `InvalidMerkleProof`: Merkle proof verification failed
- `AdminNotFound`: No admin is set
- `EntryAlreadyExists`: Entry already exists
- `InvalidPermission`: Permission is invalid

## Security Features

1. **Admin Authentication**: All admin operations require proper authentication
2. **Expiration Checks**: Automatic expiration validation on all access checks
3. **Tier Validation**: Prevents invalid tier assignments
4. **Merkle Verification**: Cryptographically secure proof verification
5. **Access Control**: Granular permission system

## Gas Optimization

- **Merkle Trees**: Use merkle proofs for large whitelists to reduce on-chain storage
- **Batch Operations**: Efficient bulk operations to reduce transaction costs
- **Persistent Storage**: Optimized storage patterns for frequently accessed data

## Testing

The contract includes comprehensive tests covering:
- Basic whitelist operations
- Tier-based access control
- Expiration handling
- Batch operations
- Merkle proof verification
- Admin controls
- Error conditions

Run tests with:
```bash
cargo test
```

## Deployment

Build the contract:
```bash
cargo build --target wasm32-unknown-unknown --release
```

Deploy to testnet using Soroban CLI or your preferred deployment method.