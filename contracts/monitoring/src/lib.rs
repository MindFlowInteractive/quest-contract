#![no_std]

mod snapshot;
mod diff;
mod audit;

#[cfg(test)]
mod test;

use soroban_sdk::{contract, contractimpl, contracttype, Bytes, Env, Map, Symbol, Vec};

#[contracttype]
pub enum DataKey {
    NextSnapshotId,
    Snapshots,
    AuditTrail,
}

#[contract]
pub struct MonitoringContract;

#[contractimpl]
impl MonitoringContract {
    pub fn take_snapshot(env: Env, contract_id: Bytes) -> u64 {
        let snapshot = snapshot::take_snapshot(&env, &contract_id);

        let mut next_snapshot_id = env
            .storage()
            .instance()
            .get(&DataKey::NextSnapshotId)
            .unwrap_or(0u64);

        let mut snapshots: Map<u64, snapshot::ContractSnapshot> = env
            .storage()
            .instance()
            .get(&DataKey::Snapshots)
            .unwrap_or_else(|| Map::new(&env));

        snapshots.set(next_snapshot_id, snapshot);
        env.storage().instance().set(&DataKey::Snapshots, &snapshots);

        let mut audit_trail: Vec<audit::AuditEntry> = env
            .storage()
            .instance()
            .get(&DataKey::AuditTrail)
            .unwrap_or_else(|| Vec::new(&env));

        audit_trail.push_back(audit::AuditEntry {
            snapshot_id: next_snapshot_id,
            timestamp: env.ledger().timestamp(),
        });
        env.storage()
            .instance()
            .set(&DataKey::AuditTrail, &audit_trail);

        next_snapshot_id += 1;
        env.storage()
            .instance()
            .set(&DataKey::NextSnapshotId, &next_snapshot_id);

        next_snapshot_id - 1
    }

    pub fn get_snapshot(env: Env, id: u64) -> Option<snapshot::ContractSnapshot> {
        let snapshots: Map<u64, snapshot::ContractSnapshot> = env
            .storage()
            .instance()
            .get(&DataKey::Snapshots)
            .unwrap_or_else(|| Map::new(&env));
        snapshots.get(id)
    }

    pub fn get_audit_trail(env: Env) -> Vec<audit::AuditEntry> {
        env.storage()
            .instance()
            .get(&DataKey::AuditTrail)
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn diff_snapshots(
        env: Env,
        snapshot1: snapshot::ContractSnapshot,
        snapshot2: snapshot::ContractSnapshot,
    ) -> diff::ContractDiff {
        diff::diff_snapshots(&env, &snapshot1, &snapshot2)
    }
}