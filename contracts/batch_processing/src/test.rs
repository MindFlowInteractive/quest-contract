use soroban_sdk::{testutils::Address as _, Env};

use super::*;

fn init(env: &Env) -> Address {
    let admin = Address::generate(env);
    BatchProcessingContract::initialize(env.clone(), admin.clone());
    admin
}

#[test]
#[should_panic]
fn queue_rejects_empty_operations() {
    let env = Env::default();
    let admin = init(&env);

    let ops: Vec<BatchOperation> = Vec::new(&env);

    BatchProcessingContract::queue_batch(env.clone(), admin.clone(), true, ops);
}
