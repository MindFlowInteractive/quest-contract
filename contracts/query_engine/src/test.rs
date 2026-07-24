#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, IntoVal, Symbol, Vec};

fn setup() -> (Env, QueryEngineContractClient<'static>, Address) {
    let env = Env::default();
    let contract_id = env.register_contract(None, QueryEngineContract);
    let client = QueryEngineContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&admin);
    client.set_role(&admin, &admin, &Role::Operator);

    (env, client, admin)
}

fn register_player_type(env: &Env, client: &QueryEngineContractClient<'static>, admin: &Address) {
    let mut fields: Vec<FieldDefinition> = Vec::new(env);
    fields.push_back(FieldDefinition {
        name: Symbol::new(env, "name"),
        field_type: FieldType::String,
        required: true,
    });
    fields.push_back(FieldDefinition {
        name: Symbol::new(env, "score"),
        field_type: FieldType::U64,
        required: false,
    });
    let type_def = TypeDefinition {
        name: Symbol::new(env, "Player"),
        fields,
        description: String::from_str(env, "A player profile"),
    };
    client.register_type(admin, &type_def);
}

#[test]
fn test_initialize() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, QueryEngineContract);
    let client = QueryEngineContractClient::new(&env, &contract_id);

    assert!(!client.is_initialized());
    env.mock_all_auths();
    client.initialize(&admin);
    assert!(client.is_initialized());
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_initialize_twice_panics() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, QueryEngineContract);
    let client = QueryEngineContractClient::new(&env, &contract_id);

    env.mock_all_auths();
    client.initialize(&admin);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.initialize(&admin);
    }));
    assert!(result.is_err());
}

#[test]
fn test_pause_unpause() {
    let (_env, client, admin) = setup();

    assert!(!client.is_paused());
    client.pause(&admin);
    assert!(client.is_paused());
    client.unpause(&admin);
    assert!(!client.is_paused());
}

#[test]
fn test_register_type() {
    let (_env, client, admin) = setup();

    register_player_type(&_env, &client, &admin);

    let type_name = Symbol::new(&_env, "Player");
    assert!(client.has_type(&type_name));
    let retrieved = client.get_type(&type_name);
    assert_eq!(retrieved.name, type_name);
}

#[test]
fn test_register_duplicate_type_panics() {
    let (_env, client, admin) = setup();
    register_player_type(&_env, &client, &admin);

    let mut fields: Vec<FieldDefinition> = Vec::new(&_env);
    fields.push_back(FieldDefinition {
        name: Symbol::new(&_env, "name"),
        field_type: FieldType::String,
        required: true,
    });
    let type_def = TypeDefinition {
        name: Symbol::new(&_env, "Player"),
        fields,
        description: String::from_str(&_env, "duplicate"),
    };

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.register_type(&admin, &type_def);
    }));
    assert!(result.is_err());
}

#[test]
fn test_mutation_create() {
    let (_env, client, admin) = setup();
    register_player_type(&_env, &client, &admin);

    let mut data: Vec<(Symbol, Val)> = Vec::new(&_env);
    data.push_back((
        Symbol::new(&_env, "name"),
        String::from_str(&_env, "Alice").into_val(&_env),
    ));
    data.push_back((
        Symbol::new(&_env, "score"),
        (100u64).into_val(&_env),
    ));

    let mutation = MutationInput {
        type_name: Symbol::new(&_env, "Player"),
        operation: MutationOp::Create,
        id: None,
        data,
    };

    let result = client.mutate(&admin, &mutation);
    assert!(result.success);
    assert!(result.id.is_some());
}

#[test]
fn test_query_empty() {
    let (_env, client, admin) = setup();
    register_player_type(&_env, &client, &admin);

    let query = QueryInput {
        type_name: Symbol::new(&_env, "Player"),
        filters: Vec::new(&_env),
        sort: None,
        pagination: Pagination {
            limit: 10,
            cursor: None,
        },
        fields: Vec::new(&_env),
    };

    let result = client.query(&query);
    assert_eq!(result.total_count, 0);
    assert!(result.items.is_empty());
    assert!(!result.has_more);
}

#[test]
fn test_query_with_data() {
    let (_env, client, admin) = setup();
    register_player_type(&_env, &client, &admin);

    let mut data: Vec<(Symbol, Val)> = Vec::new(&_env);
    data.push_back((
        Symbol::new(&_env, "name"),
        String::from_str(&_env, "Alice").into_val(&_env),
    ));
    data.push_back((
        Symbol::new(&_env, "score"),
        (100u64).into_val(&_env),
    ));

    let mutation = MutationInput {
        type_name: Symbol::new(&_env, "Player"),
        operation: MutationOp::Create,
        id: None,
        data,
    };
    client.mutate(&admin, &mutation);

    let query = QueryInput {
        type_name: Symbol::new(&_env, "Player"),
        filters: Vec::new(&_env),
        sort: None,
        pagination: Pagination {
            limit: 10,
            cursor: None,
        },
        fields: Vec::new(&_env),
    };

    let result = client.query(&query);
    assert_eq!(result.total_count, 1);
    assert_eq!(result.items.len(), 1);
    assert!(!result.has_more);
}

