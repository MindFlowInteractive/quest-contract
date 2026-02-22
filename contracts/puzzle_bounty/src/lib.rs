#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, token, Address, BytesN, Env, Vec,
};

// ─── Status ───────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BountyStatus {
    Open = 0,
    Completed = 1, // all winner slots filled
    Expired = 2,
    Cancelled = 3,
}

// ─── Core Bounty Data ─────────────────────────────────────────────────────────

/// Configuration + state for a puzzle bounty posted by a sponsor.
#[contracttype]
#[derive(Clone, Debug)]
pub struct PuzzleBounty {
    pub id: u32,
    /// The account that created and funded the bounty.
    pub sponsor: Address,
    /// Token used for rewards (escrowed in contract).
    pub token: Address,
    /// Puzzle identifier this bounty is for.
    pub puzzle_id: u32,
    /// SHA-256 (or any 32-byte) hash of the correct solution.
    /// Solvers must supply a matching hash to claim.
    pub solution_hash: BytesN<32>,
    /// Reward amounts for 1st / 2nd / 3rd place.
    pub reward_1st: i128,
    pub reward_2nd: i128,
    pub reward_3rd: i128,
    /// Unix timestamp after which no new claims are accepted.
    pub expiration: u64,
    pub status: BountyStatus,
    /// Number of winners claimed so far (0-3).
    pub winner_count: u32,
}

impl PuzzleBounty {
    /// Total tokens locked in escrow when the bounty was created.
    pub fn total_reward(&self) -> i128 {
        self.reward_1st + self.reward_2nd + self.reward_3rd
    }
}

// ─── Winner / Leaderboard Entry ───────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug)]
pub struct WinnerEntry {
    pub rank: u32,
    pub solver: Address,
    pub reward: i128,
    pub claimed_at: u64,
}

// ─── Storage Keys ─────────────────────────────────────────────────────────────

