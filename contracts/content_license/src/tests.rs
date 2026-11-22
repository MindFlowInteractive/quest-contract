#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

struct Fixture {
    env: Env,
    admin: Address,
    creator: Address,
    buyer: Address,
    other: Address,
    token: Address,
    contract: Address,
}

impl Fixture {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().with_mut(|info| info.timestamp = 1_000);

        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let buyer = Address::generate(&env);
        let other = Address::generate(&env);
        let token_admin = Address::generate(&env);

        let token = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
        StellarAssetClient::new(&env, &token).mint(&buyer, &10_000);

        let contract = env.register_contract(None, ContentLicenseContract);
        let client = ContentLicenseContractClient::new(&env, &contract);
        client.initialize(&admin, &token);

        Fixture {
            env,
            admin,
            creator,
            buyer,
            other,
            token,
            contract,
        }
    }

    fn client(&self) -> ContentLicenseContractClient<'_> {
        ContentLicenseContractClient::new(&self.env, &self.contract)
    }

    fn token(&self) -> TokenClient<'_> {
        TokenClient::new(&self.env, &self.token)
    }
}

#[test]
fn registers_immutable_versioned_terms() {
    let f = Fixture::new();
    let client = f.client();

    assert_eq!(
        client.register_terms(
            &f.creator,
            &7,
            &LicenseKind::FreeToPlay,
            &0,
            &0,
            &false,
            &false,
        ),
        1
    );
    let first = client.get_terms(&7, &1);
    assert_eq!(first.creator, f.creator);
    assert_eq!(first.kind, LicenseKind::FreeToPlay);
    assert_eq!(first.version, 1);
    assert_eq!(first.published_at, 1_000);
    assert_eq!(client.get_current_terms(&7), first);
    assert_eq!(client.get_versions(&7), soroban_sdk::vec![&f.env, 1]);

    assert_eq!(
        client.register_terms(
            &f.creator,
            &7,
            &LicenseKind::PaidPerPlay,
            &100,
            &0,
            &true,
            &true,
        ),
        2
    );
    // The first version never changes.
    assert_eq!(client.get_terms(&7, &1), first);
    assert_eq!(client.get_current_terms(&7).price, 100);
    assert_eq!(client.get_versions(&7), soroban_sdk::vec![&f.env, 1, 2]);
}

#[test]
fn only_the_puzzle_creator_publishes_new_versions() {
    let f = Fixture::new();
    let client = f.client();
    client.register_terms(&f.creator, &7, &LicenseKind::FreeToPlay, &0, &0, &false, &false);

    let result = client.try_register_terms(
        &f.other,
        &7,
        &LicenseKind::PaidPerPlay,
        &50,
        &0,
        &true,
        &false,
    );
    assert!(result.is_err());
    assert_eq!(client.get_versions(&7), soroban_sdk::vec![&f.env, 1]);
}

#[test]
fn purchase_pays_the_creator_and_binds_the_terms_version() {
    let f = Fixture::new();
    let client = f.client();
    client.register_terms(
        &f.creator,
        &7,
        &LicenseKind::PaidPerPlay,
        &250,
        &0,
        &false,
        &false,
    );

    let license = client.purchase(&f.buyer, &7, &1);
    assert_eq!(license.holder, f.buyer);
    assert_eq!(license.version, 1);
    assert_eq!(license.expires_at, 0);
    assert!(!license.revoked);
    assert!(client.is_license_active(&f.buyer, &7));

    assert_eq!(f.token().balance(&f.creator), 250);
    assert_eq!(f.token().balance(&f.buyer), 10_000 - 250);
    assert_eq!(client.get_license(&f.buyer, &7), license);
}

#[test]
fn free_to_play_purchase_charges_nothing() {
    let f = Fixture::new();
    let client = f.client();
    client.register_terms(&f.creator, &7, &LicenseKind::FreeToPlay, &0, &0, &false, &false);
    client.purchase(&f.buyer, &7, &1);
    assert_eq!(f.token().balance(&f.buyer), 10_000);
}

#[test]
fn license_expiry_is_derived_from_the_terms() {
    let f = Fixture::new();
    let client = f.client();
    client.register_terms(&f.creator, &7, &LicenseKind::PaidPerPlay, &0, &100, &false, &false);
    let license = client.purchase(&f.buyer, &7, &1);
    assert_eq!(license.expires_at, 1_100);

    f.env.ledger().with_mut(|info| info.timestamp = 1_101);
    assert!(!client.is_license_active(&f.buyer, &7));
}

#[test]
fn cannot_purchase_twice_while_a_license_is_active() {
    let f = Fixture::new();
    let client = f.client();
    client.register_terms(&f.creator, &7, &LicenseKind::FreeToPlay, &0, &0, &false, &false);
    client.purchase(&f.buyer, &7, &1);
    assert!(client.try_purchase(&f.buyer, &7, &1).is_err());
}

#[test]
fn revocation_follows_published_terms_only() {
    let f = Fixture::new();
    let client = f.client();
    client.register_terms(&f.creator, &7, &LicenseKind::PaidPerPlay, &100, &0, &false, &false);
    client.purchase(&f.buyer, &7, &1);

    // Terms published without revocation rights cannot be revoked.
    assert!(client.try_revoke(&f.creator, &f.buyer, &7).is_err());
    assert!(client.is_license_active(&f.buyer, &7));

    // Version 2 allows revocation, but a licence bought under v1 still follows v1.
    client.register_terms(&f.creator, &7, &LicenseKind::PaidPerPlay, &100, &0, &true, &false);
    assert!(client.try_revoke(&f.creator, &f.buyer, &7).is_err());

    // A different address may never revoke.
    client.purchase(&f.buyer, &8, &2);
    assert!(client.try_revoke(&f.other, &f.buyer, &8).is_err());
}

#[test]
fn revocation_refunds_when_the_terms_require_it() {
    let f = Fixture::new();
    let client = f.client();
    client.register_terms(&f.creator, &7, &LicenseKind::PaidPerPlay, &300, &0, &true, &true);
    client.purchase(&f.buyer, &7, &1);
    assert_eq!(f.token().balance(&f.buyer), 9_700);

    // Fund the contract so it can honour the refund.
    StellarAssetClient::new(&f.env, &f.token).mint(&f.contract, &300);
    let license = client.revoke(&f.creator, &f.buyer, &7);

    assert!(license.revoked);
    assert!(!client.is_license_active(&f.buyer, &7));
    assert_eq!(f.token().balance(&f.buyer), 10_000);
    assert!(client.try_revoke(&f.creator, &f.buyer, &7).is_err());
}

#[test]
fn queries_for_unknown_records_fail() {
    let f = Fixture::new();
    let client = f.client();
    assert!(client.try_get_terms(&99, &1).is_err());
    assert!(client.try_get_current_terms(&99).is_err());
    assert_eq!(client.get_versions(&99).len(), 0);
    assert!(!client.is_license_active(&f.buyer, &99));
    assert!(client.try_get_license(&f.buyer, &99).is_err());
}
