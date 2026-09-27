#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, Symbol,
    Vec,
};

/// Fee denominator (100% == 10_000 bps).
const BASIS_POINTS: i128 = 10_000;
/// Fixed-point scale for DEX prices. A price of `PRICE_SCALE` means
/// "1 unit of token_in buys 1 unit of token_out".
const PRICE_SCALE: i128 = 10_000_000;
/// Hard cap on the number of registered venues so the registry stays bounded.
const MAX_DEXES: u32 = 32;
/// Hard cap on how many venues a single split trade can fan out to.
const MAX_SPLIT_VENUES: u32 = 4;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AggregateError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    DexAlreadyRegistered = 4,
    DexNotFound = 5,
    InvalidPrice = 6,
    InvalidFee = 7,
    InvalidAmount = 8,
    IdenticalTokens = 9,
    NoRoute = 10,
    SlippageExceeded = 11,
    DeadlineExpired = 12,
    InsufficientLiquidity = 13,
    RegistryFull = 14,
    InvalidVenueCount = 15,
    RouteNotFound = 16,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
}

/// A registered exchange venue with a deterministic quoted rate.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DexEntry {
    pub id: Symbol,
    pub token_in: Address,
    pub token_out: Address,
    /// Fixed-point rate: units of `token_out` per 1 unit of `token_in`,
    /// scaled by `PRICE_SCALE`.
    pub price: i128,
    pub fee_bps: u32,
    pub enabled: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BestQuote {
    pub dex_id: Symbol,
    pub amount_in: i128,
    pub amount_out: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwapLeg {
    pub dex_id: Symbol,
    pub amount_in: i128,
    pub amount_out: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Route {
    pub token_in: Address,
    pub token_out: Address,
    pub legs: Vec<SwapLeg>,
    pub total_in: i128,
    pub total_out: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Receipt {
    pub trader: Address,
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in: i128,
    pub amount_out: i128,
    pub venues: u32,
    pub timestamp: u64,
}

/// Cumulative, bounded swap analytics.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Analytics {
    pub swap_count: u64,
    pub total_volume_in: i128,
    pub total_volume_out: i128,
}

#[contracttype]
pub enum DataKey {
    Config,
    Registry,
    Dex(Symbol),
    Analytics,
    LastRoute,
    Liquidity(Address),
}

#[contract]
pub struct SwapAggregatorContract;

#[contractimpl]
impl SwapAggregatorContract {
    /// One-time setup; stores the admin allowed to manage the registry.
    pub fn initialize(env: Env, admin: Address) -> Result<(), AggregateError> {
        if env.storage().instance().has(&DataKey::Config) {
            return Err(AggregateError::AlreadyInitialized);
        }
        admin.require_auth();

        env.storage().instance().set(&DataKey::Config, &Config { admin });
        env.storage()
            .persistent()
            .set(&DataKey::Registry, &Vec::<Symbol>::new(&env));

        Ok(())
    }

    // --- DEX registry -----------------------------------------------------

    /// Register a new venue. Only the stored admin may register.
    pub fn register_dex(
        env: Env,
        admin: Address,
        dex_id: Symbol,
        token_in: Address,
        token_out: Address,
        price: i128,
        fee_bps: u32,
    ) -> Result<(), AggregateError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        Self::validate_venue(&token_in, &token_out, price, fee_bps)?;

        if env
            .storage()
            .persistent()
            .has(&DataKey::Dex(dex_id.clone()))
        {
            return Err(AggregateError::DexAlreadyRegistered);
        }

        let mut registry = Self::get_registry(&env)?;
        if registry.len() >= MAX_DEXES {
            return Err(AggregateError::RegistryFull);
        }

        let entry = DexEntry {
            id: dex_id.clone(),
            token_in,
            token_out,
            price,
            fee_bps,
            enabled: true,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Dex(dex_id.clone()), &entry);
        registry.push_back(dex_id.clone());
        env.storage().persistent().set(&DataKey::Registry, &registry);

        env.events()
            .publish((symbol_short!("dex_reg"), dex_id), (price, fee_bps));

        Ok(())
    }

    /// Update the rate and fee of an existing venue (admin only).
    pub fn update_dex(
        env: Env,
        admin: Address,
        dex_id: Symbol,
        price: i128,
        fee_bps: u32,
    ) -> Result<(), AggregateError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        if price <= 0 {
            return Err(AggregateError::InvalidPrice);
        }
        if fee_bps > BASIS_POINTS as u32 {
            return Err(AggregateError::InvalidFee);
        }

        let mut entry = Self::must_get_dex(&env, &dex_id)?;
        entry.price = price;
        entry.fee_bps = fee_bps;
        env.storage()
            .persistent()
            .set(&DataKey::Dex(dex_id.clone()), &entry);

        env.events()
            .publish((symbol_short!("dex_upd"), dex_id), (price, fee_bps));

        Ok(())
    }

    /// Enable or disable (pause) a venue.
    pub fn set_dex_enabled(
        env: Env,
        admin: Address,
        dex_id: Symbol,
        enabled: bool,
    ) -> Result<(), AggregateError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        let mut entry = Self::must_get_dex(&env, &dex_id)?;
        entry.enabled = enabled;
        env.storage()
            .persistent()
            .set(&DataKey::Dex(dex_id.clone()), &entry);

        env.events()
            .publish((symbol_short!("dex_set"), dex_id), enabled);

        Ok(())
    }

    /// Pause a venue so it is excluded from routing.
    pub fn pause_dex(env: Env, admin: Address, dex_id: Symbol) -> Result<(), AggregateError> {
        Self::set_dex_enabled_impl(&env, &admin, &dex_id, false)
    }

    /// Re-enable a previously paused venue.
    pub fn resume_dex(env: Env, admin: Address, dex_id: Symbol) -> Result<(), AggregateError> {
        Self::set_dex_enabled_impl(&env, &admin, &dex_id, true)
    }

    /// Remove a venue from the registry entirely (admin only).
    pub fn remove_dex(env: Env, admin: Address, dex_id: Symbol) -> Result<(), AggregateError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        // Ensure it exists before removing.
        Self::must_get_dex(&env, &dex_id)?;

        let registry = Self::get_registry(&env)?;
        let mut new_registry = Vec::new(&env);
        for existing in registry.iter() {
            if existing != dex_id {
                new_registry.push_back(existing);
            }
        }
        env.storage()
            .persistent()
            .set(&DataKey::Registry, &new_registry);
        env.storage()
            .persistent()
            .remove(&DataKey::Dex(dex_id.clone()));

        env.events().publish((symbol_short!("dex_rem"), dex_id), ());

        Ok(())
    }

    // --- Price feeds ------------------------------------------------------

    /// Push a new rate for a venue (admin only).
    pub fn update_price(
        env: Env,
        admin: Address,
        dex_id: Symbol,
        new_price: i128,
    ) -> Result<(), AggregateError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        if new_price <= 0 {
            return Err(AggregateError::InvalidPrice);
        }

        let mut entry = Self::must_get_dex(&env, &dex_id)?;
        entry.price = new_price;
        env.storage()
            .persistent()
            .set(&DataKey::Dex(dex_id.clone()), &entry);

        env.events().publish(
            (symbol_short!("price_upd"), dex_id),
            (new_price, env.ledger().timestamp()),
        );

        Ok(())
    }

    /// Current rate for a venue.
    pub fn get_price(env: Env, dex_id: Symbol) -> Result<i128, AggregateError> {
        Ok(Self::must_get_dex(&env, &dex_id)?.price)
    }

    /// Full venue record.
    pub fn get_dex(env: Env, dex_id: Symbol) -> Result<DexEntry, AggregateError> {
        Self::must_get_dex(&env, &dex_id)
    }

    /// All registered venues (enabled or paused).
    pub fn get_dexes(env: Env) -> Result<Vec<DexEntry>, AggregateError> {
        let registry = Self::get_registry(&env)?;
        let mut entries = Vec::new(&env);
        for dex_id in registry.iter() {
            entries.push_back(Self::must_get_dex(&env, &dex_id)?);
        }
        Ok(entries)
    }

    /// Number of registered venues.
    pub fn dex_count(env: Env) -> Result<u32, AggregateError> {
        Ok(Self::get_registry(&env)?.len())
    }

    // --- Quotation & routing ---------------------------------------------

    /// Best single-venue quote for the requested pair, after fees.
    pub fn quote_best(
        env: Env,
        amount_in: i128,
        token_in: Address,
        token_out: Address,
    ) -> Result<BestQuote, AggregateError> {
        if amount_in <= 0 {
            return Err(AggregateError::InvalidAmount);
        }

        let registry = Self::get_registry(&env)?;
        let mut best_out: i128 = 0;
        let mut best_id: Option<Symbol> = None;

        for dex_id in registry.iter() {
            let entry = Self::must_get_dex(&env, &dex_id)?;
            if !entry.enabled {
                continue;
            }
            if &entry.token_in != &token_in || &entry.token_out != &token_out {
                continue;
            }

            let out = Self::net_out(&entry, amount_in);
            if out > best_out {
                best_out = out;
                best_id = Some(dex_id.clone());
            }
        }

        match best_id {
            Some(dex_id) => Ok(BestQuote {
                dex_id,
                amount_in,
                amount_out: best_out,
            }),
            None => Err(AggregateError::NoRoute),
        }
    }

    /// Quote a specific venue directly.
    pub fn quote_dex(
        env: Env,
        dex_id: Symbol,
        amount_in: i128,
    ) -> Result<i128, AggregateError> {
        if amount_in <= 0 {
            return Err(AggregateError::InvalidAmount);
        }
        let entry = Self::must_get_dex(&env, &dex_id)?;
        if !entry.enabled {
            return Err(AggregateError::NoRoute);
        }
        Ok(Self::net_out(&entry, amount_in))
    }

    /// Plan an optimal split route without executing it: routes `amount_in`
    /// across up to `max_venues` venues weighted by their effective rate.
    pub fn plan_route(
        env: Env,
        amount_in: i128,
        token_in: Address,
        token_out: Address,
        max_venues: u32,
    ) -> Result<Route, AggregateError> {
        Self::build_route(&env, amount_in, &token_in, &token_out, max_venues)
    }

    // --- Execution --------------------------------------------------------

    /// Execute a single-venue swap with a slippage floor and a deadline.
    pub fn execute_swap(
        env: Env,
        trader: Address,
        amount_in: i128,
        token_in: Address,
        token_out: Address,
        min_amount_out: i128,
        deadline: u64,
    ) -> Result<Receipt, AggregateError> {
        trader.require_auth();
        if amount_in <= 0 {
            return Err(AggregateError::InvalidAmount);
        }
        // MEV guard: reject swaps that can no longer be filled in time.
        if env.ledger().timestamp() > deadline {
            return Err(AggregateError::DeadlineExpired);
        }

        let quote = Self::quote_best(&env, amount_in, token_in.clone(), token_out.clone())?;
        // MEV guard: reject if the achievable output is below the trader's floor.
        if quote.amount_out < min_amount_out {
            return Err(AggregateError::SlippageExceeded);
        }

        Self::settle(
            &env,
            &trader,
            &token_in,
            &token_out,
            amount_in,
            quote.amount_out,
        )?;

        let mut legs = Vec::new(&env);
        legs.push_back(SwapLeg {
            dex_id: quote.dex_id.clone(),
            amount_in,
            amount_out: quote.amount_out,
        });
        let route = Route {
            token_in: token_in.clone(),
            token_out: token_out.clone(),
            legs,
            total_in: amount_in,
            total_out: quote.amount_out,
        };
        Self::record_execution(&env, &route);

        let receipt = Receipt {
            trader: trader.clone(),
            token_in,
            token_out,
            amount_in,
            amount_out: quote.amount_out,
            venues: 1,
            timestamp: env.ledger().timestamp(),
        };

        env.events().publish(
            (symbol_short!("swap_exec"), trader),
            (amount_in, quote.amount_out, route.legs.len()),
        );

        Ok(receipt)
    }

    /// Execute a split trade across up to `max_venues` venues. The returned
    /// route carries the per-leg allocation and the aggregate output.
    pub fn split_swap(
        env: Env,
        trader: Address,
        amount_in: i128,
        token_in: Address,
        token_out: Address,
        max_venues: u32,
        min_amount_out: i128,
        deadline: u64,
    ) -> Result<Route, AggregateError> {
        trader.require_auth();
        if amount_in <= 0 {
            return Err(AggregateError::InvalidAmount);
        }
        if max_venues == 0 {
            return Err(AggregateError::InvalidVenueCount);
        }
        if env.ledger().timestamp() > deadline {
            return Err(AggregateError::DeadlineExpired);
        }

        let route = Self::build_route(&env, amount_in, &token_in, &token_out, max_venues)?;

        if route.total_out < min_amount_out {
            return Err(AggregateError::SlippageExceeded);
        }

        Self::settle(
            &env,
            &trader,
            &token_in,
            &token_out,
            amount_in,
            route.total_out,
        )?;
        Self::record_execution(&env, &route);

        env.events().publish(
            (symbol_short!("split_ex"), trader),
            (amount_in, route.total_out, route.legs.len()),
        );

        Ok(route)
    }

    // --- Liquidity --------------------------------------------------------

    /// Fund the aggregator so it can pay out `asset` on executed swaps.
    pub fn add_liquidity(
        env: Env,
        admin: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), AggregateError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        if amount <= 0 {
            return Err(AggregateError::InvalidAmount);
        }

        let current = Self::get_liquidity(&env, &asset);
        token::Client::new(&env, &asset).transfer(
            &admin,
            &env.current_contract_address(),
            &amount,
        );
        env.storage()
            .persistent()
            .set(&DataKey::Liquidity(asset.clone()), &(current + amount));

        env.events()
            .publish((symbol_short!("liq_add"), admin), (asset, amount));

        Ok(())
    }

    /// Withdraw unused liquidity (admin only).
    pub fn withdraw_liquidity(
        env: Env,
        admin: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), AggregateError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        if amount <= 0 {
            return Err(AggregateError::InvalidAmount);
        }

        let current = Self::get_liquidity(&env, &asset);
        if amount > current {
            return Err(AggregateError::InsufficientLiquidity);
        }

        token::Client::new(&env, &asset).transfer(
            &env.current_contract_address(),
            &admin,
            &amount,
        );
        env.storage()
            .persistent()
            .set(&DataKey::Liquidity(asset.clone()), &(current - amount));

        env.events()
            .publish((symbol_short!("liq_rem"), admin), (asset, amount));

        Ok(())
    }

    /// Tracked liquidity for an asset.
    pub fn liquidity(env: Env, asset: Address) -> i128 {
        Self::get_liquidity(&env, &asset)
    }

    // --- Analytics --------------------------------------------------------

    /// Cumulative swap volume and count.
    pub fn analytics(env: Env) -> Analytics {
        Self::get_analytics(&env)
    }

    /// The most recently executed route (single or split).
    pub fn last_route(env: Env) -> Result<Route, AggregateError> {
        env.storage()
            .persistent()
            .get(&DataKey::LastRoute)
            .ok_or(AggregateError::RouteNotFound)
    }

    // --- Internals --------------------------------------------------------

    fn set_dex_enabled_impl(
        env: &Env,
        admin: &Address,
        dex_id: &Symbol,
        enabled: bool,
    ) -> Result<(), AggregateError> {
        admin.require_auth();
        Self::assert_admin(env, admin)?;

        let mut entry = Self::must_get_dex(env, dex_id)?;
        entry.enabled = enabled;
        env.storage()
            .persistent()
            .set(&DataKey::Dex(dex_id.clone()), &entry);

        env.events()
            .publish((symbol_short!("dex_set"), dex_id.clone()), enabled);

        Ok(())
    }

    fn build_route(
        env: &Env,
        amount_in: i128,
        token_in: &Address,
        token_out: &Address,
        max_venues: u32,
    ) -> Result<Route, AggregateError> {
        if amount_in <= 0 {
            return Err(AggregateError::InvalidAmount);
        }
        if max_venues == 0 {
            return Err(AggregateError::InvalidVenueCount);
        }

        let venues = Self::eligible_venues(env, token_in, token_out)?;
        if venues.len() == 0 {
            return Err(AggregateError::NoRoute);
        }

        let take = if max_venues > MAX_SPLIT_VENUES {
            MAX_SPLIT_VENUES
        } else {
            max_venues
        };
        let take = if take > venues.len() { venues.len() } else { take };

        let mut unit_sum: i128 = 0;
        for i in 0..take {
            unit_sum += Self::unit_net_price(&venues.get(i).unwrap());
        }
        if unit_sum <= 0 {
            return Err(AggregateError::NoRoute);
        }

        let mut legs = Vec::new(env);
        let mut allocated: i128 = 0;
        let mut total_out: i128 = 0;

        for i in 0..take {
            let entry = venues.get(i).unwrap();
            let unit = Self::unit_net_price(&entry);

            // Weighted allocation; the last leg absorbs rounding remainder so
            // the legs always sum exactly to `amount_in`.
            let leg_in = if i == take - 1 {
                amount_in - allocated
            } else {
                (amount_in * unit) / unit_sum
            };
            if leg_in <= 0 {
                continue;
            }

            let leg_out = Self::net_out(&entry, leg_in);
            if leg_out <= 0 {
                continue;
            }

            allocated += leg_in;
            total_out += leg_out;
            legs.push_back(SwapLeg {
                dex_id: entry.id.clone(),
                amount_in: leg_in,
                amount_out: leg_out,
            });
        }

        if legs.len() == 0 {
            return Err(AggregateError::NoRoute);
        }

        Ok(Route {
            token_in: token_in.clone(),
            token_out: token_out.clone(),
            legs,
            total_in: amount_in,
            total_out,
        })
    }

    fn eligible_venues(
        env: &Env,
        token_in: &Address,
        token_out: &Address,
    ) -> Result<Vec<DexEntry>, AggregateError> {
        let registry = Self::get_registry(env)?;
        let mut venues = Vec::new(env);

        for dex_id in registry.iter() {
            let entry = Self::must_get_dex(env, &dex_id)?;
            if entry.enabled && &entry.token_in == token_in && &entry.token_out == token_out {
                venues.push_back(entry);
            }
        }

        // Sort by effective rate, best first (bounded selection sort).
        let len = venues.len();
        for i in 0..len {
            for j in (i + 1)..len {
                let a = venues.get(i).unwrap();
                let b = venues.get(j).unwrap();
                if Self::unit_net_price(&a) < Self::unit_net_price(&b) {
                    venues.set(i, b);
                    venues.set(j, a);
                }
            }
        }

        Ok(venues)
    }

    fn record_execution(env: &Env, route: &Route) {
        let mut analytics = Self::get_analytics(env);
        analytics.swap_count += 1;
        analytics.total_volume_in += route.total_in;
        analytics.total_volume_out += route.total_out;
        env.storage()
            .persistent()
            .set(&DataKey::Analytics, &analytics);
        env.storage()
            .persistent()
            .set(&DataKey::LastRoute, route);
    }

    fn settle(
        env: &Env,
        trader: &Address,
        token_in: &Address,
        token_out: &Address,
        amount_in: i128,
        amount_out: i128,
    ) -> Result<(), AggregateError> {
        if amount_out <= 0 {
            return Err(AggregateError::InvalidAmount);
        }

        let contract = env.current_contract_address();
        let out_client = token::Client::new(env, token_out);
        if out_client.balance(&contract) < amount_out {
            return Err(AggregateError::InsufficientLiquidity);
        }

        let in_client = token::Client::new(env, token_in);
        in_client.transfer(trader, &contract, &amount_in);
        out_client.transfer(&contract, trader, &amount_out);

        Ok(())
    }

    fn validate_venue(
        token_in: &Address,
        token_out: &Address,
        price: i128,
        fee_bps: u32,
    ) -> Result<(), AggregateError> {
        if price <= 0 {
            return Err(AggregateError::InvalidPrice);
        }
        if fee_bps > BASIS_POINTS as u32 {
            return Err(AggregateError::InvalidFee);
        }
        if token_in == token_out {
            return Err(AggregateError::IdenticalTokens);
        }
        Ok(())
    }

    fn get_config(env: &Env) -> Result<Config, AggregateError> {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(AggregateError::NotInitialized)
    }

    fn assert_admin(env: &Env, user: &Address) -> Result<(), AggregateError> {
        let config = Self::get_config(env)?;
        if &config.admin != user {
            return Err(AggregateError::Unauthorized);
        }
        Ok(())
    }

    fn get_registry(env: &Env) -> Result<Vec<Symbol>, AggregateError> {
        env.storage()
            .persistent()
            .get(&DataKey::Registry)
            .ok_or(AggregateError::NotInitialized)
    }

    fn must_get_dex(env: &Env, dex_id: &Symbol) -> Result<DexEntry, AggregateError> {
        env.storage()
            .persistent()
            .get(&DataKey::Dex(dex_id.clone()))
            .ok_or(AggregateError::DexNotFound)
    }

    fn get_analytics(env: &Env) -> Analytics {
        env.storage()
            .persistent()
            .get(&DataKey::Analytics)
            .unwrap_or(Analytics {
                swap_count: 0,
                total_volume_in: 0,
                total_volume_out: 0,
            })
    }

    fn get_liquidity(env: &Env, asset: &Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Liquidity(asset.clone()))
            .unwrap_or(0)
    }

    /// Net output after fees for a given input at a venue's fixed rate.
    fn net_out(entry: &DexEntry, amount_in: i128) -> i128 {
        let gross = (amount_in * entry.price) / PRICE_SCALE;
        let fee = (gross * entry.fee_bps as i128) / BASIS_POINTS;
        gross - fee
    }

    /// Effective rate per 1 unit of input, after fees (used for weighting/sort).
    fn unit_net_price(entry: &DexEntry) -> i128 {
        (entry.price * (BASIS_POINTS - entry.fee_bps as i128)) / BASIS_POINTS
    }
}

#[cfg(test)]
mod test;
