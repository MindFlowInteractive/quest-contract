#![no_std]
use soroban_sdk::{contract, contractimpl, token, Address, Env, Vec};

mod types;
mod test;
use types::{DataKey, TournamentConfig, TournamentState};

#[contract]
pub struct TournamentContract;

#[contractimpl]
impl TournamentContract {
    pub fn initialize(e: Env, admin: Address, token: Address, entry_fee: i128) {
        if e.storage().instance().has(&DataKey::Config) {
            panic!("Already initialized");
        }
        let config = TournamentConfig {
            admin,
            token,
            entry_fee,
        };
        e.storage().instance().set(&DataKey::Config, &config);
        e.storage().instance().set(&DataKey::State, &TournamentState::Open);
        e.storage().instance().set(&DataKey::TotalPrize, &0i128);
        
        // Initialize empty participants list
        let participants: Vec<Address> = Vec::new(&e);
        e.storage().instance().set(&DataKey::Participants, &participants);
    }

    pub fn register(e: Env, player: Address) {
        player.require_auth();

        let state: TournamentState = e.storage().instance().get(&DataKey::State).unwrap();
        if state != TournamentState::Open {
            panic!("Tournament not open for registration");
        }

        let config: TournamentConfig = e.storage().instance().get(&DataKey::Config).unwrap();
        
        let mut participants: Vec<Address> = e.storage().instance().get(&DataKey::Participants).unwrap();
        if participants.contains(&player) {
            panic!("Already registered");
        }

        // Transfer entry fee
        let client = token::Client::new(&e, &config.token);
        client.transfer(&player, &e.current_contract_address(), &config.entry_fee);

        // Update prize pool
        let mut total_prize: i128 = e.storage().instance().get(&DataKey::TotalPrize).unwrap();
        total_prize += config.entry_fee;
        e.storage().instance().set(&DataKey::TotalPrize, &total_prize);

        // Add to participants
        participants.push_back(player);
        e.storage().instance().set(&DataKey::Participants, &participants);
    }

    pub fn start_tournament(e: Env) {
        let config: TournamentConfig = e.storage().instance().get(&DataKey::Config).unwrap();
        config.admin.require_auth();

        let state: TournamentState = e.storage().instance().get(&DataKey::State).unwrap();
        if state != TournamentState::Open {
            panic!("Tournament already started or ended");
        }

        let participants: Vec<Address> = e.storage().instance().get(&DataKey::Participants).unwrap();
        if participants.len() < 2 {
            panic!("Not enough participants");
        }

        e.storage().instance().set(&DataKey::State, &TournamentState::Started);
        // Bracket generation logic would go here. 
        // For simplicity in this iteration, we assume off-chain bracket management 
        // or a simple linear matching handled by the admin via `record_result`.
    }

    pub fn record_result(e: Env, winner: Address) { 
        // Note: This is a simplified version where admin declares winners of matches/tournament directly
        // In a full version, we'd pass match_id and validate against the bracket.
        let config: TournamentConfig = e.storage().instance().get(&DataKey::Config).unwrap();
        config.admin.require_auth();

        let state: TournamentState = e.storage().instance().get(&DataKey::State).unwrap();
        if state != TournamentState::Started {
            panic!("Tournament not in progress");
        }
        
        // Check if winner is a valid participant - simplified check
        let participants: Vec<Address> = e.storage().instance().get(&DataKey::Participants).unwrap();
        if !participants.contains(&winner) {
             panic!("Winner is not a participant");
        }
        
        // For this MVP, let's assume `record_result` declares the FINAL tournament winner for simplicity
        // or effectively distributes the prize.
        
        // We will move to Ended state and distribute prize
        e.storage().instance().set(&DataKey::State, &TournamentState::Ended);
        
        let total_prize: i128 = e.storage().instance().get(&DataKey::TotalPrize).unwrap();
        if total_prize > 0 {
             let client = token::Client::new(&e, &config.token);
             client.transfer(&e.current_contract_address(), &winner, &total_prize);
        }
    }

    pub fn cancel_tournament(e: Env) {
        let config: TournamentConfig = e.storage().instance().get(&DataKey::Config).unwrap();
        config.admin.require_auth();

        let state: TournamentState = e.storage().instance().get(&DataKey::State).unwrap();
        if state == TournamentState::Ended {
            panic!("Cannot cancel ended tournament");
        }

        e.storage().instance().set(&DataKey::State, &TournamentState::Cancelled);
        
        // Allow refunds - in this model, we can iterate and refund or let users pull.
        // For gas efficiency, usually pull pattern is better, but loop is okay for small numbers.
        // Let's implement a 'withdraw_refund' function for users to call instead of auto-refunding loop to be safe.
    }

    pub fn withdraw_refund(e: Env, player: Address) {
        player.require_auth();
        let state: TournamentState = e.storage().instance().get(&DataKey::State).unwrap();
        if state != TournamentState::Cancelled {
            panic!("Tournament not cancelled");
        }

        let participants: Vec<Address> = e.storage().instance().get(&DataKey::Participants).unwrap();
        if !participants.contains(&player) {
            panic!("Not a participant");
        }

        // Ideally we track if they already withdrew. 
        // Quick fix: Remove them from participants list after refund to prevent double refund.
        // Note: Vector removal by value is O(N), might be expensive for large lists.
        // Valid for MVP.
        
        let config: TournamentConfig = e.storage().instance().get(&DataKey::Config).unwrap();
        let client = token::Client::new(&e, &config.token);
        client.transfer(&e.current_contract_address(), &player, &config.entry_fee);

        // Remove from list
        let mut new_participants = Vec::new(&e);
        for p in participants.iter() {
            if p != player {
                new_participants.push_back(p);
            }
        }
        e.storage().instance().set(&DataKey::Participants, &new_participants);
    }
    
    // View functions
    pub fn get_state(e: Env) -> TournamentState {
        e.storage().instance().get(&DataKey::State).unwrap()
    }
    
    pub fn get_participants(e: Env) -> Vec<Address> {
        e.storage().instance().get(&DataKey::Participants).unwrap()
    }
    
    pub fn get_prize_pool(e: Env) -> i128 {
        e.storage().instance().get(&DataKey::TotalPrize).unwrap_or(0)
    }
}
