#![no_std]

mod storage;
pub mod types;

use soroban_sdk::{contract, contractimpl, Address, Env, String, Vec, Symbol, Val, Map};
use soroban_sdk::token::Client as TokenClient;
use crate::storage::*;
use crate::types::*;

#[contract]
pub struct PuzzleDaoContract;

#[contractimpl]
impl PuzzleDaoContract {
    /// Initialize the Puzzle DAO contract
    pub fn initialize(
        env: Env,
        token_address: Address,
        treasury_address: Address,
        voting_delay: u64,
        voting_period: u64,
        proposal_threshold: i128,
        quorum_percentage: u32,
        execution_delay: u64,
        emergency_quorum_percentage: u32,
    ) {
        if env.storage().instance().has(&DataKey::Config) {
            panic!("Already initialized");
        }
        
        if quorum_percentage > 100 || emergency_quorum_percentage > 100 {
            panic!("Invalid quorum percentage");
        }

        let config = GovernanceConfig {
            voting_delay,
            voting_period,
            proposal_threshold,
            quorum_percentage,
            token_address,
            treasury_address,
            execution_delay,
            emergency_quorum_percentage,
        };
        set_config(&env, &config);

        // Initialize treasury info
        let treasury_info = TreasuryInfo {
            total_balance: 0,
            allocated_funds: 0,
            last_distribution: 0,
        };
        set_treasury_info(&env, &treasury_info);
    }

    /// Join the DAO as a member
    pub fn join_dao(env: Env, member: Address, stake_amount: i128) {
        member.require_auth();
        
        if get_member(&env, &member).is_some() {
            panic!("Already a member");
        }

        if stake_amount <= 0 {
            panic!("Invalid stake amount");
        }

        let config = get_config(&env);
        let token = TokenClient::new(&env, &config.token_address);
        
        // Transfer stake tokens to this contract
        token.transfer(&member, &env.current_contract_address(), &stake_amount);

        // Determine membership tier based on stake
        let thresholds = get_membership_thresholds(&env);
        let tier = if stake_amount >= thresholds.get(MembershipTier::Council).unwrap() {
            MembershipTier::Council
        } else if stake_amount >= thresholds.get(MembershipTier::Premium).unwrap() {
            MembershipTier::Premium
        } else if stake_amount >= thresholds.get(MembershipTier::Active).unwrap() {
            MembershipTier::Active
        } else {
            MembershipTier::Basic
        };

        let new_member = Member {
            address: member.clone(),
            tier,
            joined_at: env.ledger().timestamp(),
            reputation: 0,
            is_active: true,
        };

        set_member(&env, &new_member);
        increment_member_count(&env);
        
        // Set token balance and voting power
        set_token_balance(&env, &member, stake_amount);
        set_voting_power(&env, &member, stake_amount);
    }

    /// Upgrade membership tier by staking additional tokens
    pub fn upgrade_membership(env: Env, member: Address, additional_stake: i128) {
        member.require_auth();
        
        if additional_stake <= 0 {
            panic!("Invalid stake amount");
        }

        let mut member_info = get_member(&env, &member).expect("Not a member");
        let current_balance = get_token_balance(&env, &member);
        
        let config = get_config(&env);
        let token = TokenClient::new(&env, &config.token_address);
        
        // Transfer additional tokens
        token.transfer(&member, &env.current_contract_address(), &additional_stake);
        
        let new_balance = current_balance + additional_stake;
        set_token_balance(&env, &member, new_balance);
        
        // Determine new tier
        let thresholds = get_membership_thresholds(&env);
        let new_tier = if new_balance >= thresholds.get(MembershipTier::Council).unwrap() {
            MembershipTier::Council
        } else if new_balance >= thresholds.get(MembershipTier::Premium).unwrap() {
            MembershipTier::Premium
        } else if new_balance >= thresholds.get(MembershipTier::Active).unwrap() {
            MembershipTier::Active
        } else {
            MembershipTier::Basic
        };
        
        member_info.tier = new_tier;
        set_member(&env, &member_info);
        
        // Update voting power
        set_voting_power(&env, &member, new_balance);
    }

