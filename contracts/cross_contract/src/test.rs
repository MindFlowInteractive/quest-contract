#![cfg(test)]
extern crate std;

use super::*;
use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::{Address as _, Ledger, LedgerInfo},
    Address, Bytes, Env, Symbol,
};

#[contracttype]
#[derive(Clone)]
enum TargetDataKey {
    CallCount,
    AltCallCount,
    LastSender,
    LastRoute,
    LastPayload,
    MutationCount,
}

#[contract]
struct MockTargetContract;

#[contractimpl]
impl MockTargetContract {
    pub fn receive(env: Env, _message_id: u64, sender: Address, route: Symbol, payload: Bytes) -> Bytes {
        let calls: u32 = env.storage().instance().get(&TargetDataKey::CallCount).unwrap_or(0);
        env.storage().instance().set(&TargetDataKey::CallCount, &(calls + 1));
        env.storage().instance().set(&TargetDataKey::LastSender, &sender);
        env.storage().instance().set(&TargetDataKey::LastRoute, &route);
        env.storage().instance().set(&TargetDataKey::LastPayload, &payload);
        payload
    }

    pub fn alternate(
        env: Env,
        _message_id: u64,
        _sender: Address,
        _route: Symbol,
        payload: Bytes,
    ) -> Bytes {
        let calls: u32 = env.storage().instance().get(&TargetDataKey::AltCallCount).unwrap_or(0);
        env.storage()
            .instance()
            .set(&TargetDataKey::AltCallCount, &(calls + 1));
        payload
    }

    pub fn fail(_env: Env, _message_id: u64, _sender: Address, _route: Symbol, _payload: Bytes) -> Bytes {
        panic!("target failed")
    }

    pub fn mutate_then_fail(
        env: Env,
        _message_id: u64,
        _sender: Address,
        _route: Symbol,
        _payload: Bytes,
    ) -> Bytes {
        let count: u32 = env.storage().instance().get(&TargetDataKey::MutationCount).unwrap_or(0);
        env.storage()
            .instance()
            .set(&TargetDataKey::MutationCount, &(count + 1));
        panic!("target failed")
    }

    pub fn get_call_count(env: Env) -> u32 {
        env.storage().instance().get(&TargetDataKey::CallCount).unwrap_or(0)
    }

    pub fn get_alt_call_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&TargetDataKey::AltCallCount)
            .unwrap_or(0)
    }

    pub fn get_mutation_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&TargetDataKey::MutationCount)
            .unwrap_or(0)
    }

    pub fn get_last_sender(env: Env) -> Option<Address> {
        env.storage().instance().get(&TargetDataKey::LastSender)
    }

    pub fn get_last_route(env: Env) -> Option<Symbol> {
        env.storage().instance().get(&TargetDataKey::LastRoute)
    }

    pub fn get_last_payload(env: Env) -> Option<Bytes> {
        env.storage().instance().get(&TargetDataKey::LastPayload)
    }
}

#[contracttype]
#[derive(Clone)]
enum CallbackDataKey {
    CallCount,
    LastMessageId,
    LastRoute,
    LastResponse,
    LastSender,
}

#[contract]
struct MockCallbackContract;

#[contractimpl]
impl MockCallbackContract {
    pub fn accept(
        env: Env,
        message_id: u64,
        route: Symbol,
        response: Bytes,
        sender: Address,
    ) -> bool {
        let calls: u32 = env.storage().instance().get(&CallbackDataKey::CallCount).unwrap_or(0);
        env.storage()
            .instance()
            .set(&CallbackDataKey::CallCount, &(calls + 1));
        env.storage()
            .instance()
            .set(&CallbackDataKey::LastMessageId, &message_id);
        env.storage().instance().set(&CallbackDataKey::LastRoute, &route);
        env.storage()
            .instance()
            .set(&CallbackDataKey::LastResponse, &response);
        env.storage().instance().set(&CallbackDataKey::LastSender, &sender);
        true
    }

    pub fn reject(
        _env: Env,
        _message_id: u64,
        _route: Symbol,
        _response: Bytes,
        _sender: Address,
    ) -> bool {
        false
    }

