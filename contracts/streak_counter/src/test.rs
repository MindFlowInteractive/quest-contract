use super::*;
use soroban_sdk::{testutils::{Address as _, Ledger}, token, vec, IntoVal};

struct Fixture { env: Env, id: Address, admin: Address, verifier: Address, token: Address, player: Address }
impl Fixture {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let verifier = Address::generate(&env);
        let player = Address::generate(&env);
        let token = env.register_stellar_asset_contract_v2(admin.clone()).address();
        let id = env.register_contract(None, StreakCounter);
        let f = Self { env, id, admin, verifier, token, player };
        let mut milestones = Map::new(&f.env);
        milestones.set(7, 70);
        milestones.set(30, 300);
        milestones.set(100, 1000);
        f.client().initialize(&f.admin, &f.verifier, &f.token, &(200 * DAY), &milestones);
        token::StellarAssetClient::new(&f.env, &f.token).mint(&f.admin, &10_000);
        f
    }
    fn client(&self) -> StreakCounterClient<'_> { StreakCounterClient::new(&self.env, &self.id) }
    fn day(&self, day: u64) { self.env.ledger().with_mut(|l| l.timestamp = day * DAY); }
    fn reach(&self, days: u64) {
        for day in 0..days { self.day(day); self.client().check_in(&self.player); }
    }
    fn tokens(&self, who: &Address) -> i128 { token::Client::new(&self.env, &self.token).balance(who) }
}

#[test]
fn milestones_pay_once_and_conserve_tokens() {
    let f = Fixture::new();
    f.client().fund(&f.admin, &1370);
    f.reach(100);
    for milestone in [7, 30, 100] {
        assert_eq!(f.client().claim(&f.player, &milestone), f.client().get_config().milestones.get(milestone).unwrap());
        assert!(f.client().has_claimed(&f.player, &milestone));
        assert_eq!(f.client().try_claim(&f.player, &milestone), Err(Ok(Error::AlreadyClaimed)));
    }
    assert_eq!(f.tokens(&f.player), 1370);
    assert_eq!(f.client().balance(), 0);
    assert_eq!(f.tokens(&f.admin) + f.tokens(&f.player), 10_000);
}

#[test]
fn underfunded_claim_remains_available_after_top_up() {
    let f = Fixture::new();
    f.reach(7);
    f.client().fund(&f.admin, &69);
    assert_eq!(f.client().try_claim(&f.player, &7), Err(Ok(Error::InsufficientFunds)));
    assert!(!f.client().has_claimed(&f.player, &7));
    assert_eq!(f.client().balance(), 69);
    f.client().fund(&f.admin, &1);
    assert_eq!(f.client().claim(&f.player, &7), 70);
}

#[test]
fn eligibility_and_players_are_independent() {
    let f = Fixture::new();
    f.client().fund(&f.admin, &140);
    assert_eq!(f.client().try_claim(&f.player, &7), Err(Ok(Error::Ineligible)));
    assert_eq!(f.client().try_claim(&f.player, &8), Err(Ok(Error::UnknownMilestone)));
    let other = Address::generate(&f.env);
    for day in 0..7 { f.day(day); f.client().check_in(&f.player); f.client().check_in(&other); }
    f.client().claim(&f.player, &7);
    assert!(!f.client().has_claimed(&other, &7));
    f.client().claim(&other, &7);
    assert_eq!(f.tokens(&other), 70);
    assert_eq!(f.client().try_claim(&f.player, &30), Err(Ok(Error::Ineligible)));
}

#[test]
fn day_zero_repeats_boundaries_and_gaps() {
    let f = Fixture::new();
    assert_eq!(f.client().check_in(&f.player).current, 1);
    f.env.ledger().with_mut(|l| l.timestamp = DAY - 1);
    assert_eq!(f.client().check_in(&f.player).current, 1);
    f.day(1);
    assert_eq!(f.client().check_in(&f.player).current, 2);
    f.day(3);
    let streak = f.client().check_in(&f.player);
    assert_eq!((streak.current, streak.best), (1, 2));
    f.day(2);
    assert_eq!(f.client().try_check_in(&f.player), Err(Ok(Error::InvalidDay)));
}

#[test]
fn earned_milestone_survives_a_reset_but_cannot_be_reearned() {
    let f = Fixture::new();
    f.client().fund(&f.admin, &140);
    f.reach(7);
    f.day(10);
    assert_eq!(f.client().check_in(&f.player).current, 1);
    f.client().claim(&f.player, &7);
    for day in 11..17 { f.day(day); f.client().check_in(&f.player); }
    assert_eq!(f.client().try_claim(&f.player, &7), Err(Ok(Error::AlreadyClaimed)));
}

