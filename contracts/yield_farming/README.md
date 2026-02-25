# Yield Farming Contract

A DeFi yield farming contract for Quest Service that allows players to stake tokens or NFTs to earn passive rewards over time.

## Features

### Core Functionality
- **Token Staking**: Stake fungible tokens to earn yield
- **NFT Staking**: Stake NFTs with enhanced reward multipliers
- **APY-Based Rewards**: Configurable annual percentage yield per pool
- **Multiple Pools**: Support for different asset types and reward structures

### Advanced Features
- **Lock-up Periods**: Configurable lock periods (in days) per pool
- **Early Withdrawal Penalties**: Percentage-based penalties for unstaking before unlock time
- **Pool Multipliers**: Bonus multipliers to boost rewards (e.g., 1.5x, 2x)
- **Auto-Compounding**: Optional automatic reinvestment of rewards into principal
- **Reward Distribution**: Periodic claim mechanism with accurate time-based calculations

### Security & Management
- **Admin Controls**: Only admin can create new pools
- **Anti-Gaming**: Lock periods prevent reward manipulation
- **Accurate Accounting**: Precise reward calculations using basis points
- **Pool Statistics**: Track total staked, stakers, and rewards distributed

## Data Structures

### PoolConfig
```rust
pub struct PoolConfig {
    pub asset_address: Address,           // Token/NFT contract address
    pub asset_type: AssetType,            // Token or NFT
    pub apy_basis_points: u32,            // 1000 = 10% APY
    pub lock_period_days: u32,            // Lock duration in days
    pub early_withdrawal_penalty_bp: u32, // 500 = 5% penalty
    pub multiplier_bp: u32,               // 10000 = 1x, 15000 = 1.5x
    pub auto_compound: bool,              // Auto-reinvest rewards
}
```

### StakePosition
```rust
pub struct StakePosition {
    pub staker: Address,
    pub pool_id: u32,
    pub amount: i128,
    pub nft_id: Option<u32>,
    pub stake_time: u64,
    pub last_claim_time: u64,
    pub unlock_time: u64,
    pub accumulated_rewards: i128,
}
```

## Functions

### Admin Functions

#### `initialize(admin: Address, reward_token: Address)`
Initialize the contract with admin and reward token.

#### `create_pool(...) -> u32`
Create a new staking pool with specified parameters.

### User Functions

#### `stake_tokens(staker: Address, pool_id: u32, amount: i128)`
Stake tokens into a pool.

#### `stake_nft(staker: Address, pool_id: u32, nft_id: u32)`
Stake an NFT into a pool.

#### `calculate_rewards(staker: Address, stake_id: u32) -> i128`
Calculate pending rewards for a stake position.

#### `claim_rewards(staker: Address, stake_id: u32) -> i128`
Claim accumulated rewards (or auto-compound if enabled).

#### `unstake(staker: Address, stake_id: u32) -> i128`
Unstake and withdraw assets (with penalty if before unlock time).

### View Functions

#### `get_pool(pool_id: u32) -> PoolConfig`
Get pool configuration.

#### `get_pool_stats(pool_id: u32) -> PoolStats`
Get pool statistics.

#### `get_stake(staker: Address, stake_id: u32) -> StakePosition`
Get stake position details.

#### `get_user_stakes(staker: Address) -> Vec<u32>`
Get all stake IDs for a user.

## Reward Calculation

Rewards are calculated using the formula:

```
base_reward = (amount × APY_bp × time_elapsed) / (10000 × seconds_per_year)
final_reward = (base_reward × multiplier_bp) / 10000
```

Where:
- `APY_bp`: Annual percentage yield in basis points (1000 = 10%)
- `time_elapsed`: Seconds since last claim
- `multiplier_bp`: Pool multiplier in basis points (10000 = 1x)

## Usage Examples

### Create a Token Pool
```rust
let pool_id = client.create_pool(
    &token_address,
    &AssetType::Token,
    &1000,   // 10% APY
    &30,     // 30 days lock
    &500,    // 5% early withdrawal penalty
    &10000,  // 1x multiplier
    &false,  // No auto-compound
);
```

### Create an NFT Pool with Bonus
```rust
let pool_id = client.create_pool(
    &nft_address,
    &AssetType::NFT,
    &2000,   // 20% APY
    &60,     // 60 days lock
    &1000,   // 10% penalty
    &20000,  // 2x multiplier
    &true,   // Auto-compound enabled
);
```

### Stake and Earn
```rust
// Stake tokens
client.stake_tokens(&user, &pool_id, &10000);

// Wait some time...

// Check rewards
let rewards = client.calculate_rewards(&user, &stake_id);

// Claim rewards
let claimed = client.claim_rewards(&user, &stake_id);

// Unstake after lock period
let returned = client.unstake(&user, &stake_id);
```

## Testing

Run the comprehensive test suite:

```bash
cargo test --package yield_farming
```

Tests cover:
- Pool creation and configuration
- Token and NFT staking
- Reward calculation accuracy
- Claim and auto-compounding
- Lock period enforcement
- Early withdrawal penalties
- Multiplier bonuses
- Pool statistics tracking

## Deployment

### Build
```bash
soroban contract build
```

### Deploy to Testnet
```bash
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/yield_farming.wasm \
  --source deployer \
  --network testnet
```

### Initialize
```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source deployer \
  --network testnet \
  -- initialize \
  --admin <ADMIN_ADDRESS> \
  --reward_token <REWARD_TOKEN_ADDRESS>
```

## Security Considerations

1. **Lock Periods**: Prevent flash loan attacks and reward gaming
2. **Penalties**: Discourage early withdrawals and maintain pool stability
3. **Accurate Math**: Use basis points (10000 = 100%) for precise calculations
4. **Admin Controls**: Only admin can create pools to prevent malicious configurations
5. **Time-Based Rewards**: Rewards calculated based on actual time elapsed

## Integration with Quest Service

This contract integrates with:
- **Reward Token Contract**: For distributing yield rewards
- **Achievement NFT Contract**: For NFT staking support
- **Leaderboard Contract**: Track top yield farmers
- **Guild Contract**: Guild-based farming pools

## License

MIT License
