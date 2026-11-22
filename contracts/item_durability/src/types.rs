use soroban_sdk::{contracttype, Address};

/// On-chain state for a durable item (#400, #401).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    pub id: u64,
    pub owner: Address,
    pub max_durability: u32,
    pub durability: u32,
    /// Token cost charged per point restored by `repair`.
    pub repair_cost_per_point: i128,
    /// Token returned to the owner when a broken item is salvaged.
    pub material_token: Address,
    pub salvage_amount: i128,
    /// Set to true when durability first reaches zero.
    pub broken: bool,
    /// Set to true after salvage; the record is then removed.
    pub salvaged: bool,
}
