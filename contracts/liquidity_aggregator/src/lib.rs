#![no_std]

#[cfg(test)]
mod test;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, vec, Address, Env,
    IntoVal, Symbol, Vec,
};

/// Basis-point denominator.
pub const BPS: i128 = 10_000;
/// Ceiling on the share of a pool's input reserve a single leg may take, used
/// to stop split routing from dumping an entire order into one pool.
pub const MAX_LEG_FILL_BPS: i128 = 3_000;
/// Hard ceiling on path length, independent of the caller's request.
pub const MAX_HOPS: u32 = 3;
/// Ceiling on the number of routes a split may fan out into.
pub const MAX_SPLIT_ROUTES: u32 = 4;
/// Q-scale used by the liquidity-incentive reward index.
const REWARD_SCALE: i128 = 1_000_000_000;

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PoolKind {
    /// x * y = k.
    ConstantProduct = 1,
    /// Near-constant sum, as used by stableswap curves.
    Stable = 2,
    /// Liquidity held by a third-party contract, reached over its `swap`.
    External = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
    pub fee_recipient: Address,
    /// Protocol cut taken from the output of a swap, in bps.
    pub fee_bps: u32,
    /// Scoring penalty applied per extra hop, in bps, so a multi-hop route has
    /// to be meaningfully better before it displaces a direct one.
    pub routing_penalty_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pool {
    pub address: Address,
    pub token_in: Address,
    pub token_out: Address,
    pub reserve_in: i128,
    pub reserve_out: i128,
    pub fee_bps: u32,
    pub kind: PoolKind,
    pub enabled: bool,
    /// Cumulative reward budget funded for liquidity providers.
    pub incentives: i128,
    /// Total incentive shares outstanding.
    pub incentive_shares: i128,
    /// Cumulative rewards accrued per share, Q-scaled. This is the standard
    /// reward index, so a claim never pays out the same token twice.
    pub reward_index: i128,
    pub updated_at: u64,
}

/// A single pool's price for a given size.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Quote {
    pub pool: Address,
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in: i128,
    pub amount_out: i128,
    pub fee: i128,
    /// Shortfall against the pool's mid price, in bps.
    pub price_impact_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteLeg {
    pub pool: Address,
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in: i128,
    pub amount_out: i128,
    pub fee: i128,
    pub price_impact_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Route {
    pub legs: Vec<RouteLeg>,
    pub amount_in: i128,
    pub expected_out: i128,
    pub total_fee: i128,
    pub price_impact_bps: u32,
    /// Ranking score: the output discounted by the per-hop penalty.
    pub score: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Config,
    Pool(Address),
    AllPools,
    Tokens,
    /// (token in, token out) -> routable pools
    Pools(Address, Address),
    /// (pool, provider) -> incentive shares
    Shares(Address, Address),
    /// (pool, provider) -> reward index already settled for this provider
    ClaimIndex(Address, Address),
    Fees,
    Volume,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidParams = 4,
    PoolNotFound = 5,
    PoolDisabled = 6,
    InsufficientLiquidity = 7,
    InvalidAmount = 8,
    Slippage = 9,
    NoRoute = 10,
    RouteMismatch = 11,
    TokenMismatch = 12,
    InconsistentRoute = 13,
    InsufficientShares = 14,
    PoolExists = 15,
    InsufficientIncentives = 16,
}

const REGISTERED: Symbol = symbol_short!("reg");
const UPDATED: Symbol = symbol_short!("updated");
const LIQUID: Symbol = symbol_short!("liquid");
const SWAP: Symbol = symbol_short!("swap");
const SPLIT: Symbol = symbol_short!("split");
const INCENT: Symbol = symbol_short!("incent");
const CLAIM: Symbol = symbol_short!("claim");
const PARAM: Symbol = symbol_short!("param");

#[contract]
pub struct LiquidityAggregator;

// ----------------------------------------------------------------------
// Pool storage and orientation
// ----------------------------------------------------------------------

fn load_pool(env: &Env, pool: &Address) -> Result<Pool, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::Pool(pool.clone()))
        .ok_or(Error::PoolNotFound)
}

fn save_pool(env: &Env, pool: &Pool) {
    env.storage()
        .persistent()
        .set(&DataKey::Pool(pool.address.clone()), pool);
}

/// Flip a stored pool so that `token_in` is the input side.
///
/// Pools are registered once in one direction; the other leg is the same curve
/// read backwards, so it is derived rather than stored.
fn orient(pool: &Pool, token_in: &Address) -> Option<Pool> {
    if pool.token_in == *token_in {
        return Some(pool.clone());
    }
    if pool.token_out != *token_in {
        return None;
    }
    Some(Pool {
        address: pool.address.clone(),
        token_in: pool.token_out.clone(),
        token_out: pool.token_in.clone(),
        reserve_in: pool.reserve_out,
        reserve_out: pool.reserve_in,
        fee_bps: pool.fee_bps,
        kind: pool.kind,
        enabled: pool.enabled,
        incentives: pool.incentives,
        incentive_shares: pool.incentive_shares,
        reward_index: pool.reward_index,
        updated_at: pool.updated_at,
    })
}

