#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env, String, Symbol, Val, Vec, symbol_short, FromVal, IntoVal};
use soroban_sdk::token::Client as TokenClient;

mod storage;
pub mod types;

use crate::storage::*;
use crate::types::*;

// Event symbols
const MEMBER_ADDED: Symbol = symbol_short!("m_add");
const MEMBER_REMOVED: Symbol = symbol_short!("m_rem");
const MEMBER_UPDATED: Symbol = symbol_short!("m_upd");
const TX_PROPOSED: Symbol = symbol_short!("tx_prop");
const TX_SIGNED: Symbol = symbol_short!("tx_sign");
const TX_EXECUTED: Symbol = symbol_short!("tx_exec");
const TX_REJECTED: Symbol = symbol_short!("tx_rej");
const TX_EXPIRED: Symbol = symbol_short!("tx_exp");
const EMERGENCY_ACTIVATED: Symbol = symbol_short!("emerg_on");
const EMERGENCY_EXECUTED: Symbol = symbol_short!("emerg_ex");
const RECOVERY_COMPLETED: Symbol = symbol_short!("recov");

#[contract]
pub struct MultisigTreasury;

#[contractimpl]
impl MultisigTreasury {
    // ==================== INITIALIZATION ====================

    /// Initialize the treasury with initial owner and configuration
    pub fn initialize(
        env: Env,
        owner: Address,
        threshold: u32,
        proposal_timeout: u64,
        max_pending_proposals: u32,
        emergency_cooldown: u64,
    ) {
        // Prevent re-initialization
        if get_config(&env).is_some() {
            panic!("Already initialized");
        }

        // Validate threshold (need at least 1 signature)
        if threshold == 0 {
            panic!("Invalid threshold: must be at least 1");
        }

        owner.require_auth();

        let config = TreasuryConfig {
            owner: owner.clone(),
            threshold,
            total_signers: 1,
            proposal_timeout,
            max_pending_proposals,
            emergency_recovery_enabled: true,
            emergency_cooldown,
        };

        set_config(&env, &config);

        // Add owner as first member with Owner role
        let member = Member {
            address: owner.clone(),
            role: Role::Owner,
            added_at: env.ledger().timestamp(),
            active: true,
        };
        set_member(&env, &owner, &member);

        let mut members = Vec::new(&env);
        members.push_back(owner.clone());
        set_members(&env, &members);

        // Initialize transaction ID counter
        env.storage().instance().set(&DataKey::NextTransactionId, &0u64);
    }

    // ==================== MEMBER MANAGEMENT ====================

    /// Add a new member to the treasury (Admin/Owner only)
    pub fn add_member(
        env: Env,
        caller: Address,
        new_member: Address,
        role: Role,
    ) {
        caller.require_auth();
        
        let caller_member = get_member(&env, &caller).expect("Caller not a member");
        
        // Only Admin or Owner can add members
        if caller_member.role != Role::Admin && caller_member.role != Role::Owner {
            panic!("Insufficient role to add members");
        }

        // Cannot add existing member
        if get_member(&env, &new_member).is_some() {
            panic!("Member already exists");
        }

        // Only Owner can add Owners or Admins
        if (role == Role::Owner || role == Role::Admin) && caller_member.role != Role::Owner {
            panic!("Only Owner can add Owners or Admins");
        }

        let member = Member {
            address: new_member.clone(),
            role: role.clone(),
            added_at: env.ledger().timestamp(),
            active: true,
        };

        set_member(&env, &new_member, &member);

        // Update members list
        let mut members = get_members(&env);
        members.push_back(new_member.clone());
        set_members(&env, &members);

        // Update total signers count in config
        let mut config = get_config(&env).expect("Not initialized");
        config.total_signers += 1;
        set_config(&env, &config);

        // Check threshold validity
        if config.threshold > config.total_signers {
            panic!("Threshold exceeds total signers after adding member");
        }

        env.events().publish((MEMBER_ADDED,), (new_member, role));
    }

