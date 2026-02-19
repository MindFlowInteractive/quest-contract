# Achievement Sets Contract

Achievement composability and set bonuses: owning multiple related achievement NFTs grants bonus rewards and unlocks special content.

## Features

- **Set structure**: Define sets with multiple required achievement (puzzle) IDs
- **Auto completion detection**: Progress synced from achievement NFT ownership
- **Bonus distribution**: Reward token minted to players on set claim
- **Rarity tiers**: Common, Rare, Epic, Legendary, Mythic (with tier bonuses)
- **Limited editions**: Cap total claims per set; mint tradable edition tokens
- **Cross-set synergies**: Bonus for completing multiple sets
- **Leaderboards**: Per-set and global
- **Set trading**: Transfer limited edition tokens between players

## Build

```bash
soroban contract build
# WASM: target/wasm32v1-none/release/achievement_sets.wasm
```

## Test

```bash
cargo test -p achievement-sets
```

## Deploy to Testnet

1. Deploy dependencies first:
   - `achievement_nft` – achievement NFTs
   - `reward_token` – reward token

2. Deploy achievement_sets:
   ```bash
   soroban contract deploy \
     --wasm target/wasm32v1-none/release/achievement_sets.wasm \
     --source deployer \
     --network testnet
   ```

3. Initialize:
   ```bash
   soroban contract invoke \
     --id ACHIEVEMENT_SETS_ID \
     --source admin \
     --network testnet \
     -- \
     initialize \
     --admin ADMIN_ADDRESS \
     --achievement_nft NFT_CONTRACT_ID \
     --reward_token REWARD_TOKEN_ID \
     --max_top_entries 100
   ```

4. Authorize achievement_sets as minter on reward_token:
   ```bash
   soroban contract invoke \
     --id REWARD_TOKEN_ID \
     --source admin \
     --network testnet \
     -- \
     authorize_minter \
     --minter ACHIEVEMENT_SETS_ID
   ```

## Main Functions

| Function | Description |
|----------|-------------|
| `create_set` | Admin: create achievement set |
| `create_synergy` | Admin: create cross-set synergy |
| `sync_player_set` | Sync progress from NFTs |
| `claim_set_bonus` | Claim set completion bonus |
| `claim_synergy_bonus` | Claim synergy bonus |
| `transfer_edition_token` | Trade limited edition token |
| `progress` | View player progress for a set |
| `get_set_leaderboard` | Per-set leaderboard |
| `get_global_leaderboard` | Global leaderboard |

## Acceptance Criteria

- [x] Sets defined with multiple achievements
- [x] Completion detected automatically from NFT ownership
- [x] Bonuses distributed correctly via reward token
- [x] Progress tracked per player
- [x] Limited editions enforced
- [x] Contract deployable to testnet
