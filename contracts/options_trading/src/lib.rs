#![no_std]

mod math;

#[cfg(test)]
mod test;

use math::{bs_greeks, bs_price, SCALE};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env,
    Symbol, Vec,
};

/// Length of a Gregorian year, used to express expiry in Black-Scholes units.
pub const SECONDS_PER_YEAR: u64 = 31_536_000;
/// Basis-point denominator.
pub const BPS: i128 = 10_000;
/// Cap on the protocol fee charged on premiums (5%).
pub const MAX_FEE_BPS: u32 = 500;

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OptionType {
    Call = 1,
    Put = 2,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OptionStyle {
    European = 1,
    American = 2,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OptionStatus {
    Open = 1,
    Exercised = 2,
    Settled = 3,
    Expired = 4,
    Closed = 5,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
    pub collateral: Address,
    pub fee_bps: u32,
    pub fee_recipient: Address,
    pub risk_free_bps: u32,
}

/// A tradeable option class: one underlying, one strike, one expiry.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Series {
    pub id: u64,
    pub underlying: Address,
    pub quote: Address,
    pub option_type: OptionType,
    pub style: OptionStyle,
    pub strike: i128,
    pub expiry: u64,
    pub premium: i128,
    pub vol_bps: u32,
    pub spot: i128,
    pub total_supply: i128,
    pub open_interest: i128,
    pub quote_reserve: i128,
    pub underlying_reserve: i128,
    pub active: bool,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Position {
    pub id: u64,
    pub series_id: u64,
    pub owner: Address,
    pub units: i128,
    pub premium_paid: i128,
    pub status: OptionStatus,
    pub opened_at: u64,
    pub closed_at: u64,
}

/// Model outputs. All monetary values are Q9.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Greeks {
    pub delta: i128,
    pub gamma: i128,
    pub vega: i128,
    pub theta: i128,
    pub rho: i128,
    pub price: i128,
    pub intrinsic: i128,
    pub time_to_expiry: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Config,
    SeriesCount,
    Series(u64),
    SeriesPositions(u64),
    PositionCount,
    Position(u64),
    UserPositions(Address),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidParams = 4,
    SeriesNotFound = 5,
    PositionNotFound = 6,
    SeriesInactive = 7,
    AlreadyExpired = 8,
    NotExpired = 9,
    NotExercisable = 10,
    InsufficientUnits = 11,
    InsufficientLiquidity = 12,
    InvalidAmount = 13,
    Slippage = 14,
    TokenMismatch = 15,
    PriceMissing = 16,
    PositionClosed = 17,
}

const OPENED: Symbol = symbol_short!("opened");
const BOUGHT: Symbol = symbol_short!("bought");
const SOLD: Symbol = symbol_short!("sold");
const EXERCSED: Symbol = symbol_short!("exercsed");
const CLOSED: Symbol = symbol_short!("closed");
const SETTLED: Symbol = symbol_short!("settled");
const EXPIRED: Symbol = symbol_short!("expired");
const CREATED: Symbol = symbol_short!("created");
const FUNDED: Symbol = symbol_short!("funded");
const SPOT: Symbol = symbol_short!("spot");
const PARAMS: Symbol = symbol_short!("params");
const FEE: Symbol = symbol_short!("fee");
const ADMIN: Symbol = symbol_short!("admin");

#[contract]
pub struct OptionsTrading;

// ----------------------------------------------------------------------
// Free helpers (kept outside `contractimpl` so they stay private)
// ----------------------------------------------------------------------

fn is_call(t: &OptionType) -> bool {
    matches!(t, OptionType::Call)
}

/// Intrinsic value of a single unit, in Q9.
fn intrinsic(spot: i128, strike: i128, call: bool) -> i128 {
    if call {
        if spot > strike {
            spot - strike
        } else {
            0
        }
    } else if strike > spot {
        strike - spot
    } else {
        0
    }
}

/// Remaining life of a series, expressed in years, in Q9.
fn t_years(env: &Env, expiry: u64) -> i128 {
    let now = env.ledger().timestamp();
    if expiry <= now {
        return 0;
    }
    ((expiry - now) as i128 * SCALE) / (SECONDS_PER_YEAR as i128)
}

/// The token a series settles payouts in, and the matching reserve.
fn payout_token(series: &Series) -> Address {
    if is_call(&series.option_type) {
        series.quote.clone()
    } else {
        series.underlying.clone()
    }
}

/// Cash available to a series for payouts, in the payout token.
fn payout_reserve(series: &Series) -> i128 {
    if is_call(&series.option_type) {
        series.quote_reserve
    } else {
        series.underlying_reserve
    }
}

fn debit_reserve(series: &mut Series, amount: i128) {
    if is_call(&series.option_type) {
        series.quote_reserve -= amount;
    } else {
        series.underlying_reserve -= amount;
    }
}

fn load_config(env: &Env) -> Result<Config, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::Config)
        .ok_or(Error::NotInitialized)
}