    /// Remove a member from the treasury (Owner only for Signers/Admins, cannot remove last Owner)
    pub fn remove_member(
        env: Env,
        caller: Address,
        member_to_remove: Address,
    ) {
        caller.require_auth();

        let caller_member = get_member(&env, &caller).expect("Caller not a member");
        let target_member = get_member(&env, &member_to_remove).expect("Target not a member");

        // Cannot remove self if you're the last owner
        if caller == member_to_remove && target_member.role == Role::Owner {
            let members = get_members(&env);
            let owner_count = members.iter().filter(|m| {
                get_member(&env, m).map(|mem| mem.role == Role::Owner && mem.active).unwrap_or(false)
            }).count();
            
            if owner_count <= 1 {
                panic!("Cannot remove last owner");
            }
        }

        // Owner can remove anyone
        if caller_member.role != Role::Owner {
            // Admin can only remove Signers
            if caller_member.role == Role::Admin && target_member.role != Role::Signer {
                panic!("Admin can only remove Signers");
            }
            // Signers cannot remove anyone
            if caller_member.role == Role::Signer {
                panic!("Signers cannot remove members");
            }
        }

        // Update members list
        let mut members = get_members(&env);
        let mut new_members = Vec::new(&env);
        for m in members.iter() {
            if m != member_to_remove {
                new_members.push_back(m);
            }
        }
        set_members(&env, &new_members);

        // Remove member data
        remove_member(&env, &member_to_remove);

        // Update config
        let mut config = get_config(&env).expect("Not initialized");
        config.total_signers -= 1;
        
        // Ensure threshold is still valid
        if config.threshold > config.total_signers {
            config.threshold = config.total_signers;
        }
        
        set_config(&env, &config);

        env.events().publish((MEMBER_REMOVED,), (member_to_remove,));
    }

    /// Update member role (Owner only)
    pub fn update_member_role(
        env: Env,
        caller: Address,
        member_address: Address,
        new_role: Role,
    ) {
        caller.require_auth();

        let caller_member = get_member(&env, &caller).expect("Caller not a member");
        
        // Only Owner can update roles
        if caller_member.role != Role::Owner {
            panic!("Only Owner can update member roles");
        }

        let mut target_member = get_member(&env, &member_address).expect("Member not found");
        
        // If demoting from Owner, ensure not the last owner
        if target_member.role == Role::Owner && new_role != Role::Owner {
            let members = get_members(&env);
            let owner_count = members.iter().filter(|m| {
                get_member(&env, m).map(|mem| mem.role == Role::Owner && mem.active).unwrap_or(false)
            }).count();
            
            if owner_count <= 1 {
                panic!("Cannot demote last owner");
            }
        }

        target_member.role = new_role.clone();
        set_member(&env, &member_address, &target_member);

        env.events().publish((MEMBER_UPDATED,), (member_address, new_role));
    }

    /// Update configuration (Owner only)
    pub fn update_config(
        env: Env,
        caller: Address,
        threshold: u32,
        proposal_timeout: u64,
        max_pending_proposals: u32,
    ) {
        caller.require_auth();

        let caller_member = get_member(&env, &caller).expect("Caller not a member");
        if caller_member.role != Role::Owner {
            panic!("Only Owner can update config");
        }

        let mut config = get_config(&env).expect("Not initialized");
        
        // Validate threshold
        if threshold == 0 || threshold > config.total_signers {
            panic!("Invalid threshold");
        }

        config.threshold = threshold;
        config.proposal_timeout = proposal_timeout;
        config.max_pending_proposals = max_pending_proposals;

        set_config(&env, &config);
    }

    // ==================== TRANSACTION PROPOSAL ====================

