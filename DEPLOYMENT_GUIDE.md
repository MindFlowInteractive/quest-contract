# Puzzle Factory Contract Deployment Guide

## Contract Built Successfully ✅

The puzzle factory contract has been built and is ready for deployment:

- **WASM File**: `target/wasm32v1-none/release/puzzle_factory.wasm`
- **Size**: 18,551 bytes
- **Hash**: `4ea640f52d13350400b81d11c9e29977b9de853c19f679fd7f6f036c4f840d3e`

## Available Keys
- `puzzle_deployer` (for testnet)
- `puzzle_deployer_futurenet` (for futurenet)

## Deployment Commands

### Option 1: Testnet Deployment
```bash
stellar contract deploy \
  --wasm target/wasm32v1-none/release/puzzle_factory.wasm \
  --source puzzle_deployer \
  --network testnet
```

### Option 2: Futurenet Deployment
```bash
stellar contract deploy \
  --wasm target/wasm32v1-none/release/puzzle_factory.wasm \
  --source puzzle_deployer_futurenet \
  --network futurenet
```

### Option 3: Using Makefile
```bash
make deploy-testnet
```

## Post-Deployment Setup

After deployment, you'll need to initialize the contract:

```bash
# Replace CONTRACT_ID with the deployed contract address
stellar contract invoke \
  --id CONTRACT_ID \
  --source puzzle_deployer \
  --network testnet \
  -- initialize \
  --admin ADMIN_ADDRESS
```

## Contract Functions

The deployed contract includes these main functions:
- `initialize(admin)` - Initialize contract with admin
- `authorize_creator(creator)` - Authorize a puzzle creator
- `create_puzzle(...)` - Create a new puzzle
- `deprecate_puzzle(puzzle_id)` - Deprecate a puzzle (with full cleanup)
- `get_puzzle(puzzle_id)` - Get puzzle details
- `get_puzzles_by_category(category)` - Filter by category
- `get_puzzles_by_creator(creator)` - Filter by creator
- `get_puzzles_by_difficulty(difficulty)` - Filter by difficulty
- `get_active_puzzles()` - Get all active puzzles
- `get_creator_stats(creator)` - Get creator statistics

## Network Issue Resolution

If you encounter SSL certificate errors like:
```
error: Networking or low-level protocol error: HTTP error: error trying to connect: invalid peer certificate: UnknownIssuer
```

Try these solutions:

1. **Update Stellar CLI** (recommended):
   ```bash
   # If installed via homebrew
   brew install stellar
   
   # Or download latest from GitHub releases
   ```

2. **Use different network endpoint**:
   ```bash
   stellar network add testnet "https://horizon-testnet.stellar.org"
   ```

3. **Check system certificates**:
   ```bash
   # On macOS
   sudo security update-certs
   
   # Or try with insecure flag (not recommended for production)
   stellar contract deploy --insecure ...
   ```

## Contract Features Implemented

✅ **Puzzle Factory and Registry**
- Complete puzzle creation and metadata storage
- Creator attribution and royalty tracking
- Multi-index filtering (category, creator, difficulty, status)
- Access control with authorized creators
- **Enhanced deprecation logic with full cleanup**

✅ **Enhanced Deprecation Logic**
- Removes puzzle from all indexes (category, creator, difficulty)
- Updates creator statistics accurately
- Cleans up empty index entries
- Prevents double deprecation
- Comprehensive test coverage

✅ **Testing**
- 6 comprehensive tests covering all functionality
- All tests passing successfully

## Next Steps

1. Resolve network connectivity issue
2. Deploy contract using one of the commands above
3. Initialize contract with admin address
4. Authorize creators and start creating puzzles
5. Test the enhanced deprecation functionality
