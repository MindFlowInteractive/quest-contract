#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

const REASON_PUZZLE: u32 = 1;
const REASON_ACHIEVEMENT: u32 = 2;

fn setup() -> (Env, PointsLedgerContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, PointsLedgerContract);
    let client = PointsLedgerContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let awarder = Address::generate(&env);
    client.add_awarder(&awarder);
    (env, client, admin, awarder)
}

fn history_sum(client: &PointsLedgerContractClient, player: &Address) -> u64 {
    client.history(player).iter().map(|e| e.amount).sum()
}

#[test]
fn new_player_has_empty_balance_and_history() {
    let (env, client, _, _) = setup();
    let player = Address::generate(&env);
    assert_eq!(client.balance(&player), 0);
    assert_eq!(client.history(&player).len(), 0);
}

#[test]
fn authorized_awarder_can_earn() {
    let (env, client, _, awarder) = setup();
    let player = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let new_balance = client.earn(&awarder, &player, &50, &REASON_PUZZLE, &0);

    assert_eq!(new_balance, 50);
    assert_eq!(client.balance(&player), 50);
    let history = client.history(&player);
    assert_eq!(history.len(), 1);
    assert_eq!(
        history.get(0).unwrap(),
        PointsEntry {
            amount: 50,
            reason: REASON_PUZZLE,
            awarder: awarder.clone(),
            timestamp: 1_000,
            expires_at: 0,
        }
    );
}

#[test]
fn balance_equals_sum_of_history() {
    let (env, client, _, awarder) = setup();
    let player = Address::generate(&env);

    client.earn(&awarder, &player, &10, &REASON_PUZZLE, &0);
    client.earn(&awarder, &player, &25, &REASON_ACHIEVEMENT, &0);
    client.earn(&awarder, &player, &7, &REASON_PUZZLE, &0);

    let history = client.history(&player);
    assert_eq!(history.len(), 3);
    assert_eq!(history.get(1).unwrap().reason, REASON_ACHIEVEMENT);
    assert_eq!(client.balance(&player), 42);
    assert_eq!(client.balance(&player), history_sum(&client, &player));
}

#[test]
fn players_are_tracked_independently() {
    let (env, client, _, awarder) = setup();
    let p1 = Address::generate(&env);
    let p2 = Address::generate(&env);

    client.earn(&awarder, &p1, &10, &REASON_PUZZLE, &0);
    client.earn(&awarder, &p2, &99, &REASON_ACHIEVEMENT, &0);

    assert_eq!(client.balance(&p1), 10);
    assert_eq!(client.balance(&p2), 99);
    assert_eq!(client.history(&p1).len(), 1);
    assert_eq!(client.history(&p2).len(), 1);
}

#[test]
#[should_panic(expected = "unauthorized awarder")]
fn unauthorized_caller_cannot_earn() {
    let (env, client, _, _) = setup();
    let stranger = Address::generate(&env);
    let player = Address::generate(&env);
    client.earn(&stranger, &player, &10, &REASON_PUZZLE, &0);
}

#[test]
#[should_panic(expected = "unauthorized awarder")]
fn removed_awarder_cannot_earn() {
    let (env, client, _, awarder) = setup();
    let player = Address::generate(&env);
    client.remove_awarder(&awarder);
    assert!(!client.is_awarder(&awarder));
    client.earn(&awarder, &player, &10, &REASON_PUZZLE, &0);
}

#[test]
#[should_panic(expected = "amount must be greater than zero")]
fn zero_amount_rejected() {
    let (env, client, _, awarder) = setup();
    let player = Address::generate(&env);
    client.earn(&awarder, &player, &0, &REASON_PUZZLE, &0);
}

#[test]
#[should_panic(expected = "already initialized")]
fn cannot_initialize_twice() {
    let (_, client, admin, _) = setup();
    client.initialize(&admin);
}