fn load_series(env: &Env, series_id: u64) -> Result<Series, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::Series(series_id))
        .ok_or(Error::SeriesNotFound)
}

fn save_series(env: &Env, series: &Series) {
    env.storage()
        .persistent()
        .set(&DataKey::Series(series.id), series);
}

fn load_position(env: &Env, position_id: u64) -> Result<Position, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::Position(position_id))
        .ok_or(Error::PositionNotFound)
}

/// Reject payouts the series cannot cover on paper *and* on chain.
fn ensure_solvent(env: &Env, series: &Series, amount: i128) -> Result<(), Error> {
    if amount <= 0 {
        return Ok(());
    }
    if payout_reserve(series) < amount {
        return Err(Error::InsufficientLiquidity);
    }
    let token_addr = payout_token(series);
    let client = token::Client::new(env, &token_addr);
    if client.balance(&env.current_contract_address()) < amount {
        return Err(Error::InsufficientLiquidity);
    }
    Ok(())
}

fn pull(env: &Env, token_addr: &Address, from: &Address, amount: i128) {
    if amount <= 0 {
        return;
    }
    let client = token::Client::new(env, token_addr);
    client.transfer(from, &env.current_contract_address(), &amount);
}

fn push(env: &Env, token_addr: &Address, to: &Address, amount: i128) {
    if amount <= 0 {
        return;
    }
    let client = token::Client::new(env, token_addr);
    client.transfer(&env.current_contract_address(), to, &amount);
}

fn proportional(total: i128, part: i128, whole: i128) -> i128 {
    if whole == 0 {
        return 0;
    }
    (total * part) / whole
}

/// Split a gross premium into the series-backing amount and the protocol fee.
fn split_premium(env: &Env, series: &Series, units: i128) -> Result<(i128, i128), Error> {
    if units <= 0 {
        return Err(Error::InvalidAmount);
    }
    let config = load_config(env)?;
    let gross = (series.premium * units) / SCALE;
    let fee = (gross * config.fee_bps as i128) / BPS;
    Ok((gross - fee, fee))
}

/// Cash due to the holder of `units`.
fn payout_for(series: &Series, units: i128) -> i128 {
    let value = intrinsic(series.spot, series.strike, is_call(&series.option_type));
    (value * units) / SCALE
}

/// Model value of `units`: intrinsic value dominates for in-the-money American
/// options, Black-Scholes otherwise.
fn fair_value(env: &Env, series: &Series, units: i128) -> Result<i128, Error> {
    if series.spot <= 0 {
        return Err(Error::PriceMissing);
    }
    let config = load_config(env)?;
    let t = t_years(env, series.expiry);
    let vol = (series.vol_bps as i128 * SCALE) / 10_000;
    let rate = (config.risk_free_bps as i128 * SCALE) / 10_000;
    let call = is_call(&series.option_type);

    let mut unit = bs_price(series.spot, series.strike, t, vol, rate, call);
    if series.style == OptionStyle::American {
        let itm = intrinsic(series.spot, series.strike, call);
        if itm > unit {
            unit = itm;
        }
    }
    Ok((unit * units) / SCALE)
}

