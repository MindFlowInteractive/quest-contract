#!/bin/bash

# Puzzle Insurance Contract Deployment Script
# This script handles the deployment of the puzzle insurance contract

set -e

# Add stellar CLI to PATH if installed locally
export PATH="$HOME/.local/bin:$PATH"

echo "🚀 Puzzle Insurance Contract Deployment"
echo "======================================="

# Set SSL certificate path to fix certificate issues
export SSL_CERT_FILE=/usr/local/etc/openssl@3/cert.pem

# Contract details
CONTRACT_NAME="puzzle_insurance"
WASM_FILE="target/wasm32v1-none/release/puzzle_insurance.wasm"
SOURCE_ACCOUNT="puzzle_deployer"
NETWORK="testnet"

# Configuration (update these with your values)
ADMIN_ADDRESS=""  # Will be set to deployer address if not provided
PAYMENT_TOKEN_ADDRESS=""  # Address of the payment token contract (will deploy if empty)
PUZZLE_VERIFICATION_ADDRESS=""  # Address of the puzzle_verification contract (will deploy if empty)
BASE_PREMIUM_RATE=100  # 1% in basis points (100 = 1%)

# Auto-deploy dependencies if addresses not provided
AUTO_DEPLOY_DEPS=true

echo "📋 Contract Details:"
echo "  - Name: $CONTRACT_NAME"
echo "  - WASM: $WASM_FILE"
echo "  - Source: $SOURCE_ACCOUNT"
echo "  - Network: $NETWORK"
echo "  - Base Premium Rate: $BASE_PREMIUM_RATE basis points (1%)"

# Check if WASM file exists
if [ ! -f "$WASM_FILE" ]; then
    echo "❌ WASM file not found. Building contract..."
    stellar contract build --package puzzle-insurance
fi

# Verify WASM file
if [ -f "$WASM_FILE" ]; then
    WASM_SIZE=$(ls -lh $WASM_FILE | awk '{print $5}')
    echo "✅ WASM file found: $WASM_SIZE"
    
    # Calculate WASM hash
    WASM_HASH=$(sha256sum $WASM_FILE | awk '{print $1}')
    echo "  - Hash: $WASM_HASH"
else
    echo "❌ Failed to build WASM file"
    exit 1
fi

# Check account balance
echo "💰 Checking account balance..."
ACCOUNT_ADDRESS=$(stellar keys address $SOURCE_ACCOUNT 2>/dev/null || echo "")
if [ -z "$ACCOUNT_ADDRESS" ]; then
    echo "❌ Account '$SOURCE_ACCOUNT' not found. Please create it first:"
    echo "   stellar keys generate $SOURCE_ACCOUNT"
    exit 1
fi
echo "  - Address: $ACCOUNT_ADDRESS"

# Set admin address if not provided
if [ -z "$ADMIN_ADDRESS" ]; then
    ADMIN_ADDRESS=$ACCOUNT_ADDRESS
    echo "  - Admin: $ADMIN_ADDRESS (using deployer address)"
fi

# Fund account if needed (check balance first)
BALANCE=$(stellar account info $ACCOUNT_ADDRESS --network $NETWORK 2>/dev/null | grep "Balance:" | awk '{print $2}' || echo "0")
if [ "$BALANCE" = "0" ] || [ -z "$BALANCE" ]; then
    echo "🪙 Funding account on testnet..."
    curl "https://friendbot.stellar.org/?addr=$ACCOUNT_ADDRESS" > /dev/null 2>&1
    sleep 2
    echo "✅ Account funded"
else
    echo "✅ Account already funded: $BALANCE XLM"
fi

# Upload WASM to network
echo "📤 Uploading WASM to network..."
UPLOAD_RESULT=$(stellar contract upload --wasm $WASM_FILE --source $SOURCE_ACCOUNT --network $NETWORK 2>&1 || echo "upload_failed")
if [[ "$UPLOAD_RESULT" == *"upload_failed"* ]] || [[ "$UPLOAD_RESULT" == *"error"* ]]; then
    echo "⚠️  WASM upload had issues, but continuing with deployment..."
else
    echo "✅ WASM uploaded"
fi

# Deploy contract
echo "🚀 Deploying contract..."
DEPLOY_RESULT=$(stellar contract deploy --wasm $WASM_FILE --source $SOURCE_ACCOUNT --network $NETWORK --ignore-checks 2>&1 || echo "deploy_failed")

if [[ "$DEPLOY_RESULT" == *"deploy_failed"* ]] || [[ "$DEPLOY_RESULT" == *"error"* ]]; then
    echo "❌ Deployment failed. Trying alternative method..."
    echo "📌 Attempting deployment with WASM hash..."
    DEPLOY_RESULT=$(stellar contract deploy --wasm-hash $WASM_HASH --source $SOURCE_ACCOUNT --network $NETWORK --ignore-checks 2>&1 || echo "deploy_failed")
