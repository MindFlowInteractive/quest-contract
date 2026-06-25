#![cfg(test)]

use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

use crate::{
    EventType, VestingScheduleContract, VestingScheduleContractClient,
};

fn create_token_contract<'a>(env: &Env, admin: &Address) -> (Address, TokenClient<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let address = sac.address();
    (address.clone(), TokenClient::new(env, &address))
}

fn setup_vesting_contract(env: &Env) -> (
    VestingScheduleContractClient,
    Address,
    Address,
    Address,
    TokenClient,
    StellarAssetClient,
) {
    let admin = Address::generate(env);
    let beneficiary = Address::generate(env);
    let token_admin = Address::generate(env);

    let (token_addr, token_client) = create_token_contract(env, &token_admin);
    let token_admin_client = StellarAssetClient::new(env, &token_addr);

    let contract_id = env.register_contract(None, VestingScheduleContract);
    let client = VestingScheduleContractClient::new(env, &contract_id);

    client.initialize(&admin, &token_addr);

    (client, admin, beneficiary, token_admin, token_client, token_admin_client)
}

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, _, _token_admin, token_client, _) = setup_vesting_contract(&env);

    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_token(), token_client.address);
    assert!(!client.is_paused());
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_initialize_twice() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, _, _token_admin, token_client, _) = setup_vesting_contract(&env);

    client.initialize(&admin, &token_client.address);
}

#[test]
fn test_create_schedule() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, beneficiary, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    // Mint tokens to admin
    token_admin_client.mint(&admin, &1000000i128);

    let total_amount = 1000i128;
    let start_time = 1000u64;
    let cliff_duration = 100u64;
    let vesting_duration = 1000u64;

    let schedule_id = client.create_schedule(
        &beneficiary,
        &total_amount,
        &start_time,
        &cliff_duration,
        &vesting_duration,
        &true,
    );

    assert_eq!(schedule_id, 0);

    let schedule = client.get_schedule(&schedule_id);
    assert_eq!(schedule.beneficiary, beneficiary);
    assert_eq!(schedule.total_amount, total_amount);
    assert_eq!(schedule.start_time, start_time);
    assert_eq!(schedule.cliff_duration, cliff_duration);
    assert_eq!(schedule.vesting_duration, vesting_duration);
    assert_eq!(schedule.revocable, true);
    assert!(!schedule.revoked);
}

#[test]
#[should_panic(expected = "Total amount must be positive")]
fn test_create_schedule_zero_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, beneficiary, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    token_admin_client.mint(&admin, &1000000i128);

    client.create_schedule(&beneficiary, &0i128, &1000u64, &100u64, &1000u64, &true);
}

#[test]
#[should_panic(expected = "Cliff duration must be less than vesting duration")]
fn test_create_schedule_invalid_cliff() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, beneficiary, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    token_admin_client.mint(&admin, &1000000i128);

    client.create_schedule(&beneficiary, &1000i128, &1000u64, &1000u64, &1000u64, &true);
}

#[test]
fn test_multiple_schedules() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, _, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    token_admin_client.mint(&admin, &1000000i128);

    let beneficiary1 = Address::generate(&env);
    let beneficiary2 = Address::generate(&env);

    let schedule_id1 = client.create_schedule(
        &beneficiary1,
        &1000i128,
        &1000u64,
        &100u64,
        &1000u64,
        &true,
    );

    let schedule_id2 = client.create_schedule(
        &beneficiary2,
        &2000i128,
        &1000u64,
        &100u64,
        &1000u64,
        &true,
    );

    assert_eq!(schedule_id1, 0);
    assert_eq!(schedule_id2, 1);

    let schedules1 = client.get_beneficiary_schedules(&beneficiary1);
    assert_eq!(schedules1.len(), 1);
    assert_eq!(schedules1.get(0), Some(schedule_id1));

    let schedules2 = client.get_beneficiary_schedules(&beneficiary2);
    assert_eq!(schedules2.len(), 1);
    assert_eq!(schedules2.get(0), Some(schedule_id2));
}

