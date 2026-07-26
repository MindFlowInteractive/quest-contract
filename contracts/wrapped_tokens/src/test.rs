#![cfg(test)]

extern crate std;

use soroban_sdk::{
    testutils::Address as _,
    Address, Bytes, BytesN, Env, String,
};

use crate::{
    types::{ChainId, OperationStatus, WrappedTokenConfig},
    WrappedTokensContract, WrappedTokensContractClient,
};

// ─────────────────────────────────────────────────────────────────────────────
// HELPERS
// ─────────────────────────────────────────────────────────────────────────────

fn make_config(env: &Env, admin: &Address, fee_collector: &Address) -> WrappedTokenConfig {
    WrappedTokenConfig {
        admin: admin.clone(),
        name: String::from_str(env, "Wrapped ETH"),
        symbol: String::from_str(env, "WETH"),
        decimals: 18,
        source_chain: ChainId::Ethereum,
        source_asset_id: Bytes::from_array(env, &[0u8; 20]),
        fee_collector: fee_collector.clone(),
        fee_bps: 30,
        min_fee: 100,
        paused: false,
        required_confirmations: 1,
    }
}

/// Register + initialize contract with one operator. Returns (client, admin, operator).
fn create_test_contract(env: &Env) -> (WrappedTokensContractClient, Address, Address) {
    let admin = Address::generate(env);
    let fee_collector = Address::generate(env);
    let operator = Address::generate(env);

    let contract_id = env.register_contract(None, WrappedTokensContract);
    let client = WrappedTokensContractClient::new(env, &contract_id);

    client.initialize(&make_config(env, &admin, &fee_collector));
    client.add_operator(&operator);

    (client, admin, operator)
}

