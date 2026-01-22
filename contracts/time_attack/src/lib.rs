#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

#[cfg(test)]
extern crate std;

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scope {
    /// Global leaderboard across all puzzles
    Global,
    /// Per-puzzle leaderboard for a specific puzzle ID
    Puzzle(u32),
}

/// Time window used for leaderboard aggregation/reset.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimePeriod {
    Daily,
    Weekly,
    AllTime,
}

#[contracttype]
pub enum DataKey {
    Admin,
    LastSubmit(Address),
    ReplayUsed(BytesN<32>),
    Best(Scope, TimePeriod),
    Board(Scope, TimePeriod),
    LastReset(Scope, TimePeriod),
}

/// Custom error codes for the contract
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotAuthorized = 2,
    InvalidTime = 3,
    TooFrequent = 4,
    DuplicateReplay = 5,
    ContractNotInitialized = 6,
    // NOTE(MVP): `InvalidPuzzleId` intentionally omitted until puzzle-id validation rules are defined.
}

/// A single player completion record for a puzzle run submission.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeRecord {
    pub player: Address,
    pub completion_time_ms: u64,
    pub timestamp: u64, // ledger timestamp (seconds)
    pub replay_hash: BytesN<32>,
}

/// Pure logic classification for future "time bracket competitions".
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeBracket {
    Beginner,
    Intermediate,
    Advanced,
    Expert,
}

#[contract]
pub struct TimeAttack;

const LEDGER_THRESHOLD_SHARED: u32 = 518_400; // ~30 days @ 5s/ledger
const LEDGER_BUMP_SHARED: u32 = 1_036_800; // ~60 days @ 5s/ledger

