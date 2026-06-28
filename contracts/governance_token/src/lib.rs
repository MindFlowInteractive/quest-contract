#![no_std]

mod storage;
pub mod types;

use soroban_sdk::{contract, contractimpl, Address, Env, String, Symbol, Val, Vec};
use crate::storage::*;
use crate::types::*;

#[contract]
pub struct GovernanceToken;

#[contractimpl]
impl GovernanceToken {
    // ═══════════════════════════════════════════
    //  Initialization
    // ═══════════════════════════════════════════

    /// One‑time contract setup.
    pub fn initialize(
        env: Env,
        admin: Address,
        name: String,
        symbol: String,
        decimals: u32,
        voting_delay: u64,
        voting_period: u64,
        proposal_threshold: i128,
        quorum_numerator: u32,
        timelock_delay: u64,
        grace_period: u64,
    ) {
        if has_admin(&env) {
            panic!("Already initialized");
        }
        if quorum_numerator > 100 {
            panic!("Invalid quorum numerator");
        }

        set_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Name, &name);
        env.storage().instance().set(&DataKey::Symbol, &symbol);
        env.storage().instance().set(&DataKey::Decimals, &decimals);
        set_total_supply(&env, 0);

        let config = GovernanceConfig {
            voting_delay,
            voting_period,
            proposal_threshold,
            quorum_numerator,
            timelock_delay,
            grace_period,
        };
        set_config(&env, &config);
    }

    // ═══════════════════════════════════════════
    //  ERC‑20‑like Token Interface
    // ═══════════════════════════════════════════

    /// Mint new tokens (admin only). Voting power is updated for the
    /// recipient's delegate (or the recipient if self‑delegated / not yet
    /// delegated).
    pub fn mint(env: Env, admin: Address, to: Address, amount: i128) {
        admin.require_auth();
        if admin != get_admin(&env) {
            panic!("Not admin");
        }
        if amount <= 0 {
            panic!("Amount must be positive");
        }

        // Update balance & supply
        let new_balance = get_balance(&env, &to) + amount;
        set_balance(&env, &to, new_balance);
        set_total_supply(&env, get_total_supply(&env) + amount);

        // Update voting power for the delegate
        let delegatee = get_delegate(&env, &to).unwrap_or(to.clone());
        Self::_move_voting_power(&env, None, Some(&delegatee), amount);
    }

    /// Burn tokens. Caller must be the token holder.
    pub fn burn(env: Env, from: Address, amount: i128) {
        from.require_auth();
        if amount <= 0 {
            panic!("Amount must be positive");
        }

        let balance = get_balance(&env, &from);
        if balance < amount {
            panic!("Insufficient balance");
        }

        set_balance(&env, &from, balance - amount);
        set_total_supply(&env, get_total_supply(&env) - amount);

        let delegatee = get_delegate(&env, &from).unwrap_or(from.clone());
        Self::_move_voting_power(&env, Some(&delegatee), None, amount);
    }

    /// Transfer tokens between accounts.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        if amount <= 0 {
            panic!("Amount must be positive");
        }

        let from_balance = get_balance(&env, &from);
        if from_balance < amount {
            panic!("Insufficient balance");
        }

        set_balance(&env, &from, from_balance - amount);
        set_balance(&env, &to, get_balance(&env, &to) + amount);

        // Move voting power between delegates
        let from_delegate = get_delegate(&env, &from).unwrap_or(from.clone());
        let to_delegate = get_delegate(&env, &to).unwrap_or(to.clone());
        Self::_move_voting_power(&env, Some(&from_delegate), Some(&to_delegate), amount);
    }

    /// Approve a spender.
    pub fn approve(env: Env, owner: Address, spender: Address, amount: i128) {
        owner.require_auth();
        if amount < 0 {
            panic!("Amount cannot be negative");
        }
        set_allowance(&env, &owner, &spender, amount);
    }

    /// Transfer using an allowance.
    pub fn transfer_from(
        env: Env,
        spender: Address,
        from: Address,
        to: Address,
        amount: i128,
    ) {
        spender.require_auth();
        if amount <= 0 {
            panic!("Amount must be positive");
        }

        let allowed = get_allowance(&env, &from, &spender);
        if allowed < amount {
            panic!("Insufficient allowance");
        }

        let from_balance = get_balance(&env, &from);
        if from_balance < amount {
            panic!("Insufficient balance");
        }

        set_balance(&env, &from, from_balance - amount);
        set_balance(&env, &to, get_balance(&env, &to) + amount);
        set_allowance(&env, &from, &spender, allowed - amount);

        let from_delegate = get_delegate(&env, &from).unwrap_or(from.clone());
        let to_delegate = get_delegate(&env, &to).unwrap_or(to.clone());
        Self::_move_voting_power(&env, Some(&from_delegate), Some(&to_delegate), amount);
    }

    // ── Read‑only token helpers ──

    pub fn balance(env: Env, account: Address) -> i128 {
        get_balance(&env, &account)
    }

    pub fn total_supply(env: Env) -> i128 {
        get_total_supply(&env)
    }

    pub fn allowance(env: Env, owner: Address, spender: Address) -> i128 {
        get_allowance(&env, &owner, &spender)
    }

    pub fn name(env: Env) -> String {
        env.storage()
            .instance()
            .get(&DataKey::Name)
            .unwrap_or(String::from_str(&env, "Governance Token"))
    }

    pub fn symbol(env: Env) -> String {
        env.storage()
            .instance()
            .get(&DataKey::Symbol)
            .unwrap_or(String::from_str(&env, "GOV"))
    }

    pub fn decimals(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::Decimals)
            .unwrap_or(6)
    }

    // ═══════════════════════════════════════════
    //  Delegation & Voting Power
    // ═══════════════════════════════════════════

    /// Delegate voting power. Pass `delegatee == delegator` to self‑delegate.
    pub fn delegate(env: Env, delegator: Address, delegatee: Address) {
        delegator.require_auth();

        let current = get_delegate(&env, &delegator).unwrap_or(delegator.clone());
        if current == delegatee {
            return; // no‑op
        }

        let balance = get_balance(&env, &delegator);

        if balance > 0 {
            // Remove power from old delegate, add to new
            Self::_move_voting_power(&env, Some(&current), Some(&delegatee), balance);
        }

        set_delegate(&env, &delegator, &delegatee);
    }

    /// Current voting power for an account.
    pub fn get_voting_power(env: Env, account: Address) -> i128 {
        get_voting_power(&env, &account)
    }

    /// Historical voting power at a past ledger sequence. Uses binary search
    /// over checkpoints.
    pub fn get_past_votes(env: Env, account: Address, sequence: u32) -> i128 {
        let current_seq = env.ledger().sequence();
        if sequence >= current_seq {
            panic!("Sequence not yet finalized");
        }
        let ckpts = get_checkpoints(&env, &account);
        Self::_find_past_votes(&ckpts, sequence)
    }

    /// Return who an account has delegated to (defaults to self).
    pub fn get_delegate(env: Env, account: Address) -> Address {
        get_delegate(&env, &account).unwrap_or(account)
    }

    /// Return the number of checkpoints for an account.
    pub fn get_num_checkpoints(env: Env, account: Address) -> u32 {
        get_num_checkpoints(&env, &account)
    }

    // ═══════════════════════════════════════════
    //  Proposals
    // ═══════════════════════════════════════════

    /// Create a new proposal. Returns the proposal id.
    pub fn propose(
        env: Env,
        proposer: Address,
        title: String,
        description: String,
        action: ProposalActionInput,
        category: u32,
    ) -> u64 {
        proposer.require_auth();

        let config = get_config(&env);
        let power = get_voting_power(&env, &proposer);
        if power < config.proposal_threshold {
            panic!("Insufficient voting power to propose");
        }

        let id = increment_proposal_count(&env);
        let start_time = env.ledger().timestamp() + config.voting_delay;
        let end_time = start_time + config.voting_period;
        let snapshot_sequence = env.ledger().sequence();

        // Quorum = total_supply * quorum_numerator / 100
        let total = get_total_supply(&env);
        let quorum = (total * config.quorum_numerator as i128) / 100;

        // Store action args separately (Vec<Val> can be large)
        set_proposal_args(&env, id, &action.args);

        let proposal = Proposal {
            id,
            proposer,
            title,
            description,
            action: ProposalAction {
                contract_id: action.contract_id,
                function_name: action.function_name,
            },
            start_time,
            end_time,
            snapshot_sequence,
            for_votes: 0,
            against_votes: 0,
            abstain_votes: 0,
            status: ProposalStatus::Pending,
            quorum,
            eta: 0,
            category,
        };

        set_proposal(&env, &proposal);
        id
    }

    /// Cast a vote on a proposal. Voting weight is read from the snapshot
    /// taken at proposal creation.
    pub fn vote(env: Env, voter: Address, proposal_id: u64, vote_type: VoteType) {
        voter.require_auth();

        let mut proposal = get_proposal(&env, proposal_id).expect("Proposal not found");
        let now = env.ledger().timestamp();

        if now < proposal.start_time {
            panic!("Voting has not started");
        }
        if now > proposal.end_time {
            panic!("Voting has ended");
        }
        if has_voted(&env, proposal_id, &voter) {
            panic!("Already voted");
        }

        // Use historical voting power at snapshot
        let ckpts = get_checkpoints(&env, &voter);
        let weight = Self::_find_past_votes(&ckpts, proposal.snapshot_sequence);
        if weight == 0 {
            // Fallback: if no checkpoint exists yet (voter delegated before
            // any checkpoint was written), use current power
            let current = get_voting_power(&env, &voter);
            if current == 0 {
                panic!("No voting power");
            }
            // Use current power as weight
            Self::_apply_vote(&env, &mut proposal, &voter, proposal_id, &vote_type, current);
        } else {
            Self::_apply_vote(&env, &mut proposal, &voter, proposal_id, &vote_type, weight);
        }
    }

    /// Queue a successful proposal for timelocked execution.
    pub fn queue(env: Env, proposal_id: u64) {
        let mut proposal = get_proposal(&env, proposal_id).expect("Proposal not found");
        let now = env.ledger().timestamp();

        if now <= proposal.end_time {
            panic!("Voting period not ended");
        }

        // Must be Pending/Active — first resolve final status
        if proposal.status != ProposalStatus::Pending
            && proposal.status != ProposalStatus::Active
        {
            panic!("Proposal not in voteable state");
        }

        let total_votes = proposal.for_votes + proposal.against_votes + proposal.abstain_votes;

        if total_votes < proposal.quorum {
            proposal.status = ProposalStatus::Defeated;
            set_proposal(&env, &proposal);
            panic!("Quorum not reached");
        }

        if proposal.for_votes <= proposal.against_votes {
            proposal.status = ProposalStatus::Defeated;
            set_proposal(&env, &proposal);
            panic!("Proposal defeated");
        }

        let config = get_config(&env);
        proposal.eta = now + config.timelock_delay;
        proposal.status = ProposalStatus::Queued;
        set_proposal(&env, &proposal);
    }

    /// Execute a queued proposal after the timelock has elapsed.
    pub fn execute(env: Env, proposal_id: u64) {
        let mut proposal = get_proposal(&env, proposal_id).expect("Proposal not found");
        let now = env.ledger().timestamp();

        if proposal.status != ProposalStatus::Queued {
            panic!("Proposal not queued");
        }

        if now < proposal.eta {
            panic!("Timelock not elapsed");
        }

        let config = get_config(&env);
        if now > proposal.eta + config.grace_period {
            proposal.status = ProposalStatus::Expired;
            set_proposal(&env, &proposal);
            panic!("Grace period expired");
        }

        // Execute action
        let args = get_proposal_args(&env, proposal_id).unwrap_or(Vec::new(&env));
        let _res: Val = env.invoke_contract(
            &proposal.action.contract_id,
            &proposal.action.function_name,
            args,
        );

        proposal.status = ProposalStatus::Executed;
        set_proposal(&env, &proposal);
    }

    /// Cancel a proposal (only proposer, only before voting starts).
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

    // ── Read‑only proposal helpers ──

    pub fn get_proposal_info(env: Env, proposal_id: u64) -> Proposal {
        get_proposal(&env, proposal_id).expect("Proposal not found")
    }

    pub fn get_vote_receipt(env: Env, proposal_id: u64, voter: Address) -> VoteReceipt {
        get_vote_receipt(&env, proposal_id, &voter).expect("No vote receipt")
    }

    pub fn get_config_info(env: Env) -> GovernanceConfig {
        get_config(&env)
    }

    // ═══════════════════════════════════════════
    //  Internal helpers
    // ═══════════════════════════════════════════

    /// Move `amount` of voting power from `src` to `dst`. Either can be
    /// `None` for mint (dst only) / burn (src only) scenarios.
    fn _move_voting_power(
        env: &Env,
        src: Option<&Address>,
        dst: Option<&Address>,
        amount: i128,
    ) {
        if amount == 0 {
            return;
        }

        if let Some(s) = src {
            let old = get_voting_power(env, s);
            let new_power = old - amount;
            set_voting_power(env, s, new_power);
            Self::_write_checkpoint(env, s, new_power);
        }

        if let Some(d) = dst {
            let old = get_voting_power(env, d);
            let new_power = old + amount;
            set_voting_power(env, d, new_power);
            Self::_write_checkpoint(env, d, new_power);
        }
    }

    /// Append or update a checkpoint for `account` with `new_votes` at the
    /// current ledger sequence.
    fn _write_checkpoint(env: &Env, account: &Address, new_votes: i128) {
        let mut ckpts = get_checkpoints(env, account);
        let seq = env.ledger().sequence();
        let num = get_num_checkpoints(env, account);

        if num > 0 {
            let last_idx = ckpts.len() - 1;
            let last = ckpts.get(last_idx).unwrap();
            if last.sequence == seq {
                // Same block — overwrite
                ckpts.set(last_idx, Checkpoint {
                    sequence: seq,
                    votes: new_votes,
                });
                set_checkpoints(env, account, &ckpts);
                return;
            }
        }

        // Append new checkpoint
        ckpts.push_back(Checkpoint {
            sequence: seq,
            votes: new_votes,
        });
        set_checkpoints(env, account, &ckpts);
        set_num_checkpoints(env, account, num + 1);
    }

    /// Binary search through checkpoints to find the voting power at or
    /// before `sequence`. Returns 0 if no checkpoint exists before the
    /// given sequence.
    fn _find_past_votes(ckpts: &Vec<Checkpoint>, sequence: u32) -> i128 {
        let len = ckpts.len();
        if len == 0 {
            return 0;
        }

        // Fast path: latest checkpoint is at or before the query
        let last = ckpts.get(len - 1).unwrap();
        if last.sequence <= sequence {
            return last.votes;
        }

        // Fast path: first checkpoint is after the query
        let first = ckpts.get(0).unwrap();
        if first.sequence > sequence {
            return 0;
        }

        // Binary search
        let mut low: u32 = 0;
        let mut high: u32 = len - 1;
        while low < high {
            let mid = low + (high - low + 1) / 2;
            let cp = ckpts.get(mid).unwrap();
            if cp.sequence <= sequence {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        ckpts.get(low).unwrap().votes
    }

    /// Apply a vote to a proposal and persist.
    fn _apply_vote(
        env: &Env,
        proposal: &mut Proposal,
        voter: &Address,
        proposal_id: u64,
        vote_type: &VoteType,
        weight: i128,
    ) {
        match vote_type {
            VoteType::For => proposal.for_votes += weight,
            VoteType::Against => proposal.against_votes += weight,
            VoteType::Abstain => proposal.abstain_votes += weight,
        }

        if proposal.status == ProposalStatus::Pending {
            proposal.status = ProposalStatus::Active;
        }

        set_proposal(env, proposal);
        set_has_voted(env, proposal_id, voter);
        set_vote_receipt(
            env,
            proposal_id,
            voter,
            &VoteReceipt {
                vote_type: vote_type.clone(),
                weight,
            },
        );
    }
}

#[cfg(test)]
mod test;