#[test]
fn test_cliff_period_enforcement() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, beneficiary, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    token_admin_client.mint(&admin, &1000000i128);

    let start_time = 1000u64;
    let cliff_duration = 100u64;
    let vesting_duration = 1000u64;

    let schedule_id = client.create_schedule(
        &beneficiary,
        &1000i128,
        &start_time,
        &cliff_duration,
        &vesting_duration,
        &true,
    );

    // Set time before cliff ends
    env.ledger().set_timestamp(start_time + 50);

    let vested = client.get_vested_amount(&schedule_id);
    assert_eq!(vested, 0);

    let releasable = client.get_releasable_amount(&schedule_id);
    assert_eq!(releasable, 0);

    // Set time after cliff ends
    env.ledger().set_timestamp(start_time + 150);

    let vested = client.get_vested_amount(&schedule_id);
    assert!(vested > 0);
}

#[test]
fn test_linear_vesting_calculation() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, beneficiary, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    token_admin_client.mint(&admin, &1000000i128);

    let total_amount = 1000i128;
    let start_time = 1000u64;
    let cliff_duration = 100u64;
    let vesting_duration = 1000u64;

    let schedule_id = client.create_schedule(
        &beneficiary,
        &total_amount,
        &start_time,
        &cliff_duration,
        &vesting_duration,
        &true,
    );

    // Test at 50% of vesting period (after cliff)
    env.ledger().set_timestamp(start_time + cliff_duration + 450);
    let vested = client.get_vested_amount(&schedule_id);
    // 50% of vesting period (900 seconds) should be 50% of total
    assert!(vested >= 490 && vested <= 510);

    // Test at 100% of vesting period
    env.ledger().set_timestamp(start_time + vesting_duration);
    let vested = client.get_vested_amount(&schedule_id);
    assert_eq!(vested, total_amount);
}

#[test]
fn test_token_release() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, beneficiary, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    token_admin_client.mint(&admin, &1000000i128);

    let total_amount = 1000i128;
    let start_time = 1000u64;
    let cliff_duration = 100u64;
    let vesting_duration = 1000u64;

    let schedule_id = client.create_schedule(
        &beneficiary,
        &total_amount,
        &start_time,
        &cliff_duration,
        &vesting_duration,
        &true,
    );

    // Set time after cliff
    env.ledger().set_timestamp(start_time + cliff_duration + 200);

    let releasable_before = client.get_releasable_amount(&schedule_id);
    let released = client.release(&schedule_id);

    assert_eq!(released, releasable_before);
    assert!(released > 0);

    let schedule = client.get_schedule(&schedule_id);
    assert_eq!(schedule.released_amount, released);

    // Try to release again - should release additional vested tokens
    env.ledger().set_timestamp(start_time + cliff_duration + 400);
    let released_again = client.release(&schedule_id);

    assert!(released_again > 0);
}

#[test]
#[should_panic(expected = "No tokens available for release")]
fn test_release_before_cliff() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, beneficiary, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    token_admin_client.mint(&admin, &1000000i128);

    let start_time = 1000u64;
    let cliff_duration = 100u64;
    let vesting_duration = 1000u64;

    let schedule_id = client.create_schedule(
        &beneficiary,
        &1000i128,
        &start_time,
        &cliff_duration,
        &vesting_duration,
        &true,
    );

    // Set time before cliff ends
    env.ledger().set_timestamp(start_time + 50);

    client.release(&schedule_id);
}

