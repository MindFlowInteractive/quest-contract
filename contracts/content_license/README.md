# Content License Registry Contract

A modular Soroban smart contract for the Quest economy that implements puzzle content licensing across Stages 1 and 2:

1. **Versioned License Terms Registry (Stage 1 - #397):**
   - Creators register on-chain terms (`FreeToPlay`, `PaidPerPlay`, `RemixAllowed`).
   - Immutable historical versioning: each update increments version while preserving previous terms.
   - Terms specify fee token, fee amount, validity duration, revocation policy, and refund rules.

2. **License Purchase & Policy-Based Revocation (Stage 2 - #398):**
   - Players purchase licenses under current puzzle terms, automatically paying the creator.
   - Licenses are bound to the specific terms version bought.
   - Revocation follows published terms only (`RevocationNotAllowed` if terms disallow).
   - Automated refunds upon revocation when terms require.
   - Expiry checks for time-limited licenses.
