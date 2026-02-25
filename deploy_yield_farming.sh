#!/bin/bash

set -e

echo "🚀 Deploying Yield Farming Contract to Testnet"
echo "=============================================="

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Configuration
NETWORK="testnet"
SOURCE_ACCOUNT="puzzle_deployer"

echo -e "${BLUE}📦 Building contract...${NC}"
soroban contract build --package yield_farming

if [ ! -f "target/wasm32-unknown-unknown/release/yield_farming.wasm" ]; then
    echo "❌ Build failed - WASM file not found"
    exit 1
fi

echo -e "${GREEN}✅ Build successful${NC}"
echo ""

echo -e "${BLUE}🔍 Optimizing WASM...${NC}"
soroban contract optimize \
    --wasm target/wasm32-unknown-unknown/release/yield_farming.wasm

echo -e "${GREEN}✅ Optimization complete${NC}"
echo ""

echo -e "${BLUE}📤 Deploying to ${NETWORK}...${NC}"
CONTRACT_ID=$(soroban contract deploy \
    --wasm target/wasm32-unknown-unknown/release/yield_farming.wasm \
    --source ${SOURCE_ACCOUNT} \
    --network ${NETWORK})

if [ -z "$CONTRACT_ID" ]; then
    echo "❌ Deployment failed"
    exit 1
fi

echo -e "${GREEN}✅ Contract deployed successfully!${NC}"
echo ""
echo "📋 Contract Details:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "Contract ID: ${YELLOW}${CONTRACT_ID}${NC}"
echo -e "Network: ${YELLOW}${NETWORK}${NC}"
echo -e "Source: ${YELLOW}${SOURCE_ACCOUNT}${NC}"
echo ""

# Get admin address
ADMIN_ADDRESS=$(soroban keys address ${SOURCE_ACCOUNT})

echo -e "${BLUE}🔧 Initializing contract...${NC}"
echo "Please provide the reward token address:"
read -p "Reward Token Address: " REWARD_TOKEN

if [ -z "$REWARD_TOKEN" ]; then
    echo -e "${YELLOW}⚠️  No reward token provided. Skipping initialization.${NC}"
    echo "You can initialize later with:"
    echo ""
    echo "soroban contract invoke \\"
    echo "  --id ${CONTRACT_ID} \\"
    echo "  --source ${SOURCE_ACCOUNT} \\"
    echo "  --network ${NETWORK} \\"
    echo "  -- initialize \\"
    echo "  --admin ${ADMIN_ADDRESS} \\"
    echo "  --reward_token <REWARD_TOKEN_ADDRESS>"
else
    soroban contract invoke \
        --id ${CONTRACT_ID} \
        --source ${SOURCE_ACCOUNT} \
        --network ${NETWORK} \
        -- initialize \
        --admin ${ADMIN_ADDRESS} \
        --reward_token ${REWARD_TOKEN}
    
    echo -e "${GREEN}✅ Contract initialized${NC}"
    echo ""
    
    echo -e "${BLUE}📊 Creating example pool...${NC}"
    echo "Create a token pool? (y/n)"
    read -p "> " CREATE_POOL
    
    if [ "$CREATE_POOL" = "y" ]; then
        echo "Enter token address for the pool:"
        read -p "Token Address: " TOKEN_ADDRESS
        
        if [ ! -z "$TOKEN_ADDRESS" ]; then
            POOL_ID=$(soroban contract invoke \
                --id ${CONTRACT_ID} \
                --source ${SOURCE_ACCOUNT} \
                --network ${NETWORK} \
                -- create_pool \
                --asset_address ${TOKEN_ADDRESS} \
                --asset_type '{"Token":{}}' \
                --apy_basis_points 1000 \
                --lock_period_days 30 \
                --early_withdrawal_penalty_bp 500 \
                --multiplier_bp 10000 \
                --auto_compound false)
            
            echo -e "${GREEN}✅ Pool created with ID: ${POOL_ID}${NC}"
        fi
    fi
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}🎉 Deployment Complete!${NC}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "📝 Next Steps:"
echo "1. Create staking pools with create_pool"
echo "2. Fund contract with reward tokens"
echo "3. Users can stake tokens/NFTs"
echo "4. Monitor pool statistics"
echo ""
echo "🔗 Useful Commands:"
echo ""
echo "# Create a pool"
echo "soroban contract invoke --id ${CONTRACT_ID} --source ${SOURCE_ACCOUNT} --network ${NETWORK} \\"
echo "  -- create_pool --asset_address <TOKEN> --asset_type '{\"Token\":{}}' \\"
echo "  --apy_basis_points 1000 --lock_period_days 30 --early_withdrawal_penalty_bp 500 \\"
echo "  --multiplier_bp 10000 --auto_compound false"
echo ""
echo "# Stake tokens"
echo "soroban contract invoke --id ${CONTRACT_ID} --source <USER> --network ${NETWORK} \\"
echo "  -- stake_tokens --staker <USER_ADDRESS> --pool_id 1 --amount 10000"
echo ""
echo "# Check rewards"
echo "soroban contract invoke --id ${CONTRACT_ID} --network ${NETWORK} \\"
echo "  -- calculate_rewards --staker <USER_ADDRESS> --stake_id 1"
echo ""
echo "# Claim rewards"
echo "soroban contract invoke --id ${CONTRACT_ID} --source <USER> --network ${NETWORK} \\"
echo "  -- claim_rewards --staker <USER_ADDRESS> --stake_id 1"
echo ""
echo "# View pool stats"
echo "soroban contract invoke --id ${CONTRACT_ID} --network ${NETWORK} \\"
echo "  -- get_pool_stats --pool_id 1"
echo ""
echo "📊 Explorer: https://stellar.expert/explorer/${NETWORK}/contract/${CONTRACT_ID}"
echo ""
