#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, token,
    Address, Bytes, Env, Vec,
};

#[cfg(test)]
mod test;

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundStatus {
    Open,
    Drawing,
    Completed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleType {
    Weekly,
    Monthly,
}

/// Duration in seconds for schedule type.
pub fn schedule_duration_sec(schedule: ScheduleType) -> u64 {
    match schedule {
        ScheduleType::Weekly => 7 * 24 * 3600,
        ScheduleType::Monthly => 30 * 24 * 3600,
    }
}

#[contracttype]
#[derive(Clone)]
pub struct PrizeTier {
    pub percent_bps: u32,
    pub winner_count: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct LotteryRound {
    pub id: u32,
    pub ticket_price: i128,
    pub prize_pool: i128,
    pub rollover: i128,
    pub start_time: u64,
    pub end_time: u64,
    pub schedule_type: ScheduleType,
    pub status: RoundStatus,
    pub total_tickets: u32,
    pub tiers: Vec<PrizeTier>,
    pub winners: Vec<Address>,
    pub claimed: Vec<bool>,
}

#[contracttype]
#[derive(Clone)]
pub struct PlayerTickets {
    pub player: Address,
    pub count: u32,
}

#[contracttype]
pub enum DataKey {
    Owner,
    Token,
    CurrentRound,
    Round(u32),
    Players(u32),
    TicketCount(u32, Address),
}

#[contract]
pub struct PuzzleLotteryContract;

#[contractimpl]
impl PuzzleLotteryContract {
    pub fn init(env: Env, owner: Address, token: Address) {
        owner.require_auth();
        if env.storage().instance().has(&DataKey::Owner) {
            panic!("Already initialized");
        }
        env.storage().instance().set(&DataKey::Owner, &owner);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::CurrentRound, &0u32);
    }

    pub fn start_round(
        env: Env,
        owner: Address,
        ticket_price: i128,
        schedule_type: ScheduleType,
        tiers: Vec<PrizeTier>,
    ) -> u32 {
        owner.require_auth();
        let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        if owner != stored_owner {
            panic!("Unauthorized");
        }
        if ticket_price <= 0 {
            panic!("Invalid ticket price");
        }
        let mut total_bps: u32 = 0;
        let mut total_winners: u32 = 0;
        for t in tiers.iter() {
            total_bps += t.percent_bps;
            total_winners += t.winner_count;
        }
        if total_bps > 10000 {
            panic!("Tier percents exceed 100%");
        }
        if total_winners == 0 {
            panic!("At least one winner required");
        }

        let now = env.ledger().timestamp();
        let duration = schedule_duration_sec(schedule_type);
        let mut round_id: u32 = env.storage().instance().get(&DataKey::CurrentRound).unwrap();
        round_id += 1;

        let round = LotteryRound {
            id: round_id,
            ticket_price,
            prize_pool: 0,
            rollover: 0,
            start_time: now,
            end_time: now + duration,
            schedule_type,
            status: RoundStatus::Open,
            total_tickets: 0,
            tiers,
            winners: Vec::new(&env),
            claimed: Vec::new(&env),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Round(round_id), &round);
        env.storage()
            .persistent()
            .set(&DataKey::Players(round_id), &Vec::<PlayerTickets>::new(&env));
        env.storage().instance().set(&DataKey::CurrentRound, &round_id);
        round_id
    }

    pub fn start_round_with_rollover(
        env: Env,
        owner: Address,
        ticket_price: i128,
        schedule_type: ScheduleType,
        tiers: Vec<PrizeTier>,
        rollover_amount: i128,
    ) -> u32 {
        owner.require_auth();
        let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        if owner != stored_owner {
            panic!("Unauthorized");
        }
        if ticket_price <= 0 {
            panic!("Invalid ticket price");
        }
        if rollover_amount < 0 {
            panic!("Invalid rollover");
        }
        let mut total_bps: u32 = 0;
        let mut total_winners: u32 = 0;
        for t in tiers.iter() {
            total_bps += t.percent_bps;
            total_winners += t.winner_count;
        }
        if total_bps > 10000 {
            panic!("Tier percents exceed 100%");
        }
        if total_winners == 0 {
            panic!("At least one winner required");
        }

        let now = env.ledger().timestamp();
        let duration = schedule_duration_sec(schedule_type);
        let mut round_id: u32 = env.storage().instance().get(&DataKey::CurrentRound).unwrap();
        round_id += 1;

        let round = LotteryRound {
            id: round_id,
            ticket_price,
            prize_pool: 0,
            rollover: rollover_amount,
            start_time: now,
            end_time: now + duration,
            schedule_type,
            status: RoundStatus::Open,
            total_tickets: 0,
            tiers,
            winners: Vec::new(&env),
            claimed: Vec::new(&env),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Round(round_id), &round);
        env.storage()
            .persistent()
            .set(&DataKey::Players(round_id), &Vec::<PlayerTickets>::new(&env));
        env.storage().instance().set(&DataKey::CurrentRound, &round_id);
        round_id
    }

    pub fn buy_ticket(env: Env, user: Address, count: u32) {
        user.require_auth();
        if count == 0 {
            panic!("Count must be positive");
        }

        let round_id: u32 = env.storage().instance().get(&DataKey::CurrentRound).unwrap();
        let mut round: LotteryRound = env
            .storage()
            .persistent()
            .get(&DataKey::Round(round_id))
            .unwrap();

        if round.status != RoundStatus::Open {
            panic!("Round not open");
        }
        let now = env.ledger().timestamp();
        if now < round.start_time || now > round.end_time {
            panic!("Round not active");
        }

        let token: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let client = token::Client::new(&env, &token);
        let cost = round.ticket_price * (count as i128);
        client.transfer(&user, &env.current_contract_address(), &cost);

        round.prize_pool += cost;
        round.total_tickets += count;

        let mut players: Vec<PlayerTickets> = env
            .storage()
            .persistent()
            .get(&DataKey::Players(round_id))
            .unwrap();
        let key = DataKey::TicketCount(round_id, user.clone());
        let existing: u32 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_count = existing + count;
        env.storage().persistent().set(&key, &new_count);

        let mut found = false;
        for i in 0..players.len() {
            let mut p = players.get(i).unwrap();
            if p.player == user {
                p.count = new_count;
                players.set(i, p);
                found = true;
                break;
            }
        }
        if !found {
            players.push_back(PlayerTickets {
                player: user,
                count: new_count,
            });
        }

        env.storage()
            .persistent()
            .set(&DataKey::Players(round_id), &players);
        env.storage()
            .persistent()
            .set(&DataKey::Round(round_id), &round);
    }

    fn prng(env: &Env, round_id: u32, nonce: u64) -> u64 {
        let seq = env.ledger().sequence();
        let ts = env.ledger().timestamp();
        let mut input = [0u8; 16];
        input[0..4].copy_from_slice(&round_id.to_be_bytes());
        input[4..8].copy_from_slice(&seq.to_be_bytes());
        input[8..16].copy_from_slice(&(ts + nonce).to_be_bytes());
        let hash = env.crypto().sha256(&Bytes::from_array(&env, &input));
        let bytes = hash.to_array();
        u64::from_be_bytes(bytes[..8].try_into().unwrap())
    }

    pub fn draw_winner(env: Env) {
        let round_id: u32 = env.storage().instance().get(&DataKey::CurrentRound).unwrap();
        let mut round: LotteryRound = env
            .storage()
            .persistent()
            .get(&DataKey::Round(round_id))
            .unwrap();

        if round.status != RoundStatus::Open {
            panic!("Round not open for draw");
        }
        if env.ledger().timestamp() < round.end_time {
            panic!("Round still active");
        }

        let players: Vec<PlayerTickets> = env
            .storage()
            .persistent()
            .get(&DataKey::Players(round_id))
            .unwrap();

        if round.total_tickets == 0 {
            round.status = RoundStatus::Completed;
            env.storage()
                .persistent()
                .set(&DataKey::Round(round_id), &round);
            return;
        }

        round.status = RoundStatus::Drawing;

        let total = round.total_tickets as usize;
        let mut ticket_to_player: Vec<Address> = Vec::new(&env);
        for i in 0..players.len() {
            let pt = players.get(i).unwrap();
            for _ in 0..pt.count {
                ticket_to_player.push_back(pt.player.clone());
            }
        }

        let mut total_winners_needed: u32 = 0;
        for t in round.tiers.iter() {
            total_winners_needed += t.winner_count;
        }

        let mut winners: Vec<Address> = Vec::new(&env);
        let mut used_indices: Vec<u32> = Vec::new(&env);
        let draws = core::cmp::min(total_winners_needed, round.total_tickets);

        for n in 0..draws {
            let mut nonce = n as u64;
            let mut idx: u32;
            loop {
                let r = Self::prng(&env, round_id, nonce);
                idx = (r % (total as u64)) as u32;
                let mut already = false;
                for i in 0..used_indices.len() {
                    if used_indices.get(i).unwrap() == idx {
                        already = true;
                        break;
                    }
                }
                if !already {
                    break;
                }
                nonce += 1_000_000;
            }
            used_indices.push_back(idx);
            let winner = ticket_to_player.get(idx).unwrap();
            winners.push_back(winner);
        }

        round.winners = winners.clone();
        for _ in 0..winners.len() {
            round.claimed.push_back(false);
        }
        round.status = RoundStatus::Completed;
        env.storage()
            .persistent()
            .set(&DataKey::Round(round_id), &round);
    }

    pub fn claim_prize(env: Env, user: Address, round_id: u32, winner_index: u32) {
        user.require_auth();

        let mut round: LotteryRound = env
            .storage()
            .persistent()
            .get(&DataKey::Round(round_id))
            .unwrap();

        if round.status != RoundStatus::Completed {
            panic!("Round not completed");
        }
        if winner_index >= (round.winners.len() as u32) {
            panic!("Invalid winner index");
        }
        if round.winners.get(winner_index).unwrap() != user {
            panic!("Not winner");
        }
        if round.claimed.get(winner_index).unwrap() {
            panic!("Already claimed");
        }

        let total_pool = round.prize_pool + round.rollover;
        if total_pool <= 0 {
            round.claimed.set(winner_index, true);
            env.storage()
                .persistent()
                .set(&DataKey::Round(round_id), &round);
            return;
        }

        let mut tier_start: u32 = 0;
        let mut amount: i128 = 0;
        for t in round.tiers.iter() {
            let tier_end = tier_start + t.winner_count;
            if winner_index >= tier_start && winner_index < tier_end {
                let tier_share = (total_pool * (t.percent_bps as i128)) / 10000;
                let per_winner = tier_share / (t.winner_count as i128);
                amount = per_winner;
                break;
            }
            tier_start = tier_end;
        }

        if amount <= 0 {
            round.claimed.set(winner_index, true);
            env.storage()
                .persistent()
                .set(&DataKey::Round(round_id), &round);
            return;
        }

        let token: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let client = token::Client::new(&env, &token);
        client.transfer(&env.current_contract_address(), &user, &amount);

        round.claimed.set(winner_index, true);
        env.storage()
            .persistent()
            .set(&DataKey::Round(round_id), &round);
    }

    pub fn cancel_round(env: Env, owner: Address) {
        owner.require_auth();
        let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        if owner != stored_owner {
            panic!("Unauthorized");
        }

        let round_id: u32 = env.storage().instance().get(&DataKey::CurrentRound).unwrap();
        let mut round: LotteryRound = env
            .storage()
            .persistent()
            .get(&DataKey::Round(round_id))
            .unwrap();

        if round.status != RoundStatus::Open {
            panic!("Round not open");
        }

        round.status = RoundStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Round(round_id), &round);
    }

    pub fn refund(env: Env, user: Address, round_id: u32) {
        user.require_auth();

        let round: LotteryRound = env
            .storage()
            .persistent()
            .get(&DataKey::Round(round_id))
            .unwrap();

        if round.status != RoundStatus::Cancelled {
            panic!("Round not cancelled");
        }

        let key = DataKey::TicketCount(round_id, user.clone());
        let count: u32 = env.storage().persistent().get(&key).unwrap_or(0);
        if count == 0 {
            panic!("No tickets to refund");
        }

        let amount = round.ticket_price * (count as i128);
        let token: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let client = token::Client::new(&env, &token);
        client.transfer(&env.current_contract_address(), &user, &amount);

        env.storage().persistent().set(&key, &0u32);
    }

    pub fn get_round(env: Env, round_id: u32) -> LotteryRound {
        env.storage()
            .persistent()
            .get(&DataKey::Round(round_id))
            .unwrap()
    }

    pub fn get_current_round_id(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::CurrentRound).unwrap()
    }

    pub fn get_players(env: Env, round_id: u32) -> Vec<PlayerTickets> {
        env.storage()
            .persistent()
            .get(&DataKey::Players(round_id))
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_ticket_count(env: Env, round_id: u32, user: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::TicketCount(round_id, user))
            .unwrap_or(0)
    }
}
