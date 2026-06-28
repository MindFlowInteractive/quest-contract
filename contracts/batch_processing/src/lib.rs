#![no_std]

//! Batch processing contract (Soroban).
//!
//! This contract stores batch definitions (a sequence of operations) and
//! executes them in order.
//!
//! Design notes:
//! - Soroban transactions are atomic: in "atomic" mode we simply `panic`
//!   on the first failure to revert all state changes.
//! - In "partial" mode we swallow per-operation errors, keep executing,
//!   and persist per-operation results in history.
//! - Operations are modeled as explicit variants because contracts cannot
//!   safely perform fully dynamic "call arbitrary function" behavior.

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, Vec};

mod events;

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatchStatus {
    Queued = 0,
    Executing = 1,
    Succeeded = 2,
    Failed = 3,
    Cancelled = 4,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatchErrorCode {
    BadBatch = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    Paused = 4,
    NotQueued = 5,
    AlreadyExecuting = 6,
    EmptyBatch = 7,
    ConcurrencyLimitExceeded = 8,
    ExecutionFailed = 9,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationResult {
    pub index: u32,
    pub ok: bool,
    pub error_code: u32,
}

// Currently, the batch contract supports Quest operations by targeting a
// specific contract address and using a fixed set of entrypoints.
//
// This keeps the contract deterministic and testable.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BatchOperation {
    // For SDK compatibility we use tuple variants.
    QuestCreateBatch(Address, Address, Vec<quest::QuestInput>),
    QuestClaimReward(Address, Address, u64),
}

// Mirrors just enough of `contracts/quest` types for cross-contract
// serialization.
//
// Using a local module named `quest` avoids name collisions.
mod quest {
    use super::*;
    #[contracttype]
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Difficulty {
        Easy = 0,
        Medium = 1,
        Hard = 2,
        Legendary = 3,
    }

    #[contracttype]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub enum TokenType {
        Native,
        ERC20,
        ERC721,
    }

    #[contracttype]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct Reward {
        pub token_type: TokenType,
        pub token_address: Option<Address>,
        pub amount: i128,
    }

    #[contracttype]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct QuestInput {
        pub title: String,
        pub description: String,
        pub tags: Vec<String>,
        pub reward: i128,
        pub difficulty: Difficulty,
        pub rewards: Vec<Reward>,
    }
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Batch {
    pub id: u64,
    pub creator: Address,
    pub status: BatchStatus,
    pub atomic: bool,
    pub operations: Vec<BatchOperation>,
    pub created_at: u64,
    pub finished_at: Option<u64>,
    pub history: Vec<OperationResult>,
    pub failed_at_index: Option<u32>,
}

#[contract]
pub struct BatchProcessingContract;

