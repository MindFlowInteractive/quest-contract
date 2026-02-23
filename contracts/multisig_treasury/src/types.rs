use soroban_sdk::{contracttype, Address, String, Symbol, Val, Vec};

/// Role-based access levels for treasury members
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Role {
    /// Can propose and sign transactions
    Signer,
    /// Can propose, sign, and manage signers
    Admin,
    /// Full control including emergency recovery
    Owner,
}

/// Status of a transaction proposal
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransactionStatus {
    /// Proposal created, awaiting signatures
    Pending,
    /// Enough signatures collected, ready to execute
    Approved,
    /// Transaction has been executed
    Executed,
    /// Transaction was rejected or canceled
    Rejected,
    /// Transaction expired before reaching threshold
    Expired,
}

/// Type of transaction that can be proposed
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransactionType {
    /// Transfer tokens to an address
    TokenTransfer,
    /// Transfer native asset (XLM)
    NativeTransfer,
    /// Call another contract
    ContractCall,
    /// Update treasury configuration
    ConfigUpdate,
    /// Add or remove a signer
    SignerManagement,
    /// Emergency action
    EmergencyAction,
}

/// Configuration for the treasury
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreasuryConfig {
    /// Contract address that owns this treasury
    pub owner: Address,
    /// Minimum number of signatures required (M in M-of-N)
    pub threshold: u32,
    /// Total number of signers (N in M-of-N)
    pub total_signers: u32,
    /// Time limit for proposals to collect signatures (in seconds)
    pub proposal_timeout: u64,
    /// Maximum number of pending proposals allowed
    pub max_pending_proposals: u32,
    /// Whether emergency recovery is enabled
    pub emergency_recovery_enabled: bool,
    /// Cooldown period after emergency recovery (in seconds)
    pub emergency_cooldown: u64,
}

/// Member of the treasury with a role
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Member {
    pub address: Address,
    pub role: Role,
    pub added_at: u64,
    pub active: bool,
}

/// A signature on a transaction proposal
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Signature {
    pub signer: Address,
    pub timestamp: u64,
}

/// Transaction proposal data
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transaction {
    pub id: u64,
    pub proposer: Address,
    pub transaction_type: TransactionType,
    pub status: TransactionStatus,
    /// Target token contract for transfers, or target contract for calls
    pub target: Option<Address>,
    /// Amount for transfers
    pub amount: Option<i128>,
    /// Destination address for transfers
    pub destination: Option<Address>,
    /// Function to call on target contract
    pub function: Option<Symbol>,
    /// Arguments for contract calls
    pub args: Option<Vec<Val>>,
    /// Signatures collected so far
    pub signatures: Vec<Signature>,
    pub created_at: u64,
    pub expires_at: u64,
    pub executed_at: Option<u64>,
    /// Description of the transaction
    pub description: String,
    /// Minimum role required to sign this transaction
    pub required_role: Role,
}

/// Record of a completed transaction for history
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionRecord {
    pub id: u64,
    pub transaction_type: TransactionType,
    pub status: TransactionStatus,
    pub proposer: Address,
    pub signers: Vec<Address>,
    pub executed_at: u64,
}

/// Emergency recovery state
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmergencyState {
    pub activated_at: u64,
    pub activated_by: Address,
    pub reason: String,
    pub new_owner: Address,
    pub recovery_approved: bool,
}

/// Data keys for storage
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Treasury configuration
    Config,
    /// Member data for an address
    Member(Address),
    /// List of all member addresses
    Members,
    /// Transaction by ID
    Transaction(u64),
    /// Next transaction ID counter
    NextTransactionId,
    /// Whether an address has signed a transaction
    HasSigned(u64, Address),
    /// Transaction history record
    TransactionHistory(u64),
    /// Total number of transactions executed
    TransactionCount,
    /// Emergency recovery state
    EmergencyState,
    /// Last emergency timestamp
    LastEmergencyAt,
    /// Pending transaction IDs
    PendingTransactions,
}

/// Errors that can occur in the contract
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TreasuryError {
    /// Contract not initialized
    NotInitialized = 1,
    /// Already initialized
    AlreadyInitialized = 2,
    /// Unauthorized access
    Unauthorized = 3,
    /// Member not found
    MemberNotFound = 4,
    /// Member already exists
    MemberAlreadyExists = 5,
    /// Transaction not found
    TransactionNotFound = 6,
    /// Transaction already executed
    AlreadyExecuted = 7,
    /// Already signed by this member
    AlreadySigned = 8,
    /// Transaction expired
    TransactionExpired = 9,
    /// Threshold not reached
    ThresholdNotReached = 10,
    /// Too many pending proposals
    TooManyPendingProposals = 11,
    /// Invalid threshold (M > N)
    InvalidThreshold = 12,
    /// Cannot remove last owner
    CannotRemoveLastOwner = 13,
    /// Emergency recovery not available
    EmergencyNotAvailable = 14,
    /// Emergency cooldown active
    EmergencyCooldownActive = 15,
    /// Invalid transaction parameters
    InvalidParameters = 16,
    /// Insufficient role level
    InsufficientRole = 17,
    /// Transfer failed
    TransferFailed = 18,
    /// Transaction not approved
    NotApproved = 19,
}
