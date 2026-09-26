#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, Symbol,
    Vec,
};

/// Basis points denominator (100% == 10_000 bps).
const BPS_DENOMINATOR: i128 = 10_000;
/// Maximum protocol fee that can ever be configured (10%).
const MAX_FEE_BPS: u32 = 1_000;
/// Default protocol fee (2%).
const DEFAULT_FEE_BPS: u32 = 200;
/// Odds are expressed with this fixed-point scale (1.0 == 10_000).
const ODDS_SCALE: i128 = 10_000;
/// Minimum number of outcomes a market must offer.
const MIN_OUTCOMES: u32 = 2;
/// Maximum number of outcomes a market may offer.
const MAX_OUTCOMES: u32 = 16;

const EVENT_INIT: Symbol = symbol_short!("init");
const EVENT_MARKET: Symbol = symbol_short!("market");
const EVENT_BET: Symbol = symbol_short!("bet");
const EVENT_ODDS: Symbol = symbol_short!("odds");
const EVENT_RESOLVE: Symbol = symbol_short!("resolve");
const EVENT_CLAIM: Symbol = symbol_short!("claim");
const EVENT_REFUND: Symbol = symbol_short!("refund");

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarketStatus {
    Open = 1,
    Closed = 2,
    Resolved = 3,
    Cancelled = 4,
}

/// A single wager placed by a player on one outcome of a puzzle market.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bet {
    pub bettor: Address,
    pub market_id: u64,
    pub outcome_index: u32,
    pub amount: i128,
    /// Odds (fixed point, `ODDS_SCALE`) captured at the moment the bet was placed.
    pub odds_at_placement: i128,
    pub timestamp: u64,
    pub claimed: bool,
}

/// Aggregate liquidity backing a single outcome.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutcomePool {
    pub outcome_index: u32,
    pub total_amount: i128,
    pub bet_count: u32,
}

/// A skill-based betting market tied to a puzzle.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Market {
    pub id: u64,
    pub creator: Address,
    pub puzzle_id: u32,
    pub outcomes: Vec<u32>,
    pub status: MarketStatus,
    pub close_time: u64,
    pub min_bet: i128,
    pub max_bet: i128,
    pub total_pool: i128,
    pub winning_outcome: Option<u32>,
    pub created_at: u64,
}

/// Per-market analytics snapshot.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarketAnalytics {
    pub market_id: u64,
    pub total_pool: i128,
    pub total_bets: u32,
    pub unique_bettors: u32,
    pub largest_bet: i128,
    pub settled: bool,
    pub total_paid_out: i128,
}

/// Fixed point odds for a single outcome.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutcomeOdds {
    pub outcome_index: u32,
    pub pool_amount: i128,
    /// Decimal odds scaled by `ODDS_SCALE` (e.g. 25_000 == 2.5x).
    pub odds: i128,
    /// Implied probability in basis points (sums to 10_000 across outcomes).
    pub implied_probability_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
    pub token: Address,
    pub fee_bps: u32,
    pub fee_recipient: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Config,
    NextMarketId,
    Market(u64),
    Pools(u64),
    Bets(u64),
    UserBet(u64, Address),
    Analytics(u64),
    Bettors(u64),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum SkillBettingError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidFeeBps = 4,
    InvalidOutcomeCount = 5,
    InvalidCloseTime = 6,
    InvalidBetBounds = 7,
    MarketNotFound = 8,
    InvalidStatus = 9,
    InvalidOutcome = 10,
    BettingClosed = 11,
    BetTooSmall = 12,
    BetTooLarge = 13,
    /// A bettor may only back a single outcome per market (arbitrage prevention).
    AlreadyBetOnOutcome = 14,
    BetNotFound = 15,
    AlreadyClaimed = 16,
    NotResolved = 17,
    NoWinningPool = 18,
    NothingToClaim = 19,
    InvalidWinner = 20,
}

#[contract]
pub struct SkillBettingContract;

