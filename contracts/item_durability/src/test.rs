use super::*;
use soroban_sdk::{
    testutils::Address as _,
    token, Env,
};

struct TestFixture {
    env: Env,
    _contract_id: Address,
    admin: Address,
    user: Address,
    sink: Address,
    token_addr: Address,
    material_addr: Address,
}

impl TestFixture {
    fn setup() -> (Self, ItemDurabilityContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let sink = Address::generate(&env);

        let payment_asset = env.register_stellar_asset_contract_v2(admin.clone());
        let token_addr = payment_asset.address();

        let material_asset = env.register_stellar_asset_contract_v2(admin.clone());
        let material_addr = material_asset.address();

        let contract_id = env.register_contract(None, ItemDurabilityContract);
        let client = ItemDurabilityContractClient::new(&env, &contract_id);

        client.initialize(&admin);

        // Mint payment tokens to user: 10,000
        token::StellarAssetClient::new(&env, &token_addr).mint(&user, &10_000);

        // Mint material tokens to contract for salvage payouts: 50,000
        token::StellarAssetClient::new(&env, &material_addr).mint(&contract_id, &50_000);

        // Configure repair: 10 tokens per durability unit
        client.set_repair_config(&admin, &token_addr, &10, &sink);

        // Configure salvage: 50 material tokens per broken item
        client.set_salvage_config(&admin, &material_addr, &50);

        let fixture = Self {
            env,
            _contract_id: contract_id,
            admin,
            user,
            sink,
            token_addr,
            material_addr,
        };

        (fixture, client)
    }
}

#[test]
fn test_register_and_use_item_decrements_durability() {
    let (f, client) = TestFixture::setup();

    // Register item 1 with max durability 5
    client.register_item(&f.admin, &1, &f.user, &5);

    assert_eq!(client.get_durability(&1), 5);
    assert_eq!(client.is_broken(&1), false);

    // Use item: 5 -> 4
    let remaining = client.use_item(&f.user, &1);
    assert_eq!(remaining, 4);
    assert_eq!(client.get_durability(&1), 4);
    assert_eq!(client.is_broken(&1), false);

    // Use item: 4 -> 3
    let remaining = client.use_item(&f.user, &1);
    assert_eq!(remaining, 3);
    assert_eq!(client.get_durability(&1), 3);
}

#[test]
fn test_item_breaks_when_durability_reaches_zero() {
    let (f, client) = TestFixture::setup();

    // Register item with durability 2
    client.register_item(&f.admin, &10, &f.user, &2);

    client.use_item(&f.user, &10); // 2 -> 1
    assert_eq!(client.is_broken(&10), false);

    client.use_item(&f.user, &10); // 1 -> 0, now broken!
    assert_eq!(client.get_durability(&10), 0);
    assert_eq!(client.is_broken(&10), true);

    // Using zero durability item fails with ZeroDurability error
    let res = client.try_use_item(&f.user, &10);
    assert_eq!(res, Err(Ok(DurabilityError::ZeroDurability)));
}

#[test]
fn test_repair_item_cost_calculation_and_cap() {
    let (f, client) = TestFixture::setup();

    // Item with max durability 10
    client.register_item(&f.admin, &20, &f.user, &10);

    // Use 3 times: durability becomes 7 (missing 3)
    client.use_item(&f.user, &20);
    client.use_item(&f.user, &20);
    client.use_item(&f.user, &20);
    assert_eq!(client.get_durability(&20), 7);

    // Repair cost for 2 units: 2 * 10 = 20 tokens
    let calculated_cost = client.calculate_repair_cost(&20, &2);
    assert_eq!(calculated_cost, 20);

    let initial_user_balance = token::Client::new(&f.env, &f.token_addr).balance(&f.user);
    let initial_sink_balance = token::Client::new(&f.env, &f.token_addr).balance(&f.sink);

    // Repair 2 units
    let new_durability = client.repair_item(&f.user, &20, &2);
    assert_eq!(new_durability, 9);
    assert_eq!(client.get_durability(&20), 9);

    // Verify token payment routed to sink
    let final_user_balance = token::Client::new(&f.env, &f.token_addr).balance(&f.user);
    let final_sink_balance = token::Client::new(&f.env, &f.token_addr).balance(&f.sink);
    assert_eq!(initial_user_balance - final_user_balance, 20);
    assert_eq!(final_sink_balance - initial_sink_balance, 20);

    // Requesting more units than missing is capped at max durability
    // Durability is 9, max is 10 (missing 1). Requesting 5 repairs only 1 unit!
    let capped_durability = client.repair_item(&f.user, &20, &5);
    assert_eq!(capped_durability, 10);
    assert_eq!(client.get_durability(&20), 10);

    // Cost for 1 capped unit was 10
    let user_balance_after_cap = token::Client::new(&f.env, &f.token_addr).balance(&f.user);
    assert_eq!(final_user_balance - user_balance_after_cap, 10);
}

#[test]
fn test_broken_item_cannot_be_repaired() {
    let (f, client) = TestFixture::setup();

    // Register item with durability 1
    client.register_item(&f.admin, &30, &f.user, &1);
    client.use_item(&f.user, &30); // durability 0, item breaks!

    assert_eq!(client.is_broken(&30), true);

    // Attempting to repair a broken item must fail with ItemBroken
    let res = client.try_repair_item(&f.user, &30, &1);
    assert_eq!(res, Err(Ok(DurabilityError::ItemBroken)));
}

#[test]
fn test_salvage_broken_item_and_destroy_record() {
    let (f, client) = TestFixture::setup();

    client.register_item(&f.admin, &40, &f.user, &1);

    // Attempting to salvage non-broken item fails with ItemNotBroken
    let res = client.try_salvage_item(&f.user, &40);
    assert_eq!(res, Err(Ok(DurabilityError::ItemNotBroken)));

    // Break the item
    client.use_item(&f.user, &40);
    assert_eq!(client.is_broken(&40), true);

    let initial_material_balance = token::Client::new(&f.env, &f.material_addr).balance(&f.user);

    // Salvage broken item
    let payout = client.salvage_item(&f.user, &40);
    assert_eq!(payout, 50);

    // Verify material token received by user
    let final_material_balance = token::Client::new(&f.env, &f.material_addr).balance(&f.user);
    assert_eq!(final_material_balance - initial_material_balance, 50);

    // Verify item record destroyed after salvage
    assert_eq!(client.get_item(&40), None);

    // Cannot salvage twice: item not found
    let res_double = client.try_salvage_item(&f.user, &40);
    assert_eq!(res_double, Err(Ok(DurabilityError::ItemNotFound)));
}

#[test]
fn test_unauthorized_user_cannot_use_repair_or_salvage() {
    let (f, client) = TestFixture::setup();
    let stranger = Address::generate(&f.env);

    client.register_item(&f.admin, &50, &f.user, &3);

    // Stranger cannot use item
    let res_use = client.try_use_item(&stranger, &50);
    assert_eq!(res_use, Err(Ok(DurabilityError::NotAuthorized)));

    // Stranger cannot repair item
    let res_repair = client.try_repair_item(&stranger, &50, &1);
    assert_eq!(res_repair, Err(Ok(DurabilityError::NotAuthorized)));

    // Break item
    client.use_item(&f.user, &50);
    client.use_item(&f.user, &50);
    client.use_item(&f.user, &50);

    // Stranger cannot salvage item
    let res_salvage = client.try_salvage_item(&stranger, &50);
    assert_eq!(res_salvage, Err(Ok(DurabilityError::NotAuthorized)));
}