#[test]
fn test_schedule_revocation() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, beneficiary, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    token_admin_client.mint(&admin, &1000000i128);

    let total_amount = 1000i128;
    let start_time = 1000u64;
    let cliff_duration = 100u64;
    let vesting_duration = 1000u64;

    let schedule_id = client.create_schedule(
        &beneficiary,
        &total_amount,
        &start_time,
        &cliff_duration,
        &vesting_duration,
        &true,
    );

    // Set time to 50% of vesting
    env.ledger().set_timestamp(start_time + cliff_duration + 450);

    let vested_before = client.get_vested_amount(&schedule_id);
    let unvested_before = total_amount - vested_before;

    let unvested_returned = client.revoke_schedule(&schedule_id);

    assert_eq!(unvested_returned, unvested_before);

    let schedule = client.get_schedule(&schedule_id);
    assert!(schedule.revoked);

    // Cannot release after revocation
    env.ledger().set_timestamp(start_time + vesting_duration);
    let releasable = client.get_releasable_amount(&schedule_id);
    assert_eq!(releasable, 0);
}

#[test]
#[should_panic(expected = "Schedule is not revocable")]
fn test_revoke_non_revocable() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, beneficiary, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    token_admin_client.mint(&admin, &1000000i128);

    let schedule_id = client.create_schedule(
        &beneficiary,
        &1000i128,
        &1000u64,
        &100u64,
        &1000u64,
        &false, // Not revocable
    );

    client.revoke_schedule(&schedule_id);
}

#[test]
fn test_schedule_modification() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, beneficiary, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    token_admin_client.mint(&admin, &1000000i128);

    let start_time = 1000u64;
    let cliff_duration = 100u64;
    let vesting_duration = 1000u64;

    let schedule_id = client.create_schedule(
        &beneficiary,
        &1000i128,
        &start_time,
        &cliff_duration,
        &vesting_duration,
        &true,
    );

    // Modify cliff duration
    client.modify_schedule(&schedule_id, &200u64, &0u64);

    let schedule = client.get_schedule(&schedule_id);
    assert_eq!(schedule.cliff_duration, 200u64);
    assert!(schedule.modified_at.is_some());

    // Modify vesting duration
    client.modify_schedule(&schedule_id, &0u64, &2000u64);

    let schedule = client.get_schedule(&schedule_id);
    assert_eq!(schedule.vesting_duration, 2000u64);
}

#[test]
#[should_panic(expected = "Modification cannot reduce vested amount")]
fn test_modify_reduce_vested() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, beneficiary, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    token_admin_client.mint(&admin, &1000000i128);

    let start_time = 1000u64;
    let cliff_duration = 100u64;
    let vesting_duration = 1000u64;

    let schedule_id = client.create_schedule(
        &beneficiary,
        &1000i128,
        &start_time,
        &cliff_duration,
        &vesting_duration,
        &true,
    );

    // Advance time well into vesting period (50% vested)
    env.ledger().set_timestamp(start_time + cliff_duration + 450);

    // Check vested amount before modification
    let vested_before = client.get_vested_amount(&schedule_id);
    assert!(vested_before > 0);

    // Try to increase cliff duration beyond current time (would reduce vested amount to 0)
    client.modify_schedule(&schedule_id, &600u64, &0u64);
}

#[test]
fn test_vesting_status() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, beneficiary, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    token_admin_client.mint(&admin, &1000000i128);

    let total_amount = 1000i128;
    let start_time = 1000u64;
    let cliff_duration = 100u64;
    let vesting_duration = 1000u64;

    let schedule_id = client.create_schedule(
        &beneficiary,
        &total_amount,
        &start_time,
        &cliff_duration,
        &vesting_duration,
        &true,
    );

    let status = client.get_vesting_status(&schedule_id);
    assert_eq!(status.schedule_id, schedule_id);
    assert_eq!(status.total_amount, total_amount);
    assert_eq!(status.cliff_end_time, start_time + cliff_duration);
    assert_eq!(status.vesting_end_time, start_time + vesting_duration);
    assert!(!status.is_revoked);
    assert!(!status.is_fully_vested);

    // Advance time to full vesting
    env.ledger().set_timestamp(start_time + vesting_duration);

    let status = client.get_vesting_status(&schedule_id);
    assert!(status.is_fully_vested);
    assert_eq!(status.vested_amount, total_amount);
}

