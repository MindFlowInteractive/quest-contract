use soroban_sdk::{Address, Env, Symbol, Val, Vec};
use crate::types::{DataKey, EngineError, Role, Subscription, SubscriptionEvent, TypeDefinition};

pub fn panic_with_error(_env: &Env, err: EngineError) -> ! {
    panic!("engine error: {}", err as u32);
}

pub fn has_admin(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Admin)
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn get_admin(env: &Env) -> Address {
    env.storage().instance().get(&DataKey::Admin).unwrap()
}

pub fn set_initialized(env: &Env) {
    env.storage().instance().set(&DataKey::Initialized, &true);
}

pub fn is_initialized(env: &Env) -> bool {
    env.storage().instance().get(&DataKey::Initialized).unwrap_or(false)
}

pub fn require_admin(env: &Env, caller: &Address) {
    caller.require_auth();
    if caller != &get_admin(env) {
        panic_with_error(env, EngineError::Unauthorized);
    }
}

pub fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&DataKey::Paused, &paused);
}

pub fn is_paused(env: &Env) -> bool {
    env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
}

pub fn require_not_paused(env: &Env) {
    if is_paused(env) {
        panic_with_error(env, EngineError::Paused);
    }
}

pub fn set_role(env: &Env, address: &Address, role: &Role) {
    env.storage().instance().set(&DataKey::Role(address.clone()), role);
}

pub fn get_role(env: &Env, address: &Address) -> Option<Role> {
    env.storage().instance().get(&DataKey::Role(address.clone()))
}

pub fn has_role(env: &Env, address: &Address, required: &Role) -> bool {
    match get_role(env, address) {
        Some(role) => role as u32 <= required.clone() as u32,
        None => false,
    }
}

pub fn require_role(env: &Env, address: &Address, required: &Role) {
    address.require_auth();
    if !has_role(env, address, required) {
        panic_with_error(env, EngineError::Unauthorized);
    }
}

pub fn register_type(env: &Env, type_def: &TypeDefinition) {
    let key = DataKey::TypeRegistry(type_def.name.clone());
    if env.storage().instance().has(&key) {
        panic_with_error(env, EngineError::TypeAlreadyExists);
    }
    env.storage().instance().set(&key, type_def);
}

pub fn get_type_definition(env: &Env, type_name: &Symbol) -> TypeDefinition {
    env.storage().instance()
        .get(&DataKey::TypeRegistry(type_name.clone()))
        .unwrap_or_else(|| panic_with_error(env, EngineError::TypeNotFound))
}

pub fn type_exists(env: &Env, type_name: &Symbol) -> bool {
    env.storage().instance().has(&DataKey::TypeRegistry(type_name.clone()))
}

pub fn save_record(env: &Env, type_name: &Symbol, id: &Val, data: &Val) {
    let key = DataKey::Record(type_name.clone(), id.clone());
    env.storage().persistent().set(&key, data);
}

pub fn get_record(env: &Env, type_name: &Symbol, id: &Val) -> Option<Val> {
    let key = DataKey::Record(type_name.clone(), id.clone());
    env.storage().persistent().get(&key)
}

pub fn remove_record(env: &Env, type_name: &Symbol, id: &Val) {
    let key = DataKey::Record(type_name.clone(), id.clone());
    env.storage().persistent().remove(&key);
}

pub fn next_record_counter(env: &Env, type_name: &Symbol) -> u32 {
    let count: u32 = env.storage().instance()
        .get(&DataKey::RecordCounter(type_name.clone()))
        .unwrap_or(0);
    let next = count + 1;
    env.storage().instance().set(&DataKey::RecordCounter(type_name.clone()), &next);
    next
}

pub fn index_record(env: &Env, type_name: &Symbol, index_pos: u32, id: &Val) {
    let key = DataKey::RecordIndex(type_name.clone(), index_pos);
    env.storage().persistent().set(&key, id);
}

pub fn get_indexed_record(env: &Env, type_name: &Symbol, index_pos: u32) -> Option<Val> {
    let key = DataKey::RecordIndex(type_name.clone(), index_pos);
    env.storage().persistent().get(&key)
}

pub fn record_count(env: &Env, type_name: &Symbol) -> u32 {
    env.storage().instance()
        .get(&DataKey::RecordCounter(type_name.clone()))
        .unwrap_or(0)
}

pub fn next_subscription_id(env: &Env) -> u64 {
    let count: u64 = env.storage().instance()
        .get(&DataKey::SubscriptionCount)
        .unwrap_or(0);
    let next = count + 1;
    env.storage().instance().set(&DataKey::SubscriptionCount, &next);
    next
}

pub fn save_subscription(env: &Env, sub: &Subscription) {
    env.storage().persistent().set(&DataKey::Subscription(sub.id), sub);
}

pub fn get_subscription(env: &Env, id: u64) -> Subscription {
    env.storage().persistent()
        .get(&DataKey::Subscription(id))
        .unwrap_or_else(|| panic_with_error(env, EngineError::SubscriptionNotFound))
}

pub fn save_subscription_event(env: &Env, sub_id: u64, event: &SubscriptionEvent) {
    let key = DataKey::SubscriptionEvent(sub_id, event.event_id);
    env.storage().persistent().set(&key, event);
    let count: u64 = env.storage().persistent()
        .get(&DataKey::SubscriptionEventCount(sub_id))
        .unwrap_or(0);
    env.storage().persistent().set(&DataKey::SubscriptionEventCount(sub_id), &(count + 1));
}

pub fn get_subscription_event_count(env: &Env, sub_id: u64) -> u64 {
    env.storage().persistent()
        .get(&DataKey::SubscriptionEventCount(sub_id))
        .unwrap_or(0)
}

pub fn get_subscription_event(env: &Env, sub_id: u64, event_id: u64) -> Option<SubscriptionEvent> {
    env.storage().persistent().get(&DataKey::SubscriptionEvent(sub_id, event_id))
}

pub fn deactivate_subscription(env: &Env, sub_id: u64) {
    let mut sub = get_subscription(env, sub_id);
    sub.active = false;
    save_subscription(env, &sub);
}