#[contractimpl]
impl TimeAttack {
    fn bump_persistent_ttl(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, LEDGER_THRESHOLD_SHARED, LEDGER_BUMP_SHARED);
    }

    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        let storage = env.storage().instance();

        if storage.has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }

        // Ensure the provided admin authorizes being set as admin
        admin.require_auth();

        // Store the admin address in contract storage
        storage.set(&DataKey::Admin, &admin);

        Ok(())
    }

    /// Submit a puzzle completion time
    ///
    /// # Arguments
    /// * `env` - Contract environment
    /// * `player` - Address of the player submitting the time
    /// * `puzzle_id` - ID of the puzzle completed (0 for global)
    /// * `completion_time_ms` - Completion time in milliseconds
    /// * `replay_hash` - Hash of the replay data for verification
    ///
    /// # Returns
    /// * `Ok(())` - Submission successful
    /// * `Err(Error)` - Submission failed validation
    ///
    /// # Errors
    /// - `InvalidTime`: Completion time is 0 or unreasonably high
    /// - `TooFrequent`: Player submitted too recently (rate limiting)
    /// - `DuplicateReplay`: Replay hash has been used before
    pub fn submit_time(
        env: Env,
        player: Address,
        puzzle_id: u32,
        completion_time_ms: u64,
        replay_hash: BytesN<32>,
    ) -> Result<(), Error> {
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::ContractNotInitialized);
        }

        // Require authentication from the player
        player.require_auth();

        // Get current ledger timestamp (seconds)
        let timestamp = env.ledger().timestamp();

        // Validate the submission
        Self::verify_submission(&env, &player, completion_time_ms, &replay_hash, timestamp)?;

        // Create the time record
        let record = TimeRecord {
            player: player.clone(),
            completion_time_ms,
            timestamp,
            replay_hash: replay_hash.clone(),
        };

        // Determine the scope based on puzzle_id
        let scope = if puzzle_id == 0 {
            Scope::Global
        } else {
            Scope::Puzzle(puzzle_id)
        };

        // Check and reset leaderboards if needed (daily/weekly)
        Self::check_and_reset_leaderboards(&env, scope, timestamp);

        // Update leaderboards for all time periods
        Self::update_leaderboard(&env, scope, TimePeriod::AllTime, &record)?;
        Self::update_leaderboard(&env, scope, TimePeriod::Daily, &record)?;
        Self::update_leaderboard(&env, scope, TimePeriod::Weekly, &record)?;

        // Update all-time best for this scope (global or per-puzzle)
        Self::update_alltime_best(&env, scope, &record);

        // Mark this submission timestamp for rate limiting (temporary storage)
        env.storage()
            .temporary()
            .set(&DataKey::LastSubmit(player.clone()), &timestamp);
        env.storage().temporary().extend_ttl(
            &DataKey::LastSubmit(player.clone()),
            300, // threshold (seconds)
            300, // extend_to (seconds)
        );

        // Mark replay hash as used (temporary storage, expires after 24 hours)
        env.storage()
            .temporary()
            .set(&DataKey::ReplayUsed(replay_hash.clone()), &true);
        env.storage().temporary().extend_ttl(
            &DataKey::ReplayUsed(replay_hash),
            86400, // threshold (seconds)
            86400, // extend_to (seconds)
        );

        // Emit event for off-chain indexing (optional but recommended)
        env.events().publish(
            (symbol_short!("TIME_SUB"), player),
            (completion_time_ms, timestamp, puzzle_id),
        );

        Ok(())
    }

    fn verify_submission(
        env: &Env,
        player: &Address,
        completion_time_ms: u64,
        replay_hash: &BytesN<32>,
        timestamp: u64,
    ) -> Result<(), Error> {
        const MIN_REASONABLE_TIME_MS: u64 = 1_000; // 1s
        const MAX_REASONABLE_TIME_MS: u64 = 60 * 60 * 1000; // 1h
        const MIN_SUBMIT_INTERVAL_S: u64 = 5;

        if !(MIN_REASONABLE_TIME_MS..=MAX_REASONABLE_TIME_MS).contains(&completion_time_ms) {
            return Err(Error::InvalidTime);
        }

        if let Some(last) = env
            .storage()
            .temporary()
            .get::<_, u64>(&DataKey::LastSubmit(player.clone()))
        {
            if timestamp.saturating_sub(last) < MIN_SUBMIT_INTERVAL_S {
                return Err(Error::TooFrequent);
            }
        }

        if let Some(true) = env
            .storage()
            .temporary()
            .get::<_, bool>(&DataKey::ReplayUsed(replay_hash.clone()))
        {
            return Err(Error::DuplicateReplay);
        }

        Ok(())
    }

    fn check_and_reset_leaderboards(env: &Env, scope: Scope, current_timestamp: u64) {
        Self::maybe_reset_period(env, scope, TimePeriod::Daily, 86_400, current_timestamp);
        Self::maybe_reset_period(env, scope, TimePeriod::Weekly, 604_800, current_timestamp);
    }

    fn maybe_reset_period(
        env: &Env,
        scope: Scope,
        period: TimePeriod,
        duration_seconds: u64,
        current_timestamp: u64,
    ) {
        let last_reset_key = DataKey::LastReset(scope, period);

        // Init-on-first-use: first time we see this scope/period, record a baseline and exit.
        let last_reset_opt: Option<u64> = env.storage().persistent().get(&last_reset_key);
        let last_reset = match last_reset_opt {
            Some(t) => t,
            None => {
                env.storage()
                    .persistent()
                    .set(&last_reset_key, &current_timestamp);
                Self::bump_persistent_ttl(env, &last_reset_key);
                return;
            }
        };

        // Use saturating_sub to avoid underflow in weird timestamp scenarios.
        if current_timestamp.saturating_sub(last_reset) >= duration_seconds {
            // Clear the leaderboard.
            let board_key = DataKey::Board(scope, period);
            env.storage()
                .persistent()
                .set(&board_key, &Vec::<TimeRecord>::new(env));
            Self::bump_persistent_ttl(env, &board_key);

            // Remove best record for this period.
            let best_key = DataKey::Best(scope, period);
            env.storage().persistent().remove(&best_key);

            // Update last reset timestamp.
            env.storage()
                .persistent()
                .set(&last_reset_key, &current_timestamp);
            Self::bump_persistent_ttl(env, &last_reset_key);

            env.events()
                .publish((symbol_short!("LB_RESET"), scope, period), current_timestamp);
        }
    }

    fn update_leaderboard(
        env: &Env,
        scope: Scope,
        period: TimePeriod,
        new_record: &TimeRecord,
    ) -> Result<(), Error> {
        const MAX_LEADERBOARD_SIZE: u32 = 10;

        let board_key = DataKey::Board(scope, period);

        // Get current leaderboard or create empty one
        let mut leaderboard: Vec<TimeRecord> = env
            .storage()
            .persistent()
            .get(&board_key)
            .unwrap_or(Vec::new(env));

        // Insert record in sorted order (fastest time first)
        let mut inserted = false;
        for i in 0..leaderboard.len() {
            if let Some(existing) = leaderboard.get(i) {
                if new_record.completion_time_ms < existing.completion_time_ms {
                    leaderboard.insert(i, new_record.clone());
                    inserted = true;
                    break;
                }
            }
        }

        // If not inserted and board has room, add to end
        if !inserted && leaderboard.len() < MAX_LEADERBOARD_SIZE {
            leaderboard.push_back(new_record.clone());
        }

        // Trim to max size
        while leaderboard.len() > MAX_LEADERBOARD_SIZE {
            leaderboard.pop_back();
        }

        env.storage().persistent().set(&board_key, &leaderboard);
        Self::bump_persistent_ttl(env, &board_key);

        Ok(())
    }

    // Renamed from `update_global_best` for clarity: this is per-scope all-time best.
    fn update_alltime_best(env: &Env, scope: Scope, record: &TimeRecord) {
        let best_key = DataKey::Best(scope, TimePeriod::AllTime);

        let current_best: Option<TimeRecord> = env.storage().persistent().get(&best_key);

        let should_update = match current_best {
            None => true,
            Some(best) => record.completion_time_ms < best.completion_time_ms,
        };

        if should_update {
            env.storage().persistent().set(&best_key, record);
            Self::bump_persistent_ttl(env, &best_key);

            env.events().publish(
                (symbol_short!("NEW_BEST"), scope),
                (record.player.clone(), record.completion_time_ms),
            );
        }
    }

    /// Get the best time for a scope
    ///
    /// # Arguments
    /// * `env` - Contract environment
    /// * `puzzle_id` - Puzzle ID (0 for global)
    ///
    /// # Returns
    /// Best time record, or None if no records exist
    pub fn get_best_time(env: Env, puzzle_id: u32) -> Option<TimeRecord> {
        let scope = if puzzle_id == 0 {
            Scope::Global
        } else {
            Scope::Puzzle(puzzle_id)
        };

        let best_key = DataKey::Best(scope, TimePeriod::AllTime);
        env.storage().persistent().get(&best_key)
    }

    /// Get leaderboard for a specific scope and period
    ///
    /// # Arguments
    /// * `env` - Contract environment
    /// * `puzzle_id` - Puzzle ID (0 for global)
    /// * `period` - Time period (Daily/Weekly/AllTime)
    ///
    /// # Returns
    /// Vector of time records, ordered by fastest time
    pub fn get_leaderboard(env: Env, puzzle_id: u32, period: TimePeriod) -> Vec<TimeRecord> {
        let scope = if puzzle_id == 0 {
            Scope::Global
        } else {
            Scope::Puzzle(puzzle_id)
        };

        let board_key = DataKey::Board(scope, period);
        let board: Vec<TimeRecord> = env
            .storage()
            .persistent()
            .get(&board_key)
            .unwrap_or(Vec::new(&env));

        // Extend TTL when reading (good practice)
        if !board.is_empty() {
            Self::bump_persistent_ttl(&env, &board_key);
        }

        board
    }

    /// Pure mapping: completion time (ms) -> bracket (no storage).
    pub fn get_time_bracket(_env: Env, completion_time_ms: u64) -> TimeBracket {
        Self::time_to_bracket(completion_time_ms)
    }

    fn time_to_bracket(completion_time_ms: u64) -> TimeBracket {
        match completion_time_ms {
            0..=300_000 => TimeBracket::Beginner,
            300_001..=600_000 => TimeBracket::Intermediate,
            600_001..=900_000 => TimeBracket::Advanced,
            _ => TimeBracket::Expert,
        }
    }

    /// Get the admin address
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin address not set")
    }

    /// Get the current timestamp (for testing)
    pub fn get_timestamp(env: Env) -> u64 {
        env.ledger().timestamp()
    }
}

