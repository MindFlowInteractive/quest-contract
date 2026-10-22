# Streak reward pool

Implements #367 in a new `streak_counter` contract. At implementation time,
the prerequisite #365/#366 contract was absent from `main`. This includes the
minimal ledger-time streak foundation needed to exercise real reward eligibility.
It does **not** implement freeze-token issuance/consumption or claim to close
#366. Its integration with that future stage requires an explicit review of the
streak update rules and storage layout.

## Terms and trust

`initialize(admin, verifier, token, expires_at, milestones)` requires admin
authorization and runs once. `milestones` is a map of positive streak-day
thresholds to positive token amounts (base units), with at most 32 entries.
For example `{7: 70, 30: 300, 100: 1000}`. Terms and token cannot be changed.
Use a trusted Stellar Asset Contract token and initialize atomically with
deployment to prevent another party from initializing an unconfigured instance.

`verifier` is the trusted puzzle-completion authority: a backend signer or puzzle
verification contract. Only its authorization can call `check_in(player)`.
The verifier must validate a completed puzzle before authorizing the call. Player
self-attestation does not establish eligibility. This PR does not wire an existing
puzzle contract to the new entry point or deploy to a network.

Days are `ledger.timestamp / 86400` (UTC). First check-in counts as one, including
day zero; same-day repeats do not increment. A consecutive day increments, and a
gap resets the current streak to one. `best` retains achieved milestones after a
reset, so rewards already earned can still be claimed until expiry. `get_streak`
returns the last recorded state; an idle streak resets on its next check-in.

## Funding and claims

- `fund(from, amount)` transfers positive amounts from an authenticated sponsor
  into the contract. The admin can top up through the same method before expiry.
- `claim(player, milestone)` requires player authorization and a best streak at
  least that threshold. It transfers the configured reward directly to the player.
  Each player/threshold is claimable once for the lifetime of this pool, including
  after streak resets. Different thresholds are cumulative, not mutually exclusive.
- The actual token balance is checked before each payout. Underfunding leaves the
  milestone unclaimed so it can be retried after a top-up. Transfer failures roll
  back the claim marker and token state atomically.
- At `timestamp >= expires_at`, claims, check-ins and deposits close, and only the
  admin can `withdraw(amount)` to its own address. Partial withdrawals are allowed.
  Unclaimed eligibility is not reserved liquidity; users compete for the funded
  balance until expiry. Admin cannot withdraw before expiry.
- `balance`, `get_config`, `get_streak` and `has_claimed` expose pool state.
  `check_in`, `fund`, `claim` and `withdraw` events expose successful changes.

Configuration uses instance storage; streaks and claim markers use persistent
storage. Access extends TTL up to the network maximum. Operators must maintain
instance lifetime and restore archived persistent entries when necessary; never
replace archived claim records with fresh state. There is no reset/reinitialization
entry point for spent claims.

## Verification

From the workspace: `cargo test -p streak-counter`.

The focused GitHub workflow copies this self-contained crate outside the workspace
so unrelated contracts with independently versioned dependencies do not affect
its dependency resolution. To reproduce that isolation:

```sh
cp -R contracts/streak_counter /tmp/streak-counter
cd /tmp/streak-counter
cargo fmt -- --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release --target wasm32v1-none
```

Requires Rust and the `wasm32v1-none` target. Tests use the Soroban host and a real
test Stellar Asset Contract for balances and rollback; no live funds are used.
The crate-local lockfile pins a compatible SDK 21 dependency graph, including
`ed25519-dalek` 2.2.0: the host's broad range otherwise selects v3 with an
incompatible RNG interface. Cargo ignores this nested lockfile for workspace-wide
commands, so use the isolated commands above for the reproducible focused check.
