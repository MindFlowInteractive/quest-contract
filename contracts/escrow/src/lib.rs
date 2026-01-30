#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, IntoVal, Symbol, Val,
};

// 1. DATA STRUCTURES

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Party {
    pub address: Address,
    pub approved: bool,
    pub deposit: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EscrowStatus {
    Created = 1,
    Active = 2,
    Disputed = 3,
    Resolved = 4,
    Refunded = 5,
    Released = 6,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseCondition {
    pub condition_id: u64,
    pub description: String,
    pub fulfilled: bool,
    pub evidence: Option<String>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscrowAgreement {
    pub escrow_id: u64,
    pub creator: Address,
    pub parties: Vec<Party>,
    pub arbitrator: Address,
    pub status: EscrowStatus,
    pub total_deposit: i128,
    pub release_conditions: Vec<ReleaseCondition>,
    pub dispute_reason: Option<String>,
    pub resolution: Option<String>,
    pub created_at: u64,
    pub timeout_at: u64,
    pub last_activity_at: u64,
}

#[contracttype]
pub enum DataKey {
    Escrow(u64),
    EscrowCount,
    NextConditionId,
}

// 2. CONTRACT LOGIC
#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    /// Initialize the contract
    pub fn init(env: Env, _admin: Address) {
        if !env.storage().instance().has(&DataKey::EscrowCount) {
            env.storage().instance().set(&DataKey::EscrowCount, &0u64);
        }
        if !env.storage().instance().has(&DataKey::NextConditionId) {
            env.storage().instance().set(&DataKey::NextConditionId, &1u64);
        }
    }

    /// Create a new escrow agreement
    pub fn create_escrow(
        env: Env,
        creator: Address,
        parties: Vec<Address>,
        arbitrator: Address,
        timeout_seconds: u64,
        release_conditions: Vec<String>,
    ) -> u64 {
        creator.require_auth();

        // Generate ID
        let mut id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::EscrowCount)
            .unwrap_or(0);
        id += 1;
        env.storage().instance().set(&DataKey::EscrowCount, &id);

        // Create Party objects
        let mut party_objects = Vec::new();
        for address in parties {
            party_objects.push(Party {
                address,
                approved: false,
                deposit: 0,
            });
        }

        // Create ReleaseConditions
        let mut conditions = Vec::new();
        let next_condition_id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextConditionId)
            .unwrap_or(1);
        for (i, desc) in release_conditions.iter().enumerate() {
            conditions.push(ReleaseCondition {
                condition_id: next_condition_id + i as u64,
                description: desc.clone(),
                fulfilled: false,
                evidence: None,
            });
        }
        env.storage().instance().set(
            &DataKey::NextConditionId,
            &(next_condition_id + release_conditions.len() as u64),
        );

        // Create Escrow Object
        let current_time = env.ledger().timestamp();
        let escrow = EscrowAgreement {
            escrow_id: id,
            creator,
            parties: party_objects,
            arbitrator,
            status: EscrowStatus::Created,
            total_deposit: 0,
            release_conditions: conditions,
            dispute_reason: None,
            resolution: None,
            created_at: current_time,
            timeout_at: current_time + timeout_seconds,
            last_activity_at: current_time,
        };

        // Save
        env.storage()
            .instance()
            .set(&DataKey::Escrow(id), &escrow);

        id
    }

    // Additional methods will be implemented next

    /// Make a deposit to the escrow
    pub fn deposit(env: Env, escrow_id: u64, amount: i128) {
        let mut escrow: EscrowAgreement = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .unwrap();

        // Check if escrow is still active
        if escrow.status != EscrowStatus::Created && escrow.status != EscrowStatus::Active {
            panic!("Escrow is not in a state to accept deposits");
        }

        // Check if caller is a party
        let caller = env.caller();
        let mut party_index = None;
        for (i, party) in escrow.parties.iter().enumerate() {
            if party.address == caller {
                party_index = Some(i);
                break;
            }
        }

        if party_index.is_none() {
            panic!("Caller is not a party to this escrow");
        }

        let party_index = party_index.unwrap();

        // Check if already deposited
        if escrow.parties[party_index].deposit > 0 {
            panic!("Party has already made a deposit");
        }

        // Transfer funds to contract
        let token_client = token::Client::new(&env, &env.current_contract_address());
        token_client.transfer(&caller, &env.current_contract_address(), &amount);

        // Update escrow state
        escrow.parties[party_index].deposit = amount;
        escrow.parties[party_index].approved = true;
        escrow.total_deposit += amount;
        escrow.last_activity_at = env.ledger().timestamp();

        // Check if all parties have deposited
        if escrow.parties.iter().all(|p| p.approved && p.deposit > 0) {
            escrow.status = EscrowStatus::Active;
        }

        env.storage()
            .instance()
            .set(&DataKey::Escrow(escrow_id), &escrow);
    }
    /// Approve the escrow terms
    pub fn approve_escrow(env: Env, escrow_id: u64) {
        let mut escrow: EscrowAgreement = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .unwrap();

        // Check if escrow is still in created state
        if escrow.status != EscrowStatus::Created {
            panic!("Escrow is not in a state to be approved");
        }

        // Check if caller is a party
        let caller = env.caller();
        let mut party_index = None;
        for (i, party) in escrow.parties.iter().enumerate() {
            if party.address == caller {
                party_index = Some(i);
                break;
            }
        }

        if party_index.is_none() {
            panic!("Caller is not a party to this escrow");
        }

        let party_index = party_index.unwrap();

        // Check if already approved
        if escrow.parties[party_index].approved {
            panic!("Party has already approved the escrow");
        }

        // Update approval status
        escrow.parties[party_index].approved = true;
        escrow.last_activity_at = env.ledger().timestamp();

        // Check if all parties have approved
        if escrow.parties.iter().all(|p| p.approved) {
            escrow.status = EscrowStatus::Active;
        }

        env.storage()
            .instance()
            .set(&DataKey::Escrow(escrow_id), &escrow);
    }
    /// Mark a release condition as fulfilled
    pub fn fulfill_condition(env: Env, escrow_id: u64, condition_id: u64, evidence: String) {
        let mut escrow: EscrowAgreement = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .unwrap();

        // Check if escrow is active
        if escrow.status != EscrowStatus::Active {
            panic!("Escrow is not in a state to fulfill conditions");
        }

        // Find the condition
        let mut condition_index = None;
        for (i, condition) in escrow.release_conditions.iter().enumerate() {
            if condition.condition_id == condition_id {
                condition_index = Some(i);
                break;
            }
        }

        if condition_index.is_none() {
            panic!("Condition not found");
        }

        let condition_index = condition_index.unwrap();

        // Check if already fulfilled
        if escrow.release_conditions[condition_index].fulfilled {
            panic!("Condition has already been fulfilled");
        }

        // Update condition
        escrow.release_conditions[condition_index].fulfilled = true;
        escrow.release_conditions[condition_index].evidence = Some(evidence);
        escrow.last_activity_at = env.ledger().timestamp();

        // Check if all conditions are fulfilled
        if escrow.release_conditions.iter().all(|c| c.fulfilled) {
            Self::release_escrow(env, escrow_id);
        } else {
            env.storage()
                .instance()
                .set(&DataKey::Escrow(escrow_id), &escrow);
        }
    }

    /// Release the escrow funds to parties
    fn release_escrow(env: Env, escrow_id: u64) {
        let mut escrow: EscrowAgreement = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .unwrap();

        // Calculate distribution
        let mut total_fulfilled_conditions = 0;
        for condition in &escrow.release_conditions {
            if condition.fulfilled {
                total_fulfilled_conditions += 1;
            }
        }

        // Distribute funds proportionally
        for party in &mut escrow.parties {
            if party.deposit > 0 {
                let share = (party.deposit * total_fulfilled_conditions as i128)
                    / escrow.total_deposit as i128;
                let token_client = token::Client::new(&env, &env.current_contract_address());
                token_client.transfer(
                    &env.current_contract_address(),
                    &party.address,
                    &share,
                );
                party.deposit = 0;
            }
        }

        escrow.status = EscrowStatus::Released;
        escrow.last_activity_at = env.ledger().timestamp();

        env.storage()
            .instance()
            .set(&DataKey::Escrow(escrow_id), &escrow);
    }
    /// Initiate a dispute
    pub fn initiate_dispute(env: Env, escrow_id: u64, reason: String) {
        let mut escrow: EscrowAgreement = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .unwrap();

        // Check if escrow is active
        if escrow.status != EscrowStatus::Active {
            panic!("Escrow is not in a state to be disputed");
        }

        // Check if caller is a party
        let caller = env.caller();
        let mut is_party = false;
        for party in &escrow.parties {
            if party.address == caller {
                is_party = true;
                break;
            }
        }

        if !is_party {
            panic!("Caller is not a party to this escrow");
        }

        // Update escrow state
        escrow.status = EscrowStatus::Disputed;
        escrow.dispute_reason = Some(reason);
        escrow.last_activity_at = env.ledger().timestamp();

        env.storage()
            .instance()
            .set(&DataKey::Escrow(escrow_id), &escrow);
    }

    /// Arbitrator resolves the dispute
    pub fn resolve_dispute(env: Env, escrow_id: u64, resolution: String) {
        let mut escrow: EscrowAgreement = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .unwrap();

        // Check if escrow is disputed
        if escrow.status != EscrowStatus::Disputed {
            panic!("Escrow is not in a disputed state");
        }

        // Check if caller is the arbitrator
        let caller = env.caller();
        if caller != escrow.arbitrator {
            panic!("Only the arbitrator can resolve disputes");
        }

        // Update escrow state
        escrow.status = EscrowStatus::Resolved;
        escrow.resolution = Some(resolution);
        escrow.last_activity_at = env.ledger().timestamp();

        env.storage()
            .instance()
            .set(&DataKey::Escrow(escrow_id), &escrow);
    }
    /// Check if escrow has timed out
    pub fn check_timeout(env: Env, escrow_id: u64) {
        let escrow: EscrowAgreement = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .unwrap();

        let current_time = env.ledger().timestamp();
        if current_time >= escrow.timeout_at && escrow.status == EscrowStatus::Active {
            Self::refund_escrow(env, escrow_id);
        }
    }
    /// Partially release funds based on fulfilled conditions
    pub fn partial_release(env: Env, escrow_id: u64) {
        let mut escrow: EscrowAgreement = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .unwrap();

        // Check if escrow is active
        if escrow.status != EscrowStatus::Active {
            panic!("Escrow is not in a state to be partially released");
        }

        // Calculate distribution based on fulfilled conditions
        let mut total_fulfilled_conditions = 0;
        for condition in &escrow.release_conditions {
            if condition.fulfilled {
                total_fulfilled_conditions += 1;
            }
        }

        // If no conditions are fulfilled, nothing to release
        if total_fulfilled_conditions == 0 {
            panic!("No conditions have been fulfilled for partial release");
        }

        // Distribute funds proportionally
        for party in &mut escrow.parties {
            if party.deposit > 0 {
                let share = (party.deposit * total_fulfilled_conditions as i128)
                    / escrow.total_deposit as i128;
                let token_client = token::Client::new(&env, &env.current_contract_address());
                token_client.transfer(
                    &env.current_contract_address(),
                    &party.address,
                    &share,
                );
                party.deposit -= share;
            }
        }

        // Update escrow state
        escrow.last_activity_at = env.ledger().timestamp();

        env.storage()
            .instance()
            .set(&DataKey::Escrow(escrow_id), &escrow);
    }
mod test;
