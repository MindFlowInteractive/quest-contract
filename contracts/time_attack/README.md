## Time Attack Contract

A Soroban smart contract for tracking puzzle completion times with
global and period-based leaderboards.

## Overview

This contract implements a time attack / speed run mode where players
submit completion times for puzzles. The contract records times using
ledger timestamps and maintains leaderboards that reset on a fixed
schedule.

## Features

- Time-stamped submissions using ledger timestamps
- Global and per-period leaderboards (daily, weekly, all-time)
- Anti-cheat safeguards (time bounds, rate limiting, replay hash checks)
- Automatic leaderboard resets based on elapsed time
- Storage-efficient design using persistent and temporary storage

> Note: Reward distribution and advanced verification are currently
> stubbed and intended to be extended in future iterations.

## Period reset behavior (LastReset initialization)

Leaderboards reset when `current_timestamp - last_reset >= duration_seconds` (implemented with
`saturating_sub` to avoid underflow if timestamps ever behave unexpectedly).
On first use for a given `(scope, period)`, `LastReset` is initialized to the
current ledger timestamp and **no reset is performed** on that call (the period
tracking starts at that moment).

## Prerequisites

This contract is written in Rust and uses Cargo for building and testing.

If you see an error like:

```text
cargo: command not found
```

you need to install Rust (which includes `cargo`) and ensure it’s on your `PATH`.

### Option A (recommended): rustup

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# make cargo available in the current shell
source ~/.cargo/env
```

Then verify:

```bash
cargo --version
rustc --version
```

### Option B: Ubuntu packages (may be older)

```bash
sudo apt update
sudo apt install -y cargo rustc
```

## Building

```bash
cargo build --target wasm32-unknown-unknown --release
```

## Testing

```bash
cargo test
```
