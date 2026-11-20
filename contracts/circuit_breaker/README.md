# Circuit Breaker (Guardian Quorum)

A reusable emergency pause module for Soroban contracts. Pausing is fast and
requires a quorum of independent guardians; unpausing is intentionally slow and
requires both a request and explicit admin approval.

## Design

| Concern | Behaviour |
| --- | --- |
| Guardian set | Configured at initialization, mutable by admin only |
| Quorum | Strict majority of the guardian set (`quorum <= N` and `quorum > N / 2`) |
| Pause | A guardian proposes (their approval counts), other guardians approve; the breaker opens as soon as `quorum` approvals are collected |
| Proposal lifetime | `pause_proposal_ttl` seconds; expired proposals cannot be approved |
| While paused | Only functions in the allowlist may run; all other calls fail with `FunctionNotCallable` |
| Unpause | A guardian/admin requests it, then the admin executes it only after `unpause_delay` seconds from the pause timestamp |
| Auditability | `PauseRecord` history and contract events for every state transition |

Defaults: `unpause_delay = 7200` (2 hours), `pause_proposal_ttl = 3600` (1 hour).
The default allowlist contains read-only queries plus the unpause flow; the
unpause functions cannot be removed from it.

## Integrating a contract

Add this crate as a dependency and call the exported guard at the start of every
state-changing entry point:

```rust
use circuit_breaker::enforce_not_paused;
use soroban_sdk::symbol_short;

pub fn deposit(env: Env, user: Address, amount: i128) {
    user.require_auth();
    let breaker: Address = /* stored breaker address */;
    enforce_not_paused(&env, &breaker, &symbol_short!("deposit"));
    // ... state changes
}
```

`enforce_not_paused` calls the breaker's `assert_callable`, which returns
`Ok(())` when the circuit is closed or when the function is explicitly
allowlisted while open. See `tests/integration.rs` for a complete example.

## Testing

```sh
cargo test
cargo build --release --target wasm32v1-none
```

Unit tests cover initialization/validation, quorum enforcement, proposal
expiry, allowlist behaviour, timelock enforcement and guardian management.
Integration tests deploy a mock vault that consumes this module and verify its
state changes are blocked while paused.
