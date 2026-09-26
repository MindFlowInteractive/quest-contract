#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, Vec};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollaboratorShare {
    pub creator: Address,
    pub share_bps: u32, // Basis points (e.g., 5000 = 50%)
    pub claimed_amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollaborationConfig {
    pub token: Address,
    pub total_revenue: i128,
    pub is_disputed: bool,
}

#[contracttype]
pub enum DataKey {
    Config,
    Collaborators,
    History,
}

#[contract]
pub struct CollaborationRevenueContract;

#[contractimpl]
impl CollaborationRevenueContract {
    /// Initializes collaboration with creators and their respective basis point shares (totaling exactly 10,000).
    pub fn initialize(env: Env, token: Address, collaborators: Vec<CollaboratorShare>) {
        if env.storage().instance().has(&DataKey::Config) {
            panic!("Collaboration contract is already initialized");
        }

        let mut total_bps: u32 = 0;
        for c in collaborators.iter() {
            if c.share_bps == 0 {
                panic!("Collaborator share must be greater than zero");
            }
            total_bps = total_bps.checked_add(c.share_bps).unwrap_or_else(|| {
                panic!("Overflow in share calculation");
            });
        }

        if total_bps != 10000 {
            panic!("Total collaborator shares must equal exactly 10,000 basis points (100%)");
        }

        let config = CollaborationConfig {
            token,
            total_revenue: 0,
            is_disputed: false,
        };

        env.storage().instance().set(&DataKey::Config, &config);
        env.storage().instance().set(&DataKey::Collaborators, &collaborators);
        env.storage().instance().set(&DataKey::History, &Vec::<String>::new(&env));

        env.events().publish(
            (String::from_str(&env, "CollaborationInitialized"),),
            collaborators.len(),
        );
    }

    /// Deposits revenue into the collaboration pool for fair distribution among creators.
    pub fn deposit_revenue(env: Env, depositor: Address, amount: i128) {
        depositor.require_auth();

        if amount <= 0 {
            panic!("Deposit amount must be greater than zero");
        }

        let mut config: CollaborationConfig = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .unwrap_or_else(|| panic!("Collaboration not initialized"));

        if config.is_disputed {
            panic!("Revenue distribution is currently paused due to an active dispute");
        }

        // Transfer revenue tokens from depositor to contract
        let token_client = soroban_sdk::token::Client::new(&env, &config.token);
        token_client.transfer(&depositor, &env.current_contract_address(), &amount);

        config.total_revenue = config.total_revenue.checked_add(amount).unwrap_or_else(|| {
            panic!("Overflow in total revenue calculation");
        });

        env.storage().instance().set(&DataKey::Config, &config);

        env.events().publish(
            (String::from_str(&env, "RevenueDeposited"), depositor),
            amount,
        );
    }

    /// Allows an individual creator to claim their proportional revenue share.
    pub fn claim_revenue(env: Env, creator: Address) -> i128 {
        creator.require_auth();

        let config: CollaborationConfig = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .unwrap_or_else(|| panic!("Collaboration not initialized"));

        if config.is_disputed {
            panic!("Revenue claims are suspended during an active dispute");
        }

        let mut collaborators: Vec<CollaboratorShare> = env
            .storage()
            .instance()
            .get(&DataKey::Collaborators)
            .unwrap_or_else(|| panic!("Collaborators not found"));

        let mut found_index = None;
        let mut target_collab = None;

        for (i, c) in collaborators.iter().enumerate() {
            if c.creator == creator {
                found_index = Some(i);
                target_collab = Some(c);
                break;
            }
        }

        let mut collab = target_collab.unwrap_or_else(|| {
            panic!("Address is not a registered collaborator");
        });

        // Calculate total entitled amount based on share_bps
        let total_entitled = (config.total_revenue as u128)
            .checked_mul(collab.share_bps as u128)
            .unwrap() / 10000u128;

        let claimable = (total_entitled as i128) - collab.claimed_amount;

        if claimable <= 0 {
            panic!("No new revenue available to claim");
        }

        collab.claimed_amount = collab.claimed_amount.checked_add(claimable).unwrap();
        collaborators.set(found_index.unwrap() as u32, collab);

        env.storage().instance().set(&DataKey::Collaborators, &collaborators);

        // Transfer claimable tokens to creator
        let token_client = soroban_sdk::token::Client::new(&env, &config.token);
        token_client.transfer(&env.current_contract_address(), &creator, &claimable);

        env.events().publish(
            (String::from_str(&env, "RevenueClaimed"), creator),
            claimable,
        );

        claimable
    }

    /// Resolves disputes or updates dispute status for revenue distribution governance.
    pub fn set_dispute_status(env: Env, admin: Address, is_disputed: bool) {
        admin.require_auth();

        let mut config: CollaborationConfig = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .unwrap_or_else(|| panic!("Collaboration not initialized"));

        config.is_disputed = is_disputed;
        env.storage().instance().set(&DataKey::Config, &config);

        let mut history: Vec<String> = env
            .storage()
            .instance()
            .get(&DataKey::History)
            .unwrap_or_else(|| Vec::new(&env));

        history.push_back(if is_disputed { String::from_str(&env, "DisputeFlagged") } else { String::from_str(&env, "DisputeResolved") });
        env.storage().instance().set(&DataKey::History, &history);
    }

    pub fn get_collaborators(env: Env) -> Vec<CollaboratorShare> {
        env.storage()
            .instance()
            .get(&DataKey::Collaborators)
            .unwrap_or_else(|| Vec::new(&env))
    }
}