fn make_tx_id(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn do_wrap(
    client: &WrappedTokensContractClient,
    operator: &Address,
    recipient: &Address,
    amount: i128,
    tx_seed: u8,
) -> u64 {
    let env = client.env();
    client.submit_wrap(
        operator,
        recipient,
        &amount,
        &ChainId::Ethereum,
        &make_tx_id(env, tx_seed),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 1 — initialize happy path
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let fee_collector = Address::generate(&env);
    let contract_id = env.register_contract(None, WrappedTokensContract);
    let client = WrappedTokensContractClient::new(&env, &contract_id);

    client.initialize(&make_config(&env, &admin, &fee_collector));

    assert_eq!(client.name(), String::from_str(&env, "Wrapped ETH"));
    assert_eq!(client.symbol(), String::from_str(&env, "WETH"));
    assert_eq!(client.decimals(), 18u32);
    assert_eq!(client.admin(), admin);
    assert!(!client.is_paused());
    assert_eq!(client.total_supply(), 0i128);

    let custody = client.get_custody_info();
    assert_eq!(custody.total_supply, 0);
    assert_eq!(custody.total_wraps, 0);
    assert_eq!(custody.total_unwraps, 0);
    assert_eq!(custody.total_fees_collected, 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 2 — cannot initialize twice
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_initialize_twice_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let fee_collector = Address::generate(&env);
    let contract_id = env.register_contract(None, WrappedTokensContract);
    let client = WrappedTokensContractClient::new(&env, &contract_id);

    let config = make_config(&env, &admin, &fee_collector);
    client.initialize(&config);
    client.initialize(&config); // AlreadyInitialized = 1
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 3 — add and remove operators
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_add_remove_operators() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, operator) = create_test_contract(&env);

    let ops = client.get_operators();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops.get(0).unwrap(), operator);

    let operator2 = Address::generate(&env);
    client.add_operator(&operator2);
    assert_eq!(client.get_operators().len(), 2);

    // Remove first operator
    client.remove_operator(&operator);
    let ops = client.get_operators();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops.get(0).unwrap(), operator2);
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_add_operator_duplicate_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, operator) = create_test_contract(&env);
    client.add_operator(&operator); // OperatorAlreadyExists = 13
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn test_remove_nonexistent_operator_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, _operator) = create_test_contract(&env);
    let ghost = Address::generate(&env);
    client.remove_operator(&ghost); // OperatorNotFound = 12
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 4 — submit_wrap single confirmation mints immediately
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_submit_wrap_single_confirmation() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, operator) = create_test_contract(&env);
    let recipient = Address::generate(&env);

    // required_confirmations = 1 → mint immediately
    // gross=1_000_000, fee_bps=30 → proportional=300, min_fee=100 → fee=300, net=999_700
    let nonce = do_wrap(&client, &operator, &recipient, 1_000_000, 0x01);

    assert_eq!(nonce, 0u64);
    assert_eq!(client.balance(&recipient), 999_700i128);
    assert_eq!(client.total_supply(), 999_700i128);

    let req = client.get_wrap_request(&nonce);
    assert_eq!(req.status, OperationStatus::Completed);
    assert_eq!(req.net_amount, 999_700i128);
    assert_eq!(req.fee_amount, 300i128);

    let custody = client.get_custody_info();
    assert_eq!(custody.total_wraps, 1);
    assert_eq!(custody.total_supply, 999_700i128);
    assert_eq!(custody.total_fees_collected, 300i128);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 5 — multi-confirmation wrap: Pending until threshold
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_submit_wrap_multi_confirmation() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let fee_collector = Address::generate(&env);
    let contract_id = env.register_contract(None, WrappedTokensContract);
    let client = WrappedTokensContractClient::new(&env, &contract_id);

    let mut config = make_config(&env, &admin, &fee_collector);
    config.required_confirmations = 2;
    client.initialize(&config);

    let operator1 = Address::generate(&env);
    let operator2 = Address::generate(&env);
    client.add_operator(&operator1);
    client.add_operator(&operator2);

    let recipient = Address::generate(&env);

    // First submit — Pending, no mint yet
    let nonce = client.submit_wrap(
        &operator1,
        &recipient,
        &2_000_000i128,
        &ChainId::Ethereum,
        &make_tx_id(&env, 0xAA),
    );

    assert_eq!(client.get_wrap_request(&nonce).status, OperationStatus::Pending);
    assert_eq!(client.balance(&recipient), 0i128);
    assert_eq!(client.total_supply(), 0i128);

    // Second confirm from operator2 — triggers mint
    // fee = 2_000_000 * 30 / 10000 = 600 > 100 → net = 1_999_400
    client.confirm_wrap(&operator2, &nonce);

    assert_eq!(client.get_wrap_request(&nonce).status, OperationStatus::Completed);
    assert_eq!(client.balance(&recipient), 1_999_400i128);
    assert_eq!(client.total_supply(), 1_999_400i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #19)")]
fn test_confirm_wrap_duplicate_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let fee_collector = Address::generate(&env);
    let contract_id = env.register_contract(None, WrappedTokensContract);
    let client = WrappedTokensContractClient::new(&env, &contract_id);

    let mut config = make_config(&env, &admin, &fee_collector);
    config.required_confirmations = 3;
    client.initialize(&config);

    let op1 = Address::generate(&env);
    let op2 = Address::generate(&env);
    client.add_operator(&op1);
    client.add_operator(&op2);

    let recipient = Address::generate(&env);
    let nonce = client.submit_wrap(
        &op1,
        &recipient,
        &500_000i128,
        &ChainId::Ethereum,
        &make_tx_id(&env, 0x10),
    );

    // op2 confirms once — fine
    client.confirm_wrap(&op2, &nonce);
    // op2 confirms again — AlreadyConfirmed = 19
    client.confirm_wrap(&op2, &nonce);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 6 — replay prevention
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_replay_prevention() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, operator) = create_test_contract(&env);
    let recipient = Address::generate(&env);
    let tx_id = make_tx_id(&env, 0x42);

    client.submit_wrap(&operator, &recipient, &500_000i128, &ChainId::Ethereum, &tx_id);
    // Same source_tx_id → NonceAlreadyUsed = 9
    client.submit_wrap(&operator, &recipient, &500_000i128, &ChainId::Ethereum, &tx_id);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 7 — initiate_unwrap burns tokens and records request
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_initiate_unwrap() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, operator) = create_test_contract(&env);
    let user = Address::generate(&env);

    // Mint 999_700 to user
    do_wrap(&client, &operator, &user, 1_000_000, 0x01);
    let balance_after_wrap = client.balance(&user); // 999_700

    let target_recipient = Bytes::from_array(&env, &[0xDEu8; 20]);
    let nonce = client.initiate_unwrap(
        &user,
        &balance_after_wrap,
        &ChainId::Ethereum,
        &target_recipient,
    );

    assert_eq!(nonce, 0u64);
    assert_eq!(client.balance(&user), 0i128);

    // fee on 999_700: 999_700 * 30 / 10000 = 299 (truncated) > 100 → fee=299, net=999_401
    let req = client.get_unwrap_request(&nonce);
    assert_eq!(req.status, OperationStatus::Pending);
    assert_eq!(req.gross_amount, balance_after_wrap);
    assert_eq!(req.fee_amount, 299i128);
    assert_eq!(req.net_amount, balance_after_wrap - 299);
    assert_eq!(req.user, user);
    assert_eq!(req.target_chain, ChainId::Ethereum);

    let custody = client.get_custody_info();
    assert_eq!(custody.total_unwraps, 1);
    assert_eq!(custody.total_supply, 0i128);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 8 — unwrap with insufficient balance fails
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_unwrap_insufficient_balance() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, operator) = create_test_contract(&env);
    let user = Address::generate(&env);

    do_wrap(&client, &operator, &user, 1_000_000, 0x01);

    let target = Bytes::from_array(&env, &[0xAAu8; 20]);
    // Try to unwrap more than balance → InsufficientBalance = 7
    client.initiate_unwrap(&user, &5_000_000i128, &ChainId::Ethereum, &target);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 9 — complete_unwrap marks request Completed
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_complete_unwrap() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, operator) = create_test_contract(&env);
    let user = Address::generate(&env);

    do_wrap(&client, &operator, &user, 1_000_000, 0x01);
    let balance = client.balance(&user);

    let target = Bytes::from_array(&env, &[0xBBu8; 20]);
    let nonce = client.initiate_unwrap(&user, &balance, &ChainId::Ethereum, &target);

    assert_eq!(client.get_unwrap_request(&nonce).status, OperationStatus::Pending);

    client.complete_unwrap(&operator, &nonce);

    assert_eq!(client.get_unwrap_request(&nonce).status, OperationStatus::Completed);
}

