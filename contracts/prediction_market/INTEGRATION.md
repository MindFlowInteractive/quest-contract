# Prediction Market Integration Examples

## TypeScript/JavaScript Integration

### Setup
```typescript
import { Contract, SorobanRpc, TransactionBuilder, Networks } from '@stellar/stellar-sdk';

const contractId = 'YOUR_CONTRACT_ID';
const server = new SorobanRpc.Server('https://soroban-testnet.stellar.org');

const contract = new Contract(contractId);
```

### Create a Market
```typescript
async function createMarket(
  creator: string,
  description: string,
  outcomes: string[],
  resolutionTime: number
) {
  const tx = new TransactionBuilder(account, {
    fee: '1000',
    networkPassphrase: Networks.TESTNET,
  })
    .addOperation(
      contract.call(
        'create_market',
        creator,
        description,
        outcomes,
        resolutionTime
      )
    )
    .setTimeout(30)
    .build();

  const result = await server.sendTransaction(tx);
  return result.id; // Market ID
}

// Example usage
const marketId = await createMarket(
  'GCREATOR...',
  'Who will win the tournament?',
  ['Player A', 'Player B', 'Player C'],
  Math.floor(Date.now() / 1000) + 86400 // 24 hours from now
);
```

### Place a Bet
```typescript
async function placeBet(
  user: string,
  marketId: number,
  outcomeIndex: number,
  amount: bigint,
  tokenAddress: string
) {
  const tx = new TransactionBuilder(account, {
    fee: '1000',
    networkPassphrase: Networks.TESTNET,
  })
    .addOperation(
      contract.call(
        'place_bet',
        user,
        marketId,
        outcomeIndex,
        amount,
        tokenAddress
      )
    )
    .setTimeout(30)
    .build();

  return await server.sendTransaction(tx);
}

// Example usage
await placeBet(
  'GUSER...',
  1,
  0, // Betting on outcome 0
  BigInt(100_0000000), // 100 tokens (7 decimals)
  'GTOKEN...'
);
```

### Query Market Data
```typescript
async function getMarket(marketId: number) {
  const result = await contract.call('get_market', marketId);
  return {
    id: result.id,
    creator: result.creator,
    description: result.description,
    outcomes: result.outcomes,
    status: result.status,
    totalPool: result.total_pool,
    winningOutcome: result.winning_outcome,
  };
}

async function getOutcomePools(marketId: number) {
  const pools = await contract.call('get_outcome_pools', marketId);
  return pools.map((pool: any) => ({
    outcomeIndex: pool.outcome_index,
    totalAmount: pool.total_amount,
  }));
}

async function getUserBets(user: string, marketId: number) {
  const bets = await contract.call('get_user_bets', user, marketId);
  return bets.map((bet: any) => ({
    outcomeIndex: bet.outcome_index,
    amount: bet.amount,
    timestamp: bet.timestamp,
    claimed: bet.claimed,
  }));
}
```

### Calculate Odds
```typescript
async function calculateOdds(marketId: number): Promise<number[]> {
  const market = await getMarket(marketId);
  const pools = await getOutcomePools(marketId);
  
  return pools.map(pool => {
    if (pool.totalAmount === 0n) return 0;
    return Number(market.totalPool) / Number(pool.totalAmount);
  });
}

// Example: Display odds
const odds = await calculateOdds(1);
console.log('Current odds:', odds.map(o => `${o.toFixed(2)}x`));
```

### Listen to Events
```typescript
async function watchMarketEvents(marketId: number) {
  const events = await server.getEvents({
    startLedger: 'latest',
    filters: [
      {
        type: 'contract',
        contractIds: [contractId],
      },
    ],
  });

  for (const event of events.events) {
    const topic = event.topic[0];
    
    switch (topic) {
      case 'bet_placed':
        console.log('New bet:', event.value);
        break;
      case 'market_resolved':
        console.log('Market resolved:', event.value);
        break;
      case 'winnings_claimed':
        console.log('Winnings claimed:', event.value);
        break;
    }
  }
}
```

## React Component Example

