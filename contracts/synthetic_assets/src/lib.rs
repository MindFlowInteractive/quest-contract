#![no_std]

#[cfg(test)]
mod test;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address,
    vec, Env, Symbol, Vec,
};

/// Fixed-point scale. Prices are Q9: `price` is the number of collateral units
/// that one unit of the synth is worth.
pub const SCALE: i128 = 1_000_000_000;
/// Basis-point denominator.
pub const BPS: i128 = 10_000;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
    pub oracle: Address,
    pub min_collateral_bps: u32,
    pub liquidation_threshold_bps: u32,
    pub liquidation_penalty_bps: u32,
    pub max_price_age: u64,
}

/// A tradeable synthetic and the asset that backs it.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Synth {
    pub id: u64,
    pub synth: Address,
    pub collateral: Address,
    pub symbol: Symbol,
    pub price: i128,
    pub liquidity: i128,
    pub total_debt: i128,
    pub updated_at: u64,
    pub active: bool,
}

/// A user's exposure to one synth.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Position {
    pub user: Address,
    pub synth_id: u64,
    pub collateral: i128,
    pub debt: i128,
    pub target_bps: u32,
    pub opened_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Config,
    SynthCount,
    Synth(u64),
    Synths,
    /// (user, synth id) -> collateral
    Collateral(Address, u64),
    /// (user, synth id) -> debt
    Debt(Address, u64),
    /// (user, synth id) -> target ratio in bps
    Target(Address, u64),
    /// (user, synth id) -> open timestamp
    Opened(Address, u64),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidParams = 4,
    SynthNotFound = 5,
    SynthInactive = 6,
    InvalidAmount = 7,
    InsufficientCollateral = 8,
    InsufficientDebt = 9,
    InsufficientLiquidity = 10,
    StalePrice = 11,
    NotLiquidatable = 12,
    PriceMissing = 13,
    PositionHealthy = 15,
    TokenMismatch = 16,
}

const REGISTERED: Symbol = symbol_short!("reg");
const PRICE: Symbol = symbol_short!("price");
const DEPOSIT: Symbol = symbol_short!("deposit");
const REMOVE: Symbol = symbol_short!("remove");
const MINTED: Symbol = symbol_short!("mint");
const BURNT: Symbol = symbol_short!("burn");
const REBAL: Symbol = symbol_short!("rebal");
const LIQUID: Symbol = symbol_short!("liquid");
const PARAM: Symbol = symbol_short!("param");
const ORACLE: Symbol = symbol_short!("oracle");
const FUNDED: Symbol = symbol_short!("funded");

#[contract]
pub struct SyntheticAssets;

// ----------------------------------------------------------------------
// Free helpers
// ----------------------------------------------------------------------

fn load_config(env: &Env) -> Result<Config, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::Config)
        .ok_or(Error::NotInitialized)
}

fn load_synth(env: &Env, id: u64) -> Result<Synth, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::Synth(id))
        .ok_or(Error::SynthNotFound)
}

fn save_synth(env: &Env, synth: &Synth) {
    env.storage().persistent().set(&DataKey::Synth(synth.id), synth);
}

fn load_collateral(env: &Env, user: &Address, id: u64) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::Collateral(user.clone(), id))
        .unwrap_or(0)
}

fn set_collateral(env: &Env, user: &Address, id: u64, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::Collateral(user.clone(), id), &amount);
}

fn load_debt(env: &Env, user: &Address, id: u64) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::Debt(user.clone(), id))
        .unwrap_or(0)
}

fn set_debt(env: &Env, user: &Address, id: u64, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::Debt(user.clone(), id), &amount);
}

fn target_of(env: &Env, user: &Address, id: u64) -> u32 {
    env.storage()
        .persistent()
        .get(&DataKey::Target(user.clone(), id))
        .unwrap_or(0)
}

/// Collateral backing per unit of debt, in basis points.
///
/// Returns `u32::MAX` for a debt-free position, which is always healthy.
fn ratio_bps(collateral: i128, debt: i128, price: i128) -> u32 {
    if debt <= 0 || price <= 0 {
        return u32::MAX;
    }
    let num = collateral * SCALE * BPS;
    let den = debt * price;
    if num / den > u32::MAX as i128 {
        return u32::MAX;
    }
    (num / den) as u32
}

