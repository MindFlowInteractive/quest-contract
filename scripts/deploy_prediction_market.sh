#!/bin/bash

# Prediction Market Contract Deployment Script
# Usage: ./deploy_prediction_market.sh [testnet|mainnet]

set -e

NETWORK=${1:-testnet}
CONTRACT_NAME="prediction_market"

echo "🚀 Deploying Prediction Market Contract to $NETWORK"

# Build the contract
echo "📦 Building contract..."
soroban contract build --package prediction-market

# Optimize the WASM
echo "⚡ Optimizing WASM..."
soroban contract optimize \
  --wasm target/wasm32-unknown-unknown/release/${CONTRACT_NAME}.wasm

# Deploy the contract
echo "🌐 Deploying to $NETWORK..."
CONTRACT_ID=$(soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/${CONTRACT_NAME}.optimized.wasm \
  --source deployer \
  --network $NETWORK)

echo "✅ Contract deployed successfully!"
echo "📝 Contract ID: $CONTRACT_ID"

# Initialize the contract
echo "🔧 Initializing contract..."
ADMIN_ADDRESS=$(soroban keys address deployer)

soroban contract invoke \
  --id $CONTRACT_ID \
  --source deployer \
  --network $NETWORK \
  -- initialize \
  --admin $ADMIN_ADDRESS

echo "✅ Contract initialized with admin: $ADMIN_ADDRESS"

# Save contract ID to file
echo $CONTRACT_ID > .prediction_market_${NETWORK}_contract_id

echo ""
echo "🎉 Deployment complete!"
echo "Contract ID saved to: .prediction_market_${NETWORK}_contract_id"
echo ""
echo "Next steps:"
echo "1. Create a test market:"
echo "   soroban contract invoke --id $CONTRACT_ID --source deployer --network $NETWORK -- create_market ..."
echo "2. Integrate with frontend/backend"
echo "3. Monitor events and transactions"
