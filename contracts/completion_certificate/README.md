# Completion Certificate NFT Contract

**Issue #115 — Quest Service Smart Contracts**  
`contracts/completion_certificate` | Soroban · Rust · Stellar Testnet

---

## Overview

The `completion_certificate` Soroban contract issues verifiable on-chain NFT certificates whenever a player successfully solves a puzzle.  
Each certificate embeds proof of completion (solution hash, timestamp, rank) and earns a **rarity tier** based on how fast the solver completed the puzzle.

---

## Features

| Feature | Detail |
|---|---|
| Certificate metadata | puzzle id, title, completion time, rank, solution hash, URI |
| Rarity tiers | Legendary / Epic / Rare / Uncommon / Common (time-based) |
| On-chain verification | `verify_certificate` returns a typed `VerificationProof` |
| Duplicate prevention | One certificate per `(puzzle_id, owner)` pair |
| Transfer restrictions | Certificates can be soulbound (`transferable = false`) |
| Burn | Owners can destroy their own certificate |
| Showcase / Gallery | `get_showcase` returns certs sorted by rarity (best first) |
| Event emissions | `mint`, `transfer`, `burn` events published via `env.events()` |
| Admin controls | Pause/unpause minting & transfers; transfer admin role |

---

## Rarity Tiers

| Tier | Completion Time | Weight |
|---|---|---|
| 🟡 Legendary | ≤ 60 seconds | 5 |
| 🟠 Epic | 61 – 300 seconds | 4 |
| 🔵 Rare | 301 – 900 seconds | 3 |
| 🟢 Uncommon | 901 – 3 600 seconds | 2 |
| ⚪ Common | > 3 600 seconds | 1 |

---

## Contract Functions

### Admin
| Function | Description |
|---|---|
| `initialize(admin)` | Deploy & set admin (once only) |
| `set_admin(new_admin)` | Transfer admin role |
| `set_paused(paused)` | Pause / resume minting and transfers |

### Minting
| Function | Description |
|---|---|
| `mint_certificate(owner, puzzle_id, puzzle_title, completion_time_secs, rank, solution_hash, metadata_uri, transferable)` | Mint a certificate NFT (admin only) |

### Transfers & Lifecycle
| Function | Description |
|---|---|
| `transfer(from, to, token_id)` | Transfer a transferable certificate |
| `burn(owner, token_id)` | Destroy a certificate (owner only) |

### Queries
| Function | Description |
|---|---|
| `get_certificate(token_id)` | Full metadata |
| `get_owner_certificates(owner)` | All token ids owned |
| `get_showcase(owner)` | Full metadata sorted by rarity |
| `verify_certificate(token_id)` | On-chain authenticity proof |
| `is_minted(puzzle_id, owner)` | Check duplicate |
| `total_supply()` | Total certificates minted |
| `get_admin()` | Current admin address |
| `is_paused()` | Contract pause state |

---

## Quick Start

### Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# WASM target
rustup target add wasm32-unknown-unknown

# Soroban CLI
cargo install --locked soroban-cli --version 21.0.0
```

### Build

```bash
# From repo root
soroban contract build

# Or directly
cargo build --target wasm32-unknown-unknown --release
```

### Optimise

```bash
soroban contract optimize \
  --wasm target/wasm32-unknown-unknown/release/completion_certificate.wasm
```

---

## Testing

```bash
# Run all tests (with output)
cargo test --package completion-certificate -- --nocapture

# Run a specific test
cargo test --package completion-certificate test_rarity_legendary -- --nocapture

# Full workspace
cargo test
```

---

## Deployment to Testnet

```bash
# 1. Add testnet network
soroban network add testnet \
  --rpc-url https://soroban-testnet.stellar.org \
  --network-passphrase "Test SDF Network ; September 2015"

# 2. Generate & fund deployer identity
soroban keys generate deployer --network testnet
soroban keys fund deployer --network testnet

# 3. Deploy
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/completion_certificate.optimized.wasm \
  --source deployer \
  --network testnet

# 4. Initialise (replace CONTRACT_ID and ADMIN_ADDRESS)
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source deployer \
  --network testnet \
  -- initialize \
  --admin <ADMIN_ADDRESS>
