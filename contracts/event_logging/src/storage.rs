use soroban_sdk::{Address, Env, Symbol, Vec};
use crate::types::{Config, DataKey, Event};

pub fn get_admin(env: &Env) -> Address {
    env.storage().instance().get(&DataKey::Admin).unwrap()
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn has_admin(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Admin)
}

pub fn get_config(env: &Env) -> Config {
    env.storage().instance().get(&DataKey::Config).unwrap()
}

pub fn set_config(env: &Env, config: &Config) {
    env.storage().instance().set(&DataKey::Config, config);
}

pub fn get_event_count(env: &Env) -> u64 {
    env.storage().instance().get(&DataKey::EventCount).unwrap_or(0)
}

pub fn increment_event_count(env: &Env) -> u64 {
    let count = get_event_count(env) + 1;
    env.storage().instance().set(&DataKey::EventCount, &count);
    count
}

pub fn get_event(env: &Env, id: u64) -> Option<Event> {
    env.storage().persistent().get(&DataKey::Event(id))
}

pub fn set_event(env: &Env, event: &Event) {
    env.storage().persistent().set(&DataKey::Event(event.id), event);
}

pub fn remove_event(env: &Env, id: u64) {
    env.storage().persistent().remove(&DataKey::Event(id));
}

pub fn get_topic_index(env: &Env, topic: &Symbol) -> Vec<u64> {
    env.storage().persistent().get(&DataKey::TopicIndex(topic.clone())).unwrap_or_else(|| Vec::new(env))
}

pub fn set_topic_index(env: &Env, topic: &Symbol, index: &Vec<u64>) {
    env.storage().persistent().set(&DataKey::TopicIndex(topic.clone()), index);
}

pub fn get_contract_index(env: &Env, contract: &Address) -> Vec<u64> {
    env.storage().persistent().get(&DataKey::ContractIndex(contract.clone())).unwrap_or_else(|| Vec::new(env))
}

pub fn set_contract_index(env: &Env, contract: &Address, index: &Vec<u64>) {
    env.storage().persistent().set(&DataKey::ContractIndex(contract.clone()), index);
}

pub fn get_analytics_topic_count(env: &Env, topic: &Symbol) -> u64 {
    env.storage().persistent().get(&DataKey::AnalyticsTopicCount(topic.clone())).unwrap_or(0)
}

pub fn increment_analytics_topic_count(env: &Env, topic: &Symbol) {
    let count = get_analytics_topic_count(env, topic) + 1;
    env.storage().persistent().set(&DataKey::AnalyticsTopicCount(topic.clone()), &count);
}
