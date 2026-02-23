# NFT Wrapper Contract

A Soroban smart contract enabling secure cross-chain NFT transfers between Stellar and other blockchains through a validator-based bridge system.

## Overview

The NFT Wrapper Contract enables:
- **Cross-chain NFT transfers** between Stellar and other blockchains
- **Lock/Unlock mechanism** to secure NFTs on source chains
- **Wrapped NFT minting** with full metadata preservation
- **Multi-signature validator verification** for security
- **Emergency pause mechanism** for safety
- **Proof-based verification** for trust
- **Fee collection** for bridge operations

## Architecture

### Core Components

#### 1. **Lock/Unlock Mechanism**
- NFTs are locked on the source chain when initiated for transfer
- Validators verify the lock event
- Wrapped NFTs are minted on the destination chain
- Original NFTs are held in escrow until unwrap

#### 2. **Wrapped NFT Standard**
- Preserves original NFT metadata (name, symbol, URI)
- Tracks original contract and token ID
- Maps wrapped tokens back to originals
- Maintains ownership lineage

#### 3. **Validator System**
- Multi-signature validation for bridge transfers
- Configurable number of required signatures (default: 2)
- Validator registration and removal by admin
- Active/inactive validator states
- Duplicate signature prevention

#### 4. **Proof Verification**
- Validators verify cross-chain transfer proofs
- Signature validation before wrapping
- Status tracking through transfer lifecycle
- Timestamp records for auditing

#### 5. **Emergency Controls**
- Admin-controlled pause/unpause
- Prevents operations when paused
- Immediate response to security issues

#### 6. **Fee Management**
- Base fee in basis points (default: 50 bps = 0.5%)
- Minimum and maximum fee boundaries
- Fee token configuration
- Fee collection by designated collector

## Data Structures

### BridgeTransfer
Represents a complete cross-chain transfer:
```rust
pub struct BridgeTransfer {
    pub id: u64,                              // Unique transfer ID
    pub action: BridgeAction,                 // Lock or Unlock
    pub nft_data: NFTData,                    // NFT information
    pub sender: Address,                      // Sender address
    pub recipient: Bytes,                     // Recipient (supports multiple chain formats)
    pub source_chain: u32,                    // Source chain ID
    pub destination_chain: u32,               // Destination chain ID
    pub status: TransferStatus,               // Current status
    pub locked_timestamp: u64,                // When NFT was locked
    pub verified_timestamp: Option<u64>,      // When verified
    pub completed_timestamp: Option<u64>,     // When completed
    pub fee_amount: i128,                     // Bridge fee
    pub nonce: u64,                           // Replay protection
}
```

### WrappedNFTData
Represents wrapped NFT on destination chain:
```rust
pub struct WrappedNFTData {
    pub transfer_id: u64,                     // Link to original transfer
    pub original_contract: Address,           // Original NFT contract
    pub original_token_id: u64,               // Original token ID
    pub original_chain: u32,                  // Original chain ID
    pub wrapped_token_address: Address,       // Wrapped token contract
    pub wrapped_token_id: u64,                // Wrapped token ID
    pub current_owner: Address,               // Current owner
    pub wrapped_timestamp: u64,               // When wrapped
    pub metadata_uri: String,                 // Metadata URI
}
```

### Validator
Bridge validator information:
```rust
pub struct Validator {
    pub address: Address,                     // Validator address
    pub public_key: BytesN<32>,               // Public key for verification
    pub active: bool,                         // Active status
    pub added_timestamp: u64,                 // When added
}
```

## Public API

### Admin Functions

#### `initialize(admin, fee_token, fee_collector, native_chain_id, current_chain_id)`
Initializes the contract with admin, fees, and chain configuration.

#### `add_validator(validator_address, public_key)`
Adds a new validator to the bridge system.

#### `remove_validator(validator_address)`
Removes a validator from the bridge system.

#### `pause()`
Pauses all bridge operations (emergency control).

#### `unpause()`
Resumes all bridge operations.

#### `update_config(required_signatures, base_fee_bps, min_fee, max_fee)`
Updates bridge configuration parameters.

#### `collect_fees()`
Collects accumulated bridge fees.

### User Functions

#### `lock_nft(nft_contract, token_id, recipient, destination_chain, name, symbol, uri)`
Locks an NFT on the source chain for cross-chain transfer.
- **Returns:** Transfer ID
- **Emits:** NFT locked event (off-chain tracking)
- **Status:** Transitions to `Locked`

#### `verify_and_wrap(transfer_id, signatures, wrapped_token_address, wrapped_token_id)`
Verifies validator signatures and mints wrapped NFT on destination chain.
- **Requires:** Sufficient validator signatures
- **Status:** Transitions to `Wrapped`
- **Creates:** WrappedNFTData record

#### `unwrap_nft(transfer_id)`
Unwraps wrapped NFT and prepares it for return to source chain.
- **Requires:** NFT owner
- **Status:** Transitions to `Cancelled`
- **Effect:** Marks for unwrapping

#### `bridge_back_nft(transfer_id, signatures)`
Bridges wrapped NFT back to original chain with validator verification.
- **Requires:** Validator signatures
- **Status:** Transitions to `Completed`
- **Effect:** Unlocks original NFT on source chain

### Query Functions

#### `get_transfer(transfer_id)`
Returns bridge transfer details.

