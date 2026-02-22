use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    Env,
};

fn create_token_contract<'a>(env: &Env, admin: &Address) -> (TokenClient<'a>, StellarAssetClient<'a>) {
    let contract_address = env.register_stellar_asset_contract_v2(admin.clone());
    (
        TokenClient::new(env, &contract_address.address()),
        StellarAssetClient::new(env, &contract_address.address()),
    )
}

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, YieldFarmingContract);
    let client = YieldFarmingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let reward_token = Address::generate(&env);
    
    client.initialize(&admin, &reward_token);
}

#[test]
fn test_create_pool() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, YieldFarmingContract);
    let client = YieldFarmingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let reward_token = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin, &reward_token);
    
    let pool_id = client.create_pool(
        &asset,
        &AssetType::Token,
        &1000,  // 10% APY
        &30,    // 30 days lock
        &500,   // 5% penalty
        &10000, // 1x multiplier
        &false,
    );
    
    assert_eq!(pool_id, 1);
    
    let pool = client.get_pool(&pool_id);
    assert_eq!(pool.apy_basis_points, 1000);
    assert_eq!(pool.lock_period_days, 30);
}

#[test]
fn test_stake_tokens() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, YieldFarmingContract);
    let client = YieldFarmingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let staker = Address::generate(&env);
    let token_admin = Address::generate(&env);
    
    let (token, token_admin_client) = create_token_contract(&env, &token_admin);
    let (reward_token, _) = create_token_contract(&env, &token_admin);
    
    token_admin_client.mint(&staker, &1000);
    
    client.initialize(&admin, &reward_token.address);
    
    let pool_id = client.create_pool(
        &token.address,
        &AssetType::Token,
        &1000,
        &30,
        &500,
        &10000,
        &false,
    );
    
    client.stake_tokens(&staker, &pool_id, &500);
    
    let stakes = client.get_user_stakes(&staker);
    assert_eq!(stakes.len(), 1);
    
    let position = client.get_stake(&staker, &1);
    assert_eq!(position.amount, 500);
    assert_eq!(position.pool_id, pool_id);
    
    let stats = client.get_pool_stats(&pool_id);
    assert_eq!(stats.total_staked, 500);
    assert_eq!(stats.total_stakers, 1);
}

#[test]
fn test_calculate_rewards() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
    });
    
    let contract_id = env.register_contract(None, YieldFarmingContract);
    let client = YieldFarmingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let staker = Address::generate(&env);
    let token_admin = Address::generate(&env);
    
    let (token, token_admin_client) = create_token_contract(&env, &token_admin);
    let (reward_token, _) = create_token_contract(&env, &token_admin);
    
    token_admin_client.mint(&staker, &10000);
    
    client.initialize(&admin, &reward_token.address);
    
    let pool_id = client.create_pool(
        &token.address,
        &AssetType::Token,
        &1000,  // 10% APY
        &30,
        &500,
        &10000, // 1x multiplier
        &false,
    );
    
    client.stake_tokens(&staker, &pool_id, &10000);
    
    // Fast forward 1 year
    env.ledger().with_mut(|li| {
        li.timestamp = 1000 + 31_536_000;
    });
    
    let rewards = client.calculate_rewards(&staker, &1);
    
    // Should be approximately 10% of 10000 = 1000
    assert!(rewards >= 900 && rewards <= 1100);
}

#[test]
fn test_claim_rewards() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
    });
    
    let contract_id = env.register_contract(None, YieldFarmingContract);
    let client = YieldFarmingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let staker = Address::generate(&env);
    let token_admin = Address::generate(&env);
    
    let (token, token_admin_client) = create_token_contract(&env, &token_admin);
    let (reward_token, reward_admin_client) = create_token_contract(&env, &token_admin);
    
    token_admin_client.mint(&staker, &10000);
    reward_admin_client.mint(&contract_id, &100000);
    
    client.initialize(&admin, &reward_token.address);
    
    let pool_id = client.create_pool(
        &token.address,
        &AssetType::Token,
        &1000,
        &30,
        &500,
        &10000,
        &false,
    );
    
    client.stake_tokens(&staker, &pool_id, &10000);
    
    // Fast forward 6 months
    env.ledger().with_mut(|li| {
        li.timestamp = 1000 + 15_768_000;
    });
    
    let initial_balance = reward_token.balance(&staker);
    let claimed = client.claim_rewards(&staker, &1);
    let final_balance = reward_token.balance(&staker);
    
    assert!(claimed > 0);
    assert_eq!(final_balance - initial_balance, claimed);
}

#[test]
fn test_unstake_after_lock() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
    });
    
    let contract_id = env.register_contract(None, YieldFarmingContract);
    let client = YieldFarmingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let staker = Address::generate(&env);
    let token_admin = Address::generate(&env);
    
    let (token, token_admin_client) = create_token_contract(&env, &token_admin);
    let (reward_token, reward_admin_client) = create_token_contract(&env, &token_admin);
    
    token_admin_client.mint(&staker, &10000);
    reward_admin_client.mint(&contract_id, &100000);
    
    client.initialize(&admin, &reward_token.address);
    
    let pool_id = client.create_pool(
        &token.address,
        &AssetType::Token,
        &1000,
        &30,
        &500,
        &10000,
        &false,
    );
    
    client.stake_tokens(&staker, &pool_id, &5000);
    
    let initial_balance = token.balance(&staker);
    
    // Fast forward past lock period (30 days)
    env.ledger().with_mut(|li| {
        li.timestamp = 1000 + (30 * 86_400) + 1;
    });
    
    let returned = client.unstake(&staker, &1);
    let final_balance = token.balance(&staker);
    
    // Should get full amount back (no penalty)
    assert_eq!(returned, 5000);
    assert_eq!(final_balance - initial_balance, 5000);
    
    let stats = client.get_pool_stats(&pool_id);
    assert_eq!(stats.total_staked, 0);
}

