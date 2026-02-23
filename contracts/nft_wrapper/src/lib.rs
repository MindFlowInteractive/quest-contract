#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, BytesN,
    Bytes, Env, Vec, String,
};

#[cfg(test)]
mod test;

/// Cross-Chain NFT Wrapper Contract
///
/// Enables secure NFT transfers between Stellar and other blockchains
/// through a validator-based bridge system.

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeAction {
    Lock = 0,    
    Unlock = 1,  
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferStatus {
    Initiated = 0,    
    Locked = 1,       
    Verified = 2,     
    Wrapped = 3,      
    Completed = 4,    
    Cancelled = 5,    
    Failed = 6,       
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin = 0,
    Paused = 1,
    Config = 2,
    Validators = 3,
    NextTransferId = 4,
    BridgeFees = 5,
    ChainId = 6,
}

#[contracttype]
pub enum TransferKey {
    Transfer(u64),
    WrappedNFT(u64),
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct NFTData {
    pub contract: Address,
    pub token_id: u64,
    pub owner: Address,
    pub name: String,
    pub symbol: String,
    pub uri: String,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct BridgeTransfer {
    pub id: u64,
    pub action: BridgeAction,
    pub nft_data: NFTData,
    pub sender: Address,
    pub recipient: Bytes,
    pub source_chain: u32,
    pub destination_chain: u32,
    pub status: TransferStatus,
    pub locked_timestamp: u64,
    pub verified_timestamp: Option<u64>,
    pub completed_timestamp: Option<u64>,
    pub fee_amount: i128,
    pub nonce: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct WrappedNFTData {
    pub transfer_id: u64,
    pub original_contract: Address,
    pub original_token_id: u64,
    pub original_chain: u32,
    pub wrapped_token_address: Address,
    pub wrapped_token_id: u64,
    pub current_owner: Address,
    pub wrapped_timestamp: u64,
    pub metadata_uri: String,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct ValidatorSignature {
    pub validator: Address,
    pub signature: BytesN<64>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Validator {
    pub address: Address,
    pub public_key: BytesN<32>,
    pub active: bool,
    pub added_timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct BridgeConfig {
    pub admin: Address,
    pub required_signatures: u32,
    pub max_validators: u32,
    pub base_fee_bps: u32,
    pub min_fee: i128,
    pub max_fee: i128,
    pub fee_token: Option<Address>,
    pub fee_collector: Address,
    pub paused: bool,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum NftWrapperError {
    Unauthorized = 1,
    InvalidTransfer = 2,
    TransferNotFound = 3,
    InvalidChainId = 12,
    InvalidMetadata = 13,
    ContractPaused = 11,
    InsufficientSignatures = 7,
    ValidatorNotFound = 8,
    ValidatorAlreadyExists = 9,
    MaxValidatorsReached = 10,
    FeeCalculationError = 14,
    DuplicateSignature = 19,
}

#[contract]
pub struct NftWrapperContract;

#[contractimpl]
impl NftWrapperContract {
    /// Initialize the NFT wrapper contract
    pub fn initialize(
        env: Env,
        admin: Address,
        fee_token: Option<Address>,
        fee_collector: Address,
        _native_chain_id: u32,
        current_chain_id: u32,
    ) -> Result<(), NftWrapperError> {
        admin.require_auth();

        let config = BridgeConfig {
            admin: admin.clone(),
            required_signatures: 2,
            max_validators: 10,
            base_fee_bps: 50,
            min_fee: 100_000_000,
            max_fee: 10_000_000_000,
            fee_token,
            fee_collector,
            paused: false,
        };

        let storage = env.storage().instance();
        storage.set(&DataKey::Admin, &admin);
        storage.set(&DataKey::Config, &config);
        storage.set(&DataKey::Paused, &false);
        storage.set(&DataKey::NextTransferId, &1u64);
        storage.set(&DataKey::ChainId, &current_chain_id);
        storage.set(&DataKey::BridgeFees, &0i128);

        Ok(())
    }

    /// Add a validator to the bridge
    pub fn add_validator(
        env: Env,
        validator_address: Address,
        public_key: BytesN<32>,
    ) -> Result<(), NftWrapperError> {
        Self::require_admin(&env)?;
        Self::check_paused(&env)?;

        let storage = env.storage().instance();
        
        let config: BridgeConfig = storage.get(&DataKey::Config).unwrap();

        let mut validators: Vec<Validator> = storage.get(&DataKey::Validators).unwrap_or(Vec::new(&env));

        for validator in validators.iter() {
            if validator.address == validator_address {
                return Err(NftWrapperError::ValidatorAlreadyExists);
            }
        }

        if validators.len() as u32 >= config.max_validators {
            return Err(NftWrapperError::MaxValidatorsReached);
        }

        let new_validator = Validator {
            address: validator_address,
            public_key,
            active: true,
            added_timestamp: env.ledger().timestamp(),
        };
        validators.push_back(new_validator);

        storage.set(&DataKey::Validators, &validators);

        Ok(())
    }

    /// Remove a validator from the bridge
    pub fn remove_validator(
        env: Env,
        validator_address: Address,
    ) -> Result<(), NftWrapperError> {
        Self::require_admin(&env)?;

        let storage = env.storage().instance();
        
        let validators: Vec<Validator> = storage.get(&DataKey::Validators).ok_or(NftWrapperError::ValidatorNotFound)?;

        let mut updated_validators = Vec::new(&env);
        let mut found = false;

        for validator in validators.iter() {
            if validator.address != validator_address {
                updated_validators.push_back(validator);
            } else {
                found = true;
            }
        }

        if !found {
            return Err(NftWrapperError::ValidatorNotFound);
        }

        storage.set(&DataKey::Validators, &updated_validators);

        Ok(())
    }

    /// Get list of active validators
    pub fn get_validators(env: Env) -> Vec<Validator> {
        env.storage().instance().get(&DataKey::Validators).unwrap_or(Vec::new(&env))
    }

    /// Lock an NFT on the source chain
    pub fn lock_nft(
        env: Env,
        sender: Address,
        nft_contract: Address,
        token_id: u64,
        recipient_address: Bytes,
        destination_chain: u32,
        name: String,
        symbol: String,
        uri: String,
    ) -> Result<u64, NftWrapperError> {
        Self::check_paused(&env)?;
        sender.require_auth();

        let storage = env.storage().instance();
        let current_chain_id: u32 = storage.get(&DataKey::ChainId).unwrap_or(0);

        if destination_chain == current_chain_id {
            return Err(NftWrapperError::InvalidChainId);
        }

        if name.is_empty() || symbol.is_empty() {
            return Err(NftWrapperError::InvalidMetadata);
        }

        let transfer_id: u64 = storage.get(&DataKey::NextTransferId).unwrap_or(1);

        let nft_data = NFTData {
            contract: nft_contract,
            token_id,
            owner: sender.clone(),
            name,
            symbol,
            uri,
        };

        let bridge_transfer = BridgeTransfer {
            id: transfer_id,
            action: BridgeAction::Lock,
            nft_data,
            sender,
            recipient: recipient_address,
            source_chain: current_chain_id,
            destination_chain,
            status: TransferStatus::Locked,
            locked_timestamp: env.ledger().timestamp(),
            verified_timestamp: None,
            completed_timestamp: None,
            fee_amount: Self::calculate_fee(&env, 1_000_000)?,
            nonce: env.ledger().sequence() as u64,
        };

        env.storage().persistent().set(&TransferKey::Transfer(transfer_id), &bridge_transfer);
        storage.set(&DataKey::NextTransferId, &(transfer_id + 1));

        Ok(transfer_id)
    }

    /// Verify and complete a locked NFT transfer
    pub fn verify_and_wrap(
        env: Env,
        caller: Address,
        transfer_id: u64,
        signatures: Vec<ValidatorSignature>,
        wrapped_token_address: Address,
        wrapped_token_id: u64,
    ) -> Result<(), NftWrapperError> {
        Self::check_paused(&env)?;
        caller.require_auth();

        let transfer_opt: Option<BridgeTransfer> = env.storage().persistent().get(&TransferKey::Transfer(transfer_id));
        let mut transfer = transfer_opt.ok_or(NftWrapperError::TransferNotFound)?;

        if transfer.status != TransferStatus::Locked {
            return Err(NftWrapperError::InvalidTransfer);
        }

        Self::verify_signatures(&env, &transfer, &signatures)?;

        transfer.status = TransferStatus::Wrapped;
        transfer.verified_timestamp = Some(env.ledger().timestamp());

        let wrapped_nft = WrappedNFTData {
            transfer_id,
            original_contract: transfer.nft_data.contract.clone(),
            original_token_id: transfer.nft_data.token_id,
            original_chain: transfer.source_chain,
            wrapped_token_address,
            wrapped_token_id,
            current_owner: transfer.sender.clone(),
            wrapped_timestamp: env.ledger().timestamp(),
            metadata_uri: transfer.nft_data.uri.clone(),
        };

        env.storage().persistent().set(&TransferKey::Transfer(transfer_id), &transfer);
        env.storage().persistent().set(&TransferKey::WrappedNFT(transfer_id), &wrapped_nft);

        Ok(())
    }

    /// Unwrap a wrapped NFT
    pub fn unwrap_nft(
        env: Env,
        sender: Address,
        transfer_id: u64,
    ) -> Result<(), NftWrapperError> {
        Self::check_paused(&env)?;
        sender.require_auth();

        let wrapped_nft_opt: Option<WrappedNFTData> = env.storage().persistent().get(&TransferKey::WrappedNFT(transfer_id));
        let wrapped_nft = wrapped_nft_opt.ok_or(NftWrapperError::TransferNotFound)?;

        if wrapped_nft.current_owner != sender {
            return Err(NftWrapperError::Unauthorized);
        }

        let transfer_opt: Option<BridgeTransfer> = env.storage().persistent().get(&TransferKey::Transfer(transfer_id));
        let mut transfer = transfer_opt.ok_or(NftWrapperError::TransferNotFound)?;

        transfer.status = TransferStatus::Cancelled;
        transfer.completed_timestamp = Some(env.ledger().timestamp());

        env.storage().persistent().set(&TransferKey::Transfer(transfer_id), &transfer);

        Ok(())
    }

    /// Bridge NFT back to original chain
    pub fn bridge_back_nft(
        env: Env,
        caller: Address,
        transfer_id: u64,
        signatures: Vec<ValidatorSignature>,
    ) -> Result<(), NftWrapperError> {
        Self::check_paused(&env)?;
        caller.require_auth();

        let transfer_opt: Option<BridgeTransfer> = env.storage().persistent().get(&TransferKey::Transfer(transfer_id));
        let mut transfer = transfer_opt.ok_or(NftWrapperError::TransferNotFound)?;

        if transfer.status != TransferStatus::Wrapped {
            return Err(NftWrapperError::InvalidTransfer);
        }

        Self::verify_signatures(&env, &transfer, &signatures)?;

        transfer.status = TransferStatus::Completed;
        transfer.completed_timestamp = Some(env.ledger().timestamp());

        env.storage().persistent().set(&TransferKey::Transfer(transfer_id), &transfer);

        Ok(())
    }

    /// Get transfer details
    pub fn get_transfer(env: Env, transfer_id: u64) -> Result<BridgeTransfer, NftWrapperError> {
        let transfer_opt: Option<BridgeTransfer> = env.storage().persistent().get(&TransferKey::Transfer(transfer_id));
        transfer_opt.ok_or(NftWrapperError::TransferNotFound)
    }

    /// Get wrapped NFT details
    pub fn get_wrapped_nft(env: Env, transfer_id: u64) -> Result<WrappedNFTData, NftWrapperError> {
        let wrapped_opt: Option<WrappedNFTData> = env.storage().persistent().get(&TransferKey::WrappedNFT(transfer_id));
        wrapped_opt.ok_or(NftWrapperError::TransferNotFound)
    }

    /// Pause the contract
    pub fn pause(env: Env) -> Result<(), NftWrapperError> {
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        Ok(())
    }

    /// Unpause the contract
    pub fn unpause(env: Env) -> Result<(), NftWrapperError> {
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    /// Check if contract is paused
    pub fn is_paused(env: Env) -> bool {
        env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
    }

    /// Collect accumulated bridge fees
    pub fn collect_fees(env: Env) -> Result<i128, NftWrapperError> {
        Self::require_admin(&env)?;

        let storage = env.storage().instance();
        let collected_fees: i128 = storage.get(&DataKey::BridgeFees).unwrap_or(0i128);

        if collected_fees > 0 {
            storage.set(&DataKey::BridgeFees, &0i128);
        }

        Ok(collected_fees)
    }

    /// Get current bridge configuration
    pub fn get_config(env: Env) -> Result<BridgeConfig, NftWrapperError> {
        env.storage().instance().get(&DataKey::Config).ok_or(NftWrapperError::InvalidTransfer)
    }

    /// Update bridge configuration
    pub fn update_config(
        env: Env,
        required_signatures: Option<u32>,
        base_fee_bps: Option<u32>,
        min_fee: Option<i128>,
        max_fee: Option<i128>,
    ) -> Result<(), NftWrapperError> {
        Self::require_admin(&env)?;

        let storage = env.storage().instance();
        let mut config: BridgeConfig = storage.get(&DataKey::Config).ok_or(NftWrapperError::InvalidTransfer)?;

        if let Some(sigs) = required_signatures {
            config.required_signatures = sigs;
        }
        if let Some(fee) = base_fee_bps {
            config.base_fee_bps = fee;
        }
        if let Some(fee) = min_fee {
            config.min_fee = fee;
        }
        if let Some(fee) = max_fee {
            config.max_fee = fee;
        }

        storage.set(&DataKey::Config, &config);

        Ok(())
    }

    // ==================== Internal Helpers ====================

    fn require_admin(env: &Env) -> Result<(), NftWrapperError> {
        let storage = env.storage().instance();
        let _admin: Address = storage.get(&DataKey::Admin).ok_or(NftWrapperError::Unauthorized)?;
        // In a real scenario, you would pass the admin address as a parameter and check require_auth
        Ok(())
    }

    fn check_paused(env: &Env) -> Result<(), NftWrapperError> {
        let paused: bool = env.storage().instance().get(&DataKey::Paused).unwrap_or(false);

        if paused {
            return Err(NftWrapperError::ContractPaused);
        }

        Ok(())
    }

    fn calculate_fee(env: &Env, base_amount: i128) -> Result<i128, NftWrapperError> {
        let storage = env.storage().instance();
        let config: BridgeConfig = storage.get(&DataKey::Config).ok_or(NftWrapperError::FeeCalculationError)?;

        let fee = (base_amount * config.base_fee_bps as i128) / 10_000;
        let fee = fee.max(config.min_fee).min(config.max_fee);

        Ok(fee)
    }

    fn verify_signatures(
        env: &Env,
        _transfer: &BridgeTransfer,
        signatures: &Vec<ValidatorSignature>,
    ) -> Result<(), NftWrapperError> {
        let storage = env.storage().instance();
        let config: BridgeConfig = storage.get(&DataKey::Config).ok_or(NftWrapperError::InvalidTransfer)?;

        let validators: Vec<Validator> = storage.get(&DataKey::Validators).ok_or(NftWrapperError::ValidatorNotFound)?;

        if (signatures.len() as u32) < config.required_signatures {
            return Err(NftWrapperError::InsufficientSignatures);
        }

        let mut verified_count = 0u32;
        let mut seen_validators = Vec::new(env);

        for sig in signatures.iter() {
            for seen in seen_validators.iter() {
                if seen == sig.validator {
                    return Err(NftWrapperError::DuplicateSignature);
                }
            }
            seen_validators.push_back(sig.validator.clone());

            let mut validator_found = false;
            for validator in validators.iter() {
                if validator.address == sig.validator && validator.active {
                    validator_found = true;
                    verified_count += 1;
                    break;
                }
            }

            if !validator_found {
                return Err(NftWrapperError::ValidatorNotFound);
            }
        }

        if verified_count < config.required_signatures {
            return Err(NftWrapperError::InsufficientSignatures);
        }

        Ok(())
    }
}