fn ensure_writable(series: &Series, env: &Env) -> Result<(), Error> {
    if !series.active {
        return Err(Error::SeriesInactive);
    }
    if env.ledger().timestamp() >= series.expiry {
        return Err(Error::AlreadyExpired);
    }
    Ok(())
}

fn ensure_exercisable(series: &Series, env: &Env) -> Result<(), Error> {
    let now = env.ledger().timestamp();
    match series.style {
        OptionStyle::American => {
            if now >= series.expiry {
                return Err(Error::AlreadyExpired);
            }
            Ok(())
        }
        OptionStyle::European => {
            if now < series.expiry {
                return Err(Error::NotExercisable);
            }
            Ok(())
        }
    }
}

/// Mark a position terminal and wipe its open quantities.
fn close_position(env: &Env, position: &mut Position, status: OptionStatus) {
    position.status = status;
    position.closed_at = env.ledger().timestamp();
    position.units = 0;
    position.premium_paid = 0;
    env.storage()
        .persistent()
        .set(&DataKey::Position(position.id), position);
}

fn load_series_positions(env: &Env, series_id: u64) -> Vec<u64> {
    env.storage()
        .persistent()
        .get(&DataKey::SeriesPositions(series_id))
        .unwrap_or(Vec::new(env))
}

/// Record a new long position and index it by owner and by series.
fn open_position(env: &Env, series_id: u64, owner: &Address, units: i128, premium: i128) -> u64 {
    let id: u64 = env
        .storage()
        .persistent()
        .get(&DataKey::PositionCount)
        .unwrap_or(0)
        + 1;

    let position = Position {
        id,
        series_id,
        owner: owner.clone(),
        units,
        premium_paid: premium,
        status: OptionStatus::Open,
        opened_at: env.ledger().timestamp(),
        closed_at: 0,
    };

    env.storage().persistent().set(&DataKey::PositionCount, &id);
    env.storage()
        .persistent()
        .set(&DataKey::Position(id), &position);

    let mut ids = load_series_positions(env, series_id);
    ids.push_back(id);
    env.storage()
        .persistent()
        .set(&DataKey::SeriesPositions(series_id), &ids);

    let mut owned: Vec<u64> = env
        .storage()
        .persistent()
        .get(&DataKey::UserPositions(owner.clone()))
        .unwrap_or(Vec::new(env));
    owned.push_back(id);
    env.storage()
        .persistent()
        .set(&DataKey::UserPositions(owner.clone()), &owned);

    id
}

fn count_of(env: &Env, key: &DataKey) -> u64 {
    env.storage().persistent().get(key).unwrap_or(0)
}

#[contractimpl]
impl OptionsTrading {
    // ------------------------------------------------------------------
    // Setup
    // ------------------------------------------------------------------

    /// Deploy-time configuration. May only be called once.
    pub fn initialize(
        env: Env,
        admin: Address,
        collateral: Address,
        fee_bps: u32,
        fee_recipient: Address,
        risk_free_bps: u32,
    ) -> Result<(), Error> {
        if env.storage().persistent().has(&DataKey::Config) {
            return Err(Error::AlreadyInitialized);
        }
        if fee_bps > MAX_FEE_BPS {
            return Err(Error::InvalidParams);
        }
        admin.require_auth();

        let config = Config {
            admin,
            collateral,
            fee_bps,
            fee_recipient,
            risk_free_bps,
        };
        env.storage().persistent().set(&DataKey::Config, &config);
        Ok(())
    }

