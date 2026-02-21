#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, token, Address, Env};

fn create_token_contract<'a>(e: &Env, admin: &Address) -> (token::Client<'a>, token::StellarAssetClient<'a>) {
    let contract_address = e.register_stellar_asset_contract_v2(admin.clone()).address();
    (
        token::Client::new(e, &contract_address),
        token::StellarAssetClient::new(e, &contract_address),
    )
}

fn create_prize_pool_contract<'a>(e: &Env) -> PrizePoolContractClient<'a> {
    let contract_id = e.register_contract(None, PrizePoolContract);
    PrizePoolContractClient::new(e, &contract_id)
}

#[test]
fn test_prize_pool_flow() {
    let e = Env::default();
    e.mock_all_auths();

    let owner = Address::generate(&e);
    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let user2 = Address::generate(&e);
    let user3 = Address::generate(&e);
    let token_admin = Address::generate(&e);

    let (token_client, token_admin_client) = create_token_contract(&e, &token_admin);
    let pp_client = create_prize_pool_contract(&e);

    // Mint to users
    token_admin_client.mint(&user1, &1000);
    token_admin_client.mint(&user2, &1000);
    token_admin_client.mint(&user3, &1000);

    // Init
    pp_client.init(&owner, &token_client.address);

    // Admin creates pool with min_threshold 300 and claim_period 0 (so rollover allowed immediately)
    let pool_id = pp_client.create_pool(&admin, &300i128, &0u64);

    // Users contribute
    pp_client.contribute(&user1, &pool_id, &100i128);
    pp_client.contribute(&user2, &pool_id, &100i128);
    pp_client.contribute(&user3, &pool_id, &150i128);

    // Fetch pool and verify total
    let pool = pp_client.get_pool(&pool_id);
    assert_eq!(pool.total, 350);

    // Distribute to user1 and user2 (admin must call)
    let mut winners: Vec<Address> = Vec::new(&e);
    winners.push_back(user1.clone());
    winners.push_back(user2.clone());

    pp_client.distribute(&admin, &pool_id, &winners);

    // user1 claims
    pp_client.claim(&user1, &pool_id);
    // user1 had 1000, contributed 100 => 900; receives 175 => 1075
    assert_eq!(token_client.balance(&user1), 1075);

    // user2 doesn't claim; rollover to a new pool
    let target_pool = pp_client.create_pool(&admin, &10i128, &0u64);
    pp_client.rollover_unclaimed(&owner, &pool_id, &target_pool);

    // target pool should have received the unclaimed amount (175)
    let t = pp_client.get_pool(&target_pool);
    assert_eq!(t.total, 175);

    // stats: total_distributed should reflect claimed amount (175)
    let stats = pp_client.get_stats();
    assert_eq!(stats.total_distributed, 175);
}
