# Puzzle Lottery and Raffle Contract

Lottery system where players buy tickets with tokens for a chance to win large token prizes across multiple prize tiers.

## Features

- Lottery rounds with configurable ticket price, weekly/monthly schedule, prize tiers
- Ticket purchase with tokens; per-user ticket count for refunds
- Verifiable random winner selection (ledger sequence + timestamp PRNG)
- Multiple prize tiers and guaranteed winner when tickets sold
- Prize distribution and rollover support; cancel round + refund

## Build and test

```bash
cargo build -p puzzle_lottery
cargo test -p puzzle_lottery
```

## Deploy to testnet

Build the WASM and deploy with Soroban CLI, or use your project's testnet deployment flow.