/// Write an oriented pool back in its registered direction.
fn write_oriented(env: &Env, view: &Pool) -> Result<(), Error> {
    let canonical = load_pool(env, &view.address)?;
    let saved = if view.token_in == canonical.token_in {
        view.clone()
    } else {
        Pool {
            token_in: view.token_out.clone(),
            token_out: view.token_in.clone(),
            reserve_in: view.reserve_out,
            reserve_out: view.reserve_in,
            ..view.clone()
        }
    };
    save_pool(env, &saved);
    Ok(())
}

// ----------------------------------------------------------------------
// Config and small helpers
// ----------------------------------------------------------------------

fn load_config(env: &Env) -> Result<Config, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::Config)
        .ok_or(Error::NotInitialized)
}

fn tokens(env: &Env) -> Vec<Address> {
    env.storage()
        .persistent()
        .get(&DataKey::Tokens)
        .unwrap_or(Vec::new(env))
}

fn pools_for(env: &Env, token_in: &Address, token_out: &Address) -> Vec<Address> {
    env.storage()
        .persistent()
        .get(&DataKey::Pools(token_in.clone(), token_out.clone()))
        .unwrap_or(Vec::new(env))
}

/// `soroban_sdk::Vec::contains` wants an equality bound `Address` does not
/// satisfy here, so membership is spelled out.
fn contains(list: &Vec<Address>, needle: &Address) -> bool {
    for item in list.iter() {
        if item == *needle {
            return true;
        }
    }
    false
}

fn index_pool(env: &Env, token_in: &Address, token_out: &Address, pool: &Address) {
    let mut list = pools_for(env, token_in, token_out);
    if !contains(&list, pool) {
        list.push_back(pool.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Pools(token_in.clone(), token_out.clone()), &list);
    }

    let mut known = tokens(env);
    if !contains(&known, token_in) {
        known.push_back(token_in.clone());
        env.storage().persistent().set(&DataKey::Tokens, &known);
    }
    if !contains(&known, token_out) {
        known.push_back(token_out.clone());
        env.storage().persistent().set(&DataKey::Tokens, &known);
    }

    let mut all: Vec<Address> = env
        .storage()
        .persistent()
        .get(&DataKey::AllPools)
        .unwrap_or(Vec::new(env));
    if !contains(&all, pool) {
        all.push_back(pool.clone());
        env.storage().persistent().set(&DataKey::AllPools, &all);
    }
}

fn load_shares(env: &Env, pool: &Address, provider: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::Shares(pool.clone(), provider.clone()))
        .unwrap_or(0)
}

fn set_shares(env: &Env, pool: &Address, provider: &Address, amount: i128) {
    env.storage().persistent().set(
        &DataKey::Shares(pool.clone(), provider.clone()),
        &amount,
    );
}

fn load_claim_index(env: &Env, pool: &Address, provider: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::ClaimIndex(pool.clone(), provider.clone()))
        .unwrap_or(0)
}

fn set_claim_index(env: &Env, pool: &Address, provider: &Address, index: i128) {
    env.storage().persistent().set(
        &DataKey::ClaimIndex(pool.clone(), provider.clone()),
        &index,
    );
}

fn token_of<'a>(env: &'a Env, address: &Address) -> token::Client<'a> {
    token::Client::new(env, address)
}

// ----------------------------------------------------------------------
// Pricing
// ----------------------------------------------------------------------

/// Output of swapping `amount_in` through `pool`, net of the pool's own fee.
fn quote_out(pool: &Pool, amount_in: i128) -> i128 {
    if amount_in <= 0 || pool.reserve_in <= 0 || pool.reserve_out <= 0 {
        return 0;
    }
    let fee = pool.fee_bps.min(9_999) as i128;

    let out = match pool.kind {
        // x * y = k, charging the fee on the way in.
        PoolKind::ConstantProduct => {
            let with_fee = amount_in * (BPS - fee);
            (pool.reserve_out * with_fee) / (pool.reserve_in * BPS + with_fee)
        }
        // x + y = k: the price barely moves near the peg, so a trade of `n`
        // returns roughly `n` less the fee. That is the property that makes a
        // stableswap pool the better fill for a large same-pair order.
        PoolKind::Stable => {
            let net = amount_in - (amount_in * fee) / BPS;
            let out = pool.reserve_in + net - pool.reserve_out;
            if out < 0 {
                0
            } else {
                out
            }
        }
        // External pools quote themselves at execution time.
        PoolKind::External => 0,
    };

    if out > pool.reserve_out {
        pool.reserve_out
    } else {
        out
    }
}

/// Fee charged by a pool for a given size.
fn pool_fee(pool: &Pool, amount_in: i128) -> i128 {
    (amount_in * pool.fee_bps.min(9_999) as i128) / BPS
}

