#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, IntoVal, Symbol};

fn setup() -> (Env, EventLoggingContractClient<'static>, Address) {
    let env = Env::default();
    let contract_id = env.register_contract(None, EventLoggingContract);
    let client = EventLoggingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    
    // Retain up to 3 events
    client.initialize(&admin, &3);
    
    (env, client, admin)
}

#[test]
fn test_log_and_get_event() {
    let (env, client, _) = setup();
    let source = Address::generate(&env);
    let topic = Symbol::new(&env, "transfer");
    let data = 100u64.into_val(&env);

    env.mock_all_auths();

    let event_id = client.log_event(&source, &topic, &data);
    assert_eq!(event_id, 1);

    let event = client.get_event(&event_id).unwrap();
    assert_eq!(event.id, 1);
    assert_eq!(event.source_contract, source);
    assert_eq!(event.topic, topic);
    assert_eq!(event.data, data);
}

#[test]
fn test_indexing_and_pagination() {
    let (env, client, _) = setup();
    let source1 = Address::generate(&env);
    let source2 = Address::generate(&env);
    let topic1 = Symbol::new(&env, "mint");
    let topic2 = Symbol::new(&env, "burn");

    env.mock_all_auths();

    client.log_event(&source1, &topic1, &10u64.into_val(&env));
    client.log_event(&source2, &topic1, &20u64.into_val(&env));
    client.log_event(&source1, &topic2, &30u64.into_val(&env));

    // Test topic index
    let events_topic1 = client.get_events_by_topic(&topic1, &0, &10);
    assert_eq!(events_topic1.len(), 2);
    
    // Test contract index
    let events_source1 = client.get_events_by_contract(&source1, &0, &10);
    assert_eq!(events_source1.len(), 2);

    // Test pagination limit
    let paginated = client.get_events_by_topic(&topic1, &0, &1);
    assert_eq!(paginated.len(), 1);

    // Test pagination start
    let paginated2 = client.get_events_by_topic(&topic1, &1, &10);
    assert_eq!(paginated2.len(), 1);
}

#[test]
fn test_analytics() {
    let (env, client, _) = setup();
    let source = Address::generate(&env);
    let topic = Symbol::new(&env, "buy");

    env.mock_all_auths();

    client.log_event(&source, &topic, &1u64.into_val(&env));
    client.log_event(&source, &topic, &2u64.into_val(&env));

    assert_eq!(client.get_topic_analytics(&topic), 2);
}

#[test]
fn test_retention() {
    let (env, client, _) = setup();
    let source = Address::generate(&env);
    let topic = Symbol::new(&env, "ping");

    env.mock_all_auths();

    // Log 4 events. Max retained is 3.
    let id1 = client.log_event(&source, &topic, &1u64.into_val(&env));
    let id2 = client.log_event(&source, &topic, &2u64.into_val(&env));
    let id3 = client.log_event(&source, &topic, &3u64.into_val(&env));
    let id4 = client.log_event(&source, &topic, &4u64.into_val(&env));

    // id1 should be pruned when id4 is logged
    assert!(client.get_event(&id1).is_none());
    assert!(client.get_event(&id2).is_some());
    assert!(client.get_event(&id3).is_some());
    assert!(client.get_event(&id4).is_some());
}