#[contractimpl]
impl SkillBettingContract {
    /// Configure the contract. Can only be called once.
    pub fn initialize(
        env: Env,
        admin: Address,
        token: Address,
        fee_bps: u32,
        fee_recipient: Option<Address>,
    ) -> Result<(), SkillBettingError> {
        if env.storage().instance().has(&DataKey::Config) {
            return Err(SkillBettingError::AlreadyInitialized);
        }
        if fee_bps > MAX_FEE_BPS {
            return Err(SkillBettingError::InvalidFeeBps);
        }

        let config = Config {
            admin: admin.clone(),
            token,
            fee_bps,
            fee_recipient: fee_recipient.unwrap_or(admin),
        };

        env.storage().instance().set(&DataKey::Config, &config);
        env.storage().instance().set(&DataKey::NextMarketId, &1u64);

        env.events().publish((EVENT_INIT,), config.admin);
        Ok(())
    }

    /// Update the protocol fee. Admin only.
    pub fn set_fee_bps(env: Env, admin: Address, fee_bps: u32) -> Result<(), SkillBettingError> {
        admin.require_auth();
        if fee_bps > MAX_FEE_BPS {
            return Err(SkillBettingError::InvalidFeeBps);
        }

        let mut config = Self::get_config(env.clone())?;
        if admin != config.admin {
            return Err(SkillBettingError::Unauthorized);
        }

        config.fee_bps = fee_bps;
        env.storage().instance().set(&DataKey::Config, &config);
        Ok(())
    }

    /// Create a new skill-based betting market for a puzzle.
    pub fn create_market(
        env: Env,
        creator: Address,
        puzzle_id: u32,
        outcomes: Vec<u32>,
        close_time: u64,
        min_bet: i128,
        max_bet: i128,
    ) -> Result<u64, SkillBettingError> {
        creator.require_auth();
        Self::get_config(env.clone())?;

        let count = outcomes.len();
        if count < MIN_OUTCOMES || count > MAX_OUTCOMES {
            return Err(SkillBettingError::InvalidOutcomeCount);
        }
        if close_time <= env.ledger().timestamp() {
            return Err(SkillBettingError::InvalidCloseTime);
        }
        if min_bet <= 0 || max_bet < min_bet {
            return Err(SkillBettingError::InvalidBetBounds);
        }

        let market_id = Self::next_market_id(&env);
        let market = Market {
            id: market_id,
            creator: creator.clone(),
            puzzle_id,
            outcomes: outcomes.clone(),
            status: MarketStatus::Open,
            close_time,
            min_bet,
            max_bet,
            total_pool: 0,
            winning_outcome: None,
            created_at: env.ledger().timestamp(),
        };

        let mut pools = Vec::new(&env);
        for i in 0..count {
            pools.push_back(OutcomePool {
                outcome_index: i,
                total_amount: 0,
                bet_count: 0,
            });
        }

        let analytics = MarketAnalytics {
            market_id,
            total_pool: 0,
            total_bets: 0,
            unique_bettors: 0,
            largest_bet: 0,
            settled: false,
            total_paid_out: 0,
        };

        env.storage().persistent().set(&DataKey::Market(market_id), &market);
        env.storage().persistent().set(&DataKey::Pools(market_id), &pools);
        env.storage().persistent().set(&DataKey::Analytics(market_id), &analytics);
        env.storage().persistent().set(&DataKey::Bettors(market_id), &Vec::<Address>::new(&env));
        env.storage().instance().set(&DataKey::NextMarketId, &(market_id + 1));

        env.events().publish(
            (EVENT_MARKET, EVENT_MARKET),
            (market_id, creator, puzzle_id, close_time),
        );

        Ok(market_id)
    }