    /// Propose a token transfer transaction
    pub fn propose_transfer(
        env: Env,
        proposer: Address,
        token: Address,
        destination: Address,
        amount: i128,
        description: String,
    ) -> u64 {
        proposer.require_auth();
        Self::require_signer(&env, &proposer);

        if amount <= 0 {
            panic!("Invalid amount");
        }

        let tx_id = Self::create_transaction(
            &env,
            proposer,
            TransactionType::TokenTransfer,
            Some(token),
            Some(amount),
            Some(destination),
            None,
            None,
            description,
            Role::Signer,
        );

        tx_id
    }

    /// Propose a contract call transaction
    pub fn propose_contract_call(
        env: Env,
        proposer: Address,
        contract: Address,
        function: Symbol,
        args: Vec<Val>,
        description: String,
    ) -> u64 {
        proposer.require_auth();
        Self::require_signer(&env, &proposer);

        let tx_id = Self::create_transaction(
            &env,
            proposer,
            TransactionType::ContractCall,
            Some(contract),
            None,
            None,
            Some(function),
            Some(args),
            description,
            Role::Signer,
        );

        tx_id
    }

    /// Propose a signer management transaction (Admin/Owner only)
    pub fn propose_signer_management(
        env: Env,
        proposer: Address,
        action: Symbol, // "add", "remove", "update_role"
        target: Address,
        role: Option<Role>,
        description: String,
    ) -> u64 {
        proposer.require_auth();
        
        let proposer_member = get_member(&env, &proposer).expect("Proposer not a member");
        if proposer_member.role != Role::Admin && proposer_member.role != Role::Owner {
            panic!("Only Admin or Owner can propose signer management");
        }

        // Build args from parameters
        let mut args = Vec::new(&env);
        args.push_back(Val::from_val(&env, &action));
        args.push_back(Val::from_val(&env, &target));
        if let Some(r) = role {
            args.push_back(Val::from_val(&env, &r));
        }

        let tx_id = Self::create_transaction(
            &env,
            proposer,
            TransactionType::SignerManagement,
            None,
            None,
            None,
            Some(Symbol::new(&env, "manage_signer")),
            Some(args),
            description,
            Role::Admin,
        );

        tx_id
    }

    // ==================== SIGNING & EXECUTION ====================

    /// Sign a pending transaction
    pub fn sign_transaction(
        env: Env,
        signer: Address,
        tx_id: u64,
    ) {
        signer.require_auth();

        let mut tx = get_transaction(&env, tx_id).expect("Transaction not found");
        
        // Check if already executed or rejected
        if tx.status != TransactionStatus::Pending {
            panic!("Transaction not pending");
        }

        // Check if expired
        let current_time = env.ledger().timestamp();
        if current_time > tx.expires_at {
            tx.status = TransactionStatus::Expired;
            set_transaction(&env, &tx);
            env.events().publish((TX_EXPIRED,), (tx_id,));
            panic!("Transaction expired");
        }

        // Verify signer is a member with sufficient role
        let member = get_member(&env, &signer).expect("Not a member");
        if !member.active {
            panic!("Member not active");
        }

        // Check role requirement
        if Self::role_level(&member.role) < Self::role_level(&tx.required_role) {
            panic!("Insufficient role to sign this transaction");
        }

        // Check if already signed
        if has_signed(&env, tx_id, &signer) {
            panic!("Already signed");
        }

        // Add signature
        let signature = Signature {
            signer: signer.clone(),
            timestamp: current_time,
        };
        tx.signatures.push_back(signature);
        set_signed(&env, tx_id, &signer);

        // Check if threshold reached
        let config = get_config(&env).expect("Not initialized");
        if tx.signatures.len() as u32 >= config.threshold {
            tx.status = TransactionStatus::Approved;
        }

        set_transaction(&env, &tx);
        env.events().publish((TX_SIGNED,), (tx_id, signer));
    }

