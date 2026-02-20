#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, IntoVal, Symbol, Vec,
};

const BASIS_POINTS: i128 = 10_000;
const MIN_FEE_BPS: u32 = 10;
const MAX_FEE_BPS: u32 = 30;

#[contracttype]
pub enum DataKey {
    Config,
    Pool(Address),
    PoolList,
    Analytics,
    FlashLoanCounter,
    FlashLoanRecord(u64),
    ReentrancyGuard,
    Paused,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlashLoanStatus {
    Active = 1,
    Repaid = 2,
    Defaulted = 3,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct FlashLoanConfig {
    pub admin: Address,
    pub fee_bps: u32,
    pub max_loan_ratio: u32,
    pub paused: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct LiquidityPool {
    pub token: Address,
    pub total_liquidity: i128,
    pub available_liquidity: i128,
    pub total_borrowed: i128,
    pub fees_collected: i128,
    pub lenders: Vec<Address>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct LenderPosition {
    pub lender: Address,
    pub amount: i128,
    pub deposit_time: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct FlashLoanRecord {
    pub loan_id: u64,
    pub borrower: Address,
    pub token: Address,
    pub principal: i128,
    pub fee: i128,
    pub repayment_amount: i128,
    pub start_time: u64,
    pub end_time: u64,
    pub status: FlashLoanStatus,
    pub callback_contract: Address,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct FlashLoanAnalytics {
    pub total_loans: u64,
    pub total_volume_borrowed: i128,
    pub total_fees_collected: i128,
    pub total_repaid: i128,
    pub defaulted_loans: u64,
    pub unique_borrowers: u64,
}

#[contract]
pub struct FlashLoanContract;

#[contractimpl]
impl FlashLoanContract {
    pub fn initialize(env: Env, admin: Address, fee_bps: u32) {
        admin.require_auth();

        if env.storage().persistent().has(&DataKey::Config) {
            panic!("Already initialized");
        }

        if fee_bps < MIN_FEE_BPS || fee_bps > MAX_FEE_BPS {
            panic!("Fee must be between 10-30 basis points (0.1%-0.3%)");
        }

        let config = FlashLoanConfig {
            admin,
            fee_bps,
            max_loan_ratio: 8000,
            paused: false,
        };

        let analytics = FlashLoanAnalytics {
            total_loans: 0,
            total_volume_borrowed: 0,
            total_fees_collected: 0,
            total_repaid: 0,
            defaulted_loans: 0,
            unique_borrowers: 0,
        };

        env.storage().persistent().set(&DataKey::Config, &config);
        env.storage()
            .persistent()
            .set(&DataKey::Analytics, &analytics);
        env.storage()
            .persistent()
            .set(&DataKey::FlashLoanCounter, &0u64);
        env.storage()
            .persistent()
            .set(&DataKey::PoolList, &Vec::<Address>::new(&env));
        env.storage().persistent().set(&DataKey::Paused, &false);
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyGuard, &false);
    }

    pub fn add_liquidity(env: Env, lender: Address, token: Address, amount: i128) {
        lender.require_auth();
        Self::assert_not_paused(&env);

        if amount <= 0 {
            panic!("Amount must be greater than zero");
        }

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&lender, &env.current_contract_address(), &amount);

        let mut pool = Self::get_or_create_pool(&env, &token);
        pool.total_liquidity += amount;
        pool.available_liquidity += amount;

        if !pool.lenders.contains(&lender) {
            pool.lenders.push_back(lender.clone());
        }

        env.storage()
            .persistent()
            .set(&DataKey::Pool(token.clone()), &pool);

        let position = LenderPosition {
            lender: lender.clone(),
            amount,
            deposit_time: env.ledger().timestamp(),
        };
        env.storage().persistent().set(
            &DataKeyKey::LenderPosition(token.clone(), lender),
            &position,
        );
    }

    pub fn remove_liquidity(env: Env, lender: Address, token: Address, amount: i128) {
        lender.require_auth();
        Self::assert_not_paused(&env);

        if amount <= 0 {
            panic!("Amount must be greater than zero");
        }

        let mut pool: LiquidityPool = env
            .storage()
            .persistent()
            .get(&DataKey::Pool(token.clone()))
            .unwrap_or_else(|| panic!("Pool not found"));

        if pool.available_liquidity < amount {
            panic!("Insufficient available liquidity");
        }

        let position: LenderPosition = env
            .storage()
            .persistent()
            .get(&DataKeyKey::LenderPosition(token.clone(), lender.clone()))
            .unwrap_or_else(|| panic!("Lender position not found"));

        if position.amount < amount {
            panic!("Insufficient lender balance");
        }

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &lender, &amount);

        pool.total_liquidity -= amount;
        pool.available_liquidity -= amount;

        env.storage()
            .persistent()
            .set(&DataKey::Pool(token.clone()), &pool);

        let new_amount = position.amount - amount;
        if new_amount == 0 {
            env.storage()
                .persistent()
                .remove(&DataKeyKey::LenderPosition(token.clone(), lender.clone()));
        } else {
            let updated_position = LenderPosition {
                lender: lender.clone(),
                amount: new_amount,
                deposit_time: position.deposit_time,
            };
            env.storage().persistent().set(
                &DataKeyKey::LenderPosition(token, lender),
                &updated_position,
            );
        }
    }

    pub fn flash_loan(
        env: Env,
        borrower: Address,
        token: Address,
        amount: i128,
        callback_contract: Address,
        callback_data: soroban_sdk::Bytes,
    ) -> u64 {
        borrower.require_auth();
        Self::assert_not_paused(&env);
        Self::assert_not_reentrant(&env);

        if amount <= 0 {
            panic!("Amount must be greater than zero");
        }

        let config: FlashLoanConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        let mut pool: LiquidityPool = env
            .storage()
            .persistent()
            .get(&DataKey::Pool(token.clone()))
            .unwrap_or_else(|| panic!("Pool not found for token"));

        let max_loan = (pool.available_liquidity as i128 * config.max_loan_ratio as i128
            / BASIS_POINTS) as i128;
        if amount > max_loan {
            panic!("Amount exceeds maximum loan limit");
        }

        if pool.available_liquidity < amount {
            panic!("Insufficient liquidity in pool");
        }

        let fee = (amount * config.fee_bps as i128) / BASIS_POINTS;
        let repayment_amount = amount + fee;

        let loan_id: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::FlashLoanCounter)
            .unwrap_or(0)
            + 1;
        env.storage()
            .persistent()
            .set(&DataKey::FlashLoanCounter, &loan_id);

        let start_time = env.ledger().timestamp();

        let record = FlashLoanRecord {
            loan_id,
            borrower: borrower.clone(),
            token: token.clone(),
            principal: amount,
            fee,
            repayment_amount,
            start_time,
            end_time: 0,
            status: FlashLoanStatus::Active,
            callback_contract: callback_contract.clone(),
        };

        env.storage()
            .persistent()
            .set(&DataKey::FlashLoanRecord(loan_id), &record);

        pool.available_liquidity -= amount;
        pool.total_borrowed += amount;
        env.storage()
            .persistent()
            .set(&DataKey::Pool(token.clone()), &pool);

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &borrower, &amount);

        Self::set_reentrancy_guard(&env, true);

        let callback_args = (borrower.clone(), token.clone(), amount, fee, callback_data);
        let _callback_result: bool = env.invoke_contract(
            &callback_contract,
            &Symbol::new(&env, "flash_loan_callback"),
            callback_args.into_val(&env),
        );

        Self::set_reentrancy_guard(&env, false);

        let current_balance = token_client.balance(&env.current_contract_address());
        let expected_balance_after_repayment = pool.available_liquidity + repayment_amount;

        if current_balance < expected_balance_after_repayment {
            let mut updated_record: FlashLoanRecord = env
                .storage()
                .persistent()
                .get(&DataKey::FlashLoanRecord(loan_id))
                .unwrap();
            updated_record.status = FlashLoanStatus::Defaulted;
            updated_record.end_time = env.ledger().timestamp();
            env.storage()
                .persistent()
                .set(&DataKey::FlashLoanRecord(loan_id), &updated_record);
            panic!("Flash loan not repaid within transaction");
        }

        let mut updated_pool: LiquidityPool = env
            .storage()
            .persistent()
            .get(&DataKey::Pool(token.clone()))
            .unwrap();

        updated_pool.available_liquidity += repayment_amount;
        updated_pool.fees_collected += fee;
        updated_pool.total_borrowed -= amount;
        env.storage()
            .persistent()
            .set(&DataKey::Pool(token), &updated_pool);

        let mut updated_record: FlashLoanRecord = env
            .storage()
            .persistent()
            .get(&DataKey::FlashLoanRecord(loan_id))
            .unwrap();
        updated_record.status = FlashLoanStatus::Repaid;
        updated_record.end_time = env.ledger().timestamp();
        env.storage()
            .persistent()
            .set(&DataKey::FlashLoanRecord(loan_id), &updated_record);

        Self::update_analytics(&env, amount, fee, true);

        loan_id
    }

    pub fn set_fee_bps(env: Env, admin: Address, fee_bps: u32) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        if fee_bps < MIN_FEE_BPS || fee_bps > MAX_FEE_BPS {
            panic!("Fee must be between 10-30 basis points (0.1%-0.3%)");
        }

        let mut config: FlashLoanConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        config.fee_bps = fee_bps;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    pub fn set_max_loan_ratio(env: Env, admin: Address, ratio: u32) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        if ratio == 0 || ratio > 10000 {
            panic!("Ratio must be between 1-10000 basis points");
        }

        let mut config: FlashLoanConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        config.max_loan_ratio = ratio;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    pub fn pause(env: Env, admin: Address) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut config: FlashLoanConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        config.paused = true;
        env.storage().persistent().set(&DataKey::Config, &config);
        env.storage().persistent().set(&DataKey::Paused, &true);
    }

