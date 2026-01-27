#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Env, Symbol, Vec, Val, IntoVal};
use reward_token::{RewardToken, RewardTokenClient};

#[test]
fn test_governance_flow() {
    let env = Env::default();
    env.mock_all_auths();

    // 1. Setup Token (Use RewardToken which has total_supply)
    let token_contract_id = env.register_contract(None, RewardToken);
    let token = RewardTokenClient::new(&env, &token_contract_id);
    
    let admin = Address::generate(&env);
    token.initialize(
        &admin,
        &String::from_str(&env, "Test Token"),
        &String::from_str(&env, "TEST"),
        &6,
    );

    // 2. Setup Governance
    let governance_contract_id = env.register_contract(None, GovernanceContract);
    let governance_client = GovernanceContractClient::new(&env, &governance_contract_id);

    // 3. Initialize Governance
    // Voting delay: 100s, Period: 1000s, Threshold: 100, Quorum: 10%
    governance_client.initialize(
        &token_contract_id,
        &100,
        &1000,
        &100,
        &10,
    );

    // Authorize governance contract as minter
    token.authorize_minter(&governance_contract_id);

    // 4. Mint tokens to users
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let user3 = Address::generate(&env);

    token.mint(&admin, &user1, &1000);
    token.mint(&admin, &user2, &500);
    token.mint(&admin, &user3, &100);

    // 5. User 1 deposits
    governance_client.deposit(&user1, &500); // User1 has 500 VP
    
    assert_eq!(governance_client.get_user_voting_power(&user1), 500);
    assert_eq!(token.balance(&user1), 500);
    assert_eq!(token.balance(&governance_contract_id), 500);

    // 6. User 2 delegates to User 1 then deposits
    governance_client.delegate(&user2, &user1);
    governance_client.deposit(&user2, &500); // User1 gets +500 VP

    assert_eq!(governance_client.get_user_voting_power(&user1), 1000); // 500 own + 500 delegated
    assert_eq!(governance_client.get_user_voting_power(&user2), 0);

    // 7. Create Proposal
    
    let action = ProposalActionInput {
        contract_id: token_contract_id.clone(),
        function_name: Symbol::new(&env, "mint"),
        args: Vec::from_array(&env, [
            governance_contract_id.into_val(&env), // minter (governance contract)
            user3.into_val(&env), // Address implements IntoVal<Env, Val>
            1000_i128.into_val(&env),
        ]),
    };

    let proposal_id = governance_client.propose(
        &user1,
        &String::from_str(&env, "Proposal 1"),
        &String::from_str(&env, "Mint tokens to User3"),
        &Some(action),
        &0,
    );

    let proposal = governance_client.get_proposal_info(&proposal_id);
    assert_eq!(proposal.status, ProposalStatus::Pending);

    // 8. Advance time to voting period
    env.ledger().with_mut(|li| {
        li.timestamp += 200; // Past delay (100)
    });

    // 9. Vote
    // User 1 votes (has 1000 VP)
    governance_client.vote(&user1, &proposal_id, &VoteType::For);

    let proposal = governance_client.get_proposal_info(&proposal_id);
    assert_eq!(proposal.for_votes, 1000);
    assert_eq!(proposal.status, ProposalStatus::Active);

    // 10. Advance time to end
    env.ledger().with_mut(|li| {
        li.timestamp += 1100; // Past end time
    });

    // 11. Execute
    // Note: Execution calls `token.mint`. `RewardToken.mint` checks `is_authorized_minter` or admin.
    // `mock_all_auths` should pass auth checks.
    
    governance_client.execute(&proposal_id);
    
    let proposal = governance_client.get_proposal_info(&proposal_id);
    assert_eq!(proposal.status, ProposalStatus::Executed);
    
    // Check if mint happened (User3 started with 100, minted 1000 -> 1100)
    assert_eq!(token.balance(&user3), 1100);
}
