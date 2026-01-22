#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec};

//
// ──────────────────────────────────────────────────────────
// TIME PERIODS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimePeriod {
    Daily = 0,
    Weekly = 1,
    AllTime = 2,
}

//
// ──────────────────────────────────────────────────────────
// DATA KEYS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
pub enum DataKey {
    Config,                                    // LeaderboardConfig
    PlayerScore(Address, TimePeriod, u64),     // PlayerScore - (player, period, period_id)
    TopScores(TimePeriod, u64),                // Vec<PlayerScore> - sorted top scores for period
    PlayerAllTimeScore(Address),               // i128 - cumulative all-time score
    TotalPlayers,                              // u32
    HighScore(TimePeriod),                     // i128 - record high score per period type
    Verifier(Address),                         // bool - authorized score verifiers
}

//
// ──────────────────────────────────────────────────────────
// STRUCTS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
#[derive(Clone, Debug)]
pub struct LeaderboardConfig {
    pub admin: Address,
    pub max_top_entries: u32,      // Maximum entries in top-N list (gas optimization)
    pub daily_period_length: u64,  // Seconds (86400 for 24 hours)
    pub weekly_period_length: u64, // Seconds (604800 for 7 days)
    pub paused: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PlayerScore {
    pub player: Address,
    pub score: i128,
    pub timestamp: u64,
    pub period: TimePeriod,
    pub period_id: u64,
}

//
// ──────────────────────────────────────────────────────────
// CONSTANTS
// ──────────────────────────────────────────────────────────
//

const DEFAULT_DAILY_PERIOD: u64 = 86_400;     // 24 hours
const DEFAULT_WEEKLY_PERIOD: u64 = 604_800;   // 7 days
const DEFAULT_MAX_TOP_ENTRIES: u32 = 100;

//
// ──────────────────────────────────────────────────────────
// EVENTS
// ──────────────────────────────────────────────────────────
//

// Event symbols
const NEW_HIGH_SCORE: Symbol = symbol_short!("hi_score");
const SCORE_SUBMIT: Symbol = symbol_short!("submit");
const RANK_CHANGE: Symbol = symbol_short!("rank_chg");

//
// ──────────────────────────────────────────────────────────
// CONTRACT
// ──────────────────────────────────────────────────────────
//

#[contract]
pub struct LeaderboardContract;

#[contractimpl]
impl LeaderboardContract {
    // ───────────── INITIALIZATION ─────────────

    /// Initialize the leaderboard contract with configuration
    ///
    /// # Arguments
    /// * `admin` - Contract administrator
    /// * `max_top_entries` - Maximum number of entries in top-N lists (for gas efficiency)
    pub fn initialize(env: Env, admin: Address, max_top_entries: u32) {
        admin.require_auth();

        if env.storage().persistent().has(&DataKey::Config) {
            panic!("Already initialized");
        }

        let max_entries = if max_top_entries == 0 {
            DEFAULT_MAX_TOP_ENTRIES
        } else {
            max_top_entries
        };

        let config = LeaderboardConfig {
            admin,
            max_top_entries: max_entries,
            daily_period_length: DEFAULT_DAILY_PERIOD,
            weekly_period_length: DEFAULT_WEEKLY_PERIOD,
            paused: false,
        };

        env.storage().persistent().set(&DataKey::Config, &config);
        env.storage().persistent().set(&DataKey::TotalPlayers, &0u32);
    }

    // ───────────── ADMIN FUNCTIONS ─────────────

    /// Add an authorized score verifier (admin only)
    pub fn add_verifier(env: Env, admin: Address, verifier: Address) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        env.storage()
            .persistent()
            .set(&DataKey::Verifier(verifier), &true);
    }

