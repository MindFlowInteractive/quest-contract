#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger as _},
        token, Env,
    };

    fn setup_faucet(
        cooldown_ledgers: u32,
        max_amount: i128,
        initial_faucet_funds: i128,
    ) -> (
        Env,
        Address,
        Address,
        Address,
        token::Client<'static>,
        TestnetFaucetContractClient<'static>,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set(soroban_sdk::testutils::LedgerInfo {
            timestamp: 1_700_000_000,
            protocol_version: 22,
            sequence_number: 100,
            network_id: [0; 32],
            base_reserve: 10,
            min_temp_entry_ttl: 100,
            min_persistent_entry_ttl: 100,
            max_entry_ttl: 10_000,
        });

        let admin = Address::generate(&env);
        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);

        let token_admin = Address::generate(&env);
        let token_contract = env.register_stellar_asset_contract_v2(token_admin.clone());
        let token_addr = token_contract.address();
        let sac_client = token::StellarAssetClient::new(&env, &token_addr);
        let token_client = token::Client::new(&env, &token_addr);

        let faucet_id = env.register(TestnetFaucetContract, ());
        let faucet_client = TestnetFaucetContractClient::new(&env, &faucet_id);

        faucet_client.initialize(&admin, &token_addr, &cooldown_ledgers, &max_amount);
        if initial_faucet_funds > 0 {
            sac_client.mint(&faucet_id, &initial_faucet_funds);
        }

        let leaked_token: token::Client<'static> = unsafe { core::mem::transmute(token_client) };
        let leaked_faucet: TestnetFaucetContractClient<'static> =
            unsafe { core::mem::transmute(faucet_client) };

        (env, admin, user1, user2, leaked_token, leaked_faucet)
    }

    #[test]
    fn test_faucet_request_enforces_cooldown_and_records_ledger() {
        let (env, _admin, user1, user2, token, faucet) = setup_faucet(50, 1_000, 10_000);

        // First request at ledger 100 succeeds
        let claimed = faucet.request(&user1, &400);
        assert_eq!(claimed, 400);
        assert_eq!(token.balance(&user1), 400);
        assert_eq!(faucet.last_request_ledger(&user1), Some(100));
        assert_eq!(faucet.next_eligible_ledger(&user1), 150);

        // Immediate repeat request at ledger 100 is rejected
        assert_eq!(
            faucet.try_request(&user1, &100),
            Err(Ok(FaucetError::CooldownActive))
        );

        // Still inside cooldown at ledger 149
        env.ledger().set_sequence_number(149);
        assert_eq!(
            faucet.try_request(&user1, &100),
            Err(Ok(FaucetError::CooldownActive))
        );

        // Another address has its own independent cooldown
        assert_eq!(faucet.request(&user2, &250), 250);
        assert_eq!(token.balance(&user2), 250);

        // Eligible again once ledger reaches 150
        env.ledger().set_sequence_number(150);
        assert_eq!(faucet.request(&user1, &300), 300);
        assert_eq!(token.balance(&user1), 700);
        assert_eq!(faucet.last_request_ledger(&user1), Some(150));
    }

    #[test]
    fn test_faucet_caps_amount_per_request() {
        let (_env, _admin, user1, _user2, token, faucet) = setup_faucet(20, 500, 5_000);

        // Above max_amount (500) is rejected
        assert_eq!(
            faucet.try_request(&user1, &501),
            Err(Ok(FaucetError::ExceedsMaxAmount))
        );
        assert_eq!(token.balance(&user1), 0);
        assert_eq!(faucet.last_request_ledger(&user1), None);

        // Non-positive amounts are rejected
        assert_eq!(
            faucet.try_request(&user1, &0),
            Err(Ok(FaucetError::InvalidAmount))
        );
        assert_eq!(
            faucet.try_request(&user1, &-10),
            Err(Ok(FaucetError::InvalidAmount))
        );

        // Exact cap (500) succeeds
        assert_eq!(faucet.request(&user1, &500), 500);
        assert_eq!(token.balance(&user1), 500);
    }
}