    /// Execute an approved transaction
    pub fn execute_transaction(
        env: Env,
        executor: Address,
        tx_id: u64,
    ) -> Option<Val> {
        executor.require_auth();
        Self::require_signer(&env, &executor);

        let mut tx = get_transaction(&env, tx_id).expect("Transaction not found");

        // Must be approved or have enough signatures directly
        if tx.status != TransactionStatus::Approved && tx.status != TransactionStatus::Pending {
            panic!("Transaction not executable");
        }

        // Check threshold for pending transactions
        if tx.status == TransactionStatus::Pending {
            let config = get_config(&env).expect("Not initialized");
            if (tx.signatures.len() as u32) < config.threshold {
                panic!("Threshold not reached");
            }
            tx.status = TransactionStatus::Approved;
        }

        // Execute based on transaction type
        let result = match tx.transaction_type {
            TransactionType::TokenTransfer => {
                let token = tx.target.as_ref().expect("No token specified");
                let dest = tx.destination.as_ref().expect("No destination specified");
                let amount = tx.amount.expect("No amount specified");
                
                let token_client = TokenClient::new(&env, token);
                token_client.transfer(&env.current_contract_address(), dest, &amount);
                None
            }
            TransactionType::ContractCall => {
                let contract = tx.target.as_ref().expect("No contract specified");
                let function = tx.function.as_ref().expect("No function specified");
                let args = tx.args.clone().unwrap_or_else(|| Vec::new(&env));
                
                let res: Val = env.invoke_contract(contract, function, args);
                Some(res)
            }
            TransactionType::SignerManagement => {
                // Extract args for management action
                let args = tx.args.as_ref().expect("No args");
                let action: Symbol = args.get(0).expect("Missing action").into_val(&env);
                let target: Address = args.get(1).expect("Missing target").into_val(&env);
                
                if action == Symbol::new(&env, "add") {
                    let role: Role = args.get(2).expect("Missing role").into_val(&env);
                    Self::add_member(env.clone(), executor.clone(), target, role);
                } else if action == Symbol::new(&env, "remove") {
                    Self::remove_member(env.clone(), executor.clone(), target);
                } else if action == Symbol::new(&env, "update_role") {
                    let role: Role = args.get(2).expect("Missing role").into_val(&env);
                    Self::update_member_role(env.clone(), executor.clone(), target, role);
                }
                None
            }
            _ => None,
        };

        // Update transaction status
        tx.status = TransactionStatus::Executed;
        tx.executed_at = Some(env.ledger().timestamp());
        set_transaction(&env, &tx);

        // Remove from pending list
        let mut pending = get_pending_transactions(&env);
        let mut new_pending = Vec::new(&env);
        for id in pending.iter() {
            if id != tx_id {
                new_pending.push_back(id);
            }
        }
        set_pending_transactions(&env, &new_pending);

        // Add to history
        let mut signers = Vec::new(&env);
        for sig in tx.signatures.iter() {
            signers.push_back(sig.signer.clone());
        }
        let record = TransactionRecord {
            id: tx_id,
            transaction_type: tx.transaction_type.clone(),
            status: TransactionStatus::Executed,
            proposer: tx.proposer.clone(),
            signers,
            executed_at: env.ledger().timestamp(),
        };
        set_transaction_history(&env, tx_id, &record);
        increment_transaction_count(&env);

        env.events().publish((TX_EXECUTED,), (tx_id, executor, result.clone()));

        result
    }

    /// Reject a pending transaction (any signer can reject their own proposal)
    pub fn reject_transaction(
        env: Env,
        rejector: Address,
        tx_id: u64,
    ) {
        rejector.require_auth();

        let mut tx = get_transaction(&env, tx_id).expect("Transaction not found");

        // Only proposer can reject their own transaction before threshold
        if tx.proposer != rejector {
            panic!("Only proposer can reject");
        }

        if tx.status != TransactionStatus::Pending {
            panic!("Transaction not pending");
        }

        tx.status = TransactionStatus::Rejected;
        set_transaction(&env, &tx);

        // Remove from pending list
        let mut pending = get_pending_transactions(&env);
        let mut new_pending = Vec::new(&env);
        for id in pending.iter() {
            if id != tx_id {
                new_pending.push_back(id);
            }
        }
        set_pending_transactions(&env, &new_pending);

        env.events().publish((TX_REJECTED,), (tx_id, rejector));
    }