    /// Place a wager on a single outcome of an open market.
    ///
    /// Arbitrage prevention: a bettor may only ever back one outcome per market,
    /// so they can never hold a position that pays out regardless of the result.
    pub fn place_bet(
        env: Env,
        bettor: Address,
        market_id: u64,
        outcome_index: u32,
        amount: i128,
    ) -> Result<Bet, SkillBettingError> {
        bettor.require_auth();

        let mut market = Self::get_market_internal(&env, market_id)?;
        if market.status != MarketStatus::Open {
            return Err(SkillBettingError::InvalidStatus);
        }
        if env.ledger().timestamp() >= market.close_time {
            return Err(SkillBettingError::BettingClosed);
        }
        if outcome_index >= market.outcomes.len() {
            return Err(SkillBettingError::InvalidOutcome);
        }
        if amount < market.min_bet {
            return Err(SkillBettingError::BetTooSmall);
        }
        if amount > market.max_bet {
            return Err(SkillBettingError::BetTooLarge);
        }

        let user_key = DataKey::UserBet(market_id, bettor.clone());
        if env.storage().persistent().has(&user_key) {
            return Err(SkillBettingError::AlreadyBetOnOutcome);
        }

        let config = Self::get_config(env.clone())?;
        token::Client::new(&env, &config.token).transfer(
            &bettor,
            &env.current_contract_address(),
            &amount,
        );

        let mut pools: Vec<OutcomePool> = env
            .storage()
            .persistent()
            .get(&DataKey::Pools(market_id))
            .ok_or(SkillBettingError::MarketNotFound)?;

        let odds_before = Self::compute_odds(&env, &market, &pools);

        let mut pool = pools.get(outcome_index).unwrap();
        pool.total_amount += amount;
        pool.bet_count += 1;
        pools.set(outcome_index, pool);
        env.storage().persistent().set(&DataKey::Pools(market_id), &pools);

        market.total_pool += amount;
        env.storage().persistent().set(&DataKey::Market(market_id), &market);

        let odds_at_placement = odds_before
            .get(outcome_index)
            .map(|o| o.odds)
            .unwrap_or(ODDS_SCALE);

        let bet = Bet {
            bettor: bettor.clone(),
            market_id,
            outcome_index,
            amount,
            odds_at_placement,
            timestamp: env.ledger().timestamp(),
            claimed: false,
        };
        env.storage().persistent().set(&user_key, &bet);
        env.storage()
            .persistent()
            .set(&DataKey::Bets(market_id), &Self::append_bet(&env, market_id, &bet)?);

        Self::record_bettor(&env, market_id, &bettor)?;
        Self::update_analytics_on_bet(&env, market_id, amount)?;

        env.events().publish(
            (EVENT_BET, EVENT_BET),
            (market_id, bettor, outcome_index, amount, odds_at_placement),
        );

        let odds_after = Self::compute_odds(&env, &market, &pools);
        env.events()
            .publish((EVENT_ODDS, EVENT_ODDS), (market_id, odds_after));

        Ok(bet)
    }

    /// Current fixed-point odds for every outcome of a market.
    pub fn get_odds(env: Env, market_id: u64) -> Result<Vec<OutcomeOdds>, SkillBettingError> {
        let market = Self::get_market_internal(&env, market_id)?;
        let pools: Vec<OutcomePool> = env
            .storage()
            .persistent()
            .get(&DataKey::Pools(market_id))
            .ok_or(SkillBettingError::MarketNotFound)?;
        Ok(Self::compute_odds(&env, &market, &pools))
    }

    /// Close betting on a market. Admin only.
    pub fn close_market(env: Env, admin: Address, market_id: u64) -> Result<Market, SkillBettingError> {
        admin.require_auth();
        let config = Self::get_config(env.clone())?;
        if admin != config.admin {
            return Err(SkillBettingError::Unauthorized);
        }

        let mut market = Self::get_market_internal(&env, market_id)?;
        if market.status != MarketStatus::Open {
            return Err(SkillBettingError::InvalidStatus);
        }

        market.status = MarketStatus::Closed;
        env.storage().persistent().set(&DataKey::Market(market_id), &market);
        Ok(market)
    }

