use soroban_sdk::{contracterror, contracttype, Address, BytesN};

/// Errors returned by the blind auction contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum BlindAuctionError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    AuctionNotFound = 4,
    InvalidDeadlines = 5,
    InvalidReserve = 6,
    InvalidDeposit = 7,
    CommitPeriodClosed = 8,
    RevealPeriodNotStarted = 9,
    RevealPeriodClosed = 10,
    CommitmentMissing = 11,
    BidAlreadyCommitted = 12,
    BidAlreadyRevealed = 13,
    CommitmentMismatch = 14,
    InvalidBidAmount = 15,
    RevealNotEnded = 16,
    AuctionAlreadyClosed = 17,
    AuctionCancelled = 18,
    NoWinningBid = 19,
    NothingToWithdraw = 20,
    ProceedsAlreadyWithdrawn = 21,
    RefundNotAvailable = 22,
    AlreadyRefunded = 23,
    NotTheWinner = 24,
    NftAlreadyTransferred = 25,
    CancelWindowClosed = 26,
    BidNotRevealed = 27,
    AuctionAlreadyExists = 28,
}

/// Contract wide configuration.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
}

/// A sealed-bid ("blind") auction for a single NFT.
///
/// Timelines are expressed as absolute ledger timestamps:
/// * `commit_deadline` - last ledger in which a sealed bid may be committed.
/// * `reveal_deadline` - last ledger in which a sealed bid may be revealed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Auction {
    pub auction_id: u64,
    pub seller: Address,
    pub nft_contract: Address,
    pub nft_id: BytesN<32>,
    pub payment_token: Address,
    pub reserve_price: i128,
    pub bid_deposit: i128,
    pub commit_deadline: u64,
    pub reveal_deadline: u64,
    pub created_at: u64,
    pub bid_count: u32,
    pub reveal_count: u32,
    pub highest_bid: i128,
    pub winning_amount: i128,
    pub escrow_balance: i128,
    pub total_refunded: i128,
    pub winner: Option<Address>,
    pub closed_at: Option<u64>,
    pub closed: bool,
    pub cancelled: bool,
    pub proceeds_paid: bool,
    pub nft_transferred: bool,
}

/// Internal per-bidder record. The plaintext bid amount only ever appears in
/// `revealed_amount`, which stays `None` until the bidder reveals the salt.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Commitment {
    pub bidder: Address,
    pub auction_id: u64,
    pub commitment: BytesN<32>,
    pub committed_at: u64,
    pub revealed_amount: Option<i128>,
    pub revealed_at: Option<u64>,
    pub escrow_amount: i128,
    pub refunded: bool,
}

/// Public projection of a commitment. Deliberately carries no plaintext bid so
/// that the commitment hash can always be inspected without leaking the bid.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitmentInfo {
    pub bidder: Address,
    pub auction_id: u64,
    pub commitment: BytesN<32>,
    pub committed_at: u64,
    pub revealed: bool,
    pub refunded: bool,
}

/// The auction winner, as decided at settlement time.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Winner {
    pub bidder: Address,
    pub amount: i128,
    pub committed_at: u64,
    pub revealed_at: u64,
}

/// Per-auction analytics derived purely from stored state.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuctionStats {
    pub auction_id: u64,
    pub bid_count: u32,
    pub reveal_count: u32,
    pub commit_ratio_bps: u32,
    pub reserve_price: i128,
    pub highest_bid: i128,
    pub winning_amount: i128,
    pub escrow_balance: i128,
    pub total_refunded: i128,
    pub winner: Option<Address>,
    pub closed: bool,
    pub cancelled: bool,
    pub created_at: u64,
    pub closed_at: Option<u64>,
    pub commit_window: u64,
    pub reveal_window: u64,
}

/// Contract wide analytics, maintained incrementally on every state transition.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlobalStats {
    pub total_auctions: u64,
    pub total_bids: u64,
    pub total_reveals: u64,
    pub closed_count: u64,
    pub cancelled_count: u64,
    pub auctions_with_winner: u64,
    pub total_volume: i128,
    pub highest_bid: i128,
    pub total_escrowed: i128,
    pub total_refunded: i128,
    pub outstanding_escrow: i128,
}
