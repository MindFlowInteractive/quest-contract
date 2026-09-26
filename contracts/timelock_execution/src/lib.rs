use soroban_sdk::{contractimpl, Address, Env, Symbol};

#[derive(Clone)]
pub struct QueuedTx {
    pub id: u64,
    pub target: Address,
    pub queued_at: u64,
    pub eta: u64,
    pub cancelled: bool,
    pub executed: bool,
}

pub struct TimelockExecutionContract;

const MIN_DELAY: u64 = 172_800; // 2 days
const GRACE_PERIOD: u64 = 1_209_600; // 14 days

#[contractimpl]
impl TimelockExecutionContract {
    pub fn queue(env: Env, admin: Address, target: Address, delay: u64) -> u64 {
        admin.require_auth();
        assert!(delay >= MIN_DELAY, "delay below minimum");
        let now = env.ledger().timestamp();
        let id: u64 = env.storage().instance().get(&Symbol::new(&env, "next_id")).unwrap_or(0);
        env.storage().instance().set(&Symbol::new(&env, "next_id"), &(id + 1));

        env.storage().persistent().set(&id, &QueuedTx {
            id,
            target,
            queued_at: now,
            eta: now + delay,
            cancelled: false,
            executed: false,
        });
        id
    }

    pub fn execute(env: Env, admin: Address, tx_id: u64) {
        admin.require_auth();
        let mut tx: QueuedTx = env.storage().persistent().get(&tx_id).unwrap();
        let now = env.ledger().timestamp();
        assert!(!tx.cancelled, "transaction cancelled");
        assert!(now >= tx.eta, "still time-locked");
        assert!(now <= tx.eta + GRACE_PERIOD, "grace period expired");
        tx.executed = true;
        env.storage().persistent().set(&tx_id, &tx);
    }

    pub fn cancel(env: Env, admin: Address, tx_id: u64) {
        admin.require_auth();
        let mut tx: QueuedTx = env.storage().persistent().get(&tx_id).unwrap();
        tx.cancelled = true;
        env.storage().persistent().set(&tx_id, &tx);
    }
}
