use soroban_sdk::{contracttype, Bytes, Env, Map};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractDiff {
    pub changed: Map<Bytes, (Bytes, Bytes)>,
}

pub fn diff_snapshots(
    _env: &Env,
    snapshot1: &super::snapshot::ContractSnapshot,
    snapshot2: &super::snapshot::ContractSnapshot,
) -> ContractDiff {
    let mut changed = Map::new(_env);

    for (key, value1) in snapshot1.state.iter() {
        if let Some(value2) = snapshot2.state.get(key.clone()) {
            if value1 != value2 {
                changed.set(key, (value1, value2));
            }
        }
    }

    ContractDiff { changed }
}