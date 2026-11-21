#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, Env,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    InvalidThreshold = 3,
    InvalidWeights = 4,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RewardTier {
    Common = 1,
    Uncommon = 2,
    Rare = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RewardWeights {
    pub common: u32,
    pub uncommon: u32,
    pub rare: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    PityThreshold,
    Weights,
    PityCounter(Address),
    BoxesOpened(Address),
    TotalBoxesOpened,
    Nonce,
}

#[contract]
pub struct MysteryBoxContract;

#[contractimpl]
impl MysteryBoxContract {
    /// Initialize with admin and default pity threshold
    pub fn init(env: Env, admin: Address, pity_threshold: u32) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        if pity_threshold == 0 {
            return Err(Error::InvalidThreshold);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::PityThreshold, &pity_threshold);

        // Default weights: Common = 70, Uncommon = 25, Rare = 5
        let weights = RewardWeights {
            common: 70,
            uncommon: 25,
            rare: 5,
        };
        env.storage().instance().set(&DataKey::Weights, &weights);
        env.storage().instance().set(&DataKey::TotalBoxesOpened, &0u64);
        env.storage().instance().set(&DataKey::Nonce, &0u64);

        Ok(())
    }

    /// Read admin
    pub fn get_admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    /// Set new pity threshold (admin only)
    pub fn set_pity_threshold(env: Env, new_threshold: u32) -> Result<(), Error> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        if new_threshold == 0 {
            return Err(Error::InvalidThreshold);
        }

        env.storage().instance().set(&DataKey::PityThreshold, &new_threshold);
        env.events().publish(
            (symbol_short!("box"), symbol_short!("pity_th")),
            new_threshold,
        );
        Ok(())
    }

    /// Set custom reward weights (admin only)
    pub fn set_weights(
        env: Env,
        common: u32,
        uncommon: u32,
        rare: u32,
    ) -> Result<(), Error> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        if common == 0 && uncommon == 0 && rare == 0 {
            return Err(Error::InvalidWeights);
        }

        let weights = RewardWeights {
            common,
            uncommon,
            rare,
        };
        env.storage().instance().set(&DataKey::Weights, &weights);
        Ok(())
    }

    /// Get current weights
    pub fn get_weights(env: Env) -> RewardWeights {
        env.storage()
            .instance()
            .get(&DataKey::Weights)
            .unwrap_or(RewardWeights {
                common: 70,
                uncommon: 25,
                rare: 5,
            })
    }

    /// Get pity threshold
    pub fn get_pity_threshold(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::PityThreshold)
            .unwrap_or(10)
    }

    /// Get player's current pity counter
    pub fn get_pity_counter(env: Env, player: Address) -> u32 {
        let key = DataKey::PityCounter(player);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    /// Get player's total boxes opened
    pub fn get_player_boxes_opened(env: Env, player: Address) -> u64 {
        let key = DataKey::BoxesOpened(player);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    /// Get total boxes opened globally
    pub fn get_total_boxes_opened(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::TotalBoxesOpened)
            .unwrap_or(0)
    }

    /// Open a mystery box.
    /// Returns the reward tier obtained.
    pub fn open_box(env: Env, player: Address) -> Result<RewardTier, Error> {
        player.require_auth();

        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::NotInitialized);
        }

        let pity_key = DataKey::PityCounter(player.clone());
        let current_pity: u32 = env.storage().persistent().get(&pity_key).unwrap_or(0);
        let pity_threshold = Self::get_pity_threshold(env.clone());

        // Increment nonce
        let mut nonce: u64 = env.storage().instance().get(&DataKey::Nonce).unwrap_or(0);
        nonce += 1;
        env.storage().instance().set(&DataKey::Nonce, &nonce);

        // Check if pity threshold reached
        let (awarded_tier, new_pity) = if current_pity + 1 >= pity_threshold {
            // Guaranteed Rare reward!
            (RewardTier::Rare, 0u32)
        } else {
            // Roll using crypto sha256 pseudo-randomness
            let weights = Self::get_weights(env.clone());
            let total_weight = weights.common + weights.uncommon + weights.rare;

            let rolled_val = Self::generate_random_value(&env, nonce, total_weight as u64) as u32;

            if rolled_val < weights.common {
                (RewardTier::Common, current_pity + 1)
            } else if rolled_val < weights.common + weights.uncommon {
                (RewardTier::Uncommon, current_pity + 1)
            } else {
                // Natural Rare reward: resets pity counter
                (RewardTier::Rare, 0u32)
            }
        };

        // Persist new pity
        env.storage().persistent().set(&pity_key, &new_pity);

        // Update player boxes count
        let box_key = DataKey::BoxesOpened(player.clone());
        let player_boxes: u64 = env.storage().persistent().get(&box_key).unwrap_or(0);
        env.storage().persistent().set(&box_key, &(player_boxes + 1));

        // Update global boxes count
        let total_boxes = Self::get_total_boxes_opened(env.clone());
        env.storage()
            .instance()
            .set(&DataKey::TotalBoxesOpened, &(total_boxes + 1));

        // Emit audit event
        env.events().publish(
            (symbol_short!("box"), symbol_short!("opened")),
            (player, awarded_tier as u32, new_pity),
        );

        Ok(awarded_tier)
    }

    /// Internal pseudo-random number generator bounded by max_bound
    fn generate_random_value(env: &Env, nonce: u64, max_bound: u64) -> u64 {
        if max_bound == 0 {
            return 0;
        }
        let seq = env.ledger().sequence() as u64;
        let ts = env.ledger().timestamp();

        let mut payload = [0u8; 24];
        payload[..8].copy_from_slice(&seq.to_be_bytes());
        payload[8..16].copy_from_slice(&ts.to_be_bytes());
        payload[16..24].copy_from_slice(&nonce.to_be_bytes());

        let hash = env.crypto().sha256(&Bytes::from_array(env, &payload));
        let bytes = hash.to_array();
        let rand_num = u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        rand_num % max_bound
    }
}

#[cfg(test)]
mod test;
