#![cfg(test)]

//! Integration tests: an independent contract depends on the circuit breaker
//! module and guards its state-changing entry point with `enforce_not_paused`.

use circuit_breaker::{CircuitBreaker, CircuitBreakerClient};
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short,
    testutils::{Address as _, Ledger},
    Address, Env, String, Vec,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
enum VaultKey {
    Breaker,
    Balance(Address),
}

#[contract]
pub struct GuardedVault;

#[contractimpl]
impl GuardedVault {
    pub fn initialize(env: Env, breaker: Address) {
        env.storage().instance().set(&VaultKey::Breaker, &breaker);
    }

    pub fn deposit(env: Env, user: Address, amount: i128) {
        user.require_auth();
        let breaker: Address = env.storage().instance().get(&VaultKey::Breaker).unwrap();
        // This is the reusable guard exported by the circuit-breaker module.
        circuit_breaker::enforce_not_paused(&env, &breaker, &symbol_short!("deposit"));

        let key = VaultKey::Balance(user);
        let current: i128 = env.storage().instance().get(&key).unwrap_or(0);
        env.storage().instance().set(&key, &(current + amount));
    }

    pub fn balance(env: Env, user: Address) -> i128 {
        let key = VaultKey::Balance(user);
        env.storage().instance().get(&key).unwrap_or(0)
    }
}

struct Integration {
    env: Env,
    breaker: CircuitBreakerClient<'static>,
    vault: GuardedVaultClient<'static>,
    admin: Address,
    g2: Address,
    user: Address,
}

impl Integration {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        let mut ledger = env.ledger().get();
        ledger.timestamp = 1_000;
        env.ledger().set(ledger);

        let admin = Address::generate(&env);
        let g2 = Address::generate(&env);
        let g3 = Address::generate(&env);
        let mut guardians = Vec::new(&env);
        guardians.push_back(admin.clone());
        guardians.push_back(g2.clone());
        guardians.push_back(g3.clone());

        let breaker_id = env.register_contract(None, CircuitBreaker);
        let breaker = CircuitBreakerClient::new(&env, &breaker_id);
        breaker.initialize(&admin, &guardians, &2, &7_200, &3_600);

        let vault_id = env.register_contract(None, GuardedVault);
        let vault = GuardedVaultClient::new(&env, &vault_id);
        vault.initialize(&breaker_id);

        Self {
            env,
            breaker,
            vault,
            admin,
            g2,
            user: Address::generate(&env),
        }
    }

    fn trip(&self) {
        let id = self
            .breaker
            .propose_pause(&self.admin, &String::from_str(&self.env, "exploit"));
        self.breaker.approve_pause(&self.g2, &id);
    }

    fn unpause(&self) {
        self.breaker
            .request_unpause(&self.admin, &String::from_str(&self.env, "resolved"));
        let mut ledger = self.env.ledger().get();
        ledger.timestamp = 1_000 + 7_200;
        self.env.ledger().set(ledger);
        self.breaker.execute_unpause(&self.admin);
    }
}

#[test]
fn integrating_contract_rejects_state_changes_while_paused() {
    let t = Integration::new();

    // Normal operation before the breaker trips.
    t.vault.deposit(&t.user, &100);
    assert_eq!(t.vault.balance(&t.user), 100);

    t.trip();
    assert!(t.breaker.is_paused());

    // State-changing calls are rejected by the shared guard.
    assert!(t.vault.try_deposit(&t.user, &50).is_err());
    assert_eq!(t.vault.balance(&t.user), 100);

    // Read-only entry points of the integrating contract remain available.
    assert_eq!(t.vault.balance(&t.user), 100);

    t.unpause();
    assert!(!t.breaker.is_paused());
    t.vault.deposit(&t.user, &50);
    assert_eq!(t.vault.balance(&t.user), 150);
}

#[test]
fn allowlisted_function_can_run_during_a_pause() {
    let t = Integration::new();
    t.trip();

    assert!(t.vault.try_deposit(&t.user, &10).is_err());
    t.breaker
        .allow_function(&t.admin, &symbol_short!("deposit"));
    t.vault.deposit(&t.user, &10);
    assert_eq!(t.vault.balance(&t.user), 10);

    t.breaker
        .disallow_function(&t.admin, &symbol_short!("deposit"));
    assert!(t.vault.try_deposit(&t.user, &10).is_err());
    assert_eq!(t.vault.balance(&t.user), 10);
}
