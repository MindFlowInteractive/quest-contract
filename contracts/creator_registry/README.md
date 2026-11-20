# Creator Registry Contract

Soroban smart contract for registering puzzle creators with on-chain verification status for the Quest Service ecosystem.

## Features

- **Creator Registration:** Creators register their profile URI (IPFS hash or metadata pointer) once per address.
- **Admin Verification:** Platform admin can grant or revoke verified status with on-chain timestamping.
- **Composable Reads:** Exposes `is_registered`, `is_verified`, and `get_creator` for consumption by puzzle factories, leaderboards, and client frontends.
- **Event Auditing:** Emits structured Soroban events on registration, profile updates, verification changes, and admin transfers.

## Interface

```rust
pub fn init(env: Env, admin: Address) -> Result<(), Error>;
pub fn get_admin(env: Env) -> Result<Address, Error>;
pub fn set_admin(env: Env, new_admin: Address) -> Result<(), Error>;
pub fn register_creator(env: Env, creator: Address, profile_uri: String) -> Result<(), Error>;
pub fn update_profile(env: Env, creator: Address, new_profile_uri: String) -> Result<(), Error>;
pub fn set_verified(env: Env, creator: Address, verified: bool) -> Result<(), Error>;
pub fn is_registered(env: Env, creator: Address) -> bool;
pub fn is_verified(env: Env, creator: Address) -> bool;
pub fn get_creator(env: Env, creator: Address) -> Option<CreatorProfile>;
pub fn get_total_creators(env: Env) -> u32;
```