#[test]
fn test_query_pagination() {
    let (_env, client, admin) = setup();
    register_player_type(&_env, &client, &admin);

    for i in 0..5u64 {
        let mut data: Vec<(Symbol, Val)> = Vec::new(&_env);
        let name = String::from_str(&_env, &format!("Player{}", i));
        data.push_back((Symbol::new(&_env, "name"), name.into_val(&_env)));
        data.push_back((Symbol::new(&_env, "score"), (i * 10).into_val(&_env)));

        let mutation = MutationInput {
            type_name: Symbol::new(&_env, "Player"),
            operation: MutationOp::Create,
            id: None,
            data,
        };
        client.mutate(&admin, &mutation);
    }

    let query = QueryInput {
        type_name: Symbol::new(&_env, "Player"),
        filters: Vec::new(&_env),
        sort: None,
        pagination: Pagination {
            limit: 2,
            cursor: None,
        },
        fields: Vec::new(&_env),
    };

    let result = client.query(&query);
    assert_eq!(result.total_count, 5);
    assert_eq!(result.items.len(), 2);
    assert!(result.has_more);
    assert!(result.next_cursor.is_some());
}

#[test]
fn test_batch_query() {
    let (_env, client, admin) = setup();
    register_player_type(&_env, &client, &admin);

    let query1 = QueryInput {
        type_name: Symbol::new(&_env, "Player"),
        filters: Vec::new(&_env),
        sort: None,
        pagination: Pagination { limit: 10, cursor: None },
        fields: Vec::new(&_env),
    };
    let query2 = QueryInput {
        type_name: Symbol::new(&_env, "Player"),
        filters: Vec::new(&_env),
        sort: None,
        pagination: Pagination { limit: 5, cursor: None },
        fields: Vec::new(&_env),
    };

    let batch_input = BatchQueryInput {
        queries: vec![&_env, query1, query2],
    };

    let batch_result = client.batch_query(&batch_input);
    assert_eq!(batch_result.results.len(), 2);
}

#[test]
fn test_subscription_flow() {
    let (_env, client, admin) = setup();

    let subscriber = Address::generate(&_env);
    let source = Address::generate(&_env);

    let sub_input = SubscriptionInput {
        source_contract: source.clone(),
        topic: Symbol::new(&_env, "quest_done"),
        subscriber: subscriber.clone(),
    };

    let sub_id = client.subscribe(&subscriber, &sub_input);
    assert!(sub_id > 0);

    let sub = client.get_subscription(&sub_id);
    assert_eq!(sub.id, sub_id);
    assert_eq!(sub.subscriber, subscriber);
    assert!(sub.active);

    let event_data: Val = (42u64).into_val(&_env);
    let event_id = client.emit_subscription_event(&admin, &sub_id, &event_data);
    assert_eq!(event_id, 1);

    let events = client.get_subscription_events(&sub_id, &0, &10);
    assert_eq!(events.len(), 1);

    client.unsubscribe(&subscriber, &sub_id);
    let sub_after = client.get_subscription(&sub_id);
    assert!(!sub_after.active);
}

#[test]
fn test_batch_get() {
    let (_env, client, admin) = setup();
    register_player_type(&_env, &client, &admin);

    let mut data: Vec<(Symbol, Val)> = Vec::new(&_env);
    let name = String::from_str(&_env, "Alice");
    data.push_back((Symbol::new(&_env, "name"), name.into_val(&_env)));
    data.push_back((Symbol::new(&_env, "score"), (100u64).into_val(&_env)));

    let mutation = MutationInput {
        type_name: Symbol::new(&_env, "Player"),
        operation: MutationOp::Create,
        id: None,
        data,
    };
    let result = client.mutate(&admin, &mutation);
    let created_id = result.id.unwrap();

    let ids = vec![&_env, created_id];
    let records = client.batch_get(&Symbol::new(&_env, "Player"), &ids);
    assert_eq!(records.len(), 1);
}

#[test]
fn test_role_management() {
    let (_env, client, admin) = setup();

    let operator = Address::generate(&_env);
    client.set_role(&admin, &operator, &Role::Operator);

    let role = client.get_role(&operator);
    assert_eq!(role, Some(Role::Operator));
}

#[test]
fn test_unauthorized_access_panics() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let contract_id = env.register_contract(None, QueryEngineContract);
    let client = QueryEngineContractClient::new(&env, &contract_id);

    env.mock_all_auths();
    client.initialize(&admin);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.pause(&attacker);
    }));
    assert!(result.is_err());
}