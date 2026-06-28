use soroban_sdk::{contracterror, contracttype, Address, BytesN, Map, Symbol, Vec};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum IntegrationError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    Paused = 4,

    // Source failures
    SourceNotConfigured = 10,
    SourceFetchFailed = 11,
    SourceStale = 12,
    InvalidPrice = 13,
    DeviationTooLarge = 14,

    // Cache
    CacheStale = 20,
    CacheNotFound = 21,

    // Signatures
    InvalidSignature = 30,
    InsufficientSignatures = 31,
    Disputed = 32,

    // Aggregation
    InsufficientValidSources = 40,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
    pub signers: Map<BytesN<32>, bool>,
    pub threshold: u32,
    pub paused: bool,

    pub stale_threshold_secs: u64,
    pub max_deviation_bps: u32,

    // Cache TTL
    pub cache_ttl_secs: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CachedPrice {
    pub price: i128,
    pub timestamp: u64,
    pub round_id: u64,
    pub expires_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OracleSource {
    /// Calls contracts/oracle with signature validation.
    /// Expects the external contract's `get_price(env, asset) -> PriceData` or compatible.
    SignedEd25519(Address, Symbol),

    /// Calls contracts/oracle_price_feed with median price logic.
    MedianFeed(Address, Symbol),
}


#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetSourceConfig {
    /// ordered list of sources to try; we will aggregate across all valid sources (best-effort)
    pub sources: Vec<OracleSource>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PriceSnapshot {
    pub price: i128,
    pub timestamp: u64,
    pub round_id: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedPayload {
    pub asset: Symbol,
    pub price: i128,
    pub timestamp: u64,
    pub round_id: u64,
    pub contract_address: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmergencyConfig {
    pub active: bool,
    pub price: i128,
    pub timestamp: u64,
    pub round_id: u64,
}
