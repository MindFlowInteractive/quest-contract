#![no_std]

mod storage;
#[cfg(test)]
mod test;

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Vec, symbol_short, BytesN, Symbol, IntoVal};
use soroban_sdk::token::Client as TokenClient;
use crate::storage::*;

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FlashChallengeError {
    AlreadyInitialized = 1,
    InvalidParameters = 2,
    ChallengeNotFound = 3,
    ChallengeNotActive = 4,
    ChallengeNotStarted = 5,
    ChallengeExpired = 6,
    AlreadyWinner = 7,
    IncorrectSolution = 8,
    ChallengeNotEnded = 9,
    AlreadyFinalized = 10,
}

fn panic_with_error(env: &Env, err: FlashChallengeError) -> ! {
    env.events().publish(("flash_challenge_error",), err as u32);
    panic!("flash challenge contract error");
}

#[contract]
pub struct FlashChallengeContract;

#[contractimpl]
impl FlashChallengeContract {
    pub fn initialize(env: Env, admin: Address, token: Address, oracle: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error(&env, FlashChallengeError::AlreadyInitialized);
        }
        set_admin(&env, &admin);
        set_token_address(&env, &token);
        set_oracle_address(&env, &oracle);
    }

    pub fn schedule(
        env: Env,
        puzzle_id: u32,
        reward_pool: i128,
        start_at: u64,
        duration_minutes: u64,
        max_winners: u32,
    ) -> u32 {
        let admin = get_admin(&env);
        admin.require_auth();

        if reward_pool <= 0 || max_winners == 0 || duration_minutes == 0 {
            panic_with_error(&env, FlashChallengeError::InvalidParameters);
        }

        let token = get_token_address(&env);
        let client = TokenClient::new(&env, &token);
        
        // Transfer reward pool from admin to contract
        client.transfer(&admin, &env.current_contract_address(), &reward_pool);

        let id = increment_challenge_count(&env);
        let end_at = start_at + (duration_minutes * 60);

        let challenge = FlashChallenge {
            id,
            puzzle_id,
            reward_pool,
            max_winners,
            start_at,
            end_at,
            winners: Vec::new(&env),
            status: ChallengeStatus::Scheduled,
        };

        set_challenge(&env, id, &challenge);
        
        env.events().publish((symbol_short!("Flash"), symbol_short!("Scheduled")), id);

        id
    }

    pub fn submit_solution(env: Env, challenge_id: u32, player: Address, solution_hash: BytesN<32>) {
        player.require_auth();

        let mut challenge = get_challenge(&env, challenge_id).unwrap_or_else(|| panic_with_error(&env, FlashChallengeError::ChallengeNotFound));
        
        if challenge.status == ChallengeStatus::Completed || challenge.status == ChallengeStatus::Expired {
            panic_with_error(&env, FlashChallengeError::ChallengeNotActive);
        }

        let now = env.ledger().timestamp();
        if now < challenge.start_at {
            panic_with_error(&env, FlashChallengeError::ChallengeNotStarted);
        }
        
        if now > challenge.end_at {
            panic_with_error(&env, FlashChallengeError::ChallengeExpired);
        }
        
        if challenge.status == ChallengeStatus::Scheduled {
            challenge.status = ChallengeStatus::Active;
        }

        if challenge.winners.contains(&player) {
            panic_with_error(&env, FlashChallengeError::AlreadyWinner);
        }

        // Oracle verifies
        let oracle = get_oracle_address(&env);
        let is_correct: bool = env.invoke_contract(
            &oracle,
            &Symbol::new(&env, "verify"),
            (challenge.puzzle_id, solution_hash.clone()).into_val(&env)
        );

        if !is_correct {
            panic_with_error(&env, FlashChallengeError::IncorrectSolution);
        }

        challenge.winners.push_back(player.clone());
        let position = challenge.winners.len();
        
        env.events().publish((symbol_short!("Flash"), symbol_short!("Accepted")), (player, position));

        if challenge.winners.len() == challenge.max_winners {
            challenge.status = ChallengeStatus::Completed;
            
            // Payout immediately since max winners reached!
            let amount_each = challenge.reward_pool / (challenge.max_winners as i128);
            let token = get_token_address(&env);
            let client = TokenClient::new(&env, &token);
            let contract_addr = env.current_contract_address();
            
            for winner in challenge.winners.iter() {
                client.transfer(&contract_addr, &winner, &amount_each);
            }
            
            env.events().publish((symbol_short!("Flash"), symbol_short!("Completed")), (challenge.winners.clone(), amount_each));
        }

        set_challenge(&env, challenge_id, &challenge);
    }
    
    pub fn expire_challenge(env: Env, challenge_id: u32) {
        let mut challenge = get_challenge(&env, challenge_id).unwrap_or_else(|| panic_with_error(&env, FlashChallengeError::ChallengeNotFound));
        let now = env.ledger().timestamp();
        
        if now <= challenge.end_at {
            panic_with_error(&env, FlashChallengeError::ChallengeNotEnded);
        }
        
        if challenge.status == ChallengeStatus::Completed || challenge.status == ChallengeStatus::Expired {
            panic_with_error(&env, FlashChallengeError::AlreadyFinalized);
        }
        
        challenge.status = ChallengeStatus::Expired;
        
        let mut unallocated = challenge.reward_pool;
        let token_addr = get_token_address(&env);
        let token = TokenClient::new(&env, &token_addr);
        let contract_addr = env.current_contract_address();
        
        let num_winners = challenge.winners.len();
        if num_winners > 0 {
            let amount_each = challenge.reward_pool / (challenge.max_winners as i128); 
            for winner in challenge.winners.iter() {
                token.transfer(&contract_addr, &winner, &amount_each);
                unallocated -= amount_each;
            }
        }
        
        if unallocated > 0 {
            let admin = get_admin(&env);
            token.transfer(&contract_addr, &admin, &unallocated);
        }
        
        set_challenge(&env, challenge_id, &challenge);
        env.events().publish((symbol_short!("Flash"), symbol_short!("Expired")), challenge_id);
    }

    pub fn get_challenge(env: Env, id: u32) -> (ChallengeStatus, Vec<Address>, u64) {
        let challenge = get_challenge(&env, id).unwrap_or_else(|| panic_with_error(&env, FlashChallengeError::ChallengeNotFound));
        let now = env.ledger().timestamp();
        let mut time_remaining = 0;
        
        if now < challenge.end_at && challenge.status != ChallengeStatus::Completed && challenge.status != ChallengeStatus::Expired {
            if now >= challenge.start_at {
                time_remaining = challenge.end_at - now;
            } else {
                time_remaining = challenge.end_at - challenge.start_at; 
            }
        }
        
        (challenge.status, challenge.winners, time_remaining)
    }
}
