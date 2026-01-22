#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, token, Address, Env};

fn create_token_contract<'a>(e: &Env, admin: &Address) -> (token::Client<'a>, token::StellarAssetClient<'a>) {
    let contract_address = e.register_stellar_asset_contract_v2(admin.clone())
        .address();
    (
        token::Client::new(e, &contract_address),
        token::StellarAssetClient::new(e, &contract_address)
    )
}

fn create_tournament_contract<'a>(e: &Env) -> TournamentContractClient<'a> {
    let contract_id = e.register_contract(None, TournamentContract);
    TournamentContractClient::new(e, &contract_id)
}

#[test]
fn test_tournament_flow() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let user2 = Address::generate(&e);
    let token_admin = Address::generate(&e);

    let (token_client, token_admin_client) = create_token_contract(&e, &token_admin);
    let tournament_client = create_tournament_contract(&e);

    // Mint tokens to users
    token_admin_client.mint(&user1, &1000);
    token_admin_client.mint(&user2, &1000);

    // Initialize tournament
    let entry_fee = 100;
    tournament_client.initialize(&admin, &token_client.address, &entry_fee);

    // Register users
    tournament_client.register(&user1);
    tournament_client.register(&user2);

    // Verify registrations
    let participants = tournament_client.get_participants();
    assert_eq!(participants.len(), 2);
    assert_eq!(tournament_client.get_prize_pool(), 200);

    // Start tournament
    tournament_client.start_tournament();
    assert_eq!(tournament_client.get_state(), TournamentState::Started);

    // Record result (User1 wins)
    tournament_client.record_result(&user1);
    
    // Verify changes
    assert_eq!(tournament_client.get_state(), TournamentState::Ended);
    // User1 should have 900 (remaining) + 200 (prize) = 1100
    assert_eq!(token_client.balance(&user1), 1100);
    // User2 should have 900
    assert_eq!(token_client.balance(&user2), 900);
}

#[test]
fn test_cancel_and_refund() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let token_admin = Address::generate(&e);

    let (token_client, token_admin_client) = create_token_contract(&e, &token_admin);
    let tournament_client = create_tournament_contract(&e);

    token_admin_client.mint(&user1, &1000);

    tournament_client.initialize(&admin, &token_client.address, &100);
    tournament_client.register(&user1);

    tournament_client.cancel_tournament();
    assert_eq!(tournament_client.get_state(), TournamentState::Cancelled);

    tournament_client.withdraw_refund(&user1);
    
    // User1 should be back to 1000
    assert_eq!(token_client.balance(&user1), 1000);
    
    // Participants list should be empty (or at least user1 removed)
    let participants = tournament_client.get_participants();
    assert!(!participants.contains(&user1));
}
