#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Env, Symbol, Vec, String};
use reward_token::{RewardToken, RewardTokenClient};
use crate::types::{ProposalCategory, VoteType, MembershipTier};

#[test]
fn test_dao_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, PuzzleDaoContract);
    let client = PuzzleDaoContractClient::new(&env, &contract_id);
    
    let token_id = env.register_contract(None, RewardToken);
    let treasury_address = Address::generate(&env);
    
    client.initialize(
        &token_id,
        &treasury_address,
        &100,     // voting_delay
        &604800,  // voting_period (7 days)
        &1000,    // proposal_threshold
        &10,      // quorum_percentage
        &86400,   // execution_delay
        &50,      // emergency_quorum_percentage
    );
    
    // Test that initialization worked
    let treasury_info = client.get_treasury_balance();
    assert_eq!(treasury_info.total_balance, 0);
    assert_eq!(treasury_info.allocated_funds, 0);
}

#[test]
fn test_membership_joining() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, PuzzleDaoContract);
    let client = PuzzleDaoContractClient::new(&env, &contract_id);
    
    let token_id = env.register_contract(None, RewardToken);
    let treasury_address = Address::generate(&env);
    let member = Address::generate(&env);
    
    client.initialize(
        &token_id,
        &treasury_address,
        &100,
        &604800,
        &1000,
        &10,
        &86400,
        &50,
    );
    
    // Setup token
    let token_client = RewardTokenClient::new(&env, &token_id);
    let admin = Address::generate(&env);
    token_client.initialize(
        &admin,
        &String::from_str(&env, "Test Token"),
        &String::from_str(&env, "TEST"),
        &6,
    );
    token_client.mint(&admin, &member, &10000);
    
    // Member joins DAO
    client.join_dao(&member, &5000);
    
    // Check membership info
    let member_info = client.get_member_info(&member);
    assert_eq!(member_info.address, member);
    assert_eq!(member_info.tier, MembershipTier::Active);
    assert!(member_info.is_active);
    
    // Check voting power
    let voting_power = client.get_user_voting_power(&member);
    assert_eq!(voting_power, 5000);
}

#[test]
fn test_membership_upgrade() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, PuzzleDaoContract);
    let client = PuzzleDaoContractClient::new(&env, &contract_id);
    
    let token_id = env.register_contract(None, RewardToken);
    let treasury_address = Address::generate(&env);
    let member = Address::generate(&env);
    
    client.initialize(
        &token_id,
        &treasury_address,
        &100,
        &604800,
        &1000,
        &10,
        &86400,
        &50,
    );
    
    let token_client = RewardTokenClient::new(&env, &token_id);
    let admin = Address::generate(&env);
    token_client.initialize(
        &admin,
        &String::from_str(&env, "Test Token"),
        &String::from_str(&env, "TEST"),
        &6,
    );
    token_client.mint(&admin, &member, &30000);
    
    // Join as Basic member
    client.join_dao(&member, &3000);
    let member_info = client.get_member_info(&member);
    assert_eq!(member_info.tier, MembershipTier::Basic);
    
    // Upgrade to Premium
    client.upgrade_membership(&member, &17000);
    let member_info = client.get_member_info(&member);
    assert_eq!(member_info.tier, MembershipTier::Premium);
    
    // Check updated voting power
    let voting_power = client.get_user_voting_power(&member);
    assert_eq!(voting_power, 20000);
}

#[test]
fn test_proposal_creation() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, PuzzleDaoContract);
    let client = PuzzleDaoContractClient::new(&env, &contract_id);
    
    let token_id = env.register_contract(None, RewardToken);
    let treasury_address = Address::generate(&env);
    let proposer = Address::generate(&env);
    let target_contract = Address::generate(&env);
    
    client.initialize(
        &token_id,
        &treasury_address,
        &100,
        &604800,
        &1000,
        &10,
        &86400,
        &50,
    );
    
    let token_client = RewardTokenClient::new(&env, &token_id);
    let admin = Address::generate(&env);
    token_client.initialize(
        &admin,
        &String::from_str(&env, "Test Token"),
        &String::from_str(&env, "TEST"),
        &6,
    );
    token_client.mint(&admin, &proposer, &5000);
    client.join_dao(&proposer, &5000);
    
    // Create proposal action
    let action = crate::types::ProposalActionInput {
        contract_id: target_contract.clone(),
        function_name: Symbol::new(&env, "some_function"),
        args: Vec::new(&env),
    };
    
    // Create proposal
    let proposal_id = client.propose(
        &proposer,
        &String::from_str(&env, "Test Proposal"),
        &String::from_str(&env, "Test Description"),
        &Some(action),
        &(ProposalCategory::PuzzleCuration as u32),
    );
    
    assert!(proposal_id > 0);
    
    // Check proposal info
    let proposal = client.get_proposal_info(&proposal_id);
    assert_eq!(proposal.proposer, proposer);
    assert_eq!(proposal.category, ProposalCategory::PuzzleCuration as u32);
    assert_eq!(proposal.status, crate::types::ProposalStatus::Pending);
}