    /// Leave the DAO and unstake tokens
    pub fn leave_dao(env: Env, member: Address) {
        member.require_auth();
        
        let _member_info = get_member(&env, &member).expect("Not a member");
        let balance = get_token_balance(&env, &member);
        
        // Remove voting power
        let delegatee = get_delegate(&env, &member).unwrap_or(member.clone());
        let current_power = get_voting_power(&env, &delegatee);
        set_voting_power(&env, &delegatee, current_power - balance);
        
        // Clear delegation
        env.storage().persistent().remove(&DataKey::Delegation(member.clone()));
        
        // Transfer tokens back
        let config = get_config(&env);
        let token = TokenClient::new(&env, &config.token_address);
        token.transfer(&env.current_contract_address(), &member, &balance);
        
        // Clear member data
        env.storage().persistent().remove(&DataKey::TokenBalance(member.clone()));
        env.storage().persistent().remove(&DataKey::VotingPower(member.clone()));
        env.storage().persistent().remove(&DataKey::Member(member.clone()));
    }

    /// Delegate voting power to another address
    pub fn delegate(env: Env, delegator: Address, delegatee: Address) {
        delegator.require_auth();

        let current_delegate = get_delegate(&env, &delegator).unwrap_or(delegator.clone());
        if current_delegate == delegatee {
            return;
        }

        let balance = get_token_balance(&env, &delegator);
        
        if balance > 0 {
            // Remove power from old delegate
            let old_power = get_voting_power(&env, &current_delegate);
            set_voting_power(&env, &current_delegate, old_power - balance);

            // Add power to new delegate
            let new_power = get_voting_power(&env, &delegatee);
            set_voting_power(&env, &delegatee, new_power + balance);
        }

        set_delegate(&env, &delegator, &delegatee);
    }

    /// Create a new proposal
    pub fn propose(
        env: Env,
        proposer: Address,
        title: String,
        description: String,
        action: Option<ProposalActionInput>,
        category: u32,
    ) -> u64 {
        proposer.require_auth();

        let member_info = get_member(&env, &proposer).expect("Not a member");
        if !member_info.is_active {
            panic!("Member is not active");
        }

        let config = get_config(&env);
        let voting_power = get_voting_power(&env, &proposer);

        // Check proposal threshold based on category
        let threshold = match category {
            5 => config.proposal_threshold / 2, // Emergency proposals have lower threshold
            _ => config.proposal_threshold,
        };

        if voting_power < threshold {
            panic!("Insufficient voting power to propose");
        }

        let id = increment_proposal_count(&env);
        let start_time = env.ledger().timestamp() + config.voting_delay;
        let end_time = start_time + config.voting_period;

        // Calculate quorum based on total supply and category
        let total_supply: i128 = env.invoke_contract(
            &config.token_address,
            &Symbol::new(&env, "total_supply"),
            Vec::new(&env),
        );

        let quorum_percentage = match category {
            5 => config.emergency_quorum_percentage, // Emergency proposals
            _ => config.quorum_percentage,
        };

        let quorum = (total_supply * quorum_percentage as i128) / 100;

        let (stored_action, args_to_store) = if let Some(input) = action {
            (
                ProposalAction {
                    contract_id: input.contract_id,
                    function_name: input.function_name,
                },
                Some(input.args),
            )
        } else {
            panic!("Action required");
        };

        if let Some(args) = args_to_store {
            set_proposal_args(&env, id, &args);
        }

        let proposal = Proposal {
            id,
            proposer,
            title,
            description,
            action: stored_action,
            start_time,
            end_time,
            for_votes: 0,
            against_votes: 0,
            abstain_votes: 0,
            status: ProposalStatus::Pending,
            quorum,
            category,
            created_at: env.ledger().timestamp(),
        };

        set_proposal(&env, &proposal);
        id
    }

    /// Vote on a proposal
    pub fn vote(env: Env, voter: Address, proposal_id: u64, vote_type: VoteType) {
        voter.require_auth();

        let member_info = get_member(&env, &voter).expect("Not a member");
        if !member_info.is_active {
            panic!("Member is not active");
        }

        let mut proposal = get_proposal(&env, proposal_id).expect("Proposal not found");
        let current_time = env.ledger().timestamp();

        if current_time < proposal.start_time {
            panic!("Voting has not started");
        }
        if current_time > proposal.end_time {
            panic!("Voting has ended");
        }
        if has_voted(&env, proposal_id, &voter) {
            panic!("Already voted");
        }

        let voting_power = get_voting_power(&env, &voter);
        if voting_power == 0 {
            panic!("No voting power");
        }

        match vote_type {
            VoteType::For => proposal.for_votes += voting_power,
            VoteType::Against => proposal.against_votes += voting_power,
            VoteType::Abstain => proposal.abstain_votes += voting_power,
        }

        // Update status to Active if it was Pending
        if proposal.status == ProposalStatus::Pending {
            proposal.status = ProposalStatus::Active;
        }

        set_proposal(&env, &proposal);
        set_voted(&env, proposal_id, &voter);
    }

