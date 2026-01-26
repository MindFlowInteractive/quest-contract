#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec,
};

#[cfg(test)]
extern crate std;

// ─────────────────────────────────────────────────────────────
// Types & Storage Keys
// ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tier {
    Bronze,
    Silver,
    Gold,
    Platinum,
    Diamond,
    Master,
    Grandmaster,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerRating {
    pub rating: i32,
    pub last_update_ts: u64,
    pub season_id: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntry {
    pub timestamp: u64,
    pub delta: i32,
    pub new_rating: i32,
    pub difficulty: u32,
    pub result_permill: i32,
    pub expected_permill: i32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
    pub base_rating: i32,            // e.g. 1000
    pub k_factor: i32,               // e.g. 32
    pub decay_period_s: u64,         // e.g. 7d
    pub decay_rate_ppm: u32,         // parts-per-million per period (e.g. 5000 = 0.5%)
    pub season_length_s: u64,        // e.g. 90d
    pub season_reset_drop: i32,      // reduce rating by fixed amount at season reset, floor at base
    pub history_limit: u32,          // store up to N entries per player
    pub difficulty_scale_ppm: u32,   // scales K by difficulty in ppm (1000000 = 1x per difficulty unit)
}

#[contracttype]
pub enum DataKey {
    Config,
    Player(Address),          // PlayerRating
    History(Address),         // Vec<HistoryEntry>
}

// ─────────────────────────────────────────────────────────────
// Errors & Events
// ─────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidParams = 4,
}

const EVT_ADJUST: Symbol = symbol_short!("adj");
const EVT_SEASON: Symbol = symbol_short!("season");

// ─────────────────────────────────────────────────────────────
// Contract
// ─────────────────────────────────────────────────────────────

#[contract]
pub struct SkillRating;

