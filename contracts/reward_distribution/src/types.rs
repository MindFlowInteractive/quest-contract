#![no_std]
use soroban_sdk::{contracttype, Address, BytesN, String};

// ─── Distribution type ───────────────────────────────────────────────────────

/// What kind of reward distribution this is (informational / UI hint).
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DistributionKind {
    /// Token airdrop to a snapshot of addresses
    Airdrop = 0,
    /// Ongoing player incentive program
    Incentive = 1,
    /// One-off player reward (e.g. quest completion)
    PlayerReward = 2,
    /// Community grant
    Grant = 3,
}

// ─── Distribution status ─────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DistributionStatus {
    /// Accepting claims
    Active = 0,
    /// Past expiry; only recovery allowed
    Expired = 1,
    /// Fully claimed or manually closed
    Exhausted = 2,
    /// Admin cancelled before expiry
    Cancelled = 3,
}

// ─── Core structs ────────────────────────────────────────────────────────────

/// A reward distribution campaign.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Distribution {
    /// Unique auto-incremented identifier
    pub id: u32,
    /// Human-readable label (e.g. "Season 3 Airdrop")
    pub label: String,
    /// Kind of distribution
    pub kind: DistributionKind,
    /// Merkle root committing to all (address, amount) pairs
    pub merkle_root: BytesN<32>,
    /// The reward token contract address
    pub token: Address,
    /// Total tokens deposited into this distribution
    pub total_allocation: i128,
    /// Tokens already claimed
    pub claimed_amount: i128,
    /// Number of individual claims processed
    pub claimed_count: u32,
    /// UNIX timestamp after which no new claims are accepted
    pub expiry: u64,
    /// Ledger timestamp when this distribution was created
    pub created_at: u64,
    /// Current status
    pub status: DistributionStatus,
    /// Address that created this distribution (can recover unclaimed tokens)
    pub creator: Address,
}

/// A snapshot of a single claim, stored for history.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ClaimRecord {
    pub distribution_id: u32,
    pub claimer: Address,
    pub amount: i128,
    pub claimed_at: u64,
}

/// Summary returned by `get_claim_history` (pagination entry).
#[contracttype]
#[derive(Clone, Debug)]
pub struct ClaimHistoryEntry {
    pub distribution_id: u32,
    pub amount: i128,
    pub claimed_at: u64,
}

// ─── Errors ──────────────────────────────────────────────────────────────────

#[soroban_sdk::contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RewardDistributionError {
    AlreadyInitialized    = 1,
    NotInitialized        = 2,
    Unauthorized          = 3,
    InvalidAmount         = 4,
    InvalidExpiry         = 5,
    DistributionNotFound  = 6,
    DistributionNotActive = 7,
    DistributionExpired   = 8,
    AlreadyClaimed        = 9,
    InvalidMerkleProof    = 10,
    InsufficientAllocation = 11,
    NotExpiredYet         = 12,
    NothingToRecover      = 13,
    InvalidBatchInput     = 14,
    ArithmeticOverflow    = 15,
}