#[test]
fn expiry_closes_claims_and_funding_and_opens_withdrawal() {
    let f = Fixture::new();
    f.client().fund(&f.admin, &100);
    f.reach(7);
    assert_eq!(f.client().try_withdraw(&1), Err(Ok(Error::NotExpired)));
    f.env.ledger().with_mut(|l| l.timestamp = 200 * DAY - 1);
    f.client().claim(&f.player, &7);
    f.day(200);
    assert_eq!(f.client().try_claim(&f.player, &30), Err(Ok(Error::Expired)));
    assert_eq!(f.client().try_fund(&f.admin, &1), Err(Ok(Error::Expired)));
    assert_eq!(f.client().try_check_in(&f.player), Err(Ok(Error::Expired)));
    assert_eq!(f.client().try_withdraw(&31), Err(Ok(Error::InsufficientFunds)));
    f.client().withdraw(&10);
    f.client().withdraw(&20);
    assert_eq!(f.client().balance(), 0);
    assert_eq!(f.tokens(&f.admin), 9930);
}

#[test]
fn invalid_deposits_and_failed_transfers_do_not_change_balance() {
    let f = Fixture::new();
    for amount in [0, -1] { assert_eq!(f.client().try_fund(&f.admin, &amount), Err(Ok(Error::InvalidAmount))); }
    assert!(f.client().try_fund(&f.admin, &10_001).is_err());
    assert_eq!(f.client().balance(), 0);
    assert_eq!(f.tokens(&f.admin), 10_000);
    f.day(200);
    assert_eq!(f.client().try_withdraw(&0), Err(Ok(Error::InvalidAmount)));
}

#[test]
fn failed_payout_rolls_back_claim_marker() {
    let f = Fixture::new();
    f.client().fund(&f.admin, &70);
    f.reach(7);
    let asset = token::StellarAssetClient::new(&f.env, &f.token);
    asset.set_authorized(&f.player, &false);
    assert!(f.client().try_claim(&f.player, &7).is_err());
    assert!(!f.client().has_claimed(&f.player, &7));
    assert_eq!(f.client().balance(), 70);
    asset.set_authorized(&f.player, &true);
    f.client().claim(&f.player, &7);
}

#[test]
fn authorization_is_required_for_sensitive_operations() {
    let f = Fixture::new();
    f.reach(7);
    f.client().fund(&f.admin, &70);
    f.env.mock_auths(&[]);
    assert!(f.client().try_check_in(&f.player).is_err());
    assert!(f.client().try_claim(&f.player, &7).is_err());
    assert!(f.client().try_fund(&f.admin, &1).is_err());
    f.day(200);
    assert!(f.client().try_withdraw(&70).is_err());
}

#[test]
fn check_in_authorizes_verifier_not_player() {
    let f = Fixture::new();
    f.client().check_in(&f.player);
    assert_eq!(f.env.auths()[0].0, f.verifier);
    assert_eq!(f.env.auths()[0].1.function, soroban_sdk::testutils::AuthorizedFunction::Contract((f.id.clone(), symbol_short!("check_in"), vec![&f.env, f.player.into_val(&f.env)])));
}

#[test]
fn config_is_validated_and_cannot_be_replaced() {
    let f = Fixture::new();
    let terms = f.client().get_config();
    assert_eq!(f.client().try_initialize(&f.admin, &f.verifier, &f.token, &terms.expires_at, &terms.milestones), Err(Ok(Error::AlreadyInitialized)));
    for (days, amount, expiry) in [(0, 1, DAY), (1, 0, DAY), (1, -1, DAY), (1, 1, 0)] {
        let id = f.env.register_contract(None, StreakCounter);
        let c = StreakCounterClient::new(&f.env, &id);
        let mut map = Map::new(&f.env); map.set(days, amount);
        assert_eq!(c.try_initialize(&f.admin, &f.verifier, &f.token, &expiry, &map), Err(Ok(Error::InvalidConfig)));
    }
    let id = f.env.register_contract(None, StreakCounter);
    let c = StreakCounterClient::new(&f.env, &id);
    assert_eq!(c.try_balance(), Err(Ok(Error::NotInitialized)));
    assert_eq!(c.try_initialize(&f.admin, &f.verifier, &f.token, &DAY, &Map::new(&f.env)), Err(Ok(Error::InvalidConfig)));
}
