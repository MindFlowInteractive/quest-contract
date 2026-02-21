#!/bin/bash

# Battle Pass Contract Deployment Script
# Deploys the battle pass contract to Soroban testnet

set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
TESTNET_RPC="https://soroban-testnet.stellar.org"
TESTNET_NETWORK="Test SDF Network ; September 2015"
CONTRACTS_DIR="$(dirname "$0")"

# Check if soroban CLI is installed
if ! command -v soroban &> /dev/null; then
    echo -e "${YELLOW}Soroban CLI not found. Please install it first.${NC}"
    echo "Visit: https://soroban.stellar.org/docs/learn/setup"
    exit 1
fi

# Function to print status messages
print_status() {
    echo -e "${BLUE}➜${NC} $1"
}

print_success() {
    echo -e "${GREEN}✓${NC} $1"
}

# Get source account
if [ -z "$SOURCE_ACCOUNT" ]; then
    read -p "Enter your Stellar account address (or set SOURCE_ACCOUNT env): " SOURCE_ACCOUNT
fi

if [ -z "$SOURCE_ACCOUNT" ]; then
    echo "Error: No source account provided"
    exit 1
fi

print_status "Source account: $SOURCE_ACCOUNT"

# Check if testnet network is configured
print_status "Checking testnet network configuration..."
if ! soroban config network list | grep -q "testnet"; then
    print_status "Adding testnet network..."
    soroban config network add \
        --rpc-url "$TESTNET_RPC" \
        --network-passphrase "$TESTNET_NETWORK" \
        testnet
    print_success "Testnet network added"
else
    print_success "Testnet network already configured"
fi

# Build contract
print_status "Building battle_pass contract..."
cd "$CONTRACTS_DIR"
cargo build --target wasm32-unknown-unknown --release
print_success "Contract built"

# Deploy contract
print_status "Deploying contract to testnet..."
WASM_PATH="$CONTRACTS_DIR/target/wasm32-unknown-unknown/release/battle_pass.wasm"

CONTRACT_ID=$(soroban contract deploy \
    --wasm "$WASM_PATH" \
    --source "$SOURCE_ACCOUNT" \
    --network testnet)

print_success "Contract deployed!"
print_status "Contract ID: $CONTRACT_ID"

# Initialize first season
print_status "Initializing season 1..."
soroban contract invoke \
    --id "$CONTRACT_ID" \
    --source "$SOURCE_ACCOUNT" \
    --network testnet \
    -- init_season \
    --season-number 1 \
    --reward-pool 1000000000

print_success "Season 1 initialized with 1B tokens reward pool"

# Verify deployment
print_status "Verifying deployment..."
CURRENT_SEASON=$(soroban contract invoke \
    --id "$CONTRACT_ID" \
    --network testnet \
    -- get_current_season)

print_success "Current season: $CURRENT_SEASON"

# Output summary
echo ""
echo -e "${GREEN}═══════════════════════════════════════════════${NC}"
echo -e "${GREEN}Battle Pass Contract Deployment Complete!${NC}"
echo -e "${GREEN}═══════════════════════════════════════════════${NC}"
echo ""
echo "Contract ID: $CONTRACT_ID"
echo "Network: Testnet"
echo "Current Season: $CURRENT_SEASON"
echo ""
echo "Next steps:"
echo "1. Save the Contract ID for later use"
echo "2. Test purchase_pass:"
echo "   soroban contract invoke \\"
echo "     --id $CONTRACT_ID \\"
echo "     --source <YOUR_ADDRESS> \\"
echo "     --network testnet \\"
echo "     -- purchase_pass \\"
echo "     --player <PLAYER_ADDRESS> \\"
echo "     --tier Free"
echo ""
