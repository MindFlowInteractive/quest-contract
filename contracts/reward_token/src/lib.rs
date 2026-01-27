#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, Vec};

#[contracttype]
pub enum DataKey {
    Balance(Address),
    TotalSupply,
    Admin,
    Allowance(Address, Address), // (owner, spender)
    AuthorizedMinters(Address),
    Name,
    Symbol,
    Decimals,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RewardType {
    HintPurchase,
    LevelUnlock,
    Achievement,
}

#[contract]
pub struct RewardToken;

#[contractimpl]
impl RewardToken {
    /// Initialize the token contract with metadata
    pub fn initialize(env: Env, admin: Address, name: String, symbol: String, decimals: u32) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TotalSupply, &0i128);
        env.storage().instance().set(&DataKey::Name, &name);
        env.storage().instance().set(&DataKey::Symbol, &symbol);
        env.storage().instance().set(&DataKey::Decimals, &decimals);
    }

    /// Get token name
    pub fn name(env: Env) -> String {
        env.storage()
            .instance()
            .get(&DataKey::Name)
            .unwrap_or(String::from_str(&env, "Reward Token"))
    }

    /// Get token symbol
    pub fn symbol(env: Env) -> String {
        env.storage()
            .instance()
            .get(&DataKey::Symbol)
            .unwrap_or(String::from_str(&env, "RWD"))
    }

    /// Get token decimals
    pub fn decimals(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::Decimals)
            .unwrap_or(6)
    }

    /// Authorize a minter address (admin only)
    pub fn authorize_minter(env: Env, minter: Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        env.storage()
            .instance()
            .set(&DataKey::AuthorizedMinters(minter), &true);
    }

    /// Revoke minter authorization (admin only)
    pub fn revoke_minter(env: Env, minter: Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        env.storage()
            .instance()
            .remove(&DataKey::AuthorizedMinters(minter));
    }

    /// Check if address is authorized minter
    pub fn is_authorized_minter(env: Env, minter: Address) -> bool {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();

        // Admin is always authorized
        if minter == admin {
            return true;
        }

        env.storage()
            .instance()
            .get(&DataKey::AuthorizedMinters(minter))
            .unwrap_or(false)
    }

    /// Mint new tokens (admin or authorized minter only)
    pub fn mint(env: Env, minter: Address, to: Address, amount: i128) {
        if amount <= 0 {
            panic!("Amount must be positive");
        }

        minter.require_auth();

        // Check if minter is authorized
        if !Self::is_authorized_minter(env.clone(), minter.clone()) {
            panic!("Not authorized");
        }

        let balance = Self::balance(env.clone(), to.clone());
        env.storage()
            .instance()
            .set(&DataKey::Balance(to), &(balance + amount));

        let total_supply: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalSupply)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalSupply, &(total_supply + amount));
    }

    /// Distribute rewards to multiple addresses
    pub fn distribute_rewards(env: Env, recipients: Vec<Address>, amounts: Vec<i128>) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        if recipients.len() != amounts.len() {
            panic!("Recipients and amounts length mismatch");
        }

        for i in 0..recipients.len() {
            let recipient = recipients.get(i).unwrap();
            let amount = amounts.get(i).unwrap();

            if amount > 0 {
                let balance = Self::balance(env.clone(), recipient.clone());
                env.storage()
                    .instance()
                    .set(&DataKey::Balance(recipient), &(balance + amount));

                let total_supply: i128 = env
                    .storage()
                    .instance()
                    .get(&DataKey::TotalSupply)
                    .unwrap_or(0);
                env.storage()
                    .instance()
                    .set(&DataKey::TotalSupply, &(total_supply + amount));
            }
        }
    }

    /// Transfer tokens
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();

        if amount <= 0 {
            panic!("Amount must be positive");
        }

        let from_balance = Self::balance(env.clone(), from.clone());
        let to_balance = Self::balance(env.clone(), to.clone());

        if from_balance < amount {
            panic!("Insufficient balance");
        }

        env.storage()
            .instance()
            .set(&DataKey::Balance(from), &(from_balance - amount));
        env.storage()
            .instance()
            .set(&DataKey::Balance(to), &(to_balance + amount));
    }

    /// Approve spender to spend tokens on behalf of owner
    pub fn approve(env: Env, owner: Address, spender: Address, amount: i128) {
        owner.require_auth();

        if amount < 0 {
            panic!("Amount cannot be negative");
        }

        env.storage()
            .instance()
            .set(&DataKey::Allowance(owner, spender), &amount);
    }

    /// Transfer tokens from one address to another using allowance
    pub fn transfer_from(
        env: Env,
        spender: Address,
        from: Address,
        to: Address,
        amount: i128,
    ) {
        spender.require_auth();

        if amount <= 0 {
            panic!("Amount must be positive");
        }

        let allowance = Self::allowance(env.clone(), from.clone(), spender.clone());
        if allowance < amount {
            panic!("Insufficient allowance");
        }

        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            panic!("Insufficient balance");
        }

        let to_balance = Self::balance(env.clone(), to.clone());

        // Update balances
        env.storage()
            .instance()
            .set(&DataKey::Balance(from.clone()), &(from_balance - amount));
        env.storage()
            .instance()
            .set(&DataKey::Balance(to), &(to_balance + amount));

        // Update allowance
        env.storage()
            .instance()
            .set(&DataKey::Allowance(from, spender), &(allowance - amount));
    }

    /// Spend tokens for in-game unlocks (burn tokens)
    pub fn spend_for_unlock(
        env: Env,
        spender: Address,
        amount: i128,
        _unlock_type: String,
    ) {
        spender.require_auth();

        if amount <= 0 {
            panic!("Amount must be positive");
        }

        let balance = Self::balance(env.clone(), spender.clone());
        if balance < amount {
            panic!("Insufficient balance to spend");
        }

        // Deduct from balance (burn)
        env.storage()
            .instance()
            .set(&DataKey::Balance(spender), &(balance - amount));

        // Reduce total supply
        let total_supply: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalSupply)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalSupply, &(total_supply - amount));
    }

    /// Burn tokens (reduce total supply)
    pub fn burn(env: Env, from: Address, amount: i128) {
        from.require_auth();

        if amount <= 0 {
            panic!("Amount must be positive");
        }

        let balance = Self::balance(env.clone(), from.clone());
        if balance < amount {
            panic!("Insufficient balance to burn");
        }

        env.storage()
            .instance()
            .set(&DataKey::Balance(from), &(balance - amount));

        let total_supply: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalSupply)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalSupply, &(total_supply - amount));
    }

    /// Get balance of an account
    pub fn balance(env: Env, account: Address) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::Balance(account))
            .unwrap_or(0)
    }

    /// Get allowance
    pub fn allowance(env: Env, owner: Address, spender: Address) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::Allowance(owner, spender))
            .unwrap_or(0)
    }

    /// Get total supply
    pub fn total_supply(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalSupply)
            .unwrap_or(0)
    }

    /// Get admin address
    pub fn admin(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn test_initialization() {
        let env = Env::default();
        let contract_id = env.register_contract(None, RewardToken);
        let client = RewardTokenClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let name = String::from_str(&env, "Game Reward Token");
        let symbol = String::from_str(&env, "GRWD");

        client.initialize(&admin, &name, &symbol, &6);

        assert_eq!(client.name(), name);
        assert_eq!(client.symbol(), symbol);
        assert_eq!(client.decimals(), 6);
        assert_eq!(client.admin(), admin);
    }

    #[test]
    fn test_mint_and_balance() {
        let env = Env::default();
        let contract_id = env.register_contract(None, RewardToken);
        let client = RewardTokenClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);

        client.initialize(
            &admin,
            &String::from_str(&env, "Reward"),
            &String::from_str(&env, "RWD"),
            &6,
        );

        env.mock_all_auths();

        client.mint(&admin, &user, &1000);

        assert_eq!(client.balance(&user), 1000);
        assert_eq!(client.total_supply(), 1000);
    }

    #[test]
    fn test_transfer() {
        let env = Env::default();
        let contract_id = env.register_contract(None, RewardToken);
        let client = RewardTokenClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);

        client.initialize(
            &admin,
            &String::from_str(&env, "Reward"),
            &String::from_str(&env, "RWD"),
            &6,
        );

        env.mock_all_auths();

        client.mint(&admin, &user1, &1000);
        client.transfer(&user1, &user2, &300);

        assert_eq!(client.balance(&user1), 700);
        assert_eq!(client.balance(&user2), 300);
    }

    #[test]
    fn test_approve_and_transfer_from() {
        let env = Env::default();
        let contract_id = env.register_contract(None, RewardToken);
        let client = RewardTokenClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let owner = Address::generate(&env);
        let spender = Address::generate(&env);
        let recipient = Address::generate(&env);

        client.initialize(
            &admin,
            &String::from_str(&env, "Reward"),
            &String::from_str(&env, "RWD"),
            &6,
        );

        env.mock_all_auths();

        client.mint(&admin, &owner, &1000);
        client.approve(&owner, &spender, &500);

        assert_eq!(client.allowance(&owner, &spender), 500);

        client.transfer_from(&spender, &owner, &recipient, &200);

        assert_eq!(client.balance(&owner), 800);
        assert_eq!(client.balance(&recipient), 200);
        assert_eq!(client.allowance(&owner, &spender), 300);
    }

    #[test]
    fn test_burn() {
        let env = Env::default();
        let contract_id = env.register_contract(None, RewardToken);
        let client = RewardTokenClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);

        client.initialize(
            &admin,
            &String::from_str(&env, "Reward"),
            &String::from_str(&env, "RWD"),
            &6,
        );

        env.mock_all_auths();

        client.mint(&admin, &user, &1000);
        client.burn(&user, &300);

        assert_eq!(client.balance(&user), 700);
        assert_eq!(client.total_supply(), 700);
    }

    #[test]
    fn test_spend_for_unlock() {
        let env = Env::default();
        let contract_id = env.register_contract(None, RewardToken);
        let client = RewardTokenClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let player = Address::generate(&env);

        client.initialize(
            &admin,
            &String::from_str(&env, "Reward"),
            &String::from_str(&env, "RWD"),
            &6,
        );

        env.mock_all_auths();

        client.mint(&admin, &player, &1000);
        client.spend_for_unlock(&player, &250, &String::from_str(&env, "level_unlock"));

        assert_eq!(client.balance(&player), 750);
        assert_eq!(client.total_supply(), 750);
    }

    #[test]
    fn test_distribute_rewards() {
        let env = Env::default();
        let contract_id = env.register_contract(None, RewardToken);
        let client = RewardTokenClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);
        let user3 = Address::generate(&env);

        client.initialize(
            &admin,
            &String::from_str(&env, "Reward"),
            &String::from_str(&env, "RWD"),
            &6,
        );

        env.mock_all_auths();

        let mut recipients = Vec::new(&env);
        recipients.push_back(user1.clone());
        recipients.push_back(user2.clone());
        recipients.push_back(user3.clone());

        let mut amounts = Vec::new(&env);
        amounts.push_back(100);
        amounts.push_back(200);
        amounts.push_back(300);

        client.distribute_rewards(&recipients, &amounts);

        assert_eq!(client.balance(&user1), 100);
        assert_eq!(client.balance(&user2), 200);
        assert_eq!(client.balance(&user3), 300);
        assert_eq!(client.total_supply(), 600);
    }

    #[test]
    fn test_authorize_minter() {
        let env = Env::default();
        let contract_id = env.register_contract(None, RewardToken);
        let client = RewardTokenClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let minter = Address::generate(&env);

        client.initialize(
            &admin,
            &String::from_str(&env, "Reward"),
            &String::from_str(&env, "RWD"),
            &6,
        );

        env.mock_all_auths();

        assert_eq!(client.is_authorized_minter(&minter), false);

        client.authorize_minter(&minter);
        assert_eq!(client.is_authorized_minter(&minter), true);

        client.revoke_minter(&minter);
        assert_eq!(client.is_authorized_minter(&minter), false);
    }

    #[test]
    #[should_panic(expected = "Insufficient balance")]
    fn test_transfer_insufficient_balance() {
        let env = Env::default();
        let contract_id = env.register_contract(None, RewardToken);
        let client = RewardTokenClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);

        client.initialize(
            &admin,
            &String::from_str(&env, "Reward"),
            &String::from_str(&env, "RWD"),
            &6,
        );

        env.mock_all_auths();

        client.mint(&admin, &user1, &100);
        client.transfer(&user1, &user2, &200);
    }
}
