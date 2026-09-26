#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, Vec};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalletConfig {
    pub owner: Address,
    pub guardians: Vec<Address>,
    pub recovery_threshold: u32,
}

#[contracttype]
pub enum DataKey {
    Config,
    Delegation(Address),
    RecoveryApprovals,
}

#[contract]
pub struct UniversalWalletContract;

#[contractimpl]
impl UniversalWalletContract {
    /// Initializes the universal wallet with an owner, recovery guardians, and a recovery threshold.
    pub fn initialize(env: Env, owner: Address, guardians: Vec<Address>, recovery_threshold: u32) {
        if env.storage().instance().has(&DataKey::Config) {
            panic!("Wallet is already initialized");
        }

        if guardians.len() == 0 || recovery_threshold == 0 || recovery_threshold > guardians.len() {
            panic!("Invalid guardian configuration or recovery threshold");
        }

        let config = WalletConfig {
            owner: owner.clone(),
            guardians,
            recovery_threshold,
        };

        env.storage().instance().set(&DataKey::Config, &config);
        env.storage().instance().set(&DataKey::RecoveryApprovals, &Vec::<Address>::new(&env));

        env.events().publish(
            (String::from_str(&env, "WalletInitialized"), owner),
            recovery_threshold,
        );
    }

    /// Delegates spending or operational rights to a trusted operator/signer.
    pub fn set_delegation(env: Env, operator: Address, is_authorized: bool) {
        let config: WalletConfig = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .unwrap_or_else(|| panic!("Wallet not initialized"));

        config.owner.require_auth();

        env.storage().persistent().set(&DataKey::Delegation(operator.clone()), &is_authorized);

        env.events().publish(
            (String::from_str(&env, "DelegationUpdated"), operator),
            is_authorized,
        );
    }

    /// Executes a token transfer from the wallet custody to a recipient, authorized by owner or authorized delegate.
    pub fn transfer_asset(env: Env, caller: Address, token: Address, recipient: Address, amount: i128) {
        caller.require_auth();

        let config: WalletConfig = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .unwrap_or_else(|| panic!("Wallet not initialized"));

        // Check if caller is owner or authorized delegate
        let is_owner = caller == config.owner;
        let is_delegate: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Delegation(caller.clone()))
            .unwrap_or(false);

        if !is_owner && !is_delegate {
            panic!("Unauthorized caller: must be owner or authorized delegate");
        }

        if amount <= 0 {
            panic!("Transfer amount must be greater than zero");
        }

        // Transfer tokens from wallet contract address to recipient
        let token_client = soroban_sdk::token::Client::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &recipient, &amount);

        env.events().publish(
            (String::from_str(&env, "AssetTransferred"), token),
            (recipient, amount),
        );
    }

    /// Initiates or supports wallet recovery via trusted guardians.
    pub fn approve_recovery(env: Env, guardian: Address, new_owner: Address) {
        guardian.require_auth();

        let config: WalletConfig = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .unwrap_or_else(|| panic!("Wallet not initialized"));

        // Verify caller is a registered guardian
        let mut is_guardian = false;
        for g in config.guardians.iter() {
            if g == guardian {
                is_guardian = true;
                break;
            }
        }

        if !is_guardian {
            panic!("Caller is not an authorized recovery guardian");
        }

        let mut approvals: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::RecoveryApprovals)
            .unwrap_or_else(|| Vec::new(&env));

        // Prevent duplicate guardian approvals
        for a in approvals.iter() {
            if a == guardian {
                panic!("Guardian has already approved recovery");
            }
        }

        approvals.push_back(guardian.clone());
        env.storage().persistent().set(&DataKey::RecoveryApprovals, &approvals);

        // Check if recovery threshold is met
        if approvals.len() >= config.recovery_threshold {
            let mut updated_config = config;
            updated_config.owner = new_owner.clone();
            env.storage().instance().set(&DataKey::Config, &updated_config);

            // Clear approvals
            env.storage().persistent().set(&DataKey::RecoveryApprovals, &Vec::<Address>::new(&env));

            env.events().publish(
                (String::from_str(&env, "WalletRecovered"), new_owner.clone()),
                approvals.len(),
            );
        }
    }

    pub fn get_wallet_config(env: Env) -> WalletConfig {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .unwrap_or_else(|| panic!("Wallet not initialized"))
    }
}