/// Value of `amount` synth in collateral units, Q9.
fn debt_value(amount: i128, price: i128) -> i128 {
    (amount * price) / SCALE
}

/// A price is stale once the oracle has not refreshed it inside the window.
fn price_is_stale(env: &Env, synth: &Synth, max_age: u64) -> bool {
    if synth.price <= 0 {
        return true;
    }
    if max_age == 0 {
        return false;
    }
    env.ledger().timestamp() > synth.updated_at.saturating_add(max_age)
}

fn require_fresh(env: &Env, synth: &Synth, max_age: u64) -> Result<(), Error> {
    if synth.price <= 0 {
        return Err(Error::PriceMissing);
    }
    if price_is_stale(env, synth, max_age) {
        return Err(Error::StalePrice);
    }
    Ok(())
}

/// Largest debt a user may take on, in synth units, given their collateral.
fn borrowing_power(collateral: i128, price: i128, min_bps: u32) -> i128 {
    if price <= 0 || min_bps == 0 {
        return 0;
    }
    // collateral * SCALE * BPS >= debt * price * min_bps
    (collateral * SCALE * BPS) / (price * min_bps as i128)
}

fn collateral_token<'a>(env: &'a Env, synth: &Synth) -> token::Client<'a> {
    token::Client::new(env, &synth.collateral)
}

fn synth_token<'a>(env: &'a Env, synth: &Synth) -> token::Client<'a> {
    token::Client::new(env, &synth.synth)
}

#[contractimpl]
impl SyntheticAssets {
    // ------------------------------------------------------------------
    // Setup
    // ------------------------------------------------------------------

    /// Deploy-time configuration. May only be called once.
    pub fn initialize(
        env: Env,
        admin: Address,
        oracle: Address,
        min_collateral_bps: u32,
        liquidation_threshold_bps: u32,
        liquidation_penalty_bps: u32,
        max_price_age: u64,
    ) -> Result<(), Error> {
        if env.storage().persistent().has(&DataKey::Config) {
            return Err(Error::AlreadyInitialized);
        }
        if min_collateral_bps == 0 || min_collateral_bps > BPS as u32 {
            return Err(Error::InvalidParams);
        }
        // Liquidation has to trigger strictly before the position is underwater.
        if liquidation_threshold_bps >= min_collateral_bps {
            return Err(Error::InvalidParams);
        }
        if liquidation_penalty_bps >= BPS as u32 {
            return Err(Error::InvalidParams);
        }
        admin.require_auth();

        let config = Config {
            admin,
            oracle,
            min_collateral_bps,
            liquidation_threshold_bps,
            liquidation_penalty_bps,
            max_price_age,
        };
        env.storage().persistent().set(&DataKey::Config, &config);
        Ok(())
    }

    /// Retune the risk parameters.
    pub fn set_parameters(
        env: Env,
        min_collateral_bps: u32,
        liquidation_threshold_bps: u32,
        liquidation_penalty_bps: u32,
        max_price_age: u64,
    ) -> Result<(), Error> {
        let mut config = load_config(&env)?;
        config.admin.require_auth();

        if min_collateral_bps == 0 || min_collateral_bps > BPS as u32 {
            return Err(Error::InvalidParams);
        }
        if liquidation_threshold_bps >= min_collateral_bps {
            return Err(Error::InvalidParams);
        }
        if liquidation_penalty_bps >= BPS as u32 {
            return Err(Error::InvalidParams);
        }

        config.min_collateral_bps = min_collateral_bps;
        config.liquidation_threshold_bps = liquidation_threshold_bps;
        config.liquidation_penalty_bps = liquidation_penalty_bps;
        config.max_price_age = max_price_age;
        env.storage().persistent().set(&DataKey::Config, &config);
        env.events().publish((PARAM,), min_collateral_bps);
        Ok(())
    }

    /// Point the contract at a different price feed.
    pub fn set_oracle(env: Env, oracle: Address) -> Result<(), Error> {
        let mut config = load_config(&env)?;
        config.admin.require_auth();
        config.oracle = oracle.clone();
        env.storage().persistent().set(&DataKey::Config, &config);
        env.events().publish((ORACLE,), oracle);
        Ok(())
    }

    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        let mut config = load_config(&env)?;
        config.admin.require_auth();
        config.admin = new_admin.clone();
        env.storage().persistent().set(&DataKey::Config, &config);
        env.events().publish((ORACLE,), new_admin);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Synth registry
    // ------------------------------------------------------------------

