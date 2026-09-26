#![no_std]

#[cfg(test)]
mod test;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, Symbol,
};

/// Fixed-point scale. Prices and ratios are Q9: 1.0 is `SCALE`.
pub const SCALE: i128 = 1_000_000_000;
/// Basis-point denominator.
pub const BPS: i128 = 10_000;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
    pub collateral: Address,
    pub stability_pool: Address,
    /// Per-user collateralisation floor, in bps of minted value.
    pub min_collateral_ratio_bps: u32,
    /// System-wide collateralisation floor, in bps of total supply.
    pub min_reserve_ratio_bps: u32,
    /// Share of a de-peg charged on each mint, in bps of the shortfall.
    pub stability_fee_bps: u32,
    /// Liquidator reward, in bps of the repaid value.
    pub liquidation_bonus_bps: u32,
    /// Emergency stop for every state-changing entry point.
    pub paused: bool,
}

/// Global accounting, kept separately from the configuration.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct State {
    /// Collateral held in custody, in collateral units.
    pub total_collateral: i128,
    /// Stablecoin in circulation.
    pub total_supply: i128,
    /// The peg the coin defends, Q9. 1.0 means one unit per unit.
    pub peg: i128,
    /// Latest market price from the oracle, Q9.
    pub market_price: i128,
}

/// A single holder's position.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Position {
    pub user: Address,
    pub collateral: i128,
    pub stable_balance: i128,
    /// Collateral per unit of stablecoin held, in bps.
    pub ratio_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Config,
    State,
    /// user -> collateral deposited
    Collateral(Address),
    /// user -> stablecoin balance
    Balance(Address),
    /// user -> stability pool shares
    PoolShares(Address),
    /// stability pool collateral
    PoolCollateral,
    /// stability pool bad debt absorbed from liquidations
    PoolBadDebt,
    /// total shares outstanding in the stability pool
    PoolShareSupply,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidParams = 4,
    ContractPaused = 5,
    InvalidAmount = 6,
    InsufficientCollateral = 7,
    InsufficientBalance = 8,
    InsufficientShares = 9,
    ReserveRatioTooLow = 10,
    PositionHealthy = 11,
    ReserveRatioTooHigh = 12,
    InsufficientLiquidity = 13,
    PriceMissing = 14,
    PriceOutOfRange = 15,
    SelfLiquidation = 16,
}

const INIT: Symbol = symbol_short!("init");
const DEPOSIT: Symbol = symbol_short!("deposit");
const WITHDRAW: Symbol = symbol_short!("withdraw");
const MINT: Symbol = symbol_short!("mint");
const BURN: Symbol = symbol_short!("burn");
const REDEEM: Symbol = symbol_short!("redeem");
const PRICE: Symbol = symbol_short!("price");
const PARAM: Symbol = symbol_short!("param");
const PAUSE: Symbol = symbol_short!("pause");
const LIQUID: Symbol = symbol_short!("liquid");
const POOLDEP: Symbol = symbol_short!("pooldep");
const POOLWD: Symbol = symbol_short!("poolwd");

#[contract]
pub struct StableCoin;

// ----------------------------------------------------------------------
// Free helpers
// ----------------------------------------------------------------------

fn load_config(env: &Env) -> Result<Config, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::Config)
        .ok_or(Error::NotInitialized)
}

fn load_state(env: &Env) -> Result<State, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::State)
        .ok_or(Error::NotInitialized)
}

fn save_state(env: &Env, state: &State) {
    env.storage().persistent().set(&DataKey::State, state);
}

fn load_collateral(env: &Env, user: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::Collateral(user.clone()))
        .unwrap_or(0)
}

fn set_collateral(env: &Env, user: &Address, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::Collateral(user.clone()), &amount);
}

fn load_balance(env: &Env, user: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::Balance(user.clone()))
        .unwrap_or(0)
}

fn set_balance(env: &Env, user: &Address, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::Balance(user.clone()), &amount);
}

fn load_shares(env: &Env, user: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::PoolShares(user.clone()))
        .unwrap_or(0)
}

fn set_shares(env: &Env, user: &Address, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::PoolShares(user.clone()), &amount);
}

