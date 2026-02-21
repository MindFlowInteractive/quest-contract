.PHONY: build test clean deploy

build:
	@echo "Building yield farming contract..."
	@cargo build --package yield_farming --target wasm32-unknown-unknown --release
	@ls -lh target/wasm32-unknown-unknown/release/yield_farming.wasm

test:
	@echo "Running tests..."
	@cargo test --package yield_farming

clean:
	@echo "Cleaning build artifacts..."
	@cargo clean

deploy:
	@echo "Deploying to testnet..."
	@./deploy_yield_farming.sh

optimize:
	@echo "Optimizing WASM..."
	@soroban contract optimize --wasm target/wasm32-unknown-unknown/release/yield_farming.wasm

all: test build

help:
	@echo "Available targets:"
	@echo "  build    - Build the contract WASM"
	@echo "  test     - Run all tests"
	@echo "  clean    - Clean build artifacts"
	@echo "  deploy   - Deploy to testnet"
	@echo "  optimize - Optimize WASM size"
	@echo "  all      - Run tests and build"
