use soroban_sdk::{Address, Env, symbol_short};

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum DataKey {
    Initialized = 0,
    Admin = 1,
    TokenA = 2,
    TokenB = 3,
    ReserveA = 4,
    ReserveB = 5,
    TotalSupply = 6,
    FeeBps = 7,
    FeeRecipient = 8,
    FeesA = 9,
    FeesB = 10,
    PriceOracleTimestamp = 11,
    CumulativePrice = 12,
}

#[derive(Debug, Clone, Copy)]
pub struct Reserves {
    pub reserve_a: i128,
    pub reserve_b: i128,
    pub fees_a: i128,
    pub fees_b: i128,
}

#[derive(Debug, Clone, Copy)]
pub struct PriceOracle {
    pub last_timestamp: u64,
    pub cumulative_price: u128,
}

impl Reserves {
    pub fn new() -> Self {
        Reserves {
            reserve_a: 0,
            reserve_b: 0,
            fees_a: 0,
            fees_b: 0,
        }
    }
}

impl PriceOracle {
    pub fn new() -> Self {
        PriceOracle {
            last_timestamp: 0,
            cumulative_price: 0,
        }
    }
}

pub(crate) fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub(crate) fn get_admin(env: &Env) -> Address {
    env.storage().instance().get(&DataKey::Admin).unwrap()
}

pub(crate) fn set_tokens(env: &Env, token0: &Address, token1: &Address) {
    env.storage().instance().set(&DataKey::TokenA, token0);
    env.storage().instance().set(&DataKey::TokenB, token1);
}

pub(crate) fn get_tokens(env: &Env) -> (Address, Address) {
    let token0: Address = env.storage().instance().get(&DataKey::TokenA).unwrap();
    let token1: Address = env.storage().instance().get(&DataKey::TokenB).unwrap();
    (token0, token1)
}

pub(crate) fn set_reserves(env: &Env, reserves: &Reserves) {
    env.storage().instance().set(&DataKey::ReserveA, &reserves.reserve_a);
    env.storage().instance().set(&DataKey::ReserveB, &reserves.reserve_b);
    env.storage().instance().set(&DataKey::FeesA, &reserves.fees_a);
    env.storage().instance().set(&DataKey::FeesB, &reserves.fees_b);
}

pub(crate) fn get_reserves(env: &Env) -> Reserves {
    let reserve_a: i128 = env.storage().instance().get(&DataKey::ReserveA).unwrap_or(0);
    let reserve_b: i128 = env.storage().instance().get(&DataKey::ReserveB).unwrap_or(0);
    let fees_a: i128 = env.storage().instance().get(&DataKey::FeesA).unwrap_or(0);
    let fees_b: i128 = env.storage().instance().get(&DataKey::FeesB).unwrap_or(0);
    
    Reserves {
        reserve_a,
        reserve_b,
        fees_a,
        fees_b,
    }
}

pub(crate) fn set_total_supply(env: &Env, supply: &i128) {
    env.storage().instance().set(&DataKey::TotalSupply, supply);
}

pub(crate) fn get_total_supply(env: &Env) -> i128 {
    env.storage().instance().get(&DataKey::TotalSupply).unwrap_or(0)
}

pub(crate) fn set_fee_bps(env: &Env, bps: &u32) {
    env.storage().instance().set(&DataKey::FeeBps, bps);
}

pub(crate) fn get_fee_bps(env: &Env) -> u32 {
    env.storage().instance().get(&DataKey::FeeBps).unwrap()
}

pub(crate) fn set_fee_recipient(env: &Env, recipient: &Address) {
    env.storage().instance().set(&DataKey::FeeRecipient, recipient);
}

pub(crate) fn get_fee_recipient(env: &Env) -> Address {
    env.storage().instance().get(&DataKey::FeeRecipient).unwrap()
}

pub(crate) fn set_balance(env: &Env, owner: &Address, balance: &i128) {
    env.storage().persistent().set(&symbol_short!("balance"), owner, balance);
}

pub(crate) fn get_balance(env: &Env, owner: &Address) -> i128 {
    env.storage().persistent().get(&symbol_short!("balance"), owner).unwrap_or(0)
}

pub(crate) fn set_price_oracle(env: &Env, oracle: &PriceOracle) {
    env.storage().instance().set(&DataKey::PriceOracleTimestamp, &oracle.last_timestamp);
    env.storage().instance().set(&DataKey::CumulativePrice, &oracle.cumulative_price);
}

pub(crate) fn get_price_oracle(env: &Env) -> PriceOracle {
    let last_timestamp: u64 = env.storage().instance().get(&DataKey::PriceOracleTimestamp).unwrap_or(0);
    let cumulative_price: u128 = env.storage().instance().get(&DataKey::CumulativePrice).unwrap_or(0);
    
    PriceOracle {
        last_timestamp,
        cumulative_price,
    }
}

pub(crate) fn update_price_oracle(env: &Env, reserves: &Reserves) {
    let mut oracle = get_price_oracle(env);
    let current_timestamp = env.ledger().timestamp();
    
    if oracle.last_timestamp > 0 && current_timestamp > oracle.last_timestamp {
        let price = if reserves.reserve_b > 0 {
            reserves.reserve_a as u128 * 1_000_000_000_000_000_000 / reserves.reserve_b as u128
        } else {
            0
        };
        
        let time_elapsed = (current_timestamp - oracle.last_timestamp) as u128;
        oracle.cumulative_price += price * time_elapsed;
    }
    
    oracle.last_timestamp = current_timestamp;
    set_price_oracle(env, &oracle);
}