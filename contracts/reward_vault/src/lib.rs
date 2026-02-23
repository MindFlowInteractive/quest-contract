#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Vec};

const BASIS_POINTS: i128 = 10_000;

#[contracttype]
#[derive(Clone, Debug)]
pub enum DataKey {
    Config,
    Vault(Address),
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct LockOption {
    pub period: u64,
    pub bonus_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct VaultConfig {
    pub admin: Address,
    pub token: Address,
    pub early_withdraw_penalty_bps: u32,
    pub emergency_penalty_bps: u32,
    pub emergency_unlock_enabled: bool,
    pub lock_options: Vec<LockOption>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct VaultPosition {
    pub owner: Address,
    pub amount: i128,
    pub lock_period: u64,
    pub bonus_bps: u32,
    pub deposited_at: u64,
    pub maturity_at: u64,
    pub beneficiary: Option<Address>,
}

#[contract]
pub struct RewardVaultContract;

#[contractimpl]
impl RewardVaultContract {
    pub fn initialize(
        env: Env,
        admin: Address,
        token: Address,
        early_withdraw_penalty_bps: u32,
        emergency_penalty_bps: u32,
        lock_periods: Vec<u64>,
        bonus_bps: Vec<u32>,
    ) {
        admin.require_auth();

        if env.storage().persistent().has(&DataKey::Config) {
            panic!("Already initialized");
        }

        if lock_periods.is_empty() || lock_periods.len() != bonus_bps.len() {
            panic!("Invalid lock options");
        }
        if early_withdraw_penalty_bps > BASIS_POINTS as u32 || emergency_penalty_bps > BASIS_POINTS as u32 {
            panic!("Invalid penalty");
        }

        let mut lock_options = Vec::new(&env);
        let mut i: u32 = 0;
        while i < lock_periods.len() {
            let period = lock_periods.get(i).unwrap();
            let bonus = bonus_bps.get(i).unwrap();
            if period == 0 {
                panic!("Lock period must be positive");
            }
            lock_options.push_back(LockOption {
                period,
                bonus_bps: bonus,
            });
            i += 1;
        }

        let config = VaultConfig {
            admin,
            token,
            early_withdraw_penalty_bps,
            emergency_penalty_bps,
            emergency_unlock_enabled: false,
            lock_options,
        };

        env.storage().persistent().set(&DataKey::Config, &config);
    }

    pub fn fund_bonus_pool(env: Env, admin: Address, amount: i128) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);
        if amount <= 0 {
            panic!("Amount must be positive");
        }

        let config = Self::get_config(env.clone());
        let token_client = token::Client::new(&env, &config.token);
        token_client.transfer(&admin, &env.current_contract_address(), &amount);
    }

    pub fn set_emergency_unlock(env: Env, admin: Address, enabled: bool) {
        admin.require_auth();
        let mut config = Self::get_config(env.clone());
        if config.admin != admin {
            panic!("Admin only");
        }
        config.emergency_unlock_enabled = enabled;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    pub fn deposit(env: Env, owner: Address, amount: i128, lock_period: u64) {
        owner.require_auth();
        if amount <= 0 {
            panic!("Amount must be positive");
        }

        if let Some(existing) = Self::get_vault(env.clone(), owner.clone()) {
            if existing.amount > 0 {
                panic!("Active vault exists");
            }
        }

        let config = Self::get_config(env.clone());
        let selected_bonus = Self::bonus_for_lock_period(&config, lock_period);
        let now = env.ledger().timestamp();

        let vault = VaultPosition {
            owner: owner.clone(),
            amount,
            lock_period,
            bonus_bps: selected_bonus,
            deposited_at: now,
            maturity_at: now + lock_period,
            beneficiary: None,
        };

        let token_client = token::Client::new(&env, &config.token);
        token_client.transfer(&owner, &env.current_contract_address(), &amount);

        env.storage().persistent().set(&DataKey::Vault(owner), &vault);
    }

    pub fn set_beneficiary(env: Env, owner: Address, beneficiary: Address) {
        owner.require_auth();
        let mut vault = Self::must_get_vault(env.clone(), owner.clone());
        vault.beneficiary = Some(beneficiary);
        env.storage().persistent().set(&DataKey::Vault(owner), &vault);
    }

    pub fn extend_lock(env: Env, owner: Address, additional_lock_period: u64) {
        owner.require_auth();
        if additional_lock_period == 0 {
            panic!("Additional lock must be positive");
        }

        let config = Self::get_config(env.clone());
        let mut vault = Self::must_get_vault(env.clone(), owner.clone());
        if env.ledger().timestamp() >= vault.maturity_at {
            panic!("Vault already matured");
        }

        let current_total_lock = vault.maturity_at - vault.deposited_at;
        let new_total_lock = current_total_lock + additional_lock_period;
        let new_bonus_bps = Self::bonus_for_lock_period(&config, new_total_lock);

        vault.lock_period = new_total_lock;
        vault.bonus_bps = new_bonus_bps;
        vault.maturity_at += additional_lock_period;

        env.storage().persistent().set(&DataKey::Vault(owner), &vault);
    }

    pub fn withdraw_mature(env: Env, owner: Address) -> i128 {
        owner.require_auth();
        let vault = Self::must_get_vault(env.clone(), owner.clone());
        if env.ledger().timestamp() < vault.maturity_at {
            panic!("Vault not matured");
        }

        let payout = vault.amount + Self::calculate_bonus(vault.amount, vault.bonus_bps);
        let config = Self::get_config(env.clone());
        let token_client = token::Client::new(&env, &config.token);
        token_client.transfer(&env.current_contract_address(), &owner, &payout);

        env.storage().persistent().remove(&DataKey::Vault(owner));
        payout
    }

    pub fn distribute_mature_payout(env: Env, owner: Address) -> i128 {
        let vault = Self::must_get_vault(env.clone(), owner.clone());
        if env.ledger().timestamp() < vault.maturity_at {
            panic!("Vault not matured");
        }

        let payout = vault.amount + Self::calculate_bonus(vault.amount, vault.bonus_bps);
        let config = Self::get_config(env.clone());
        let token_client = token::Client::new(&env, &config.token);
        token_client.transfer(&env.current_contract_address(), &owner, &payout);

        env.storage().persistent().remove(&DataKey::Vault(owner));
        payout
    }

    pub fn early_withdraw(env: Env, owner: Address) -> i128 {
        owner.require_auth();
        let vault = Self::must_get_vault(env.clone(), owner.clone());
        if env.ledger().timestamp() >= vault.maturity_at {
            panic!("Vault already matured");
        }

        let config = Self::get_config(env.clone());
        let penalty = (vault.amount * config.early_withdraw_penalty_bps as i128) / BASIS_POINTS;
        let payout = vault.amount - penalty;

        let token_client = token::Client::new(&env, &config.token);
        token_client.transfer(&env.current_contract_address(), &owner, &payout);

        env.storage().persistent().remove(&DataKey::Vault(owner));
        payout
    }

    pub fn emergency_withdraw(env: Env, owner: Address) -> i128 {
        owner.require_auth();
        let vault = Self::must_get_vault(env.clone(), owner.clone());
        let config = Self::get_config(env.clone());
        if !config.emergency_unlock_enabled {
            panic!("Emergency unlock disabled");
        }

        let penalty = (vault.amount * config.emergency_penalty_bps as i128) / BASIS_POINTS;
        let payout = vault.amount - penalty;
        let token_client = token::Client::new(&env, &config.token);
        token_client.transfer(&env.current_contract_address(), &owner, &payout);

        env.storage().persistent().remove(&DataKey::Vault(owner));
        payout
    }

    pub fn claim_inheritance(env: Env, beneficiary: Address, owner: Address) -> i128 {
        beneficiary.require_auth();
        let vault = Self::must_get_vault(env.clone(), owner.clone());
        if env.ledger().timestamp() < vault.maturity_at {
            panic!("Vault not matured");
        }

        match vault.beneficiary {
            Some(ref saved_beneficiary) if *saved_beneficiary == beneficiary => {}
            _ => panic!("Not beneficiary"),
        }

        let payout = vault.amount + Self::calculate_bonus(vault.amount, vault.bonus_bps);
        let config = Self::get_config(env.clone());
        let token_client = token::Client::new(&env, &config.token);
        token_client.transfer(&env.current_contract_address(), &beneficiary, &payout);

        env.storage().persistent().remove(&DataKey::Vault(owner));
        payout
    }

    pub fn get_config(env: Env) -> VaultConfig {
        env.storage().persistent().get(&DataKey::Config).unwrap()
    }

    pub fn get_vault(env: Env, owner: Address) -> Option<VaultPosition> {
        env.storage().persistent().get(&DataKey::Vault(owner))
    }

    pub fn is_mature(env: Env, owner: Address) -> bool {
        if let Some(vault) = Self::get_vault(env.clone(), owner) {
            env.ledger().timestamp() >= vault.maturity_at
        } else {
            false
        }
    }

    pub fn get_time_until_maturity(env: Env, owner: Address) -> u64 {
        if let Some(vault) = Self::get_vault(env.clone(), owner) {
            let now = env.ledger().timestamp();
            if now >= vault.maturity_at {
                0
            } else {
                vault.maturity_at - now
            }
        } else {
            0
        }
    }

    pub fn preview_mature_payout(env: Env, owner: Address) -> i128 {
        if let Some(vault) = Self::get_vault(env, owner) {
            vault.amount + Self::calculate_bonus(vault.amount, vault.bonus_bps)
        } else {
            0
        }
    }

    pub fn quote_bonus_for_lock(env: Env, lock_period: u64, amount: i128) -> i128 {
        if amount <= 0 {
            panic!("Amount must be positive");
        }
        let config = Self::get_config(env);
        let bps = Self::bonus_for_lock_period(&config, lock_period);
        Self::calculate_bonus(amount, bps)
    }

    fn must_get_vault(env: Env, owner: Address) -> VaultPosition {
        env.storage()
            .persistent()
            .get(&DataKey::Vault(owner))
            .expect("Vault not found")
    }

    fn bonus_for_lock_period(config: &VaultConfig, lock_period: u64) -> u32 {
        for option in config.lock_options.iter() {
            if option.period == lock_period {
                return option.bonus_bps;
            }
        }
        panic!("Unsupported lock period");
    }

    fn calculate_bonus(amount: i128, bonus_bps: u32) -> i128 {
        (amount * bonus_bps as i128) / BASIS_POINTS
    }

    fn assert_admin(env: &Env, user: &Address) {
        let config: VaultConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        if config.admin != *user {
            panic!("Admin only");
        }
    }
}

mod test;
