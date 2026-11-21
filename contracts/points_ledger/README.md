# Achievement Points Ledger Contract

A modular Soroban smart contract for the Quest economy that implements player achievement point tracking across Stages 1 and 2:

1. **Points Ledger & Authorized Earning (Stage 1 - #402):**
   - Access-controlled point issuance: only contract admin and authorized game contracts can award points.
   - Immutable audit trail recording player, amount, reason code, timestamp, and calling contract.
   - Total balance strictly equals sum of all historical earn entries.

2. **FIFO Spending & Expiration Policy (Stage 2 - #403):**
   - Spend function at in-game shop with balance validation.
   - Duration-based expiration tracked per earn entry.
   - Strict First-In First-Out (FIFO) consumption: oldest unexpired points are spent first.
   - Expired points are strictly excluded from available balance and cannot be spent.
   - Spending is rejected if requested amount exceeds unexpired available balance.