fn collateral_client<'a>(env: &'a Env, config: &Config) -> token::Client<'a> {
    token::Client::new(env, &config.collateral)
}

/// Reserve ratio: system collateral against circulating supply, in bps.
fn reserve_ratio(total_collateral: i128, total_supply: i128) -> u32 {
    if total_supply <= 0 {
        return u32::MAX;
    }
    let num = total_collateral * BPS;
    if num / total_supply > u32::MAX as i128 {
        return u32::MAX;
    }
    (num / total_supply) as u32
}

/// A position's collateralisation, in bps. Debt-free positions are `u32::MAX`.
fn position_ratio(collateral: i128, minted: i128) -> u32 {
    if minted <= 0 {
        return u32::MAX;
    }
    let num = collateral * BPS;
    if num / minted > u32::MAX as i128 {
        return u32::MAX;
    }
    (num / minted) as u32
}

/// How far the market trades below the peg, Q9. Zero while at or above peg.
fn depeg_gap(state: &State) -> i128 {
    if state.market_price <= 0 || state.market_price >= state.peg {
        return 0;
    }
    state.peg - state.market_price
}

/// Stability fee owed on `amount`, in collateral units. Charged only while the
/// market sits below the peg, and scaled by the depth of the break.
fn stability_fee(state: &State, config: &Config, amount: i128) -> i128 {
    let gap = depeg_gap(state);
    if gap == 0 || state.peg == 0 || config.stability_fee_bps == 0 {
        return 0;
    }
    let shortfall = (amount * gap) / state.peg;
    (shortfall * config.stability_fee_bps as i128) / BPS
}

fn ensure_running(config: &Config) -> Result<(), Error> {
    if config.paused {
        return Err(Error::ContractPaused);
    }
    Ok(())
}

fn credit(env: &Env, user: &Address, amount: i128) {
    if amount == 0 {
        return;
    }
    let next = load_balance(env, user) + amount;
    set_balance(env, user, next);
}

fn debit(env: &Env, user: &Address, amount: i128) -> Result<(), Error> {
    let held = load_balance(env, user);
    if held < amount {
        return Err(Error::InsufficientBalance);
    }
    set_balance(env, user, held - amount);
    Ok(())
}

fn pool_collateral(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::PoolCollateral)
        .unwrap_or(0)
}

fn set_pool_collateral(env: &Env, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::PoolCollateral, &amount);
}

fn pool_bad_debt(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::PoolBadDebt)
        .unwrap_or(0)
}

fn set_pool_bad_debt(env: &Env, amount: i128) {
    env.storage().persistent().set(&DataKey::PoolBadDebt, &amount);
}

fn share_supply(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::PoolShareSupply)
        .unwrap_or(0)
}

fn set_share_supply(env: &Env, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::PoolShareSupply, &amount);
}

#[contractimpl]
impl StableCoin {
    // ------------------------------------------------------------------
    // Setup
    // ------------------------------------------------------------------

    /// Deploy-time configuration. May only be called once.
    pub fn initialize(
        env: Env,
        admin: Address,
        collateral: Address,
        stability_pool: Address,
        min_collateral_ratio_bps: u32,
        min_reserve_ratio_bps: u32,
        stability_fee_bps: u32,
        liquidation_bonus_bps: u32,
    ) -> Result<(), Error> {
        if env.storage().persistent().has(&DataKey::Config) {
            return Err(Error::AlreadyInitialized);
        }
        if min_collateral_ratio_bps == 0 || min_collateral_ratio_bps > BPS as u32 {
            return Err(Error::InvalidParams);
        }
        if min_reserve_ratio_bps == 0 || min_reserve_ratio_bps > BPS as u32 {
            return Err(Error::InvalidParams);
        }
        if stability_fee_bps >= BPS as u32 || liquidation_bonus_bps >= BPS as u32 {
            return Err(Error::InvalidParams);
        }
        admin.require_auth();

        let config = Config {
            admin: admin.clone(),
            collateral,
            stability_pool,
            min_collateral_ratio_bps,
            min_reserve_ratio_bps,
            stability_fee_bps,
            liquidation_bonus_bps,
            paused: false,
        };
        env.storage().persistent().set(&DataKey::Config, &config);

        // Until the oracle reports, the market is assumed to sit on the peg.
        let state = State {
            total_collateral: 0,
            total_supply: 0,
            peg: SCALE,
            market_price: SCALE,
        };
        save_state(&env, &state);

        env.events().publish((INIT,), admin);
        Ok(())
    }