    /// List a new synthetic asset. Returns its id.
    pub fn register_synth(
        env: Env,
        synth: Address,
        collateral: Address,
        symbol: Symbol,
        initial_price: i128,
    ) -> Result<u64, Error> {
        let config = load_config(&env)?;
        config.admin.require_auth();

        if synth == collateral {
            return Err(Error::TokenMismatch);
        }
        if initial_price <= 0 {
            return Err(Error::InvalidParams);
        }

        let id: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::SynthCount)
            .unwrap_or(0)
            + 1;

        let entry = Synth {
            id,
            synth,
            collateral,
            symbol,
            price: initial_price,
            liquidity: 0,
            total_debt: 0,
            updated_at: env.ledger().timestamp(),
            active: true,
        };

        save_synth(&env, &entry);
        env.storage().persistent().set(&DataKey::SynthCount, &id);

        let mut all: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::Synths)
            .unwrap_or(Vec::new(&env));
        all.push_back(id);
        env.storage().persistent().set(&DataKey::Synths, &all);

        env.events().publish((REGISTERED, id), entry);
        Ok(id)
    }

    /// Pause or resume minting on a synth.
    pub fn set_synth_active(env: Env, synth_id: u64, active: bool) -> Result<(), Error> {
        let config = load_config(&env)?;
        config.admin.require_auth();

        let mut synth = load_synth(&env, synth_id)?;
        synth.active = active;
        save_synth(&env, &synth);
        env.events().publish((PARAM, synth_id), active);
        Ok(())
    }

    /// Add synth tokens to the contract so it can pay out new positions.
    pub fn add_synth_liquidity(
        env: Env,
        provider: Address,
        synth_id: u64,
        amount: i128,
    ) -> Result<(), Error> {
        provider.require_auth();
        let mut synth = load_synth(&env, synth_id)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let client = synth_token(&env, &synth);
        client.transfer(&provider, &env.current_contract_address(), &amount);
        synth.liquidity += amount;
        save_synth(&env, &synth);

        env.events().publish((FUNDED, synth_id), amount);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Price feed
    // ------------------------------------------------------------------

    /// Push a new price. Callable by the configured oracle (the usual Stellar
    /// pattern) or by the admin.
    pub fn update_price(env: Env, synth_id: u64, price: i128) -> Result<(), Error> {
        let config = load_config(&env)?;
        // The configured feed is the only address allowed to push a price, which
        // matches the Stellar oracle -> consumer calling convention.
        config.oracle.require_auth();
        if price <= 0 {
            return Err(Error::InvalidParams);
        }

        let mut synth = load_synth(&env, synth_id)?;
        synth.price = price;
        synth.updated_at = env.ledger().timestamp();
        save_synth(&env, &synth);

        env.events().publish((PRICE, synth_id), (price, synth.updated_at));
        Ok(())
    }

    /// Pull the current price from the oracle contract. Useful for keepers who
    /// would rather refresh every synth in one call.
    pub fn sync_price(env: Env, synth_id: u64) -> Result<i128, Error> {
        let config = load_config(&env)?;
        config.admin.require_auth();

        let synth = load_synth(&env, synth_id)?;
        let price: i128 = env.invoke_contract(
            &config.oracle,
            &Symbol::new(&env, "lastprice"),
            vec![&env, synth.synth.to_val()],
        );
        if price <= 0 {
            return Err(Error::PriceMissing);
        }

        let mut synth = synth;
        synth.price = price;
        synth.updated_at = env.ledger().timestamp();
        save_synth(&env, &synth);

        env.events().publish((PRICE, synth_id), (price, synth.updated_at));
        Ok(price)
    }

    // ------------------------------------------------------------------
    // Collateral
    // ------------------------------------------------------------------

    /// Lock collateral against a synth position.
    pub fn add_collateral(
        env: Env,
        user: Address,
        synth_id: u64,
        amount: i128,
    ) -> Result<(), Error> {
        user.require_auth();
        let synth = load_synth(&env, synth_id)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        collateral_token(&env, &synth).transfer(
            &user,
            &env.current_contract_address(),
            &amount,
        );

        let total = load_collateral(&env, &user, synth_id) + amount;
        set_collateral(&env, &user, synth_id, total);
        if load_debt(&env, &user, synth_id) == 0 {
            env.storage()
                .persistent()
                .set(&DataKey::Opened(user.clone(), synth_id), &env.ledger().timestamp());
        }

        env.events()
            .publish((DEPOSIT, synth_id), (user.clone(), amount, total));
        Ok(())
    }

    /// Release collateral, refusing to leave the position under-collateralised.
    pub fn remove_collateral(
        env: Env,
        user: Address,
        synth_id: u64,
        amount: i128,
        min_ratio_bps: u32,
    ) -> Result<(), Error> {
        user.require_auth();
        let synth = load_synth(&env, synth_id)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let held = load_collateral(&env, &user, synth_id);
        if amount > held {
            return Err(Error::InsufficientCollateral);
        }
        let remaining = held - amount;
        let debt = load_debt(&env, &user, synth_id);
        if ratio_bps(remaining, debt, synth.price) < min_ratio_bps {
            return Err(Error::PositionHealthy);
        }

        set_collateral(&env, &user, synth_id, remaining);
        collateral_token(&env, &synth).transfer(
            &env.current_contract_address(),
            &user,
            &amount,
        );

        env.events()
            .publish((REMOVE, synth_id), (user.clone(), amount, remaining));
        Ok(())
    }

    // ------------------------------------------------------------------
    // Mint and burn
    // ------------------------------------------------------------------

    /// Borrow synth against collateral.
    pub fn mint_synth(env: Env, user: Address, synth_id: u64, amount: i128) -> Result<(), Error> {
        user.require_auth();
        let config = load_config(&env)?;

        let mut synth = load_synth(&env, synth_id)?;
        if !synth.active {
            return Err(Error::SynthInactive);
        }
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        require_fresh(&env, &synth, config.max_price_age)?;

        if synth.liquidity < amount {
            return Err(Error::InsufficientLiquidity);
        }

        let held = load_collateral(&env, &user, synth_id);
        let debt = load_debt(&env, &user, synth_id);
        let new_debt = debt + amount;
        if ratio_bps(held, new_debt, synth.price) < config.min_collateral_bps {
            return Err(Error::InsufficientCollateral);
        }

        set_debt(&env, &user, synth_id, new_debt);
        if debt == 0 {
            env.storage()
                .persistent()
                .set(&DataKey::Opened(user.clone(), synth_id), &env.ledger().timestamp());
        }

        synth.liquidity -= amount;
        synth.total_debt += amount;
        save_synth(&env, &synth);

        synth_token(&env, &synth).transfer(
            &env.current_contract_address(),
            &user,
            &amount,
        );

        env.events()
            .publish((MINTED, synth_id), (user.clone(), amount, new_debt));
        Ok(())
    }

    /// Repay synth debt and return the equivalent collateral.
    pub fn burn_synth(env: Env, user: Address, synth_id: u64, amount: i128) -> Result<i128, Error> {
        user.require_auth();
        let config = load_config(&env)?;

        let mut synth = load_synth(&env, synth_id)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        require_fresh(&env, &synth, config.max_price_age)?;

        let debt = load_debt(&env, &user, synth_id);
        if amount > debt {
            return Err(Error::InsufficientDebt);
        }

        let held = load_collateral(&env, &user, synth_id);
        let mut released = debt_value(amount, synth.price);
        if released > held {
            released = held;
        }
        let remaining_debt = debt - amount;
        let remaining_collateral = held - released;

        // Burning releases collateral proportionally, so a healthy position
        // stays healthy; refuse to strand one that is already underwater.
        if ratio_bps(remaining_collateral, remaining_debt, synth.price)
            < config.liquidation_threshold_bps
        {
            return Err(Error::InsufficientCollateral);
        }

        set_debt(&env, &user, synth_id, remaining_debt);
        set_collateral(&env, &user, synth_id, remaining_collateral);

        synth.liquidity += amount;
        synth.total_debt -= amount;
        save_synth(&env, &synth);

        synth_token(&env, &synth).transfer(&user, &env.current_contract_address(), &amount);
        collateral_token(&env, &synth).transfer(
            &env.current_contract_address(),
            &user,
            &released,
        );

        env.events()
            .publish((BURNT, synth_id), (user.clone(), amount, released));
        Ok(released)
    }

    // ------------------------------------------------------------------
    // Rebalancing
    // ------------------------------------------------------------------

    /// Choose the collateral ratio this position should sit at.
    pub fn set_target_ratio(
        env: Env,
        user: Address,
        synth_id: u64,
        target_bps: u32,
    ) -> Result<(), Error> {
        user.require_auth();
        let config = load_config(&env)?;
        if target_bps < config.min_collateral_bps {
            return Err(Error::InvalidParams);
        }
        env.storage()
            .persistent()
            .set(&DataKey::Target(user.clone(), synth_id), &target_bps);
        env.events().publish((REBAL, synth_id), target_bps);
        Ok(())
    }

    /// Move the position towards its target ratio.
    ///
    /// Over-collateralised positions mint more synth against the idle
    /// collateral; under-collateralised ones burn synth to claw back the
    /// shortfall. Returns the signed synth amount moved: positive for a mint,
    /// negative for a burn.
    pub fn rebalance(env: Env, user: Address, synth_id: u64) -> Result<i128, Error> {
        user.require_auth();
        let config = load_config(&env)?;

        let mut synth = load_synth(&env, synth_id)?;
        if !synth.active {
            return Err(Error::SynthInactive);
        }
        require_fresh(&env, &synth, config.max_price_age)?;

        let target = target_of(&env, &user, synth_id);
        if target == 0 {
            return Err(Error::InvalidParams);
        }
        if target < config.min_collateral_bps {
            return Err(Error::InvalidParams);
        }

        let held = load_collateral(&env, &user, synth_id);
        let debt = load_debt(&env, &user, synth_id);
        if held == 0 && debt == 0 {
            return Err(Error::InvalidAmount);
        }

        let current = ratio_bps(held, debt, synth.price);

        if current > target {
            // Too much collateral: mint synth until the ratio lands on target.
            let target_debt = borrowing_power(held, synth.price, target);
            if target_debt <= debt {
                return Ok(0);
            }
            let amount = target_debt - debt;
            if synth.liquidity < amount {
                return Err(Error::InsufficientLiquidity);
            }

            set_debt(&env, &user, synth_id, debt + amount);
            synth.liquidity -= amount;
            synth.total_debt += amount;
            save_synth(&env, &synth);
            synth_token(&env, &synth).transfer(
                &env.current_contract_address(),
                &user,
                &amount,
            );

            env.events().publish((REBAL, synth_id), (user.clone(), amount));
            Ok(amount)
        } else if current < target {
            // Too little collateral: burn synth to restore the ratio. The
            // collateral released goes straight back to the user.
            let target_debt = borrowing_power(held, synth.price, target);
            if target_debt >= debt {
                return Ok(0);
            }
            let amount = debt - target_debt;
            // The holder pays synth in, so no contract liquidity is needed.

            let mut released = debt_value(amount, synth.price);
            if released > held {
                released = held;
            }
            let remaining = held - released;
            if ratio_bps(remaining, target_debt, synth.price) < config.min_collateral_bps {
                return Err(Error::InsufficientCollateral);
            }

            set_debt(&env, &user, synth_id, target_debt);
            set_collateral(&env, &user, synth_id, remaining);

            synth.liquidity += amount;
            synth.total_debt -= amount;
            save_synth(&env, &synth);
            synth_token(&env, &synth).transfer(
                &user,
                &env.current_contract_address(),
                &amount,
            );
            collateral_token(&env, &synth).transfer(
                &env.current_contract_address(),
                &user,
                &released,
            );

            env.events().publish((REBAL, synth_id), (user.clone(), -amount));
            Ok(-amount)
        } else {
            Ok(0)
        }
    }

    // ------------------------------------------------------------------
    // Liquidation
    // ------------------------------------------------------------------

    /// Repay part of an unhealthy position's debt and take its collateral,
    /// plus the liquidation penalty, as a reward.
    ///
    /// Returns the collateral handed to the liquidator.
    pub fn liquidate(
        env: Env,
        liquidator: Address,
        user: Address,
        synth_id: u64,
        amount: i128,
    ) -> Result<i128, Error> {
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if liquidator == user {
            return Err(Error::InvalidParams);
        }

        let config = load_config(&env)?;
        let mut synth = load_synth(&env, synth_id)?;
        // Liquidation must work even on a stale price, otherwise a dead oracle
        // would freeze the whole system.
        if synth.price <= 0 {
            return Err(Error::PriceMissing);
        }

        let held = load_collateral(&env, &user, synth_id);
        let debt = load_debt(&env, &user, synth_id);
        if debt == 0 || held == 0 {
            return Err(Error::NotLiquidatable);
        }
        if ratio_bps(held, debt, synth.price) >= config.liquidation_threshold_bps {
            return Err(Error::PositionHealthy);
        }

        let repay = if amount > debt { debt } else { amount };

        // Collateral worth the repaid debt, grossed up by the penalty.
        let worth = debt_value(repay, synth.price);
        let seize = (worth * (BPS + config.liquidation_penalty_bps as i128)) / BPS;
        let seize = if seize > held { held } else { seize };

        // The remainder is allowed to stay under water: a deeply underwater
        // position has to remain liquidatable in full, otherwise the shortfall
        // would be permanently frozen.
        let remaining_debt = debt - repay;
        let remaining_collateral = held - seize;

        set_debt(&env, &user, synth_id, remaining_debt);
        set_collateral(&env, &user, synth_id, remaining_collateral);

        synth.liquidity += repay;
        synth.total_debt -= repay;
        save_synth(&env, &synth);

        synth_token(&env, &synth).transfer(&liquidator, &env.current_contract_address(), &repay);
        collateral_token(&env, &synth).transfer(
            &env.current_contract_address(),
            &liquidator,
            &seize,
        );

        env.events().publish(
            (LIQUID, synth_id),
            (liquidator.clone(), user.clone(), repay, seize),
        );
        Ok(seize)
    }

    // ------------------------------------------------------------------
    // Views
    // ------------------------------------------------------------------

    /// Collateral per unit of debt, in basis points. `u32::MAX` means healthy.
    pub fn health_factor(env: Env, user: Address, synth_id: u64) -> Result<u32, Error> {
        let synth = load_synth(&env, synth_id)?;
        Ok(ratio_bps(
            load_collateral(&env, &user, synth_id),
            load_debt(&env, &user, synth_id),
            synth.price,
        ))
    }

    /// Whether the position is currently eligible for liquidation.
    pub fn is_liquidatable(env: Env, user: Address, synth_id: u64) -> Result<bool, Error> {
        let config = load_config(&env)?;
        let synth = load_synth(&env, synth_id)?;
        if synth.price <= 0 {
            return Ok(false);
        }
        let ratio = ratio_bps(
            load_collateral(&env, &user, synth_id),
            load_debt(&env, &user, synth_id),
            synth.price,
        );
        Ok(ratio != u32::MAX && ratio < config.liquidation_threshold_bps)
    }

    /// Full position snapshot.
    pub fn position(env: Env, user: Address, synth_id: u64) -> Result<Position, Error> {
        load_synth(&env, synth_id)?;
        let collateral = load_collateral(&env, &user, synth_id);
        let debt = load_debt(&env, &user, synth_id);
        let target_bps = target_of(&env, &user, synth_id);
        let opened_at = env
            .storage()
            .persistent()
            .get(&DataKey::Opened(user.clone(), synth_id))
            .unwrap_or(0);

        Ok(Position {
            user,
            synth_id,
            collateral,
            debt,
            target_bps,
            opened_at,
        })
    }

    pub fn get_synth(env: Env, synth_id: u64) -> Result<Synth, Error> {
        load_synth(&env, synth_id)
    }

    pub fn get_config(env: Env) -> Result<Config, Error> {
        load_config(&env)
    }

    pub fn get_price(env: Env, synth_id: u64) -> Result<i128, Error> {
        Ok(load_synth(&env, synth_id)?.price)
    }

    pub fn is_stale(env: Env, synth_id: u64) -> Result<bool, Error> {
        let config = load_config(&env)?;
        let synth = load_synth(&env, synth_id)?;
        Ok(price_is_stale(&env, &synth, config.max_price_age))
    }

    pub fn collateral_of(env: Env, user: Address, synth_id: u64) -> i128 {
        load_collateral(&env, &user, synth_id)
    }

    pub fn debt_of(env: Env, user: Address, synth_id: u64) -> i128 {
        load_debt(&env, &user, synth_id)
    }

    pub fn synth_count(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::SynthCount)
            .unwrap_or(0)
    }

    /// Every registered synth id.
    pub fn all_synths(env: Env) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::Synths)
            .unwrap_or(Vec::new(&env))
    }
}
