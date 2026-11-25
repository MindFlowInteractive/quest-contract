use soroban_sdk::{contracttype, Address};

/// A single points-earning event in a player's history.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PointsEntry {
    /// Points awarded by this event.
    pub amount: u64,
    /// Reason code describing why points were awarded
    /// (e.g. puzzle completed, achievement unlocked).
    pub reason: u32,
    /// Contract that awarded the points.
    pub awarder: Address,
    /// Ledger timestamp of the event.
    pub timestamp: u64,
    /// Ledger timestamp at which these points expire; `0` means they never
    /// expire.
    pub expires_at: u64,
}

#[contracttype]
pub enum DataKey {
    Admin,
    /// Whether an address is allowed to award points.
    Awarder(Address),
    /// A player's current points balance.
    Balance(Address),
    /// A player's full earn history.
    History(Address),
}
