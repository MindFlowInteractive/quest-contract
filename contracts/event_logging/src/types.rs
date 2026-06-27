use soroban_sdk::{contracttype, Address, Symbol, Val, Vec};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Event {
    pub id: u64,
    pub source_contract: Address,
    pub topic: Symbol,
    pub data: Val,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub max_events_retained: u32,
}

#[contracttype]
pub enum DataKey {
    Admin,
    Config,
    EventCount,
    Event(u64), // event payload
    TopicIndex(Symbol), // Vec<u64> of event IDs
    ContractIndex(Address), // Vec<u64> of event IDs
    AnalyticsTopicCount(Symbol), // u64 total count
}
