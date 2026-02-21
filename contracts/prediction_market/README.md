# Prediction Market Contract

A decentralized prediction market smart contract for Quest Service that allows players to bet on puzzle completion outcomes, tournament winners, and game events.

## Features

### Core Functionality
- **Market Creation**: Create prediction markets with multiple outcomes
- **Bet Placement**: Place bets on specific outcomes with token pooling
- **Outcome Resolution**: Admin-controlled resolution mechanism
- **Winner Payouts**: Proportional distribution based on pool shares
- **Liquidity Provision**: Market makers can add liquidity for incentives
- **Partial Cashout**: Exit positions early with a 10% fee
- **Dispute Resolution**: Challenge and review market outcomes

### Key Components

#### Market Structure
```rust
pub struct Market {
    pub id: u64,
    pub creator: Address,
    pub description: String,
    pub outcomes: Vec<String>,
    pub status: MarketStatus,
    pub resolution_time: u64,
    pub winning_outcome: Option<u32>,
    pub total_pool: i128,
    pub liquidity_provider: Option<Address>,
    pub liquidity_amount: i128,
}
```

#### Market States
- **Open**: Accepting bets
- **Closed**: No new bets, awaiting resolution
- **Resolved**: Outcome determined, winners can claim
- **Disputed**: Under review for potential resolution change

## Contract Methods

### Initialization
```rust
pub fn initialize(env: Env, admin: Address)
```
Initialize the contract with an admin address.

### Market Management
```rust
pub fn create_market(
    env: Env,
    creator: Address,
    description: String,
    outcomes: Vec<String>,
    resolution_time: u64,
) -> u64
```
Create a new prediction market. Requires at least 2 outcomes.

```rust
pub fn close_market(env: Env, admin: Address, market_id: u64)
```
Close a market to new bets (admin only).

```rust
pub fn resolve_market(
    env: Env,
    admin: Address,
    market_id: u64,
    winning_outcome: u32
)
```
Resolve a market by declaring the winning outcome (admin only).

### Betting
```rust
pub fn place_bet(
    env: Env,
    user: Address,
    market_id: u64,
    outcome_index: u32,
    amount: i128,
    token: Address,
)
```
Place a bet on a specific outcome. Transfers tokens to the contract.

```rust
pub fn partial_cashout(
    env: Env,
    user: Address,
    market_id: u64,
    bet_index: u32,
    token: Address,
) -> i128
```
Cash out a bet early for 90% of the original amount (10% fee).

### Claiming Winnings
```rust
pub fn claim_winnings(
    env: Env,
    user: Address,
    market_id: u64,
    token: Address,
) -> i128
```
Claim winnings after market resolution. Payout is proportional to bet share in winning pool.

**Payout Formula:**
```
user_payout = (user_bet_amount * total_pool) / winning_pool_total
```

### Liquidity
```rust
pub fn add_liquidity(
    env: Env,
    provider: Address,
    market_id: u64,
    amount: i128,
    token: Address,
)
```
Add liquidity to a market to incentivize participation.

### Disputes
```rust
pub fn raise_dispute(
    env: Env,
    user: Address,
    market_id: u64,
    reason: String,
)
```
Raise a dispute on a resolved market.

```rust
pub fn resolve_dispute(
    env: Env,
    admin: Address,
    market_id: u64,
    new_outcome: Option<u32>,
)
```
Resolve a dispute, optionally changing the winning outcome (admin only).

### Query Methods
```rust
pub fn get_market(env: Env, market_id: u64) -> Market
pub fn get_outcome_pools(env: Env, market_id: u64) -> Vec<OutcomePool>
pub fn get_user_bets(env: Env, user: Address, market_id: u64) -> Vec<Bet>
```

## Usage Examples

### Creating a Market
```rust
let outcomes = vec![
    String::from_str(&env, "Player A wins"),
    String::from_str(&env, "Player B wins"),
    String::from_str(&env, "Draw"),
];

let market_id = client.create_market(
    &creator,
    &String::from_str(&env, "Tournament Final"),
    &outcomes,
    &1735689600, // Unix timestamp
);
```

### Placing Bets
```rust
// User bets 100 tokens on outcome 0
client.place_bet(
    &user,
    &market_id,
    &0,
    &100,
    &token_address,
);
```

### Resolving and Claiming
```rust
// Admin resolves market
client.resolve_market(&admin, &market_id, &0);

// Winner claims payout
let payout = client.claim_winnings(&user, &market_id, &token_address);
```

## Events

The contract emits events for all major actions:
- `market_created`: When a new market is created
- `bet_placed`: When a bet is placed
- `liquidity_added`: When liquidity is added
- `market_resolved`: When a market is resolved
- `winnings_claimed`: When winnings are claimed
- `partial_cashout`: When a user cashes out early
- `dispute_raised`: When a dispute is raised
- `dispute_resolved`: When a dispute is resolved

## Testing

Run the comprehensive test suite:
```bash
cargo test --package prediction-market
```

### Test Coverage
- ✅ Market creation with multiple outcomes
- ✅ Bet placement and pooling
- ✅ Winner payout distribution
- ✅ Liquidity provision
- ✅ Partial cashout mechanism
- ✅ Dispute raising and resolution
- ✅ Multiple bets from same user
- ✅ Error handling (insufficient outcomes, premature claims)

## Security Considerations

1. **Authorization**: All state-changing operations require proper authentication
2. **Validation**: Input validation on all parameters (amounts, indices, timestamps)
3. **Atomicity**: Token transfers and state updates are atomic
4. **Admin Controls**: Critical operations (resolution, disputes) restricted to admin
5. **Anti-Gaming**: Partial cashout fee prevents manipulation

## Integration with Quest Service

This contract integrates with:
- **Tournament Contract**: Bet on tournament outcomes
- **Puzzle Verification**: Predict puzzle completion times
- **Leaderboard**: Bet on ranking changes
- **Reward Token**: Use quest tokens for betting

## Deployment

### Build
```bash
cargo build --package prediction-market --target wasm32-unknown-unknown --release
```

### Deploy to Testnet
```bash
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/prediction_market.wasm \
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
  --admin <ADMIN_ADDRESS>
```

## Future Enhancements

- Automated market makers (AMM) for dynamic odds
- Time-weighted betting (earlier bets get bonuses)
- Multi-outcome partial payouts
- Oracle integration for automated resolution
- Market creation fees and revenue sharing
- Reputation-based dispute resolution

## License

MIT License
