# Prediction Market - Quick Reference

## Contract Address
```
Testnet: [Deploy using ./scripts/deploy_prediction_market.sh testnet]
Mainnet: [TBD]
```

## Core Functions

| Function | Access | Description |
|----------|--------|-------------|
| `initialize(admin)` | Admin | Initialize contract |
| `create_market(creator, desc, outcomes, time)` | Anyone | Create new market |
| `place_bet(user, market_id, outcome, amount, token)` | Anyone | Place bet |
| `claim_winnings(user, market_id, token)` | Anyone | Claim winnings |
| `partial_cashout(user, market_id, bet_idx, token)` | Anyone | Exit early (10% fee) |
| `add_liquidity(provider, market_id, amount, token)` | Anyone | Add liquidity |
| `resolve_market(admin, market_id, outcome)` | Admin | Resolve market |
| `raise_dispute(user, market_id, reason)` | Anyone | Dispute outcome |
| `resolve_dispute(admin, market_id, new_outcome)` | Admin | Resolve dispute |
| `close_market(admin, market_id)` | Admin | Close to new bets |

## Query Functions

| Function | Returns |
|----------|---------|
| `get_market(market_id)` | Market details |
| `get_outcome_pools(market_id)` | Pool amounts per outcome |
| `get_user_bets(user, market_id)` | User's bets |

## Market Status Flow

```
Open → Closed → Resolved
  ↓              ↓
  └──────→ Disputed → Resolved
```

## Payout Formula

```
user_payout = (user_bet_amount × total_pool) / winning_pool_total
```

## Events

- `market_created` - New market
- `bet_placed` - New bet
- `liquidity_added` - Liquidity added
- `market_resolved` - Market resolved
- `winnings_claimed` - Winnings claimed
- `partial_cashout` - Early exit
- `dispute_raised` - Dispute opened
- `dispute_resolved` - Dispute closed

## Example: Create & Bet

```bash
# 1. Create market
MARKET_ID=$(soroban contract invoke \
  --id $CONTRACT_ID --source creator --network testnet \
  -- create_market \
  --creator $CREATOR \
  --description "Tournament Winner" \
  --outcomes '["Team A", "Team B"]' \
  --resolution_time 1735689600)

# 2. Place bet
soroban contract invoke \
  --id $CONTRACT_ID --source user --network testnet \
  -- place_bet \
  --user $USER \
  --market_id $MARKET_ID \
  --outcome_index 0 \
  --amount 1000000000 \
  --token $TOKEN

# 3. Resolve (admin)
soroban contract invoke \
  --id $CONTRACT_ID --source admin --network testnet \
  -- resolve_market \
  --admin $ADMIN \
  --market_id $MARKET_ID \
  --winning_outcome 0

# 4. Claim winnings
soroban contract invoke \
  --id $CONTRACT_ID --source user --network testnet \
  -- claim_winnings \
  --user $USER \
  --market_id $MARKET_ID \
  --token $TOKEN
```

## TypeScript Quick Start

```typescript
import { Contract } from '@stellar/stellar-sdk';

const contract = new Contract(contractId);

// Create market
const marketId = await contract.call('create_market',
  creator, 'Tournament Winner', ['A', 'B'], timestamp);

// Place bet
await contract.call('place_bet',
  user, marketId, 0, BigInt(100e7), tokenAddr);

// Get market
const market = await contract.call('get_market', marketId);

// Claim winnings
await contract.call('claim_winnings', user, marketId, tokenAddr);
```

## Security Notes

✅ All operations require authentication
✅ Input validation on all parameters
✅ Admin-only resolution & disputes
✅ Atomic token transfers
✅ Anti-gaming: 10% cashout fee

## Test Coverage

✅ 8/8 tests passing
- Market creation
- Bet placement & pooling
- Winner payouts
- Liquidity provision
- Partial cashout
- Dispute resolution
- Multiple bets
- Error handling

## Support

- 📖 Full docs: `README.md`
- 🔌 Integration: `INTEGRATION.md`
- 📊 Summary: `IMPLEMENTATION_SUMMARY.md`
- 🚀 Deploy: `scripts/deploy_prediction_market.sh`