```tsx
import { useState, useEffect } from 'react';

interface Market {
  id: number;
  description: string;
  outcomes: string[];
  totalPool: bigint;
  status: string;
}

function PredictionMarket({ marketId }: { marketId: number }) {
  const [market, setMarket] = useState<Market | null>(null);
  const [odds, setOdds] = useState<number[]>([]);
  const [selectedOutcome, setSelectedOutcome] = useState(0);
  const [betAmount, setBetAmount] = useState('');

  useEffect(() => {
    loadMarketData();
  }, [marketId]);

  async function loadMarketData() {
    const marketData = await getMarket(marketId);
    const oddsData = await calculateOdds(marketId);
    setMarket(marketData);
    setOdds(oddsData);
  }

  async function handlePlaceBet() {
    await placeBet(
      userAddress,
      marketId,
      selectedOutcome,
      BigInt(parseFloat(betAmount) * 1e7),
      tokenAddress
    );
    await loadMarketData();
  }

  if (!market) return <div>Loading...</div>;

  return (
    <div className="prediction-market">
      <h2>{market.description}</h2>
      <p>Total Pool: {Number(market.totalPool) / 1e7} tokens</p>
      
      <div className="outcomes">
        {market.outcomes.map((outcome, index) => (
          <div key={index} className="outcome">
            <input
              type="radio"
              checked={selectedOutcome === index}
              onChange={() => setSelectedOutcome(index)}
            />
            <label>{outcome}</label>
            <span className="odds">{odds[index]?.toFixed(2)}x</span>
          </div>
        ))}
      </div>

      <div className="bet-form">
        <input
          type="number"
          value={betAmount}
          onChange={(e) => setBetAmount(e.target.value)}
          placeholder="Bet amount"
        />
        <button onClick={handlePlaceBet}>Place Bet</button>
      </div>
    </div>
  );
}
```

## Backend Integration (Node.js)

```typescript
import express from 'express';
import { Contract, SorobanRpc } from '@stellar/stellar-sdk';

const app = express();
const contract = new Contract(process.env.CONTRACT_ID!);
const server = new SorobanRpc.Server(process.env.RPC_URL!);

// Get all active markets
app.get('/api/markets', async (req, res) => {
  const marketCount = await contract.call('get_market_counter');
  const markets = [];
  
  for (let i = 1; i <= marketCount; i++) {
    const market = await getMarket(i);
    if (market.status === 'Open') {
      markets.push(market);
    }
  }
  
  res.json(markets);
});

// Get market details with odds
app.get('/api/markets/:id', async (req, res) => {
  const marketId = parseInt(req.params.id);
  const market = await getMarket(marketId);
  const pools = await getOutcomePools(marketId);
  const odds = await calculateOdds(marketId);
  
  res.json({
    ...market,
    pools,
    odds,
  });
});

// Get user's bets
app.get('/api/users/:address/bets', async (req, res) => {
  const { address } = req.params;
  const { marketId } = req.query;
  
  const bets = await getUserBets(address, parseInt(marketId as string));
  res.json(bets);
});

// Webhook for market resolution
app.post('/api/markets/:id/resolve', async (req, res) => {
  const { id } = req.params;
  const { winningOutcome } = req.body;
  
  // Verify admin authorization
  if (!isAdmin(req.headers.authorization)) {
    return res.status(403).json({ error: 'Unauthorized' });
  }
  
  await resolveMarket(adminAddress, parseInt(id), winningOutcome);
  res.json({ success: true });
});

app.listen(3000, () => {
  console.log('Prediction market API running on port 3000');
});
```

## CLI Usage Examples

### Create Market
```bash
soroban contract invoke \
  --id CCONTRACT... \
  --source creator \
  --network testnet \
  -- create_market \
  --creator GCREATOR... \
  --description "Tournament Winner" \
  --outcomes '["Team A", "Team B", "Team C"]' \
  --resolution_time 1735689600
```

### Place Bet
```bash
soroban contract invoke \
  --id CCONTRACT... \
  --source user \
  --network testnet \
  -- place_bet \
  --user GUSER... \
  --market_id 1 \
  --outcome_index 0 \
  --amount 1000000000 \
  --token GTOKEN...
```

### Query Market
```bash
soroban contract invoke \
  --id CCONTRACT... \
  --network testnet \
  -- get_market \
  --market_id 1
```

### Resolve Market (Admin)
```bash
soroban contract invoke \
  --id CCONTRACT... \
  --source admin \
  --network testnet \
  -- resolve_market \
  --admin GADMIN... \
  --market_id 1 \
  --winning_outcome 0
```

### Claim Winnings
```bash
soroban contract invoke \
  --id CCONTRACT... \
  --source user \
  --network testnet \
  -- claim_winnings \
  --user GUSER... \
  --market_id 1 \
  --token GTOKEN...
```
