use soroban_sdk::{contracttype, Address, String, Symbol, Val, Vec};

// ──────────────────────────────────────────────
// Storage keys
// ──────────────────────────────────────────────

#[contracttype]
pub enum DataKey {
    // Token metadata
    Admin,
    Name,
    Symbol,
    Decimals,
    TotalSupply,

    // Token balances / allowances
    Balance(Address),
    Allowance(Address, Address),

    // Delegation
    Delegate(Address),
    VotingPower(Address),

    // Checkpoints (vote‑weight snapshots)
    Checkpoints(Address),
    NumCheckpoints(Address),

    // Governance config
    Config,

    // Proposals
    Proposal(u64),
    ProposalCount,
    ProposalArgs(u64),

    // Voting records
    HasVoted(u64, Address),
    VoteReceipt(u64, Address),
}

// ──────────────────────────────────────────────
// Checkpoint (vote‑weight snapshot)
// ──────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Checkpoint {
    pub sequence: u32,
    pub votes: i128,
}

// ──────────────────────────────────────────────
// Governance configuration
// ──────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernanceConfig {
    /// Delay (seconds) between proposal creation and voting start
    pub voting_delay: u64,
    /// Duration (seconds) of the voting window
    pub voting_period: u64,
    /// Minimum voting power required to create a proposal
    pub proposal_threshold: i128,
    /// Quorum expressed as percentage (0–100) of total supply
    pub quorum_numerator: u32,
    /// Seconds a queued proposal must wait before execution
    pub timelock_delay: u64,
    /// Grace period (seconds) after timelock in which execution is still allowed
    pub grace_period: u64,
}

// ──────────────────────────────────────────────
// Proposal types
// ──────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalStatus {
    Pending,
    Active,
    Defeated,
    Succeeded,
    Queued,
    Executed,
    Canceled,
    Expired,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VoteType {
    For,
    Against,
    Abstain,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProposalAction {
    pub contract_id: Address,
    pub function_name: Symbol,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProposalActionInput {
    pub contract_id: Address,
    pub function_name: Symbol,
    pub args: Vec<Val>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub title: String,
    pub description: String,
    pub action: ProposalAction,
    pub start_time: u64,
    pub end_time: u64,
    /// Ledger sequence captured at proposal creation for snapshot lookups
    pub snapshot_sequence: u32,
    pub for_votes: i128,
    pub against_votes: i128,
    pub abstain_votes: i128,
    pub status: ProposalStatus,
    pub quorum: i128,
    /// Earliest execution time (set when queued)
    pub eta: u64,
    pub category: u32,
}

// ──────────────────────────────────────────────
// Vote receipt
// ──────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoteReceipt {
    pub vote_type: VoteType,
    pub weight: i128,
}
