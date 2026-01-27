#![no_std]

mod storage;
pub mod types;

use soroban_sdk::{contract, contractimpl, Address, Env, String, Vec, Symbol, Val};
use soroban_sdk::token::Client as TokenClient;
use crate::storage::*;
use crate::types::*;

#[contract]
pub struct GovernanceContract;

#[contractimpl]
impl GovernanceContract {
    /// Initialize the governance contract
    pub fn initialize(
        env: Env,
        token_address: Address,
        voting_delay: u64,
        voting_period: u64,
        proposal_threshold: i128,
        quorum_percentage: u32,
    ) {
        if env.storage().instance().has(&DataKey::Config) {
            panic!("Already initialized");
        }
        
        if quorum_percentage > 100 {
            panic!("Invalid quorum percentage");
        }

        let config = GovernanceConfig {
            voting_delay,
            voting_period,
            proposal_threshold,
            quorum_percentage,
            token_address,
        };
        set_config(&env, &config);
    }

    /// Deposit tokens to gain voting power
    pub fn deposit(env: Env, from: Address, amount: i128) {
        from.require_auth();
        if amount <= 0 {
            panic!("Invalid amount");
        }

        let config = get_config(&env);
        let token = TokenClient::new(&env, &config.token_address);
        
        // Transfer tokens to this contract
        token.transfer(&from, &env.current_contract_address(), &amount);

        // Update balance
        let current_balance = get_token_balance(&env, &from);
        set_token_balance(&env, &from, current_balance + amount);

        // Update voting power
        let delegatee = get_delegate(&env, &from).unwrap_or(from.clone());
        let current_power = get_voting_power(&env, &delegatee);
        set_voting_power(&env, &delegatee, current_power + amount);
    }

    /// Withdraw tokens and lose voting power
    pub fn withdraw(env: Env, to: Address, amount: i128) {
        to.require_auth();
        if amount <= 0 {
            panic!("Invalid amount");
        }

        let current_balance = get_token_balance(&env, &to);
        if current_balance < amount {
            panic!("Insufficient balance");
        }

        // Update balance
        set_token_balance(&env, &to, current_balance - amount);

        // Update voting power
        let delegatee = get_delegate(&env, &to).unwrap_or(to.clone());
        let current_power = get_voting_power(&env, &delegatee);
        set_voting_power(&env, &delegatee, current_power - amount);

        // Transfer tokens back
        let config = get_config(&env);
        let token = TokenClient::new(&env, &config.token_address);
        token.transfer(&env.current_contract_address(), &to, &amount);
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

        let config = get_config(&env);
        let voting_power = get_voting_power(&env, &proposer);

        if voting_power < config.proposal_threshold {
            panic!("Insufficient voting power to propose");
        }

        let id = increment_proposal_count(&env);
        let start_time = env.ledger().timestamp() + config.voting_delay;
        let end_time = start_time + config.voting_period;

        // Calculate quorum based on total supply at creation time
        // We use invoke_contract to call `total_supply` on the token contract
        let total_supply: i128 = env.invoke_contract(
            &config.token_address,
            &Symbol::new(&env, "total_supply"),
            Vec::new(&env),
        );

        let quorum = (total_supply * config.quorum_percentage as i128) / 100;

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
        };

        set_proposal(&env, &proposal);
        id
    }

    /// Vote on a proposal
    pub fn vote(env: Env, voter: Address, proposal_id: u64, vote_type: VoteType) {
        voter.require_auth();

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
}

#[cfg(test)]
mod test;
