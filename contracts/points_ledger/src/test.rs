use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Env,
};

struct TestFixture {
    env: Env,
    _admin: Address,
    game_contract: Address,
    player: Address,
}

impl TestFixture {
    fn setup() -> (Self, PointsLedgerContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let game_contract = Address::generate(&env);
        let player = Address::generate(&env);

        let contract_id = env.register_contract(None, PointsLedgerContract);
        let client = PointsLedgerContractClient::new(&env, &contract_id);

        client.initialize(&admin);
        client.authorize_caller(&admin, &game_contract);

        let fixture = Self {
            env,
            _admin: admin,
            game_contract,
            player,
        };

        (fixture, client)
    }
}

#[test]
fn test_authorized_earn_and_balance_equals_sum() {
    let (f, client) = TestFixture::setup();

    let reason_puzzle = Symbol::new(&f.env, "PUZZLE_1");
    let reason_achieve = Symbol::new(&f.env, "ACHIEVE_1");

    // Unauthorized caller cannot award points
    let stranger = Address::generate(&f.env);
    let res = client.try_earn(&stranger, &f.player, &100, &reason_puzzle);
    assert_eq!(res, Err(Ok(PointsLedgerError::NotAuthorized)));

    // Authorized game contract awards points
    client.earn(&f.game_contract, &f.player, &150, &reason_puzzle);
    client.earn(&f.game_contract, &f.player, &50, &reason_achieve);

    assert_eq!(client.get_balance(&f.player), 200);
    assert_eq!(client.get_total_earned(&f.player), 200);

    let history = client.get_history(&f.player);
    assert_eq!(history.len(), 2);
    assert_eq!(history.get(0).unwrap().amount, 150);
    assert_eq!(history.get(1).unwrap().amount, 50);
}

#[test]
fn test_fifo_consumption_order() {
    let (f, client) = TestFixture::setup();

    let tag1 = Symbol::new(&f.env, "FIRST");
    let tag2 = Symbol::new(&f.env, "SECOND");
    let tag3 = Symbol::new(&f.env, "THIRD");

    // Earn 3 distinct batches: 100, 50, 200 (Total = 350)
    client.earn(&f.game_contract, &f.player, &100, &tag1);
    client.earn(&f.game_contract, &f.player, &50, &tag2);
    client.earn(&f.game_contract, &f.player, &200, &tag3);

    assert_eq!(client.get_balance(&f.player), 350);

    // Spend 120 points at shop:
    // Should consume all 100 from FIRST batch, and 20 from SECOND batch!
    let shop_reason = Symbol::new(&f.env, "SHOP_SWORD");
    let remaining = client.spend(&f.player, &120, &shop_reason);
    assert_eq!(remaining, 230);
    assert_eq!(client.get_balance(&f.player), 230);

    let history = client.get_history(&f.player);
    // Batch 1 (100) has remaining = 0
    assert_eq!(history.get(0).unwrap().remaining, 0);
    // Batch 2 (50) has remaining = 30
    assert_eq!(history.get(1).unwrap().remaining, 30);
    // Batch 3 (200) has remaining = 200
    assert_eq!(history.get(2).unwrap().remaining, 200);
}

#[test]
fn test_expired_points_excluded_from_balance_and_spend() {
    let (f, client) = TestFixture::setup();

    let tag_expiring = Symbol::new(&f.env, "TIMED");
    let tag_permanent = Symbol::new(&f.env, "PERM");

    // Earn 100 expiring in 3600 seconds, and 50 permanent (validity = 0)
    client.earn_with_expiry(&f.game_contract, &f.player, &100, &tag_expiring, &3600);
    client.earn_with_expiry(&f.game_contract, &f.player, &50, &tag_permanent, &0);

    assert_eq!(client.get_balance(&f.player), 150);

    // Fast-forward ledger time past 3600 seconds (e.g. 4000s)
    f.env.ledger().with_mut(|l| l.timestamp += 4000);

    // Expired points must be excluded from available balance
    assert_eq!(client.get_balance(&f.player), 50);

    // Attempting to spend 60 points must fail with InsufficientBalance
    let shop_reason = Symbol::new(&f.env, "POTION");
    let res = client.try_spend(&f.player, &60, &shop_reason);
    assert_eq!(res, Err(Ok(PointsLedgerError::InsufficientBalance)));

    // Spending 30 points succeeds from unexpired balance
    let remaining = client.spend(&f.player, &30, &shop_reason);
    assert_eq!(remaining, 20);
    assert_eq!(client.get_balance(&f.player), 20);
}

#[test]
fn test_spending_exceeding_balance_fails() {
    let (f, client) = TestFixture::setup();
    let tag = Symbol::new(&f.env, "BONUS");

    client.earn(&f.game_contract, &f.player, &50, &tag);

    // Attempting to spend 51 points fails
    let shop = Symbol::new(&f.env, "ITEM");
    let res = client.try_spend(&f.player, &51, &shop);
    assert_eq!(res, Err(Ok(PointsLedgerError::InsufficientBalance)));
}