#[contractimpl]
impl BatchProcessingContract {
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error(&env, BatchErrorCode::NotInitialized);
        }

        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::BatchCounter, &0u64);
        env.storage()
            .instance()
            .set(&DataKey::ActiveExecutions, &0u32);
        env.storage().instance().set(&DataKey::QueueHead, &0u64);
        env.storage().instance().set(&DataKey::QueueTail, &0u64);
    }

    pub fn pause(env: Env, caller: Address) {
        require_admin(&env, &caller);
        env.storage().instance().set(&DataKey::Paused, &true);
    }

    pub fn unpause(env: Env, caller: Address) {
        require_admin(&env, &caller);
        env.storage().instance().set(&DataKey::Paused, &false);
    }

    pub fn is_paused(env: Env) -> bool {
        is_paused_internal(&env)
    }

    /// Queue a batch for later execution.
    /// Returns batch id.
    pub fn queue_batch(
        env: Env,
        creator: Address,
        atomic: bool,
        operations: Vec<BatchOperation>,
    ) -> u64 {
        require_not_paused(&env);
        creator.require_auth();

        if operations.is_empty() {
            panic_with_error(&env, BatchErrorCode::EmptyBatch);
        }

        // Basic validation: ensure operation variants have required fields.
        // Deeper validation occurs during execution via the target contracts.
        for i in 0..operations.len() {
            let op = operations.get(i).unwrap();
            validate_operation(&env, &op);
        }

        let id = next_batch_id(&env);
        let batch = Batch {
            id,
            creator: creator.clone(),
            status: BatchStatus::Queued,
            atomic,
            operations: operations.clone(),
            created_at: env.ledger().timestamp(),
            finished_at: None,
            history: Vec::new(&env),
            failed_at_index: None,
        };

        env.storage().persistent().set(&DataKey::Batch(id), &batch);

        // enqueue id
        let tail: u64 = env
            .storage()
            .instance()
            .get(&DataKey::QueueTail)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::QueueTail, &(tail + 1));
        env.storage()
            .persistent()
            .set(&DataKey::QueueSlot(tail), &id);

        events::batch_queued(&env, id);
        id
    }

    /// Execute up to `max_batches` queued batches.
    /// Concurrency limit is modeled as max number of batches per call.
    pub fn process_queue(env: Env, admin: Address, max_batches: u32) {
        require_admin(&env, &admin);
        require_not_paused(&env);

        if max_batches == 0 {
            return;
        }

        let head: u64 = env
            .storage()
            .instance()
            .get(&DataKey::QueueHead)
            .unwrap_or(0);
        let tail: u64 = env
            .storage()
            .instance()
            .get(&DataKey::QueueTail)
            .unwrap_or(0);
        let pending = tail.saturating_sub(head);
        if pending == 0 {
            return;
        }

        let to_process = core::cmp::min(pending as u32, max_batches) as u64;
        for _ in 0..to_process {
            let slot_id = env
                .storage()
                .persistent()
                .get(&DataKey::QueueSlot(head))
                .unwrap();
            // Advance head regardless; if batch fails we mark failed and it won't be retried.
            head_increment(&env, head);

            // Execute batch
            Self::execute_batch_inner(&env, slot_id);
        }

        env.storage()
            .instance()
            .set(&DataKey::QueueHead, &(head + to_process));
    }

    /// Execute a single batch immediately (admin only).
    pub fn execute_batch(env: Env, caller: Address, batch_id: u64) {
        require_admin(&env, &caller);
        require_not_paused(&env);
        Self::execute_batch_inner(&env, batch_id);
    }

    pub fn get_batch(env: Env, batch_id: u64) -> Batch {
        env.storage()
            .persistent()
            .get(&DataKey::Batch(batch_id))
            .unwrap_or_else(|| panic_with_error(&env, BatchErrorCode::BadBatch))
    }

    pub fn get_batch_status(env: Env, batch_id: u64) -> BatchStatus {
        Self::get_batch(env, batch_id).status
    }

    pub fn get_batch_history(env: Env, batch_id: u64) -> Vec<OperationResult> {
        Self::get_batch(env, batch_id).history
    }
}

