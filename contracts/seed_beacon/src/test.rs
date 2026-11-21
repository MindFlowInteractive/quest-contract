use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Env,
};

struct TestFixture {
    env: Env,
    admin: Address,
    player: Address,
}

impl TestFixture {
    fn setup() -> (Self, SeedBeaconContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let player = Address::generate(&env);

        let contract_id = env.register_contract(None, SeedBeaconContract);
        let client = SeedBeaconContractClient::new(&env, &contract_id);

        client.initialize(&admin);

        let fixture = Self {
            env,
            admin,
            player,
        };

        (fixture, client)
    }
}

#[test]
fn test_commit_requires_future_ledger_and_admin() {
    let (f, client) = TestFixture::setup();

    let salt = BytesN::from_array(&f.env, &[42u8; 32]);
    let current_seq = f.env.ledger().sequence();

    // Past or present ledger must be rejected
    let res_past = client.try_commit_round(&f.admin, &1, &current_seq, &salt);
    assert_eq!(res_past, Err(Ok(BeaconError::TargetLedgerMustBeInFuture)));

    // Non-admin cannot commit
    let res_unauth = client.try_commit_round(&f.player, &1, &(current_seq + 10), &salt);
    assert_eq!(res_unauth, Err(Ok(BeaconError::NotAuthorized)));

    // Valid commit to future ledger (current + 10)
    client.commit_round(&f.admin, &1, &(current_seq + 10), &salt);

    let round = client.get_round(&1).unwrap();
    assert_eq!(round.target_ledger_sequence, current_seq + 10);
    assert_eq!(round.is_revealed, false);
    assert_eq!(client.is_revealed(&1), false);

    // Duplicate round commit fails
    let res_dup = client.try_commit_round(&f.admin, &1, &(current_seq + 20), &salt);
    assert_eq!(res_dup, Err(Ok(BeaconError::RoundAlreadyExists)));
}

#[test]
fn test_seed_cannot_be_revealed_before_target_ledger() {
    let (f, client) = TestFixture::setup();

    let salt = BytesN::from_array(&f.env, &[7u8; 32]);
    let ledger_hash = BytesN::from_array(&f.env, &[99u8; 32]);
    let current_seq = f.env.ledger().sequence();

    // Commit to target ledger 50
    client.commit_round(&f.admin, &10, &(current_seq + 50), &salt);

    // Trying to get seed before reveal fails with SeedNotRevealedYet
    let res_get = client.try_get_seed(&10);
    assert_eq!(res_get, Err(Ok(BeaconError::SeedNotRevealedYet)));

    // Trying to reveal before reaching ledger 50 fails with CommittedLedgerNotReached
    let res_reveal = client.try_reveal_seed(&10, &ledger_hash);
    assert_eq!(res_reveal, Err(Ok(BeaconError::CommittedLedgerNotReached)));
    assert_eq!(client.is_revealed(&10), false);
}

#[test]
fn test_reveal_seed_and_deterministic_verification() {
    let (f, client) = TestFixture::setup();

    let salt = BytesN::from_array(&f.env, &[15u8; 32]);
    let ledger_hash = BytesN::from_array(&f.env, &[88u8; 32]);
    let current_seq = f.env.ledger().sequence();
    let target_seq = current_seq + 25;

    client.commit_round(&f.admin, &20, &target_seq, &salt);

    // Advance ledger sequence past target
    f.env.ledger().with_mut(|l| l.sequence_number = target_seq);

    // Reveal seed
    let seed = client.reveal_seed(&20, &ledger_hash);
    assert_eq!(client.is_revealed(&20), true);
    assert_eq!(client.get_seed(&20), seed);

    // Third party can verify seed
    let is_valid = client.verify_seed(&20, &ledger_hash, &seed);
    assert_eq!(is_valid, true);

    // Altered seed fails verification
    let wrong_seed = BytesN::from_array(&f.env, &[0u8; 32]);
    let is_valid_wrong = client.verify_seed(&20, &ledger_hash, &wrong_seed);
    assert_eq!(is_valid_wrong, false);

    // Idempotent reveal: exactly one seed per round
    let seed_again = client.reveal_seed(&20, &ledger_hash);
    assert_eq!(seed, seed_again);
}
