#!/bin/bash

# Reward Vault Contract Deployment Script (Testnet)

set -e

GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

TESTNET_RPC="https://soroban-testnet.stellar.org"
TESTNET_NETWORK="Test SDF Network ; September 2015"
ROOT_DIR="$(dirname "$0")"
WASM_PATH="$ROOT_DIR/.stellar-artifacts/reward_vault.wasm"

print_status() {
  echo -e "${BLUE}➜${NC} $1"
}

print_success() {
  echo -e "${GREEN}✓${NC} $1"
}

if ! command -v stellar &> /dev/null; then
  echo -e "${YELLOW}Stellar CLI not found. Install it first.${NC}"
  exit 1
fi

if [ -z "$SOURCE_ACCOUNT" ]; then
  read -p "Enter SOURCE_ACCOUNT: " SOURCE_ACCOUNT
fi

if [ -z "$TOKEN_ADDRESS" ]; then
  read -p "Enter TOKEN_ADDRESS used by vault: " TOKEN_ADDRESS
fi

if [ -z "$SOURCE_ACCOUNT" ] || [ -z "$TOKEN_ADDRESS" ]; then
  echo "Missing SOURCE_ACCOUNT or TOKEN_ADDRESS."
  exit 1
fi

print_status "Checking testnet network configuration..."
if ! stellar network ls | grep -q "testnet"; then
  stellar network add testnet \
    --rpc-url "$TESTNET_RPC" \
    --network-passphrase "$TESTNET_NETWORK"
fi
print_success "Network is ready"

DEPLOYER_ADDRESS=$(stellar keys address "$SOURCE_ACCOUNT")
print_status "Deployer address: $DEPLOYER_ADDRESS"

print_status "Building reward_vault wasm..."
cd "$ROOT_DIR"
stellar contract build --package reward_vault --profile release --out-dir .stellar-artifacts
print_success "Build complete"

print_status "Deploying reward_vault..."
CONTRACT_ID=$(stellar contract deploy \
  --wasm "$WASM_PATH" \
  --source "$SOURCE_ACCOUNT" \
  --network testnet)
print_success "Deployed: $CONTRACT_ID"

# Defaults: 10% early penalty, 25% emergency penalty
# Lock periods (seconds): 7d, 30d, 90d
# Bonuses (bps): 500, 1200, 2500
print_status "Initializing contract..."
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$SOURCE_ACCOUNT" \
  --network testnet \
  -- initialize \
  --admin "$DEPLOYER_ADDRESS" \
  --token "$TOKEN_ADDRESS" \
  --early-withdraw-penalty-bps 1000 \
  --emergency-penalty-bps 2500 \
  --lock-periods "[604800,2592000,7776000]" \
  --bonus-bps "[500,1200,2500]"

print_success "Reward vault initialized"
echo "Contract ID: $CONTRACT_ID"
