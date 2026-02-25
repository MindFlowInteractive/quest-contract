#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, String, Vec};

#[contracttype]
pub enum DataKey {
    Admin,
    MarketCounter,
    Market(u64),
    Bets(u64),
    UserBets(Address, u64),
    LiquidityPool(u64),
    Dispute(u64),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarketStatus {
    Open = 1,
    Closed = 2,
    Resolved = 3,
    Disputed = 4,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Market {
    pub id: u64,
    pub creator: Address,
    pub description: String,
    pub outcomes: Vec<String>,
    pub status: MarketStatus,
    pub resolution_time: u64,
    pub winning_outcome: Option<u32>,
    pub total_pool: i128,
    pub liquidity_provider: Option<Address>,
    pub liquidity_amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bet {
    pub user: Address,
    pub outcome_index: u32,
    pub amount: i128,
    pub timestamp: u64,
    pub claimed: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutcomePool {
    pub outcome_index: u32,
    pub total_amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dispute {
    pub market_id: u64,
    pub disputer: Address,
    pub reason: String,
    pub timestamp: u64,
    pub resolved: bool,
}

#[contract]
pub struct PredictionMarket;

#[contractimpl]
impl PredictionMarket {
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::MarketCounter, &0u64);
    }

    pub fn create_market(
        env: Env,
        creator: Address,
        description: String,
        outcomes: Vec<String>,
        resolution_time: u64,
    ) -> u64 {
        creator.require_auth();
        assert!(outcomes.len() >= 2, "Need at least 2 outcomes");
        assert!(resolution_time > env.ledger().timestamp(), "Invalid time");

        let market_id: u64 = env.storage().instance().get(&DataKey::MarketCounter).unwrap_or(0);
        let new_id = market_id + 1;

        let market = Market {
            id: new_id,
            creator: creator.clone(),
            description,
            outcomes: outcomes.clone(),
            status: MarketStatus::Open,
            resolution_time,
            winning_outcome: None,
            total_pool: 0,
            liquidity_provider: None,
            liquidity_amount: 0,
        };

        env.storage().instance().set(&DataKey::Market(new_id), &market);
        env.storage().instance().set(&DataKey::MarketCounter, &new_id);

        let mut outcome_pools = Vec::new(&env);
        for i in 0..outcomes.len() {
            outcome_pools.push_back(OutcomePool {
                outcome_index: i as u32,
                total_amount: 0,
            });
        }
        env.storage().instance().set(&DataKey::Bets(new_id), &outcome_pools);

        env.events().publish(
            (String::from_str(&env, "market_created"), new_id),
            creator,
        );

        new_id
    }

    pub fn place_bet(
        env: Env,
        user: Address,
        market_id: u64,
        outcome_index: u32,
        amount: i128,
        token: Address,
    ) {
        user.require_auth();
        assert!(amount > 0, "Amount must be positive");

        let mut market: Market = env.storage().instance().get(&DataKey::Market(market_id)).unwrap();
        assert!(market.status == MarketStatus::Open, "Market not open");
        assert!(env.ledger().timestamp() < market.resolution_time, "Market closed");
        assert!((outcome_index as usize) < market.outcomes.len() as usize, "Invalid outcome");

        token::Client::new(&env, &token).transfer(&user, &env.current_contract_address(), &amount);

        let bet = Bet {
            user: user.clone(),
            outcome_index,
            amount,
            timestamp: env.ledger().timestamp(),
            claimed: false,
        };

        let key = DataKey::UserBets(user.clone(), market_id);
        let mut user_bets: Vec<Bet> = env.storage().instance().get(&key).unwrap_or(Vec::new(&env));
        user_bets.push_back(bet);
        env.storage().instance().set(&key, &user_bets);

        let mut outcome_pools: Vec<OutcomePool> = env.storage().instance().get(&DataKey::Bets(market_id)).unwrap();
        let mut pool = outcome_pools.get(outcome_index).unwrap();
        pool.total_amount += amount;
        outcome_pools.set(outcome_index, pool);
        env.storage().instance().set(&DataKey::Bets(market_id), &outcome_pools);

        market.total_pool += amount;
        env.storage().instance().set(&DataKey::Market(market_id), &market);

        env.events().publish(
            (String::from_str(&env, "bet_placed"), market_id, outcome_index),
            (user, amount),
        );
    }

    pub fn add_liquidity(env: Env, provider: Address, market_id: u64, amount: i128, token: Address) {
        provider.require_auth();
        assert!(amount > 0, "Amount must be positive");

        let mut market: Market = env.storage().instance().get(&DataKey::Market(market_id)).unwrap();
        assert!(market.status == MarketStatus::Open, "Market not open");

        token::Client::new(&env, &token).transfer(&provider, &env.current_contract_address(), &amount);

        market.liquidity_provider = Some(provider.clone());
        market.liquidity_amount += amount;
        env.storage().instance().set(&DataKey::Market(market_id), &market);

        env.storage().instance().set(&DataKey::LiquidityPool(market_id), &amount);

        env.events().publish(
            (String::from_str(&env, "liquidity_added"), market_id),
            (provider, amount),
        );
    }

    pub fn resolve_market(env: Env, admin: Address, market_id: u64, winning_outcome: u32) {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        assert!(admin == stored_admin, "Unauthorized");

        let mut market: Market = env.storage().instance().get(&DataKey::Market(market_id)).unwrap();
        assert!(market.status == MarketStatus::Open || market.status == MarketStatus::Closed, "Invalid status");
        assert!((winning_outcome as usize) < market.outcomes.len() as usize, "Invalid outcome");

        market.status = MarketStatus::Resolved;
        market.winning_outcome = Some(winning_outcome);
        env.storage().instance().set(&DataKey::Market(market_id), &market);

        env.events().publish(
            (String::from_str(&env, "market_resolved"), market_id),
            winning_outcome,
        );
    }

    pub fn claim_winnings(env: Env, user: Address, market_id: u64, token: Address) -> i128 {
        user.require_auth();

        let market: Market = env.storage().instance().get(&DataKey::Market(market_id)).unwrap();
        assert!(market.status == MarketStatus::Resolved, "Market not resolved");

        let winning_outcome = market.winning_outcome.unwrap();
        let key = DataKey::UserBets(user.clone(), market_id);
        let mut user_bets: Vec<Bet> = env.storage().instance().get(&key).unwrap_or(Vec::new(&env));

        let outcome_pools: Vec<OutcomePool> = env.storage().instance().get(&DataKey::Bets(market_id)).unwrap();
        let winning_pool = outcome_pools.get(winning_outcome).unwrap();

        let mut total_payout = 0i128;
        for i in 0..user_bets.len() {
            let mut bet = user_bets.get(i).unwrap();
            if bet.outcome_index == winning_outcome && !bet.claimed {
                let share = (bet.amount * market.total_pool) / winning_pool.total_amount;
                total_payout += share;
                bet.claimed = true;
                user_bets.set(i, bet);
            }
        }

        assert!(total_payout > 0, "No winnings to claim");

        env.storage().instance().set(&key, &user_bets);
        token::Client::new(&env, &token).transfer(&env.current_contract_address(), &user, &total_payout);

        env.events().publish(
            (String::from_str(&env, "winnings_claimed"), market_id),
            (user, total_payout),
        );

        total_payout
    }

    pub fn partial_cashout(env: Env, user: Address, market_id: u64, bet_index: u32, token: Address) -> i128 {
        user.require_auth();

        let market: Market = env.storage().instance().get(&DataKey::Market(market_id)).unwrap();
        assert!(market.status == MarketStatus::Open, "Market not open");

        let key = DataKey::UserBets(user.clone(), market_id);
        let mut user_bets: Vec<Bet> = env.storage().instance().get(&key).unwrap();
        let mut bet = user_bets.get(bet_index).unwrap();
        assert!(!bet.claimed, "Already claimed");

        let cashout_amount = (bet.amount * 90) / 100; // 10% fee
        bet.claimed = true;
        user_bets.set(bet_index, bet);
        env.storage().instance().set(&key, &user_bets);

        token::Client::new(&env, &token).transfer(&env.current_contract_address(), &user, &cashout_amount);

        env.events().publish(
            (String::from_str(&env, "partial_cashout"), market_id),
            (user, cashout_amount),
        );

        cashout_amount
    }

    pub fn raise_dispute(env: Env, user: Address, market_id: u64, reason: String) {
        user.require_auth();

        let mut market: Market = env.storage().instance().get(&DataKey::Market(market_id)).unwrap();
        assert!(market.status == MarketStatus::Resolved, "Market not resolved");

        market.status = MarketStatus::Disputed;
        env.storage().instance().set(&DataKey::Market(market_id), &market);

        let dispute = Dispute {
            market_id,
            disputer: user.clone(),
            reason,
            timestamp: env.ledger().timestamp(),
            resolved: false,
        };

        env.storage().instance().set(&DataKey::Dispute(market_id), &dispute);

        env.events().publish(
            (String::from_str(&env, "dispute_raised"), market_id),
            user,
        );
    }

    pub fn resolve_dispute(env: Env, admin: Address, market_id: u64, new_outcome: Option<u32>) {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        assert!(admin == stored_admin, "Unauthorized");

        let mut market: Market = env.storage().instance().get(&DataKey::Market(market_id)).unwrap();
        assert!(market.status == MarketStatus::Disputed, "No dispute");

        let mut dispute: Dispute = env.storage().instance().get(&DataKey::Dispute(market_id)).unwrap();
        dispute.resolved = true;
        env.storage().instance().set(&DataKey::Dispute(market_id), &dispute);

        if let Some(outcome) = new_outcome {
            market.winning_outcome = Some(outcome);
        }
        market.status = MarketStatus::Resolved;
        env.storage().instance().set(&DataKey::Market(market_id), &market);

        env.events().publish(
            (String::from_str(&env, "dispute_resolved"), market_id),
            new_outcome,
        );
    }

    pub fn close_market(env: Env, admin: Address, market_id: u64) {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        assert!(admin == stored_admin, "Unauthorized");

        let mut market: Market = env.storage().instance().get(&DataKey::Market(market_id)).unwrap();
        assert!(market.status == MarketStatus::Open, "Market not open");

        market.status = MarketStatus::Closed;
        env.storage().instance().set(&DataKey::Market(market_id), &market);
    }

    pub fn get_market(env: Env, market_id: u64) -> Market {
        env.storage().instance().get(&DataKey::Market(market_id)).unwrap()
    }

    pub fn get_outcome_pools(env: Env, market_id: u64) -> Vec<OutcomePool> {
        env.storage().instance().get(&DataKey::Bets(market_id)).unwrap()
    }

    pub fn get_user_bets(env: Env, user: Address, market_id: u64) -> Vec<Bet> {
        env.storage().instance().get(&DataKey::UserBets(user, market_id)).unwrap_or(Vec::new(&env))
    }
}

#[cfg(test)]
mod test;
