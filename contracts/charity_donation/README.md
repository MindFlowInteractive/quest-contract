# Charity & Public Goods Donation Contract

Transparent donation contract with quadratic funding mechanism for Quest Service.

## Features

- **Direct Donations**: Players donate tokens to verified charities
- **Quadratic Funding**: Matching pool distribution using QF formula: (Σ√contributions)² - total_raised
- **Charity Verification**: Admin-approved charity system
- **NFT Receipts**: Donors can mint donation certificates
- **Recurring Donations**: Set up automatic recurring contributions
- **Leaderboard**: Top 10 donors tracked globally
- **Impact Tracking**: Full donation history per donor/charity

## Core Functions

### Admin Functions
- `initialize(admin, token)` - Set up contract
- `add_charity(admin, name, wallet)` - Register new charity
- `verify_charity(admin, charity_id)` - Approve charity for donations
- `distribute_matching(admin, charity_id)` - Distribute QF matching funds

### Donor Functions
- `donate(donor, charity_id, amount)` - Make direct donation
- `fund_matching_pool(funder, amount)` - Add to matching pool
- `issue_receipt(donor, charity_id)` - Mint donation certificate NFT
- `set_recurring(donor, charity_id, amount, enabled)` - Enable/disable recurring

### Query Functions
- `get_charity(charity_id)` - Get charity details
- `get_donor_total(donor, charity_id)` - Get total donated by donor
- `get_leaderboard()` - Get top 10 donors

## Quadratic Funding Formula

```
matching_amount = (Σ√individual_contributions)² - total_direct_donations
```

This rewards charities with broad community support over those with few large donors.

## Testing

```bash
cargo test --package charity
```

## Deployment

```bash
soroban contract build
soroban contract deploy --wasm target/wasm32-unknown-unknown/release/charity.wasm
```