#[test]
#[should_panic]
fn only_admin_can_add_awarder() {
    let env = Env::default();
    let id = env.register_contract(None, PointsLedgerContract);
    let client = PointsLedgerContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    env.set_auths(&[]);
    client.add_awarder(&Address::generate(&env));
}

// ---------------------------------------------------------------------------
// Spend and expiry
// ---------------------------------------------------------------------------

#[test]
fn spend_reduces_balance_and_returns_new_balance() {
    let (env, client, _, awarder) = setup();
    let player = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    client.earn(&awarder, &player, &50, &REASON_PUZZLE, &0);
    assert_eq!(client.spend(&player, &20), 30);
    assert_eq!(client.balance(&player), 30);
}

#[test]
#[should_panic(expected = "amount must be greater than zero")]
fn spend_requires_positive_amount() {
    let (env, client, _, awarder) = setup();
    let player = Address::generate(&env);
    client.earn(&awarder, &player, &50, &REASON_PUZZLE, &0);
    client.spend(&player, &0);
}

#[test]
#[should_panic(expected = "insufficient unexpired balance")]
fn spend_cannot_exceed_unexpired_balance() {
    let (env, client, _, awarder) = setup();
    let player = Address::generate(&env);
    client.earn(&awarder, &player, &10, &REASON_PUZZLE, &0);
    client.spend(&player, &11);
    assert_eq!(client.balance(&player), 10);
}

#[test]
fn spend_consumes_oldest_entries_first() {
    let (env, client, _, awarder) = setup();
    let player = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.earn(&awarder, &player, &10, &REASON_PUZZLE, &0);
    env.ledger().with_mut(|l| l.timestamp = 2_000);
    client.earn(&awarder, &player, &20, &REASON_PUZZLE, &0);

    // FIFO: the first 10-point entry is consumed, the 20-point entry is
    // reduced to cover the remainder.
    assert_eq!(client.spend(&player, &25), 5);
    let history = client.history(&player);
    assert_eq!(history.len(), 1);
    assert_eq!(history.get(0).unwrap().amount, 5);
    assert_eq!(client.balance(&player), 5);
}

#[test]
fn expired_points_are_excluded_from_balance() {
    let (env, client, _, awarder) = setup();
    let player = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.earn(&awarder, &player, &50, &REASON_PUZZLE, &2_000);
    assert_eq!(client.balance(&player), 50);

    env.ledger().with_mut(|l| l.timestamp = 2_000);
    assert_eq!(client.balance(&player), 0);
}

#[test]
fn spend_skips_expired_entries() {
    let (env, client, _, awarder) = setup();
    let player = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.earn(&awarder, &player, &30, &REASON_PUZZLE, &0);
    env.ledger().with_mut(|l| l.timestamp = 2_000);
    client.earn(&awarder, &player, &20, &REASON_ACHIEVEMENT, &3_000);

    // The second entry is now expired; the unexpired 30 still spendable.
    env.ledger().with_mut(|l| l.timestamp = 3_000);
    assert_eq!(client.balance(&player), 30);
    assert_eq!(client.spend(&player, &30), 0);
}

#[test]
#[should_panic(expected = "insufficient unexpired balance")]
fn expired_points_cannot_be_spent() {
    let (env, client, _, awarder) = setup();
    let player = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.earn(&awarder, &player, &50, &REASON_PUZZLE, &2_000);
    env.ledger().with_mut(|l| l.timestamp = 2_000);

    client.spend(&player, &50);
    assert_eq!(client.balance(&player), 0);
}

#[test]
#[should_panic]
fn only_player_can_spend() {
    let (env, client, _, awarder) = setup();
    let player = Address::generate(&env);
    client.earn(&awarder, &player, &10, &REASON_PUZZLE, &0);
    env.set_auths(&[]);
    client.spend(&player, &10);
}

#[test]
#[should_panic(expected = "expires_at must be in the future")]
fn earn_rejects_expiry_in_the_past() {
    let (env, client, _, awarder) = setup();
    let player = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.earn(&awarder, &player, &10, &REASON_PUZZLE, &999);
}