#[test]
fn test_vesting_history() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, beneficiary, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    token_admin_client.mint(&admin, &1000000i128);

    let start_time = 1000u64;
    let cliff_duration = 100u64;
    let vesting_duration = 1000u64;

    let schedule_id = client.create_schedule(
        &beneficiary,
        &1000i128,
        &start_time,
        &cliff_duration,
        &vesting_duration,
        &true,
    );

    let history = client.get_vesting_history(&schedule_id);
    assert_eq!(history.len(), 1);

    let first_event = history.get(0u32).unwrap();
    assert_eq!(first_event.event_type, EventType::ScheduleCreated);
    assert_eq!(first_event.amount, 1000i128);

    // Release tokens
    env.ledger().set_timestamp(start_time + cliff_duration + 200);
    client.release(&schedule_id);

    let history = client.get_vesting_history(&schedule_id);
    assert_eq!(history.len(), 2);

    let second_event = history.get(1u32).unwrap();
    assert_eq!(second_event.event_type, EventType::TokensReleased);
}

#[test]
fn test_pause_unpause() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let token = Address::generate(&env);

    let contract_id = env.register_contract(None, VestingScheduleContract);
    let client = VestingScheduleContractClient::new(&env, &contract_id);

    client.initialize(&admin, &token);

    assert!(!client.is_paused());

    client.pause();
    assert!(client.is_paused());

    client.unpause();
    assert!(!client.is_paused());
}

#[test]
#[should_panic(expected = "Contract is paused")]
fn test_operations_when_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, beneficiary, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    token_admin_client.mint(&admin, &1000000i128);

    client.pause();

    client.create_schedule(&beneficiary, &1000i128, &1000u64, &100u64, &1000u64, &true);
}

#[test]
fn test_fully_vested_release() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, beneficiary, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    token_admin_client.mint(&admin, &1000000i128);

    let total_amount = 1000i128;
    let start_time = 1000u64;
    let cliff_duration = 100u64;
    let vesting_duration = 1000u64;

    let schedule_id = client.create_schedule(
        &beneficiary,
        &total_amount,
        &start_time,
        &cliff_duration,
        &vesting_duration,
        &true,
    );

    // Advance to full vesting
    env.ledger().set_timestamp(start_time + vesting_duration);

    let releasable = client.get_releasable_amount(&schedule_id);
    assert_eq!(releasable, total_amount);

    let released = client.release(&schedule_id);
    assert_eq!(released, total_amount);

    let schedule = client.get_schedule(&schedule_id);
    assert_eq!(schedule.released_amount, total_amount);

    // No more tokens to release
    let _releasable_after = client.get_releasable_amount(&schedule_id);
    assert_eq!(_releasable_after, 0);
}

#[test]
fn test_multiple_schedules_same_beneficiary() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, beneficiary, _token_admin, _token_client, token_admin_client) =
        setup_vesting_contract(&env);

    token_admin_client.mint(&admin, &1000000i128);

    let schedule_id1 = client.create_schedule(
        &beneficiary,
        &1000i128,
        &1000u64,
        &100u64,
        &1000u64,
        &true,
    );

    let schedule_id2 = client.create_schedule(
        &beneficiary,
        &2000i128,
        &1000u64,
        &200u64,
        &2000u64,
        &true,
    );

    let schedules = client.get_beneficiary_schedules(&beneficiary);
    assert_eq!(schedules.len(), 2);

    let schedule1 = client.get_schedule(&schedule_id1);
    let schedule2 = client.get_schedule(&schedule_id2);

    assert_eq!(schedule1.total_amount, 1000i128);
    assert_eq!(schedule2.total_amount, 2000i128);
}
