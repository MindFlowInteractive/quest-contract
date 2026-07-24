#![no_std]

mod storage;
pub mod types;

use soroban_sdk::{contract, contractimpl, Address, Env, IntoVal, Symbol, Val, Vec};
use crate::storage::*;
use crate::types::*;

#[contract]
pub struct QueryEngineContract;

#[contractimpl]
impl QueryEngineContract {
    // ──────────────────────────────────────────────────
    // INITIALIZATION
    // ──────────────────────────────────────────────────

    pub fn initialize(env: Env, admin: Address) {
        if is_initialized(&env) {
            panic_with_error(&env, EngineError::AlreadyInitialized);
        }
        admin.require_auth();
        set_admin(&env, &admin);
        set_initialized(&env);
        set_role(&env, &admin, &Role::Admin);
    }

    pub fn is_initialized(env: Env) -> bool {
        is_initialized(&env)
    }

    pub fn get_admin(env: Env) -> Address {
        get_admin(&env)
    }

    // ──────────────────────────────────────────────────
    // PAUSE / UNPAUSE
    // ──────────────────────────────────────────────────

    pub fn pause(env: Env, caller: Address) {
        require_admin(&env, &caller);
        if is_paused(&env) {
            panic_with_error(&env, EngineError::Paused);
        }
        set_paused(&env, true);
    }

    pub fn unpause(env: Env, caller: Address) {
        require_admin(&env, &caller);
        if !is_paused(&env) {
            panic_with_error(&env, EngineError::NotPaused);
        }
        set_paused(&env, false);
    }

    pub fn is_paused(env: Env) -> bool {
        is_paused(&env)
    }

    // ──────────────────────────────────────────────────
    // ROLE MANAGEMENT
    // ──────────────────────────────────────────────────

    pub fn set_role(env: Env, caller: Address, target: Address, role: Role) {
        require_admin(&env, &caller);
        set_role(&env, &target, &role);
    }

    pub fn get_role(env: Env, address: Address) -> Option<Role> {
        get_role(&env, &address)
    }

    // ──────────────────────────────────────────────────
    // SCHEMA REGISTRATION
    // ──────────────────────────────────────────────────

    pub fn register_type(env: Env, caller: Address, type_def: TypeDefinition) {
        require_not_paused(&env);
        require_role(&env, &caller, &Role::Operator);
        register_type(&env, &type_def);
    }

    pub fn get_type(env: Env, type_name: Symbol) -> TypeDefinition {
        get_type_definition(&env, &type_name)
    }

    pub fn has_type(env: Env, type_name: Symbol) -> bool {
        type_exists(&env, &type_name)
    }

    // ──────────────────────────────────────────────────
    // DATA MUTATIONS
    // ──────────────────────────────────────────────────

    pub fn mutate(env: Env, caller: Address, input: MutationInput) -> MutationResult {
        require_not_paused(&env);
        require_role(&env, &caller, &Role::Operator);
        caller.require_auth();

        match input.operation {
            MutationOp::Create => {
                let id: Val = env.ledger().timestamp().into_val(&env);
                let data: Val = input.data.into_val(&env);
                save_record(&env, &input.type_name, &id, &data);
                let pos = next_record_counter(&env, &input.type_name);
                index_record(&env, &input.type_name, pos, &id);
                MutationResult {
                    success: true,
                    id: Some(id),
                    error: None,
                }
            }
            MutationOp::Update => {
                let id = input.id.unwrap_or_else(|| panic_with_error(&env, EngineError::InvalidMutation));
                let data: Val = input.data.into_val(&env);
                save_record(&env, &input.type_name, &id, &data);
                MutationResult {
                    success: true,
                    id: Some(id),
                    error: None,
                }
            }
            MutationOp::Delete => {
                let id = input.id.unwrap_or_else(|| panic_with_error(&env, EngineError::InvalidMutation));
                remove_record(&env, &input.type_name, &id);
                MutationResult {
                    success: true,
                    id: Some(id),
                    error: None,
                }
            }
        }
    }

    // ──────────────────────────────────────────────────
    // QUERY EXECUTION
    // ──────────────────────────────────────────────────

    pub fn query(env: Env, input: QueryInput) -> QueryResult {
        let _type_def = get_type_definition(&env, &input.type_name);

        let total = record_count(&env, &input.type_name);
        let limit = input.pagination.limit.min(MAX_PAGE_SIZE);

        let start_idx = match &input.pagination.cursor {
            Some(cursor_val) => {
                let cursor_u32: u32 = cursor_val.clone().try_into().unwrap_or(0);
                cursor_u32
            }
            None => 0u32,
        };

        let end_idx = (start_idx + limit).min(total);
        let mut items: Vec<Val> = Vec::new(&env);

        for i in start_idx..end_idx {
            let pos = if input.sort.is_some() && input.sort.clone().unwrap().direction == SortDirection::Desc {
                total - 1 - i
            } else {
                i
            };
            if let Some(id) = get_indexed_record(&env, &input.type_name, pos + 1) {
                if let Some(record) = get_record(&env, &input.type_name, &id) {
                    if Self::matches_filters(&record, &input.filters) {
                        let projected = Self::project_fields(&env, &record, &input.fields);
                        items.push_back(projected);
                    }
                }
            }
        }

        let has_more = end_idx < total;
        let next_cursor = if has_more {
            Some((end_idx as u64).into_val(&env))
        } else {
            None
        };

        QueryResult {
            items,
            total_count: total,
            next_cursor,
            has_more,
        }
    }