#[contracttype]
pub enum DataKey {
    Admin,
    BountyCount,
    Bounty(u32),
    Winners(u32), // Vec<WinnerEntry> per bounty
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct PuzzleBountyContract;

#[contractimpl]
impl PuzzleBountyContract {
    // ── Admin / Initialisation ─────────────────────────────────────────────

    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::BountyCount, &0u32);
    }

    // ── Create ────────────────────────────────────────────────────────────

    /// Sponsor creates a bounty by depositing the full reward pool into escrow.
    ///
    /// Returns the new bounty ID.
    pub fn create_bounty(
        env: Env,
        sponsor: Address,
        token_address: Address,
        puzzle_id: u32,
        solution_hash: BytesN<32>,
        reward_1st: i128,
        reward_2nd: i128,
        reward_3rd: i128,
        duration: u64,
    ) -> u32 {
        sponsor.require_auth();

        if reward_1st <= 0 {
            panic!("First-place reward must be positive");
        }
        if reward_2nd < 0 || reward_3rd < 0 {
            panic!("Rewards cannot be negative");
        }
        if reward_1st < reward_2nd || reward_2nd < reward_3rd {
            panic!("Rewards must be in descending order: 1st >= 2nd >= 3rd");
        }
        if duration == 0 {
            panic!("Duration must be greater than zero");
        }

        let total = reward_1st + reward_2nd + reward_3rd;

        let mut count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::BountyCount)
            .unwrap_or(0);
        count += 1;

        let expiration = env.ledger().timestamp() + duration;

        let bounty = PuzzleBounty {
            id: count,
            sponsor: sponsor.clone(),
            token: token_address.clone(),
            puzzle_id,
            solution_hash,
            reward_1st,
            reward_2nd,
            reward_3rd,
            expiration,
            status: BountyStatus::Open,
            winner_count: 0,
        };

        // Escrow: transfer total reward from sponsor → contract
        let token_client = token::Client::new(&env, &token_address);
        token_client.transfer(&sponsor, &env.current_contract_address(), &total);

        env.storage()
            .instance()
            .set(&DataKey::Bounty(count), &bounty);
        env.storage().instance().set(&DataKey::BountyCount, &count);

        // Initialise empty winners list
        let empty: Vec<WinnerEntry> = Vec::new(&env);
        env.storage()
            .instance()
            .set(&DataKey::Winners(count), &empty);

        env.events().publish(
            (symbol_short!("pb"), symbol_short!("created")),
            (count, sponsor, puzzle_id, total),
        );

        count
    }

    // ── Claim ─────────────────────────────────────────────────────────────

    /// Solver submits their solution hash to claim a rank on the bounty.
    ///
    /// Returns the rank they achieved (1, 2, or 3).
    pub fn claim_bounty(
        env: Env,
        solver: Address,
        bounty_id: u32,
        solution_hash: BytesN<32>,
    ) -> u32 {
        solver.require_auth();

        let mut bounty = Self::get_bounty(env.clone(), bounty_id).expect("Bounty not found");

        if bounty.status != BountyStatus::Open {
            panic!("Bounty is not open");
        }

        if env.ledger().timestamp() > bounty.expiration {
            panic!("Bounty has expired");
        }

        if solution_hash != bounty.solution_hash {
            panic!("Incorrect solution");
        }

        // Determine rank based on winner_count
        let rank = bounty.winner_count + 1;
        if rank > 3 {
            panic!("All winner slots are filled");
        }

        // Ensure this solver hasn't already claimed
        let mut winners: Vec<WinnerEntry> = env
            .storage()
            .instance()
            .get(&DataKey::Winners(bounty_id))
            .unwrap_or(Vec::new(&env));

        for w in winners.iter() {
            if w.solver == solver {
                panic!("Solver has already claimed a reward");
            }
        }

        let reward = match rank {
            1 => bounty.reward_1st,
            2 => bounty.reward_2nd,
            3 => bounty.reward_3rd,
            _ => panic!("Invalid rank"),
        };

        // Pay solver
        let token_client = token::Client::new(&env, &bounty.token);
        token_client.transfer(&env.current_contract_address(), &solver, &reward);

        // Record winner
        winners.push_back(WinnerEntry {
            rank,
            solver: solver.clone(),
            reward,
            claimed_at: env.ledger().timestamp(),
        });
        env.storage()
            .instance()
            .set(&DataKey::Winners(bounty_id), &winners);

        bounty.winner_count = rank;

        // Mark completed if 3rd winner just claimed
        if rank == 3 {
            bounty.status = BountyStatus::Completed;
        }

        env.storage()
            .instance()
            .set(&DataKey::Bounty(bounty_id), &bounty);

        env.events().publish(
            (symbol_short!("pb"), symbol_short!("claimed")),
            (bounty_id, solver, rank, reward),
        );

        rank
    }

    // ── Refund Expired ────────────────────────────────────────────────────

    /// Anyone can call this after a bounty expires to refund unclaimed rewards
    /// back to the sponsor.
    pub fn refund_expired(env: Env, bounty_id: u32) {
        let mut bounty = Self::get_bounty(env.clone(), bounty_id).expect("Bounty not found");

        if bounty.status != BountyStatus::Open {
            panic!("Bounty is not open");
        }

        if env.ledger().timestamp() <= bounty.expiration {
            panic!("Bounty has not expired yet");
        }

        // Calculate unclaimed amount
        let winners: Vec<WinnerEntry> = env
            .storage()
            .instance()
            .get(&DataKey::Winners(bounty_id))
            .unwrap_or(Vec::new(&env));

        let claimed: i128 = winners.iter().map(|w| w.reward).sum();
        let unclaimed = bounty.total_reward() - claimed;

        if unclaimed > 0 {
            let token_client = token::Client::new(&env, &bounty.token);
            token_client.transfer(&env.current_contract_address(), &bounty.sponsor, &unclaimed);
        }

        bounty.status = BountyStatus::Expired;
        env.storage()
            .instance()
            .set(&DataKey::Bounty(bounty_id), &bounty);

        env.events().publish(
            (symbol_short!("pb"), symbol_short!("refunded")),
            (bounty_id, bounty.sponsor, unclaimed),
        );
    }

    // ── Cancel ────────────────────────────────────────────────────────────

    /// Sponsor cancels an open bounty (only allowed when no winners have
    /// claimed yet). Full escrow amount is refunded.
    pub fn cancel_bounty(env: Env, sponsor: Address, bounty_id: u32) {
        sponsor.require_auth();

        let mut bounty = Self::get_bounty(env.clone(), bounty_id).expect("Bounty not found");

        if bounty.sponsor != sponsor {
            panic!("Only the sponsor can cancel");
        }

        if bounty.status != BountyStatus::Open {
            panic!("Only open bounties can be cancelled");
        }

        if bounty.winner_count > 0 {
            panic!("Cannot cancel after winners have claimed");
        }

        let total = bounty.total_reward();
        let token_client = token::Client::new(&env, &bounty.token);
        token_client.transfer(&env.current_contract_address(), &sponsor, &total);

        bounty.status = BountyStatus::Cancelled;
        env.storage()
            .instance()
            .set(&DataKey::Bounty(bounty_id), &bounty);

        env.events().publish(
            (symbol_short!("pb"), symbol_short!("cancelled")),
            (bounty_id, sponsor, total),
        );
    }

    // ── Queries ───────────────────────────────────────────────────────────

    pub fn get_bounty(env: Env, bounty_id: u32) -> Option<PuzzleBounty> {
        env.storage().instance().get(&DataKey::Bounty(bounty_id))
    }

    pub fn get_bounty_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::BountyCount)
            .unwrap_or(0)
    }

    /// Returns the winner leaderboard for a specific bounty (up to 3 entries).
    pub fn get_leaderboard(env: Env, bounty_id: u32) -> Vec<WinnerEntry> {
        env.storage()
            .instance()
            .get(&DataKey::Winners(bounty_id))
            .unwrap_or(Vec::new(&env))
    }

    /// Returns all open bounties for a given puzzle_id (paginated).
    pub fn get_bounties_for_puzzle(
        env: Env,
        puzzle_id: u32,
        offset: u32,
        limit: u32,
    ) -> Vec<PuzzleBounty> {
        let count = Self::get_bounty_count(env.clone());
        let mut result: Vec<PuzzleBounty> = Vec::new(&env);
        let mut seen = 0u32;

        for i in 1..=count {
            if let Some(b) = Self::get_bounty(env.clone(), i) {
                if b.puzzle_id == puzzle_id && b.status == BountyStatus::Open {
                    if seen >= offset && result.len() < limit {
                        result.push_back(b);
                    }
                    seen += 1;
                }
            }
        }
        result
    }
}

mod test;