    /// Remove an authorized score verifier (admin only)
    pub fn remove_verifier(env: Env, admin: Address, verifier: Address) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        env.storage()
            .persistent()
            .remove(&DataKey::Verifier(verifier));
    }

    /// Pause/unpause the contract (admin only)
    pub fn set_paused(env: Env, admin: Address, paused: bool) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut config: LeaderboardConfig =
            env.storage().persistent().get(&DataKey::Config).unwrap();
        config.paused = paused;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    /// Update period lengths (admin only)
    pub fn update_period_lengths(
        env: Env,
        admin: Address,
        daily_period_length: u64,
        weekly_period_length: u64,
    ) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut config: LeaderboardConfig =
            env.storage().persistent().get(&DataKey::Config).unwrap();
        config.daily_period_length = daily_period_length;
        config.weekly_period_length = weekly_period_length;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    /// Update max top entries (admin only)
    pub fn update_max_entries(env: Env, admin: Address, max_top_entries: u32) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        if max_top_entries == 0 {
            panic!("Max entries must be positive");
        }

        let mut config: LeaderboardConfig =
            env.storage().persistent().get(&DataKey::Config).unwrap();
        config.max_top_entries = max_top_entries;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    // ───────────── SCORE SUBMISSION ─────────────

    /// Submit a verified score for a player
    /// Can be called by admin or authorized verifier
    ///
    /// # Arguments
    /// * `submitter` - The admin or verifier submitting the score
    /// * `player` - The player who earned the score
    /// * `score` - The score value
    pub fn submit_score(env: Env, submitter: Address, player: Address, score: i128) {
        submitter.require_auth();
        Self::assert_not_paused(&env);
        Self::assert_authorized_submitter(&env, &submitter);

        if score < 0 {
            panic!("Score must be non-negative");
        }

        let config: LeaderboardConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let current_time = env.ledger().timestamp();

        // Calculate period IDs
        let daily_period_id = current_time / config.daily_period_length;
        let weekly_period_id = current_time / config.weekly_period_length;
        let all_time_period_id = 0u64; // All-time uses 0 as period ID

        // Update scores for each time period
        Self::update_period_score(&env, &config, &player, score, TimePeriod::Daily, daily_period_id, current_time);
        Self::update_period_score(&env, &config, &player, score, TimePeriod::Weekly, weekly_period_id, current_time);
        Self::update_period_score(&env, &config, &player, score, TimePeriod::AllTime, all_time_period_id, current_time);

        // Update cumulative all-time score
        let current_all_time: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::PlayerAllTimeScore(player.clone()))
            .unwrap_or(0);
        
        let is_new_player = current_all_time == 0;
        let new_all_time = current_all_time + score;
        
        env.storage()
            .persistent()
            .set(&DataKey::PlayerAllTimeScore(player.clone()), &new_all_time);

        // Update total players count if new player
        if is_new_player {
            let total: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::TotalPlayers)
                .unwrap_or(0);
            env.storage()
                .persistent()
                .set(&DataKey::TotalPlayers, &(total + 1));
        }

        // Emit score submission event
        env.events()
            .publish((SCORE_SUBMIT, player.clone()), (score, current_time));
    }

    /// Update a player's score directly (replace existing score)
    /// Admin only - useful for corrections
    pub fn update_score(
        env: Env,
        admin: Address,
        player: Address,
        score: i128,
        period: TimePeriod,
    ) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);
        Self::assert_not_paused(&env);

        if score < 0 {
            panic!("Score must be non-negative");
        }

        let config: LeaderboardConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let current_time = env.ledger().timestamp();

        let period_id = Self::get_current_period_id(&config, period, current_time);

        // Create or update player score
        let player_score = PlayerScore {
            player: player.clone(),
            score,
            timestamp: current_time,
            period,
            period_id,
        };

        env.storage().persistent().set(
            &DataKey::PlayerScore(player.clone(), period, period_id),
            &player_score,
        );

        // Update top scores list
        Self::update_top_scores_list(&env, &config, &player_score, period, period_id);
    }

    // ───────────── VIEW FUNCTIONS ─────────────

    /// Get top N players for a specific time period
    ///
    /// # Arguments
    /// * `period` - The time period (Daily, Weekly, AllTime)
    /// * `limit` - Maximum number of players to return
    pub fn get_top_players(env: Env, period: TimePeriod, limit: u32) -> Vec<PlayerScore> {
        let config: LeaderboardConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let current_time = env.ledger().timestamp();
        let period_id = Self::get_current_period_id(&config, period, current_time);

        let top_scores: Vec<PlayerScore> = env
            .storage()
            .persistent()
            .get(&DataKey::TopScores(period, period_id))
            .unwrap_or(Vec::new(&env));

        // Return limited results
        let actual_limit = if limit > config.max_top_entries {
            config.max_top_entries
        } else {
            limit
        };

        let mut result = Vec::new(&env);
        for i in 0..top_scores.len().min(actual_limit) {
            result.push_back(top_scores.get(i).unwrap());
        }

        result
    }

    /// Get a player's rank for a specific time period
    /// Returns 0 if player not found in top rankings
    ///
    /// # Arguments
    /// * `player` - Player address
    /// * `period` - The time period (Daily, Weekly, AllTime)
    pub fn get_player_rank(env: Env, player: Address, period: TimePeriod) -> u32 {
        let config: LeaderboardConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let current_time = env.ledger().timestamp();
        let period_id = Self::get_current_period_id(&config, period, current_time);

        let top_scores: Vec<PlayerScore> = env
            .storage()
            .persistent()
            .get(&DataKey::TopScores(period, period_id))
            .unwrap_or(Vec::new(&env));

        for i in 0..top_scores.len() {
            let score = top_scores.get(i).unwrap();
            if score.player == player {
                return (i + 1) as u32; // Rank is 1-indexed
            }
        }

        0 // Not ranked
    }

    /// Get a player's score for a specific time period
    pub fn get_player_score(env: Env, player: Address, period: TimePeriod) -> Option<PlayerScore> {
        let config: LeaderboardConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let current_time = env.ledger().timestamp();
        let period_id = Self::get_current_period_id(&config, period, current_time);

        env.storage()
            .persistent()
            .get(&DataKey::PlayerScore(player, period, period_id))
    }

    /// Get a player's cumulative all-time score
    pub fn get_player_all_time_total(env: Env, player: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::PlayerAllTimeScore(player))
            .unwrap_or(0)
    }

    /// Get the current high score record for a time period type
    pub fn get_high_score(env: Env, period: TimePeriod) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::HighScore(period))
            .unwrap_or(0)
    }

    /// Get total number of unique players
    pub fn get_total_players(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::TotalPlayers)
            .unwrap_or(0)
    }

    /// Get the current configuration
    pub fn get_config(env: Env) -> LeaderboardConfig {
        env.storage().persistent().get(&DataKey::Config).unwrap()
    }

    /// Check if an address is an authorized verifier
    pub fn is_verifier(env: Env, address: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Verifier(address))
            .unwrap_or(false)
    }

    /// Get the current period ID for a time period type
    pub fn get_current_period_id_view(env: Env, period: TimePeriod) -> u64 {
        let config: LeaderboardConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let current_time = env.ledger().timestamp();
        Self::get_current_period_id(&config, period, current_time)
    }

    // ───────────── INTERNAL HELPERS ─────────────

    fn get_current_period_id(config: &LeaderboardConfig, period: TimePeriod, current_time: u64) -> u64 {
        match period {
            TimePeriod::Daily => current_time / config.daily_period_length,
            TimePeriod::Weekly => current_time / config.weekly_period_length,
            TimePeriod::AllTime => 0,
        }
    }

    fn update_period_score(
        env: &Env,
        config: &LeaderboardConfig,
        player: &Address,
        score: i128,
        period: TimePeriod,
        period_id: u64,
        current_time: u64,
    ) {
        // Get existing score for this period
        let existing_score: Option<PlayerScore> = env
            .storage()
            .persistent()
            .get(&DataKey::PlayerScore(player.clone(), period, period_id));

        let new_score = match existing_score {
            Some(mut existing) => {
                existing.score += score;
                existing.timestamp = current_time;
                existing
            }
            None => PlayerScore {
                player: player.clone(),
                score,
                timestamp: current_time,
                period,
                period_id,
            },
        };

        // Save player's score
        env.storage().persistent().set(
            &DataKey::PlayerScore(player.clone(), period, period_id),
            &new_score,
        );

        // Update top scores list
        Self::update_top_scores_list(env, config, &new_score, period, period_id);

        // Check and update high score record
        let current_high: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::HighScore(period))
            .unwrap_or(0);

        if new_score.score > current_high {
            env.storage()
                .persistent()
                .set(&DataKey::HighScore(period), &new_score.score);

            // Emit new high score event
            env.events()
                .publish((NEW_HIGH_SCORE, period), (player.clone(), new_score.score));
        }
    }

    fn update_top_scores_list(
        env: &Env,
        config: &LeaderboardConfig,
        player_score: &PlayerScore,
        period: TimePeriod,
        period_id: u64,
    ) {
        let top_scores: Vec<PlayerScore> = env
            .storage()
            .persistent()
            .get(&DataKey::TopScores(period, period_id))
            .unwrap_or(Vec::new(env));

        // Remove existing entry for this player if present
        let mut new_list: Vec<PlayerScore> = Vec::new(env);
        let mut old_rank: u32 = 0;
        let mut index: u32 = 1;
        
        for existing in top_scores.iter() {
            if existing.player != player_score.player {
                new_list.push_back(existing);
            } else {
                old_rank = index;
            }
            index += 1;
        }

        // Insert new score in sorted position (descending order)
        let mut inserted = false;
        let mut final_list: Vec<PlayerScore> = Vec::new(env);
        let mut new_rank: u32 = 0;
        index = 1;

        for existing in new_list.iter() {
            if !inserted && player_score.score > existing.score {
                final_list.push_back(player_score.clone());
                new_rank = index;
                inserted = true;
                index += 1;
            }
            if final_list.len() < config.max_top_entries {
                final_list.push_back(existing);
            }
            index += 1;
        }

        // If not inserted yet and list not full, append
        if !inserted && final_list.len() < config.max_top_entries {
            final_list.push_back(player_score.clone());
            new_rank = final_list.len() as u32;
        }

        // Emit rank change event if rank changed
        if new_rank > 0 && new_rank != old_rank {
            env.events().publish(
                (RANK_CHANGE, player_score.player.clone()),
                (period, old_rank, new_rank),
            );
        }

        env.storage()
            .persistent()
            .set(&DataKey::TopScores(period, period_id), &final_list);
    }

    fn assert_admin(env: &Env, user: &Address) {
        let config: LeaderboardConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        if config.admin != *user {
            panic!("Admin only");
        }
    }

    fn assert_not_paused(env: &Env) {
        let config: LeaderboardConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        if config.paused {
            panic!("Contract is paused");
        }
    }

    fn assert_authorized_submitter(env: &Env, submitter: &Address) {
        let config: LeaderboardConfig = env.storage().persistent().get(&DataKey::Config).unwrap();

        // Admin is always authorized
        if config.admin == *submitter {
            return;
        }

        // Check if authorized verifier
        let is_verifier: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Verifier(submitter.clone()))
            .unwrap_or(false);

        if !is_verifier {
            panic!("Unauthorized submitter");
        }
    }
}

mod test;