    pub fn unpause(env: Env, admin: Address) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut config: FlashLoanConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        config.paused = false;
        env.storage().persistent().set(&DataKey::Config, &config);
        env.storage().persistent().set(&DataKey::Paused, &false);
    }

    pub fn withdraw_fees(env: Env, admin: Address, token: Address, amount: i128) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut pool: LiquidityPool = env
            .storage()
            .persistent()
            .get(&DataKey::Pool(token.clone()))
            .unwrap_or_else(|| panic!("Pool not found"));

        if pool.fees_collected < amount {
            panic!("Insufficient fees collected");
        }

        pool.fees_collected -= amount;
        pool.total_liquidity -= amount;
        env.storage()
            .persistent()
            .set(&DataKey::Pool(token.clone()), &pool);

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &admin, &amount);
    }

    pub fn get_pool(env: Env, token: Address) -> Option<LiquidityPool> {
        env.storage().persistent().get(&DataKey::Pool(token))
    }

    pub fn get_flash_loan(env: Env, loan_id: u64) -> Option<FlashLoanRecord> {
        env.storage()
            .persistent()
            .get(&DataKey::FlashLoanRecord(loan_id))
    }

    pub fn get_config(env: Env) -> FlashLoanConfig {
        env.storage().persistent().get(&DataKey::Config).unwrap()
    }

    pub fn get_analytics(env: Env) -> FlashLoanAnalytics {
        env.storage().persistent().get(&DataKey::Analytics).unwrap()
    }

    pub fn get_all_pools(env: Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::PoolList)
            .unwrap_or(Vec::new(&env))
    }

    pub fn calculate_fee(env: Env, amount: i128) -> i128 {
        let config: FlashLoanConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        (amount * config.fee_bps as i128) / BASIS_POINTS
    }

    pub fn get_lender_position(
        env: Env,
        token: Address,
        lender: Address,
    ) -> Option<LenderPosition> {
        env.storage()
            .persistent()
            .get(&DataKeyKey::LenderPosition(token, lender))
    }

    fn get_or_create_pool(env: &Env, token: &Address) -> LiquidityPool {
        if let Some(pool) = env
            .storage()
            .persistent()
            .get(&DataKey::Pool(token.clone()))
        {
            return pool;
        }

        let mut pool_list: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::PoolList)
            .unwrap_or(Vec::new(env));

        if !pool_list.contains(token) {
            pool_list.push_back(token.clone());
            env.storage()
                .persistent()
                .set(&DataKey::PoolList, &pool_list);
        }

        LiquidityPool {
            token: token.clone(),
            total_liquidity: 0,
            available_liquidity: 0,
            total_borrowed: 0,
            fees_collected: 0,
            lenders: Vec::new(env),
        }
    }

    fn update_analytics(env: &Env, amount: i128, fee: i128, repaid: bool) {
        let mut analytics: FlashLoanAnalytics =
            env.storage().persistent().get(&DataKey::Analytics).unwrap();

        analytics.total_loans += 1;
        analytics.total_volume_borrowed += amount;
        analytics.total_fees_collected += fee;

        if repaid {
            analytics.total_repaid += amount + fee;
        } else {
            analytics.defaulted_loans += 1;
        }

        env.storage()
            .persistent()
            .set(&DataKey::Analytics, &analytics);
    }

    fn assert_admin(env: &Env, user: &Address) {
        let config: FlashLoanConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        if config.admin != *user {
            panic!("Admin only");
        }
    }

    fn assert_not_paused(env: &Env) {
        let config: FlashLoanConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        if config.paused {
            panic!("Contract is paused");
        }
    }

    fn assert_not_reentrant(env: &Env) {
        let guard: bool = env
            .storage()
            .persistent()
            .get(&DataKey::ReentrancyGuard)
            .unwrap_or(false);
        if guard {
            panic!("Reentrancy detected");
        }
    }

    fn set_reentrancy_guard(env: &Env, value: bool) {
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyGuard, &value);
    }
}

#[contracttype]
pub enum DataKeyKey {
    LenderPosition(Address, Address),
}

#[cfg(test)]
mod test;
