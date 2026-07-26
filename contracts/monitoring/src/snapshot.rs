use soroban_sdk::{contracttype, Bytes, Env, Map, IntoVal, Val, Vec};
use access_control::{AccessControlContractClient, RoleGrant};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractSnapshot {
    pub state: Map<Bytes, Bytes>,
}

pub fn take_snapshot(env: &Env, contract_id: &Bytes) -> ContractSnapshot {
    let mut state = Map::new(env);
    let client = AccessControlContractClient::new(env, contract_id);

    // --- Admin ---
    let admin = client.get_admin();
    state.set(
        Bytes::from_slice(env, b"Admin"),
        admin.into_val(env).to_bytes(),
    );

    // --- Paused ---
    let paused = client.is_paused();
    state.set(
        Bytes::from_slice(env, b"Paused"),
        paused.into_val(env).to_bytes(),
    );

    // --- Role Grants ---
    let role_grants: Vec<(soroban_sdk::Address, Vec<RoleGrant>)> = client.get_all_role_grants();
    for (address, grants) in role_grants.iter() {
        let key = format!("RoleGrants:{}", address);
        state.set(
            Bytes::from_slice(env, key.as_bytes()),
            grants.into_val(env).to_bytes(),
        );
    }

    // --- Role Permissions ---
    let role_permissions: Vec<(soroban_sdk::Symbol, Vec<soroban_sdk::Symbol>)> = client.get_all_role_permissions();
    for (role, perms) in role_permissions.iter() {
        let key = format!("RolePerms:{}", role);
        state.set(
            Bytes::from_slice(env, key.as_bytes()),
            perms.into_val(env).to_bytes(),
        );
    }

    ContractSnapshot { state }
}