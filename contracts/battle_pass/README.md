# Battle Pass Contract

A comprehensive Soroban smart contract implementing a battle pass system with free and premium tiers, progressive rewards, and seasonal gameplay mechanics.

## Features

### Core Battle Pass System
- **Free & Premium Tiers**: Two-tier system with different reward sets
- **Seasonal Structure**: Time-limited seasons with expiration
- **Progressive Leveling**: 100 levels per season with XP-based progression
- **Reward System**: Progressive reward scaling based on level achievements

### Advanced Mechanics

#### Experience & Leveling
- XP tracking per player per season
- 1,000 XP required per level (configurable)
- Maximum 100 levels per season
- Bonus XP events for special promotions

#### Reward Unlocking
- First 50 levels available to free tier
- Levels 51-100 exclusive to premium tier
- Progressive reward scaling (higher levels = better rewards)
- Retroactive claiming for premium upgrades

#### Season Management
- 30-day seasons (configurable via SEASON_DURATION)
- Season expiration enforcement
- Season activation/deactivation
- Season history tracking

#### Pass Transfers
- Gift battle passes to other players
- Preserves XP and level progress
- Prevents ownership conflicts

#### Bonus XP Events
- Admin-controlled XP multipliers (1-10x)
- Event activation/deactivation
- Applied retroactively to all XP earned during event

### Data Structures

```rust
BattlePass {
    owner: Address,
    season: u32,
    tier: PassTier,
    current_level: u32,
    is_active: bool,
    purchase_time: u64,
}

SeasonInfo {
    season_number: u32,
    start_time: u64,
    end_time: u64,
    is_active: bool,
    total_players: u32,
    reward_pool: u128,
}

SeasonRecord {
    season: u32,
    final_level: u32,
    total_xp: u32,
    rewards_claimed: bool,
    tier: PassTier,
}
```

## API Reference

### Season Management

#### `init_season(season_number: u32, reward_pool: u128)`
Initialize a new season with reward pool.

```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  -- init_season \
  --season-number 1 \
  --reward-pool 50000000
```

#### `get_current_season() -> u32`
Get the currently active season number.

### Battle Pass Operations

#### `purchase_pass(player: Address, tier: PassTier)`
Purchase a new battle pass (free or premium).

```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  -- purchase_pass \
  --player <PLAYER_ADDRESS> \
  --tier Free  # or Premium
```

#### `get_player_pass(player: Address) -> Option<BattlePass>`
Retrieve player's current battle pass.

#### `transfer_pass(from: Address, to: Address)`
Gift battle pass to another player.

```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  -- transfer_pass \
  --from <SOURCE_ADDRESS> \
  --to <RECIPIENT_ADDRESS>
```

### Experience & Leveling

#### `add_xp(player: Address, amount: u32)`
Award XP to a player (automatically applies bonus events).

```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  -- add_xp \
  --player <PLAYER_ADDRESS> \
  --amount 500
```

#### `get_player_xp(player: Address) -> u32`
Get player's total XP for current season.

### Rewards

#### `claim_reward(player: Address, level: u32) -> u128`
Claim reward for reaching a specific level.

```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  -- claim_reward \
  --player <PLAYER_ADDRESS> \
  --level 10
```

#### `claim_retroactive_rewards(player: Address) -> u128`
Claim all unclaimed rewards (premium upgrade scenario).

```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  -- claim_retroactive_rewards \
  --player <PLAYER_ADDRESS>
```

### Events & Promotions

#### `set_bonus_xp_event(multiplier: u32)`
Start a bonus XP event (1-10x multiplier).

```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  -- set_bonus_xp_event \
  --multiplier 2
```

#### `clear_bonus_xp_event()`
End the current bonus XP event.

#### `get_bonus_xp_multiplier() -> u32`
Get current XP multiplier.

### Season History

#### `archive_season(player: Address)`
Archive current season to player's history.

```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  -- archive_season \
  --player <PLAYER_ADDRESS>
```

#### `get_season_history(player: Address) -> Vec<SeasonRecord>`
Retrieve all archived seasons for a player.

#### `deactivate_season(season: u32)`
Deactivate a season (prevents new actions).

## Reward Formula

Rewards scale progressively with level and tier:

```
reward = (100 * level * (level / 10 + 1)) / 10
```

Examples:
- Level 5: 275 tokens
- Level 10: 1,100 tokens
- Level 20: 4,200 tokens
- Level 50: 27,500 tokens
- Level 100: 110,000 tokens

## Deployment

### Prerequisites
- Soroban CLI installed
- Connected to testnet
- Account with funds

### Build Contract
```bash
cd contracts/battle_pass
cargo build --target wasm32-unknown-unknown --release
```

### Deploy to Testnet
```bash
# Set testnet config
soroban config network add \
  --rpc-url https://soroban-testnet.stellar.org \
  --network-passphrase "Test SDF Network ; September 2015" \
  testnet

# Deploy contract
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/battle_pass.wasm \
  --source <YOUR_SOURCE_ACCOUNT> \
  --network testnet
```

### Post-Deployment Setup
```bash
# Initialize season 1
soroban contract invoke \
  --id <DEPLOYED_CONTRACT_ID> \
  --source <ADMIN_ACCOUNT> \
  --network testnet \
  -- init_season \
  --season-number 1 \
  --reward-pool 1000000000

# Verify deployment
soroban contract invoke \
  --id <DEPLOYED_CONTRACT_ID> \
  --network testnet \
  -- get_current_season
```

## Testing

### Run Tests
```bash
cd contracts/battle_pass
cargo test
```

### Test Coverage
- Pass purchase (free/premium)
- Duplicate purchase prevention
- XP addition and leveling
- Bonus XP events
- Reward claiming
- Reward restrictions (premium-only)
- Retroactive claiming
- Pass transfers
- Season expiration
- Season history
- Multiple seasons
- Progressive reward scaling
- Max level cap

## Constants

```rust
const SEASON_DURATION: u64 = 2_592_000;        // 30 days
const LEVELS_PER_SEASON: u32 = 100;            // Max level
const FREE_TIER_REWARDS: u32 = 50;             // Free tier max
const PREMIUM_TIER_REWARDS: u32 = 100;         // Premium tier max
const XP_PER_LEVEL: u32 = 1000;                // XP requirement
```

## Configuration

To modify core parameters:
1. Edit constants at top of `src/lib.rs`
2. Rebuild: `cargo build --target wasm32-unknown-unknown --release`
3. Redeploy contract

## Security Considerations

- All player actions require authentication (`require_auth()`)
- Season expiration enforced to prevent stale season interactions
- Level requirements enforced before reward claims
- Premium tier verification prevents free tier from claiming premium rewards
- Duplicate rewards prevented via claim tracking
- Season deactivation prevents further interactions

## Future Enhancements

- Daily streak bonuses
- Achievement milestones
- Custom reward tokens
- Crafting system integration
- Guild-based bonuses
- Dynamic difficulty scaling
- Seasonal theme customization
- Cross-season progression

## License

Part of the Quest Contract ecosystem.
