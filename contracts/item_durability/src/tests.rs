#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

struct Fixture {
    env: Env,
    owner: Address,
    other: Address,
    sink: Address,
    payment_token: Address,
    material_token: Address,
    contract: Address,
}

impl Fixture {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let owner = Address::generate(&env);
        let other = Address::generate(&env);
        let sink = Address::generate(&env);
        let token_admin = Address::generate(&env);

        let payment_token = env
            .register_stellar_asset_contract_v2(token_admin.clone())
            .address();
        let material_token = env
            .register_stellar_asset_contract_v2(token_admin)
            .address();
        let payment_asset = StellarAssetClient::new(&env, &payment_token);
        payment_asset.mint(&owner, &10_000);

        let contract = env.register_contract(None, ItemDurabilityContract);
        let client = ItemDurabilityContractClient::new(&env, &contract);
        client.initialize(&admin, &payment_token, &sink);

        Fixture {
            env,
            owner,
            other,
            sink,
            payment_token,
            material_token,
            contract,
        }
    }

    fn client(&self) -> ItemDurabilityContractClient<'_> {
        ItemDurabilityContractClient::new(&self.env, &self.contract)
    }

    fn payment(&self) -> TokenClient<'_> {
        TokenClient::new(&self.env, &self.payment_token)
    }

    fn material(&self) -> TokenClient<'_> {
        TokenClient::new(&self.env, &self.material_token)
    }
}

fn register_item(f: &Fixture) -> u64 {
    f.client().register_item(&f.owner, &100, &5, &f.material_token, &400)
}

#[test]
fn registers_an_item_at_full_durability() {
    let f = Fixture::new();
    let id = register_item(&f);
    let item = f.client().get_item(&id);

    assert_eq!(item.id, id);
    assert_eq!(item.owner, f.owner);
    assert_eq!(item.max_durability, 100);
    assert_eq!(item.durability, 100);
    assert!(!item.broken);
    assert!(!item.salvaged);
}

#[test]
fn using_an_item_reduces_durability_and_marks_zero_as_broken() {
    let f = Fixture::new();
    let id = register_item(&f);
    assert_eq!(f.client().use_item(&f.owner, &id, &30), 70);
    assert_eq!(f.client().get_item(&id).durability, 70);
    assert!(!f.client().is_broken(&id));

    assert_eq!(f.client().use_item(&f.owner, &id, &70), 0);
    assert!(f.client().is_broken(&id));
    assert!(f.client().try_use_item(&f.owner, &id, &1).is_err());
}

#[test]
fn use_is_capped_at_remaining_durability() {
    let f = Fixture::new();
    let id = register_item(&f);
    assert_eq!(f.client().use_item(&f.owner, &id, &1_000), 0);
    assert!(f.client().is_broken(&id));
}

#[test]
fn repair_charges_proportional_cost_and_routes_it_to_the_sink() {
    let f = Fixture::new();
    let id = register_item(&f);
    f.client().use_item(&f.owner, &id, &40);

    assert_eq!(f.client().repair(&f.owner, &id), 100);
    let item = f.client().get_item(&id);
    assert_eq!(item.durability, 100);
    assert_eq!(item.max_durability, 100);
    assert_eq!(f.payment().balance(&f.owner), 10_000 - 200);
    assert_eq!(f.payment().balance(&f.sink), 200);
}

#[test]
fn repair_never_exceeds_max_durability() {
    let f = Fixture::new();
    let id = register_item(&f);
    f.client().use_item(&f.owner, &id, &10);
    assert_eq!(f.client().repair(&f.owner, &id), 100);
    // A second repair is a no-op and costs nothing.
    assert_eq!(f.client().repair(&f.owner, &id), 100);
    assert_eq!(f.payment().balance(&f.sink), 50);
}

#[test]
fn broken_items_cannot_be_repaired() {
    let f = Fixture::new();
    let id = register_item(&f);
    f.client().use_item(&f.owner, &id, &100);
    assert!(f.client().try_repair(&f.owner, &id).is_err());
    assert_eq!(f.payment().balance(&f.sink), 0);
}

#[test]
fn only_the_owner_can_use_repair_or_salvage() {
    let f = Fixture::new();
    let id = register_item(&f);
    assert!(f.client().try_use_item(&f.other, &id, &1).is_err());
    assert!(f.client().try_repair(&f.other, &id).is_err());
    assert!(f.client().try_salvage(&f.other, &id).is_err());
    assert_eq!(f.client().get_item(&id).durability, 100);
}

#[test]
fn salvage_returns_configured_materials_and_destroys_the_item() {
    let f = Fixture::new();
    let id = register_item(&f);
    f.client().use_item(&f.owner, &id, &100);

    StellarAssetClient::new(&f.env, &f.material_token).mint(&f.contract, &400);
    assert_eq!(f.client().salvage(&f.owner, &id), 400);
    assert_eq!(f.material().balance(&f.owner), 400);

    // The record is gone, so it cannot be salvaged, repaired or used again.
    assert!(f.client().try_get_item(&id).is_err());
    assert!(f.client().try_salvage(&f.owner, &id).is_err());
    assert!(f.client().try_repair(&f.owner, &id).is_err());
    assert!(f.client().try_use_item(&f.owner, &id, &1).is_err());
}

#[test]
fn only_broken_items_can_be_salvaged() {
    let f = Fixture::new();
    let id = register_item(&f);
    f.client().use_item(&f.owner, &id, &10);
    assert!(f.client().try_salvage(&f.owner, &id).is_err());
    assert_eq!(f.client().get_item(&id).durability, 90);
}