/// Output at the pool's mid price, ignoring fees and curve effects.
fn mid_price_out(pool: &Pool, amount_in: i128) -> i128 {
    if pool.reserve_in <= 0 {
        return 0;
    }
    (amount_in * pool.reserve_out) / pool.reserve_in
}

fn impact_bps(pool: &Pool, amount_in: i128, amount_out: i128) -> u32 {
    let mid = mid_price_out(pool, amount_in);
    if mid <= 0 || amount_out >= mid {
        return 0;
    }
    let gap = (mid - amount_out) * BPS;
    if gap / mid > u32::MAX as i128 {
        return u32::MAX;
    }
    (gap / mid) as u32
}

/// Price one leg, or `None` when the pool cannot serve it.
fn price_leg(env: &Env, pool_addr: &Address, token_in: &Address, amount_in: i128) -> Option<RouteLeg> {
    let canonical = load_pool(env, pool_addr).ok()?;
    if !canonical.enabled {
        return None;
    }
    let pool = orient(&canonical, token_in)?;
    let out = quote_out(&pool, amount_in);
    if out <= 0 {
        return None;
    }
    Some(RouteLeg {
        pool: pool_addr.clone(),
        token_in: pool.token_in.clone(),
        token_out: pool.token_out.clone(),
        amount_in,
        amount_out: out,
        fee: pool_fee(&pool, amount_in),
        price_impact_bps: impact_bps(&pool, amount_in, out),
    })
}

/// Assemble a route from the legs walked so far.
fn build_route(legs: &Vec<RouteLeg>, amount_in: i128, penalty_bps: u32) -> Route {
    let mut expected_out: i128 = 0;
    let mut total_fee: i128 = 0;
    let mut impact: i128 = 0;
    let mut hops: i128 = 0;

    for leg in legs.iter() {
        expected_out = leg.amount_out;
        total_fee += leg.fee;
        impact += leg.price_impact_bps as i128;
        hops += 1;
    }

    // Discount every hop after the first so a longer path has to earn the extra
    // risk and gas before it wins. This is the fee-aware half of routing.
    let score = (expected_out * BPS) / (BPS + penalty_bps as i128 * (hops - 1).max(0));

    Route {
        legs: legs.clone(),
        amount_in,
        expected_out,
        total_fee,
        price_impact_bps: impact.min(u32::MAX as i128) as u32,
        score,
    }
}

/// Walk every simple path from `current` to `target`, collecting the ones that
/// get there. `visited` prevents cycles, `hops` bounds the length.
fn explore(
    env: &Env,
    penalty_bps: u32,
    current: &Address,
    target: &Address,
    amount_in: i128,
    amount_out: i128,
    max_hops: u32,
    hops: u32,
    legs: &mut Vec<RouteLeg>,
    visited: &mut Vec<Address>,
    found: &mut Vec<Route>,
) {
    if hops > 0 && current == target && amount_out > 0 {
        found.push_back(build_route(legs, amount_in, penalty_bps));
    }
    if hops >= max_hops {
        return;
    }

    for next in tokens(env).iter() {
        if next == *current {
            continue;
        }
        for pool_addr in pools_for(env, current, &next).iter() {
            if contains(visited, &pool_addr) {
                continue;
            }
            let leg = match price_leg(env, &pool_addr, current, amount_out) {
                Some(l) => l,
                None => continue,
            };

            visited.push_back(pool_addr.clone());
            legs.push_back(leg);

            explore(
                env,
                penalty_bps,
                &next,
                target,
                amount_in,
                legs.last().unwrap().amount_out,
                max_hops,
                hops + 1,
                legs,
                visited,
                found,
            );

            legs.pop_back();
            visited.pop_back();
        }
    }
}

/// Descending insertion sort over quotes. The candidate list is tiny, so this
/// keeps ordering deterministic without pulling in a sort primitive.
fn sort_quotes(mut list: Vec<Quote>) -> Vec<Quote> {
    let n = list.len();
    let mut i = 1u32;
    while i < n {
        let mut j = i;
        while j > 0 {
            let prev = list.get(j - 1).unwrap();
            let cur = list.get(j).unwrap();
            if prev.amount_out >= cur.amount_out {
                break;
            }
            list.set(j - 1, cur);
            list.set(j, prev);
            j -= 1;
        }
        i += 1;
    }
    list
}

/// Descending insertion sort over routes by routing score.
fn sort_routes(mut list: Vec<Route>) -> Vec<Route> {
    let n = list.len();
    let mut i = 1u32;
    while i < n {
        let mut j = i;
        while j > 0 {
            let prev = list.get(j - 1).unwrap();
            let cur = list.get(j).unwrap();
            if prev.score >= cur.score {
                break;
            }
            list.set(j - 1, cur);
            list.set(j, prev);
            j -= 1;
        }
        i += 1;
    }
    list
}