    /// Hand the admin role to a new address.
    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        let mut config = load_config(&env)?;
        config.admin.require_auth();
        config.admin = new_admin.clone();
        env.storage().persistent().set(&DataKey::Config, &config);
        env.events().publish((ADMIN,), new_admin);
        Ok(())
    }

    /// Adjust the premium fee and/or the address that collects it.
    pub fn set_fee(
        env: Env,
        fee_bps: u32,
        fee_recipient: Address,
    ) -> Result<(), Error> {
        let mut config = load_config(&env)?;
        config.admin.require_auth();
        if fee_bps > MAX_FEE_BPS {
            return Err(Error::InvalidParams);
        }
        config.fee_bps = fee_bps;
        config.fee_recipient = fee_recipient;
        env.storage().persistent().set(&DataKey::Config, &config);
        env.events().publish((FEE,), fee_bps);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Series administration
    // ------------------------------------------------------------------

    /// List a new option class and return its id.
    pub fn create_series(
        env: Env,
        underlying: Address,
        quote: Address,
        option_type: OptionType,
        style: OptionStyle,
        strike: i128,
        expiry: u64,
        vol_bps: u32,
    ) -> Result<u64, Error> {
        let config = load_config(&env)?;
        config.admin.require_auth();

        if strike <= 0 {
            return Err(Error::InvalidParams);
        }
        if expiry <= env.ledger().timestamp() {
            return Err(Error::AlreadyExpired);
        }

        let id = count_of(&env, &DataKey::SeriesCount) + 1;
        let series = Series {
            id,
            underlying,
            quote,
            option_type,
            style,
            strike,
            expiry,
            premium: 0,
            vol_bps,
            spot: 0,
            total_supply: 0,
            open_interest: 0,
            quote_reserve: 0,
            underlying_reserve: 0,
            active: true,
            created_at: env.ledger().timestamp(),
        };

        save_series(&env, &series);
        env.storage().persistent().set(&DataKey::SeriesCount, &id);
        env.storage()
            .persistent()
            .set(&DataKey::SeriesPositions(id), &Vec::<u64>::new(&env));
        env.events().publish((CREATED, id), series);

        Ok(id)
    }

    /// Push a new spot price for the underlying, in Q9.
    pub fn set_spot(env: Env, series_id: u64, spot: i128) -> Result<(), Error> {
        let config = load_config(&env)?;
        config.admin.require_auth();

        let mut series = load_series(&env, series_id)?;
        if spot <= 0 {
            return Err(Error::InvalidParams);
        }
        series.spot = spot;
        save_series(&env, &series);
        env.events().publish((SPOT, series_id), spot);
        Ok(())
    }

    /// Re-price the series.
    pub fn set_premium(env: Env, series_id: u64, premium: i128) -> Result<(), Error> {
        let config = load_config(&env)?;
        config.admin.require_auth();

        let mut series = load_series(&env, series_id)?;
        if premium < 0 {
            return Err(Error::InvalidParams);
        }
        series.premium = premium;
        save_series(&env, &series);
        env.events().publish((PARAMS, series_id), premium);
        Ok(())
    }

    /// Update the implied volatility used by the pricing model.
    pub fn set_volatility(env: Env, series_id: u64, vol_bps: u32) -> Result<(), Error> {
        let config = load_config(&env)?;
        config.admin.require_auth();

        let mut series = load_series(&env, series_id)?;
        series.vol_bps = vol_bps;
        save_series(&env, &series);
        env.events().publish((PARAMS, series_id), vol_bps);
        Ok(())
    }

    /// Pause or resume writes on a series.
    pub fn set_series_active(
        env: Env,
        series_id: u64,
        active: bool,
    ) -> Result<(), Error> {
        let config = load_config(&env)?;
        config.admin.require_auth();

        let mut series = load_series(&env, series_id)?;
        series.active = active;
        save_series(&env, &series);
        env.events().publish((PARAMS, series_id), active);
        Ok(())
    }

    /// Back a series so that exercise payouts are always covered. Calls take
    /// quote token, puts take the underlying.
    pub fn fund_series(
        env: Env,
        provider: Address,
        series_id: u64,
        token_addr: Address,
        amount: i128,
    ) -> Result<(), Error> {
        provider.require_auth();
        let mut series = load_series(&env, series_id)?;

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if token_addr == series.quote {
            series.quote_reserve += amount;
        } else if token_addr == series.underlying {
            series.underlying_reserve += amount;
        } else {
            return Err(Error::TokenMismatch);
        }

        let client = token::Client::new(&env, &token_addr);
        client.transfer(&provider, &env.current_contract_address(), &amount);

        save_series(&env, &series);
        env.events().publish((FUNDED, series_id), (token_addr, amount));
        Ok(())
    }

    // ------------------------------------------------------------------
    // Writing and transferring options
    // ------------------------------------------------------------------

    /// Buy `units` of a series at the listed premium. Returns a position id.
    pub fn write(env: Env, series_id: u64, buyer: Address, units: i128) -> Result<u64, Error> {
        buyer.require_auth();

        let mut series = load_series(&env, series_id)?;
        ensure_writable(&series, &env)?;

        let (premium, fee) = split_premium(&env, &series, units)?;
        let config = load_config(&env)?;

        pull(&env, &series.quote, &buyer, premium + fee);
        push(&env, &series.quote, &config.fee_recipient, fee);

        series.quote_reserve += premium;
        series.total_supply += units;
        series.open_interest += units;
        save_series(&env, &series);

        let id = open_position(&env, series_id, &buyer, units, premium);
        env.events()
            .publish((OPENED, id), (buyer.clone(), units, premium));

        Ok(id)
    }

    /// Acquire part of an existing long position from its current owner.
    pub fn buy(
        env: Env,
        taker: Address,
        position_id: u64,
        units: i128,
    ) -> Result<u64, Error> {
        taker.require_auth();

        let mut position = load_position(&env, position_id)?;
        if position.status != OptionStatus::Open {
            return Err(Error::PositionClosed);
        }
        if units <= 0 || units > position.units {
            return Err(Error::InsufficientUnits);
        }

        let series = load_series(&env, position.series_id)?;
        let (premium, fee) = split_premium(&env, &series, units)?;
        let config = load_config(&env)?;

        pull(&env, &series.quote, &taker, premium);
        pull(&env, &series.quote, &taker, fee);
        push(&env, &series.quote, &config.fee_recipient, fee);

        // Transfer the units plus a proportional slice of the writer's cost
        // basis so that the two sides net out to the same economics.
        let share = proportional(position.premium_paid, units, position.units);
        position.units -= units;
        position.premium_paid -= share;
        env.storage()
            .persistent()
            .set(&DataKey::Position(position_id), &position);

        let id = open_position(&env, position.series_id, &taker, units, share);
        env.events()
            .publish((BOUGHT, id), (taker.clone(), units, share));

        Ok(id)
    }

    /// Sell part of an owned position back to the contract at the model price.
    pub fn sell(
        env: Env,
        holder: Address,
        position_id: u64,
        units: i128,
    ) -> Result<i128, Error> {
        holder.require_auth();

        let mut position = load_position(&env, position_id)?;
        if position.status != OptionStatus::Open {
            return Err(Error::PositionClosed);
        }
        if position.owner != holder {
            return Err(Error::Unauthorized);
        }
        if units <= 0 || units > position.units {
            return Err(Error::InsufficientUnits);
        }

        let mut series = load_series(&env, position.series_id)?;
        ensure_writable(&series, &env)?;

        let value = fair_value(&env, &series, units)?;
        ensure_solvent(&env, &series, value)?;

        let share = proportional(position.premium_paid, units, position.units);
        position.units -= units;
        position.premium_paid -= share;
        if position.units == 0 {
            position.status = OptionStatus::Closed;
            position.closed_at = env.ledger().timestamp();
        }
        env.storage()
            .persistent()
            .set(&DataKey::Position(position_id), &position);

        series.quote_reserve -= value;
        series.total_supply -= units;
        save_series(&env, &series);

        push(&env, &series.quote, &holder, value);
        env.events()
            .publish((SOLD, position_id), (holder.clone(), units, value));

        Ok(value)
    }

    // ------------------------------------------------------------------
    // Exercise, settlement and expiry
    // ------------------------------------------------------------------

    /// Exercise a position.
    ///
    /// American options may be exercised at any point up to (but not on) expiry.
    /// European options only become exercisable once the expiry timestamp has
    /// passed, which is the standard way to avoid early-exercise risk in an
    /// on-chain market where a writer cannot unilaterally settle.
    pub fn exercise(env: Env, holder: Address, position_id: u64) -> Result<i128, Error> {
        holder.require_auth();

        let mut position = load_position(&env, position_id)?;
        if position.status != OptionStatus::Open {
            return Err(Error::PositionClosed);
        }
        if position.owner != holder {
            return Err(Error::Unauthorized);
        }

        let mut series = load_series(&env, position.series_id)?;
        ensure_exercisable(&series, &env)?;

        let units = position.units;
        let amount = payout_for(&series, units);
        ensure_solvent(&env, &series, amount)?;

        close_position(&env, &mut position, OptionStatus::Exercised);

        series.open_interest -= units;
        series.total_supply -= units;
        debit_reserve(&mut series, amount);
        save_series(&env, &series);

        push(&env, &payout_token(&series), &holder, amount);
        env.events()
            .publish((EXERCSED, position_id), (holder.clone(), units, amount));

        Ok(amount)
    }

    /// Close a position at the model value before expiry.
    pub fn close(
        env: Env,
        holder: Address,
        position_id: u64,
        min_out: i128,
    ) -> Result<i128, Error> {
        holder.require_auth();

        let mut position = load_position(&env, position_id)?;
        if position.status != OptionStatus::Open {
            return Err(Error::PositionClosed);
        }
        if position.owner != holder {
            return Err(Error::Unauthorized);
        }

        let mut series = load_series(&env, position.series_id)?;
        if env.ledger().timestamp() >= series.expiry {
            return Err(Error::AlreadyExpired);
        }

        let value = fair_value(&env, &series, position.units)?;
        if value < min_out {
            return Err(Error::Slippage);
        }
        ensure_solvent(&env, &series, value)?;

        let units = position.units;
        close_position(&env, &mut position, OptionStatus::Closed);

        series.open_interest -= units;
        series.total_supply -= units;
        series.quote_reserve -= value;
        save_series(&env, &series);

        push(&env, &series.quote, &holder, value);
        env.events()
            .publish((CLOSED, position_id), (holder.clone(), value));

        Ok(value)
    }

    /// Settle every remaining position of an expired series. Positions that
    /// expired worthless are marked settled without a payout.
    pub fn settle(env: Env, series_id: u64) -> Result<u32, Error> {
        let mut series = load_series(&env, series_id)?;
        if env.ledger().timestamp() < series.expiry {
            return Err(Error::NotExpired);
        }

        let token_addr = payout_token(&series);
        let mut settled: u32 = 0;
        let mut paid: i128 = 0;

        for id in load_series_positions(&env, series_id).iter() {
            let mut position = load_position(&env, id)?;
            if position.status != OptionStatus::Open {
                continue;
            }

            let units = position.units;
            let amount = payout_for(&series, units);
            if amount > 0 && ensure_solvent(&env, &series, amount).is_err() {
                // Leave the position open rather than settle it short; the
                // operator can fund the series and retry.
                continue;
            }

            close_position(&env, &mut position, OptionStatus::Settled);
            series.open_interest -= units;
            series.total_supply -= units;
            debit_reserve(&mut series, amount);
            push(&env, &token_addr, &position.owner, amount);
            paid += amount;
            settled += 1;
        }

        save_series(&env, &series);
        env.events().publish((SETTLED, series_id), (settled, paid));

        Ok(settled)
    }

    /// Mark an expired series worthless without paying out. Intended for
    /// cleanup once a series' reserves have already been withdrawn.
    pub fn expire(env: Env, series_id: u64) -> Result<u32, Error> {
        if env.ledger().timestamp() < load_series(&env, series_id)?.expiry {
            return Err(Error::NotExpired);
        }

        let mut expired: u32 = 0;
        for id in load_series_positions(&env, series_id).iter() {
            let mut position = load_position(&env, id)?;
            if position.status != OptionStatus::Open {
                continue;
            }
            close_position(&env, &mut position, OptionStatus::Expired);
            expired += 1;
        }

        env.events().publish((EXPIRED, series_id), expired);
        Ok(expired)
    }

    // ------------------------------------------------------------------
    // Views
    // ------------------------------------------------------------------

    /// Full Black-Scholes greeks for the current spot, strike and time to
    /// expiry. `delta` is per 1.00 of spot, `vega` and `rho` per 1%, and
    /// `theta` per elapsed day.
    pub fn greeks(env: Env, series_id: u64) -> Result<Greeks, Error> {
        let series = load_series(&env, series_id)?;
        let config = load_config(&env)?;
        if series.spot <= 0 {
            return Err(Error::PriceMissing);
        }

        let t = t_years(&env, series.expiry);
        let vol = (series.vol_bps as i128 * SCALE) / 10_000;
        let rate = (config.risk_free_bps as i128 * SCALE) / 10_000;
        let call = is_call(&series.option_type);

        let g = bs_greeks(series.spot, series.strike, t, vol, rate, call);

        Ok(Greeks {
            delta: g.delta,
            gamma: g.gamma,
            vega: g.vega,
            theta: g.theta,
            rho: g.rho,
            price: bs_price(series.spot, series.strike, t, vol, rate, call),
            intrinsic: intrinsic(series.spot, series.strike, call),
            time_to_expiry: t,
        })
    }

    /// Theoretical option value for `units` at the current spot.
    pub fn theoretical_price(env: Env, series_id: u64, units: i128) -> Result<i128, Error> {
        let series = load_series(&env, series_id)?;
        if series.spot <= 0 {
            return Err(Error::PriceMissing);
        }
        let config = load_config(&env)?;
        let t = t_years(&env, series.expiry);
        let vol = (series.vol_bps as i128 * SCALE) / 10_000;
        let rate = (config.risk_free_bps as i128 * SCALE) / 10_000;
        let unit = bs_price(series.spot, series.strike, t, vol, rate, is_call(&series.option_type));
        Ok((unit * units) / SCALE)
    }

    /// Intrinsic value of `units` at the current spot.
    pub fn intrinsic_value(env: Env, series_id: u64, units: i128) -> Result<i128, Error> {
        let series = load_series(&env, series_id)?;
        if series.spot <= 0 {
            return Err(Error::PriceMissing);
        }
        let unit = intrinsic(series.spot, series.strike, is_call(&series.option_type));
        Ok((unit * units) / SCALE)
    }

    /// Total premium, including the protocol fee, for writing `units`.
    pub fn quote_premium(env: Env, series_id: u64, units: i128) -> Result<i128, Error> {
        let series = load_series(&env, series_id)?;
        let (premium, fee) = split_premium(&env, &series, units)?;
        Ok(premium + fee)
    }

    pub fn get_series(env: Env, series_id: u64) -> Result<Series, Error> {
        load_series(&env, series_id)
    }

    pub fn get_position(env: Env, position_id: u64) -> Result<Position, Error> {
        load_position(&env, position_id)
    }

    pub fn get_config(env: Env) -> Result<Config, Error> {
        load_config(&env)
    }

    pub fn series_count(env: Env) -> u64 {
        count_of(&env, &DataKey::SeriesCount)
    }

    pub fn position_count(env: Env) -> u64 {
        count_of(&env, &DataKey::PositionCount)
    }

    /// Every position id ever opened by `owner`.
    pub fn user_positions(env: Env, owner: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::UserPositions(owner))
            .unwrap_or(Vec::new(&env))
    }

    /// Every position id belonging to a series.
    pub fn series_positions(env: Env, series_id: u64) -> Vec<u64> {
        load_series_positions(&env, series_id)
    }
}