#[contractimpl]
impl SkillRating {
    // Initialization
    pub fn initialize(
        env: Env,
        admin: Address,
        base_rating: i32,
        k_factor: i32,
        decay_period_s: u64,
        decay_rate_ppm: u32,
        season_length_s: u64,
        season_reset_drop: i32,
        history_limit: u32,
        difficulty_scale_ppm: u32,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Config) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        if base_rating <= 0 || k_factor <= 0 || history_limit == 0 {
            return Err(Error::InvalidParams);
        }
        let cfg = Config {
            admin,
            base_rating,
            k_factor,
            decay_period_s,
            decay_rate_ppm,
            season_length_s,
            season_reset_drop,
            history_limit,
            difficulty_scale_ppm,
        };
        env.storage().instance().set(&DataKey::Config, &cfg);
        Ok(())
    }

    // Admin update config knobs
    pub fn update_config(env: Env, admin: Address, cfg: Config) -> Result<(), Error> {
        admin.require_auth();
        let current: Config = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(Error::NotInitialized)?;
        if current.admin != admin {
            return Err(Error::Unauthorized);
        }
        env.storage().instance().set(&DataKey::Config, &cfg);
        Ok(())
    }

    // Core rating adjustment API
    // result_permill and expected_permill are in [0,1000]
    pub fn adjust_after_puzzle(
        env: Env,
        player: Address,
        difficulty: u32,
        result_permill: i32,
        expected_permill: i32,
    ) -> Result<i32, Error> {
        player.require_auth();
        if !(0..=1000).contains(&result_permill) || !(0..=1000).contains(&expected_permill) {
            return Err(Error::InvalidParams);
        }

        let mut rating = Self::get_or_init_rating(&env, &player)?;
        let cfg = Self::cfg(&env)?;

        // Apply season reset and decay before adjustment
        Self::maybe_apply_season_reset(&env, &cfg, &player, &mut rating);
        Self::maybe_apply_decay(&env, &cfg, &mut rating);

        // ELO delta = K * diff_scale * (res - exp)
        let k = cfg.k_factor as i64;
        // difficulty factor: 1 + difficulty * difficulty_scale_ppm/1e6
        let diff_scale_ppm = 1_000_000i64
            + (difficulty as i64) * (cfg.difficulty_scale_ppm as i64);
        let res_minus_exp_perm = (result_permill as i64) - (expected_permill as i64); // -1000..1000
        let delta_i64 = k * diff_scale_ppm * res_minus_exp_perm / 1_000_000 / 1000;
        let delta = delta_i64 as i32;

        // Update rating, clamp min at base
        let mut new_rating = rating.rating.saturating_add(delta);
        if new_rating < cfg.base_rating {
            new_rating = cfg.base_rating;
        }
        rating.rating = new_rating;
        rating.last_update_ts = env.ledger().timestamp();

        // Persist
        env.storage()
            .persistent()
            .set(&DataKey::Player(player.clone()), &rating);

        // Push history entry (bounded)
        let mut hist: Vec<HistoryEntry> = env
            .storage()
            .persistent()
            .get(&DataKey::History(player.clone()))
            .unwrap_or(Vec::new(&env));
        let entry = HistoryEntry {
            timestamp: rating.last_update_ts,
            delta,
            new_rating,
            difficulty,
            result_permill,
            expected_permill,
        };
        hist.push_back(entry);
        while hist.len() > cfg.history_limit {
            // drop oldest
            let mut tmp: Vec<HistoryEntry> = Vec::new(&env);
            for i in 1..hist.len() {
                tmp.push_back(hist.get(i).unwrap());
            }
            hist = tmp;
        }
        env.storage()
            .persistent()
            .set(&DataKey::History(player.clone()), &hist);

        env.events()
            .publish((EVT_ADJUST, player), (delta, new_rating, difficulty));

        Ok(new_rating)
    }

    // Views
    pub fn get_rating(env: Env, player: Address) -> Result<PlayerRating, Error> {
        let mut rating = Self::get_or_init_rating(&env, &player)?;
        let cfg = Self::cfg(&env)?;
        // Apply passive updates for accurate view
        Self::maybe_apply_season_reset(&env, &cfg, &player, &mut rating);
        Self::maybe_apply_decay(&env, &cfg, &mut rating);
        env.storage()
            .persistent()
            .set(&DataKey::Player(player), &rating);
        Ok(rating)
    }

    pub fn get_history(env: Env, player: Address) -> Vec<HistoryEntry> {
        env.storage()
            .persistent()
            .get(&DataKey::History(player))
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_config(env: Env) -> Config {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .expect("not init")
    }

    pub fn get_tier(env: Env, player: Address) -> Result<(Tier, u32), Error> {
        let rating = Self::get_rating(env, player)?.rating;
        Ok(Self::tier_for(rating))
    }

    pub fn get_current_season_id(env: Env) -> Result<u64, Error> {
        let cfg = Self::cfg(&env)?;
        Ok(Self::season_id_for(env.ledger().timestamp(), cfg.season_length_s))
    }

    // ────────────────────────────────
    // Internal helpers
    // ────────────────────────────────

    fn cfg(env: &Env) -> Result<Config, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(Error::NotInitialized)
    }

    fn get_or_init_rating(env: &Env, player: &Address) -> Result<PlayerRating, Error> {
        let cfg = Self::cfg(env)?;
        let season_id = Self::season_id_for(env.ledger().timestamp(), cfg.season_length_s);
        let pr: Option<PlayerRating> = env.storage().persistent().get(&DataKey::Player(player.clone()));
        Ok(pr.unwrap_or(PlayerRating {
            rating: cfg.base_rating,
            last_update_ts: env.ledger().timestamp(),
            season_id,
        }))
    }

    fn season_id_for(now: u64, season_len: u64) -> u64 {
        if season_len == 0 {
            0
        } else {
            now / season_len
        }
    }

    fn maybe_apply_season_reset(env: &Env, cfg: &Config, player: &Address, pr: &mut PlayerRating) {
        let current_season = Self::season_id_for(env.ledger().timestamp(), cfg.season_length_s);
        if pr.season_id != current_season {
            // Apply drop and floor at base
            let mut new_rating = pr.rating.saturating_sub(cfg.season_reset_drop);
            if new_rating < cfg.base_rating {
                new_rating = cfg.base_rating;
            }
            pr.rating = new_rating;
            pr.season_id = current_season;
            pr.last_update_ts = env.ledger().timestamp();

            env.events().publish((EVT_SEASON, player.clone()), (current_season, new_rating));
        }
    }

    fn maybe_apply_decay(env: &Env, cfg: &Config, pr: &mut PlayerRating) {
        if cfg.decay_period_s == 0 || cfg.decay_rate_ppm == 0 {
            return;
        }
        let now = env.ledger().timestamp();
        if now <= pr.last_update_ts {
            return;
        }
        let elapsed = now - pr.last_update_ts;
        if elapsed < cfg.decay_period_s {
            return;
        }
        let periods = elapsed / cfg.decay_period_s;
        if periods == 0 {
            return;
        }
        // Apply multiplicative decay toward base: rating -= max(1, (rating-base)*ppm/1e6) per period
        for _ in 0..periods {
            let over = (pr.rating as i64) - (cfg.base_rating as i64);
            if over > 0 {
                let dec = (over * (cfg.decay_rate_ppm as i64)) / 1_000_000;
                let dec = if dec <= 0 { 1 } else { dec } as i32;
                pr.rating = pr.rating.saturating_sub(dec);
            } else {
                break;
            }
        }
        pr.last_update_ts = now;
    }

    fn tier_for(rating: i32) -> (Tier, u32) {
        // Example thresholds
        // Bronze <1000, Silver 1000-1199, Gold 1200-1399, Platinum 1400-1599,
        // Diamond 1600-1799, Master 1800-1999, Grandmaster 2000+
        // Divisions 5 (5 highest), computed by position within tier band.
        let (tier, floor, ceil) = if rating < 1000 {
            (Tier::Bronze, 0, 1000)
        } else if rating < 1200 {
            (Tier::Silver, 1000, 1200)
        } else if rating < 1400 {
            (Tier::Gold, 1200, 1400)
        } else if rating < 1600 {
            (Tier::Platinum, 1400, 1600)
        } else if rating < 1800 {
            (Tier::Diamond, 1600, 1800)
        } else if rating < 2000 {
            (Tier::Master, 1800, 2000)
        } else {
            (Tier::Grandmaster, 2000, 2400) // open-ended; use 2400 band for division calc
        };

        let bands = 5i32;
        let span = (ceil - floor) as i32;
        let pos = (rating - floor as i32).clamp(0, span);
        let mut division = bands - (pos * bands / span.max(1)); // 5..1 (higher rating -> lower division number)
        if division < 1 {
            division = 1;
        }
        (tier, division as u32)
    }
}

