#![no_std]

mod storage;
mod types;

use soroban_sdk::{contract, contractimpl, env, Env, Address, BytesN, Symbol, Vec, IntoVal, Vec as SDKVec};
use storage::{DataKey, set_admin, get_admin, set_paused, is_paused, push_upgrade_history, get_upgrade_history};
use types::UpgradeError;

#[contract]
pub struct ProxyContract;

#[contractimpl]
impl ProxyContract {
    /// Initialize the proxy with an admin address.
    pub fn initialize(env: Env, admin: Address) {
        // Only allow first initialization
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }
        set_admin(&env, &admin);
        set_paused(&env, false);
    }

    /// Upgrade the contract's WASM code. Only admin can call.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin = get_admin(&env);
        admin.require_auth();
        // Record upgrade history before performing upgrade
        push_upgrade_history(&env, &new_wasm_hash);
        // Perform the protocol-level upgrade
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }

    /// Pause contract functionality (emergency stop).
    pub fn pause(env: Env) {
        let admin = get_admin(&env);
        admin.require_auth();
        set_paused(&env, true);
    }

    /// Unpause contract functionality.
    pub fn unpause(env: Env) {
        let admin = get_admin(&env);
        admin.require_auth();
        set_paused(&env, false);
    }

    /// Returns the admin address.
    pub fn admin(env: Env) -> Address {
        get_admin(&env)
    }

    /// Returns whether the contract is paused.
    pub fn paused(env: Env) -> bool {
        is_paused(&env)
    }

    /// Returns the list of previously applied WASM hashes.
    pub fn upgrade_history(env: Env) -> Vec<BytesN<32>> {
        get_upgrade_history(&env)
    }

    /// Fallback called when an undefined method is invoked.
    pub fn fallback(_env: Env) {
        // In Soroban, undefined calls result in a panic.
        panic!("Method not found");
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Env as _};
    use soroban_sdk::Symbol;

    #[test]
    fn test_initialize_and_admin() {
        let env = Env::default();
        let admin = Address::generate(&env);
        ProxyContract::initialize(env.clone(), admin.clone());
        assert_eq!(ProxyContract::admin(env.clone()), admin);
        assert_eq!(ProxyContract::paused(env.clone()), false);
    }

    #[test]
    fn test_pause_unpause() {
        let env = Env::default();
        let admin = Address::generate(&env);
        ProxyContract::initialize(env.clone(), admin.clone());
        ProxyContract::pause(env.clone());
        assert_eq!(ProxyContract::paused(env.clone()), true);
        ProxyContract::unpause(env.clone());
        assert_eq!(ProxyContract::paused(env.clone()), false);
    }

    #[test]
    fn test_upgrade_history_and_upgrade() {
        let env = Env::default();
        let admin = Address::generate(&env);
        ProxyContract::initialize(env.clone(), admin.clone());
        // Simulate an upgrade hash
        let hash1 = BytesN::<32>::from_array(&env, &[0; 32]);
        ProxyContract::upgrade(env.clone(), hash1.clone());
        let history = ProxyContract::upgrade_history(env.clone());
        assert_eq!(history.len(), 1);
        assert_eq!(history.get_unchecked(0), hash1);
    }

    #[test]
    #[should_panic(expected = "Method not found")]
    fn test_fallback() {
        let env = Env::default();
        ProxyContract::fallback(env);
    }
}
