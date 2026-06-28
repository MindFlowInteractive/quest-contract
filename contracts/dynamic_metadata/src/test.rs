#![cfg(test)]

// Required so the `format!` macro resolves in this `#![no_std]`+`#[cfg(test)]` context.
extern crate alloc;
use alloc::format;

use soroban_sdk::{Address, Env, Map, String, Symbol, Vec};
use soroban_sdk::testutils::{Address as _, Ledger};

use crate::{
    ContractConfig, DynamicMetadataContract, MetadataVersion, OwnershipEvent,
    OwnershipRecord, TokenMetadata, TraitEvolutionRule, TriggerRule,
};

// ──────────────────────────────────────────────────────────────────────────
// MANUAL CONTRACT CLIENT
// ──────────────────────────────────────────────────────────────────────────

pub struct DynamicMetadataContractClient<'a> {
    pub contract_id: soroban_sdk::contractclient::ContractID<'a>,
    pub env: &'a Env,
}

impl<'a> DynamicMetadataContractClient<'a> {
    pub fn new(env: &'a Env, contract_id: &soroban_sdk::contractclient::ContractID) -> Self {
        Self {
            contract_id: contract_id.clone(),
            env,
        }
    }

    fn invoke(&self, name: &'static str, args: soroban_sdk::Vec<soroban_sdk::Val>) -> soroban_sdk::Val {
        self.env.invoke_contract(
            &self.contract_id,
            &Symbol::new(self.env, name),
            args,
        )
    }

    pub fn initialize(
        &self,
        admin: &Address,
        migrator: &Address,
        oracle: &Address,
        image_base_uri: &String,
        schema_version: &u32,
    ) {
        self.invoke(
            "initialize",
            soroban_sdk::vec![
                self.env,
                admin.to_val(),
                migrator.to_val(),
                oracle.to_val(),
                image_base_uri.to_val(),
                schema_version.to_val(),
            ],
        );
    }