#[test]
fn test_voting_process() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, PuzzleDaoContract);
    let client = PuzzleDaoContractClient::new(&env, &contract_id);
    
    let token_id = env.register_contract(None, RewardToken);
    let treasury_address = Address::generate(&env);
    let proposer = Address::generate(&env);
    let voter1 = Address::generate(&env);
    let voter2 = Address::generate(&env);
    let target_contract = Address::generate(&env);
    
    client.initialize(
        &token_id,
        &treasury_address,
        &0,       // No voting delay for testing
        &604800,
        &1000,
        &10,
        &86400,
        &50,
    );
    
    let token_client = RewardTokenClient::new(&env, &token_id);
    let admin = Address::generate(&env);
    token_client.initialize(
        &admin,
        &String::from_str(&env, "Test Token"),
        &String::from_str(&env, "TEST"),
        &6,
    );
    token_client.mint(&admin, &proposer, &5000);
    token_client.mint(&admin, &voter1, &3000);
    token_client.mint(&admin, &voter2, &2000);
    
    client.join_dao(&proposer, &5000);
    client.join_dao(&voter1, &3000);
    client.join_dao(&voter2, &2000);
    
    // Create proposal
    let action = crate::types::ProposalActionInput {
        contract_id: target_contract.clone(),
        function_name: Symbol::new(&env, "some_function"),
        args: Vec::new(&env),
    };
    
    let proposal_id = client.propose(
        &proposer,
        &String::from_str(&env, "Test Proposal"),
        &String::from_str(&env, "Test Description"),
        &Some(action),
        &(ProposalCategory::PuzzleCuration as u32),
    );
    
    // Advance time to start voting
    env.ledger().with_mut(|li| {
        li.timestamp += 1;
    });
    
    // Vote
    client.vote(&voter1, &proposal_id, &VoteType::For);
    client.vote(&voter2, &proposal_id, &VoteType::Against);
    
    // Check proposal state
    let proposal = client.get_proposal_info(&proposal_id);
    assert_eq!(proposal.for_votes, 3000);
    assert_eq!(proposal.against_votes, 2000);
    assert_eq!(proposal.status, crate::types::ProposalStatus::Active);
}

#[test]
fn test_vote_delegation() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, PuzzleDaoContract);
    let client = PuzzleDaoContractClient::new(&env, &contract_id);
    
    let token_id = env.register_contract(None, RewardToken);
    let treasury_address = Address::generate(&env);
    let delegator = Address::generate(&env);
    let delegatee = Address::generate(&env);
    
    client.initialize(
        &token_id,
        &treasury_address,
        &100,
        &604800,
        &1000,
        &10,
        &86400,
        &50,
    );
    
    let token_client = RewardTokenClient::new(&env, &token_id);
    let admin = Address::generate(&env);
    token_client.initialize(
        &admin,
        &String::from_str(&env, "Test Token"),
        &String::from_str(&env, "TEST"),
        &6,
    );
    token_client.mint(&admin, &delegator, &5000);
    client.join_dao(&delegator, &5000);
    
    // Check initial voting power
    let initial_power = client.get_user_voting_power(&delegator);
    assert_eq!(initial_power, 5000);
    
    let delegatee_power = client.get_user_voting_power(&delegatee);
    assert_eq!(delegatee_power, 0);
    
    // Delegate voting power
    client.delegate(&delegator, &delegatee);
    
    // Check updated voting power
    let delegator_power = client.get_user_voting_power(&delegator);
    assert_eq!(delegator_power, 0);
    
    let delegatee_power = client.get_user_voting_power(&delegatee);
    assert_eq!(delegatee_power, 5000);
}

#[test]
fn test_emergency_proposal() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, PuzzleDaoContract);
    let client = PuzzleDaoContractClient::new(&env, &contract_id);
    
    let token_id = env.register_contract(None, RewardToken);
    let treasury_address = Address::generate(&env);
    let proposer = Address::generate(&env);
    let target_contract = Address::generate(&env);
    
    client.initialize(
        &token_id,
        &treasury_address,
        &0,       // No voting delay for emergency
        &3600,    // Shorter voting period for emergency
        &1000,
        &10,
        &86400,
        &50,      // Higher quorum for emergency
    );
    
    let token_client = RewardTokenClient::new(&env, &token_id);
    let admin = Address::generate(&env);
    token_client.initialize(
        &admin,
        &String::from_str(&env, "Test Token"),
        &String::from_str(&env, "TEST"),
        &6,
    );
    token_client.mint(&admin, &proposer, &1000);
    client.join_dao(&proposer, &1000);
    
    // Create emergency proposal (lower threshold)
    let action = crate::types::ProposalActionInput {
        contract_id: target_contract.clone(),
        function_name: Symbol::new(&env, "emergency_function"),
        args: Vec::new(&env),
    };
    
    let proposal_id = client.propose(
        &proposer,
        &String::from_str(&env, "Emergency Proposal"),
        &String::from_str(&env, "Emergency Description"),
        &Some(action),
        &(ProposalCategory::Emergency as u32),
    );
    
    let proposal = client.get_proposal_info(&proposal_id);
    assert_eq!(proposal.category, ProposalCategory::Emergency as u32);
}

#[test]
fn test_treasury_management() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, PuzzleDaoContract);
    let client = PuzzleDaoContractClient::new(&env, &contract_id);
    
    let token_id = env.register_contract(None, RewardToken);
    let treasury_address = Address::generate(&env);
    let _recipient = Address::generate(&env);
    
    client.initialize(
        &token_id,
        &treasury_address,
        &100,
        &604800,
        &1000,
        &10,
        &86400,
        &50,
    );
    
    let token_client = RewardTokenClient::new(&env, &token_id);
    let admin = Address::generate(&env);
    token_client.initialize(
        &admin,
        &String::from_str(&env, "Test Token"),
        &String::from_str(&env, "TEST"),
        &6,
    );
    
    // Fund treasury
    token_client.mint(&admin, &treasury_address, &10000);
    
    // This would normally be called through a governance proposal
    // For testing, we'll check the treasury info
    let treasury_info = client.get_treasury_balance();
    assert_eq!(treasury_info.total_balance, 0); // Not updated until funds are transferred to contract
}
