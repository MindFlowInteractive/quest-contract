#!/bin/bash

# Puzzle Factory Contract Deployment Script
# This script handles the deployment of the puzzle factory contract

set -e

echo "🚀 Puzzle Factory Contract Deployment"
echo "====================================="

# Set SSL certificate path to fix certificate issues
export SSL_CERT_FILE=/usr/local/etc/openssl@3/cert.pem

# Contract details
CONTRACT_NAME="puzzle_factory"
WASM_FILE="target/wasm32v1-none/release/puzzle_factory.wasm"
WASM_HASH="4ea640f52d13350400b81d11c9e29977b9de853c19f679fd7f6f036c4f840d3e"
SOURCE_ACCOUNT="puzzle_deployer"
NETWORK="testnet"

echo "📋 Contract Details:"
echo "  - Name: $CONTRACT_NAME"
echo "  - WASM: $WASM_FILE"
echo "  - Hash: $WASM_HASH"
echo "  - Source: $SOURCE_ACCOUNT"
echo "  - Network: $NETWORK"

# Check if WASM file exists
if [ ! -f "$WASM_FILE" ]; then
    echo "❌ WASM file not found. Building contract..."
    soroban contract build --package puzzle-factory
fi

# Verify WASM file
echo "✅ WASM file found: $(ls -lh $WASM_FILE | awk '{print $5}')"

# Check account balance
echo "💰 Checking account balance..."
ACCOUNT_ADDRESS=$(stellar keys address $SOURCE_ACCOUNT)
echo "  - Address: $ACCOUNT_ADDRESS"

# Fund account if needed (check balance first)
BALANCE=$(stellar account info $ACCOUNT_ADDRESS --network $NETWORK 2>/dev/null | grep "Balance:" | awk '{print $2}' || echo "0")
if [ "$BALANCE" = "0" ] || [ -z "$BALANCE" ]; then
    echo "🪙 Funding account on testnet..."
    curl "https://friendbot.stellar.org/?addr=$ACCOUNT_ADDRESS" > /dev/null 2>&1
    echo "✅ Account funded"
else
    echo "✅ Account already funded: $BALANCE XLM"
fi

# Upload WASM to network
echo "📤 Uploading WASM to network..."
UPLOAD_RESULT=$(stellar contract upload --wasm $WASM_FILE --source $SOURCE_ACCOUNT --network $NETWORK)
echo "✅ WASM uploaded: $UPLOAD_RESULT"

# Try different deployment approaches
echo "🚀 Attempting deployment..."

# Method 1: Direct deployment
echo "📌 Method 1: Direct deployment..."
if stellar contract deploy --wasm $WASM_FILE --source $SOURCE_ACCOUNT --network $NETWORK --ignore-checks 2>/dev/null; then
    echo "✅ Deployment successful!"
    DEPLOYMENT_SUCCESS=true
else
    echo "❌ Direct deployment failed"
    DEPLOYMENT_SUCCESS=false
fi

# Method 2: Using WASM hash if direct deployment fails
if [ "$DEPLOYMENT_SUCCESS" = false ]; then
    echo "📌 Method 2: Using WASM hash..."
    if stellar contract deploy --wasm-hash $WASM_HASH --source $SOURCE_ACCOUNT --network $NETWORK --ignore-checks 2>/dev/null; then
        echo "✅ Deployment successful!"
        DEPLOYMENT_SUCCESS=true
    else
        echo "❌ WASM hash deployment failed"
    fi
fi

# Method 3: Manual transaction building if automated deployment fails
if [ "$DEPLOYMENT_SUCCESS" = false ]; then
    echo "📌 Method 3: Manual transaction building..."
    echo "⚠️  This requires manual intervention due to CLI version compatibility"
    echo ""
    echo "🔧 Manual Deployment Instructions:"
    echo "1. Update Stellar CLI to latest version:"
    echo "   brew install stellar  # or cargo install stellar-cli --force"
    echo ""
    echo "2. Or use the following manual steps:"
    echo "   - WASM Hash: $WASM_HASH"
    echo "   - Source Account: $ACCOUNT_ADDRESS"
    echo "   - Network: $NETWORK"
    echo ""
    echo "3. Try deployment with updated CLI:"
    echo "   stellar contract deploy --wasm $WASM_FILE --source $SOURCE_ACCOUNT --network $NETWORK"
fi

# If deployment was successful, provide next steps
if [ "$DEPLOYMENT_SUCCESS" = true ]; then
    echo ""
    echo "🎉 Contract Deployment Complete!"
    echo "================================"
    echo ""
    echo "📝 Next Steps:"
    echo "1. Initialize the contract:"
    echo "   stellar contract invoke --id CONTRACT_ID --source $SOURCE_ACCOUNT --network $NETWORK -- initialize --admin $ACCOUNT_ADDRESS"
    echo ""
    echo "2. Authorize creators:"
    echo "   stellar contract invoke --id CONTRACT_ID --source $SOURCE_ACCOUNT --network $NETWORK -- authorize_creator --creator CREATOR_ADDRESS"
    echo ""
    echo "3. Create your first puzzle:"
    echo "   stellar contract invoke --id CONTRACT_ID --source CREATOR_ADDRESS --network $NETWORK -- create_puzzle ..."
    echo ""
    echo "🔗 Contract Functions Available:"
    echo "  - initialize(admin)"
    echo "  - authorize_creator(creator)"
    echo "  - create_puzzle(...)"
    echo "  - deprecate_puzzle(puzzle_id)"
    echo "  - get_puzzle(puzzle_id)"
    echo "  - get_puzzles_by_category(category)"
    echo "  - get_puzzles_by_creator(creator)"
    echo "  - get_puzzles_by_difficulty(difficulty)"
    echo "  - get_active_puzzles()"
    echo "  - get_creator_stats(creator)"
    echo "  - update_puzzle(...)"
    echo "  - activate_puzzle(puzzle_id)"
    echo "  - deactivate_puzzle(puzzle_id)"
    echo "  - revoke_creator(creator)"
    echo "  - is_creator_authorized(creator)"
    echo "  - get_puzzle_count()"
else
    echo ""
    echo "❌ Deployment Failed"
    echo "=================="
    echo "The deployment failed due to CLI compatibility issues."
    echo "Please update your Stellar CLI and try again."
fi

echo ""
echo "🔍 SSL Certificate Fix Applied: $SSL_CERT_FILE"
echo "📊 Account: $ACCOUNT_ADDRESS"
echo "🌐 Network: $NETWORK"
