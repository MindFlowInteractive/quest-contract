use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Env, String,
};

struct TestFixture {
    env: Env,
    _admin: Address,
    creator: Address,
    player: Address,
    token_addr: Address,
}

impl TestFixture {
    fn setup() -> (Self, ContentLicenseContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let player = Address::generate(&env);

        let asset = env.register_stellar_asset_contract_v2(admin.clone());
        let token_addr = asset.address();

        let contract_id = env.register_contract(None, ContentLicenseContract);
        let client = ContentLicenseContractClient::new(&env, &contract_id);

        client.initialize(&admin);

        // Mint payment tokens to player: 10,000
        token::StellarAssetClient::new(&env, &token_addr).mint(&player, &10_000);

        let fixture = Self {
            env,
            _admin: admin,
            creator,
            player,
            token_addr,
        };

        (fixture, client)
    }
}

#[test]
fn test_register_terms_immutability_and_versions() {
    let (f, client) = TestFixture::setup();

    let uri_v1 = String::from_str(&f.env, "ipfs://v1");
    let uri_v2 = String::from_str(&f.env, "ipfs://v2");

    // Register v1: FreeToPlay
    let v1 = client.register_terms(
        &f.creator,
        &1,
        &LicenseType::FreeToPlay,
        &f.token_addr,
        &0,
        &0,
        &false,
        &false,
        &uri_v1,
    );
    assert_eq!(v1, 1);

    // Another user cannot register terms for puzzle 1
    let stranger = Address::generate(&f.env);
    let res = client.try_register_terms(
        &stranger,
        &1,
        &LicenseType::RemixAllowed,
        &f.token_addr,
        &0,
        &0,
        &false,
        &false,
        &uri_v1,
    );
    assert_eq!(res, Err(Ok(LicenseError::NotAuthorized)));

    // Register v2: PaidPerPlay with 100 fee
    let v2 = client.register_terms(
        &f.creator,
        &1,
        &LicenseType::PaidPerPlay,
        &f.token_addr,
        &100,
        &86400,
        &true,
        &true,
        &uri_v2,
    );
    assert_eq!(v2, 2);

    assert_eq!(client.get_latest_version(&1), 2);

    let terms_v1 = client.get_terms_by_version(&1, &1).unwrap();
    assert_eq!(terms_v1.version, 1);
    assert_eq!(terms_v1.fee_amount, 0);

    let terms_v2 = client.get_terms_by_version(&1, &2).unwrap();
    assert_eq!(terms_v2.version, 2);
    assert_eq!(terms_v2.fee_amount, 100);
}

#[test]
fn test_purchase_license_pays_creator_and_records_grant() {
    let (f, client) = TestFixture::setup();
    let uri = String::from_str(&f.env, "ipfs://terms");

    // Creator registers terms: 200 fee, paid per play
    client.register_terms(
        &f.creator,
        &10,
        &LicenseType::PaidPerPlay,
        &f.token_addr,
        &200,
        &0, // lifetime
        &false,
        &false,
        &uri,
    );

    let initial_player_balance = token::Client::new(&f.env, &f.token_addr).balance(&f.player);
    let initial_creator_balance = token::Client::new(&f.env, &f.token_addr).balance(&f.creator);

    // Player purchases license
    let version_bought = client.purchase_license(&f.player, &10);
    assert_eq!(version_bought, 1);

    // Verify creator received fee from player
    let final_player_balance = token::Client::new(&f.env, &f.token_addr).balance(&f.player);
    let final_creator_balance = token::Client::new(&f.env, &f.token_addr).balance(&f.creator);
    assert_eq!(initial_player_balance - final_player_balance, 200);
    assert_eq!(final_creator_balance - initial_creator_balance, 200);

    // Verify active license
    assert_eq!(client.is_license_active(&10, &f.player), true);

    let grant = client.get_license(&10, &f.player).unwrap();
    assert_eq!(grant.version_bought, 1);
    assert_eq!(grant.price_paid, 200);
    assert_eq!(grant.is_revoked, false);
}

#[test]
fn test_revocation_follows_terms_policy() {
    let (f, client) = TestFixture::setup();
    let uri = String::from_str(&f.env, "ipfs://non-revocable");

    // Terms with allows_revocation = false
    client.register_terms(
        &f.creator,
        &20,
        &LicenseType::FreeToPlay,
        &f.token_addr,
        &0,
        &0,
        &false, // no revocation allowed!
        &false,
        &uri,
    );

    client.purchase_license(&f.player, &20);
    assert_eq!(client.is_license_active(&20, &f.player), true);

    // Creator attempts to revoke -> fails because terms prohibit revocation
    let res = client.try_revoke_license(&f.creator, &20, &f.player);
    assert_eq!(res, Err(Ok(LicenseError::RevocationNotAllowed)));
    assert_eq!(client.is_license_active(&20, &f.player), true);
}

#[test]
fn test_revocation_with_refund_transfers_fee_back() {
    let (f, client) = TestFixture::setup();
    let uri = String::from_str(&f.env, "ipfs://refundable");

    // Terms with revocation allowed AND refund on revocation required
    client.register_terms(
        &f.creator,
        &30,
        &LicenseType::PaidPerPlay,
        &f.token_addr,
        &300,
        &0,
        &true, // allows revocation
        &true, // requires refund
        &uri,
    );

    client.purchase_license(&f.player, &30);

    let player_bal_after_purchase = token::Client::new(&f.env, &f.token_addr).balance(&f.player);
    let creator_bal_after_purchase = token::Client::new(&f.env, &f.token_addr).balance(&f.creator);

    // Creator revokes license -> player gets refunded 300 tokens
    client.revoke_license(&f.creator, &30, &f.player);

    let player_bal_after_refund = token::Client::new(&f.env, &f.token_addr).balance(&f.player);
    let creator_bal_after_refund = token::Client::new(&f.env, &f.token_addr).balance(&f.creator);

    assert_eq!(player_bal_after_refund - player_bal_after_purchase, 300);
    assert_eq!(creator_bal_after_purchase - creator_bal_after_refund, 300);

    // License is no longer active
    assert_eq!(client.is_license_active(&30, &f.player), false);

    // Double revoke fails with AlreadyRevoked
    let res_double = client.try_revoke_license(&f.creator, &30, &f.player);
    assert_eq!(res_double, Err(Ok(LicenseError::AlreadyRevoked)));
}

#[test]
fn test_license_expiration() {
    let (f, client) = TestFixture::setup();
    let uri = String::from_str(&f.env, "ipfs://timed");

    // Validity duration: 100 seconds
    client.register_terms(
        &f.creator,
        &40,
        &LicenseType::FreeToPlay,
        &f.token_addr,
        &0,
        &100,
        &false,
        &false,
        &uri,
    );

    client.purchase_license(&f.player, &40);
    assert_eq!(client.is_license_active(&40, &f.player), true);

    // Advance ledger time by 101 seconds
    f.env.ledger().with_mut(|l| l.timestamp += 101);

    // License is now expired
    assert_eq!(client.is_license_active(&40, &f.player), false);
}