#[cfg(test)]
mod test {
    // Dev checks:
    // - cargo test -p time_attack
    // - cargo clippy --all-targets -p time_attack -- -D warnings
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger},
        Address, BytesN, Env,
    };

    #[test]
    fn test_initialize() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, TimeAttack);
        let client = TimeAttackClient::new(&env, &contract_id);

        let admin = Address::generate(&env);

        // Should initialize successfully (panics if it fails)
        client.initialize(&admin);

        // Should fail on second initialization
        let result = client.try_initialize(&admin);
        assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
    }

    #[test]
    fn test_submit_time_success() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, TimeAttack);
        let client = TimeAttackClient::new(&env, &contract_id);

        // Initialize
        let admin = Address::generate(&env);
        client.initialize(&admin);

        // Submit a time (will panic if it fails)
        let player = Address::generate(&env);
        let puzzle_id = 1u32;
        let completion_time = 120_000u64; // 2 minutes
        let replay_hash = BytesN::from_array(&env, &[1u8; 32]);

        client.submit_time(&player, &puzzle_id, &completion_time, &replay_hash);

        // Verify it was recorded
        let best = client.get_best_time(&puzzle_id);
        assert!(best.is_some());
        assert_eq!(best.unwrap().completion_time_ms, completion_time);
    }

    #[test]
    fn test_submit_time_invalid_time() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, TimeAttack);
        let client = TimeAttackClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        client.initialize(&admin);

        let player = Address::generate(&env);
        let replay_hash = BytesN::from_array(&env, &[1u8; 32]);

        // Test time too low (< 1 second) - use try_submit_time for errors
        let result = client.try_submit_time(&player, &1u32, &500u64, &replay_hash);
        assert_eq!(result, Err(Ok(Error::InvalidTime)));

        // Test time too high (> 1 hour)
        let replay_hash2 = BytesN::from_array(&env, &[2u8; 32]);
        let result = client.try_submit_time(&player, &1u32, &4_000_000u64, &replay_hash2);
        assert_eq!(result, Err(Ok(Error::InvalidTime)));
    }

    #[test]
    fn test_submit_time_rate_limiting() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, TimeAttack);
        let client = TimeAttackClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        client.initialize(&admin);

        let player = Address::generate(&env);
        let completion_time = 120_000u64;

        // First submission should succeed
        let replay1 = BytesN::from_array(&env, &[1u8; 32]);
        client.submit_time(&player, &1u32, &completion_time, &replay1);

        // Second submission immediately should fail (rate limiting)
        let replay2 = BytesN::from_array(&env, &[2u8; 32]);
        let result = client.try_submit_time(&player, &1u32, &completion_time, &replay2);
        assert_eq!(result, Err(Ok(Error::TooFrequent)));
    }

    #[test]
    fn test_submit_time_duplicate_replay() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, TimeAttack);
        let client = TimeAttackClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        client.initialize(&admin);

        let player1 = Address::generate(&env);
        let player2 = Address::generate(&env);
        let replay_hash = BytesN::from_array(&env, &[1u8; 32]);
        let completion_time = 120_000u64;

        // First player submits
        client.submit_time(&player1, &1u32, &completion_time, &replay_hash);

        // Second player tries to use same replay (should fail)
        let result = client.try_submit_time(&player2, &1u32, &completion_time, &replay_hash);
        assert_eq!(result, Err(Ok(Error::DuplicateReplay)));
    }

    #[test]
    fn test_leaderboard_ordering() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, TimeAttack);
        let client = TimeAttackClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        client.initialize(&admin);

        // Submit multiple times with different speeds
        let player1 = Address::generate(&env);
        let player2 = Address::generate(&env);
        let player3 = Address::generate(&env);

        client.submit_time(
            &player1,
            &1u32,
            &150_000u64,
            &BytesN::from_array(&env, &[1u8; 32]),
        );

        // Wait to avoid rate limiting
        env.ledger().with_mut(|li| li.timestamp += 61);

        client.submit_time(
            &player2,
            &1u32,
            &100_000u64,
            &BytesN::from_array(&env, &[2u8; 32]),
        );

        env.ledger().with_mut(|li| li.timestamp += 61);

        client.submit_time(
            &player3,
            &1u32,
            &125_000u64,
            &BytesN::from_array(&env, &[3u8; 32]),
        );

        // Check leaderboard is sorted (fastest first)
        let leaderboard = client.get_leaderboard(&1u32, &TimePeriod::AllTime);
        assert_eq!(leaderboard.len(), 3);
        assert_eq!(leaderboard.get(0).unwrap().completion_time_ms, 100_000); // player2
        assert_eq!(leaderboard.get(1).unwrap().completion_time_ms, 125_000); // player3
        assert_eq!(leaderboard.get(2).unwrap().completion_time_ms, 150_000); // player1
    }

    #[test]
    fn test_daily_board_resets_after_24h() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, TimeAttack);
        let client = TimeAttackClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        client.initialize(&admin);

        let player1 = Address::generate(&env);
        let player2 = Address::generate(&env);

        // Submit first time
        client.submit_time(
            &player1,
            &1u32,
            &100_000u64,
            &BytesN::from_array(&env, &[1u8; 32]),
        );

        // Check daily leaderboard has 1 entry
        let daily_board = client.get_leaderboard(&1u32, &TimePeriod::Daily);
        assert_eq!(daily_board.len(), 1);

        // Advance time by 24 hours + 1 second (86401 seconds)
        env.ledger().with_mut(|li| {
            li.timestamp += 86_401;
        });

        // Submit second time (should trigger reset)
        client.submit_time(
            &player2,
            &1u32,
            &120_000u64,
            &BytesN::from_array(&env, &[2u8; 32]),
        );

        // Check daily leaderboard was reset and now has only 1 entry (player2)
        let daily_board_after_reset = client.get_leaderboard(&1u32, &TimePeriod::Daily);
        assert_eq!(daily_board_after_reset.len(), 1);
        assert_eq!(daily_board_after_reset.get(0).unwrap().player, player2);

        // AllTime board should still have both
        let alltime_board = client.get_leaderboard(&1u32, &TimePeriod::AllTime);
        assert_eq!(alltime_board.len(), 2);
    }
}
