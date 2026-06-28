use soroban_sdk::Env;

// Events are intentionally minimal: the contract uses panics for errors and
// stores detailed history in storage.

pub fn batch_queued(env: &Env, batch_id: u64) {
    env.events().publish(("batch_queued",), batch_id);
}

pub fn batch_executing(env: &Env, batch_id: u64) {
    env.events().publish(("batch_executing",), batch_id);
}

pub fn batch_finished_success(env: &Env, batch_id: u64) {
    env.events().publish(("batch_finished_success",), batch_id);
}

pub fn batch_finished_failed(env: &Env, batch_id: u64, failed_at_index: u32) {
    env.events()
        .publish(("batch_finished_failed",), (batch_id, failed_at_index));
}

#[allow(dead_code)]
pub fn batch_cancelled(env: &Env, batch_id: u64) {
    env.events().publish(("batch_cancelled",), batch_id);
}

pub fn operation_result(env: &Env, batch_id: u64, index: u32, ok: bool) {
    env.events()
        .publish(("batch_operation_result",), (batch_id, index, ok));
}