/// Every route from `token_in` to `token_out`, best first.
fn enumerate_routes(
    env: &Env,
    token_in: &Address,
    token_out: &Address,
    amount_in: i128,
    max_hops: u32,
    penalty_bps: u32,
) -> Vec<Route> {
    let mut found: Vec<Route> = Vec::new(env);
    if token_in == token_out {
        return found;
    }

    let mut legs: Vec<RouteLeg> = Vec::new(env);
    let mut visited: Vec<Address> = Vec::new(env);
    explore(
        env,
        penalty_bps,
        token_in,
        token_out,
        amount_in,
        amount_in,
        if max_hops == 0 { 1 } else { max_hops.min(MAX_HOPS) },
        0,
        &mut legs,
        &mut visited,
        &mut found,
    );

    sort_routes(found)
}

#[contractimpl]
impl LiquidityAggregator {
    // ------------------------------------------------------------------
    // Setup
    // ------------------------------------------------------------------

    /// Deploy-time configuration. May only be called once.
    pub fn initialize(
        env: Env,
        admin: Address,
        fee_recipient: Address,
        fee_bps: u32,
        routing_penalty_bps: u32,
    ) -> Result<(), Error> {
        if env.storage().persistent().has(&DataKey::Config) {
            return Err(Error::AlreadyInitialized);
        }
        if fee_bps >= BPS as u32 {
            return Err(Error::InvalidParams);
        }
        admin.require_auth();

        let config = Config {
            admin,
            fee_recipient,
            fee_bps,
            routing_penalty_bps,
        };
        env.storage().persistent().set(&DataKey::Config, &config);
        env.storage().persistent().set(&DataKey::Fees, &0i128);
        env.storage().persistent().set(&DataKey::Volume, &0i128);
        Ok(())
    }

    /// Adjust the protocol fee and/or its recipient.
    pub fn set_fee(env: Env, fee_bps: u32, fee_recipient: Address) -> Result<(), Error> {
        let mut config = load_config(&env)?;
        config.admin.require_auth();
        if fee_bps >= BPS as u32 {
            return Err(Error::InvalidParams);
        }
        config.fee_bps = fee_bps;
        config.fee_recipient = fee_recipient;
        env.storage().persistent().set(&DataKey::Config, &config);
        env.events().publish((PARAM,), fee_bps);
        Ok(())
    }