impl BatchProcessingContract {
    fn execute_batch_inner(env: &Env, batch_id: u64) {
        let mut batch: Batch = env
            .storage()
            .persistent()
            .get(&DataKey::Batch(batch_id))
            .unwrap_or_else(|| panic_with_error(env, BatchErrorCode::BadBatch));

        if batch.status != BatchStatus::Queued {
            panic_with_error(env, BatchErrorCode::NotQueued);
        }

        // Concurrency limit guard. Because we execute synchronously, we just ensure
        // we don't exceed 1 execution per transaction by default.
        let active: u32 = env
            .storage()
            .instance()
            .get(&DataKey::ActiveExecutions)
            .unwrap_or(0);
        if active >= 1 {
            panic_with_error(env, BatchErrorCode::AlreadyExecuting);
        }
        env.storage()
            .instance()
            .set(&DataKey::ActiveExecutions, &(active + 1));

        events::batch_executing(env, batch_id);

        batch.status = BatchStatus::Executing;
        let mut history: Vec<OperationResult> = Vec::new(env);

        // Execute operations sequentially.
        for i in 0..batch.operations.len() {
            let op = batch.operations.get(i).unwrap();
            let index_u32: u32 = i as u32;

            let res = execute_operation(env, &op, &batch.creator);
            match res {
                Ok(()) => {
                    events::operation_result(env, batch_id, index_u32, true);
                    history.push_back(OperationResult {
                        index: index_u32,
                        ok: true,
                        error_code: 0,
                    });
                }
                Err(code) => {
                    events::operation_result(env, batch_id, index_u32, false);
                    history.push_back(OperationResult {
                        index: index_u32,
                        ok: false,
                        error_code: code,
                    });

                    if batch.atomic {
                        // rollback by panicking
                        // In atomic mode we simply panic to revert the transaction.
                        // The in-transaction state we may have mutated will be reverted.
                        env.storage()
                            .persistent()
                            .set(&DataKey::Batch(batch_id), &batch);

                        env.storage()
                            .instance()
                            .set(&DataKey::ActiveExecutions, &0u32);
                        panic_with_error(env, BatchErrorCode::ExecutionFailed);
                    } else {
                        batch.failed_at_index = Some(index_u32);
                        // continue execution for partial
                    }
                }
            }

            // Persist intermediate batch status/history so tests can observe.
            batch.history = history.clone();
            env.storage()
                .persistent()
                .set(&DataKey::Batch(batch_id), &batch);
        }

        // Finalize
        batch.history = history;
        batch.finished_at = Some(env.ledger().timestamp());

        if batch.failed_at_index.is_some() {
            batch.status = BatchStatus::Failed;
            events::batch_finished_failed(env, batch_id, batch.failed_at_index.unwrap());
        } else {
            batch.status = BatchStatus::Succeeded;
            events::batch_finished_success(env, batch_id);
        }

        env.storage()
            .instance()
            .set(&DataKey::ActiveExecutions, &0u32);
        env.storage()
            .persistent()
            .set(&DataKey::Batch(batch_id), &batch);
    }
}

fn validate_operation(env: &Env, op: &BatchOperation) {
    match op {
        BatchOperation::QuestCreateBatch(_quest_contract, creator, quests) => {
            // Just ensure non-empty quests.
            if quests.is_empty() {
                panic_with_error(env, BatchErrorCode::BadBatch);
            }
            let _ = creator;
        }
        BatchOperation::QuestClaimReward(_quest_contract, _participant, _quest_id) => {
            // deeper validation occurs during execution by the target contract.
        }
    }
}

// NOTE:
// Soroban cross-contract calls require the correct contract client interface for the target contract.
// The current workspace does not include such generated client bindings in this crate.
//
// To keep this contract compile-ready and to allow unit tests for queue/history/status,
// we stub operation execution for now.
fn execute_operation(
    _env: &Env,
    _op: &BatchOperation,
    _batch_creator: &Address,
) -> Result<(), u32> {
    // Deterministic success stub.
    Ok(())
}

// Storage keys must be Soroban-serializable.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
enum DataKey {
    Admin,
    Paused,
    BatchCounter,
    QueueHead,
    QueueTail,
    ActiveExecutions,
    Batch(u64),
    QueueSlot(u64),
}

fn next_batch_id(env: &Env) -> u64 {
    let current: u64 = env
        .storage()
        .instance()
        .get(&DataKey::BatchCounter)
        .unwrap_or(0);
    let next = current + 1;
    env.storage().instance().set(&DataKey::BatchCounter, &next);
    next
}

fn head_increment(env: &Env, _head: u64) {
    // no-op placeholder
    let _ = env;
}

fn is_paused_internal(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

fn require_not_paused(env: &Env) {
    if is_paused_internal(env) {
        panic_with_error(env, BatchErrorCode::Paused);
    }
}

fn require_admin(env: &Env, caller: &Address) {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .unwrap_or_else(|| panic_with_error(env, BatchErrorCode::NotInitialized));
    if &admin != caller {
        panic_with_error(env, BatchErrorCode::Unauthorized);
    }
    caller.require_auth();
}

fn panic_with_error(env: &Env, err: BatchErrorCode) -> ! {
    env.events().publish(("batch_error",), err as u32);
    panic!("batch_processing error");
}

#[cfg(test)]
mod test;