#[test]
fn test_early_withdrawal_penalty() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
    });
    
    let contract_id = env.register_contract(None, YieldFarmingContract);
    let client = YieldFarmingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let staker = Address::generate(&env);
    let token_admin = Address::generate(&env);
    
    let (token, token_admin_client) = create_token_contract(&env, &token_admin);
    let (reward_token, reward_admin_client) = create_token_contract(&env, &token_admin);
    
    token_admin_client.mint(&staker, &10000);
    reward_admin_client.mint(&contract_id, &100000);
    
    client.initialize(&admin, &reward_token.address);
    
    let pool_id = client.create_pool(
        &token.address,
        &AssetType::Token,
        &1000,
        &30,
        &500,   // 5% penalty
        &10000,
        &false,
    );
    
    client.stake_tokens(&staker, &pool_id, &10000);
    
    let initial_balance = token.balance(&staker);
    
    // Unstake early (only 10 days)
    env.ledger().with_mut(|li| {
        li.timestamp = 1000 + (10 * 86_400);
    });
    
    let returned = client.unstake(&staker, &1);
    let final_balance = token.balance(&staker);
    
    // Should get 95% back (5% penalty)
    assert_eq!(returned, 9500);
    assert_eq!(final_balance - initial_balance, 9500);
}

#[test]
fn test_auto_compounding() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
    });
    
    let contract_id = env.register_contract(None, YieldFarmingContract);
    let client = YieldFarmingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let staker = Address::generate(&env);
    let token_admin = Address::generate(&env);
    
    let (token, token_admin_client) = create_token_contract(&env, &token_admin);
    let (reward_token, _) = create_token_contract(&env, &token_admin);
    
    token_admin_client.mint(&staker, &10000);
    
    client.initialize(&admin, &reward_token.address);
    
    let pool_id = client.create_pool(
        &token.address,
        &AssetType::Token,
        &1000,
        &30,
        &500,
        &10000,
        &true,  // Auto-compound enabled
    );
    
    client.stake_tokens(&staker, &pool_id, &10000);
    
    let initial_position = client.get_stake(&staker, &1);
    assert_eq!(initial_position.amount, 10000);
    
    // Fast forward 6 months
    env.ledger().with_mut(|li| {
        li.timestamp = 1000 + 15_768_000;
    });
    
    client.claim_rewards(&staker, &1);
    
    let updated_position = client.get_stake(&staker, &1);
    
    // Amount should have increased due to auto-compounding
    assert!(updated_position.amount > 10000);
}

#[test]
fn test_multiplier_bonus() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
    });
    
    let contract_id = env.register_contract(None, YieldFarmingContract);
    let client = YieldFarmingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let staker = Address::generate(&env);
    let token_admin = Address::generate(&env);
    
    let (token, token_admin_client) = create_token_contract(&env, &token_admin);
    let (reward_token, _) = create_token_contract(&env, &token_admin);
    
    token_admin_client.mint(&staker, &20000);
    
    client.initialize(&admin, &reward_token.address);
    
    // Pool with 1.5x multiplier
    let pool_id = client.create_pool(
        &token.address,
        &AssetType::Token,
        &1000,
        &30,
        &500,
        &15000, // 1.5x multiplier
        &false,
    );
    
    client.stake_tokens(&staker, &pool_id, &10000);
    
    // Fast forward 1 year
    env.ledger().with_mut(|li| {
        li.timestamp = 1000 + 31_536_000;
    });
    
    let rewards = client.calculate_rewards(&staker, &1);
    
    // Should be approximately 15% of 10000 = 1500 (10% APY * 1.5x multiplier)
    assert!(rewards >= 1400 && rewards <= 1600);
}

#[test]
fn test_nft_staking() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, YieldFarmingContract);
    let client = YieldFarmingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let staker = Address::generate(&env);
    let token_admin = Address::generate(&env);
    
    let (nft_token, nft_admin_client) = create_token_contract(&env, &token_admin);
    let (reward_token, _) = create_token_contract(&env, &token_admin);
    
    nft_admin_client.mint(&staker, &1);
    
    client.initialize(&admin, &reward_token.address);
    
    let pool_id = client.create_pool(
        &nft_token.address,
        &AssetType::NFT,
        &2000,  // 20% APY for NFTs
        &60,
        &1000,
        &20000, // 2x multiplier
        &false,
    );
    
    client.stake_nft(&staker, &pool_id, &123);
    
    let position = client.get_stake(&staker, &1);
    assert_eq!(position.amount, 1);
    assert_eq!(position.nft_id, Some(123));
    
    let stats = client.get_pool_stats(&pool_id);
    assert_eq!(stats.total_staked, 1);
}
