# Mystery Box Contract with Pity Timer

Soroban smart contract implementing gamified mystery box openings with verifiable randomness and a guaranteed pity timer for rare rewards.

## Features

- **Weighted Reward Tiers:** Common, Uncommon, and Rare rewards configurable via weights.
- **Pity Timer Guarantee:** Tracks consecutive non-rare opens per player. Once the configurable threshold is reached, the next open is guaranteed to yield a Rare reward.
- **Pity Reset:** Any Rare reward (natural roll or pity-forced) resets the player's pity counter back to 0.
- **On-Chain Audit:** Emits structured Soroban events for each box opened, including player address, reward tier, and new pity counter.

## Interface

```rust
pub fn init(env: Env, admin: Address, pity_threshold: u32) -> Result<(), Error>;
pub fn get_admin(env: Env) -> Result<Address, Error>;
pub fn set_pity_threshold(env: Env, new_threshold: u32) -> Result<(), Error>;
pub fn set_weights(env: Env, common: u32, uncommon: u32, rare: u32) -> Result<(), Error>;
pub fn get_weights(env: Env) -> RewardWeights;
pub fn get_pity_threshold(env: Env) -> u32;
pub fn get_pity_counter(env: Env, player: Address) -> u32;
pub fn get_player_boxes_opened(env: Env, player: Address) -> u64;
pub fn get_total_boxes_opened(env: Env) -> u64;
pub fn open_box(env: Env, player: Address) -> Result<RewardTier, Error>;
```
