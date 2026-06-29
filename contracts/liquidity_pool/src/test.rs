use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};
use soroban_sdk::token::Client as TokenClient;

fn create_token_contract(e: &Env, admin: &Address) -> Address {
    let token = e.register_stellar_asset_contract(admin.clone());
    token
}

#[test]
fn test_initialize_pool() {
    let env = Env::default();
    let admin = Address::generate(&env);
    
    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);
    
    let pool = env.register(LiquidityPoolContract, ());
    let pool_client = LiquidityPoolPoolClient::new(&env, &pool);
    
    pool_client.initialize(&admin, &token_a, &token_b, &30); // 0.3% fee
    
    let (token0, token1) = pool_client.get_tokens();
    assert!(token0 < token1);
    assert_eq!(pool_client.get_reserves(), (0, 0));
}

#[test]
fn test_add_liquidity() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);
    
    let pool = env.register(LiquidityPoolContract, ());
    let pool_client = LiquidityPoolPoolClient::new(&env, &pool);
    
    pool_client.initialize(&admin, &token_a, &token_b, &30);
    
    // Mint tokens to user
    let client_a = TokenClient::new(&env, &token_a);
    let client_b = TokenClient::new(&env, &token_b);
    client_a.mint(&admin, &user, &1000);
    client_b.mint(&admin, &user, &1000);
    
    // Add liquidity
    let liquidity = pool_client.add_liquidity(
        &user,
        &100,
        &100,
        &100,
        &100,
        &user,
    );
    
    assert!(liquidity == 100); // sqrt(100*100) = 100
    assert_eq!(pool_client.get_reserves(), (100, 100));
    assert_eq!(pool_client.balance_of(&user), 100);
    assert_eq!(pool_client.total_supply(), 100);
}

#[test]
fn test_swap() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);
    
    let pool = env.register(LiquidityPoolContract, ());
    let pool_client = LiquidityPoolPoolClient::new(&env, &pool);
    
    pool_client.initialize(&admin, &token_a, &token_b, &30); // 0.3% fee
    
    // Mint tokens and add liquidity
    let client_a = TokenClient::new(&env, &token_a);
    let client_b = TokenClient::new(&env, &token_b);
    client_a.mint(&admin, &user, &10000);
    client_b.mint(&admin, &user, &10000);
    
    pool_client.add_liquidity(
        &user,
        &1000,
        &1000,
        &1000,
        &1000,
        &user,
    );
    
    // Swap 100 token_a for token_b
    let (token0, _) = pool_client.get_tokens();
    let amount_out = pool_client.swap(
        &user,
        &100,
        &90, // Min output
        &token0,
        &user,
    );
    
    // With 0.3% fee, amount_out should be ~99.7
    assert!(amount_out > 90 && amount_out < 100);
    
    let (reserve_a, reserve_b) = pool_client.get_reserves();
    assert_eq!(reserve_a, 1100);
    assert_eq!(reserve_b, 1000 - amount_out);
}

#[test]
fn test_remove_liquidity() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);
    
    let pool = env.register(LiquidityPoolContract, ());
    let pool_client = LiquidityPoolPoolClient::new(&env, &pool);
    
    pool_client.initialize(&admin, &token_a, &token_b, &30);
    
    // Mint and add liquidity
    let client_a = TokenClient::new(&env, &token_a);
    let client_b = TokenClient::new(&env, &token_b);
    client_a.mint(&admin, &user, &1000);
    client_b.mint(&admin, &user, &1000);
    
    pool_client.add_liquidity(
        &user,
        &100,
        &100,
        &100,
        &100,
        &user,
    );
    
    // Remove liquidity
    let (amount_a, amount_b) = pool_client.remove_liquidity(
        &user,
        &100,
        &100,
        &100,
        &user,
    );
    
    assert_eq!(amount_a, 100);
    assert_eq!(amount_b, 100);
    assert_eq!(pool_client.balance_of(&user), 0);
    assert_eq!(pool_client.total_supply(), 0);
    assert_eq!(pool_client.get_reserves(), (0, 0));
}

#[test]
#[should_panic(expected = "code = 9")]
fn test_slippage_protection() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);
    
    let pool = env.register(LiquidityPoolContract, ());
    let pool_client = LiquidityPoolPoolClient::new(&env, &pool);
    
    pool_client.initialize(&admin, &token_a, &token_b, &30);
    
    // Mint and add liquidity
    let client_a = TokenClient::new(&env, &token_a);
    let client_b = TokenClient::new(&env, &token_b);
    client_a.mint(&admin, &user, &10000);
    client_b.mint(&admin, &user, &10000);
    
    pool_client.add_liquidity(
        &user,
        &1000,
        &1000,
        &1000,
        &1000,
        &user,
    );
    
    // Try to swap with unrealistic min output to trigger panic
    let (token0, _) = pool_client.get_tokens();
    pool_client.swap(
        &user,
        &100,
        &999, // Way too high min output
        &token0,
        &user,
    );
}

#[test]
fn test_impermanent_loss_calculation() {
    let env = Env::default();
    let admin = Address::generate(&env);
    
    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);
    
    let pool = env.register(LiquidityPoolContract, ());
    let pool_client = LiquidityPoolPoolClient::new(&env, &pool);
    
    pool_client.initialize(&admin, &token_a, &token_b, &30);
    
    // Price doubles: initial 1.0, current 2.0
    let il = pool_client.calculate_impermanent_loss(
        &1_000_000_000_000, // 1.0
        &2_000_000_000_000, // 2.0
    );
    
    // IL should be approx -0.057 (5.7% loss)
    assert!(il < 0 && il > -60_000_000_000);
}

#[test]
fn test_price_oracle() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);
    
    let pool = env.register(LiquidityPoolContract, ());
    let pool_client = LiquidityPoolPoolClient::new(&env, &pool);
    
    pool_client.initialize(&admin, &token_a, &token_b, &30);
    
    // Add initial liquidity
    let client_a = TokenClient::new(&env, &token_a);
    let client_b = TokenClient::new(&env, &token_b);
    client_a.mint(&admin, &user, &10000);
    client_b.mint(&admin, &user, &10000);
    
    pool_client.add_liquidity(
        &user,
        &1000,
        &1000,
        &1000,
        &1000,
        &user,
    );
    
    // Advance ledger time and swap to update oracle
    env.ledger().set_timestamp(1000);
    let (token0, _) = pool_client.get_tokens();
    
    pool_client.swap(
        &user,
        &100,
        &90,
        &token0,
        &user,
    );
    
    let cumulative_price = pool_client.get_cumulative_price();
    assert!(cumulative_price > 0);
}