    pub fn get_call_count(env: Env) -> u32 {
        env.storage().instance().get(&CallbackDataKey::CallCount).unwrap_or(0)
    }

    pub fn get_last_response(env: Env) -> Option<Bytes> {
        env.storage().instance().get(&CallbackDataKey::LastResponse)
    }

    pub fn get_last_sender(env: Env) -> Option<Address> {
        env.storage().instance().get(&CallbackDataKey::LastSender)
    }
}

fn bytes(env: &Env, input: &[u8]) -> Bytes {
    Bytes::from_slice(env, input)
}

fn setup() -> (
    Env,
    CrossContractCommunicationClient<'static>,
    Address,
    Address,
    MockTargetContractClient<'static>,
    Address,
    MockCallbackContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set(LedgerInfo {
        protocol_version: 20,
        sequence_number: 1,
        timestamp: 0,
        network_id: Default::default(),
        base_reserve: 10,
        min_persistent_entry_ttl: 100,
        min_temp_entry_ttl: 100,
        max_entry_ttl: 100000,
    });

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);

    let contract_id = env.register_contract(None, CrossContractCommunication);
    let client = CrossContractCommunicationClient::new(&env, &contract_id);
    client.initialize(&admin, &60u64, &2u32).unwrap();

    let target_id = env.register_contract(None, MockTargetContract);
    let target_client = MockTargetContractClient::new(&env, &target_id);

    let callback_id = env.register_contract(None, MockCallbackContract);
    let callback_client = MockCallbackContractClient::new(&env, &callback_id);

    (env, client, admin, sender, target_client, callback_id, callback_client)
}

#[test]
fn registers_routes_and_queues_messages() {
    let (env, client, admin, sender, target_client, callback_id, _) = setup();

    client.register_route(
        &admin,
        &Symbol::new(&env, "quests"),
        &target_client.address,
        &Symbol::new(&env, "receive"),
        &Some(callback_id.clone()),
        &Some(Symbol::new(&env, "accept")),
    )
    .unwrap();

    let payload = bytes(&env, &[1, 2, 3]);
    let message_id = client
        .queue_message(
        &sender,
        &Symbol::new(&env, "quests"),
        &payload,
        &true,
        &None,
        &None,
        )
        .unwrap();

    assert_eq!(message_id, 1);
    assert_eq!(client.get_queue_size().unwrap(), 1);

    let message = client.get_message(&message_id).unwrap();
    assert_eq!(message.route, Symbol::new(&env, "quests"));
    assert_eq!(message.sender, sender);
    assert_eq!(message.payload, payload);
    assert_eq!(message.callback_contract, Some(callback_id));

    let audit = client.get_audit_trail(&message_id);
    assert_eq!(audit.len(), 1);
    assert_eq!(audit.get(0).unwrap().action, AuditAction::Enqueued);
}

#[test]
fn routes_messages_to_expected_target_and_callback() {
    let (env, client, admin, sender, target_client, callback_id, callback_client) = setup();

    client.register_route(
        &admin,
        &Symbol::new(&env, "quests"),
        &target_client.address,
        &Symbol::new(&env, "receive"),
        &Some(callback_id),
        &Some(Symbol::new(&env, "accept")),
    )
    .unwrap();

    let payload = bytes(&env, &[9, 9, 9]);
    let message_id = client
        .queue_message(
        &sender,
        &Symbol::new(&env, "quests"),
        &payload.clone(),
        &true,
        &None,
        &None,
        )
        .unwrap();

    let outcome = client.process_next().unwrap();
    assert_eq!(outcome.message_id, message_id);
    assert_eq!(outcome.status, MessageStatus::Delivered);
    assert_eq!(outcome.response, Some(payload.clone()));

    assert_eq!(target_client.get_call_count(), 1);
    assert_eq!(target_client.get_last_sender().unwrap(), sender);
    assert_eq!(target_client.get_last_route().unwrap(), Symbol::new(&env, "quests"));
    assert_eq!(target_client.get_last_payload().unwrap(), payload);

    assert_eq!(callback_client.get_call_count(), 1);
    assert_eq!(callback_client.get_last_response().unwrap(), bytes(&env, &[9, 9, 9]));
    assert_eq!(callback_client.get_last_sender().unwrap(), sender);

    let message = client.get_message(&message_id).unwrap();
    assert_eq!(message.status, MessageStatus::Delivered);
    assert_eq!(message.response, Some(bytes(&env, &[9, 9, 9])));
    assert_eq!(client.get_queue_size().unwrap(), 0);

    let audit = client.get_audit_trail(&message_id);
    assert_eq!(audit.len(), 4);
    assert_eq!(audit.get(1).unwrap().action, AuditAction::Routed);
    assert_eq!(audit.get(2).unwrap().action, AuditAction::CallbackSucceeded);
    assert_eq!(audit.get(3).unwrap().action, AuditAction::Delivered);
}