#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn test_complete_unwrap_twice_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, operator) = create_test_contract(&env);
    let user = Address::generate(&env);

    do_wrap(&client, &operator, &user, 1_000_000, 0x01);
    let balance = client.balance(&user);
    let target = Bytes::from_array(&env, &[0xCCu8; 20]);
    let nonce = client.initiate_unwrap(&user, &balance, &ChainId::Ethereum, &target);

    client.complete_unwrap(&operator, &nonce);
    client.complete_unwrap(&operator, &nonce); // RequestAlreadyProcessed = 11
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 10 — transfer between accounts
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_transfer() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, operator) = create_test_contract(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    do_wrap(&client, &operator, &alice, 1_000_000, 0x01);
    let alice_initial = client.balance(&alice); // 999_700

    client.transfer(&alice, &bob, &100_000i128);

    assert_eq!(client.balance(&alice), alice_initial - 100_000);
    assert_eq!(client.balance(&bob), 100_000i128);
    // Supply unchanged by transfer
    assert_eq!(client.total_supply(), alice_initial);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_transfer_insufficient_balance_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, operator) = create_test_contract(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    do_wrap(&client, &operator, &alice, 1_000_000, 0x01);
    let alice_balance = client.balance(&alice);

    // Try transferring more than alice has → InsufficientBalance = 7
    client.transfer(&alice, &bob, &(alice_balance + 1));
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 11 — approve and transfer_from
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_approve_and_transfer_from() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, operator) = create_test_contract(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let charlie = Address::generate(&env);

    do_wrap(&client, &operator, &alice, 1_000_000, 0x01);
    let alice_initial = client.balance(&alice); // 999_700

    // Alice approves bob to spend 200_000
    client.approve(&alice, &bob, &200_000i128);
    assert_eq!(client.allowance(&alice, &bob), 200_000i128);

    // Bob transfers 150_000 from alice to charlie
    client.transfer_from(&bob, &alice, &charlie, &150_000i128);

    assert_eq!(client.balance(&alice), alice_initial - 150_000);
    assert_eq!(client.balance(&charlie), 150_000i128);
    assert_eq!(client.allowance(&alice, &bob), 50_000i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_transfer_from_exceeds_allowance_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, operator) = create_test_contract(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let charlie = Address::generate(&env);

    do_wrap(&client, &operator, &alice, 1_000_000, 0x01);
    client.approve(&alice, &bob, &50_000i128);

    // Attempt to spend more than allowance → InsufficientBalance = 7
    client.transfer_from(&bob, &alice, &charlie, &100_000i128);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 12 — pause blocks operations, unpause restores them
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_pause_unpause() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, operator) = create_test_contract(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    do_wrap(&client, &operator, &alice, 1_000_000, 0x01);

    assert!(!client.is_paused());
    client.pause(&operator);
    assert!(client.is_paused());

    // Unpaused operations blocked — verify via the try_ client methods
    let result = client.try_transfer(&alice, &bob, &1_000i128);
    assert!(result.is_err());

    let result = client.try_submit_wrap(
        &operator,
        &alice,
        &500_000i128,
        &ChainId::Ethereum,
        &make_tx_id(&env, 0xFE),
    );
    assert!(result.is_err());

    // Admin unpauses
    client.unpause();
    assert!(!client.is_paused());

    // Operations restored
    client.transfer(&alice, &bob, &1_000i128);
    assert_eq!(client.balance(&bob), 1_000i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_pause_twice_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, operator) = create_test_contract(&env);
    client.pause(&operator);
    client.pause(&operator); // ContractPaused = 4
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_unpause_when_not_paused_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, _operator) = create_test_contract(&env);
    client.unpause(); // ContractNotPaused = 5
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 13 — fees accumulate and can be collected
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fee_collection() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let fee_collector = Address::generate(&env);
    let operator = Address::generate(&env);

    let contract_id = env.register_contract(None, WrappedTokensContract);
    let client = WrappedTokensContractClient::new(&env, &contract_id);
    client.initialize(&make_config(&env, &admin, &fee_collector));
    client.add_operator(&operator);

    let recipient = Address::generate(&env);

    // Wrap 1: gross=1_000_000, fee=300
    do_wrap(&client, &operator, &recipient, 1_000_000, 0x01);
    // Wrap 2: gross=2_000_000, fee=600
    do_wrap(&client, &operator, &recipient, 2_000_000, 0x02);

    assert_eq!(client.get_custody_info().total_fees_collected, 900i128);
    assert_eq!(client.balance(&fee_collector), 0i128);

    client.collect_fees();

    assert_eq!(client.balance(&fee_collector), 900i128);
    assert_eq!(client.get_custody_info().total_fees_collected, 0i128);

    // Second collect when empty is a no-op
    client.collect_fees();
    assert_eq!(client.balance(&fee_collector), 900i128);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 14 — total_supply goes up on mint, down on burn
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_total_supply_tracking() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, operator) = create_test_contract(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    assert_eq!(client.total_supply(), 0i128);

    // Wrap into alice: gross=1_000_000, fee=300, net=999_700
    do_wrap(&client, &operator, &alice, 1_000_000, 0x01);
    assert_eq!(client.total_supply(), 999_700i128);

    // Wrap into bob: gross=500_000, fee=150>100, net=499_850
    do_wrap(&client, &operator, &bob, 500_000, 0x02);
    assert_eq!(client.total_supply(), 999_700 + 499_850);

    // Transfer does not change supply
    client.transfer(&alice, &bob, &50_000i128);
    assert_eq!(client.total_supply(), 999_700 + 499_850);

    // Unwrap burns gross_amount from supply
    // alice balance now = 949_700; unwrap 200_000
    // fee on 200_000: 200_000*30/10000=60 < 100 → fee=100, net=199_900
    let target = Bytes::from_array(&env, &[0xCCu8; 20]);
    client.initiate_unwrap(&alice, &200_000i128, &ChainId::Ethereum, &target);

    let expected = (999_700i128 + 499_850) - 200_000;
    assert_eq!(client.total_supply(), expected);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 15 — non-operator cannot submit wrap
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_unauthorized_operator() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, _operator) = create_test_contract(&env);
    let imposter = Address::generate(&env);
    let recipient = Address::generate(&env);

    // imposter is not registered as operator → Unauthorized = 3
    client.submit_wrap(
        &imposter,
        &recipient,
        &1_000_000i128,
        &ChainId::Ethereum,
        &make_tx_id(&env, 0x77),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 16 — get_wrap_request / get_unwrap_request return correct data
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_wrap_unwrap_request() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let fee_collector = Address::generate(&env);
    let contract_id = env.register_contract(None, WrappedTokensContract);
    let client = WrappedTokensContractClient::new(&env, &contract_id);

    // 2-confirmation setup so we can inspect Pending state
    let mut config = make_config(&env, &admin, &fee_collector);
    config.required_confirmations = 2;
    client.initialize(&config);

    let op1 = Address::generate(&env);
    let op2 = Address::generate(&env);
    client.add_operator(&op1);
    client.add_operator(&op2);

    let recipient = Address::generate(&env);
    let source_tx = make_tx_id(&env, 0x55);

    let nonce = client.submit_wrap(
        &op1,
        &recipient,
        &3_000_000i128,
        &ChainId::Polygon,
        &source_tx,
    );

    // Inspect WrapRequest while Pending
    let req = client.get_wrap_request(&nonce);
    assert_eq!(req.nonce, nonce);
    assert_eq!(req.recipient, recipient);
    assert_eq!(req.gross_amount, 3_000_000i128);
    assert_eq!(req.source_chain, ChainId::Polygon);
    assert_eq!(req.source_tx_id, source_tx);
    assert_eq!(req.status, OperationStatus::Pending);
    assert_eq!(req.operator, op1);
    // fee = 3_000_000 * 30 / 10000 = 900 > 100 → fee=900, net=2_999_100
    assert_eq!(req.fee_amount, 900i128);
    assert_eq!(req.net_amount, 2_999_100i128);

    // Second confirmation → Completed
    client.confirm_wrap(&op2, &nonce);
    assert_eq!(client.get_wrap_request(&nonce).status, OperationStatus::Completed);

    // Inspect UnwrapRequest
    let user = recipient.clone();
    let user_balance = client.balance(&user);
    let target = Bytes::from_array(&env, &[0xEEu8; 20]);
    let unwrap_nonce = client.initiate_unwrap(
        &user,
        &user_balance,
        &ChainId::BinanceSmartChain,
        &target,
    );

    let ureq = client.get_unwrap_request(&unwrap_nonce);
    assert_eq!(ureq.nonce, unwrap_nonce);
    assert_eq!(ureq.user, user);
    assert_eq!(ureq.gross_amount, user_balance);
    assert_eq!(ureq.target_chain, ChainId::BinanceSmartChain);
    assert_eq!(ureq.target_recipient, target);
    assert_eq!(ureq.status, OperationStatus::Pending);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_get_nonexistent_wrap_request_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, _operator) = create_test_contract(&env);
    client.get_wrap_request(&9999u64); // RequestNotFound = 10
}