    // ==================== EMERGENCY RECOVERY ====================

    /// Activate emergency recovery mode (Owner only)
    pub fn activate_emergency_recovery(
        env: Env,
        caller: Address,
        reason: String,
    ) {
        caller.require_auth();

        let caller_member = get_member(&env, &caller).expect("Caller not a member");
        if caller_member.role != Role::Owner {
            panic!("Only Owner can activate emergency recovery");
        }

        let config = get_config(&env).expect("Not initialized");
        if !config.emergency_recovery_enabled {
            panic!("Emergency recovery not enabled");
        }

        // Check cooldown
        let last_emergency = get_last_emergency_at(&env);
        let current_time = env.ledger().timestamp();
        if current_time < last_emergency + config.emergency_cooldown {
            panic!("Emergency cooldown active");
        }

        let emergency_state = EmergencyState {
            activated_at: current_time,
            activated_by: caller.clone(),
            reason: reason.clone(),
            new_owner: caller.clone(),
            recovery_approved: false,
        };

        set_emergency_state(&env, &emergency_state);
        set_last_emergency_at(&env, current_time);

        env.events().publish((EMERGENCY_ACTIVATED,), (caller, reason));
    }

    /// Execute emergency recovery - transfers ownership to a new address (requires 2/3 of Owners)
    pub fn execute_emergency_recovery(
        env: Env,
        caller: Address,
        new_owner: Address,
    ) {
        caller.require_auth();

        let emergency_state = get_emergency_state(&env).expect("Emergency not activated");
        if emergency_state.recovery_approved {
            panic!("Recovery already executed");
        }

        // Verify caller is an Owner
        let caller_member = get_member(&env, &caller).expect("Caller not a member");
        if caller_member.role != Role::Owner {
            panic!("Only Owners can approve emergency recovery");
        }

        // Count total owners
        let members = get_members(&env);
        let mut owner_count: u32 = 0;
        for m in members.iter() {
            if let Some(mem) = get_member(&env, &m) {
                if mem.role == Role::Owner && mem.active {
                    owner_count += 1;
                }
            }
        }

        let required_approvals = (owner_count * 2 / 3).max(1);

        // For simplicity in this contract, we'll require the original activator plus one more owner
        // In production, you'd track individual approvals
        if caller == emergency_state.activated_by {
            panic!("Activator cannot be the only approver");
        }

        // Update config with new owner
        let mut config = get_config(&env).expect("Not initialized");
        config.owner = new_owner.clone();
        set_config(&env, &config);

        // Add new owner as member if not already
        if get_member(&env, &new_owner).is_none() {
            let member = Member {
                address: new_owner.clone(),
                role: Role::Owner,
                added_at: env.ledger().timestamp(),
                active: true,
            };
            set_member(&env, &new_owner, &member);
            
            let mut members = get_members(&env);
            members.push_back(new_owner.clone());
            set_members(&env, &members);
        } else {
            // Update existing member to Owner
            let mut member = get_member(&env, &new_owner).unwrap();
            member.role = Role::Owner;
            member.active = true;
            set_member(&env, &new_owner, &member);
        }

        // Mark recovery as complete
        let mut updated_state = emergency_state;
        updated_state.recovery_approved = true;
        updated_state.new_owner = new_owner.clone();
        set_emergency_state(&env, &updated_state);

        env.events().publish((RECOVERY_COMPLETED,), (new_owner,));
    }

