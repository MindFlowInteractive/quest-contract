.PHONY: build test clean deploy check fmt

# Build the contract
build:
	@echo "Building battle_pass contract..."
	cargo build --target wasm32-unknown-unknown --release
	@echo "Build complete: target/wasm32-unknown-unknown/release/battle_pass.wasm"

# Run tests
test:
	@echo "Running battle_pass tests..."
	cargo test --lib

# Run tests with output
test-verbose:
	@echo "Running battle_pass tests (verbose)..."
	cargo test --lib -- --nocapture

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	cargo clean

# Format code
fmt:
	@echo "Formatting code..."
	cargo fmt

# Check code without building
check:
	@echo "Checking code..."
	cargo check

# Build optimized WASM
build-optimized:
	@echo "Building optimized WASM..."
	cargo build --target wasm32-unknown-unknown --release
	@echo "Optimized build complete"

# Display contract size
size: build
	@echo "Contract size:"
	@ls -lh target/wasm32-unknown-unknown/release/battle_pass.wasm

# Deploy to testnet (requires soroban CLI and testnet setup)
deploy: build
	@echo "Deploying to Soroban testnet..."
	@echo "Note: Set CONTRACT_ID and ADMIN_ACCOUNT environment variables"
	soroban contract deploy \
		--wasm target/wasm32-unknown-unknown/release/battle_pass.wasm \
		--source $(ADMIN_ACCOUNT) \
		--network testnet

# Initialize first season (requires CONTRACT_ID)
init-season:
	@echo "Initializing season 1..."
	soroban contract invoke \
		--id $(CONTRACT_ID) \
		--source $(ADMIN_ACCOUNT) \
		--network testnet \
		-- init_season \
		--season-number 1 \
		--reward-pool 1000000000

# Quick development workflow
dev: fmt check test build
	@echo "Development workflow complete!"

# All checks
all: clean fmt check test build
	@echo "All checks passed!"
