#![no_std]

mod tests;
mod types;

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Symbol};
pub use types::Item;

const EVT_REGISTERED: &str = "item_registered";
const EVT_USED: &str = "item_used";
const EVT_BROKEN: &str = "item_broken";
const EVT_REPAIRED: &str = "item_repaired";
const EVT_SALVAGED: &str = "item_salvaged";

#[contracttype]
pub enum DataKey {
    Admin,
    /// Token used to pay for repairs.
    PaymentToken,
    /// Address that receives repair payments.
    Sink,
    Item(u64),
    NextItemId,
}

fn read_item(env: &Env, item_id: u64) -> Item {
    env.storage()
        .persistent()
        .get(&DataKey::Item(item_id))
        .expect("item not found")
}

#[contract]
pub struct ItemDurabilityContract;

#[contractimpl]
impl ItemDurabilityContract {
    pub fn initialize(env: Env, admin: Address, payment_token: Address, sink: Address) {
        admin.require_auth();
        if env.storage().persistent().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().persistent().set(&DataKey::Admin, &admin);
        env.storage()
            .persistent()
            .set(&DataKey::PaymentToken, &payment_token);
        env.storage().persistent().set(&DataKey::Sink, &sink);
        env.storage().persistent().set(&DataKey::NextItemId, &1u64);
    }

    /// Register a new item at full durability with its repair and salvage
    /// configuration. The owner may be a player or another contract.
    pub fn register_item(
        env: Env,
        owner: Address,
        max_durability: u32,
        repair_cost_per_point: i128,
        material_token: Address,
        salvage_amount: i128,
    ) -> u64 {
        owner.require_auth();
        if max_durability == 0 {
            panic!("max durability must be greater than zero");
        }
        if repair_cost_per_point < 0 || salvage_amount < 0 {
            panic!("amounts must be non-negative");
        }

        let item_id = env
            .storage()
            .persistent()
            .get::<_, u64>(&DataKey::NextItemId)
            .expect("contract not initialized");
        env.storage()
            .persistent()
            .set(&DataKey::NextItemId, &(item_id + 1));

        let item = Item {
            id: item_id,
            owner: owner.clone(),
            max_durability,
            durability: max_durability,
            repair_cost_per_point,
            material_token,
            salvage_amount,
            broken: false,
            salvaged: false,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Item(item_id), &item);
        env.events()
            .publish((Symbol::new(&env, EVT_REGISTERED), owner), item_id);
        item_id
    }

    pub fn get_item(env: Env, item_id: u64) -> Item {
        read_item(&env, item_id)
    }

    pub fn is_broken(env: Env, item_id: u64) -> bool {
        read_item(&env, item_id).broken
    }

    /// Consume `amount` durability points. Broken items cannot be used.
    pub fn use_item(env: Env, caller: Address, item_id: u64, amount: u32) -> u32 {
        caller.require_auth();
        let mut item = read_item(&env, item_id);
        if item.owner != caller {
            panic!("only the owner may use this item");
        }
        if item.broken {
            panic!("broken items cannot be used");
        }

        let consumed = core::cmp::min(amount, item.durability);
        item.durability -= consumed;
        if item.durability == 0 {
            item.broken = true;
            env.events()
                .publish((Symbol::new(&env, EVT_BROKEN), item.owner.clone()), item_id);
        }
        env.storage()
            .persistent()
            .set(&DataKey::Item(item_id), &item);
        env.events().publish(
            (Symbol::new(&env, EVT_USED), item.owner.clone()),
            (item_id, consumed),
        );
        item.durability
    }

    /// Restore durability by paying `missing * repair_cost_per_point` to the
    /// configured sink. Broken items can never be repaired (#400, #401).
    pub fn repair(env: Env, caller: Address, item_id: u64) -> u32 {
        caller.require_auth();
        let mut item = read_item(&env, item_id);
        if item.owner != caller {
            panic!("only the owner may repair this item");
        }
        if item.broken {
            panic!("broken items cannot be repaired");
        }

        let missing = item.max_durability - item.durability;
        if missing == 0 {
            return item.durability;
        }

        let cost = (missing as i128) * item.repair_cost_per_point;
        if cost > 0 {
            let payment_token: Address = env
                .storage()
                .persistent()
                .get(&DataKey::PaymentToken)
                .expect("payment token not configured");
            let sink: Address = env
                .storage()
                .persistent()
                .get(&DataKey::Sink)
                .expect("repair sink not configured");
            token::Client::new(&env, &payment_token).transfer(&caller, &sink, &cost);
        }

        // Never exceed the configured maximum.
        item.durability = item.max_durability;
        env.storage()
            .persistent()
            .set(&DataKey::Item(item_id), &item);
        env.events().publish(
            (Symbol::new(&env, EVT_REPAIRED), item.owner.clone()),
            (item_id, cost),
        );
        item.durability
    }

    /// Salvage a broken item for its configured materials, then destroy the
    /// record so it can never be salvaged or repaired twice (#401).
    pub fn salvage(env: Env, caller: Address, item_id: u64) -> i128 {
        caller.require_auth();
        let mut item = read_item(&env, item_id);
        if item.owner != caller {
            panic!("only the owner may salvage this item");
        }
        if !item.broken {
            panic!("only broken items can be salvaged");
        }
        if item.salvaged {
            panic!("item has already been salvaged");
        }

        if item.salvage_amount > 0 {
            token::Client::new(&env, &item.material_token).transfer(
                &env.current_contract_address(),
                &caller,
                &item.salvage_amount,
            );
        }

        item.salvaged = true;
        env.storage().persistent().remove(&DataKey::Item(item_id));
        env.events().publish(
            (Symbol::new(&env, EVT_SALVAGED), caller),
            (item_id, item.salvage_amount),
        );
        item.salvage_amount
    }
}