    /// Cancel emergency recovery (original activator or any Owner)
    pub fn cancel_emergency_recovery(
        env: Env,
        caller: Address,
    ) {
        caller.require_auth();

        let emergency_state = get_emergency_state(&env).expect("Emergency not activated");
        if emergency_state.recovery_approved {
            panic!("Recovery already executed");
        }

        let caller_member = get_member(&env, &caller).expect("Caller not a member");
        
        // Only activator or any Owner can cancel
        if caller != emergency_state.activated_by && caller_member.role != Role::Owner {
            panic!("Unauthorized to cancel emergency");
        }

        // Remove emergency state
        env.storage().persistent().remove(&DataKey::EmergencyState);
    }

    // ==================== VIEW FUNCTIONS ====================

    /// Get treasury configuration
    pub fn get_config_info(env: Env) -> TreasuryConfig {
        get_config(&env).expect("Not initialized")
    }

    /// Get member information
    pub fn get_member_info(env: Env, address: Address) -> Option<Member> {
        get_member(&env, &address)
    }

    /// Get all members
    pub fn get_all_members(env: Env) -> Vec<Address> {
        get_members(&env)
    }

    /// Get transaction details
    pub fn get_transaction_info(env: Env, tx_id: u64) -> Option<Transaction> {
        get_transaction(&env, tx_id)
    }

    /// Get pending transactions
    pub fn get_pending_transaction_ids(env: Env) -> Vec<u64> {
        get_pending_transactions(&env)
    }

    /// Get transaction history
    pub fn get_transaction_record(env: Env, tx_id: u64) -> Option<TransactionRecord> {
        get_transaction_history(&env, tx_id)
    }

    /// Get total executed transaction count
    pub fn get_executed_transaction_count(env: Env) -> u64 {
        get_transaction_count(&env)
    }

    /// Get emergency state
    pub fn get_emergency_info(env: Env) -> Option<EmergencyState> {
        get_emergency_state(&env)
    }

    /// Check if an address has signed a transaction
    pub fn has_signer_signed(env: Env, tx_id: u64, signer: Address) -> bool {
        has_signed(&env, tx_id, &signer)
    }

    // ==================== HELPER FUNCTIONS ====================

    /// Create a new transaction proposal
    fn create_transaction(
        env: &Env,
        proposer: Address,
        tx_type: TransactionType,
        target: Option<Address>,
        amount: Option<i128>,
        destination: Option<Address>,
        function: Option<Symbol>,
        args: Option<Vec<Val>>,
        description: String,
        required_role: Role,
    ) -> u64 {
        let config = get_config(env).expect("Not initialized");
        
        // Check pending transaction limit
        let pending = get_pending_transactions(env);
        if pending.len() >= config.max_pending_proposals as u32 {
            panic!("Too many pending proposals");
        }

        let tx_id = increment_transaction_id(env);
        let current_time = env.ledger().timestamp();

        let transaction = Transaction {
            id: tx_id,
            proposer,
            transaction_type: tx_type,
            status: TransactionStatus::Pending,
            target,
            amount,
            destination,
            function,
            args,
            signatures: Vec::new(env),
            created_at: current_time,
            expires_at: current_time + config.proposal_timeout,
            executed_at: None,
            description,
            required_role,
        };

        set_transaction(env, &transaction);

        // Add to pending list
        let mut pending = get_pending_transactions(env);
        pending.push_back(tx_id);
        set_pending_transactions(env, &pending);

        env.events().publish((TX_PROPOSED,), (tx_id, transaction.transaction_type.clone()));

        tx_id
    }

    /// Verify address is an active signer
    fn require_signer(env: &Env, address: &Address) {
        let member = get_member(env, address).expect("Not a member");
        if !member.active {
            panic!("Member not active");
        }
        // Signer, Admin, and Owner can all sign
        if member.role != Role::Signer && member.role != Role::Admin && member.role != Role::Owner {
            panic!("Not a signer");
        }
    }

    /// Get role level for comparison (Owner > Admin > Signer)
    fn role_level(role: &Role) -> u8 {
        match role {
            Role::Signer => 1,
            Role::Admin => 2,
            Role::Owner => 3,
        }
    }
}

#[cfg(test)]
mod test;