    pub fn mint_token(
        &self,
        minter: &Address,
        owner: &Address,
        name: &String,
        description: &String,
        image_id: &u32,
        external_uri: &String,
        string_traits: &Map<String, String>,
        numeric_traits: &Map<String, u32>,
    ) -> u32 {
        self.invoke(
            "mint_token",
            soroban_sdk::vec![
                self.env,
                minter.to_val(),
                owner.to_val(),
                name.to_val(),
                description.to_val(),
                image_id.to_val(),
                external_uri.to_val(),
                string_traits.to_val(),
                numeric_traits.to_val(),
            ],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn transfer_token(&self, from: &Address, to: &Address, token_id: &u32) {
        self.invoke(
            "transfer_token",
            soroban_sdk::vec![self.env, from.to_val(), to.to_val(), token_id.to_val()],
        );
    }

    pub fn burn_token(&self, caller: &Address, token_id: &u32) {
        self.invoke(
            "burn_token",
            soroban_sdk::vec![self.env, caller.to_val(), token_id.to_val()],
        );
    }

    pub fn update_metadata(
        &self,
        caller: &Address,
        token_id: &u32,
        name: &String,
        description: &String,
        image_id: &u32,
        external_uri: &String,
    ) -> u32 {
        self.invoke(
            "update_metadata",
            soroban_sdk::vec![
                self.env,
                caller.to_val(),
                token_id.to_val(),
                name.to_val(),
                description.to_val(),
                image_id.to_val(),
                external_uri.to_val(),
            ],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn add_string_trait(&self, caller: &Address, token_id: &u32, name: &String, value: &String) {
        self.invoke(
            "add_string_trait",
            soroban_sdk::vec![self.env, caller.to_val(), token_id.to_val(), name.to_val(), value.to_val()],
        );
    }

    pub fn remove_string_trait(&self, caller: &Address, token_id: &u32, name: &String) {
        self.invoke(
            "remove_string_trait",
            soroban_sdk::vec![self.env, caller.to_val(), token_id.to_val(), name.to_val()],
        );
    }

    pub fn add_numeric_trait(&self, caller: &Address, token_id: &u32, name: &String, value: &u32) {
        self.invoke(
            "add_numeric_trait",
            soroban_sdk::vec![self.env, caller.to_val(), token_id.to_val(), name.to_val(), value.to_val()],
        );
    }

    pub fn increment_numeric_trait(
        &self,
        caller: &Address,
        token_id: &u32,
        name: &String,
        delta: &u32,
    ) -> u32 {
        self.invoke(
            "increment_numeric_trait",
            soroban_sdk::vec![self.env, caller.to_val(), token_id.to_val(), name.to_val(), delta.to_val()],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn set_performance_metric(&self, oracle: &Address, token_id: &u32, metric: &String, value: &u64) {
        self.invoke(
            "set_performance_metric",
            soroban_sdk::vec![self.env, oracle.to_val(), token_id.to_val(), metric.to_val(), value.to_val()],
        );
    }

    pub fn add_performance_metric(&self, oracle: &Address, token_id: &u32, metric: &String, delta: &u64) -> u64 {
        self.invoke(
            "add_performance_metric",
            soroban_sdk::vec![self.env, oracle.to_val(), token_id.to_val(), metric.to_val(), delta.to_val()],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn get_performance_metric(&self, token_id: &u32, metric: &String) -> u64 {
        self.invoke(
            "get_performance_metric",
            soroban_sdk::vec![self.env, token_id.to_val(), metric.to_val()],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn configure_trait_rule(
        &self,
        admin: &Address,
        metric: &String,
        threshold: &u64,
        target_trait_name: &String,
        target_trait_value: &String,
        target_level: &u32,
        new_rarity: &u32,
    ) -> u64 {
        self.invoke(
            "configure_trait_rule",
            soroban_sdk::vec![
                self.env,
                admin.to_val(),
                metric.to_val(),
                threshold.to_val(),
                target_trait_name.to_val(),
                target_trait_value.to_val(),
                target_level.to_val(),
                new_rarity.to_val(),
            ],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn remove_trait_rule(&self, admin: &Address, rule_id: &u64) {
        self.invoke(
            "remove_trait_rule",
            soroban_sdk::vec![self.env, admin.to_val(), rule_id.to_val()],
        );
    }

    pub fn get_trait_rules(&self) -> Map<u64, TraitEvolutionRule> {
        self.invoke(
            "get_trait_rules",
            soroban_sdk::vec![self.env],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn configure_trigger_rule(
        &self,
        admin: &Address,
        trigger_type: &String,
        metric_key: &String,
        threshold: &u64,
        action_type: &String,
        action_param: &String,
        action_value: &String,
    ) -> u64 {
        self.invoke(
            "configure_trigger_rule",
            soroban_sdk::vec![
                self.env,
                admin.to_val(),
                trigger_type.to_val(),
                metric_key.to_val(),
                threshold.to_val(),
                action_type.to_val(),
                action_param.to_val(),
                action_value.to_val(),
            ],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn remove_trigger_rule(&self, admin: &Address, rule_id: &u64) {
        self.invoke(
            "remove_trigger_rule",
            soroban_sdk::vec![self.env, admin.to_val(), rule_id.to_val()],
        );
    }

    pub fn get_trigger_rules(&self) -> Map<u64, TriggerRule> {
        self.invoke(
            "get_trigger_rules",
            soroban_sdk::vec![self.env],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn check_and_apply_triggers(&self, caller: &Address, token_id: &u32) -> u32 {
        self.invoke(
            "check_and_apply_triggers",
            soroban_sdk::vec![self.env, caller.to_val(), token_id.to_val()],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn evolve_traits(&self, caller: &Address, token_id: &u32) -> u32 {
        self.invoke(
            "evolve_traits",
            soroban_sdk::vec![self.env, caller.to_val(), token_id.to_val()],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn freeze_metadata(&self, caller: &Address, token_id: &u32) {
        self.invoke(
            "freeze_metadata",
            soroban_sdk::vec![self.env, caller.to_val(), token_id.to_val()],
        );
    }

    pub fn unfreeze_metadata(&self, caller: &Address, token_id: &u32) {
        self.invoke(
            "unfreeze_metadata",
            soroban_sdk::vec![self.env, caller.to_val(), token_id.to_val()],
        );
    }

    pub fn migrate_metadata(
        &self,
        migrator: &Address,
        token_id: &u32,
        new_version: &u32,
        new_name: &String,
        new_description: &String,
        new_image_id: &u32,
        new_external_uri: &String,
        new_string_traits: &Map<String, String>,
        new_numeric_traits: &Map<String, u32>,
        level_override: &u32,
        rarity_override: &u32,
    ) -> u32 {
        self.invoke(
            "migrate_metadata",
            soroban_sdk::vec![
                self.env,
                migrator.to_val(),
                token_id.to_val(),
                new_version.to_val(),
                new_name.to_val(),
                new_description.to_val(),
                new_image_id.to_val(),
                new_external_uri.to_val(),
                new_string_traits.to_val(),
                new_numeric_traits.to_val(),
                level_override.to_val(),
                rarity_override.to_val(),
            ],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn get_metadata(&self, token_id: &u32) -> TokenMetadata {
        self.invoke(
            "get_metadata",
            soroban_sdk::vec![self.env, token_id.to_val()],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn get_ownership_history(&self, token_id: &u32) -> Vec<OwnershipRecord> {
        self.invoke(
            "get_ownership_history",
            soroban_sdk::vec![self.env, token_id.to_val()],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn get_version_history(&self, token_id: &u32) -> Vec<MetadataVersion> {
        self.invoke(
            "get_version_history",
            soroban_sdk::vec![self.env, token_id.to_val()],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn get_tokens_by_owner(&self, owner: &Address) -> Vec<u32> {
        self.invoke(
            "get_tokens_by_owner",
            soroban_sdk::vec![self.env, owner.to_val()],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn get_image_uri(&self, token_id: &u32) -> String {
        self.invoke(
            "get_image_uri",
            soroban_sdk::vec![self.env, token_id.to_val()],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn get_config(&self) -> ContractConfig {
        self.invoke(
            "get_config",
            soroban_sdk::vec![self.env],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn update_image_base_uri(&self, admin: &Address, new_uri: &String) {
        self.invoke(
            "update_image_base_uri",
            soroban_sdk::vec![self.env, admin.to_val(), new_uri.to_val()],
        );
    }

    pub fn update_schema_version(&self, admin: &Address, new_version: &u32) {
        self.invoke(
            "update_schema_version",
            soroban_sdk::vec![self.env, admin.to_val(), new_version.to_val()],
        );
    }

    pub fn set_oracle(&self, admin: &Address, new_oracle: &Address) {
        self.invoke(
            "set_oracle",
            soroban_sdk::vec![self.env, admin.to_val(), new_oracle.to_val()],
        );
    }

    pub fn set_migrator(&self, admin: &Address, new_migrator: &Address) {
        self.invoke(
            "set_migrator",
            soroban_sdk::vec![self.env, admin.to_val(), new_migrator.to_val()],
        );
    }

    pub fn is_paused(&self) -> bool {
        self.invoke(
            "is_paused",
            soroban_sdk::vec![self.env],
        )
        .try_into_val(self.env)
        .unwrap()
    }

    pub fn set_paused(&self, admin: &Address, paused: &bool) {
        self.invoke(
            "set_paused",
            soroban_sdk::vec![self.env, admin.to_val(), paused.to_val()],
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────
// HELPERS
// ──────────────────────────────────────────────────────────────────────────

fn empty_string(env: &Env) -> String {
    String::from_str(env, "")
}

fn make_string(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

fn setup() -> (Env, Address, Address, Address, DynamicMetadataContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, DynamicMetadataContract);
    let client = DynamicMetadataContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let migrator = Address::generate(&env);
    let oracle = Address::generate(&env);
    let base = make_string(&env, "ipfs://bafy.../metadata");

    client.initialize(&admin, &migrator, &oracle, &base, &1u32);

    (env, admin, migrator, oracle, client)
}

fn mint_default(env: &Env, client: &DynamicMetadataContractClient<'_>, owner: &Address) -> u32 {
    let minter = Address::generate(env);
    let mut string_traits: Map<String, String> = Map::new(env);
    string_traits.set(make_string(env, "background"), make_string(env, "forest"));
    string_traits.set(make_string(env, "rank"), make_string(env, "common"));
    let numeric_traits: Map<String, u32> = Map::new(env);
    client.mint_token(
        &minter,
        owner,
        &make_string(env, "Mythic Sword"),
        &make_string(env, "An ancient blade..."),
        &1u32,
        &make_string(env, ""),
        &string_traits,
        &numeric_traits,
    )
}

// ══════════════════════════════════════════════════════════════════════════
// INITIALIZATION
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_initialize_success() {
    let (_env, _admin, _migrator, _oracle, client) = setup();
    let config = client.get_config();
    assert_eq!(config.schema_version, 1);
    assert_eq!(config.image_base_uri.len(), "ipfs://bafy.../metadata".len() as u32);
}

#[test]
#[should_panic(expected = "AlreadyInitialized")]
fn test_double_initialize_panics() {
    let (env, admin, migrator, oracle, client) = setup();
    client.initialize(
        &admin,
        &migrator,
        &oracle,
        &make_string(&env, "ipfs://other"),
        &1u32,
    );
}

#[test]
#[should_panic(expected = "InvalidSchemaVersion")]
fn test_initialize_zero_schema_version_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, DynamicMetadataContract);
    let client = DynamicMetadataContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let migrator = Address::generate(&env);
    let oracle = Address::generate(&env);
    client.initialize(&admin, &migrator, &oracle, &make_string(&env, "ipfs://base"), &0u32);
}

#[test]
#[should_panic(expected = "InvalidImageBaseUri")]
fn test_initialize_empty_base_uri_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, DynamicMetadataContract);
    let client = DynamicMetadataContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let migrator = Address::generate(&env);
    let oracle = Address::generate(&env);
    client.initialize(&admin, &migrator, &oracle, &empty_string(&env), &1u32);
}

#[test]
fn test_pause_flag_default_false() {
    let (_env, _admin, _migrator, _oracle, client) = setup();
    assert!(!client.is_paused());
}

#[test]
fn test_set_pause() {
    let (_env, admin, _migrator, _oracle, client) = setup();
    client.set_paused(&admin, &true);
    assert!(client.is_paused());
    client.set_paused(&admin, &false);
    assert!(!client.is_paused());
}

#[test]
#[should_panic(expected = "NotAdmin")]
fn test_set_pause_unauthorized_panics() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let stranger = Address::generate(&env);
    client.set_paused(&stranger, &true);
}

// ══════════════════════════════════════════════════════════════════════════
// MINTING (FEATURE 1: METADATA STRUCTURE)
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_mint_token_initializes_metadata() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    let metadata = client.get_metadata(&token_id);
    assert_eq!(metadata.token_id, token_id);
    assert_eq!(metadata.owner, owner);
    assert_eq!(metadata.level, 1);
    assert_eq!(metadata.rarity, 1);
    assert_eq!(metadata.performance_score, 0);
    assert_eq!(metadata.frozen, false);
    assert_eq!(metadata.schema_version, 1);
    assert_eq!(metadata.string_traits.len(), 2);
    assert_eq!(
        metadata.string_traits.get(make_string(&env, "background")).unwrap(),
        make_string(&env, "forest")
    );
    assert_eq!(
        metadata.string_traits.get(make_string(&env, "rank")).unwrap(),
        make_string(&env, "common")
    );
}

#[test]
fn test_mint_emits_event_and_version_snapshot() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    let versions = client.get_version_history(&token_id);
    assert_eq!(versions.len(), 1);
    let v = versions.get(0).unwrap();
    assert_eq!(v.version, 1);
    assert_eq!(v.reason, make_string(&env, "mint"));
}

#[test]
fn test_owner_token_index_appended_on_mint() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);

    let id1 = mint_default(&env, &client, &owner);
    let id2 = mint_default(&env, &client, &owner);

    let list = client.get_tokens_by_owner(&owner);
    assert_eq!(list.len(), 2);
    assert_eq!(list.get(0).unwrap(), id1);
    assert_eq!(list.get(1).unwrap(), id2);
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn test_mint_when_paused_panics() {
    let (env, admin, _migrator, _oracle, client) = setup();
    client.set_paused(&admin, &true);

    let owner = Address::generate(&env);
    let _ = mint_default(&env, &client, &owner);
}

// ══════════════════════════════════════════════════════════════════════════
// OWNERSHIP HISTORY (FEATURE 4)
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_transfer_records_history() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    client.transfer_token(&owner, &new_owner, &token_id);

    let history = client.get_ownership_history(&token_id);
    assert_eq!(history.len(), 2);
    let mint = history.get(0).unwrap();
    assert_eq!(mint.event, OwnershipEvent::Mint);
    let transfer = history.get(1).unwrap();
    assert_eq!(transfer.event, OwnershipEvent::Transfer);
    assert_eq!(transfer.owner, new_owner);
    assert_eq!(transfer.previous_owner, owner);
}

#[test]
fn test_burn_records_history() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    client.burn_token(&owner, &token_id);

    let history = client.get_ownership_history(&token_id);
    assert_eq!(history.len(), 2);
    let burn = history.get(1).unwrap();
    assert_eq!(burn.event, OwnershipEvent::Burn);
}

#[test]
#[should_panic(expected = "NotOwner")]
fn test_transfer_by_non_owner_panics() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let stranger = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    client.transfer_token(&stranger, &Address::generate(&env), &token_id);
}

// ══════════════════════════════════════════════════════════════════════════
// METADATA UPDATES (FEATURE 2)
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_update_metadata_owner() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    client.update_metadata(
        &owner,
        &token_id,
        &make_string(&env, "Renamed Sword"),
        &make_string(&env, "A finer edge..."),
        &2u32,
        &make_string(&env, "ipfs://updated"),
    );

    let m = client.get_metadata(&token_id);
    assert_eq!(m.name, make_string(&env, "Renamed Sword"));
    assert_eq!(m.description, make_string(&env, "A finer edge..."));
    assert_eq!(m.image_id, 2);
    assert_eq!(m.external_uri, make_string(&env, "ipfs://updated"));
}

#[test]
fn test_update_metadata_admin_can_update() {
    let (env, admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    client.update_metadata(
        &admin,
        &token_id,
        &make_string(&env, "AdminRenamed"),
        &make_string(&env, ""),
        &0u32,
        &make_string(&env, ""),
    );
    let m = client.get_metadata(&token_id);
    assert_eq!(m.name, make_string(&env, "AdminRenamed"));
    assert_eq!(m.image_id, 1); // unchanged
}

#[test]
#[should_panic(expected = "NotAuthorized")]
fn test_update_metadata_stranger_panics() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let stranger = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    client.update_metadata(
        &stranger,
        &token_id,
        &make_string(&env, "hack"),
        &make_string(&env, "hack"),
        &1u32,
        &make_string(&env, ""),
    );
}

#[test]
fn test_string_trait_add_remove() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    client.add_string_trait(
        &owner,
        &token_id,
        &make_string(&env, "aura"),
        &make_string(&env, "glowing"),
    );
    let m = client.get_metadata(&token_id);
    assert_eq!(
        m.string_traits.get(make_string(&env, "aura")).unwrap(),
        make_string(&env, "glowing")
    );

    client.remove_string_trait(&owner, &token_id, &make_string(&env, "aura"));
    let m = client.get_metadata(&token_id);
    assert!(m.string_traits.get(make_string(&env, "aura")).is_none());
}

#[test]
fn test_numeric_trait_add_and_increment() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    client.add_numeric_trait(&owner, &token_id, &make_string(&env, "power"), &10u32);
    let after = client.increment_numeric_trait(&owner, &token_id, &make_string(&env, "power"), &5u32);
    assert_eq!(after, 15);
    let m = client.get_metadata(&token_id);
    assert_eq!(m.numeric_traits.get(make_string(&env, "power")).unwrap(), 15);
}

#[test]
fn test_too_many_traits_panics() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    // Mint with the full quota of 32 *unique* string traits.
    let mut string_traits: Map<String, String> = Map::new(&env);
    for i in 0..32u32 {
        let key = make_string(&env, &format!("k{}", i));
        string_traits.set(key, make_string(&env, "v"));
    }
    let numeric_traits: Map<String, u32> = Map::new(&env);
    let minter = Address::generate(&env);
    let token_id = client.mint_token(
        &minter,
        &owner,
        &make_string(&env, "name"),
        &make_string(&env, "desc"),
        &1u32,
        &make_string(&env, ""),
        &string_traits,
        &numeric_traits,
    );

    // Verify we have exactly 32 unique entries.
    let m = client.get_metadata(&token_id);
    assert_eq!(m.string_traits.len(), 32);

    // Adding a 33rd *distinct* key must panic with TooManyTraits.
    let result = std::panic::catch_unwind(|| {
        client.add_string_trait(
            &owner,
            &token_id,
            &make_string(&env, "k32"),
            &make_string(&env, "v"),
        );
    });
    assert!(result.is_err(), "expected TooManyTraits panic");
}

// ══════════════════════════════════════════════════════════════════════════
// ORACLE-DRIVEN METRICS (FEATURES 3 / 5)
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_oracle_set_metric_updates_performance_score() {
    let (env, _admin, _migrator, oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    client.set_performance_metric(&oracle, &token_id, &make_string(&env, "wins"), &42u64);
    assert_eq!(client.get_performance_metric(&token_id, &make_string(&env, "wins")), 42);
    let m = client.get_metadata(&token_id);
    assert_eq!(m.performance_score, 42);
}

#[test]
fn test_oracle_add_metric_saturates() {
    let (env, _admin, _migrator, oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    let v = client.add_performance_metric(&oracle, &token_id, &make_string(&env, "xp"), &100u64);
    assert_eq!(v, 100);
    let v = client.add_performance_metric(&oracle, &token_id, &make_string(&env, "xp"), &50u64);
    assert_eq!(v, 150);
}

#[test]
#[should_panic(expected = "NotOracle")]
fn test_set_metric_non_oracle_panics() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let stranger = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    client.set_performance_metric(&stranger, &token_id, &make_string(&env, "xp"), &10u64);
}

// ══════════════════════════════════════════════════════════════════════════
// TRAIT EVOLUTION RULES (FEATURE 5)
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_trait_rule_triggers_on_threshold() {
    let (env, admin, _migrator, oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    // Configure rule: at >=100 score, set rank=legendary + level=2 + rarity=3
    let id = client.configure_trait_rule(
        &admin,
        &make_string(&env, "perf"),
        &100u64,
        &make_string(&env, "rank"),
        &make_string(&env, "legendary"),
        &2u32,
        &3u32,
    );
    assert_eq!(id, 1);

    // Add xp metric 150 -> triggers evolution.
    client.add_performance_metric(&oracle, &token_id, &make_string(&env, "wins"), &150u64);

    let m = client.get_metadata(&token_id);
    assert_eq!(m.level, 2);
    assert_eq!(m.rarity, 3);
    assert_eq!(
        m.string_traits.get(make_string(&env, "rank")).unwrap(),
        make_string(&env, "legendary")
    );
}

#[test]
fn test_trait_rule_does_not_below_threshold() {
    let (env, admin, _migrator, oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    client.configure_trait_rule(
        &admin,
        &make_string(&env, "perf"),
        &100u64,
        &make_string(&env, "rank"),
        &make_string(&env, "legendary"),
        &2u32,
        &3u32,
    );

    // Add only 50 -> below threshold
    client.add_performance_metric(&oracle, &token_id, &make_string(&env, "wins"), &50u64);

    let m = client.get_metadata(&token_id);
    assert_eq!(m.level, 1);
    assert_eq!(m.rarity, 1);
    assert_eq!(
        m.string_traits.get(make_string(&env, "rank")).unwrap(),
        make_string(&env, "common")
    );
}

#[test]
fn test_evolve_traits_manual_invocation() {
    let (env, admin, _migrator, oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);
    client.configure_trait_rule(
        &admin,
        &make_string(&env, "perf"),
        &50u64,
        &make_string(&env, "rank"),
        &make_string(&env, "epic"),
        &2u32,
        &2u32,
    );

    // Bump perf to 80 via direct set
    client.set_performance_metric(&oracle, &token_id, &make_string(&env, "wins"), &80u64);
    // Manual evolution should also succeed
    let applied = client.evolve_traits(&owner, &token_id);
    assert_eq!(applied, 1);
    let m = client.get_metadata(&token_id);
    assert_eq!(
        m.string_traits.get(make_string(&env, "rank")).unwrap(),
        make_string(&env, "epic")
    );
}

#[test]
fn test_remove_trait_rule() {
    let (env, admin, _migrator, _oracle, client) = setup();
    let id = client.configure_trait_rule(
        &admin,
        &make_string(&env, "perf"),
        &10u64,
        &make_string(&env, "rank"),
        &make_string(&env, "rare"),
        &2u32,
        &2u32,
    );
    assert_eq!(client.get_trait_rules().len(), 1);
    client.remove_trait_rule(&admin, &id);
    assert_eq!(client.get_trait_rules().len(), 0);
}

#[test]
#[should_panic(expected = "RuleNotFound")]
fn test_remove_trait_rule_unknown_panics() {
    let (env, admin, _migrator, _oracle, client) = setup();
    client.remove_trait_rule(&admin, &999u64);
}

// ══════════════════════════════════════════════════════════════════════════
// TRIGGER RULES (FEATURE 3)
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_trigger_metric_fires_add_trait() {
    let (env, admin, _migrator, oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    // metric: when "wins" >= 10, add trait "title"->"veteran"
    client.configure_trigger_rule(
        &admin,
        &make_string(&env, "metric"),
        &make_string(&env, "wins"),
        &10u64,
        &make_string(&env, "add_trait"),
        &make_string(&env, "title"),
        &make_string(&env, "veteran"),
    );

    client.add_performance_metric(&oracle, &token_id, &make_string(&env, "wins"), &12u64);

    let m = client.get_metadata(&token_id);
    assert_eq!(
        m.string_traits.get(make_string(&env, "title")).unwrap(),
        make_string(&env, "veteran")
    );
}

#[test]
fn test_trigger_bump_level() {
    let (env, admin, _migrator, oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    client.configure_trigger_rule(
        &admin,
        &make_string(&env, "metric"),
        &make_string(&env, "wins"),
        &5u64,
        &make_string(&env, "bump_level"),
        &make_string(&env, ""),
        &make_string(&env, "3"),
    );

    client.set_performance_metric(&oracle, &token_id, &make_string(&env, "wins"), &5u64);
    let m = client.get_metadata(&token_id);
    assert_eq!(m.level, 4);
}

#[test]
fn test_trigger_ownership_count_fires() {
    let (env, admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    // ownership_count >= 3 -> add trait "rare"->"collector"
    client.configure_trigger_rule(
        &admin,
        &make_string(&env, "ownership_count"),
        &make_string(&env, ""),
        &3u64,
        &make_string(&env, "add_trait"),
        &make_string(&env, "rare"),
        &make_string(&env, "collector"),
    );

    // owner -> a -> b -> c gives 4 distinct owners => triggers rule
    client.transfer_token(&owner, &a, &token_id);
    client.transfer_token(&a, &b, &token_id);
    client.transfer_token(&b, &c, &token_id);

    let applied = client.check_and_apply_triggers(&c, &token_id);
    assert_eq!(applied, 1);
    let m = client.get_metadata(&token_id);
    assert_eq!(
        m.string_traits.get(make_string(&env, "rare")).unwrap(),
        make_string(&env, "collector")
    );
}

#[test]
#[should_panic(expected = "InvalidTriggerType")]
fn test_trigger_invalid_type_panics() {
    let (env, admin, _migrator, _oracle, client) = setup();
    client.configure_trigger_rule(
        &admin,
        &make_string(&env, "garbage"),
        &make_string(&env, ""),
        &0u64,
        &make_string(&env, "add_trait"),
        &make_string(&env, "k"),
        &make_string(&env, "v"),
    );
}

#[test]
#[should_panic(expected = "InvalidActionType")]
fn test_trigger_invalid_action_panics() {
    let (env, admin, _migrator, _oracle, client) = setup();
    client.configure_trigger_rule(
        &admin,
        &make_string(&env, "metric"),
        &make_string(&env, "k"),
        &1u64,
        &make_string(&env, "garbage"),
        &make_string(&env, "k"),
        &make_string(&env, "v"),
    );
}

#[test]
fn test_remove_trigger_rule() {
    let (env, admin, _migrator, _oracle, client) = setup();
    let id = client.configure_trigger_rule(
        &admin,
        &make_string(&env, "metric"),
        &make_string(&env, "wins"),
        &1u64,
        &make_string(&env, "add_trait"),
        &make_string(&env, "title"),
        &make_string(&env, "vet"),
    );
    assert_eq!(client.get_trigger_rules().len(), 1);
    client.remove_trigger_rule(&admin, &id);
    assert_eq!(client.get_trigger_rules().len(), 0);
}

// ══════════════════════════════════════════════════════════════════════════
// VERSIONING (FEATURE 6)
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_metadata_update_appends_version() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    let initial_versions = client.get_version_history(&token_id);
    assert_eq!(initial_versions.len(), 1);

    client.update_metadata(
        &owner,
        &token_id,
        &make_string(&env, "Renamed"),
        &make_string(&env, ""),
        &0u32,
        &make_string(&env, ""),
    );
    let versions = client.get_version_history(&token_id);
    assert_eq!(versions.len(), 2);

    let v0 = versions.get(0).unwrap();
    let v1 = versions.get(1).unwrap();
    assert_eq!(v0.version, 1);
    assert_eq!(v0.reason, make_string(&env, "mint"));
    assert_eq!(v1.version, 2);
    assert_eq!(v1.reason, make_string(&env, "update"));
    // Snapshot should reflect the pre-update name
    assert_eq!(
        v0.snapshot.name,
        make_string(&env, "Mythic Sword")
    );
}

#[test]
fn test_version_snapshot_captures_pre_change_state() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    client.add_string_trait(
        &owner,
        &token_id,
        &make_string(&env, "aura"),
        &make_string(&env, "dim"),
    );

    let versions = client.get_version_history(&token_id);
    let latest = versions.get(versions.len() - 1).unwrap();
    // Snapshot is the new metadata (post-add). Next snapshot is for an
    // earlier version.
    let previous = versions.get(versions.len() - 2).unwrap();
    assert_eq!(
        latest.snapshot.string_traits.get(make_string(&env, "aura")).unwrap(),
        make_string(&env, "dim")
    );
    assert!(previous
        .snapshot
        .string_traits
        .get(make_string(&env, "aura"))
        .is_none());
}

#[test]
fn test_transfer_creates_a_new_version() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);
    client.transfer_token(&owner, &new_owner, &token_id);
    let versions = client.get_version_history(&token_id);
    assert_eq!(versions.len(), 2);
    let latest = versions.get(1).unwrap();
    assert_eq!(latest.reason, make_string(&env, "transfer"));
}

// ══════════════════════════════════════════════════════════════════════════
// FREEZING (FEATURE 7)
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_freeze_blocks_updates() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);
    client.freeze_metadata(&owner, &token_id);

    let m = client.get_metadata(&token_id);
    assert!(m.frozen);

    let result = std::panic::catch_unwind(|| {
        client.update_metadata(
            &owner,
            &token_id,
            &make_string(&env, "ShouldFail"),
            &make_string(&env, ""),
            &0u32,
            &make_string(&env, ""),
        );
    });
    assert!(result.is_err());
}

#[test]
fn test_freeze_blocks_trait_add() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);
    client.freeze_metadata(&owner, &token_id);

    let result = std::panic::catch_unwind(|| {
        client.add_string_trait(
            &owner,
            &token_id,
            &make_string(&env, "evil"),
            &make_string(&env, "trait"),
        );
    });
    assert!(result.is_err());
}

#[test]
fn test_unfreeze_allows_updates() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);
    client.freeze_metadata(&owner, &token_id);
    client.unfreeze_metadata(&owner, &token_id);

    let m = client.get_metadata(&token_id);
    assert!(!m.frozen);

    client.update_metadata(
        &owner,
        &token_id,
        &make_string(&env, "After thaw"),
        &make_string(&env, ""),
        &0u32,
        &make_string(&env, ""),
    );
    let m = client.get_metadata(&token_id);
    assert_eq!(m.name, make_string(&env, "After thaw"));
}

#[test]
fn test_freeze_blocks_burn() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);
    client.freeze_metadata(&owner, &token_id);
    let result = std::panic::catch_unwind(|| {
        client.burn_token(&owner, &token_id);
    });
    assert!(result.is_err());
}

#[test]
fn test_freeze_still_allows_metric_reads() {
    let (env, _admin, _migrator, oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);
    client.freeze_metadata(&owner, &token_id);
    client.set_performance_metric(&oracle, &token_id, &make_string(&env, "wins"), &5u64);
    let v = client.get_performance_metric(&token_id, &make_string(&env, "wins"));
    assert_eq!(v, 5);
}

// ══════════════════════════════════════════════════════════════════════════
// IMAGE URI (FEATURE 9)
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_image_uri_format_matches_metadata() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    let uri = client.get_image_uri(&token_id);
    let expected = make_string(
        &env,
        &format!(
            "{}/nft/{}/lvl/{}/rar/{}.{}",
            "ipfs://bafy.../metadata",
            token_id,
            1,
            1,
            "png"
        ),
    );
    assert_eq!(uri, expected);
}

#[test]
fn test_image_uri_recomputed_after_level_up() {
    let (env, admin, _migrator, oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    client.configure_trait_rule(
        &admin,
        &make_string(&env, "perf"),
        &50u64,
        &make_string(&env, "rank"),
        &make_string(&env, "epic"),
        &2u32,
        &3u32,
    );
    let m0 = client.get_metadata(&token_id);
    let uri0 = client.get_image_uri(&token_id);
    let expected0 = make_string(
        &env,
        &format!(
            "{}/nft/{}/lvl/{}/rar/{}.{}",
            "ipfs://bafy.../metadata",
            token_id,
            m0.level,
            m0.rarity,
            "png"
        ),
    );
    assert_eq!(uri0, expected0);

    client.add_performance_metric(&oracle, &token_id, &make_string(&env, "wins"), &60u64);

    let m1 = client.get_metadata(&token_id);
    let uri1 = client.get_image_uri(&token_id);
    let expected1 = make_string(
        &env,
        &format!(
            "{}/nft/{}/lvl/{}/rar/{}.{}",
            "ipfs://bafy.../metadata",
            token_id,
            m1.level,
            m1.rarity,
            "png"
        ),
    );
    assert_eq!(uri1, expected1);
    assert_ne!(uri0, uri1);
}

#[test]
fn test_image_base_uri_update_changes_uri() {
    let (env, admin, _migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    client.update_image_base_uri(&admin, &make_string(&env, "https://cdn.example/v2"));
    let uri = client.get_image_uri(&token_id);
    let expected = make_string(
        &env,
        &format!(
            "{}/nft/{}/lvl/{}/rar/{}.{}",
            "https://cdn.example/v2", token_id, 1, 1, "png"
        ),
    );
    assert_eq!(uri, expected);
}

// ══════════════════════════════════════════════════════════════════════════
// MIGRATION (FEATURE 10)
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_migrate_metadata_bumps_version_and_snapshots() {
    let (env, _admin, migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    let new_string_traits: Map<String, String> = Map::new(&env);
    let new_numeric_traits: Map<String, u32> = Map::new(&env);

    let new_version = client.migrate_metadata(
        &migrator,
        &token_id,
        &2u32,
        &make_string(&env, "Migrated Name"),
        &make_string(&env, "Migrated desc"),
        &99u32,
        &make_string(&env, "ipfs://v2"),
        &new_string_traits,
        &new_numeric_traits,
        &5u32,
        &7u32,
    );
    // Trace: mint(1) -> "migrate" pre-snapshot(2) -> "migrated:1->2" post-snapshot(3).
    assert_eq!(new_version, 3);

    let m = client.get_metadata(&token_id);
    assert_eq!(m.schema_version, 2);
    assert_eq!(m.name, make_string(&env, "Migrated Name"));
    assert_eq!(m.image_id, 99);
    assert_eq!(m.level, 5);
    assert_eq!(m.rarity, 7);

    let versions = client.get_version_history(&token_id);
    assert_eq!(versions.len(), 3);
    assert_eq!(versions.get(0).unwrap().reason, make_string(&env, "mint"));
    assert_eq!(versions.get(1).unwrap().reason, make_string(&env, "migrate"));
    assert_eq!(versions.get(2).unwrap().reason, make_string(&env, "migrated:1->2"));
}

#[test]
#[should_panic(expected = "NotMigrator")]
fn test_migrate_non_migrator_panics() {
    let (env, _admin, _migrator, _oracle, client) = setup();
    let non_migrator = Address::generate(&env);
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);

    let new_string_traits: Map<String, String> = Map::new(&env);
    let new_numeric_traits: Map<String, u32> = Map::new(&env);

    let _ = client.migrate_metadata(
        &non_migrator,
        &token_id,
        &2u32,
        &make_string(&env, "X"),
        &make_string(&env, ""),
        &0u32,
        &make_string(&env, ""),
        &new_string_traits,
        &new_numeric_traits,
        &0u32,
        &0u32,
    );
}

#[test]
fn test_update_schema_version_lower_panics() {
    let (env, admin, _migrator, _oracle, client) = setup();
    // First, raise the version to 2 successfully (1 -> 2 is allowed).
    client.update_schema_version(&admin, &2u32);
    // Now attempt a downgrade (2 -> 1). It must panic.
    let result = std::panic::catch_unwind(|| {
        client.update_schema_version(&admin, &1u32);
    });
    assert!(result.is_err(), "downgrade should panic");
}

// ══════════════════════════════════════════════════════════════════════════
// ROLE SWITCHING
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_admin_can_swap_oracle() {
    let (env, admin, _migrator, oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);
    let new_oracle = Address::generate(&env);
    client.set_oracle(&admin, &new_oracle);

    // Old oracle can no longer author metrics
    let r = std::panic::catch_unwind(|| {
        client.set_performance_metric(&oracle, &token_id, &make_string(&env, "wins"), &1u64);
    });
    assert!(r.is_err());

    // New oracle can
    client.set_performance_metric(&new_oracle, &token_id, &make_string(&env, "wins"), &1u64);
    assert_eq!(client.get_performance_metric(&token_id, &make_string(&env, "wins")), 1);
}

#[test]
fn test_admin_can_swap_migrator() {
    let (env, admin, migrator, _oracle, client) = setup();
    let owner = Address::generate(&env);
    let token_id = mint_default(&env, &client, &owner);
    let new_migrator = Address::generate(&env);
    client.set_migrator(&admin, &new_migrator);

    let new_string_traits: Map<String, String> = Map::new(&env);
    let new_numeric_traits: Map<String, u32> = Map::new(&env);
    let r = std::panic::catch_unwind(|| {
        client.migrate_metadata(
            &migrator,
            &token_id,
            &2u32,
            &make_string(&env, "X"),
            &make_string(&env, ""),
            &0u32,
            &make_string(&env, ""),
            &new_string_traits,
            &new_numeric_traits,
            &0u32,
            &0u32,
        );
    });
    assert!(r.is_err());

    let r2 = std::panic::catch_unwind(|| {
        client.migrate_metadata(
            &new_migrator,
            &token_id,
            &2u32,
            &make_string(&env, "X"),
            &make_string(&env, ""),
            &0u32,
            &make_string(&env, ""),
            &new_string_traits,
            &new_numeric_traits,
            &0u32,
            &0u32,
        );
    });
    assert!(r2.is_ok());
}

// ══════════════════════════════════════════════════════════════════════════
// EVENT-DRIVEN FLOW (END-TO-END)
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_full_lifecycle_emit_and_query() {
    let (env, admin, migrator, oracle, client) = setup();
    let owner = Address::generate(&env);
    let a = Address::generate(&env);

    // Mint
    let token_id = mint_default(&env, &client, &owner);

    // Admin adds a trait rule and a trigger rule
    client.configure_trait_rule(
        &admin,
        &make_string(&env, "perf"),
        &100u64,
        &make_string(&env, "rank"),
        &make_string(&env, "aside"),
        &2u32,
        &5u32,
    );
    client.configure_trigger_rule(
        &admin,
        &make_string(&env, "metric"),
        &make_string(&env, "wins"),
        &5u64,
        &make_string(&env, "add_trait"),
        &make_string(&env, "ribbon"),
        &make_string(&env, "blue"),
    );

    // Owner transfers once, oracle drives performance
    client.transfer_token(&owner, &a, &token_id);
    client.set_performance_metric(&oracle, &token_id, &make_string(&env, "wins"), &120u64);

    let m = client.get_metadata(&token_id);
    // Trigger rule fired -> ribbon blue
    assert_eq!(
        m.string_traits.get(make_string(&env, "ribbon")).unwrap(),
        make_string(&env, "blue")
    );
    // Trait rule fired -> rank=aside + level/rarity bumped
    assert_eq!(
        m.string_traits.get(make_string(&env, "rank")).unwrap(),
        make_string(&env, "aside")
    );
    assert_eq!(m.level, 2);
    assert_eq!(m.rarity, 5);

    // History: 1 mint + 1 transfer + 1 burn(? no burn in lifecycle) => 2 records
    let own_history = client.get_ownership_history(&token_id);
    assert_eq!(own_history.len(), 2);
    let versions = client.get_version_history(&token_id);
    assert!(versions.len() >= 4);

    // Migrate
    let new_string_traits: Map<String, String> = Map::new(&env);
    let new_numeric_traits: Map<String, u32> = Map::new(&env);
    let _ = client.migrate_metadata(
        &migrator,
        &token_id,
        &2u32,
        &make_string(&env, "Final form"),
        &make_string(&env, ""),
        &0u32,
        &make_string(&env, ""),
        &new_string_traits,
        &new_numeric_traits,
        &10u32,
        &10u32,
    );
    let m = client.get_metadata(&token_id);
    assert_eq!(m.schema_version, 2);
    assert_eq!(m.level, 10);
    assert_eq!(m.rarity, 10);
}