    /// Tune how strongly extra hops are penalised when ranking routes.
    pub fn set_routing_penalty(env: Env, penalty_bps: u32) -> Result<(), Error> {
        let mut config = load_config(&env)?;
        config.admin.require_auth();
        if penalty_bps >= BPS as u32 {
            return Err(Error::InvalidParams);
        }
        config.routing_penalty_bps = penalty_bps;
        env.storage().persistent().set(&DataKey::Config, &config);
        env.events().publish((PARAM,), penalty_bps);
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

    // ------------------------------------------------------------------
    // Pool registry
    // ------------------------------------------------------------------

    /// Register a pool for a token pair. Both directions become routable.
    pub fn register_pool(
        env: Env,
        pool: Address,
        token_in: Address,
        token_out: Address,
        reserve_in: i128,
        reserve_out: i128,
        fee_bps: u32,
        kind: PoolKind,
    ) -> Result<(), Error> {
        let config = load_config(&env)?;
        config.admin.require_auth();

        if token_in == token_out {
            return Err(Error::TokenMismatch);
        }
        if reserve_in <= 0 || reserve_out <= 0 {
            return Err(Error::InvalidParams);
        }
        if fee_bps >= BPS as u32 {
            return Err(Error::InvalidParams);
        }
        if env.storage().persistent().has(&DataKey::Pool(pool.clone())) {
            return Err(Error::PoolExists);
        }

        let entry = Pool {
            address: pool.clone(),
            token_in: token_in.clone(),
            token_out: token_out.clone(),
            reserve_in,
            reserve_out,
            fee_bps,
            kind,
            enabled: true,
            incentives: 0,
            incentive_shares: 0,
            reward_index: 0,
            updated_at: env.ledger().timestamp(),
        };

        save_pool(&env, &entry);
        index_pool(&env, &token_in, &token_out, &pool);
        index_pool(&env, &token_out, &token_in, &pool);

        env.events().publish((REGISTERED, pool), entry);
        Ok(())
    }

    /// Refresh a pool's reserves, normally from a keeper.
    pub fn sync_pool(
        env: Env,
        pool: Address,
        token_in: Address,
        reserve_in: i128,
        reserve_out: i128,
    ) -> Result<(), Error> {
        load_config(&env)?;
        let mut view = orient(&load_pool(&env, &pool)?, &token_in)
            .ok_or(Error::TokenMismatch)?;

        if reserve_in <= 0 || reserve_out <= 0 {
            return Err(Error::InvalidParams);
        }
        view.reserve_in = reserve_in;
        view.reserve_out = reserve_out;
        view.updated_at = env.ledger().timestamp();
        write_oriented(&env, &view)?;
        env.events().publish((UPDATED, pool), (reserve_in, reserve_out));
        Ok(())
    }

    /// Take a pool in or out of service without unregistering it.
    pub fn set_pool_enabled(env: Env, pool: Address, enabled: bool) -> Result<(), Error> {
        let config = load_config(&env)?;
        config.admin.require_auth();

        let mut entry = load_pool(&env, &pool)?;
        entry.enabled = enabled;
        save_pool(&env, &entry);
        env.events().publish((UPDATED, pool), enabled);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Liquidity
    // ------------------------------------------------------------------

    /// Provide liquidity the aggregator can route against.
    pub fn add_liquidity(
        env: Env,
        provider: Address,
        pool: Address,
        token_addr: Address,
        amount: i128,
    ) -> Result<i128, Error> {
        provider.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let mut view = orient(&load_pool(&env, &pool)?, &token_addr)
            .ok_or(Error::TokenMismatch)?;

        token_of(&env, &token_addr).transfer(
            &provider,
            &env.current_contract_address(),
            &amount,
        );

        if view.token_in == token_addr {
            view.reserve_in += amount;
        } else {
            view.reserve_out += amount;
        }
        view.updated_at = env.ledger().timestamp();
        write_oriented(&env, &view)?;

        let remaining = if view.token_in == token_addr {
            view.reserve_in
        } else {
            view.reserve_out
        };
        env.events().publish(
            (LIQUID, pool),
            (provider.clone(), token_addr, amount),
        );
        Ok(remaining)
    }

    /// Withdraw liquidity the aggregator is not routing against.
    pub fn remove_liquidity(
        env: Env,
        provider: Address,
        pool: Address,
        token_addr: Address,
        amount: i128,
    ) -> Result<i128, Error> {
        provider.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let mut view = orient(&load_pool(&env, &pool)?, &token_addr)
            .ok_or(Error::TokenMismatch)?;
        let reserved = if view.token_in == token_addr {
            view.reserve_in
        } else {
            view.reserve_out
        };
        if reserved <= amount {
            return Err(Error::InsufficientLiquidity);
        }
        if token_of(&env, &token_addr).balance(&env.current_contract_address()) < amount {
            return Err(Error::InsufficientLiquidity);
        }

        if view.token_in == token_addr {
            view.reserve_in -= amount;
        } else {
            view.reserve_out -= amount;
        }
        write_oriented(&env, &view)?;
        token_of(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &provider,
            &amount,
        );

        let remaining = if view.token_in == token_addr {
            view.reserve_in
        } else {
            view.reserve_out
        };
        env.events().publish(
            (LIQUID, pool),
            (provider.clone(), token_addr, amount),
        );
        Ok(remaining)
    }

    // ------------------------------------------------------------------
    // Quotes and rate comparison
    // ------------------------------------------------------------------

    /// Price a swap on a single pool.
    pub fn quote(
        env: Env,
        pool: Address,
        token_in: Address,
        amount_in: i128,
    ) -> Result<Quote, Error> {
        let view = orient(&load_pool(&env, &pool)?, &token_in)
            .ok_or(Error::TokenMismatch)?;
        if !view.enabled {
            return Err(Error::PoolDisabled);
        }
        if amount_in <= 0 {
            return Err(Error::InvalidAmount);
        }

        let amount_out = quote_out(&view, amount_in);
        if amount_out <= 0 {
            return Err(Error::InsufficientLiquidity);
        }
        let fee = pool_fee(&view, amount_in);
        let impact = impact_bps(&view, amount_in, amount_out);

        Ok(Quote {
            pool,
            token_in: view.token_in,
            token_out: view.token_out,
            amount_in,
            amount_out,
            fee,
            price_impact_bps: impact,
        })
    }

    /// Price the same swap across every registered pool, best first.
    pub fn compare_rates(
        env: Env,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
    ) -> Result<Vec<Quote>, Error> {
        if amount_in <= 0 {
            return Err(Error::InvalidAmount);
        }
        if token_in == token_out {
            return Err(Error::TokenMismatch);
        }

        let mut out: Vec<Quote> = Vec::new(&env);
        for pool in pools_for(&env, &token_in, &token_out).iter() {
            let canonical = match load_pool(&env, &pool) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let view = match orient(&canonical, &token_in) {
                Some(v) => v,
                None => continue,
            };
            if !view.enabled {
                continue;
            }
            let amount_out = quote_out(&view, amount_in);
            if amount_out <= 0 {
                continue;
            }
            out.push_back(Quote {
                pool: pool.clone(),
                token_in: view.token_in.clone(),
                token_out: view.token_out.clone(),
                amount_in,
                amount_out,
                fee: pool_fee(&view, amount_in),
                price_impact_bps: impact_bps(&view, amount_in, amount_out),
            });
        }

        Ok(sort_quotes(out))
    }

    /// The single best pool for a swap.
    pub fn best_route(
        env: Env,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
    ) -> Result<Quote, Error> {
        let quotes = Self::compare_rates(env, token_in, token_out, amount_in)?;
        if quotes.is_empty() {
            return Err(Error::NoRoute);
        }
        quotes.get(0).ok_or(Error::NoRoute)
    }

    // ------------------------------------------------------------------
    // Path optimisation
    // ------------------------------------------------------------------

    /// Every viable path, ranked by score. `max_hops` is clamped to three.
    pub fn find_paths(
        env: Env,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
        max_hops: u32,
    ) -> Result<Vec<Route>, Error> {
        if amount_in <= 0 {
            return Err(Error::InvalidAmount);
        }
        if token_in == token_out {
            return Err(Error::TokenMismatch);
        }
        let config = load_config(&env)?;
        let routes = enumerate_routes(
            &env,
            &token_in,
            &token_out,
            amount_in,
            max_hops,
            config.routing_penalty_bps,
        );
        if routes.is_empty() {
            return Err(Error::NoRoute);
        }
        Ok(routes)
    }

    /// The single best path, hops included.
    pub fn find_path(
        env: Env,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
        max_hops: u32,
    ) -> Result<Route, Error> {
        let routes = Self::find_paths(env, token_in, token_out, amount_in, max_hops)?;
        routes.get(0).ok_or(Error::NoRoute)
    }

    /// Fan an order out across several paths.
    ///
    /// Each candidate may absorb at most `MAX_LEG_FILL_BPS` of its first pool's
    /// input reserve. If the candidates together cannot hold the whole order the
    /// input is shared out in proportion to those capacities, so the split
    /// always sums back to `amount_in`.
    pub fn find_split_routes(
        env: Env,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
        max_routes: u32,
    ) -> Result<Vec<Route>, Error> {
        let candidates = Self::find_paths(env.clone(), token_in.clone(), token_out, amount_in, MAX_HOPS)?;

        let want = if max_routes == 0 {
            1
        } else {
            max_routes.min(MAX_SPLIT_ROUTES)
        };
        let limit = if candidates.len() < want {
            candidates.len()
        } else {
            want
        };

        // Capacity of each candidate, taken from its first pool.
        let mut caps: Vec<i128> = Vec::new(&env);
        let mut total_cap: i128 = 0;
        let mut i = 0u32;
        while i < limit {
            let route = candidates.get(i).ok_or(Error::NoRoute)?;
            let leg = route.legs.first().ok_or(Error::InconsistentRoute)?;
            let canonical = load_pool(&env, &leg.pool)?;
            let view = orient(&canonical, &leg.token_in).ok_or(Error::TokenMismatch)?;
            let capacity = (view.reserve_in * MAX_LEG_FILL_BPS) / BPS;
            let capacity = if capacity <= 0 { amount_in } else { capacity };
            caps.push_back(capacity);
            total_cap += capacity;
            i += 1;
        }

        // Hand out the input, best route first. Allocations are decided up
        // front so the split always sums back to `amount_in`.
        let mut allocations: Vec<i128> = Vec::new(&env);
        let mut running: i128 = 0;
        i = 0;
        while i < limit {
            let capacity = caps.get(i).unwrap_or(amount_in);
            let allocation = if total_cap >= amount_in {
                if capacity < amount_in - running {
                    capacity
                } else {
                    amount_in - running
                }
            } else {
                (amount_in * capacity) / total_cap
            };
            allocations.push_back(allocation);
            running += allocation;
            i += 1;
        }
        // Absorb any rounding remainder into the last leg.
        if running < amount_in {
            if let Some(last) = allocations.last() {
                allocations.set(limit - 1, last + (amount_in - running));
            }
        }

        let mut splits: Vec<Route> = Vec::new(&env);
        i = 0;
        while i < limit {
            let allocation = allocations.get(i).unwrap_or(0);
            if allocation > 0 {
                let route = candidates.get(i).ok_or(Error::NoRoute)?;
                if let Some(resized) = Self::resize_route(&env, &route, allocation)? {
                    if resized.expected_out > 0 {
                        splits.push_back(resized);
                    }
                }
            }
            i += 1;
        }

        if splits.is_empty() {
            return Err(Error::NoRoute);
        }
        Ok(splits)
    }

    // ------------------------------------------------------------------
    // Execution
    // ------------------------------------------------------------------

    /// Execute a single route. Every leg settles inside this one invocation, so
    /// a failure part-way through reverts the whole swap.
    pub fn swap(
        env: Env,
        user: Address,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
        min_amount_out: i128,
        route: Route,
    ) -> Result<i128, Error> {
        user.require_auth();
        let config = load_config(&env)?;
        if amount_in <= 0 {
            return Err(Error::InvalidAmount);
        }
        if token_in == token_out {
            return Err(Error::TokenMismatch);
        }
        if route.legs.is_empty() {
            return Err(Error::InconsistentRoute);
        }

        // The route has to actually describe the pair being swapped.
        let first = route.legs.first().ok_or(Error::InconsistentRoute)?;
        if first.token_in != token_in {
            return Err(Error::RouteMismatch);
        }
        let last = route.legs.last().ok_or(Error::InconsistentRoute)?;
        if last.token_out != token_out {
            return Err(Error::RouteMismatch);
        }

        token_of(&env, &token_in).transfer(
            &user,
            &env.current_contract_address(),
            &amount_in,
        );

        let out = Self::walk(&env, &route, amount_in)?;
        let fee = (out * config.fee_bps as i128) / BPS;
        let net = out - fee;

        if net < min_amount_out {
            return Err(Error::Slippage);
        }

        if fee > 0 {
            token_of(&env, &token_out).transfer(
                &env.current_contract_address(),
                &config.fee_recipient,
                &fee,
            );
        }
        token_of(&env, &token_out).transfer(
            &env.current_contract_address(),
            &user,
            &net,
        );

        Self::bookkeep(&env, amount_in, fee);
        env.events()
            .publish((SWAP,), (user.clone(), amount_in, net, fee));
        Ok(net)
    }

    /// Execute a fan-out across several routes in one atomic call.
    pub fn execute_split(
        env: Env,
        user: Address,
        token_in: Address,
        amount_in: i128,
        min_amount_out: i128,
        routes: Vec<Route>,
    ) -> Result<i128, Error> {
        user.require_auth();
        let config = load_config(&env)?;
        if amount_in <= 0 {
            return Err(Error::InvalidAmount);
        }
        if routes.is_empty() {
            return Err(Error::InconsistentRoute);
        }

        // The split has to account for the entire order, and every route must
        // start from the same token.
        let mut allocated: i128 = 0;
        for route in routes.iter() {
            allocated += route.amount_in;
            let first = route.legs.first().ok_or(Error::InconsistentRoute)?;
            if first.token_in != token_in {
                return Err(Error::RouteMismatch);
            }
        }
        if allocated != amount_in {
            return Err(Error::InconsistentRoute);
        }

        let last = routes.last().ok_or(Error::InconsistentRoute)?;
        let final_leg = last.legs.last().ok_or(Error::InconsistentRoute)?;
        let token_out = final_leg.token_out.clone();

        token_of(&env, &token_in).transfer(
            &user,
            &env.current_contract_address(),
            &amount_in,
        );

        let mut total_out: i128 = 0;
        for route in routes.iter() {
            total_out += Self::walk(&env, &route, route.amount_in)?;
        }

        let fee = (total_out * config.fee_bps as i128) / BPS;
        let net = total_out - fee;
        if net < min_amount_out {
            return Err(Error::Slippage);
        }

        if fee > 0 {
            token_of(&env, &token_out).transfer(
                &env.current_contract_address(),
                &config.fee_recipient,
                &fee,
            );
        }
        token_of(&env, &token_out).transfer(
            &env.current_contract_address(),
            &user,
            &net,
        );

        Self::bookkeep(&env, amount_in, fee);
        env.events()
            .publish((SPLIT,), (user.clone(), amount_in, net, fee));
        Ok(net)
    }

    // ------------------------------------------------------------------
    // Liquidity incentives
    // ------------------------------------------------------------------

    /// Add to a pool's reward budget and take shares in it.
    ///
    /// The budget is booked into a cumulative reward index before the new
    /// shares are issued, so a funder never earns a reward on the very tokens
    /// they just deposited.
    pub fn fund_incentives(
        env: Env,
        provider: Address,
        pool: Address,
        amount: i128,
    ) -> Result<i128, Error> {
        provider.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let mut entry = load_pool(&env, &pool)?;
        let token_addr = entry.token_in.clone();
        token_of(&env, &token_addr).transfer(
            &provider,
            &env.current_contract_address(),
            &amount,
        );

        let existing = load_shares(&env, &pool, &provider);
        let outstanding = entry.incentive_shares;

        // The increment is booked before the new shares are issued, so it is
        // divided by the shares that were actually at risk.
        if outstanding > 0 {
            entry.reward_index += (amount * REWARD_SCALE) / outstanding;
        }
        entry.incentives += amount;
        entry.incentive_shares = outstanding + amount;

        let shares = existing + amount;
        set_shares(&env, &pool, &provider, shares);

        if existing == 0 {
            // A first-time funder joins at the post-increment index and so
            // earns nothing on the very tokens they just deposited.
            set_claim_index(&env, &pool, &provider, entry.reward_index);
        }
        save_pool(&env, &entry);

        env.events()
            .publish((INCENT, pool), (provider.clone(), amount, shares));
        Ok(shares)
    }

    /// Claim everything accrued to `provider` since their last claim.
    pub fn claim_incentives(
        env: Env,
        provider: Address,
        pool: Address,
    ) -> Result<i128, Error> {
        provider.require_auth();
        let entry = load_pool(&env, &pool)?;

        let shares = load_shares(&env, &pool, &provider);
        if shares <= 0 {
            return Err(Error::InsufficientShares);
        }

        let settled = load_claim_index(&env, &pool, &provider);
        if entry.reward_index <= settled {
            return Ok(0);
        }

        let payout = ((entry.reward_index - settled) * shares) / REWARD_SCALE;
        if payout <= 0 {
            return Ok(0);
        }

        let token_addr = entry.token_in.clone();
        if token_of(&env, &token_addr).balance(&env.current_contract_address()) < payout {
            return Err(Error::InsufficientIncentives);
        }

        set_claim_index(&env, &pool, &provider, entry.reward_index);
        save_pool(&env, &entry);
        token_of(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &provider,
            &payout,
        );

        env.events()
            .publish((CLAIM, pool), (provider.clone(), payout));
        Ok(payout)
    }

    // ------------------------------------------------------------------
    // Views
    // ------------------------------------------------------------------

    pub fn get_pool(env: Env, pool: Address) -> Result<Pool, Error> {
        load_pool(&env, &pool)
    }

    pub fn get_config(env: Env) -> Result<Config, Error> {
        load_config(&env)
    }

    /// Pools indexed for a pair, in registration order.
    pub fn pools_for(env: Env, token_in: Address, token_out: Address) -> Vec<Address> {
        pools_for(&env, &token_in, &token_out)
    }

    /// Every token the aggregator knows about.
    pub fn all_tokens(env: Env) -> Vec<Address> {
        tokens(&env)
    }

    pub fn pool_count(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::AllPools)
            .map(|v: Vec<Address>| v.len())
            .unwrap_or(0)
    }

    /// Cumulative protocol fees taken.
    pub fn fees_collected(env: Env) -> i128 {
        env.storage().persistent().get(&DataKey::Fees).unwrap_or(0)
    }

    /// Cumulative input volume routed.
    pub fn volume(env: Env) -> i128 {
        env.storage().persistent().get(&DataKey::Volume).unwrap_or(0)
    }

    pub fn incentive_shares(env: Env, pool: Address, provider: Address) -> i128 {
        load_shares(&env, &pool, &provider)
    }

    /// Reward index this provider has already settled up to.
    pub fn claim_index(env: Env, pool: Address, provider: Address) -> i128 {
        load_claim_index(&env, &pool, &provider)
    }

    // ------------------------------------------------------------------
    // Internals
    // ------------------------------------------------------------------

    fn bookkeep(env: &Env, amount_in: i128, fee: i128) {
        let fees: i128 = env.storage().persistent().get(&DataKey::Fees).unwrap_or(0);
        let volume: i128 = env.storage().persistent().get(&DataKey::Volume).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::Fees, &(fees + fee));
        env.storage()
            .persistent()
            .set(&DataKey::Volume, &(volume + amount_in));
    }