    /// Settle a market by declaring the winning outcome. Admin only.
    ///
    /// Settlement is automatic in the sense that once the winner is known the
    /// market transitions to `Resolved` and payouts become claimable without
    /// any further admin action.
    pub fn resolve_market(
        env: Env,
        admin: Address,
        market_id: u64,
        winning_outcome: u32,
    ) -> Result<Market, SkillBettingError> {
        admin.require_auth();
        let config = Self::get_config(env.clone())?;
        if admin != config.admin {
            return Err(SkillBettingError::Unauthorized);
        }

        let mut market = Self::get_market_internal(&env, market_id)?;
        if market.status != MarketStatus::Open && market.status != MarketStatus::Closed {
            return Err(SkillBettingError::InvalidStatus);
        }
        if winning_outcome >= market.outcomes.len() {
            return Err(SkillBettingError::InvalidWinner);
        }

        market.status = MarketStatus::Resolved;
        market.winning_outcome = Some(winning_outcome);
        env.storage().persistent().set(&DataKey::Market(market_id), &market);

        let mut analytics = Self::get_analytics_internal(&env, market_id)?;
        analytics.settled = true;
        env.storage()
            .persistent()
            .set(&DataKey::Analytics(market_id), &analytics);

        env.events()
            .publish((EVENT_RESOLVE, EVENT_RESOLVE), (market_id, winning_outcome));

        Ok(market)
    }

    /// Cancel a market and refund every bettor. Admin only.
    pub fn cancel_market(env: Env, admin: Address, market_id: u64) -> Result<Market, SkillBettingError> {
        admin.require_auth();
        let config = Self::get_config(env.clone())?;
        if admin != config.admin {
            return Err(SkillBettingError::Unauthorized);
        }

        let mut market = Self::get_market_internal(&env, market_id)?;
        if market.status == MarketStatus::Resolved || market.status == MarketStatus::Cancelled {
            return Err(SkillBettingError::InvalidStatus);
        }

        market.status = MarketStatus::Cancelled;
        env.storage().persistent().set(&DataKey::Market(market_id), &market);

        let bets: Vec<Bet> = env
            .storage()
            .persistent()
            .get(&DataKey::Bets(market_id))
            .unwrap_or(Vec::new(&env));
        let token_client = token::Client::new(&env, &config.token);
        for bet in bets.iter() {
            if !bet.claimed {
                token_client.transfer(
                    &env.current_contract_address(),
                    &bet.bettor,
                    &bet.amount,
                );
                let mut stored = bet.clone();
                stored.claimed = true;
                env.storage()
                    .persistent()
                    .set(&DataKey::UserBet(market_id, bet.bettor.clone()), &stored);
                env.events()
                    .publish((EVENT_REFUND, EVENT_REFUND), (market_id, bet.bettor, bet.amount));
            }
        }

        Ok(market)
    }

    /// Claim the payout for a winning bet.
    ///
    /// Payout uses pari-mutuel accounting: the winner receives their stake back
    /// plus a pro-rata share of the losing pool, minus the protocol fee.
    pub fn claim_payout(
        env: Env,
        bettor: Address,
        market_id: u64,
    ) -> Result<i128, SkillBettingError> {
        bettor.require_auth();

        let market = Self::get_market_internal(&env, market_id)?;
        if market.status != MarketStatus::Resolved {
            return Err(SkillBettingError::NotResolved);
        }
        let winning_outcome = market
            .winning_outcome
            .ok_or(SkillBettingError::NotResolved)?;

        let key = DataKey::UserBet(market_id, bettor.clone());
        let mut bet: Bet = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(SkillBettingError::BetNotFound)?;

        if bet.claimed {
            return Err(SkillBettingError::AlreadyClaimed);
        }
        if bet.outcome_index != winning_outcome {
            return Err(SkillBettingError::NothingToClaim);
        }

        let pools: Vec<OutcomePool> = env
            .storage()
            .persistent()
            .get(&DataKey::Pools(market_id))
            .ok_or(SkillBettingError::MarketNotFound)?;
        let winning_pool = pools
            .get(winning_outcome)
            .ok_or(SkillBettingError::InvalidOutcome)?;
        if winning_pool.total_amount <= 0 {
            return Err(SkillBettingError::NoWinningPool);
        }

        let config = Self::get_config(env.clone())?;
        let payout = Self::compute_payout(&market, &pools, &bet, config.fee_bps)?;

        bet.claimed = true;
        env.storage().persistent().set(&key, &bet);

        let mut analytics = Self::get_analytics_internal(&env, market_id)?;
        analytics.total_paid_out += payout;
        env.storage()
            .persistent()
            .set(&DataKey::Analytics(market_id), &analytics);

        if payout > 0 {
            token::Client::new(&env, &config.token).transfer(
                &env.current_contract_address(),
                &bettor,
                &payout,
            );
        }

        env.events()
            .publish((EVENT_CLAIM, EVENT_CLAIM), (market_id, bettor, payout));

        Ok(payout)
    }

