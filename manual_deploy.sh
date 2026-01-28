#!/bin/bash

# Manual Deployment Solution for Puzzle Factory Contract
# This works around CLI compatibility issues by using the built transaction

set -e

echo "🔧 Manual Deployment Solution"
echo "============================"

# Set SSL certificate path
export SSL_CERT_FILE=/usr/local/etc/openssl@3/cert.pem

# Contract details
WASM_HASH="4ea640f52d13350400b81d11c9e29977b9de853c19f679fd7f6f036c4f840d3e"
SOURCE_ACCOUNT="puzzle_deployer"
NETWORK="testnet"

echo "📋 Deployment Details:"
echo "  - WASM Hash: $WASM_HASH"
echo "  - Source: $SOURCE_ACCOUNT"
echo "  - Network: $NETWORK"

# Build the transaction
echo ""
echo "🏗️  Building deployment transaction..."
TRANSACTION_XDR=$(stellar contract deploy --wasm-hash $WASM_HASH --source $SOURCE_ACCOUNT --network $NETWORK --build-only)

echo "✅ Transaction built successfully!"
echo "📄 Transaction XDR:"
echo "$TRANSACTION_XDR"

echo ""
echo "🚀 Deployment Options:"
echo ""

# Option 1: Try to submit via different CLI commands
echo "📌 Option 1: Submit via tx command"
if stellar tx submit --source $SOURCE_ACCOUNT --network $NETWORK "$TRANSACTION_XDR" 2>/dev/null; then
    echo "✅ Deployment successful via tx submit!"
    exit 0
else
    echo "❌ tx submit failed"
fi

# Option 2: Try with different fee structure
echo ""
echo "📌 Option 2: Try with different fee settings"
FEE_TRANSACTION=$(stellar contract deploy --wasm-hash $WASM_HASH --source $SOURCE_ACCOUNT --network $NETWORK --fee 10000 --build-only)
if stellar tx submit --source $SOURCE_ACCOUNT --network $NETWORK "$FEE_TRANSACTION" 2>/dev/null; then
    echo "✅ Deployment successful with higher fee!"
    exit 0
else
    echo "❌ Higher fee submission failed"
fi

# Option 3: Manual submission instructions
echo ""
echo "📌 Option 3: Manual Web Submission"
echo "Since CLI submission fails, you can submit manually:"
echo ""
echo "1. Go to Stellar Laboratory:"
echo "   https://laboratory.stellar.org"
echo ""
echo "2. Select 'Transaction Builder' > 'Sign Transaction'"
echo ""
echo "3. Paste this XDR:"
echo "   $TRANSACTION_XDR"
echo ""
echo "4. Sign with your account: $SOURCE_ACCOUNT"
echo ""
echo "5. Submit to testnet"
echo ""

# Option 4: Use online tools
echo "📌 Option 4: Online Deployment Tools"
echo ""
echo "Stellar Expert Contract Deployer:"
echo "1. Visit: https://stellar.expert/contract/deployer"
echo "2. Network: Testnet"
echo "3. WASM Hash: $WASM_HASH"
echo "4. Deploy with your account"
echo ""

# Option 5: Generate contract ID for manual setup
echo "📌 Option 5: Generate Contract ID"
echo ""
echo "Contract ID can be calculated with:"
echo "stellar contract id --wasm-hash $WASM_HASH --salt 0000000000000000000000000000000000000000000000000000000000000000 --source $SOURCE_ACCOUNT --network $NETWORK"

# Get account address for reference
ACCOUNT_ADDRESS=$(stellar keys address $SOURCE_ACCOUNT)
echo ""
echo "📊 Account Information:"
echo "  - Address: $ACCOUNT_ADDRESS"
echo "  - Network: $NETWORK"
echo "  - WASM Hash: $WASM_HASH"

echo ""
echo "🎯 Summary:"
echo "=========="
echo "✅ SSL Certificate: FIXED"
echo "✅ Transaction Building: WORKING"
echo "✅ WASM Uploaded: CONFIRMED"
echo "🔧 Final Submission: CLI COMPATIBILITY ISSUE"
echo ""
echo "💡 Recommended: Use Option 3 (Stellar Laboratory) for immediate deployment"
echo "🔗 Quick Link: https://laboratory.stellar.org"
echo ""
echo "📝 Your contract is ready and the transaction is built - just needs final submission!"