    // ──────────────────────────────────────────────────
    // BATCH QUERY
    // ──────────────────────────────────────────────────

    pub fn batch_query(env: Env, input: BatchQueryInput) -> BatchQueryResult {
        if input.queries.len() > MAX_BATCH_SIZE {
            panic_with_error(&env, EngineError::BatchLimitExceeded);
        }

        let mut results: Vec<QueryResult> = Vec::new(&env);
        for i in 0..input.queries.len() {
            let q = input.queries.get(i).unwrap();
            results.push_back(Self::query(env.clone(), q));
        }

        BatchQueryResult { results }
    }

    // ──────────────────────────────────────────────────
    // BATCH GET BY IDS
    // ──────────────────────────────────────────────────

    pub fn batch_get(env: Env, type_name: Symbol, ids: Vec<Val>) -> Vec<Val> {
        let mut results: Vec<Val> = Vec::new(&env);
        for i in 0..ids.len() {
            let id = ids.get(i).unwrap();
            if let Some(record) = get_record(&env, &type_name, &id) {
                results.push_back(record);
            }
        }
        results
    }

    // ──────────────────────────────────────────────────
    // SUBSCRIPTIONS
    // ──────────────────────────────────────────────────

    pub fn subscribe(env: Env, caller: Address, input: SubscriptionInput) -> u64 {
        require_not_paused(&env);
        caller.require_auth();
        if caller != input.subscriber {
            panic_with_error(&env, EngineError::Unauthorized);
        }

        let id = next_subscription_id(&env);
        let sub = Subscription {
            id,
            source_contract: input.source_contract.clone(),
            topic: input.topic.clone(),
            subscriber: input.subscriber.clone(),
            created_at: env.ledger().timestamp(),
            active: true,
        };
        save_subscription(&env, &sub);
        id
    }

    pub fn unsubscribe(env: Env, caller: Address, subscription_id: u64) {
        caller.require_auth();
        let _sub = get_subscription(&env, subscription_id);
        if caller != _sub.subscriber && caller != get_admin(&env) {
            panic_with_error(&env, EngineError::Unauthorized);
        }
        deactivate_subscription(&env, subscription_id);
    }

    pub fn get_subscription(env: Env, subscription_id: u64) -> Subscription {
        get_subscription(&env, subscription_id)
    }

    pub fn emit_subscription_event(
        env: Env,
        caller: Address,
        subscription_id: u64,
        data: Val,
    ) -> u64 {
        require_not_paused(&env);
        require_role(&env, &caller, &Role::Operator);

        let _sub = get_subscription(&env, subscription_id);
        if !_sub.active {
            panic_with_error(&env, EngineError::SubscriptionNotFound);
        }

        let count = get_subscription_event_count(&env, subscription_id);
        let event_id = count + 1;
        let event = SubscriptionEvent {
            subscription_id,
            event_id,
            timestamp: env.ledger().timestamp(),
            data,
        };
        save_subscription_event(&env, subscription_id, &event);
        event_id
    }

    pub fn get_subscription_events(
        env: Env,
        subscription_id: u64,
        start: u64,
        limit: u64,
    ) -> Vec<SubscriptionEvent> {
        let _sub = get_subscription(&env, subscription_id);
        let total = get_subscription_event_count(&env, subscription_id);
        let end = (start + limit).min(total);
        let mut events: Vec<SubscriptionEvent> = Vec::new(&env);
        for i in start..end {
            let event_id = i + 1;
            if let Some(event) = get_subscription_event(&env, subscription_id, event_id) {
                events.push_back(event);
            }
        }
        events
    }

    // ──────────────────────────────────────────────────
    // INTERNAL HELPERS
    // ──────────────────────────────────────────────────

    fn matches_filters(data: &Val, filters: &Vec<Filter>) -> bool {
        if filters.is_empty() {
            return true;
        }
        for i in 0..filters.len() {
            let f = filters.get(i).unwrap();
            if !Self::apply_filter(data, &f) {
                return false;
            }
        }
        true
    }

    fn apply_filter(_data: &Val, _filter: &Filter) -> bool {
        true
    }

    fn project_fields(_env: &Env, data: &Val, _fields: &Vec<Symbol>) -> Val {
        data.clone()
    }
}

#[cfg(test)]
mod test;