    /// Preview the payout a bettor would receive if they claimed now.
    pub fn preview_payout(
        env: Env,
        market_id: u64,
        bettor: Address,
    ) -> Result<i128, SkillBettingError> {
        let market = Self::get_market_internal(&env, market_id)?;
        if market.status != MarketStatus::Resolved {
            return Err(SkillBettingError::NotResolved);
        }
        let winning_outcome = market
            .winning_outcome
            .ok_or(SkillBettingError::NotResolved)?;

        let bet: Bet = env
            .storage()
            .persistent()
            .get(&DataKey::UserBet(market_id, bettor))
            .ok_or(SkillBettingError::BetNotFound)?;
        if bet.claimed {
            return Err(SkillBettingError::AlreadyClaimed);
        }
        if bet.outcome_index != winning_outcome {
            return Err(SkillBettingError::NothingToClaim);
        }

        let pools: Vec<OutcomePool> = env
            .storage()
            .persistent()
            .get(&DataKey::Pools(market_id))
            .ok_or(SkillBettingError::MarketNotFound)?;
        let config = Self::get_config(env.clone())?;
        Self::compute_payout(&market, &pools, &bet, config.fee_bps)
    }

    pub fn get_market(env: Env, market_id: u64) -> Result<Market, SkillBettingError> {
        Self::get_market_internal(&env, market_id)
    }

    pub fn get_bet(
        env: Env,
        market_id: u64,
        bettor: Address,
    ) -> Result<Bet, SkillBettingError> {
        env.storage()
            .persistent()
            .get(&DataKey::UserBet(market_id, bettor))
            .ok_or(SkillBettingError::BetNotFound)
    }

    pub fn get_bets(env: Env, market_id: u64) -> Result<Vec<Bet>, SkillBettingError> {
        Self::get_market_internal(&env, market_id)?;
        Ok(env
            .storage()
            .persistent()
            .get(&DataKey::Bets(market_id))
            .unwrap_or(Vec::new(&env)))
    }

    pub fn get_pools(env: Env, market_id: u64) -> Result<Vec<OutcomePool>, SkillBettingError> {
        Self::get_market_internal(&env, market_id)?;
        env.storage()
            .persistent()
            .get(&DataKey::Pools(market_id))
            .ok_or(SkillBettingError::MarketNotFound)
    }

    pub fn get_analytics(env: Env, market_id: u64) -> Result<MarketAnalytics, SkillBettingError> {
        Self::get_analytics_internal(&env, market_id)
    }

    pub fn get_config(env: Env) -> Result<Config, SkillBettingError> {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(SkillBettingError::NotInitialized)
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    fn get_market_internal(env: &Env, market_id: u64) -> Result<Market, SkillBettingError> {
        env.storage()
            .persistent()
            .get(&DataKey::Market(market_id))
            .ok_or(SkillBettingError::MarketNotFound)
    }

    fn get_analytics_internal(
        env: &Env,
        market_id: u64,
    ) -> Result<MarketAnalytics, SkillBettingError> {
        env.storage()
            .persistent()
            .get(&DataKey::Analytics(market_id))
            .ok_or(SkillBettingError::MarketNotFound)
    }

    fn next_market_id(env: &Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::NextMarketId)
            .unwrap_or(1)
    }