fi

# Extract contract ID from deployment result
CONTRACT_ID=$(echo "$DEPLOY_RESULT" | grep -oE '[A-Z0-9]{56}' | head -1 || echo "")

if [ -z "$CONTRACT_ID" ]; then
    echo "❌ Failed to extract contract ID from deployment"
    echo "📋 Deployment output:"
    echo "$DEPLOY_RESULT"
    echo ""
    echo "🔧 Manual Deployment Instructions:"
    echo "1. Deploy the contract manually:"
    echo "   stellar contract deploy --wasm $WASM_FILE --source $SOURCE_ACCOUNT --network $NETWORK"
    echo ""
    echo "2. Or use WASM hash:"
    echo "   stellar contract deploy --wasm-hash $WASM_HASH --source $SOURCE_ACCOUNT --network $NETWORK"
    echo ""
    echo "3. After deployment, initialize with:"
    echo "   stellar contract invoke --id CONTRACT_ID --source $SOURCE_ACCOUNT --network $NETWORK -- \\"
    echo "     initialize \\"
    echo "     --admin $ADMIN_ADDRESS \\"
    echo "     --payment_token PAYMENT_TOKEN_ADDRESS \\"
    echo "     --puzzle_verification PUZZLE_VERIFICATION_ADDRESS \\"
    echo "     --base_premium_rate $BASE_PREMIUM_RATE"
    exit 1
fi

echo "✅ Contract deployed successfully!"
echo "  - Contract ID: $CONTRACT_ID"

# Deploy dependencies if not provided
if [ "$AUTO_DEPLOY_DEPS" = true ]; then
    # Deploy puzzle_verification if not provided
    if [ -z "$PUZZLE_VERIFICATION_ADDRESS" ]; then
        echo ""
        echo "📦 Deploying puzzle_verification contract..."
        PUZZLE_VERIFICATION_WASM="target/wasm32v1-none/release/puzzle_verification.wasm"
        if [ -f "$PUZZLE_VERIFICATION_WASM" ]; then
            VERIFY_DEPLOY=$(stellar contract deploy --wasm $PUZZLE_VERIFICATION_WASM --source $SOURCE_ACCOUNT --network $NETWORK --ignore-checks 2>&1 || echo "deploy_failed")
            PUZZLE_VERIFICATION_ADDRESS=$(echo "$VERIFY_DEPLOY" | grep -oE '[A-Z0-9]{56}' | head -1 || echo "")
            if [ -n "$PUZZLE_VERIFICATION_ADDRESS" ]; then
                echo "✅ Puzzle verification deployed: $PUZZLE_VERIFICATION_ADDRESS"
                # Initialize puzzle_verification
                stellar contract invoke --id $PUZZLE_VERIFICATION_ADDRESS --source $SOURCE_ACCOUNT --network $NETWORK -- initialize --admin $ADMIN_ADDRESS 2>&1 || echo "Init may have failed"
            else
                echo "❌ Failed to deploy puzzle_verification"
            fi
        fi
    fi
    
    # Deploy reward_token if not provided
    if [ -z "$PAYMENT_TOKEN_ADDRESS" ]; then
        echo ""
        echo "📦 Deploying reward_token contract..."
        REWARD_TOKEN_WASM="target/wasm32v1-none/release/reward_token.wasm"
        if [ -f "$REWARD_TOKEN_WASM" ]; then
            TOKEN_DEPLOY=$(stellar contract deploy --wasm $REWARD_TOKEN_WASM --source $SOURCE_ACCOUNT --network $NETWORK --ignore-checks 2>&1 || echo "deploy_failed")
            PAYMENT_TOKEN_ADDRESS=$(echo "$TOKEN_DEPLOY" | grep -oE '[A-Z0-9]{56}' | head -1 || echo "")
            if [ -n "$PAYMENT_TOKEN_ADDRESS" ]; then
                echo "✅ Reward token deployed: $PAYMENT_TOKEN_ADDRESS"
            else
                echo "❌ Failed to deploy reward_token"
            fi
        fi
    fi
fi

# Check if required addresses are provided for initialization
if [ -z "$PAYMENT_TOKEN_ADDRESS" ] || [ -z "$PUZZLE_VERIFICATION_ADDRESS" ]; then
    echo ""
    echo "⚠️  Missing required addresses for initialization"
    echo "📝 To initialize the contract, you need:"
    echo "   1. Payment Token Address (token contract for premiums/payouts)"
    echo "   2. Puzzle Verification Address (puzzle_verification contract)"
    echo ""
    echo "🔧 Initialize manually with:"
    echo "   stellar contract invoke --id $CONTRACT_ID --source $SOURCE_ACCOUNT --network $NETWORK -- \\"
    echo "     initialize \\"
    echo "     --admin $ADMIN_ADDRESS \\"
    echo "     --payment_token PAYMENT_TOKEN_ADDRESS \\"
    echo "     --puzzle_verification PUZZLE_VERIFICATION_ADDRESS \\"
    echo "     --base_premium_rate $BASE_PREMIUM_RATE"
