use soroban_sdk::{contracttype, Address};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TournamentState {
    Open,
    Started,
    Ended,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TournamentConfig {
    pub admin: Address,
    pub token: Address,
    pub entry_fee: i128,
}

#[contracttype]
#[allow(dead_code)]
pub enum DataKey {
    Config,
    State,
    Participants, // Vector<Address>
    Match(u32),   // Map match_id to Match
    Results,      // Map match_id to Winner Address
    TotalPrize,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub struct Match {
    pub p1: Address,
    pub p2: Address,
    pub winner: Option<Address>,
}