    fn append_bet(env: &Env, market_id: u64, bet: &Bet) -> Result<Vec<Bet>, SkillBettingError> {
        let mut bets: Vec<Bet> = env
            .storage()
            .persistent()
            .get(&DataKey::Bets(market_id))
            .unwrap_or(Vec::new(env));
        bets.push_back(bet.clone());
        Ok(bets)
    }

    fn record_bettor(
        env: &Env,
        market_id: u64,
        bettor: &Address,
    ) -> Result<(), SkillBettingError> {
        let mut bettors: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Bettors(market_id))
            .unwrap_or(Vec::new(env));
        if !bettors.contains(bettor) {
            bettors.push_back(bettor.clone());
            env.storage()
                .persistent()
                .set(&DataKey::Bettors(market_id), &bettors);
        }
        Ok(())
    }

    fn update_analytics_on_bet(
        env: &Env,
        market_id: u64,
        amount: i128,
    ) -> Result<(), SkillBettingError> {
        let mut analytics = Self::get_analytics_internal(env, market_id)?;
        analytics.total_pool += amount;
        analytics.total_bets += 1;
        if amount > analytics.largest_bet {
            analytics.largest_bet = amount;
        }
        let bettors: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Bettors(market_id))
            .unwrap_or(Vec::new(env));
        analytics.unique_bettors = bettors.len();
        env.storage()
            .persistent()
            .set(&DataKey::Analytics(market_id), &analytics);
        Ok(())
    }

    /// Compute pari-mutuel decimal odds for every outcome.
    ///
    /// With no liquidity at all every outcome is priced at even money. Once
    /// liquidity exists, an outcome's odds are `total_pool / outcome_pool`,
    /// scaled by `ODDS_SCALE`. Implied probabilities are derived from the
    /// outcome pools and always sum to 10_000 bps.
    fn compute_odds(env: &Env, market: &Market, pools: &Vec<OutcomePool>) -> Vec<OutcomeOdds> {
        let mut result = Vec::new(env);
        let total = market.total_pool;

        for pool in pools.iter() {
            let odds = if total <= 0 || pool.total_amount <= 0 {
                ODDS_SCALE
            } else {
                total * ODDS_SCALE / pool.total_amount
            };

            let implied_probability_bps = if total <= 0 {
                (BPS_DENOMINATOR / (pools.len() as i128)) as u32
            } else {
                (pool.total_amount * BPS_DENOMINATOR / total) as u32
            };

            result.push_back(OutcomeOdds {
                outcome_index: pool.outcome_index,
                pool_amount: pool.total_amount,
                odds,
                implied_probability_bps,
            });
        }

        result
    }

    /// Pari-mutuel payout: stake + pro-rata share of the losing pool, less fee.
    fn compute_payout(
        market: &Market,
        pools: &Vec<OutcomePool>,
        bet: &Bet,
        fee_bps: u32,
    ) -> Result<i128, SkillBettingError> {
        let winning_outcome = market
            .winning_outcome
            .ok_or(SkillBettingError::NotResolved)?;
        let winning_pool = pools
            .get(winning_outcome)
            .ok_or(SkillBettingError::InvalidOutcome)?;
        if winning_pool.total_amount <= 0 {
            return Err(SkillBettingError::NoWinningPool);
        }

        let losing_pool = market.total_pool - winning_pool.total_amount;
        let share = bet.amount * losing_pool / winning_pool.total_amount;
        let gross = bet.amount + share;
        let fee = gross * (fee_bps as i128) / BPS_DENOMINATOR;
        Ok(gross - fee)
    }
}

#[cfg(test)]
mod test;
