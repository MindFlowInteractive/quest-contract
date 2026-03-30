#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec, token,
    panic_with_error, contracterror,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    InvalidShares = 1,
    SplitAlreadyExists = 2,
    SplitNotFound = 3,
    Unauthorized = 4,
    DepositRequired = 5,
    RecipientsLocked = 6,
    InsufficientBalance = 7,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct SplitConfig {
    pub id: u64,
    pub recipients: Vec<(Address, i128)>, // (Recipient, Basis Points)
    pub token: Address,
    pub total_released: i128,
}

#[contracttype]
pub enum DataKey {
    Split(u64),      // SplitConfig
    Balance(u64),    // Total deposited for this split
    Released(u64, Address), // Amount released to specific recipient for a split
}

#[contract]
pub struct PaymentSplitter;

#[contractimpl]
impl PaymentSplitter {
    /// Creates a new payment split.
    /// Validate shares = 10000 bps (100%).
    pub fn create_split(env: Env, id: u64, recipients: Vec<(Address, i128)>, token: Address) -> Result<(), Error> {
        if env.storage().persistent().has(&DataKey::Split(id)) {
            return Err(Error::SplitAlreadyExists);
        }

        validate_shares(&recipients)?;

        let config = SplitConfig {
            id,
            recipients,
            token,
            total_released: 0,
        };

        env.storage().persistent().set(&DataKey::Split(id), &config);
        env.storage().persistent().set(&DataKey::Balance(id), &0i128);

        Ok(())
    }

    /// Deposits tokens into a specific split.
    /// Transfers tokens from the caller to the contract.
    pub fn deposit(env: Env, split_id: u64, amount: i128, from: Address) -> Result<(), Error> {
        from.require_auth();

        let mut config = get_split_config(&env, split_id)?;
        
        let token_client = token::Client::new(&env, &config.token);
        token_client.transfer(&from, &env.current_contract_address(), &amount);

        let current_balance: i128 = env.storage().persistent().get(&DataKey::Balance(split_id)).unwrap_or(0);
        env.storage().persistent().set(&DataKey::Balance(split_id), &(current_balance + amount));

        env.events().publish(
            (symbol_short!("deposited"), split_id),
            (from, amount),
        );

        Ok(())
    }

    /// Releases all pending shares for all recipients in a split.
    pub fn release(env: Env, split_id: u64) -> Result<(), Error> {
        let mut config = get_split_config(&env, split_id)?;
        let total_deposited: i128 = env.storage().persistent().get(&DataKey::Balance(split_id)).unwrap_or(0);
        
        if total_deposited == 0 {
            return Err(Error::DepositRequired);
        }

        let token_client = token::Client::new(&env, &config.token);
        let mut total_released_in_this_call = 0i128;

        for recipient_data in config.recipients.iter() {
            let (recipient, bps) = recipient_data;
            
            // Calculate total share ever earned: (Total Deposited * BPS) / 10000
            let total_share = (total_deposited * bps) / 10000;
            
            // Get already released amount for this recipient
            let released_to_date: i128 = env.storage().persistent().get(&DataKey::Released(split_id, recipient.clone())).unwrap_or(0);
            
            let amount_to_release = total_share - released_to_date;
            
            if amount_to_release > 0 {
                token_client.transfer(&env.current_contract_address(), &recipient, &amount_to_release);
                
                env.storage().persistent().set(&DataKey::Released(split_id, recipient.clone()), &total_share);
                total_released_in_this_call += amount_to_release;

                env.events().publish(
                    (symbol_short!("released"), split_id, recipient),
                    amount_to_release,
                );
            }
        }

        config.total_released += total_released_in_this_call;
        env.storage().persistent().set(&DataKey::Split(split_id), &config);

        Ok(())
    }

    /// Releases pending shares for a specific recipient.
    pub fn release_to(env: Env, split_id: u64, recipient: Address) -> Result<(), Error> {
        let mut config = get_split_config(&env, split_id)?;
        let total_deposited: i128 = env.storage().persistent().get(&DataKey::Balance(split_id)).unwrap_or(0);
        
        if total_deposited == 0 {
            return Err(Error::DepositRequired);
        }

        let mut recipient_bps = 0i128;
        let mut found = false;
        for data in config.recipients.iter() {
            if data.0 == recipient {
                recipient_bps = data.1;
                found = true;
                break;
            }
        }

        if !found {
            return Err(Error::Unauthorized);
        }

        let total_share = (total_deposited * recipient_bps) / 10000;
        let released_to_date: i128 = env.storage().persistent().get(&DataKey::Released(split_id, recipient.clone())).unwrap_or(0);
        
        let amount_to_release = total_share - released_to_date;
        
        if amount_to_release > 0 {
            let token_client = token::Client::new(&env, &config.token);
            token_client.transfer(&env.current_contract_address(), &recipient, &amount_to_release);
            
            env.storage().persistent().set(&DataKey::Released(split_id, recipient.clone()), &total_share);
            
            config.total_released += amount_to_release;
            env.storage().persistent().set(&DataKey::Split(split_id), &config);

            env.events().publish(
                (symbol_short!("released"), split_id, recipient),
                amount_to_release,
            );
        }

        Ok(())
    }

    /// Updates recipients for a split. 
    /// Only allowed if no deposits have been made yet (Lock Config).
    pub fn update_recipients(env: Env, split_id: u64, new_recipients: Vec<(Address, i128)>, admin: Address) -> Result<(), Error> {
        admin.require_auth();
        
        let mut config = get_split_config(&env, split_id)?;
        
        let total_deposited: i128 = env.storage().persistent().get(&DataKey::Balance(split_id)).unwrap_or(0);
        if total_deposited > 0 {
            return Err(Error::RecipientsLocked);
        }

        validate_shares(&new_recipients)?;

        config.recipients = new_recipients;
        env.storage().persistent().set(&DataKey::Split(split_id), &config);

        Ok(())
    }

    /// Returns split details including deposit and release tracking.
    pub fn get_split(env: Env, split_id: u64) -> Result<(SplitConfig, i128, i128, Vec<(Address, i128)>), Error> {
        let config = get_split_config(&env, split_id)?;
        let total_deposited: i128 = env.storage().persistent().get(&DataKey::Balance(split_id)).unwrap_or(0);
        
        let mut per_recipient_released = Vec::new(&env);
        for data in config.recipients.iter() {
            let recipient = data.0;
            let released: i128 = env.storage().persistent().get(&DataKey::Released(split_id, recipient.clone())).unwrap_or(0);
            per_recipient_released.push_back((recipient, released));
        }

        Ok((config.clone(), total_deposited, config.total_released, per_recipient_released))
    }
}

fn validate_shares(recipients: &Vec<(Address, i128)>) -> Result<(), Error> {
    let mut total_bps = 0i128;
    for data in recipients.iter() {
        total_bps += data.1;
    }

    if total_bps != 10000 {
        return Err(Error::InvalidShares);
    }
    Ok(())
}

fn get_split_config(env: &Env, split_id: u64) -> Result<SplitConfig, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::Split(split_id))
        .ok_or(Error::SplitNotFound)
}
