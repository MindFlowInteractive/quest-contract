#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, String, Vec,
};

struct Ctx {
    env: Env,
    contract: Address,
    token: Address,
    admin: Address,
    treasury: Address,
    reporter: Address,
    target: Address,
    panel: Address,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let reporter = Address::generate(&env);
    let target = Address::generate(&env);
    let panel = Address::generate(&env);

    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let contract = env.register_contract(None, DisputeBondContract);

    let client = DisputeBondContractClient::new(&env, &contract);
    client.initialize(&admin, &token, &treasury).unwrap();

    StellarAssetClient::new(&env, &token).mint(&reporter, &10_000);

    Ctx {
        env,
        contract,
        token,
        admin,
        treasury,
        reporter,
        target,
        panel,
    }
}

fn client(ctx: &Ctx) -> DisputeBondContractClient<'_> {
    DisputeBondContractClient::new(&ctx.env, &ctx.contract)
}

#[test]
fn test_file_report_locks_bond() {
    let ctx = setup();
    let c = client(&ctx);

    let id = c.file_report(
        &ctx.reporter,
        &ctx.target,
        &String::from_str(&ctx.env, "spam report"),
        &500,
    );

    assert_eq!(id, 1);
    let report = c.get_report(&id).unwrap();
    assert_eq!(report.reporter, ctx.reporter);
    assert_eq!(report.target, ctx.target);
    assert_eq!(report.bond, 500);
    assert_eq!(report.status, ReportStatus::Open);
    assert!(!c.is_resolved(&id));

    assert_eq!(
        TokenClient::new(&ctx.env, &ctx.token).balance(&ctx.contract),
        500
    );
    assert_eq!(
        TokenClient::new(&ctx.env, &ctx.token).balance(&ctx.reporter),
        9_500
    );
}

#[test]
fn test_upheld_refunds_reporter_and_resolves_once() {
    let ctx = setup();
    let c = client(&ctx);

    let id = c.file_report(
        &ctx.reporter,
        &ctx.target,
        &String::from_str(&ctx.env, "valid"),
        &500,
    );

    c.resolve_report(&ctx.admin, &id, &ReportStatus::Upheld)
        .unwrap();

    let report = c.get_report(&id).unwrap();
    assert_eq!(report.status, ReportStatus::Upheld);
    assert_eq!(report.resolver, Some(ctx.admin.clone()));
    assert!(c.is_resolved(&id));
    assert_eq!(
        TokenClient::new(&ctx.env, &ctx.token).balance(&ctx.reporter),
        10_000
    );

    assert_eq!(
        c.try_resolve_report(&ctx.admin, &id, &ReportStatus::Rejected),
        Err(Ok(DisputeBondError::AlreadyResolved))
    );
}

#[test]
fn test_rejected_forfeits_bond_to_treasury() {
    let ctx = setup();
    let c = client(&ctx);

    let id = c.file_report(
        &ctx.reporter,
        &ctx.target,
        &String::from_str(&ctx.env, "invalid"),
        &500,
    );

    c.resolve_report(&ctx.admin, &id, &ReportStatus::Rejected)
        .unwrap();

    assert_eq!(
        c.get_report(&id).unwrap().status,
        ReportStatus::Rejected
    );
    assert_eq!(
        TokenClient::new(&ctx.env, &ctx.token).balance(&ctx.treasury),
        500
    );
    assert_eq!(
        TokenClient::new(&ctx.env, &ctx.token).balance(&ctx.reporter),
        9_500
    );
}

#[test]
fn test_only_admin_or_panel_can_resolve() {
    let ctx = setup();
    let c = client(&ctx);

    let id = c.file_report(
        &ctx.reporter,
        &ctx.target,
        &String::from_str(&ctx.env, "valid"),
        &500,
    );

    assert_eq!(
        c.try_resolve_report(&ctx.reporter, &id, &ReportStatus::Upheld),
        Err(Ok(DisputeBondError::Unauthorized))
    );

    c.set_panel(&ctx.admin, &Vec::from_array(&ctx.env, [ctx.panel.clone()]))
        .unwrap();
    c.resolve_report(&ctx.panel, &id, &ReportStatus::Upheld)
        .unwrap();

    assert_eq!(
        c.get_report(&id).unwrap().status,
        ReportStatus::Upheld
    );
}
