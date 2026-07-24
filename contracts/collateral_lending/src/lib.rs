#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, IntoVal, Symbol, Vec,
};

const BASIS_POINTS: i128 = 10_000;
const MAX_RESERVES: u32 = 32;
const PRICE_DECIMALS: i128 = 100_000_000;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReserveConfig {
    pub collateral_factor_bps: u32,
    pub liquidation_threshold_bps: u32,
    pub liquidation_bonus_bps: u32,
    pub base_rate_bps: u32,
    pub slope1_bps: u32,
    pub slope2_bps: u32,
    pub optimal_utilization_bps: u32,
    pub reserve_factor_bps: u32,
    pub is_active: bool,
    pub is_frozen: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReserveData {
    pub total_liquidity: i128,
    pub available_liquidity: i128,
    pub total_borrows: i128,
    pub current_borrow_rate_bps: u32,
    pub last_update_timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserDeposit {
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserBorrow {
    pub amount: i128,
    pub accumulated_interest: i128,
    pub last_accrual_timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserPosition {
    pub total_collateral_value: i128,
    pub total_borrow_value: i128,
    pub health_factor_bps: i128,
}

#[contracttype]
pub enum DataKey {
    Config,
    ReserveConfig(Address),
    ReserveData(Address),
    UserDeposit(Address, Address),
    UserBorrow(Address, Address),
    IsCollateral(Address, Address),
    OracleContract,
    ReserveList,
}

#[contract]
pub struct CollateralLendingContract;

#[contractimpl]
impl CollateralLendingContract {
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Config) {
            panic!("Already initialized");
        }
        let config = Config { admin };
        env.storage().instance().set(&DataKey::Config, &config);
        env.storage()
            .persistent()
            .set(&DataKey::ReserveList, &Vec::<Address>::new(&env));
    }

    pub fn set_oracle(env: Env, admin: Address, oracle_contract: Address) {
        admin.require_auth();
        Self::require_initialized(&env);
        env.storage()
            .instance()
            .set(&DataKey::OracleContract, &oracle_contract);
    }

    pub fn get_oracle(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::OracleContract)
    }

    pub fn configure_reserve(
        env: Env,
        admin: Address,
        asset: Address,
        config: ReserveConfig,
    ) {
        admin.require_auth();
        Self::require_initialized(&env);

        if env
            .storage()
            .persistent()
            .has(&DataKey::ReserveConfig(asset.clone()))
        {
            panic!("Reserve already configured");
        }
        Self::validate_reserve_config(&config);

        let mut reserve_list: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::ReserveList)
            .unwrap_or(Vec::new(&env));

        if (reserve_list.len() as u32) >= MAX_RESERVES {
            panic!("Max reserves exceeded");
        }
        reserve_list.push_back(asset.clone());
        env.storage()
            .persistent()
            .set(&DataKey::ReserveList, &reserve_list);

        env.storage()
            .persistent()
            .set(&DataKey::ReserveConfig(asset.clone()), &config);

        let reserve_data = ReserveData {
            total_liquidity: 0,
            available_liquidity: 0,
            total_borrows: 0,
            current_borrow_rate_bps: 0,
            last_update_timestamp: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::ReserveData(asset), &reserve_data);
    }

    pub fn update_reserve_config(
        env: Env,
        admin: Address,
        asset: Address,
        config: ReserveConfig,
    ) {
        admin.require_auth();
        Self::require_initialized(&env);

        if !env
            .storage()
            .persistent()
            .has(&DataKey::ReserveConfig(asset.clone()))
        {
            panic!("Reserve not configured");
        }
        Self::validate_reserve_config(&config);
        env.storage()
            .persistent()
            .set(&DataKey::ReserveConfig(asset), &config);
    }

    pub fn deposit(env: Env, from: Address, asset: Address, amount: i128) {
        from.require_auth();
        Self::require_initialized(&env);
        if amount <= 0 {
            panic!("Invalid amount");
        }

        let config: ReserveConfig = Self::get_reserve_config_or_panic(&env, &asset);
        if !config.is_active {
            panic!("Asset not active");
        }
        if config.is_frozen {
            panic!("Reserve is frozen");
        }

        let token_client = token::Client::new(&env, &asset);
        token_client.transfer(&from, &env.current_contract_address(), &amount);

        let mut reserve_data: ReserveData = env
            .storage()
            .persistent()
            .get(&DataKey::ReserveData(asset.clone()))
            .unwrap();
        reserve_data.total_liquidity += amount;
        reserve_data.available_liquidity += amount;
        reserve_data.last_update_timestamp = env.ledger().timestamp();
        env.storage()
            .persistent()
            .set(&DataKey::ReserveData(asset.clone()), &reserve_data);

        let key = DataKey::UserDeposit(from.clone(), asset.clone());
        let mut deposit: UserDeposit = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(UserDeposit { amount: 0 });
        deposit.amount += amount;
        env.storage().persistent().set(&key, &deposit);
    }

    pub fn withdraw(env: Env, from: Address, asset: Address, amount: i128) {
        from.require_auth();
        Self::require_initialized(&env);
        if amount <= 0 {
            panic!("Invalid amount");
        }

        let config: ReserveConfig = Self::get_reserve_config_or_panic(&env, &asset);
        if config.is_frozen {
            panic!("Reserve is frozen");
        }

        let deposit_key = DataKey::UserDeposit(from.clone(), asset.clone());
        let mut deposit: UserDeposit = env
            .storage()
            .persistent()
            .get(&deposit_key)
            .unwrap_or(UserDeposit { amount: 0 });

        if deposit.amount < amount {
            panic!("Insufficient balance");
        }

        let is_collateral_key = DataKey::IsCollateral(from.clone(), asset.clone());
        if env
            .storage()
            .persistent()
            .get(&is_collateral_key)
            .unwrap_or(false)
        {
            let withdraw_after = deposit.amount - amount;
            if withdraw_after > 0 {
                Self::enforce_health_factor(&env, &from);
            }
        }

        let mut reserve_data: ReserveData = env
            .storage()
            .persistent()
            .get(&DataKey::ReserveData(asset.clone()))
            .unwrap();

        if reserve_data.available_liquidity < amount {
            panic!("Insufficient liquidity");
        }

        deposit.amount -= amount;
        if deposit.amount == 0 {
            env.storage().persistent().remove(&deposit_key);
        } else {
            env.storage().persistent().set(&deposit_key, &deposit);
        }

        reserve_data.total_liquidity -= amount;
        reserve_data.available_liquidity -= amount;
        env.storage()
            .persistent()
            .set(&DataKey::ReserveData(asset.clone()), &reserve_data);

        let token_client = token::Client::new(&env, &asset);
        token_client.transfer(&env.current_contract_address(), &from, &amount);
    }

    pub fn enable_collateral(env: Env, user: Address, asset: Address) {
        user.require_auth();
        Self::require_initialized(&env);

        let config: ReserveConfig = Self::get_reserve_config_or_panic(&env, &asset);
        if !config.is_active {
            panic!("Asset not active");
        }

        let deposit_key = DataKey::UserDeposit(user.clone(), asset.clone());
        let deposit: UserDeposit = env
            .storage()
            .persistent()
            .get(&deposit_key)
            .unwrap_or(UserDeposit { amount: 0 });
        if deposit.amount <= 0 {
            panic!("Insufficient balance");
        }

        let collateral_key = DataKey::IsCollateral(user.clone(), asset.clone());
        if env.storage().persistent().has(&collateral_key) {
            panic!("Collateral already enabled");
        }
        env.storage()
            .persistent()
            .set(&collateral_key, &true);
    }

    pub fn disable_collateral(env: Env, user: Address, asset: Address) {
        user.require_auth();
        Self::require_initialized(&env);

        let collateral_key = DataKey::IsCollateral(user.clone(), asset.clone());
        if !env.storage().persistent().has(&collateral_key) {
            panic!("Collateral not enabled");
        }
        Self::enforce_health_factor(&env, &user);
        env.storage().persistent().remove(&collateral_key);
    }

    pub fn borrow(env: Env, borrower: Address, asset: Address, amount: i128) {
        borrower.require_auth();
        Self::require_initialized(&env);
        if amount <= 0 {
            panic!("Invalid amount");
        }

        let config: ReserveConfig = Self::get_reserve_config_or_panic(&env, &asset);
        if !config.is_active || config.is_frozen {
            panic!("Asset not available for borrowing");
        }

        let mut reserve_data: ReserveData = env
            .storage()
            .persistent()
            .get(&DataKey::ReserveData(asset.clone()))
            .unwrap();

        if reserve_data.available_liquidity < amount {
            panic!("Insufficient liquidity");
        }

        Self::accrue_interest(&env, &asset, &mut reserve_data);
        Self::enforce_health_factor(&env, &borrower);

        let borrow_key = DataKey::UserBorrow(borrower.clone(), asset.clone());
        let mut user_borrow: UserBorrow = env
            .storage()
            .persistent()
            .get(&borrow_key)
            .unwrap_or(UserBorrow {
                amount: 0,
                accumulated_interest: 0,
                last_accrual_timestamp: env.ledger().timestamp(),
            });

        user_borrow.amount += amount;
        user_borrow.last_accrual_timestamp = env.ledger().timestamp();
        env.storage()
            .persistent()
            .set(&borrow_key, &user_borrow);

        reserve_data.total_borrows += amount;
        reserve_data.available_liquidity -= amount;
        reserve_data.current_borrow_rate_bps =
            Self::calculate_borrow_rate(&config, &reserve_data);
        env.storage()
            .persistent()
            .set(&DataKey::ReserveData(asset.clone()), &reserve_data);

        let token_client = token::Client::new(&env, &asset);
        token_client.transfer(&env.current_contract_address(), &borrower, &amount);
    }

    pub fn repay(
        env: Env,
        repayer: Address,
        borrower: Address,
        asset: Address,
        amount: i128,
    ) -> i128 {
        repayer.require_auth();
        Self::require_initialized(&env);
        if amount <= 0 {
            panic!("Invalid amount");
        }

        let config: ReserveConfig = Self::get_reserve_config_or_panic(&env, &asset);
        if !config.is_active {
            panic!("Asset not active");
        }

        let borrow_key = DataKey::UserBorrow(borrower.clone(), asset.clone());
        let mut user_borrow: UserBorrow = env
            .storage()
            .persistent()
            .get(&borrow_key)
            .unwrap_or(UserBorrow {
                amount: 0,
                accumulated_interest: 0,
                last_accrual_timestamp: env.ledger().timestamp(),
            });

        let mut reserve_data: ReserveData = env
            .storage()
            .persistent()
            .get(&DataKey::ReserveData(asset.clone()))
            .unwrap();

        Self::accrue_interest(&env, &asset, &mut reserve_data);

        let total_debt = user_borrow.amount + user_borrow.accumulated_interest;
        if total_debt <= 0 {
            panic!("No outstanding debt");
        }

        let repay_amount = if amount > total_debt {
            total_debt
        } else {
            amount
        };

        let token_client = token::Client::new(&env, &asset);
        token_client
            .transfer(&repayer, &env.current_contract_address(), &repay_amount);

        let mut principal_paid: i128 = repay_amount;

        if user_borrow.accumulated_interest > 0 {
            let interest_paid = if repay_amount < user_borrow.accumulated_interest {
                repay_amount
            } else {
                user_borrow.accumulated_interest
            };
            user_borrow.accumulated_interest -= interest_paid;
            principal_paid = repay_amount - interest_paid;
        }

        user_borrow.amount -= principal_paid;

        if user_borrow.amount <= 0 && user_borrow.accumulated_interest <= 0 {
            env.storage().persistent().remove(&borrow_key);
        } else {
            user_borrow.last_accrual_timestamp = env.ledger().timestamp();
            env.storage().persistent().set(&borrow_key, &user_borrow);
        }

        reserve_data.total_borrows -= principal_paid;
        reserve_data.available_liquidity += repay_amount;
        reserve_data.total_liquidity += repay_amount;
        reserve_data.current_borrow_rate_bps =
            Self::calculate_borrow_rate(&config, &reserve_data);
        env.storage()
            .persistent()
            .set(&DataKey::ReserveData(asset), &reserve_data);

        repay_amount
    }

    pub fn liquidate(
        env: Env,
        liquidator: Address,
        borrower: Address,
        asset: Address,
        repay_amount: i128,
    ) -> i128 {
        liquidator.require_auth();
        Self::require_initialized(&env);
        if repay_amount <= 0 {
            panic!("Invalid amount");
        }

        let config: ReserveConfig = Self::get_reserve_config_or_panic(&env, &asset);
        if !config.is_active {
            panic!("Asset not active");
        }

        let borrow_key = DataKey::UserBorrow(borrower.clone(), asset.clone());
        let mut user_borrow: UserBorrow = env
            .storage()
            .persistent()
            .get(&borrow_key)
            .unwrap_or(UserBorrow {
                amount: 0,
                accumulated_interest: 0,
                last_accrual_timestamp: env.ledger().timestamp(),
            });

        let mut reserve_data: ReserveData = env
            .storage()
            .persistent()
            .get(&DataKey::ReserveData(asset.clone()))
            .unwrap();

        Self::accrue_interest(&env, &asset, &mut reserve_data);

        let total_debt = user_borrow.amount + user_borrow.accumulated_interest;
        if total_debt <= 0 {
            panic!("No outstanding debt");
        }

        let health = Self::compute_health_factor(&env, &borrower);
        if health.health_factor_bps >= BASIS_POINTS {
            panic!("Position healthy");
        }

        let actual_repay = if repay_amount > total_debt {
            total_debt
        } else {
            repay_amount
        };

        let token_client = token::Client::new(&env, &asset);
        token_client
            .transfer(&liquidator, &env.current_contract_address(), &actual_repay);

        let mut principal_paid: i128 = actual_repay;

        if user_borrow.accumulated_interest > 0 {
            let interest_paid = if actual_repay < user_borrow.accumulated_interest {
                actual_repay
            } else {
                user_borrow.accumulated_interest
            };
            user_borrow.accumulated_interest -= interest_paid;
            principal_paid = actual_repay - interest_paid;
        }

        user_borrow.amount -= principal_paid;

        if user_borrow.amount <= 0 && user_borrow.accumulated_interest <= 0 {
            env.storage().persistent().remove(&borrow_key);
        } else {
            user_borrow.last_accrual_timestamp = env.ledger().timestamp();
            env.storage().persistent().set(&borrow_key, &user_borrow);
        }

        reserve_data.total_borrows -= principal_paid;
        reserve_data.available_liquidity += actual_repay;
        reserve_data.total_liquidity += actual_repay;
        reserve_data.current_borrow_rate_bps =
            Self::calculate_borrow_rate(&config, &reserve_data);
        env.storage()
            .persistent()
            .set(&DataKey::ReserveData(asset.clone()), &reserve_data);

        let bonus = (actual_repay * config.liquidation_bonus_bps as i128) / BASIS_POINTS;
        let collateral_seized = actual_repay + bonus;

        Self::seize_collateral(&env, &borrower, &liquidator, &asset, collateral_seized);

        actual_repay
    }

    pub fn get_health_factor(env: Env, user: Address) -> UserPosition {
        Self::require_initialized(&env);
        Self::compute_health_factor(&env, &user)
    }

    pub fn get_user_deposit(env: Env, user: Address, asset: Address) -> UserDeposit {
        env.storage()
            .persistent()
            .get(&DataKey::UserDeposit(user, asset))
            .unwrap_or(UserDeposit { amount: 0 })
    }

    pub fn get_user_borrow(env: Env, user: Address, asset: Address) -> UserBorrow {
        env.storage()
            .persistent()
            .get(&DataKey::UserBorrow(user, asset))
            .unwrap_or(UserBorrow {
                amount: 0,
                accumulated_interest: 0,
                last_accrual_timestamp: env.ledger().timestamp(),
            })
    }

    pub fn get_reserve_data(env: Env, asset: Address) -> Option<ReserveData> {
        env.storage()
            .persistent()
            .get(&DataKey::ReserveData(asset))
    }

    pub fn get_reserve_config(env: Env, asset: Address) -> Option<ReserveConfig> {
        env.storage()
            .persistent()
            .get(&DataKey::ReserveConfig(asset))
    }

    pub fn get_reserve_list(env: Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::ReserveList)
            .unwrap_or(Vec::new(&env))
    }

    pub fn is_asset_collateral(env: Env, user: Address, asset: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::IsCollateral(user, asset))
            .unwrap_or(false)
    }

    pub fn get_simulation_rate(
        env: Env,
        asset: Address,
        total_liquidity: i128,
        total_borrows: i128,
    ) -> u32 {
        let config: ReserveConfig = Self::get_reserve_config_or_panic(&env, &asset);
        let reserve = ReserveData {
            total_liquidity,
            available_liquidity: total_liquidity - total_borrows,
            total_borrows,
            current_borrow_rate_bps: 0,
            last_update_timestamp: 0,
        };
        Self::calculate_borrow_rate(&config, &reserve)
    }

    fn require_initialized(env: &Env) {
        if !env.storage().instance().has(&DataKey::Config) {
            panic!("Not initialized");
        }
    }

    fn get_reserve_config_or_panic(env: &Env, asset: &Address) -> ReserveConfig {
        env.storage()
            .persistent()
            .get(&DataKey::ReserveConfig(asset.clone()))
            .unwrap_or_else(|| panic!("Reserve not configured"))
    }

    fn validate_reserve_config(config: &ReserveConfig) {
        if config.collateral_factor_bps > BASIS_POINTS as u32 {
            panic!("Invalid collateral factor");
        }
        if config.liquidation_threshold_bps > BASIS_POINTS as u32 {
            panic!("Invalid liquidation threshold");
        }
        if config.liquidation_bonus_bps > 1000 {
            panic!("Invalid liquidation bonus");
        }
        if config.reserve_factor_bps > BASIS_POINTS as u32 {
            panic!("Invalid reserve factor");
        }
        if config.collateral_factor_bps > config.liquidation_threshold_bps {
            panic!("Collateral factor must be <= liquidation threshold");
        }
    }

    fn accrue_interest(env: &Env, asset: &Address, reserve: &mut ReserveData) {
        let now = env.ledger().timestamp();
        if now <= reserve.last_update_timestamp {
            return;
        }
        if reserve.total_borrows == 0 {
            reserve.last_update_timestamp = now;
            return;
        }

        let delta = now - reserve.last_update_timestamp;
        let rate = reserve.current_borrow_rate_bps as i128;
        let interest = (reserve.total_borrows * rate * delta as i128)
            / (BASIS_POINTS * 365 * 24 * 60 * 60);
        if interest > 0 {
            let rf = Self::get_reserve_config_or_panic(env, asset).reserve_factor_bps as i128;
            let reserve_interest = (interest * rf) / BASIS_POINTS;
            let borrower_interest = interest - reserve_interest;

            reserve.total_borrows += borrower_interest;
            reserve.total_liquidity += reserve_interest;
        }
        reserve.last_update_timestamp = now;
    }

    fn calculate_borrow_rate(config: &ReserveConfig, reserve: &ReserveData) -> u32 {
        if reserve.total_liquidity == 0 {
            return config.base_rate_bps;
        }
        let utilization =
            (reserve.total_borrows * BASIS_POINTS) / reserve.total_liquidity;
        let opt = config.optimal_utilization_bps as i128;

        if utilization <= opt {
            let rate = config.base_rate_bps as i128
                + ((config.slope1_bps as i128) * utilization) / opt;
            rate.min(5000) as u32
        } else {
            let excess = utilization - opt;
            let range = BASIS_POINTS - opt;
            let rate = config.base_rate_bps as i128
                + config.slope1_bps as i128
                + ((config.slope2_bps as i128) * excess) / range;
            rate.min(5000) as u32
        }
    }

    fn get_asset_price(env: &Env, _asset: &Address) -> i128 {
        let oracle: Address = env
            .storage()
            .instance()
            .get(&DataKey::OracleContract)
            .expect("Oracle not set");

        let asset_sym = Symbol::new(env, "USD");
        let args = (asset_sym,);
        let price: i128 = env.invoke_contract(
            &oracle,
            &Symbol::new(env, "price"),
            args.into_val(env),
        );

        if price <= 0 {
            panic!("Invalid price from oracle");
        }
        price
    }

    fn compute_health_factor(env: &Env, user: &Address) -> UserPosition {
        let reserve_list: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::ReserveList)
            .unwrap_or(Vec::new(env));

        let mut total_collateral_value: i128 = 0;
        let mut total_borrow_value: i128 = 0;

        for asset in reserve_list.iter() {
            let is_collateral: bool = env
                .storage()
                .persistent()
                .get(&DataKey::IsCollateral(user.clone(), asset.clone()))
                .unwrap_or(false);

            if is_collateral {
                let deposit: UserDeposit = env
                    .storage()
                    .persistent()
                    .get(&DataKey::UserDeposit(user.clone(), asset.clone()))
                    .unwrap_or(UserDeposit { amount: 0 });

                let config: ReserveConfig =
                    Self::get_reserve_config_or_panic(env, &asset);

                if deposit.amount > 0 {
                    let price = Self::get_asset_price(env, &asset);
                    let value = (deposit.amount * price) / PRICE_DECIMALS;
                    let weighted = (value * config.liquidation_threshold_bps as i128)
                        / BASIS_POINTS;
                    total_collateral_value += weighted;
                }
            }

            let borrow_key = DataKey::UserBorrow(user.clone(), asset.clone());
            let user_borrow: UserBorrow = env
                .storage()
                .persistent()
                .get(&borrow_key)
                .unwrap_or(UserBorrow {
                    amount: 0,
                    accumulated_interest: 0,
                    last_accrual_timestamp: env.ledger().timestamp(),
                });

            if user_borrow.amount > 0 || user_borrow.accumulated_interest > 0 {
                let total_debt = user_borrow.amount + user_borrow.accumulated_interest;
                let price = Self::get_asset_price(env, &asset);
                let value = (total_debt * price) / PRICE_DECIMALS;
                total_borrow_value += value;
            }
        }

        let health_factor_bps = if total_borrow_value > 0 {
            (total_collateral_value * BASIS_POINTS) / total_borrow_value
        } else {
            i128::MAX
        };

        UserPosition {
            total_collateral_value,
            total_borrow_value,
            health_factor_bps,
        }
    }

    fn enforce_health_factor(env: &Env, user: &Address) {
        let health = Self::compute_health_factor(env, user);
        if health.total_borrow_value > 0 && health.health_factor_bps < BASIS_POINTS {
            panic!("Health factor too low");
        }
    }

    fn seize_collateral(
        env: &Env,
        borrower: &Address,
        liquidator: &Address,
        asset: &Address,
        amount: i128,
    ) {
        let deposit_key = DataKey::UserDeposit(borrower.clone(), asset.clone());
        let mut deposit: UserDeposit = env
            .storage()
            .persistent()
            .get(&deposit_key)
            .unwrap_or(UserDeposit { amount: 0 });

        let seize_amount = if amount > deposit.amount {
            deposit.amount
        } else {
            amount
        };
        if seize_amount <= 0 {
            panic!("No collateral to seize");
        }

        deposit.amount -= seize_amount;
        if deposit.amount == 0 {
            env.storage().persistent().remove(&deposit_key);
            env.storage()
                .persistent()
                .remove(&DataKey::IsCollateral(
                    borrower.clone(),
                    asset.clone(),
                ));
        } else {
            env.storage().persistent().set(&deposit_key, &deposit);
        }

        let mut reserve_data: ReserveData = env
            .storage()
            .persistent()
            .get(&DataKey::ReserveData(asset.clone()))
            .unwrap();

        reserve_data.total_liquidity -= seize_amount;
        reserve_data.available_liquidity -= seize_amount;
        env.storage()
            .persistent()
            .set(&DataKey::ReserveData(asset.clone()), &reserve_data);

        let token_client = token::Client::new(env, asset);
        token_client.transfer(&env.current_contract_address(), liquidator, &seize_amount);
    }
}

#[cfg(test)]
mod test;
