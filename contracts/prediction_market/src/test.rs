#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, token, Env};

fn create_token_contract<'a>(env: &Env, admin: &Address) -> (token::TokenClient<'a>, token::StellarAssetClient<'a>) {
    let contract_id = env.register_stellar_asset_contract_v2(admin.clone());
    (
        token::TokenClient::new(env, &contract_id.address()),
        token::StellarAssetClient::new(env, &contract_id.address()),
    )
}

#[test]
fn test_create_market() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let contract_id = env.register_contract(None, PredictionMarket);
    let client = PredictionMarketClient::new(&env, &contract_id);

    client.initialize(&admin);

    let mut outcomes = Vec::new(&env);
    outcomes.push_back(String::from_str(&env, "Team A wins"));
    outcomes.push_back(String::from_str(&env, "Team B wins"));

    let market_id = client.create_market(
        &creator,
        &String::from_str(&env, "Tournament Winner"),
        &outcomes,
        &1000000,
    );

    assert_eq!(market_id, 1);

    let market = client.get_market(&market_id);
    assert_eq!(market.status, MarketStatus::Open);
    assert_eq!(market.outcomes.len(), 2);
}

#[test]
fn test_place_bet_and_claim_winnings() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let (token, token_admin_client) = create_token_contract(&env, &token_admin);
    token_admin_client.mint(&user1, &1000);
    token_admin_client.mint(&user2, &1000);

    let contract_id = env.register_contract(None, PredictionMarket);
    let client = PredictionMarketClient::new(&env, &contract_id);

    client.initialize(&admin);

    let mut outcomes = Vec::new(&env);
    outcomes.push_back(String::from_str(&env, "Outcome A"));
    outcomes.push_back(String::from_str(&env, "Outcome B"));

    let market_id = client.create_market(
        &creator,
        &String::from_str(&env, "Test Market"),
        &outcomes,
        &1000000,
    );

    client.place_bet(&user1, &market_id, &0, &600, &token.address);
    client.place_bet(&user2, &market_id, &1, &400, &token.address);

    let market = client.get_market(&market_id);
    assert_eq!(market.total_pool, 1000);

    client.resolve_market(&admin, &market_id, &0);

    let payout = client.claim_winnings(&user1, &market_id, &token.address);
    assert_eq!(payout, 1000);
}

#[test]
fn test_liquidity_provision() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let liquidity_provider = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let (token, token_admin_client) = create_token_contract(&env, &token_admin);
    token_admin_client.mint(&liquidity_provider, &5000);

    let contract_id = env.register_contract(None, PredictionMarket);
    let client = PredictionMarketClient::new(&env, &contract_id);

    client.initialize(&admin);

    let mut outcomes = Vec::new(&env);
    outcomes.push_back(String::from_str(&env, "Yes"));
    outcomes.push_back(String::from_str(&env, "No"));

    let market_id = client.create_market(
        &creator,
        &String::from_str(&env, "Liquidity Test"),
        &outcomes,
        &1000000,
    );

    client.add_liquidity(&liquidity_provider, &market_id, &2000, &token.address);

    let market = client.get_market(&market_id);
    assert_eq!(market.liquidity_amount, 2000);
    assert_eq!(market.liquidity_provider, Some(liquidity_provider));
}

#[test]
fn test_partial_cashout() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let user = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let (token, token_admin_client) = create_token_contract(&env, &token_admin);
    token_admin_client.mint(&user, &1000);

    let contract_id = env.register_contract(None, PredictionMarket);
    let client = PredictionMarketClient::new(&env, &contract_id);

    client.initialize(&admin);

    let mut outcomes = Vec::new(&env);
    outcomes.push_back(String::from_str(&env, "A"));
    outcomes.push_back(String::from_str(&env, "B"));

    let market_id = client.create_market(
        &creator,
        &String::from_str(&env, "Cashout Test"),
        &outcomes,
        &1000000,
    );

    client.place_bet(&user, &market_id, &0, &500, &token.address);

    let cashout = client.partial_cashout(&user, &market_id, &0, &token.address);
    assert_eq!(cashout, 450); // 90% of 500
}

#[test]
fn test_dispute_resolution() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let user = Address::generate(&env);

    let contract_id = env.register_contract(None, PredictionMarket);
    let client = PredictionMarketClient::new(&env, &contract_id);

    client.initialize(&admin);

    let mut outcomes = Vec::new(&env);
    outcomes.push_back(String::from_str(&env, "X"));
    outcomes.push_back(String::from_str(&env, "Y"));

    let market_id = client.create_market(
        &creator,
        &String::from_str(&env, "Dispute Test"),
        &outcomes,
        &1000000,
    );

    client.resolve_market(&admin, &market_id, &0);

    client.raise_dispute(&user, &market_id, &String::from_str(&env, "Wrong outcome"));

    let market = client.get_market(&market_id);
    assert_eq!(market.status, MarketStatus::Disputed);

    client.resolve_dispute(&admin, &market_id, &Some(1));

    let resolved_market = client.get_market(&market_id);
    assert_eq!(resolved_market.status, MarketStatus::Resolved);
    assert_eq!(resolved_market.winning_outcome, Some(1));
}

#[test]
fn test_multiple_bets_same_user() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let user = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let (token, token_admin_client) = create_token_contract(&env, &token_admin);
    token_admin_client.mint(&user, &2000);

    let contract_id = env.register_contract(None, PredictionMarket);
    let client = PredictionMarketClient::new(&env, &contract_id);

    client.initialize(&admin);

    let mut outcomes = Vec::new(&env);
    outcomes.push_back(String::from_str(&env, "Option 1"));
    outcomes.push_back(String::from_str(&env, "Option 2"));

    let market_id = client.create_market(
        &creator,
        &String::from_str(&env, "Multi Bet Test"),
        &outcomes,
        &1000000,
    );

    client.place_bet(&user, &market_id, &0, &300, &token.address);
    client.place_bet(&user, &market_id, &0, &700, &token.address);

    let user_bets = client.get_user_bets(&user, &market_id);
    assert_eq!(user_bets.len(), 2);

    let pools = client.get_outcome_pools(&market_id);
    assert_eq!(pools.get(0).unwrap().total_amount, 1000);
}

#[test]
#[should_panic(expected = "Need at least 2 outcomes")]
fn test_create_market_insufficient_outcomes() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);

    let contract_id = env.register_contract(None, PredictionMarket);
    let client = PredictionMarketClient::new(&env, &contract_id);

    client.initialize(&admin);

    let mut outcomes = Vec::new(&env);
    outcomes.push_back(String::from_str(&env, "Only One"));

    client.create_market(
        &creator,
        &String::from_str(&env, "Invalid Market"),
        &outcomes,
        &1000000,
    );
}

#[test]
#[should_panic(expected = "Market not resolved")]
fn test_claim_before_resolution() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let user = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let (token, token_admin_client) = create_token_contract(&env, &token_admin);
    token_admin_client.mint(&user, &1000);

    let contract_id = env.register_contract(None, PredictionMarket);
    let client = PredictionMarketClient::new(&env, &contract_id);

    client.initialize(&admin);

    let mut outcomes = Vec::new(&env);
    outcomes.push_back(String::from_str(&env, "A"));
    outcomes.push_back(String::from_str(&env, "B"));

    let market_id = client.create_market(
        &creator,
        &String::from_str(&env, "Test"),
        &outcomes,
        &1000000,
    );

    client.place_bet(&user, &market_id, &0, &500, &token.address);
    client.claim_winnings(&user, &market_id, &token.address);
}