else
    # Initialize contract
    echo ""
    echo "🔧 Initializing contract..."
    INIT_RESULT=$(stellar contract invoke \
        --id $CONTRACT_ID \
        --source $SOURCE_ACCOUNT \
        --network $NETWORK \
        -- \
        initialize \
        --admin $ADMIN_ADDRESS \
        --payment_token $PAYMENT_TOKEN_ADDRESS \
        --puzzle_verification $PUZZLE_VERIFICATION_ADDRESS \
        --base_premium_rate $BASE_PREMIUM_RATE \
        2>&1 || echo "init_failed")
    
    if [[ "$INIT_RESULT" == *"init_failed"* ]] || [[ "$INIT_RESULT" == *"error"* ]]; then
        echo "❌ Initialization failed"
        echo "📋 Error output:"
        echo "$INIT_RESULT"
        echo ""
        echo "🔧 Try initializing manually:"
        echo "   stellar contract invoke --id $CONTRACT_ID --source $SOURCE_ACCOUNT --network $NETWORK -- \\"
        echo "     initialize \\"
        echo "     --admin $ADMIN_ADDRESS \\"
        echo "     --payment_token $PAYMENT_TOKEN_ADDRESS \\"
        echo "     --puzzle_verification $PUZZLE_VERIFICATION_ADDRESS \\"
        echo "     --base_premium_rate $BASE_PREMIUM_RATE"
    else
        echo "✅ Contract initialized successfully!"
    fi
fi

echo ""
echo "🎉 Puzzle Insurance Contract Deployment Complete!"
echo "================================================"
echo ""
echo "📝 Contract Information:"
echo "  - Contract ID: $CONTRACT_ID"
echo "  - Network: $NETWORK"
echo "  - Admin: $ADMIN_ADDRESS"
echo "  - Base Premium Rate: $BASE_PREMIUM_RATE basis points"
echo ""
echo "🔗 Available Functions:"
echo "  - purchase_policy(owner, puzzle_id, difficulty, coverage_amount, coverage_period, attempts_covered)"
echo "  - cancel_policy(owner, puzzle_id)"
echo "  - submit_claim(claimant, puzzle_id, claim_amount, description, attempt_timestamp)"
echo "  - review_claim(admin, claim_id, approved, review_notes, payout_amount)"
echo "  - process_payout(admin, claim_id)"
echo "  - add_to_pool(admin, amount)"
echo "  - withdraw_from_pool(admin, amount)"
echo "  - get_policy(user, puzzle_id)"
echo "  - get_claim(claim_id)"
echo "  - get_premium_pool()"
echo "  - calculate_premium(difficulty, coverage_amount, coverage_period, attempts_covered)"
echo ""
echo "📊 Example Usage:"
echo ""
echo "1. Purchase a policy:"
echo "   stellar contract invoke --id $CONTRACT_ID --source USER_ADDRESS --network $NETWORK -- \\"
echo "     purchase_policy \\"
echo "     --owner USER_ADDRESS \\"
echo "     --puzzle_id 1 \\"
echo "     --difficulty 5 \\"
echo "     --coverage_amount 1000000000 \\"
echo "     --coverage_period 2592000 \\"
echo "     --attempts_covered 5"
echo ""
echo "2. Submit a claim:"
echo "   stellar contract invoke --id $CONTRACT_ID --source USER_ADDRESS --network $NETWORK -- \\"
echo "     submit_claim \\"
echo "     --claimant USER_ADDRESS \\"
echo "     --puzzle_id 1 \\"
echo "     --claim_amount 500000000 \\"
echo "     --description \"Failed puzzle attempt\" \\"
echo "     --attempt_timestamp TIMESTAMP"
echo ""
echo "3. Review and approve claim (admin only):"
echo "   stellar contract invoke --id $CONTRACT_ID --source $ADMIN_ADDRESS --network $NETWORK -- \\"
echo "     review_claim \\"
echo "     --admin $ADMIN_ADDRESS \\"
echo "     --claim_id 1 \\"
echo "     --approved true \\"
echo "     --review_notes \"Approved\" \\"
echo "     --payout_amount 450000000"
echo ""
echo "4. Process payout (admin only):"
echo "   stellar contract invoke --id $CONTRACT_ID --source $ADMIN_ADDRESS --network $NETWORK -- \\"
echo "     process_payout \\"
echo "     --admin $ADMIN_ADDRESS \\"
echo "     --claim_id 1"
echo ""
echo "🔍 SSL Certificate Fix Applied: $SSL_CERT_FILE"
echo "📊 Deployer Account: $ACCOUNT_ADDRESS"
echo "🌐 Network: $NETWORK"