```

---

## Step-by-Step Testing Guide

Follow these steps to verify a complete, successful implementation:

### Step 1 — Clone & Navigate

```bash
git clone <repo-url>
cd quest-service-contracts
```

### Step 2 — Build the Contract

```bash
cargo build --target wasm32-unknown-unknown --release --package completion-certificate
```
✅ **Expected:** Compilation succeeds with no errors. A `.wasm` file is produced at  
`target/wasm32-unknown-unknown/release/completion_certificate.wasm`

### Step 3 — Run Unit Tests

```bash
cargo test --package completion-certificate -- --nocapture
```
✅ **Expected output (example):**
```
running 20 tests
test tests::test_rarity_legendary ... ok
test tests::test_rarity_epic ... ok
test tests::test_rarity_rare ... ok
test tests::test_rarity_uncommon ... ok
test tests::test_rarity_common ... ok
test tests::test_rarity_weights_ordered ... ok
test tests::test_initialize ... ok
test tests::test_double_initialize_panics ... ok
test tests::test_mint_certificate_basic ... ok
test tests::test_mint_increments_token_id ... ok
test tests::test_is_minted ... ok
test tests::test_mint_duplicate_panics ... ok
test tests::test_transfer_transferable_cert ... ok
test tests::test_transfer_non_transferable_panics ... ok
test tests::test_burn_certificate ... ok
test tests::test_burn_by_non_owner_panics ... ok
test tests::test_verify_valid_certificate ... ok
test tests::test_verify_nonexistent_returns_inauthentic ... ok
test tests::test_showcase_sorted_by_rarity ... ok
test tests::test_mint_when_paused_panics ... ok
test tests::test_unpause_allows_mint ... ok
test tests::test_set_admin ... ok
test tests::test_get_owner_certificates_empty ... ok
test tests::test_get_owner_certificates_multiple ... ok

test result: ok. 24 passed; 0 failed; 0 ignored
```

### Step 4 — Optimise the WASM

```bash
soroban contract optimize \
  --wasm target/wasm32-unknown-unknown/release/completion_certificate.wasm
```
✅ **Expected:** `.optimized.wasm` file created. File size will be significantly smaller.

### Step 5 — Deploy to Testnet

```bash
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/completion_certificate.optimized.wasm \
  --source deployer \
  --network testnet
```
✅ **Expected:** Contract ID printed, e.g. `CAABC...XYZ`

### Step 6 — Initialise on Testnet

```bash
soroban contract invoke \
  --id <CONTRACT_ID> --source deployer --network testnet \
  -- initialize --admin <ADMIN_ADDRESS>
```
✅ **Expected:** No error; transaction confirmed.

### Step 7 — Mint a Certificate

```bash
soroban contract invoke \
  --id <CONTRACT_ID> --source deployer --network testnet \
  -- mint_certificate \
  --owner <PLAYER_ADDRESS> \
  --puzzle_id '"PUZZLE-001"' \
  --puzzle_title '"The Lost Labyrinth"' \
  --completion_time_secs 45 \
  --rank 1 \
  --solution_hash '"abc123def456"' \
  --metadata_uri '"ipfs://QmYourHash"' \
  --transferable true
```
✅ **Expected:** Returns `1` (first token id).

### Step 8 — Verify Certificate

```bash
soroban contract invoke \
  --id <CONTRACT_ID> --source deployer --network testnet \
  -- verify_certificate --token_id 1
```
✅ **Expected:** Returns a `VerificationProof` with `authentic: true`, rarity `Legendary` (45 s ≤ 60 s).

### Step 9 — View Showcase

```bash
soroban contract invoke \
  --id <CONTRACT_ID> --source deployer --network testnet \
  -- get_showcase --owner <PLAYER_ADDRESS>
```
✅ **Expected:** Array of certificates sorted by rarity weight (highest first).

### Step 10 — Verify Transfer Restriction (Soulbound)

Mint a certificate with `--transferable false`, then attempt to transfer it.  
✅ **Expected:** Transaction fails with error code `5` (`TransferRestricted`).

---

## Error Reference

| Code | Name | Trigger |
|---|---|---|
| 1 | `NotAdmin` | Caller is not admin |
| 2 | `NotOwner` | Caller does not own the token |
| 3 | `CertNotFound` | Token does not exist or is burned |
| 4 | `AlreadyMinted` | `(puzzle_id, owner)` already has a cert |
| 5 | `TransferRestricted` | Token is soulbound |
| 6 | `ContractPaused` | Mint/transfer while paused |
| 7 | `InvalidInput` | Bad argument |
| 8 | `Unauthorized` | Unauthorised initialisation attempt |

---

## License

MIT