#### `get_wrapped_nft(transfer_id)`
Returns wrapped NFT information.

#### `get_validators()`
Returns list of active validators.

#### `get_config()`
Returns current bridge configuration.

#### `is_paused()`
Returns contract pause status.

## Transfer Lifecycle

```
1. LOCK PHASE (Source Chain)
   └─ NFT locked: lock_nft()
   └─ Status: Locked
   └─ Off-chain: Bridge validators monitor lock events

2. VERIFICATION PHASE (Destination Chain)
   └─ Validators sign proof
   └─ verify_and_wrap() called with signatures
   └─ Status: Wrapped
   └─ Wrapped NFT minted

3. BRIDGE BACK PHASE (Optional, Destination → Source)
   └─ Owner calls bridge_back_nft() or unwrap_nft()
   └─ Status: Completed/Cancelled
   └─ Original NFT unlocked on source

4. COMPLETION
   └─ Status: Completed or Cancelled
   └─ Transfer record maintained for audit
```

## Error Handling

| Error | Cause |
|-------|-------|
| `Unauthorized` | Caller not admin or not NFT owner |
| `InvalidTransfer` | Transfer ID invalid or status mismatch |
| `ContractPaused` | Bridge operations paused |
| `InvalidChainId` | Source and destination chains are same |
| `InsufficientSignatures` | Not enough validator signatures |
| `DuplicateSignature` | Same validator signed twice |
| `ValidatorNotFound` | Validator not registered or inactive |
| `InvalidMetadata` | Missing or invalid NFT metadata |
| `MaxValidatorsReached` | Cannot add more validators |

## Security Features

1. **Multi-Signature Verification**
   - Requires configurable number of validator signatures
   - Prevents single point of failure

2. **Replay Protection**
   - Nonce per transfer
   - Sequence-based signing

3. **Duplicate Signature Prevention**
   - Tracks unique validator signatures per transfer
   - Rejects duplicate signatures from same validator

4. **Emergency Pause**
   - Admin can immediately pause operations
   - Prevents malicious or exploited transfers

5. **Reentrancy Protection**
   - Status checks before state changes
   - Atomic operations

6. **Access Control**
   - Admin-only functions require authorization
   - NFT operations require ownership

## Configuration

Default configuration (customizable):
```rust
required_signatures: 2        // 2 of N validators required
max_validators: 10           // Maximum 10 validators
base_fee_bps: 50             // 0.5% base fee
min_fee: 100_000_000         // Minimum 0.1 tokens
max_fee: 10_000_000_000      // Maximum 10 tokens
```

## Testing

Run tests with:
```bash
cd contracts/nft_wrapper
cargo test
```

Test coverage includes:
- Contract initialization
- Validator management
- NFT locking and wrapping
- Signature verification
- Complete bridge flows
- Error conditions
- Emergency pause mechanism
- Configuration updates

## Deployment

### Build
```bash
cd contracts/nft_wrapper
cargo build --release
```

### Deploy to Testnet
Use Stellar testnet deployment tools with the generated WASM binary.

```bash
stellar contract deploy \
  --wasm build/nft_wrapper.wasm \
  --source <admin-keypair> \
  --network testnet
```

### Initialize
```bash
stellar contract invoke \
  --id <contract-id> \
  --source <admin-keypair> \
  --network testnet \
  -- initialize \
  --admin <admin-address> \
  --fee-collector <fee-collector-address> \
  --native-chain-id 1 \
  --current-chain-id 2
```

## Key Features Summary

✅ **Lock/Unlock Mechanism** - Secure NFT locking on source chain
✅ **Wrapped Token Standard** - Full metadata preservation
✅ **Bridge Validator System** - Multi-signature verification
✅ **Proof Verification Logic** - Cryptographic proof validation
✅ **Unwrap & Bridge Back** - Return NFTs to original chain
✅ **Bridge Flow Tests** - Comprehensive test coverage
✅ **Emergency Pause** - Safety mechanism for admin
✅ **Fee Collection** - Bridge operation fees

## Error Codes

- `1` - Unauthorized
- `2` - InvalidTransfer
- `3` - TransferNotFound
- `4` - AlreadyLocked
- `5` - NotLocked
- `6` - InvalidSignature
- `7` - InsufficientSignatures
- `8` - ValidatorNotFound
- `9` - ValidatorAlreadyExists
- `10` - MaxValidatorsReached
- `11` - ContractPaused
- `12` - InvalidChainId
- `13` - InvalidMetadata
- `14` - FeeCalculationError
- `15` - WrappingFailed
- `16` - UnwrappingFailed
- `17` - ProofVerificationFailed
- `18` - InvalidNFTData
- `19` - DuplicateSignature
- `20` - SignatureVerificationFailed

## Future Enhancements

1. **Ed25519 Signature Verification** - Full cryptographic verification implementation
2. **Event Logging** - Emit events for off-chain indexing
3. **Time-locked Unlocking** - Configurable unlock delays
4. **Batch Transfers** - Support multiple NFTs in single transaction
5. **Dynamic Fee Structures** - Fee adjustments based on network congestion
6. **Cross-chain Atomic Swaps** - NFT-to-token swaps across chains
7. **Governance** - DAO-based validator management

## License

Part of the Quest Contracts ecosystem.