    /// Execute a successful proposal
    pub fn execute(env: Env, proposal_id: u64) {
        let mut proposal = get_proposal(&env, proposal_id).expect("Proposal not found");
        let current_time = env.ledger().timestamp();

        if current_time <= proposal.end_time {
            panic!("Voting period not ended");
        }
        
        if proposal.status == ProposalStatus::Executed {
            panic!("Already executed");
        }
        
        if proposal.status == ProposalStatus::Canceled {
            panic!("Proposal canceled");
        }

        let total_votes = proposal.for_votes + proposal.against_votes + proposal.abstain_votes;
        
        // Check Quorum
        if total_votes < proposal.quorum {
            proposal.status = ProposalStatus::Defeated;
            set_proposal(&env, &proposal);
            panic!("Quorum not reached");
        }

        // Check Vote Outcome (Simple Majority)
        if proposal.for_votes <= proposal.against_votes {
            proposal.status = ProposalStatus::Defeated;
            set_proposal(&env, &proposal);
            panic!("Proposal defeated");
        }

        // For non-emergency proposals, add execution delay
        if proposal.category != 5 {
            let config = get_config(&env);
            if current_time < proposal.end_time + config.execution_delay {
                panic!("Execution delay not met");
            }
        }

        // Execute Action
        let action = &proposal.action;
        let args = get_proposal_args(&env, proposal_id).unwrap_or(Vec::new(&env));
        let _res: Val = env.invoke_contract(&action.contract_id, &action.function_name, args);

        proposal.status = ProposalStatus::Executed;
        set_proposal(&env, &proposal);
    }

    /// Cancel a proposal (only proposer can cancel, and only before voting starts)
    pub fn cancel(env: Env, proposer: Address, proposal_id: u64) {
        proposer.require_auth();
        let mut proposal = get_proposal(&env, proposal_id).expect("Proposal not found");

        if proposal.proposer != proposer {
            panic!("Not proposer");
        }

        if env.ledger().timestamp() >= proposal.start_time {
            panic!("Voting already started");
        }

        proposal.status = ProposalStatus::Canceled;
        set_proposal(&env, &proposal);
    }

    /// Treasury management functions
    
    /// Allocate funds from treasury
    pub fn allocate_treasury_funds(env: Env, amount: i128, recipient: Address) {
        let config = get_config(&env);
        let mut treasury_info = get_treasury_info(&env);
        
        if amount <= 0 {
            panic!("Invalid amount");
        }
        
        if treasury_info.allocated_funds + amount > treasury_info.total_balance {
            panic!("Insufficient treasury funds");
        }
        
        let token = TokenClient::new(&env, &config.token_address);
        token.transfer(&config.treasury_address, &recipient, &amount);
        
        treasury_info.allocated_funds += amount;
        set_treasury_info(&env, &treasury_info);
    }

    /// Update membership thresholds (requires governance proposal)
    pub fn update_membership_thresholds(env: Env, thresholds: Map<MembershipTier, i128>) {
        // This should only be callable through a successful governance proposal
        set_membership_thresholds(&env, &thresholds);
    }
    
    // Read-only helpers
    pub fn get_proposal_info(env: Env, proposal_id: u64) -> Proposal {
        get_proposal(&env, proposal_id).expect("Proposal not found")
    }

    pub fn get_user_voting_power(env: Env, user: Address) -> i128 {
        get_voting_power(&env, &user)
    }
    
    pub fn get_user_deposited_balance(env: Env, user: Address) -> i128 {
        get_token_balance(&env, &user)
    }

    pub fn get_member_info(env: Env, member: Address) -> Member {
        get_member(&env, &member).expect("Member not found")
    }

    pub fn get_treasury_balance(env: Env) -> TreasuryInfo {
        get_treasury_info(&env)
    }

    pub fn get_membership_requirements(env: Env) -> Map<MembershipTier, i128> {
        get_membership_thresholds(&env)
    }
}

#[cfg(test)]
mod test;
