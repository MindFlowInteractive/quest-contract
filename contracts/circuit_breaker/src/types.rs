use soroban_sdk::{contracterror, contracttype, Address, String, Symbol, Vec};

/// Errors returned by the circuit breaker.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum CircuitBreakerError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    InvalidConfiguration = 4,
    AlreadyPaused = 5,
    NotPaused = 6,
    ProposalNotFound = 7,
    ProposalNotPending = 8,
    ProposalExpired = 9,
    DuplicateApproval = 10,
    QuorumNotReached = 11,
    FunctionNotCallable = 12,
    UnpauseNotRequested = 13,
    UnpauseDelayNotElapsed = 14,
    GuardianAlreadyExists = 15,
    GuardianNotFound = 16,
    PendingProposalExists = 17,
    NotProposer = 18,
    UnpauseAlreadyRequested = 19,
}

/// Configuration of the guardian set and the pause/unpause policy.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuardianConfig {
    /// Address that owns configuration changes and approves unpausing.
    pub admin: Address,
    /// Addresses allowed to propose and approve a pause.
    pub guardians: Vec<Address>,
    /// Number of distinct guardian approvals required to trip the breaker.
    pub quorum: u32,
    /// Minimum seconds between a pause and a permitted unpause.
    pub unpause_delay: u64,
    /// Lifetime of a pause proposal, in seconds.
    pub pause_proposal_ttl: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalStatus {
    Pending,
    Executed,
    Expired,
    Canceled,
}

/// A guardian proposal to trip the circuit breaker.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PauseProposal {
    pub id: u64,
    pub proposer: Address,
    pub reason: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub approvals: Vec<Address>,
    pub status: ProposalStatus,
    pub executed_at: u64,
}

/// Current state of the circuit breaker.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PauseState {
    pub active: bool,
    pub paused_at: u64,
    pub paused_by: Option<Address>,
    pub reason: String,
    pub pause_id: u64,
    pub unpause_requested_at: u64,
    pub unpause_requested_by: Option<Address>,
    pub unpause_reason: String,
    pub total_pauses: u64,
}

/// Immutable record kept for audits.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PauseRecord {
    pub id: u64,
    pub paused_at: u64,
    pub paused_by: Address,
    pub reason: String,
    pub unpaused_at: u64,
}

/// Read-only summary used by dashboards and integrating contracts.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CircuitStatus {
    pub paused: bool,
    pub quorum: u32,
    pub pending_approvals: u32,
    pub unpause_delay: u64,
    pub seconds_until_unpause: u64,
    pub total_pauses: u64,
}

/// Storage keys.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Config,
    PauseState,
    PauseProposal(u64),
    NextProposalId,
    /// Functions that stay callable while the breaker is open.
    Allowlist,
    PauseHistory,
}

/// Default allowlist: read-only queries and the unpause flow.
pub fn default_allowlist(env: &soroban_sdk::Env) -> Vec<Symbol> {
    let mut functions = Vec::new(env);
    functions.push_back(Symbol::new(env, "is_paused"));
    functions.push_back(Symbol::new(env, "get_status"));
    functions.push_back(Symbol::new(env, "get_config"));
    functions.push_back(Symbol::new(env, "get_pause_state"));
    functions.push_back(Symbol::new(env, "get_pause_proposal"));
    functions.push_back(Symbol::new(env, "get_guardians"));
    functions.push_back(Symbol::new(env, "get_allowlist"));
    functions.push_back(Symbol::new(env, "get_pause_history"));
    functions.push_back(Symbol::new(env, "assert_callable"));
    functions.push_back(Symbol::new(env, "request_unpause"));
    functions.push_back(Symbol::new(env, "execute_unpause"));
    functions
}
