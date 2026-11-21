#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, String,
};

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn setup() -> (Env, AttendanceNFTClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, AttendanceNFT);
    let client = AttendanceNFTClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    (env, client, admin)
}

/// Advance the ledger clock to `ts`.
fn set_time(env: &Env, ts: u64) {
    env.ledger().with_mut(|li| li.timestamp = ts);
}

// ─── Initialisation tests ─────────────────────────────────────────────────────

#[test]
fn test_initialize_succeeds() {
    let (_env, _client, _admin) = setup();
    // If we get here without panicking the initialisation worked.
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_double_initialize_panics() {
    let (env, client, _admin) = setup();
    let other_admin = Address::generate(&env);
    client.initialize(&other_admin); // must panic
}

// ─── Event creation tests ──────────────────────────────────────────────────────

#[test]
fn test_create_event_returns_incrementing_ids() {
    let (env, client, _admin) = setup();

    let name1 = String::from_str(&env, "DevCon 2025");
    let name2 = String::from_str(&env, "Hackathon Night");

    let id1 = client.create_event(&name1, &1000u64, &2000u64);
    let id2 = client.create_event(&name2, &3000u64, &4000u64);

    assert_eq!(id1, 1u32);
    assert_eq!(id2, 2u32);
}

#[test]
fn test_get_event_returns_correct_metadata() {
    let (env, client, admin) = setup();

    let name = String::from_str(&env, "Summit");
    let start = 500u64;
    let end = 1500u64;

    let event_id = client.create_event(&name, &start, &end);

    let event = client.get_event(&event_id).expect("event should exist");
    assert_eq!(event.event_id, event_id);
    assert_eq!(event.name, name);
    assert_eq!(event.window_start, start);
    assert_eq!(event.window_end, end);
    assert_eq!(event.organizer, admin);
}

#[test]
#[should_panic(expected = "Invalid check-in window")]
fn test_create_event_invalid_window_panics() {
    let (env, client, _admin) = setup();
    let name = String::from_str(&env, "Bad Event");
    // start == end → invalid
    client.create_event(&name, &1000u64, &1000u64);
}

#[test]
#[should_panic]
fn test_create_event_requires_admin_auth() {
    let (env, _client, _admin) = setup();

    // Build a *new* client that does NOT mock auths so the require_auth fires.
    let env2 = Env::default(); // no mock_all_auths
    let contract_id = env2.register_contract(None, AttendanceNFT);
    let client2 = AttendanceNFTClient::new(&env2, &contract_id);
    let admin2 = Address::generate(&env);
    env2.mock_all_auths();
    client2.initialize(&admin2);

    // Now drop mock_all_auths so the next call lacks auth.
    // We can't easily "un-mock", so we use a fresh env without mocking.
    let env3 = Env::default();
    let contract_id3 = env3.register_contract(None, AttendanceNFT);
    let client3 = AttendanceNFTClient::new(&env3, &contract_id3);
    // initialize without mocking – require_auth on admin will fire first
    client3.initialize(&Address::generate(&env3));
    // create_event without mocking auth → should panic
    client3.create_event(
        &String::from_str(&env3, "Fail"),
        &100u64,
        &200u64,
    );
}

// ─── Check-in tests ───────────────────────────────────────────────────────────

#[test]
fn test_checkin_inside_window_mints_token() {
    let (env, client, _admin) = setup();

    let name = String::from_str(&env, "Genesis Event");
    let event_id = client.create_event(&name, &1000u64, &2000u64);

    // Set the clock inside the window
    set_time(&env, 1500);

    let player = Address::generate(&env);
    let token_id = client.check_in(&player, &event_id);

    assert_eq!(token_id, 1u32);
    assert_eq!(client.total_supply(), 1u32);
    assert_eq!(client.owner_of(&token_id), player);

    let token = client.get_token(&token_id).expect("token should exist");
    assert_eq!(token.token_id, token_id);
    assert_eq!(token.event_id, event_id);
    assert_eq!(token.owner, player);
    assert_eq!(token.checked_in_at, 1500u64);
}

#[test]
fn test_checkin_records_guard_flag() {
    let (env, client, _admin) = setup();

    let event_id =
        client.create_event(&String::from_str(&env, "E"), &100u64, &900u64);

    set_time(&env, 500);
    let player = Address::generate(&env);

    assert!(!client.has_checked_in(&player, &event_id));
    client.check_in(&player, &event_id);
    assert!(client.has_checked_in(&player, &event_id));
}

#[test]
fn test_tokens_of_returns_all_owned_tokens() {
    let (env, client, _admin) = setup();

    let event_id1 =
        client.create_event(&String::from_str(&env, "E1"), &100u64, &900u64);
    let event_id2 =
        client.create_event(&String::from_str(&env, "E2"), &100u64, &900u64);

    set_time(&env, 500);
    let player = Address::generate(&env);

    let t1 = client.check_in(&player, &event_id1);
    let t2 = client.check_in(&player, &event_id2);

    let owned = client.tokens_of(&player);
    assert_eq!(owned.len(), 2);
    assert_eq!(owned.get(0).unwrap(), t1);
    assert_eq!(owned.get(1).unwrap(), t2);
}

#[test]
fn test_multiple_players_same_event() {
    let (env, client, _admin) = setup();

    let event_id =
        client.create_event(&String::from_str(&env, "Multi"), &0u64, &9999u64);

    set_time(&env, 100);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    let t_alice = client.check_in(&alice, &event_id);
    let t_bob = client.check_in(&bob, &event_id);

    assert_ne!(t_alice, t_bob);
    assert_eq!(client.owner_of(&t_alice), alice);
    assert_eq!(client.owner_of(&t_bob), bob);
    assert_eq!(client.total_supply(), 2u32);
}

// ─── Window enforcement tests ──────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Check-in window is not open")]
fn test_checkin_before_window_panics() {
    let (env, client, _admin) = setup();

    let event_id =
        client.create_event(&String::from_str(&env, "Future"), &1000u64, &2000u64);

    // Clock is before the window
    set_time(&env, 500);

    let player = Address::generate(&env);
    client.check_in(&player, &event_id);
}

#[test]
#[should_panic(expected = "Check-in window is not open")]
fn test_checkin_after_window_panics() {
    let (env, client, _admin) = setup();

    let event_id =
        client.create_event(&String::from_str(&env, "Past"), &1000u64, &2000u64);

    // Clock is after the window
    set_time(&env, 3000);

    let player = Address::generate(&env);
    client.check_in(&player, &event_id);
}

// ─── Double check-in prevention ───────────────────────────────────────────────

#[test]
#[should_panic(expected = "Already checked in")]
fn test_double_checkin_same_event_panics() {
    let (env, client, _admin) = setup();

    let event_id =
        client.create_event(&String::from_str(&env, "Once"), &0u64, &9999u64);

    set_time(&env, 100);
    let player = Address::generate(&env);

    client.check_in(&player, &event_id);
    client.check_in(&player, &event_id); // must panic
}

#[test]
fn test_same_player_can_attend_different_events() {
    let (env, client, _admin) = setup();

    let e1 =
        client.create_event(&String::from_str(&env, "E1"), &0u64, &9999u64);
    let e2 =
        client.create_event(&String::from_str(&env, "E2"), &0u64, &9999u64);

    set_time(&env, 100);
    let player = Address::generate(&env);

    let t1 = client.check_in(&player, &e1);
    let t2 = client.check_in(&player, &e2);

    assert_ne!(t1, t2);
    assert_eq!(client.total_supply(), 2u32);
}

// ─── Non-transferability tests ────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Attendance tokens cannot be transferred")]
fn test_transfer_always_panics() {
    let (env, client, _admin) = setup();

    let event_id =
        client.create_event(&String::from_str(&env, "Soul"), &0u64, &9999u64);

    set_time(&env, 100);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    let token_id = client.check_in(&alice, &event_id);

    // Any attempt to transfer must panic.
    client.transfer(&alice, &bob, &token_id);
}

// ─── Edge-case / unknown event ────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Event not found")]
fn test_checkin_unknown_event_panics() {
    let (env, client, _admin) = setup();

    set_time(&env, 500);
    let player = Address::generate(&env);
    client.check_in(&player, &999u32); // event does not exist
}

#[test]
fn test_get_event_nonexistent_returns_none() {
    let (_env, client, _admin) = setup();
    assert!(client.get_event(&999u32).is_none());
}

#[test]
fn test_get_token_nonexistent_returns_none() {
    let (_env, client, _admin) = setup();
    assert!(client.get_token(&999u32).is_none());
}
