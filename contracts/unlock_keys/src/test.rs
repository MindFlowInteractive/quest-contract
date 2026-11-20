#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, String};

struct Ctx {
    env: Env,
    contract: Address,
    admin: Address,
    minter: Address,
    player1: Address,
    player2: Address,
    silver: String,
    gold: String,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let player1 = Address::generate(&env);
    let player2 = Address::generate(&env);
    let silver = String::from_str(&env, "silver");
    let gold = String::from_str(&env, "gold");

    let contract = env.register_contract(None, UnlockKeysContract);
    let client = UnlockKeysContractClient::new(&env, &contract);
    client.initialize(&admin).unwrap();
    client.register_key_type(&admin, &silver).unwrap();
    client.register_key_type(&admin, &gold).unwrap();

    Ctx {
        env,
        contract,
        admin,
        minter,
        player1,
        player2,
        silver,
        gold,
    }
}

fn client(ctx: &Ctx) -> UnlockKeysContractClient<'_> {
    UnlockKeysContractClient::new(&ctx.env, &ctx.contract)
}

#[test]
fn test_mint_tracks_balance_and_supply() {
    let ctx = setup();
    let c = client(&ctx);

    c.mint(&ctx.admin, &ctx.player1, &ctx.silver, &5).unwrap();

    assert_eq!(c.balance(&ctx.player1, &ctx.silver), 5);
    assert_eq!(c.total_supply(&ctx.silver), 5);
    assert_eq!(c.balance(&ctx.player2, &ctx.silver), 0);
}

#[test]
fn test_transfer_between_players_keeps_supply() {
    let ctx = setup();
    let c = client(&ctx);

    c.mint(&ctx.admin, &ctx.player1, &ctx.gold, &10).unwrap();
    c.transfer(&ctx.player1, &ctx.player2, &ctx.gold, &4)
        .unwrap();

    assert_eq!(c.balance(&ctx.player1, &ctx.gold), 6);
    assert_eq!(c.balance(&ctx.player2, &ctx.gold), 4);
    assert_eq!(c.total_supply(&ctx.gold), 10);

    assert_eq!(
        c.try_transfer(&ctx.player1, &ctx.player2, &ctx.gold, &7),
        Err(Ok(KeyError::InsufficientBalance))
    );
}

#[test]
fn test_supply_tracked_per_key_type() {
    let ctx = setup();
    let c = client(&ctx);

    c.mint(&ctx.admin, &ctx.player1, &ctx.silver, &3).unwrap();
    c.mint(&ctx.admin, &ctx.player2, &ctx.gold, &8).unwrap();
    c.mint(&ctx.admin, &ctx.player2, &ctx.gold, &2).unwrap();

    assert_eq!(c.total_supply(&ctx.silver), 3);
    assert_eq!(c.total_supply(&ctx.gold), 10);
    assert_eq!(c.key_types().len(), 2);
}

#[test]
fn test_only_admin_or_authorized_minter_can_mint() {
    let ctx = setup();
    let c = client(&ctx);

    assert_eq!(
        c.try_mint(&ctx.player2, &ctx.player1, &ctx.silver, &1),
        Err(Ok(KeyError::NotAuthorized))
    );

    c.authorize_minter(&ctx.admin, &ctx.minter).unwrap();
    assert!(c.is_authorized_minter(&ctx.minter));
    c.mint(&ctx.minter, &ctx.player1, &ctx.silver, &2).unwrap();
    assert_eq!(c.balance(&ctx.player1, &ctx.silver), 2);

    c.revoke_minter(&ctx.admin, &ctx.minter).unwrap();
    assert!(!c.is_authorized_minter(&ctx.minter));
    assert_eq!(
        c.try_mint(&ctx.minter, &ctx.player1, &ctx.silver, &1),
        Err(Ok(KeyError::NotAuthorized))
    );
}

#[test]
fn test_burn_reduces_supply() {
    let ctx = setup();
    let c = client(&ctx);

    c.mint(&ctx.admin, &ctx.player1, &ctx.silver, &5).unwrap();
    c.burn(&ctx.player1, &ctx.silver, &2).unwrap();

    assert_eq!(c.balance(&ctx.player1, &ctx.silver), 3);
    assert_eq!(c.total_supply(&ctx.silver), 3);
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    fn setup_env() -> (Env, Address, Address, Address, UnlockKeysContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(UnlockKeysContract, ());
        let client = UnlockKeysContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let player1 = Address::generate(&env);
        let player2 = Address::generate(&env);

        client.initialize(&admin);

        // Leak client lifetime for test helper ergonomics in no_std test module
        let leaked_client: UnlockKeysContractClient<'static> =
            unsafe { core::mem::transmute(client) };

        (env, admin, player1, player2, leaked_client)
    }

    #[test]
    fn test_open_consumes_one_key_and_records_unlock() {
        let (_env, admin, player1, player2, client) = setup_env();
        let puzzle_id = 101;
        let key_type = 7;

        client.set_puzzle_key(&admin, &puzzle_id, &key_type);
        client.grant_keys(&admin, &player1, &key_type, &3);

        assert_eq!(client.is_unlocked(&player1, &puzzle_id), false);
        assert_eq!(client.open(&player1, &puzzle_id), true);

        // Consumed exactly 1 key (3 -> 2)
        assert_eq!(client.key_balance(&player1, &key_type), 2);
        assert_eq!(client.is_unlocked(&player1, &puzzle_id), true);

        // Other players remain unaffected
        assert_eq!(client.is_unlocked(&player2, &puzzle_id), false);
    }

    #[test]
    fn test_rejects_when_player_lacks_key() {
        let (_env, admin, player1, _player2, client) = setup_env();
        let puzzle_id = 102;
        let key_type = 5;
        let other_key_type = 9;

        client.set_puzzle_key(&admin, &puzzle_id, &key_type);
        client.grant_keys(&admin, &player1, &other_key_type, &2);

        let res = client.try_open(&player1, &puzzle_id);
        assert_eq!(res, Err(Ok(UnlockError::InsufficientKeys)));
        assert_eq!(client.is_unlocked(&player1, &puzzle_id), false);
        assert_eq!(client.key_balance(&player1, &other_key_type), 2);
    }

    #[test]
    fn test_prevents_opening_same_puzzle_twice() {
        let (_env, admin, player1, _player2, client) = setup_env();
        let puzzle_id = 103;
        let key_type = 2;

        client.set_puzzle_key(&admin, &puzzle_id, &key_type);
        client.grant_keys(&admin, &player1, &key_type, &2);

        assert_eq!(client.open(&player1, &puzzle_id), true);
        assert_eq!(client.key_balance(&player1, &key_type), 1);

        // Second attempt fails even though player still has another key
        let second_open = client.try_open(&player1, &puzzle_id);
        assert_eq!(second_open, Err(Ok(UnlockError::AlreadyUnlocked)));
        assert_eq!(client.key_balance(&player1, &key_type), 1);
    }

    #[test]
    fn test_rejects_unconfigured_puzzle() {
        let (_env, _admin, player1, _player2, client) = setup_env();
        let res = client.try_open(&player1, &999);
        assert_eq!(res, Err(Ok(UnlockError::PuzzleKeyNotConfigured)));
    }
}