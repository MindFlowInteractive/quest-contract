.PHONY: build test clean deploy-testnet optimize

build:
	soroban contract build

test:
	cargo test

clean:
	cargo clean
	rm -rf target/

optimize:
	soroban contract optimize --wasm target/wasm32-unknown-unknown/release/*.wasm

deploy-testnet:
	@echo "Deploying puzzle factory to testnet..."
	soroban contract deploy \
		--wasm target/wasm32v1-none/release/puzzle_factory.wasm \
		--source deployer \
		--network testnet

check:
	cargo check
	cargo clippy

fmt:
	cargo fmt
