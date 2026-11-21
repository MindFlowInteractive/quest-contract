# Verifiable Puzzle Seed Beacon Contract

A modular Soroban smart contract for the Quest economy that implements unpredictable and publicly verifiable random seed generation for puzzle rounds:

1. **Commit Phase:**
   - Contract admin commits to a specific future ledger sequence and entropy salt.
   - Target ledger sequence must be strictly greater than current ledger sequence (`TargetLedgerMustBeInFuture`).
   - Seeds cannot be known in advance by players or malicious actors.

2. **Reveal Phase:**
   - Seed derivation is unlocked once the committed ledger sequence is reached (`CommittedLedgerNotReached` error if called early).
   - Seed is deterministically derived: `sha256(round_id || committed_ledger_hash || entropy_salt)`.
   - Exactly one seed is produced per round (idempotent reveals return existing seed).

3. **Public Verification:**
   - `verify_seed` function allows any player or third-party observer to independently verify that a puzzle was generated from the published rule and ledger hash.
