#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, BytesN, Env, Symbol, Vec,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PuzzleStatus {
    Active,
    Inactive,
    Deprecated,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PuzzleCategory {
    Logic,
    Mathematics,
    Pattern,
    Cryptography,
    Spatial,
    Sequence,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PuzzleMetadata {
    pub id: u32,
    pub creator: Address,
    pub category: PuzzleCategory,
    pub difficulty: u32, // 1-10 scale
    pub title: Symbol,
    pub description: Symbol,
    pub version: u32,
    pub status: PuzzleStatus,
    pub created_at: u64,
    pub updated_at: u64,
    pub royalty_percentage: u32, // basis points (100 = 1%)
    pub total_plays: u64,
    pub successful_plays: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PuzzleConfig {
    pub solution_hash: BytesN<32>,
    pub start_time: u64,
    pub end_time: u64,
    pub max_attempts: u32,
    pub time_limit: Option<u64>, // seconds
    pub reward_points: i128,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PuzzleInstance {
    pub metadata: PuzzleMetadata,
    pub config: PuzzleConfig,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct CreatorStats {
    pub address: Address,
    pub total_puzzles: u32,
    pub active_puzzles: u32,
    pub total_royalties_earned: i128,
    pub average_difficulty: u32,
    pub success_rate: u32, // basis points
}

#[contracttype]
pub enum DataKey {
    Admin,
    PuzzleCounter,
    Puzzle(u32),
    PuzzlesByCategory(PuzzleCategory),
    PuzzlesByCreator(Address),
    PuzzlesByDifficulty(u32),
    ActivePuzzles,
    CreatorStats(Address),
    AuthorizedCreators(Address),
    PendingRoyalties(Address),
    RoyaltyPool,
    TotalRoyaltiesDistributed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FactoryEvent {
    PuzzleCreated,
    PuzzleUpdated,
    PuzzleActivated,
    PuzzleDeactivated,
    PuzzleDeprecated,
    CreatorAuthorized,
    CreatorRevoked,
    PuzzlePlayed,
    RoyaltyCalculated,
    RoyaltyDistributed,
    RoyaltyWithdrawn,
}

#[contract]
pub struct PuzzleFactory;

#[contractimpl]
impl PuzzleFactory {
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::PuzzleCounter, &0u32);
    }

    fn require_admin(env: &Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not set");
        admin.require_auth();
    }

    fn require_authorized_creator(env: &Env, creator: &Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not set");
        
        if creator == &admin {
            return;
        }
        
        let is_authorized: bool = env
            .storage()
            .instance()
            .get(&DataKey::AuthorizedCreators(creator.clone()))
            .unwrap_or(false);
        
        if !is_authorized {
            panic!("creator not authorized");
        }
    }

    fn generate_puzzle_id(env: &Env) -> u32 {
        let mut counter: u32 = env
            .storage()
            .instance()
            .get(&DataKey::PuzzleCounter)
            .unwrap_or(0u32);
        
        counter += 1;
        env.storage().instance().set(&DataKey::PuzzleCounter, &counter);
        counter
    }

    fn update_creator_stats(env: &Env, creator: &Address, difficulty: u32, _success: bool) {
        let mut stats: CreatorStats = env
            .storage()
            .instance()
            .get(&DataKey::CreatorStats(creator.clone()))
            .unwrap_or(CreatorStats {
                address: creator.clone(),
                total_puzzles: 0,
                active_puzzles: 0,
                total_royalties_earned: 0,
                average_difficulty: 0,
                success_rate: 0,
            });

        stats.total_puzzles += 1;
        stats.active_puzzles += 1;
        
        // Update average difficulty
        stats.average_difficulty = ((stats.average_difficulty * (stats.total_puzzles - 1)) + difficulty) / stats.total_puzzles;

        env.storage().instance().set(&DataKey::CreatorStats(creator.clone()), &stats);
    }

    pub fn authorize_creator(env: Env, creator: Address) {
        Self::require_admin(&env);
        env.storage().instance().set(&DataKey::AuthorizedCreators(creator.clone()), &true);
        env.events().publish((FactoryEvent::CreatorAuthorized, creator), ());
    }

    pub fn revoke_creator(env: Env, creator: Address) {
        Self::require_admin(&env);
        env.storage().instance().set(&DataKey::AuthorizedCreators(creator.clone()), &false);
        env.events().publish((FactoryEvent::CreatorRevoked, creator), ());
    }

    pub fn create_puzzle(
        env: Env,
        creator: Address,
        category: PuzzleCategory,
        difficulty: u32,
        title: Symbol,
        description: Symbol,
        config: PuzzleConfig,
        royalty_percentage: u32,
    ) -> u32 {
        Self::require_authorized_creator(&env, &creator);
        creator.require_auth();

        if difficulty < 1 || difficulty > 10 {
            panic!("difficulty must be between 1 and 10");
        }

        if royalty_percentage > 1000 { // 10% max
            panic!("royalty percentage too high");
        }

        if config.end_time <= config.start_time {
            panic!("invalid time window");
        }

        let puzzle_id = Self::generate_puzzle_id(&env);
        let now = env.ledger().timestamp();

        let metadata = PuzzleMetadata {
            id: puzzle_id,
            creator: creator.clone(),
            category: category.clone(),
            difficulty,
            title,
            description,
            version: 1,
            status: PuzzleStatus::Active,
            created_at: now,
            updated_at: now,
            royalty_percentage,
            total_plays: 0,
            successful_plays: 0,
        };

        let puzzle_instance = PuzzleInstance {
            metadata: metadata.clone(),
            config,
        };

        // Store puzzle
        env.storage().instance().set(&DataKey::Puzzle(puzzle_id), &puzzle_instance);

        // Update indexes
        let mut category_puzzles: Vec<u32> = env
            .storage()
            .instance()
            .get(&DataKey::PuzzlesByCategory(category.clone()))
            .unwrap_or(Vec::new(&env));
        category_puzzles.push_back(puzzle_id);
        env.storage().instance().set(&DataKey::PuzzlesByCategory(category), &category_puzzles);

        let mut creator_puzzles: Vec<u32> = env
            .storage()
            .instance()
            .get(&DataKey::PuzzlesByCreator(creator.clone()))
            .unwrap_or(Vec::new(&env));
        creator_puzzles.push_back(puzzle_id);
        env.storage().instance().set(&DataKey::PuzzlesByCreator(creator), &creator_puzzles);

        let mut difficulty_puzzles: Vec<u32> = env
            .storage()
            .instance()
            .get(&DataKey::PuzzlesByDifficulty(difficulty))
            .unwrap_or(Vec::new(&env));
        difficulty_puzzles.push_back(puzzle_id);
        env.storage().instance().set(&DataKey::PuzzlesByDifficulty(difficulty), &difficulty_puzzles);

        let mut active_puzzles: Vec<u32> = env
            .storage()
            .instance()
            .get(&DataKey::ActivePuzzles)
            .unwrap_or(Vec::new(&env));
        active_puzzles.push_back(puzzle_id);
        env.storage().instance().set(&DataKey::ActivePuzzles, &active_puzzles);

        // Update creator stats
        Self::update_creator_stats(&env, &metadata.creator, difficulty, true);

        env.events().publish((FactoryEvent::PuzzleCreated, puzzle_id, metadata.creator), ());

        puzzle_id
    }

    pub fn update_puzzle(
        env: Env,
        puzzle_id: u32,
        title: Option<Symbol>,
        description: Option<Symbol>,
        config: Option<PuzzleConfig>,
        royalty_percentage: Option<u32>,
    ) {
        let puzzle: PuzzleInstance = env
            .storage()
            .instance()
            .get(&DataKey::Puzzle(puzzle_id))
            .expect("puzzle not found");

        Self::require_authorized_creator(&env, &puzzle.metadata.creator);
        puzzle.metadata.creator.require_auth();

        let mut updated_puzzle = puzzle;

        if let Some(new_title) = title {
            updated_puzzle.metadata.title = new_title;
        }

        if let Some(new_description) = description {
            updated_puzzle.metadata.description = new_description;
        }

        if let Some(new_config) = config {
            if new_config.end_time <= new_config.start_time {
                panic!("invalid time window");
            }
            updated_puzzle.config = new_config;
        }

        if let Some(new_royalty) = royalty_percentage {
            if new_royalty > 1000 {
                panic!("royalty percentage too high");
            }
            updated_puzzle.metadata.royalty_percentage = new_royalty;
        }

        updated_puzzle.metadata.updated_at = env.ledger().timestamp();
        updated_puzzle.metadata.version += 1;

        env.storage().instance().set(&DataKey::Puzzle(puzzle_id), &updated_puzzle);

        env.events().publish((FactoryEvent::PuzzleUpdated, puzzle_id), ());
    }

    pub fn activate_puzzle(env: Env, puzzle_id: u32) {
        let mut puzzle: PuzzleInstance = env
            .storage()
            .instance()
            .get(&DataKey::Puzzle(puzzle_id))
            .expect("puzzle not found");

        Self::require_authorized_creator(&env, &puzzle.metadata.creator);
        puzzle.metadata.creator.require_auth();

        if puzzle.metadata.status == PuzzleStatus::Active {
            return;
        }

        puzzle.metadata.status = PuzzleStatus::Active;
        puzzle.metadata.updated_at = env.ledger().timestamp();

        let mut active_puzzles: Vec<u32> = env
            .storage()
            .instance()
            .get(&DataKey::ActivePuzzles)
            .unwrap_or(Vec::new(&env));
        
        if !active_puzzles.contains(&puzzle_id) {
            active_puzzles.push_back(puzzle_id);
            env.storage().instance().set(&DataKey::ActivePuzzles, &active_puzzles);
        }

        env.storage().instance().set(&DataKey::Puzzle(puzzle_id), &puzzle);

        env.events().publish((FactoryEvent::PuzzleActivated, puzzle_id), ());
    }

    pub fn deactivate_puzzle(env: Env, puzzle_id: u32) {
        let mut puzzle: PuzzleInstance = env
            .storage()
            .instance()
            .get(&DataKey::Puzzle(puzzle_id))
            .expect("puzzle not found");

        Self::require_authorized_creator(&env, &puzzle.metadata.creator);
        puzzle.metadata.creator.require_auth();

        if puzzle.metadata.status == PuzzleStatus::Inactive {
            return;
        }

        puzzle.metadata.status = PuzzleStatus::Inactive;
        puzzle.metadata.updated_at = env.ledger().timestamp();

        // Remove from active puzzles
        let mut active_puzzles: Vec<u32> = env
            .storage()
            .instance()
            .get(&DataKey::ActivePuzzles)
            .unwrap_or(Vec::new(&env));
        
        let index = active_puzzles.iter().position(|id| id == puzzle_id);
        if let Some(idx) = index {
            active_puzzles.remove(idx.try_into().unwrap());
            env.storage().instance().set(&DataKey::ActivePuzzles, &active_puzzles);
        }

        env.storage().instance().set(&DataKey::Puzzle(puzzle_id), &puzzle);

        env.events().publish((FactoryEvent::PuzzleDeactivated, puzzle_id), ());
    }

    fn update_creator_stats_on_deprecation(env: &Env, creator: &Address, difficulty: u32) {
        let mut stats: CreatorStats = env
            .storage()
            .instance()
            .get(&DataKey::CreatorStats(creator.clone()))
            .unwrap_or(CreatorStats {
                address: creator.clone(),
                total_puzzles: 0,
                active_puzzles: 0,
                total_royalties_earned: 0,
                average_difficulty: 0,
                success_rate: 0,
            });

        if stats.active_puzzles > 0 {
            stats.active_puzzles -= 1;
        }

        if stats.total_puzzles > 0 {
            stats.total_puzzles -= 1;
            
            // Recalculate average difficulty
            if stats.total_puzzles > 0 {
                stats.average_difficulty = (stats.average_difficulty * (stats.total_puzzles + 1) - difficulty) / stats.total_puzzles;
            } else {
                stats.average_difficulty = 0;
            }
        }

        // If creator has no more puzzles, remove stats entry
        if stats.total_puzzles == 0 {
            env.storage().instance().remove(&DataKey::CreatorStats(creator.clone()));
        } else {
            env.storage().instance().set(&DataKey::CreatorStats(creator.clone()), &stats);
        }
    }

    pub fn deprecate_puzzle(env: Env, puzzle_id: u32) {
        let mut puzzle: PuzzleInstance = env
            .storage()
            .instance()
            .get(&DataKey::Puzzle(puzzle_id))
            .expect("puzzle not found");

        Self::require_authorized_creator(&env, &puzzle.metadata.creator);
        puzzle.metadata.creator.require_auth();

        if puzzle.metadata.status == PuzzleStatus::Deprecated {
            panic!("puzzle already deprecated");
        }

        let old_status = puzzle.metadata.status.clone();
        puzzle.metadata.status = PuzzleStatus::Deprecated;
        puzzle.metadata.updated_at = env.ledger().timestamp();

        // Remove from active puzzles if it was active
        if old_status == PuzzleStatus::Active {
            let mut active_puzzles: Vec<u32> = env
                .storage()
                .instance()
                .get(&DataKey::ActivePuzzles)
                .unwrap_or(Vec::new(&env));
            
            let index = active_puzzles.iter().position(|id| id == puzzle_id);
            if let Some(idx) = index {
                active_puzzles.remove(idx.try_into().unwrap());
                env.storage().instance().set(&DataKey::ActivePuzzles, &active_puzzles);
            }
        }

        // Remove from category index
        let mut category_puzzles: Vec<u32> = env
            .storage()
            .instance()
            .get(&DataKey::PuzzlesByCategory(puzzle.metadata.category.clone()))
            .unwrap_or(Vec::new(&env));
        
        let category_index = category_puzzles.iter().position(|id| id == puzzle_id);
        if let Some(idx) = category_index {
            category_puzzles.remove(idx.try_into().unwrap());
            
            // Remove category index if empty
            if category_puzzles.is_empty() {
                env.storage().instance().remove(&DataKey::PuzzlesByCategory(puzzle.metadata.category.clone()));
            } else {
                env.storage().instance().set(&DataKey::PuzzlesByCategory(puzzle.metadata.category.clone()), &category_puzzles);
            }
        }

        // Remove from creator index
        let mut creator_puzzles: Vec<u32> = env
            .storage()
            .instance()
            .get(&DataKey::PuzzlesByCreator(puzzle.metadata.creator.clone()))
            .unwrap_or(Vec::new(&env));
        
        let creator_index = creator_puzzles.iter().position(|id| id == puzzle_id);
        if let Some(idx) = creator_index {
            creator_puzzles.remove(idx.try_into().unwrap());
            
            // Remove creator index if empty
            if creator_puzzles.is_empty() {
                env.storage().instance().remove(&DataKey::PuzzlesByCreator(puzzle.metadata.creator.clone()));
            } else {
                env.storage().instance().set(&DataKey::PuzzlesByCreator(puzzle.metadata.creator.clone()), &creator_puzzles);
            }
        }

        // Remove from difficulty index
        let mut difficulty_puzzles: Vec<u32> = env
            .storage()
            .instance()
            .get(&DataKey::PuzzlesByDifficulty(puzzle.metadata.difficulty))
            .unwrap_or(Vec::new(&env));
        
        let difficulty_index = difficulty_puzzles.iter().position(|id| id == puzzle_id);
        if let Some(idx) = difficulty_index {
            difficulty_puzzles.remove(idx.try_into().unwrap());
            
            // Remove difficulty index if empty
            if difficulty_puzzles.is_empty() {
                env.storage().instance().remove(&DataKey::PuzzlesByDifficulty(puzzle.metadata.difficulty));
            } else {
                env.storage().instance().set(&DataKey::PuzzlesByDifficulty(puzzle.metadata.difficulty), &difficulty_puzzles);
            }
        }

        // Update creator stats
        Self::update_creator_stats_on_deprecation(&env, &puzzle.metadata.creator, puzzle.metadata.difficulty);

        env.storage().instance().set(&DataKey::Puzzle(puzzle_id), &puzzle);

        env.events().publish((FactoryEvent::PuzzleDeprecated, puzzle_id), ());
    }

    pub fn get_puzzle(env: Env, puzzle_id: u32) -> PuzzleInstance {
        env.storage()
            .instance()
            .get(&DataKey::Puzzle(puzzle_id))
            .expect("puzzle not found")
    }

    pub fn get_puzzles_by_category(env: Env, category: PuzzleCategory) -> Vec<u32> {
        env.storage()
            .instance()
            .get(&DataKey::PuzzlesByCategory(category))
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_puzzles_by_creator(env: Env, creator: Address) -> Vec<u32> {
        env.storage()
            .instance()
            .get(&DataKey::PuzzlesByCreator(creator))
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_puzzles_by_difficulty(env: Env, difficulty: u32) -> Vec<u32> {
        env.storage()
            .instance()
            .get(&DataKey::PuzzlesByDifficulty(difficulty))
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_active_puzzles(env: Env) -> Vec<u32> {
        env.storage()
            .instance()
            .get(&DataKey::ActivePuzzles)
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_creator_stats(env: Env, creator: Address) -> CreatorStats {
        env.storage()
            .instance()
            .get(&DataKey::CreatorStats(creator.clone()))
            .unwrap_or(CreatorStats {
                address: creator.clone(),
                total_puzzles: 0,
                active_puzzles: 0,
                total_royalties_earned: 0,
                average_difficulty: 0,
                success_rate: 0,
            })
    }

    pub fn is_creator_authorized(env: Env, creator: Address) -> bool {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not set");
        
        if creator == admin {
            return true;
        }
        
        env.storage()
            .instance()
            .get(&DataKey::AuthorizedCreators(creator))
            .unwrap_or(false)
    }

    pub fn get_puzzle_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::PuzzleCounter)
            .unwrap_or(0u32)
    }

    pub fn record_play(env: Env, puzzle_id: u32, player: Address, success: bool, payment_amount: Option<i128>) {
        let mut puzzle: PuzzleInstance = env
            .storage()
            .instance()
            .get(&DataKey::Puzzle(puzzle_id))
            .expect("puzzle not found");

        // Check if puzzle is active and within time window
        if puzzle.metadata.status != PuzzleStatus::Active {
            panic!("puzzle is not active");
        }

        let now = env.ledger().timestamp();
        if now < puzzle.config.start_time || now > puzzle.config.end_time {
            panic!("puzzle is not in playable time window");
        }

        player.require_auth();

        // Update play statistics
        puzzle.metadata.total_plays += 1;
        if success {
            puzzle.metadata.successful_plays += 1;
        }

        // Calculate and distribute royalties if payment provided
        if let Some(amount) = payment_amount {
            if amount > 0 {
                let royalty_amount = Self::calculate_royalty_amount(&env, amount, puzzle.metadata.royalty_percentage);
                if royalty_amount > 0 {
                    Self::distribute_royalty(&env, &puzzle.metadata.creator, royalty_amount);
                    
                    // Update creator stats
                    Self::update_creator_royalties(&env, &puzzle.metadata.creator, royalty_amount);
                }
            }
        }

        // Update puzzle in storage
        env.storage().instance().set(&DataKey::Puzzle(puzzle_id), &puzzle);

        // Update creator success rate
        Self::update_creator_success_rate(&env, &puzzle.metadata.creator);

        env.events().publish((
            FactoryEvent::PuzzlePlayed, 
            puzzle_id, 
            player, 
            success, 
            payment_amount.unwrap_or(0)
        ), ());
    }

    fn calculate_royalty_amount(_env: &Env, payment_amount: i128, royalty_percentage: u32) -> i128 {
        if royalty_percentage == 0 {
            return 0;
        }
        
        // Calculate royalty: payment_amount * (royalty_percentage / 10000)
        // royalty_percentage is in basis points (100 = 1%)
        (payment_amount * royalty_percentage as i128) / 10000
    }

    fn distribute_royalty(env: &Env, creator: &Address, amount: i128) {
        if amount <= 0 {
            return;
        }

        // Add to pending royalties for the creator
        let mut pending_royalties: i128 = env
            .storage()
            .instance()
            .get(&DataKey::PendingRoyalties(creator.clone()))
            .unwrap_or(0);
        
        pending_royalties += amount;
        env.storage().instance().set(&DataKey::PendingRoyalties(creator.clone()), &pending_royalties);

        // Update total royalties distributed
        let mut total_distributed: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalRoyaltiesDistributed)
            .unwrap_or(0);
        
        total_distributed += amount;
        env.storage().instance().set(&DataKey::TotalRoyaltiesDistributed, &total_distributed);

        env.events().publish((FactoryEvent::RoyaltyDistributed, creator.clone(), amount), ());
    }

    fn update_creator_royalties(env: &Env, creator: &Address, amount: i128) {
        let mut stats: CreatorStats = env
            .storage()
            .instance()
            .get(&DataKey::CreatorStats(creator.clone()))
            .unwrap_or(CreatorStats {
                address: creator.clone(),
                total_puzzles: 0,
                active_puzzles: 0,
                total_royalties_earned: 0,
                average_difficulty: 0,
                success_rate: 0,
            });

        stats.total_royalties_earned += amount;
        env.storage().instance().set(&DataKey::CreatorStats(creator.clone()), &stats);
    }

    fn update_creator_success_rate(env: &Env, creator: &Address) {
        let creator_puzzles: Vec<u32> = env
            .storage()
            .instance()
            .get(&DataKey::PuzzlesByCreator(creator.clone()))
            .unwrap_or(Vec::new(env));

        if creator_puzzles.is_empty() {
            return;
        }

        let mut total_plays: u64 = 0;
        let mut successful_plays: u64 = 0;

        for puzzle_id in creator_puzzles.iter() {
            if let Some(puzzle) = env.storage().instance().get::<DataKey, PuzzleInstance>(&DataKey::Puzzle(puzzle_id)) {
                total_plays += puzzle.metadata.total_plays;
                successful_plays += puzzle.metadata.successful_plays;
            }
        }

        let success_rate: u32 = if total_plays > 0 {
            ((successful_plays * 10000) / total_plays) as u32 // basis points
        } else {
            0
        };

        let mut stats: CreatorStats = env
            .storage()
            .instance()
            .get(&DataKey::CreatorStats(creator.clone()))
            .unwrap_or(CreatorStats {
                address: creator.clone(),
                total_puzzles: 0,
                active_puzzles: 0,
                total_royalties_earned: 0,
                average_difficulty: 0,
                success_rate: 0,
            });

        stats.success_rate = success_rate;
        env.storage().instance().set(&DataKey::CreatorStats(creator.clone()), &stats);
    }

    pub fn withdraw_royalties(env: Env, creator: Address) -> i128 {
        creator.require_auth();

        let pending_royalties: i128 = env
            .storage()
            .instance()
            .get(&DataKey::PendingRoyalties(creator.clone()))
            .unwrap_or(0);

        if pending_royalties <= 0 {
            return 0;
        }

        // Clear pending royalties
        env.storage().instance().remove(&DataKey::PendingRoyalties(creator.clone()));

        // In a real implementation, you would transfer tokens here
        // For now, we just emit an event and return the amount
        env.events().publish((FactoryEvent::RoyaltyWithdrawn, creator.clone(), pending_royalties), ());

        pending_royalties
    }

    pub fn get_pending_royalties(env: Env, creator: Address) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::PendingRoyalties(creator))
            .unwrap_or(0)
    }

    pub fn get_total_royalties_distributed(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalRoyaltiesDistributed)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::symbol_short;

    #[test]
    fn test_initialization() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleFactory);
        let client = PuzzleFactoryClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        
        env.mock_all_auths();
        client.initialize(&admin);
        
        // Test that admin is set
        assert_eq!(client.get_puzzle_count(), 0);
    }

    #[test]
    #[should_panic(expected = "already initialized")]
    fn test_double_initialization() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleFactory);
        let client = PuzzleFactoryClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        
        env.mock_all_auths();
        client.initialize(&admin);
        client.initialize(&admin);
    }

    #[test]
    fn test_puzzle_deprecation_cleanup() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleFactory);
        let client = PuzzleFactoryClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        
        env.mock_all_auths();
        client.initialize(&admin);
        client.authorize_creator(&creator);

        // Create test config
        let config = PuzzleConfig {
            solution_hash: BytesN::from_array(&env, &[0; 32]),
            start_time: 1000,
            end_time: 2000,
            max_attempts: 3,
            time_limit: Some(300),
            reward_points: 100,
        };

        // Create a puzzle
        let puzzle_id = client.create_puzzle(
            &creator,
            &PuzzleCategory::Logic,
            &5,
            &symbol_short!("TestPzl"),
            &symbol_short!("TestDesc"),
            &config,
            &100,
        );

        // Verify puzzle exists in all indexes
        let category_puzzles = client.get_puzzles_by_category(&PuzzleCategory::Logic);
        assert!(category_puzzles.contains(&puzzle_id));

        let creator_puzzles = client.get_puzzles_by_creator(&creator);
        assert!(creator_puzzles.contains(&puzzle_id));

        let difficulty_puzzles = client.get_puzzles_by_difficulty(&5);
        assert!(difficulty_puzzles.contains(&puzzle_id));

        let active_puzzles = client.get_active_puzzles();
        assert!(active_puzzles.contains(&puzzle_id));

        // Verify creator stats
        let stats = client.get_creator_stats(&creator);
        assert_eq!(stats.total_puzzles, 1);
        assert_eq!(stats.active_puzzles, 1);
        assert_eq!(stats.average_difficulty, 5);

        // Deprecate the puzzle
        client.deprecate_puzzle(&puzzle_id);

        // Verify puzzle is deprecated
        let puzzle = client.get_puzzle(&puzzle_id);
        assert_eq!(puzzle.metadata.status, PuzzleStatus::Deprecated);

        // Verify puzzle is removed from all indexes
        let category_puzzles = client.get_puzzles_by_category(&PuzzleCategory::Logic);
        assert!(!category_puzzles.contains(&puzzle_id));

        let creator_puzzles = client.get_puzzles_by_creator(&creator);
        assert!(!creator_puzzles.contains(&puzzle_id));

        let difficulty_puzzles = client.get_puzzles_by_difficulty(&5);
        assert!(!difficulty_puzzles.contains(&puzzle_id));

        let active_puzzles = client.get_active_puzzles();
        assert!(!active_puzzles.contains(&puzzle_id));

        // Verify creator stats are updated
        let stats = client.get_creator_stats(&creator);
        assert_eq!(stats.total_puzzles, 0);
        assert_eq!(stats.active_puzzles, 0);
        assert_eq!(stats.average_difficulty, 0);
    }

    #[test]
    #[should_panic(expected = "puzzle already deprecated")]
    fn test_double_deprecation() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleFactory);
        let client = PuzzleFactoryClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        
        env.mock_all_auths();
        client.initialize(&admin);
        client.authorize_creator(&creator);

        let config = PuzzleConfig {
            solution_hash: BytesN::from_array(&env, &[0; 32]),
            start_time: 1000,
            end_time: 2000,
            max_attempts: 3,
            time_limit: Some(300),
            reward_points: 100,
        };

        let puzzle_id = client.create_puzzle(
            &creator,
            &PuzzleCategory::Mathematics,
            &3,
            &symbol_short!("MathPzl"),
            &symbol_short!("MathDesc"),
            &config,
            &50,
        );

        // Deprecate once
        client.deprecate_puzzle(&puzzle_id);

        // Try to deprecate again - should panic
        client.deprecate_puzzle(&puzzle_id);
    }

    #[test]
    fn test_deprecation_inactive_puzzle() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleFactory);
        let client = PuzzleFactoryClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        
        env.mock_all_auths();
        client.initialize(&admin);
        client.authorize_creator(&creator);

        let config = PuzzleConfig {
            solution_hash: BytesN::from_array(&env, &[0; 32]),
            start_time: 1000,
            end_time: 2000,
            max_attempts: 3,
            time_limit: Some(300),
            reward_points: 100,
        };

        let puzzle_id = client.create_puzzle(
            &creator,
            &PuzzleCategory::Pattern,
            &7,
            &symbol_short!("PatPzl"),
            &symbol_short!("PatDesc"),
            &config,
            &150,
        );

        // Deactivate the puzzle first
        client.deactivate_puzzle(&puzzle_id);

        // Verify it's not in active puzzles
        let active_puzzles = client.get_active_puzzles();
        assert!(!active_puzzles.contains(&puzzle_id));

        // Now deprecate it
        client.deprecate_puzzle(&puzzle_id);

        // Verify it's deprecated and removed from other indexes
        let puzzle = client.get_puzzle(&puzzle_id);
        assert_eq!(puzzle.metadata.status, PuzzleStatus::Deprecated);

        let category_puzzles = client.get_puzzles_by_category(&PuzzleCategory::Pattern);
        assert!(!category_puzzles.contains(&puzzle_id));
    }

    #[test]
    fn test_multiple_puzzles_deprecation() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleFactory);
        let client = PuzzleFactoryClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        
        env.mock_all_auths();
        client.initialize(&admin);
        client.authorize_creator(&creator);

        let config = PuzzleConfig {
            solution_hash: BytesN::from_array(&env, &[0; 32]),
            start_time: 1000,
            end_time: 2000,
            max_attempts: 3,
            time_limit: Some(300),
            reward_points: 100,
        };

        // Create multiple puzzles
        let puzzle_id1 = client.create_puzzle(
            &creator,
            &PuzzleCategory::Logic,
            &3,
            &symbol_short!("LogPzl1"),
            &symbol_short!("LogDesc1"),
            &config.clone(),
            &100,
        );

        let puzzle_id2 = client.create_puzzle(
            &creator,
            &PuzzleCategory::Logic,
            &7,
            &symbol_short!("LogPzl2"),
            &symbol_short!("LogDesc2"),
            &config.clone(),
            &200,
        );

        // Verify initial state
        let stats = client.get_creator_stats(&creator);
        assert_eq!(stats.total_puzzles, 2);
        assert_eq!(stats.active_puzzles, 2);
        assert_eq!(stats.average_difficulty, 5); // (3 + 7) / 2

        let category_puzzles = client.get_puzzles_by_category(&PuzzleCategory::Logic);
        assert_eq!(category_puzzles.len(), 2);

        // Deprecate first puzzle
        client.deprecate_puzzle(&puzzle_id1);

        // Verify first puzzle is removed but second remains
        let category_puzzles = client.get_puzzles_by_category(&PuzzleCategory::Logic);
        assert_eq!(category_puzzles.len(), 1);
        assert!(category_puzzles.contains(&puzzle_id2));
        assert!(!category_puzzles.contains(&puzzle_id1));

        // Verify creator stats are updated correctly
        let stats = client.get_creator_stats(&creator);
        assert_eq!(stats.total_puzzles, 1);
        assert_eq!(stats.active_puzzles, 1);
        assert_eq!(stats.average_difficulty, 7); // Only puzzle 2 remains
    }

    #[test]
    fn test_play_tracking_without_payment() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleFactory);
        let client = PuzzleFactoryClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let player = Address::generate(&env);
        
        env.mock_all_auths();
        client.initialize(&admin);
        client.authorize_creator(&creator);

        let config = PuzzleConfig {
            solution_hash: BytesN::from_array(&env, &[0; 32]),
            start_time: 1000,
            end_time: 2000,
            max_attempts: 3,
            time_limit: Some(300),
            reward_points: 100,
        };

        let puzzle_id = client.create_puzzle(
            &creator,
            &PuzzleCategory::Logic,
            &5,
            &symbol_short!("TestPzl"),
            &symbol_short!("TestDesc"),
            &config,
            &100,
        );

        // Record successful play without payment
        client.record_play(&puzzle_id, &player, &true, &None::<i128>);

        // Verify play statistics updated
        let puzzle = client.get_puzzle(&puzzle_id);
        assert_eq!(puzzle.metadata.total_plays, 1);
        assert_eq!(puzzle.metadata.successful_plays, 1);

        // Verify creator success rate updated
        let stats = client.get_creator_stats(&creator);
        assert_eq!(stats.success_rate, 10000); // 100% success rate in basis points
    }

    #[test]
    fn test_play_tracking_with_royalties() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleFactory);
        let client = PuzzleFactoryClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let player = Address::generate(&env);
        
        env.mock_all_auths();
        client.initialize(&admin);
        client.authorize_creator(&creator);

        let config = PuzzleConfig {
            solution_hash: BytesN::from_array(&env, &[0; 32]),
            start_time: 1000,
            end_time: 2000,
            max_attempts: 3,
            time_limit: Some(300),
            reward_points: 100,
        };

        // Create puzzle with 10% royalty (1000 basis points)
        let puzzle_id = client.create_puzzle(
            &creator,
            &PuzzleCategory::Mathematics,
            &3,
            &symbol_short!("MathPzl"),
            &symbol_short!("MathDesc"),
            &config,
            &1000, // 10% royalty
        );

        // Record play with payment
        let payment_amount = 1000i128; // 10 tokens
        client.record_play(&puzzle_id, &player, &true, &Some(payment_amount));

        // Verify play statistics updated
        let puzzle = client.get_puzzle(&puzzle_id);
        assert_eq!(puzzle.metadata.total_plays, 1);
        assert_eq!(puzzle.metadata.successful_plays, 1);

        // Verify royalties calculated and distributed
        let expected_royalty = payment_amount * 1000 / 10000; // 10% of payment
        assert_eq!(expected_royalty, 100);

        let pending_royalties = client.get_pending_royalties(&creator);
        assert_eq!(pending_royalties, expected_royalty);

        let total_distributed = client.get_total_royalties_distributed();
        assert_eq!(total_distributed, expected_royalty);

        // Verify creator stats updated
        let stats = client.get_creator_stats(&creator);
        assert_eq!(stats.total_royalties_earned, expected_royalty);
    }

    #[test]
    fn test_royalty_withdrawal() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleFactory);
        let client = PuzzleFactoryClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let player = Address::generate(&env);
        
        env.mock_all_auths();
        client.initialize(&admin);
        client.authorize_creator(&creator);

        let config = PuzzleConfig {
            solution_hash: BytesN::from_array(&env, &[0; 32]),
            start_time: 1000,
            end_time: 2000,
            max_attempts: 3,
            time_limit: Some(300),
            reward_points: 100,
        };

        let puzzle_id = client.create_puzzle(
            &creator,
            &PuzzleCategory::Pattern,
            &7,
            &symbol_short!("PatPzl"),
            &symbol_short!("PatDesc"),
            &config,
            &500, // 5% royalty
        );

        // Record multiple plays with payments
        for _ in 0..3 {
            client.record_play(&puzzle_id, &player, &true, &Some(2000i128));
        }

        let expected_total_royalties = 3 * (2000 * 500 / 10000); // 3 plays * 5% of 2000
        assert_eq!(expected_total_royalties, 300);

        // Verify pending royalties
        let pending_before = client.get_pending_royalties(&creator);
        assert_eq!(pending_before, expected_total_royalties);

        // Withdraw royalties
        let withdrawn_amount = client.withdraw_royalties(&creator);
        assert_eq!(withdrawn_amount, expected_total_royalties);

        // Verify pending royalties cleared
        let pending_after = client.get_pending_royalties(&creator);
        assert_eq!(pending_after, 0);

        // Total distributed should remain the same
        let total_distributed = client.get_total_royalties_distributed();
        assert_eq!(total_distributed, expected_total_royalties);
    }

    #[test]
    fn test_zero_royalty_handling() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleFactory);
        let client = PuzzleFactoryClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let player = Address::generate(&env);
        
        env.mock_all_auths();
        client.initialize(&admin);
        client.authorize_creator(&creator);

        let config = PuzzleConfig {
            solution_hash: BytesN::from_array(&env, &[0; 32]),
            start_time: 1000,
            end_time: 2000,
            max_attempts: 3,
            time_limit: Some(300),
            reward_points: 100,
        };

        // Create puzzle with 0% royalty
        let puzzle_id = client.create_puzzle(
            &creator,
            &PuzzleCategory::Spatial,
            &4,
            &symbol_short!("SpaPzl"),
            &symbol_short!("SpaDesc"),
            &config,
            &0, // 0% royalty
        );

        // Record play with payment
        client.record_play(&puzzle_id, &player, &true, &Some(1000i128));

        // Verify play statistics updated but no royalties
        let puzzle = client.get_puzzle(&puzzle_id);
        assert_eq!(puzzle.metadata.total_plays, 1);
        assert_eq!(puzzle.metadata.successful_plays, 1);

        let pending_royalties = client.get_pending_royalties(&creator);
        assert_eq!(pending_royalties, 0);

        let total_distributed = client.get_total_royalties_distributed();
        assert_eq!(total_distributed, 0);
    }

    #[test]
    #[should_panic(expected = "puzzle is not active")]
    fn test_play_on_inactive_puzzle() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleFactory);
        let client = PuzzleFactoryClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let player = Address::generate(&env);
        
        env.mock_all_auths();
        client.initialize(&admin);
        client.authorize_creator(&creator);

        let config = PuzzleConfig {
            solution_hash: BytesN::from_array(&env, &[0; 32]),
            start_time: 1000,
            end_time: 2000,
            max_attempts: 3,
            time_limit: Some(300),
            reward_points: 100,
        };

        let puzzle_id = client.create_puzzle(
            &creator,
            &PuzzleCategory::Cryptography,
            &6,
            &symbol_short!("CrypPzl"),
            &symbol_short!("CrypDesc"),
            &config,
            &200,
        );

        // Deactivate puzzle
        client.deactivate_puzzle(&puzzle_id);

        // Try to record play - should panic
        client.record_play(&puzzle_id, &player, &true, &Some(1000i128));
    }

    #[test]
    #[should_panic(expected = "puzzle is not in playable time window")]
    fn test_play_outside_time_window() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleFactory);
        let client = PuzzleFactoryClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let player = Address::generate(&env);
        
        env.mock_all_auths();
        client.initialize(&admin);
        client.authorize_creator(&creator);

        // Create puzzle with future start time
        let future_time = 10000;
        let config = PuzzleConfig {
            solution_hash: BytesN::from_array(&env, &[0; 32]),
            start_time: future_time,
            end_time: future_time + 1000,
            max_attempts: 3,
            time_limit: Some(300),
            reward_points: 100,
        };

        let puzzle_id = client.create_puzzle(
            &creator,
            &PuzzleCategory::Sequence,
            &5,
            &symbol_short!("SeqPzl"),
            &symbol_short!("SeqDesc"),
            &config,
            &150,
        );

        // Try to record play before start time - should panic
        client.record_play(&puzzle_id, &player, &true, &Some(1000i128));
    }

    #[test]
    fn test_creator_success_rate_calculation() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PuzzleFactory);
        let client = PuzzleFactoryClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let player = Address::generate(&env);
        
        env.mock_all_auths();
        client.initialize(&admin);
        client.authorize_creator(&creator);

        let config = PuzzleConfig {
            solution_hash: BytesN::from_array(&env, &[0; 32]),
            start_time: 1000,
            end_time: 2000,
            max_attempts: 3,
            time_limit: Some(300),
            reward_points: 100,
        };

        // Create puzzle
        let puzzle_id = client.create_puzzle(
            &creator,
            &PuzzleCategory::Logic,
            &5,
            &symbol_short!("TestPzl"),
            &symbol_short!("TestDesc"),
            &config,
            &100,
        );

        // Record mixed success/failure plays
        client.record_play(&puzzle_id, &player, &true, &None::<i128>);  // Success
        client.record_play(&puzzle_id, &player, &false, &None::<i128>); // Failure
        client.record_play(&puzzle_id, &player, &true, &None::<i128>);  // Success

        // Verify success rate: 2 successes out of 3 plays = 66.66%
        let stats = client.get_creator_stats(&creator);
        assert_eq!(stats.success_rate, 6666); // 66.66% in basis points
    }
}