    /// Push `amount_in` through every leg of a route, settling each pool.
    ///
    /// Each leg's output stays in custody and becomes the next leg's input, so
    /// the whole path is self-financing.
    fn walk(env: &Env, route: &Route, amount_in: i128) -> Result<i128, Error> {
        let me = env.current_contract_address();
        let mut carry = amount_in;

        for leg in route.legs.iter() {
            let mut view = orient(&load_pool(env, &leg.pool)?, &leg.token_in)
                .ok_or(Error::TokenMismatch)?;
            if !view.enabled {
                return Err(Error::PoolDisabled);
            }

            let out = if view.kind == PoolKind::External {
                Self::call_external(env, &view, &me, carry)?
            } else {
                quote_out(&view, carry)
            };
            if out <= 0 {
                return Err(Error::InsufficientLiquidity);
            }
            if token_of(env, &view.token_out).balance(&me) < out {
                return Err(Error::InsufficientLiquidity);
            }

            view.reserve_in += carry;
            view.reserve_out -= out;
            view.updated_at = env.ledger().timestamp();
            write_oriented(env, &view)?;

            carry = out;
        }

        Ok(carry)
    }

    /// Call a third-party pool's `swap(to, amount_in, min_amount_out)`.
    fn call_external(
        env: &Env,
        pool: &Pool,
        to: &Address,
        amount_in: i128,
    ) -> Result<i128, Error> {
        let out: i128 = env.invoke_contract(
            &pool.address,
            &Symbol::new(env, "swap"),
            vec![env, to.into_val(env), amount_in.into_val(env), 0i128.into_val(env)],
        );
        if out <= 0 {
            return Err(Error::InsufficientLiquidity);
        }
        Ok(out)
    }

    /// Re-quote an existing route for a different input size.
    fn resize_route(
        env: &Env,
        route: &Route,
        amount_in: i128,
    ) -> Result<Option<Route>, Error> {
        let penalty_bps = load_config(env)?.routing_penalty_bps;
        let mut legs: Vec<RouteLeg> = Vec::new(env);
        let mut carry = amount_in;

        for leg in route.legs.iter() {
            match price_leg(env, &leg.pool, &leg.token_in, carry) {
                Some(next) => {
                    carry = next.amount_out;
                    legs.push_back(next);
                }
                None => return Ok(None),
            }
        }

        if legs.is_empty() {
            return Ok(None);
        }
        Ok(Some(build_route(&legs, amount_in, penalty_bps)))
    }
}
