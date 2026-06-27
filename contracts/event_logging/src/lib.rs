#![no_std]

mod storage;
pub mod types;

use soroban_sdk::{contract, contractimpl, Address, Env, Symbol, Val, Vec};
use crate::storage::*;
use crate::types::*;

#[contract]
pub struct EventLoggingContract;

#[contractimpl]
impl EventLoggingContract {
    /// Initialize the event logging contract with an admin and config.
    pub fn initialize(env: Env, admin: Address, max_events_retained: u32) {
        if has_admin(&env) {
            panic!("Already initialized");
        }
        set_admin(&env, &admin);
        
        let config = Config { max_events_retained };
        set_config(&env, &config);
    }

    /// Update the retention config.
    pub fn update_config(env: Env, admin: Address, max_events_retained: u32) {
        admin.require_auth();
        if admin != get_admin(&env) {
            panic!("Not admin");
        }
        let config = Config { max_events_retained };
        set_config(&env, &config);
    }

    /// Logs an event on-chain and publishes it off-chain.
    pub fn log_event(env: Env, source_contract: Address, topic: Symbol, data: Val) -> u64 {
        source_contract.require_auth();

        let id = increment_event_count(&env);
        let timestamp = env.ledger().timestamp();

        let event = Event {
            id,
            source_contract: source_contract.clone(),
            topic: topic.clone(),
            data: data.clone(),
            timestamp,
        };

        // 1. Store event
        set_event(&env, &event);

        // 2. Update Topic Index
        let mut topic_idx = get_topic_index(&env, &topic);
        topic_idx.push_back(id);
        set_topic_index(&env, &topic, &topic_idx);

        // 3. Update Contract Index
        let mut contract_idx = get_contract_index(&env, &source_contract);
        contract_idx.push_back(id);
        set_contract_index(&env, &source_contract, &contract_idx);

        // 4. Update Analytics
        increment_analytics_topic_count(&env, &topic);

        // 5. Emit native Soroban event for off-chain indexing
        env.events().publish((topic.clone(), source_contract.clone()), data.clone());

        // 6. Prune if needed (keep it simple: if we exceed limit by a lot, we prune)
        // Here we just prune the oldest event if total count > max_events_retained
        // For production, a more sophisticated pruning strategy is needed.
        let config = get_config(&env);
        if config.max_events_retained > 0 {
            // Note: Event count always goes up. The oldest event we might have is (id - max_events_retained).
            // We can try to prune the oldest possible one.
            if id > (config.max_events_retained as u64) {
                let prune_id = id - (config.max_events_retained as u64);
                remove_event(&env, prune_id);
                // Note: We don't remove from topic/contract indices here to save compute, 
                // but queries should handle missing events gracefully.
            }
        }

        id
    }

    /// Fetch a single event by ID.
    pub fn get_event(env: Env, id: u64) -> Option<Event> {
        get_event(&env, id)
    }

    /// Fetch events by topic with pagination.
    pub fn get_events_by_topic(env: Env, topic: Symbol, start: u32, limit: u32) -> Vec<Event> {
        let index = get_topic_index(&env, &topic);
        Self::paginate_index(&env, index, start, limit)
    }

    /// Fetch events by source contract with pagination.
    pub fn get_events_by_contract(env: Env, contract: Address, start: u32, limit: u32) -> Vec<Event> {
        let index = get_contract_index(&env, &contract);
        Self::paginate_index(&env, index, start, limit)
    }
    
    /// Get analytics for a topic.
    pub fn get_topic_analytics(env: Env, topic: Symbol) -> u64 {
        get_analytics_topic_count(&env, &topic)
    }

    /// Helper to paginate through an index of event IDs.
    fn paginate_index(env: &Env, index: Vec<u64>, start: u32, limit: u32) -> Vec<Event> {
        let mut results = Vec::new(env);
        let len = index.len();
        
        if start >= len {
            return results;
        }

        let end = if start + limit > len { len } else { start + limit };

        for i in start..end {
            if let Some(id) = index.get(i) {
                if let Some(event) = get_event(env, id) {
                    results.push_back(event);
                }
            }
        }
        results
    }
}

#[cfg(test)]
mod test;
