#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype,
    Env, Address, Vec, Bytes
};

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum RoundStatus {
    Open,
    Drawing,
    Completed,
    Cancelled,
}


fn generate_number(env: &Env, seed: u64) -> u64 {
    let mut seed_bytes = Bytes::new(&env);
    seed_bytes.extend_from_slice(&seed.to_be_bytes());

    let hash = env.crypto().sha256(&seed_bytes);

    // Convert hash to array
    let hash_bytes = hash.to_array();

    // Take first 8 bytes → u64
    u64::from_be_bytes([
        hash_bytes[0],
        hash_bytes[1],
        hash_bytes[2],
        hash_bytes[3],
        hash_bytes[4],
        hash_bytes[5],
        hash_bytes[6],
        hash_bytes[7],
    ])
}

// Entry point for the contract
pub fn main() {
    // This function is required for the crate to compile but can remain empty for now.
}

#[contracttype]
#[derive(Clone)]
pub struct LotteryRound {
    pub id: u32,
    pub ticket_price: i128,
    pub prize_pool: i128,
    pub start_time: u64,
    pub end_time: u64,
    pub winner: Option<Address>,
    pub status: RoundStatus,
}

#[contracttype]
pub enum DataKey {
    Owner,
    Token,
    CurrentRound,
    Round(u32),
    Players(u32),
    Randomness(u32),
}

// Please note:
// Soroban does not yet support native VRF.
// This implementation uses hash-based pseudo-randomness
// suitable for MVP and educational use.


#[contract]
pub struct LotteryContract;

#[contractimpl]
impl LotteryContract {
    pub fn init(env: Env, owner: Address, token: Address) {
        owner.require_auth();
        env.storage().instance().set(&DataKey::Owner, &owner);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::CurrentRound, &0u32);
    }
}

pub fn start_round(env: Env, ticket_price: i128, duration: u64) {
    let owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
    owner.require_auth();

    let mut round_id: u32 = env.storage().instance().get(&DataKey::CurrentRound).unwrap();
    round_id += 1;

    let now = env.ledger().timestamp();

    let round = LotteryRound {
        id: round_id,
        ticket_price,
        prize_pool: 0,
        start_time: now,
        end_time: now + duration,
        winner: None,
        status: RoundStatus::Open,
    };

    env.storage().persistent().set(&DataKey::Round(round_id), &round);
    env.storage().instance().set(&DataKey::CurrentRound, &round_id);
    env.storage().persistent().set(&DataKey::Players(round_id), &Vec::<Address>::new(&env));
}

pub fn buy_ticket(env: Env, user: Address) {
    user.require_auth();

    let round_id: u32 = env.storage().instance().get(&DataKey::CurrentRound).unwrap();
    let mut round: LotteryRound = env.storage().persistent().get(&DataKey::Round(round_id)).unwrap();

    if round.status != RoundStatus::Open {
        panic!("Round not open");
    }

    let token: Address = env.storage().instance().get(&DataKey::Token).unwrap();
    let client = soroban_sdk::token::Client::new(&env, &token);

    client.transfer(&user, &env.current_contract_address(), &round.ticket_price);

    round.prize_pool += round.ticket_price;

    let mut players: Vec<Address> =
        env.storage().persistent().get(&DataKey::Players(round_id)).unwrap();

    players.push_back(user);

    env.storage().persistent().set(&DataKey::Players(round_id), &players);
    env.storage().persistent().set(&DataKey::Round(round_id), &round);
}

fn generate_random(env: &Env) -> u64 {
    let seed = env.ledger().sequence();
    let hash = env.crypto().sha256(&soroban_sdk::Bytes::from_array(env, &seed.to_be_bytes()));
    let mut bytes = [0u8; 8];
    let hash_bytes = hash.to_array();
    bytes.copy_from_slice(&hash_bytes[..8]);
    u64::from_be_bytes(bytes)
}

pub fn draw_winner(env: Env) {
    let round_id: u32 = env.storage().instance().get(&DataKey::CurrentRound).unwrap();
    let mut round: LotteryRound = env.storage().persistent().get(&DataKey::Round(round_id)).unwrap();

    if env.ledger().timestamp() < round.end_time {
        panic!("Round still active");
    }

    let players: Vec<Address> =
        env.storage().persistent().get(&DataKey::Players(round_id)).unwrap();

    let rand = generate_random(&env);
    let winner_index = (rand % players.len() as u64) as u32;

    let winner = players.get(winner_index).unwrap();

    round.winner = Some(winner.clone());
    round.status = RoundStatus::Completed;

    env.storage().persistent().set(&DataKey::Round(round_id), &round);
}

pub fn claim_prize(env: Env, user: Address, round_id: u32) {
    user.require_auth();

    let mut round: LotteryRound = env.storage().persistent().get(&DataKey::Round(round_id)).unwrap();

    if round.winner != Some(user.clone()) {
        panic!("Not winner");
    }

    let token: Address = env.storage().instance().get(&DataKey::Token).unwrap();
    let client = soroban_sdk::token::Client::new(&env, &token);

    let amount = round.prize_pool;
    round.prize_pool = 0;

    client.transfer(&env.current_contract_address(), &user, &amount);

    env.storage().persistent().set(&DataKey::Round(round_id), &round);
}

pub fn cancel_round(env: Env) {
    let owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
    owner.require_auth();

    let round_id: u32 = env.storage().instance().get(&DataKey::CurrentRound).unwrap();
    let mut round: LotteryRound = env.storage().persistent().get(&DataKey::Round(round_id)).unwrap();

    round.status = RoundStatus::Cancelled;
    env.storage().persistent().set(&DataKey::Round(round_id), &round);
}

pub fn refund(env: Env, round_id: u32, user: Address) {
    let round = get_round(env.clone(), round_id);
    assert!(round.status == RoundStatus::Cancelled);

    // transfer ticket cost back
}

pub fn get_round (env:Env, round_id: u32) -> LotteryRound {
    env.storage().persistent().get(&DataKey::CurrentRound).unwrap()
}