#[test]
fn route_keys_dispatch_to_different_methods() {
    let (env, client, admin, sender, target_client, _, _) = setup();

    client.register_route(
        &admin,
        &Symbol::new(&env, "primary"),
        &target_client.address,
        &Symbol::new(&env, "receive"),
        &None,
        &None,
    )
    .unwrap();
    client.register_route(
        &admin,
        &Symbol::new(&env, "secondary"),
        &target_client.address,
        &Symbol::new(&env, "alternate"),
        &None,
        &None,
    )
    .unwrap();

    client.queue_message(
        &sender,
        &Symbol::new(&env, "secondary"),
        &bytes(&env, &[7]),
        &true,
        &None,
        &None,
    )
    .unwrap();

    let outcome = client.process_next().unwrap();
    assert_eq!(outcome.status, MessageStatus::Delivered);
    assert_eq!(target_client.get_call_count(), 0);
    assert_eq!(target_client.get_alt_call_count(), 1);
}

#[test]
fn rate_limiting_blocks_excess_messages_and_resets_next_window() {
    let (env, client, admin, sender, target_client, _, _) = setup();

    client.register_route(
        &admin,
        &Symbol::new(&env, "quests"),
        &target_client.address,
        &Symbol::new(&env, "receive"),
        &None,
        &None,
    )
    .unwrap();

    assert!(client
        .queue_message(
            &sender,
            &Symbol::new(&env, "quests"),
            &bytes(&env, &[1]),
            &true,
            &None,
            &None,
        )
        .is_ok());
    assert!(client
        .queue_message(
            &sender,
            &Symbol::new(&env, "quests"),
            &bytes(&env, &[2]),
            &true,
            &None,
            &None,
        )
        .is_ok());
    assert_eq!(
        client.queue_message(
            &sender,
            &Symbol::new(&env, "quests"),
            &bytes(&env, &[3]),
            &true,
            &None,
            &None,
        ),
        Err(CrossContractError::RateLimited)
    );

    env.ledger().with_mut(|ledger| ledger.timestamp = 61);

    assert!(client
        .queue_message(
            &sender,
            &Symbol::new(&env, "quests"),
            &bytes(&env, &[4]),
            &true,
            &None,
            &None,
        )
        .is_ok());

    let window = client.get_sender_window(&sender).unwrap();
    assert_eq!(window.window, 1);
    assert_eq!(window.count, 1);
}

#[test]
fn disabled_routes_are_rejected() {
    let (env, client, admin, sender, target_client, _, _) = setup();

    client.register_route(
        &admin,
        &Symbol::new(&env, "quests"),
        &target_client.address,
        &Symbol::new(&env, "receive"),
        &None,
        &None,
    )
    .unwrap();
    client
        .set_route_enabled(&admin, &Symbol::new(&env, "quests"), &false)
        .unwrap();

    let result = client.queue_message(
        &sender,
        &Symbol::new(&env, "quests"),
        &bytes(&env, &[1]),
        &true,
        &None,
        &None,
    );
    assert_eq!(result, Err(CrossContractError::RouteDisabled));
}