    /// Retune the risk parameters.
    pub fn set_parameters(
        env: Env,
        min_collateral_ratio_bps: u32,
        min_reserve_ratio_bps: u32,
        stability_fee_bps: u32,
        liquidation_bonus_bps: u32,
    ) -> Result<(), Error> {
        let mut config = load_config(&env)?;
        config.admin.require_auth();

        if min_collateral_ratio_bps == 0 || min_collateral_ratio_bps > BPS as u32 {
            return Err(Error::InvalidParams);
        }
        if min_reserve_ratio_bps == 0 || min_reserve_ratio_bps > BPS as u32 {
            return Err(Error::InvalidParams);
        }
        if stability_fee_bps >= BPS as u32 || liquidation_bonus_bps >= BPS as u32 {
            return Err(Error::InvalidParams);
        }

        config.min_collateral_ratio_bps = min_collateral_ratio_bps;
        config.min_reserve_ratio_bps = min_reserve_ratio_bps;
        config.stability_fee_bps = stability_fee_bps;
        config.liquidation_bonus_bps = liquidation_bonus_bps;
        env.storage().persistent().set(&DataKey::Config, &config);
        env.events().publish((PARAM,), min_collateral_ratio_bps);
        Ok(())
    }

    /// Move the peg. The market price is re-clamped under it so that a peg
    /// change can never leave the coin marked above par.
    pub fn set_peg(env: Env, peg: i128) -> Result<(), Error> {
        let config = load_config(&env)?;
        config.admin.require_auth();
        if peg <= 0 {
            return Err(Error::InvalidParams);
        }

        let mut state = load_state(&env)?;
        state.peg = peg;
        if state.market_price > peg {
            state.market_price = peg;
        }
        save_state(&env, &state);
        env.events().publish((PARAM,), peg);
        Ok(())
    }

    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        let mut config = load_config(&env)?;
        config.admin.require_auth();
        config.admin = new_admin.clone();
        env.storage().persistent().set(&DataKey::Config, &config);
        env.events().publish((PARAM,), new_admin);
        Ok(())
    }

    /// Point at a new stability pool.
    pub fn set_stability_pool(env: Env, pool: Address) -> Result<(), Error> {
        let mut config = load_config(&env)?;
        config.admin.require_auth();
        config.stability_pool = pool.clone();
        env.storage().persistent().set(&DataKey::Config, &config);
        env.events().publish((PARAM,), pool);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Emergency stop
    // ------------------------------------------------------------------

    /// Halt every state-changing operation.
    pub fn pause(env: Env) -> Result<(), Error> {
        let mut config = load_config(&env)?;
        config.admin.require_auth();
        config.paused = true;
        env.storage().persistent().set(&DataKey::Config, &config);
        env.events().publish((PAUSE,), true);
        Ok(())
    }

    /// Resume normal operation.
    pub fn unpause(env: Env) -> Result<(), Error> {
        let mut config = load_config(&env)?;
        config.admin.require_auth();
        config.paused = false;
        env.storage().persistent().set(&DataKey::Config, &config);
        env.events().publish((PAUSE,), false);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Collateral
    // ------------------------------------------------------------------

    /// Deposit backing collateral.
    pub fn deposit_collateral(env: Env, user: Address, amount: i128) -> Result<i128, Error> {
        user.require_auth();
        let config = load_config(&env)?;
        ensure_running(&config)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        collateral_client(&env, &config).transfer(
            &user,
            &env.current_contract_address(),
            &amount,
        );

        let total = load_collateral(&env, &user) + amount;
        set_collateral(&env, &user, total);

        let mut state = load_state(&env)?;
        state.total_collateral += amount;
        save_state(&env, &state);

        env.events()
            .publish((DEPOSIT,), (user.clone(), amount, total));
        Ok(total)
    }

    /// Release collateral that is not needed to back the user's balance.
    pub fn withdraw_collateral(env: Env, user: Address, amount: i128) -> Result<i128, Error> {
        user.require_auth();
        let config = load_config(&env)?;
        ensure_running(&config)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let held = load_collateral(&env, &user);
        if held < amount {
            return Err(Error::InsufficientCollateral);
        }
        let remaining = held - amount;
        let minted = load_balance(&env, &user);
        if position_ratio(remaining, minted) < config.min_collateral_ratio_bps {
            return Err(Error::InsufficientCollateral);
        }

        // The system-wide floor binds too: a withdrawal must not push reserves
        // under the required ratio.
        let state = load_state(&env)?;
        let after: i128 = state.total_collateral - amount;
        if reserve_ratio(after, state.total_supply) < config.min_reserve_ratio_bps {
            return Err(Error::ReserveRatioTooLow);
        }

        set_collateral(&env, &user, remaining);
        let mut state = state;
        state.total_collateral = after;
        save_state(&env, &state);

        collateral_client(&env, &config).transfer(
            &env.current_contract_address(),
            &user,
            &amount,
        );

        env.events()
            .publish((WITHDRAW,), (user.clone(), amount, remaining));
        Ok(remaining)
    }

    // ------------------------------------------------------------------
    // Mint, burn, redeem
    // ------------------------------------------------------------------

    /// Mint stablecoin against deposited collateral.
    pub fn mint(env: Env, user: Address, amount: i128) -> Result<i128, Error> {
        user.require_auth();
        let config = load_config(&env)?;
        ensure_running(&config)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let mut state = load_state(&env)?;
        let held = load_collateral(&env, &user);
        let minted = load_balance(&env, &user);

        // While the market trades below the peg a slice of the mint is charged
        // as a stability fee and routed to the pool.
        let fee = stability_fee(&state, &config, amount);
        if fee > held {
            return Err(Error::InsufficientCollateral);
        }

        // Both floors are checked against the post-mint state, since the fee
        // leaves the user's collateral while the supply grows.
        let new_collateral = held - fee;
        let new_minted = minted + amount;
        if position_ratio(new_collateral, new_minted) < config.min_collateral_ratio_bps {
            return Err(Error::InsufficientCollateral);
        }

        let new_total_collateral = state.total_collateral - fee;
        let new_supply = state.total_supply + amount;
        if reserve_ratio(new_total_collateral, new_supply) < config.min_reserve_ratio_bps {
            return Err(Error::ReserveRatioTooLow);
        }

        state.total_collateral = new_total_collateral;
        state.total_supply = new_supply;
        save_state(&env, &state);

        set_collateral(&env, &user, new_collateral);
        credit(&env, &user, amount);

        if fee > 0 {
            set_pool_collateral(&env, pool_collateral(&env) + fee);
        }

        let stable_balance = load_balance(&env, &user);
        let ratio = position_ratio(new_collateral, stable_balance);
        env.events()
            .publish((MINT,), (user.clone(), amount, fee, ratio));
        Ok(stable_balance)
    }

    /// Burn stablecoin to repay debt. No collateral moves.
    pub fn burn(env: Env, user: Address, amount: i128) -> Result<i128, Error> {
        user.require_auth();
        let config = load_config(&env)?;
        ensure_running(&config)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        debit(&env, &user, amount)?;

        let mut state = load_state(&env)?;
        state.total_supply -= amount;
        save_state(&env, &state);

        let remaining = load_balance(&env, &user);
        env.events().publish((BURN,), (user.clone(), amount, remaining));
        Ok(remaining)
    }

    /// Burn stablecoin and return the matching collateral.
    pub fn redeem(env: Env, user: Address, amount: i128) -> Result<i128, Error> {
        user.require_auth();
        let config = load_config(&env)?;
        ensure_running(&config)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let held = load_collateral(&env, &user);
        let minted = load_balance(&env, &user);
        if minted < amount {
            return Err(Error::InsufficientBalance);
        }

        // Collateral is released in proportion to the share of the position
        // being burned, so the remaining ratio is unchanged.
        let mut out = if minted == 0 {
            0
        } else {
            (held * amount) / minted
        };
        if out > held {
            out = held;
        }
        if out <= 0 {
            return Err(Error::InvalidAmount);
        }

        debit(&env, &user, amount)?;
        set_collateral(&env, &user, held - out);

        let mut state = load_state(&env)?;
        state.total_supply -= amount;
        state.total_collateral -= out;
        save_state(&env, &state);

        collateral_client(&env, &config).transfer(
            &env.current_contract_address(),
            &user,
            &out,
        );

        env.events().publish((REDEEM,), (user.clone(), amount, out));
        Ok(out)
    }

    // ------------------------------------------------------------------
    // Peg protection
    // ------------------------------------------------------------------

    /// Push a new market price from the oracle.
    ///
    /// The admin is expected to be the oracle or stability-pool address in
    /// production, so a single authorization covers both. Prices outside a
    /// 4x band around the peg are rejected: a bad feed reading must not be able
    /// to trigger mass liquidations.
    pub fn set_market_price(env: Env, price: i128) -> Result<(), Error> {
        let config = load_config(&env)?;
        config.admin.require_auth();
        if price <= 0 {
            return Err(Error::InvalidParams);
        }

        let mut state = load_state(&env)?;
        if price > state.peg * 4 || price * 4 < state.peg {
            return Err(Error::PriceOutOfRange);
        }

        state.market_price = price;
        save_state(&env, &state);
        env.events().publish((PRICE,), price);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Stability pool
    // ------------------------------------------------------------------

    /// Deposit collateral into the stability pool and receive shares.
    pub fn deposit_to_stability_pool(env: Env, user: Address, amount: i128) -> Result<i128, Error> {
        user.require_auth();
        let config = load_config(&env)?;
        ensure_running(&config)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        collateral_client(&env, &config).transfer(
            &user,
            &env.current_contract_address(),
            &amount,
        );

        let held = pool_collateral(&env);
        let supply = share_supply(&env);
        let shares = if supply == 0 || held == 0 {
            amount
        } else {
            (amount * supply) / held
        };
        if shares <= 0 {
            return Err(Error::InvalidAmount);
        }

        set_pool_collateral(&env, held + amount);
        set_share_supply(&env, supply + shares);
        set_shares(&env, &user, load_shares(&env, &user) + shares);

        env.events()
            .publish((POOLDEP,), (user.clone(), amount, shares));
        Ok(shares)
    }

    /// Burn pool shares and take the pro-rata collateral back.
    pub fn withdraw_from_stability_pool(env: Env, user: Address, shares: i128) -> Result<i128, Error> {
        user.require_auth();
        let config = load_config(&env)?;
        ensure_running(&config)?;
        if shares <= 0 {
            return Err(Error::InvalidAmount);
        }

        let held_shares = load_shares(&env, &user);
        if held_shares < shares {
            return Err(Error::InsufficientShares);
        }
        let supply = share_supply(&env);
        let assets = pool_collateral(&env);
        if supply <= 0 {
            return Err(Error::InvalidAmount);
        }

        let mut out = (assets * shares) / supply;
        if out > assets {
            out = assets;
        }

        set_shares(&env, &user, held_shares - shares);
        set_share_supply(&env, supply - shares);
        set_pool_collateral(&env, assets - out);

        collateral_client(&env, &config).transfer(
            &env.current_contract_address(),
            &user,
            &out,
        );

        env.events()
            .publish((POOLWD,), (user.clone(), shares, out));
        Ok(out)
    }

    // ------------------------------------------------------------------
    // Liquidation
    // ------------------------------------------------------------------

    /// Repay part of an under-collateralised position and take its collateral
    /// plus the liquidation bonus.
    pub fn liquidate(
        env: Env,
        liquidator: Address,
        user: Address,
        amount: i128,
    ) -> Result<i128, Error> {
        let config = load_config(&env)?;
        ensure_running(&config)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if liquidator == user {
            return Err(Error::SelfLiquidation);
        }

        let mut state = load_state(&env)?;
        let held = load_collateral(&env, &user);
        let minted = load_balance(&env, &user);
        if minted == 0 || held == 0 {
            return Err(Error::PositionHealthy);
        }
        if position_ratio(held, minted) >= config.min_collateral_ratio_bps {
            return Err(Error::PositionHealthy);
        }

        let repay = if amount > minted { minted } else { amount };

        // The liquidator receives the repaid value plus the bonus, paid out of
        // the borrower's collateral; the bonus itself is skimmed to the pool.
        let value = (held * repay) / minted;
        let mut seized = (value * (BPS + config.liquidation_bonus_bps as i128)) / BPS;
        if seized > held {
            seized = held;
        }

        let bonus = if seized > value { seized - value } else { 0 };

        set_collateral(&env, &user, held - seized);
        set_balance(&env, &user, minted - repay);

        state.total_supply -= repay;
        state.total_collateral -= seized;
        save_state(&env, &state);

        // The bonus is the pool's cut; anything repaid beyond the collateral it
        // was entitled to is genuine bad debt the pool has taken on.
        set_pool_collateral(&env, pool_collateral(&env) + bonus);
        let uncovered = if repay > value { repay - value } else { 0 };
        if uncovered > 0 {
            set_pool_bad_debt(&env, pool_bad_debt(&env) + uncovered);
        }

        collateral_client(&env, &config).transfer(
            &env.current_contract_address(),
            &liquidator,
            &seized,
        );

        // Burning the repaid stablecoin is what shrinks the supply.
        env.events().publish(
            (LIQUID,),
            (liquidator.clone(), user.clone(), repay, seized, bonus),
        );
        Ok(seized)
    }

    // ------------------------------------------------------------------
    // Views
    // ------------------------------------------------------------------

    /// A holder's full position.
    pub fn position(env: Env, user: Address) -> Result<Position, Error> {
        let collateral = load_collateral(&env, &user);
        let stable_balance = load_balance(&env, &user);
        Ok(Position {
            user,
            collateral,
            stable_balance,
            ratio_bps: position_ratio(collateral, stable_balance),
        })
    }

    pub fn collateral_of(env: Env, user: Address) -> i128 {
        load_collateral(&env, &user)
    }

    pub fn balance_of(env: Env, user: Address) -> i128 {
        load_balance(&env, &user)
    }

    /// System collateral against supply, in bps.
    pub fn reserve_ratio(env: Env) -> Result<u32, Error> {
        let state = load_state(&env)?;
        Ok(reserve_ratio(state.total_collateral, state.total_supply))
    }

    /// Collateral per unit of stablecoin, in bps.
    pub fn collateral_ratio(env: Env, user: Address) -> u32 {
        position_ratio(load_collateral(&env, &user), load_balance(&env, &user))
    }

    /// Whether the market trades below the peg.
    pub fn is_peg_broken(env: Env) -> Result<bool, Error> {
        let state = load_state(&env)?;
        Ok(depeg_gap(&state) > 0)
    }

    /// Stability fee that a mint of `amount` would attract right now.
    pub fn stability_fee_for(env: Env, amount: i128) -> Result<i128, Error> {
        let config = load_config(&env)?;
        let state = load_state(&env)?;
        Ok(stability_fee(&state, &config, amount))
    }

    /// Whether the position can be liquidated right now.
    pub fn is_liquidatable(env: Env, user: Address) -> Result<bool, Error> {
        let config = load_config(&env)?;
        let ratio = position_ratio(load_collateral(&env, &user), load_balance(&env, &user));
        Ok(ratio != u32::MAX && ratio < config.min_collateral_ratio_bps)
    }

    pub fn is_paused(env: Env) -> Result<bool, Error> {
        Ok(load_config(&env)?.paused)
    }

    pub fn pool_shares(env: Env, user: Address) -> i128 {
        load_shares(&env, &user)
    }

    pub fn pool_collateral(env: Env) -> i128 {
        pool_collateral(&env)
    }

    pub fn pool_bad_debt(env: Env) -> i128 {
        pool_bad_debt(&env)
    }

    pub fn pool_share_supply(env: Env) -> i128 {
        share_supply(&env)
    }

    pub fn get_config(env: Env) -> Result<Config, Error> {
        load_config(&env)
    }

    pub fn get_state(env: Env) -> Result<State, Error> {
        load_state(&env)
    }
}
