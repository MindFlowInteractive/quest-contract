#![no_std]
use soroban_sdk::{contracttype, Address, Bytes, BytesN, String};

/// Chain identifiers for supported source chains
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChainId {
    Stellar = 0,
    Ethereum = 1,
    BinanceSmartChain = 2,
    Polygon = 3,
    Avalanche = 4,
    Solana = 5,
}

/// Status of a wrap/unwrap operation
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationStatus {
    Pending = 0,
    Completed = 1,
    Failed = 2,
    Cancelled = 3,
}

/// Contract configuration / initialization params
#[contracttype]
#[derive(Clone, Debug)]
pub struct WrappedTokenConfig {
    /// Contract admin
    pub admin: Address,
    /// Name of the wrapped token (e.g. "Wrapped ETH")
    pub name: String,
    /// Symbol (e.g. "WETH")
    pub symbol: String,
    /// Number of decimal places
    pub decimals: u32,
    /// Source chain of the underlying asset
    pub source_chain: ChainId,
    /// Original asset identifier on source chain (contract address / mint address)
    pub source_asset_id: Bytes,
    /// Fee collector address
    pub fee_collector: Address,
    /// Bridge fee in basis points (e.g. 30 = 0.3%)
    pub fee_bps: u32,
    /// Minimum fee (in smallest token unit)
    pub min_fee: i128,
    /// Whether contract is paused
    pub paused: bool,
    /// Required number of operator confirmations for a wrap to be minted
    pub required_confirmations: u32,
}

/// A pending wrap (mint) request that needs operator confirmation
#[contracttype]
#[derive(Clone, Debug)]
pub struct WrapRequest {
    /// Unique request nonce
    pub nonce: u64,
    /// Stellar recipient address
    pub recipient: Address,
    /// Gross amount (before fee deduction)
    pub gross_amount: i128,
    /// Fee to be taken
    pub fee_amount: i128,
    /// Net amount to mint
    pub net_amount: i128,
    /// Source chain where lock occurred
    pub source_chain: ChainId,
    /// Source chain transaction ID proving the lock
    pub source_tx_id: BytesN<32>,
    /// Status of this request
    pub status: OperationStatus,
    /// Timestamp of request submission (ledger timestamp)
    pub created_at: u64,
    /// Submitting operator
    pub operator: Address,
}

/// An unwrap (burn) request — user wants their underlying asset back on source chain
#[contracttype]
#[derive(Clone, Debug)]
pub struct UnwrapRequest {
    /// Unique request nonce
    pub nonce: u64,
    /// User burning their wrapped tokens
    pub user: Address,
    /// Gross amount burned
    pub gross_amount: i128,
    /// Fee taken
    pub fee_amount: i128,
    /// Net amount to release on source chain
    pub net_amount: i128,
    /// Target chain to release assets on
    pub target_chain: ChainId,
    /// Target chain recipient address (bytes, supports multiple address formats)
    pub target_recipient: Bytes,
    /// Status of this unwrap
    pub status: OperationStatus,
    /// Timestamp
    pub created_at: u64,
}

/// Aggregate custody stats for the contract
#[contracttype]
#[derive(Clone, Debug)]
pub struct CustodyInfo {
    /// Total wrapped supply currently outstanding
    pub total_supply: i128,
    /// Total fees collected (in wrapped token units, pending withdrawal)
    pub total_fees_collected: i128,
    /// Total wrap operations completed
    pub total_wraps: u64,
    /// Total unwrap operations initiated
    pub total_unwraps: u64,
    /// Last operation timestamp
    pub last_operation_at: u64,
}

/// Error codes for the wrapped tokens contract
#[soroban_sdk::contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum WrappedTokenError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    ContractPaused = 4,
    ContractNotPaused = 5,
    InvalidAmount = 6,
    InsufficientBalance = 7,
    InvalidNonce = 8,
    NonceAlreadyUsed = 9,
    RequestNotFound = 10,
    RequestAlreadyProcessed = 11,
    OperatorNotFound = 12,
    OperatorAlreadyExists = 13,
    MaxOperatorsReached = 14,
    InvalidFee = 15,
    InvalidChain = 16,
    ArithmeticOverflow = 17,
    InsufficientConfirmations = 18,
    AlreadyConfirmed = 19,
}