// ─────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Env};

    fn setup(env: &Env) -> (SkillRatingClient<'_>, Address) {
        env.mock_all_auths();
        let admin = Address::generate(env);
        let id = env.register_contract(None, SkillRating);
        let client = SkillRatingClient::new(env, &id);
        // Initialize with sane defaults
        client.initialize(
            &admin,
            &1000i32,
            &32i32,
            &(7 * 24 * 60 * 60u64),
            &5_000u32, // 0.5% per period
            &(90 * 24 * 60 * 60u64),
            &100i32,
            &10u32,
            &0u32, // no difficulty scaling by default
        );
        (client, admin)
    }

    #[test]
    fn test_initialize_once() {
        let env = Env::default();
        let (c, admin) = setup(&env);
        // Try again on the same contract should fail
        let err = c.try_initialize(&admin, &1000, &32, &1, &1000, &100, &50, &10, &0);
        assert_eq!(err, Err(Ok(Error::AlreadyInitialized)));
    }

    #[test]
    fn test_adjust_basic_elo() {
        let env = Env::default();
        let (client, _admin) = setup(&env);
        let player = Address::generate(&env);
        env.ledger().set_timestamp(1000);

        // Equal expectations, win (1000 permill vs 500 expected)
        let new_rating = client.adjust_after_puzzle(&player, &1u32, &1000i32, &500i32);
        assert!(new_rating > 1000);

        // Loss (0 vs 500 expected)
        let new_rating2 = client.adjust_after_puzzle(&player, &1u32, &0i32, &500i32);
        assert!(new_rating2 < new_rating);
    }

    #[test]
    fn test_difficulty_scaling() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        // Turn on scaling: +10% K per difficulty level
        let mut cfg = client.get_config();
        cfg.difficulty_scale_ppm = 100_000; // 0.1 per level
        client.update_config(&admin, &cfg);

        let player = Address::generate(&env);
        env.ledger().set_timestamp(1000);
        let r1 = client.adjust_after_puzzle(&player, &1u32, &1000, &500);
        env.ledger().with_mut(|li| li.timestamp += 10);
        let r2 = client.adjust_after_puzzle(&player, &5u32, &1000, &500);
        assert!(r2 - r1 > 0); // higher difficulty increases gain on win
    }

    #[test]
    fn test_decay() {
        let env = Env::default();
        let (client, _admin) = setup(&env);
        let player = Address::generate(&env);

        env.ledger().set_timestamp(0);
        let _ = client.adjust_after_puzzle(&player, &1u32, &1000, &500);
        let r_now = client.get_rating(&player);

        // Advance > one decay period
        env.ledger().with_mut(|li| li.timestamp += 8 * 24 * 60 * 60);
        let r_after = client.get_rating(&player);
        assert!(r_after.rating <= r_now.rating);
    }

    #[test]
    fn test_season_reset() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        // Shorten season to 1 day
        let mut cfg = client.get_config();
        cfg.season_length_s = 86_400;
        cfg.season_reset_drop = 100;
        client.update_config(&admin, &cfg);

        let player = Address::generate(&env);
        env.ledger().set_timestamp(0);
        let _ = client.adjust_after_puzzle(&player, &1u32, &1000, &500);
        let before = client.get_rating(&player);

        // Move to next season
        env.ledger().set_timestamp(100_000);
        let after = client.get_rating(&player);
        assert!(after.season_id > before.season_id);
        assert!(after.rating <= before.rating);
    }

    #[test]
    fn test_tier_division() {
        let env = Env::default();
        let (client, _admin) = setup(&env);
        let player = Address::generate(&env);
        let (tier, _div) = client.get_tier(&player);
        assert_eq!(tier, Tier::Silver); // base 1000 -> Silver
    }

    #[test]
    fn test_history_bounded() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        let mut cfg = client.get_config();
        cfg.history_limit = 3;
        client.update_config(&admin, &cfg);

        let player = Address::generate(&env);
        env.ledger().set_timestamp(0);
        for i in 0..5 {
            let _ = client.adjust_after_puzzle(&player, &1u32, &if i%2==0 {1000} else {0}, &500);
            env.ledger().with_mut(|li| li.timestamp += 1);
        }
        let hist = client.get_history(&player);
        assert_eq!(hist.len(), 3);
    }
}