#[test]
fn non_atomic_target_failures_are_recorded_and_removed_from_queue() {
    let (env, client, admin, sender, target_client, _, _) = setup();

    client.register_route(
        &admin,
        &Symbol::new(&env, "quests"),
        &target_client.address,
        &Symbol::new(&env, "fail"),
        &None,
        &None,
    )
    .unwrap();

    let message_id = client
        .queue_message(
        &sender,
        &Symbol::new(&env, "quests"),
        &bytes(&env, &[8]),
        &false,
        &None,
        &None,
        )
        .unwrap();

    let result = client.process_next();
    assert_eq!(result, Err(CrossContractError::TargetInvocationFailed));

    let message = client.get_message(&message_id).unwrap();
    assert_eq!(message.status, MessageStatus::Failed);
    assert_eq!(
        message.last_error,
        Some(CrossContractError::TargetInvocationFailed as u32)
    );
    assert_eq!(client.get_queue_size().unwrap(), 0);

    let audit = client.get_audit_trail(&message_id);
    assert_eq!(audit.len(), 2);
    assert_eq!(audit.get(1).unwrap().action, AuditAction::Failed);
}

#[test]
fn non_atomic_callback_failures_propagate_and_preserve_target_response() {
    let (env, client, admin, sender, target_client, callback_id, callback_client) = setup();

    client.register_route(
        &admin,
        &Symbol::new(&env, "quests"),
        &target_client.address,
        &Symbol::new(&env, "receive"),
        &Some(callback_id),
        &Some(Symbol::new(&env, "reject")),
    )
    .unwrap();

    let message_id = client
        .queue_message(
        &sender,
        &Symbol::new(&env, "quests"),
        &bytes(&env, &[4, 5]),
        &false,
        &None,
        &None,
        )
        .unwrap();

    let result = client.process_next();
    assert_eq!(result, Err(CrossContractError::CallbackInvocationFailed));

    assert_eq!(target_client.get_call_count(), 1);
    assert_eq!(callback_client.get_call_count(), 0);

    let message = client.get_message(&message_id).unwrap();
    assert_eq!(message.status, MessageStatus::CallbackFailed);
    assert_eq!(message.response, Some(bytes(&env, &[4, 5])));
    assert_eq!(
        message.last_error,
        Some(CrossContractError::CallbackInvocationFailed as u32)
    );
}

#[test]
fn atomic_failures_keep_message_queued_and_rollback_target_side_effects() {
    let (env, client, admin, sender, target_client, _, _) = setup();

    client.register_route(
        &admin,
        &Symbol::new(&env, "quests"),
        &target_client.address,
        &Symbol::new(&env, "mutate_then_fail"),
        &None,
        &None,
    )
    .unwrap();

    let message_id = client
        .queue_message(
        &sender,
        &Symbol::new(&env, "quests"),
        &bytes(&env, &[1, 2]),
        &true,
        &None,
        &None,
        )
        .unwrap();

    let result = client.process_next();
    assert_eq!(result, Err(CrossContractError::TargetInvocationFailed));

    assert_eq!(target_client.get_mutation_count(), 0);
    assert_eq!(client.get_queue_size().unwrap(), 1);

    let message = client.get_message(&message_id).unwrap();
    assert_eq!(message.status, MessageStatus::Queued);
    assert!(message.response.is_none());

    let audit = client.get_audit_trail(&message_id);
    assert_eq!(audit.len(), 1);
    assert_eq!(audit.get(0).unwrap().action, AuditAction::Enqueued);
}

#[test]
fn callback_override_is_used_instead_of_default_route_callback() {
    let (env, client, admin, sender, target_client, callback_id, callback_client) = setup();
    let alt_callback_id = env.register_contract(None, MockCallbackContract);
    let alt_callback_client = MockCallbackContractClient::new(&env, &alt_callback_id);

    client.register_route(
        &admin,
        &Symbol::new(&env, "quests"),
        &target_client.address,
        &Symbol::new(&env, "receive"),
        &Some(callback_id),
        &Some(Symbol::new(&env, "accept")),
    )
    .unwrap();

    client.queue_message(
        &sender,
        &Symbol::new(&env, "quests"),
        &bytes(&env, &[6]),
        &true,
        &Some(alt_callback_id),
        &Some(Symbol::new(&env, "accept")),
    )
    .unwrap();

    let outcome = client.process_next().unwrap();
    assert_eq!(outcome.status, MessageStatus::Delivered);
    assert_eq!(callback_client.get_call_count(), 0);
    assert_eq!(alt_callback_client.get_call_count(), 1);
}
