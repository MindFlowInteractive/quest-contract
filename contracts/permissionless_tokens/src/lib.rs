use soroban_sdk::{contractimpl, Address, Env, String, Symbol};

#[derive(Clone)]
pub struct TokenParams {
    pub name: String,
    pub symbol: String,
    pub supply_cap: i128,
    pub creator: Address,
}

pub struct PermissionlessTokenFactory;

#[contractimpl]
impl PermissionlessTokenFactory {
    /// Anyone may create a token; params are validated before registration.
    pub fn create_token(env: Env, creator: Address, name: String, symbol: String, supply_cap: i128) -> u64 {
        creator.require_auth();
        assert!(supply_cap > 0, "supply_cap must be positive");
        assert!(name.len() > 0 && symbol.len() > 0, "name/symbol required");

        let token_id: u64 = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "next_id"))
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "next_id"), &(token_id + 1));

        // registry entry keyed by token_id; full TokenParams struct stored for lookup
        env.storage().persistent().set(&token_id, &TokenParams {
            name,
            symbol,
            supply_cap,
            creator,
        });

        token_id
    }

    pub fn get_token(env: Env, token_id: u64) -> TokenParams {
        env.storage().persistent().get(&token_id).unwrap()
    }
}
