# Dynamic NFT Evolution Contract

A Soroban smart contract implementing dynamic NFTs that evolve and change properties based on player achievements, time, and milestones.

## Overview

This contract provides a complete system for creating and managing NFTs that change properties over time through:
- **Milestone-Triggered Upgrades**: Admin/verifier-controlled evolution
- **Time-Based Evolution**: Automatic rarity upgrades after elapsed time
- **Rarity Evolution**: NFTs gain rarity rank through upgrades and fusion
- **Fusion Mechanics**: Combine two NFTs into a higher-rarity token
- **Downgrade & Reversal**: Remove evolution effects and reduce levels
- **Evolution History**: Track all evolution events immutably

## Features

### Core Mechanics

#### 1. Minting
Create a new dynamic NFT with initial properties:
- Owner and metadata
- Starting level: 1
- Starting rarity: 1
- Empty evolution history

```rust
pub fn mint(env: Env, minter: Address, owner: Address, metadata: String, traits: String) -> u32
```

#### 2. Milestone Evolution (Admin/Verifier Only)
Manually trigger NFT evolution:
- Increase level by a specified amount
- Increase rarity (rank) by a specified amount
- Optionally update visual traits
- Records evolution event in history

```rust
pub fn evolve_milestone(
    env: Env, 
    submitter: Address,         // must be admin or verifier
    token_id: u32, 
    level_inc: u32, 
    rarity_inc: u32, 
    new_traits: Option<String>
)
```

#### 3. Time-Based Evolution (Player-Triggered)
Automatically advance NFT after elapsed time:
- Requires `required_secs` to have passed since last evolution
- Increases level by 1
- Resets the internal timer for next evolution
- Records in history

```rust
pub fn evolve_time(
    env: Env, 
    caller: Address, 
    token_id: u32, 
    required_secs: u64
)
```

#### 4. Downgrade (Admin/Verifier Only)
Reduce NFT level (penalty or correction):
- Decreases level by specified amount
- Rarity remains unchanged
- Records downgrade event

```rust
pub fn downgrade(
    env: Env, 
    submitter: Address,     // must be admin or verifier
    token_id: u32, 
    level_dec: u32
)
```

#### 5. Fusion
Combine two NFTs into a higher-rarity token:
- Both NFTs must have same owner
- Only owner can trigger fusion
- New NFT receives:
  - Combined level (sum of both)
  - Highest rarity + 1
  - New token ID
- Original NFTs are burned (removed from storage)
- Records fusion event

```rust
pub fn fuse(
    env: Env, 
    submitter: Address,     // must be owner of both NFTs
    token_a: u32, 
    token_b: u32
) -> u32  // returns new token_id
```

### Access Control

#### Admin Functions
- `initialize(env, admin)`: Set up contract (once)
- `add_verifier(env, admin, verifier)`: Grant evolution privileges
- `remove_verifier(env, admin, verifier)`: Revoke evolution privileges

#### Protected Endpoints
- `evolve_milestone`: Admin or verifier only
- `downgrade`: Admin or verifier only
- `fuse`: NFT owner only

#### Open Endpoints
- `mint`: Any minter address
- `evolve_time`: Any caller (player-initiated)
- `get_nft`: Any caller
- `get_history`: Any caller

## Data Structures

### DynamicNft
```rust
pub struct DynamicNft {
    pub owner: Address,           // Owner of the NFT
    pub level: u32,               // Current evolved level (starts at 1)
    pub rarity: u32,              // Rarity rank (starts at 1)
    pub traits: String,           // Visual/gameplay traits (updatable)
    pub metadata: String,         // JSON/IPFS URI or descriptive metadata
    pub history: Vec<String>,     // Evolution event log
    pub minted_at: u64,           // Timestamp for time-based evolution
}
```

### Events
All events are typed using `#[contractevent]`:

- **MintEvent**: Emitted when an NFT is minted
- **EvolveMilestoneEvent**: Emitted on milestone evolution
- **EvolveTimeEvent**: Emitted on time-based evolution
- **DowngradeEvent**: Emitted on level downgrade
- **FuseEvent**: Emitted when NFTs are fused into a new token

## Usage Examples

### Setup
```rust
// Initialize the contract
let admin = Address::generate(&env);
client.initialize(&admin);

// Add a verifier (e.g., achievement oracle)
let verifier = Address::generate(&env);
client.add_verifier(&admin, &verifier);
```

### Minting
```rust
let owner = Address::generate(&env);
let token_id = client.mint(
    &minter,
    &owner,
    &String::from_str(&env, "ipfs://..."),
    &String::from_str(&env, "fire_dragon")
);
```

### Milestone Evolution (Achievement Unlock)
```rust
// Player unlocked achievement 5; verifier evolves NFT
client.evolve_milestone(
    &verifier,      // must be admin or verifier
    &token_id,      // which NFT
    &2u32,          // level +2
    &1u32,          // rarity +1
    &None           // keep existing traits
);
```

### Time-Based Evolution
```rust
// Player's NFT evolved enough; they trigger manual evolution
client.evolve_time(
    &owner,         // player calling
    &token_id,
    &86400u64       // requires 1 day (86400 seconds)
);
```

### Fusion
```rust
// Combine two fire dragons into a legendary
let fused_id = client.fuse(
    &owner,         // must own both
    &token_a,
    &token_b
);
// token_a and token_b are now burned; fused_id is the new super NFT
```

### View State
```rust
// Get full NFT details
let nft = client.get_nft(&token_id).unwrap();

// Get evolution history
let history = client.get_history(&token_id).unwrap();
for event in history.iter() {
    println!("Evolution: {}", event);
}
```

## Acceptance Criteria Met

✅ **NFT properties change based on conditions**: Level and rarity update via evolution calls  
✅ **Milestones trigger upgrades**: `evolve_milestone` with admin/verifier control  
✅ **Time-based evolution works**: `evolve_time` checks elapsed time  
✅ **Evolution history preserved**: Vec<String> tracks all events  
✅ **Fusion creates new NFT**: `fuse` burns originals, creates higher-rank token  
✅ **Contract deployable**: Standard Soroban package structure with tests  

## Testing

Run the full test suite:
```bash
cargo test --manifest-path contracts/dynamic_nft/Cargo.toml
```

Tests cover:
- Mint and retrieval
- Time-based evolution
- Milestone evolution (admin/verifier)
- Downgrade mechanics
- Fusion and token burn
- Authorization checks (panics on unauthorized access)
- Verifier add/remove

## Future Enhancements

- JSON metadata schema with visual trait definitions
- IPFS integration for off-chain metadata
- Configurable rarity tiers and evolution curves
- Batch operations for admin/verifier
- Event indexing/listening support
- Cross-contract evolution triggers (oracle integration)
- Breeding mechanics (different from fusion)

## License

This contract is part of the quest-contract project.
