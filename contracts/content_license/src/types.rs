use soroban_sdk::{contracttype, Address};

/// The usage rights a creator publishes for a puzzle (#397).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LicenseKind {
    FreeToPlay,
    PaidPerPlay,
    RemixAllowed,
}

/// Immutable, versioned terms published by a puzzle creator (#397).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LicenseTerms {
    pub puzzle_id: u64,
    pub version: u32,
    pub creator: Address,
    pub kind: LicenseKind,
    /// Price charged on purchase. Zero for free terms.
    pub price: i128,
    /// Lifetime in seconds. Zero means perpetual.
    pub duration_secs: u64,
    /// Whether the creator may revoke licences bought under these terms.
    pub revocable: bool,
    /// Whether revocation must refund the purchase price.
    pub refund_on_revoke: bool,
    pub published_at: u64,
}

/// A player's licence, bound to the exact terms version purchased (#398).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct License {
    pub holder: Address,
    pub puzzle_id: u64,
    pub version: u32,
    pub purchased_at: u64,
    /// Zero means perpetual.
    pub expires_at: u64,
    pub revoked: bool,
}
