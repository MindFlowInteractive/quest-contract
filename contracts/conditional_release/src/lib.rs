#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, Vec};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MilestoneStatus {
    Pending = 1,
    Completed = 2,
    Verified = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Milestone {
    pub id: u32,
    pub description: String,
    pub status: MilestoneStatus,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscrowConfig {
    pub depositor: Address,
    pub beneficiary: Address,
    pub token: Address,
    pub amount: i128,
    pub is_released: bool,
    pub is_refunded: bool,
}

#[contracttype]
pub enum DataKey {
    EscrowConfig,
    Milestones,
    ConditionHistory,
}

#[contract]
pub struct ConditionalReleaseEscrowContract;

#[contractimpl]
impl ConditionalReleaseEscrowContract {
    /// Initializes the escrow with depositor, beneficiary, token, amount, and initial milestones.
    pub fn initialize(
        env: Env,
        depositor: Address,
        beneficiary: Address,
        token: Address,
        amount: i128,
        milestone_descriptions: Vec<String>,
    ) {
        depositor.require_auth();

        if env.storage().instance().has(&DataKey::EscrowConfig) {
            panic!("Escrow is already initialized");
        }

        if amount <= 0 {
            panic!("Escrow amount must be greater than zero");
        }

        // Transfer escrow tokens from depositor to contract
        let token_client = soroban_sdk::token::Client::new(&env, &token);
        token_client.transfer(&depositor, &env.current_contract_address(), &amount);

        let config = EscrowConfig {
            depositor,
            beneficiary,
            token,
            amount,
            is_released: false,
            is_refunded: false,
        };

        // Initialize milestones
        let mut milestones = Vec::new(&env);
        for (index, desc) in milestone_descriptions.iter().enumerate() {
            milestones.push_back(Milestone {
                id: index as u32,
                description: desc,
                status: MilestoneStatus::Pending,
            });
        }

        env.storage().instance().set(&DataKey::EscrowConfig, &config);
        env.storage().instance().set(&DataKey::Milestones, &milestones);
        env.storage().instance().set(&DataKey::ConditionHistory, &Vec::<String>::new(&env));

        env.events().publish(
            (String::from_str(&env, "EscrowInitialized"),),
            amount,
        );
    }

    /// Updates milestone status and records condition history.
    pub fn verify_milestone(env: Env, verifier: Address, milestone_id: u32) {
        verifier.require_auth();

        let mut config: EscrowConfig = env
            .storage()
            .instance()
            .get(&DataKey::EscrowConfig)
            .unwrap_or_else(|| panic!("Escrow not initialized"));

        if config.is_released || config.is_refunded {
            panic!("Escrow is already finalized");
        }

        let mut milestones: Vec<Milestone> = env
            .storage()
            .instance()
            .get(&DataKey::Milestones)
            .unwrap_or_else(|| panic!("Milestones not found"));

        let mut found = false;
        let mut updated_milestones = Vec::new(&env);

        for m in milestones.iter() {
            if m.id == milestone_id {
                let mut updated_m = m.clone();
                updated_m.status = MilestoneStatus::Verified;
                updated_milestones.push_back(updated_m);
                found = true;
            } else {
                updated_milestones.push_back(m);
            }
        }

        if !found {
            panic!("Milestone ID not found");
        }

        env.storage().instance().set(&DataKey::Milestones, &updated_milestones);

        // Record condition history
        let mut history: Vec<String> = env
            .storage()
            .instance()
            .get(&DataKey::ConditionHistory)
            .unwrap_or_else(|| Vec::new(&env));
        
        history.push_back(String::from_str(&env, "Milestone Verified"));
        env.storage().instance().set(&DataKey::ConditionHistory, &history);

        // Automatically trigger release if all milestones are verified
        Self::check_and_release(env, config, updated_milestones);
    }

    /// Automatically releases escrow funds to the beneficiary when all milestones are verified.
    fn check_and_release(env: Env, mut config: EscrowConfig, milestones: Vec<Milestone>) {
        for m in milestones.iter() {
            if m.status != MilestoneStatus::Verified {
                return; // Not all milestones completed yet
            }
        }

        config.is_released = true;
        env.storage().instance().set(&DataKey::EscrowConfig, &config);

        // Transfer funds to beneficiary
        let token_client = soroban_sdk::token::Client::new(&env, &config.token);
        token_client.transfer(&env.current_contract_address(), &config.beneficiary, &config.amount);

        env.events().publish(
            (String::from_str(&env, "EscrowReleased"), config.beneficiary.clone()),
            config.amount,
        );
    }

    /// Processes refund to the depositor if conditions fail or cancellation is approved.
    pub fn refund_escrow(env: Env) {
        let mut config: EscrowConfig = env
            .storage()
            .instance()
            .get(&DataKey::EscrowConfig)
            .unwrap_or_else(|| panic!("Escrow not initialized"));

        config.depositor.require_auth();

        if config.is_released || config.is_refunded {
            panic!("Escrow already finalized");
        }

        config.is_refunded = true;
        env.storage().instance().set(&DataKey::EscrowConfig, &config);

        // Refund tokens to depositor
        let token_client = soroban_sdk::token::Client::new(&env, &config.token);
        token_client.transfer(&env.current_contract_address(), &config.depositor, &config.amount);

        env.events().publish(
            (String::from_str(&env, "EscrowRefunded"), config.depositor.clone()),
            config.amount,
        );
    }

    pub fn get_milestones(env: Env) -> Vec<Milestone> {
        env.storage()
            .instance()
            .get(&DataKey::Milestones)
            .unwrap_or_else(|| Vec::new(&env))
    }
}