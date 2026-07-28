#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env, Map, String, Vec, log};
use validation_lib::{validate_amount, validate_address, validate_range, ValidationError};

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
#[soroban_sdk::Serialize, soroban_sdk::Deserialize]
pub enum OperationStatus {
    Pending,
    Success,
    Failed,
    RolledBack,
}

#[derive(Debug, Clone, PartialEq)]
#[soroban_sdk::Serialize, soroban_sdk::Deserialize]
pub enum BatchError {
    ValidationFailed(ValidationError),
    InsufficientBalance,
    Unauthorized,
    BatchTooLarge,
    TokenNotFound,
    RollbackFailed,
    OperationAlreadyExecuted,
}

#[derive(Debug, Clone)]
#[soroban_sdk::Serialize, soroban_sdk::Deserialize]
pub struct MintOperation {
    pub to: Address,
    pub token_id: u128,
    pub amount: i128,
}

#[derive(Debug, Clone)]
#[soroban_sdk::Serialize, soroban_sdk::Deserialize]
pub struct TransferOperation {
    pub from: Address,
    pub to: Address,
    pub token_id: u128,
    pub amount: i128,
}

#[derive(Debug, Clone)]
#[soroban_sdk::Serialize, soroban_sdk::Deserialize]
pub struct BurnOperation {
    pub from: Address,
    pub token_id: u128,
    pub amount: i128,
}

#[derive(Debug, Clone)]
#[soroban_sdk::Serialize, soroban_sdk::Deserialize]
pub struct OperationResult {
    pub index: u32,
    pub status: OperationStatus,
    pub error: Option<String>,
    pub gas_used: u64,
}

#[derive(Debug, Clone)]
#[soroban_sdk::Serialize, soroban_sdk::Deserialize]
pub struct BatchExecution {
    pub batch_id: u64,
    pub operation_count: u32,
    pub successful_ops: u32,
    pub failed_ops: u32,
    pub rolled_back: bool,
    pub total_gas_used: u64,
    pub timestamp: u64,
    pub initiator: Address,
}

#[contract]
pub struct BatchMinting;

#[contractimpl]
impl BatchMinting {
    pub fn __constructor(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().persistent().set(&crate::types::DataKey::Admin, &admin);
    }

    // Batch mint multiple tokens in single transaction
    pub fn batch_mint(
        env: Env,
        operations: Vec<MintOperation>,
        admin: Address,
    ) -> Result<Vec<OperationResult>, BatchError> {
        admin.require_auth();
        let start_gas = env.gas_used();
        let start_time = env.ledger().timestamp();

        // Validate batch size first (gas optimization)
        validate_range!(operations.len() as u32, 1, 100);
        if operations.len() > 100 {
            return Err(BatchError::BatchTooLarge);
        }

        let mut results = Vec::new(&env);
        let mut successful = 0;
        let mut failed = 0;
        let mut rollback_operations = Vec::new(&env);

        // First pass: validate all operations
        for (idx, op) in operations.iter().enumerate() {
            let op_gas_start = env.gas_used();
            let result = || -> Result<(), BatchError> {
                // Validate inputs using our validation library
                validate_address!(&op.to);
                validate_amount!(op.amount, 1, i128::MAX);
                if op.token_id == 0 {
                    return Err(BatchError::TokenNotFound);
                }
                Ok(())
            }();

            match result {
                Ok(_) => {
                    // Execute the mint (in real implementation, call token contract)
                    rollback_operations.push((idx as u32, op.clone()));
                    
                    // Update storage for the recipient
                    let balance_key = crate::types::DataKey::Balance(op.to.clone(), op.token_id);
                    let current_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
                    env.storage().persistent().set(&balance_key, &(current_balance + op.amount));
                    
                    successful += 1;
                    results.push(OperationResult {
                        index: idx as u32,
                        status: OperationStatus::Success,
                        error: None,
                        gas_used: env.gas_used() - op_gas_start,
                    });
                }
                Err(e) => {
                    failed += 1;
                    let error_str = String::from_str(&env, &format!("{:?}", e));
                    results.push(OperationResult {
                        index: idx as u32,
                        status: OperationStatus::Failed,
                        error: Some(error_str),
                        gas_used: env.gas_used() - op_gas_start,
                    });
                }
            }
        }

        // Check if we need to rollback based on failure threshold
        let failure_rate = failed as f32 / operations.len() as f32;
        let mut rolled_back = false;
        
        if failure_rate > 0.5 {
            // More than 50% failed - rollback all successful operations
            if let Err(_) = Self::rollback_mint(&env, rollback_operations) {
                log!(&env, "Rollback failed for batch mint");
            } else {
                rolled_back = true;
                successful = 0;
                // Update results to reflect rollback
                for result in results.iter_mut() {
                    if result.status == OperationStatus::Success {
                        result.status = OperationStatus::RolledBack;
                    }
                }
            }
        }

        // Create execution record
        let batch_id = env.ledger().sequence() as u64;
        let execution = BatchExecution {
            batch_id,
            operation_count: operations.len() as u32,
            successful_ops: successful,
            failed_ops: failed,
            rolled_back,
            total_gas_used: env.gas_used() - start_gas,
            timestamp: start_time,
            initiator: admin,
        };

        // Store execution record
        env.storage().persistent().set(&crate::types::DataKey::Execution(batch_id), &execution);
        
        // Emit event
        env.events().publish(("batch_completed", batch_id), execution.clone());

        Ok(results)
    }

    // Batch transfer multiple tokens
    pub fn batch_transfer(
        env: Env,
        operations: Vec<TransferOperation>,
        initiator: Address,
    ) -> Result<Vec<OperationResult>, BatchError> {
        initiator.require_auth();
        let start_gas = env.gas_used();
        let start_time = env.ledger().timestamp();

        // Validate batch size
        if operations.len() > 100 {
            return Err(BatchError::BatchTooLarge);
        }

        let mut results = Vec::new(&env);
        let mut successful = 0;
        let mut failed = 0;
        let mut rollback_ops = Vec::new(&env);

        // Validate and execute all transfers
        for (idx, op) in operations.iter().enumerate() {
            let op_gas_start = env.gas_used();
            let result = || -> Result<(), BatchError> {
                validate_address!(&op.from);
                validate_address!(&op.to);
                validate_amount!(op.amount, 1, i128::MAX);
                
                // Check sender has sufficient balance
                let balance_key = crate::types::DataKey::Balance(op.from.clone(), op.token_id);
                let balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
                if balance < op.amount {
                    return Err(BatchError::InsufficientBalance);
                }
                
                // Ensure sender is authorized
                if !op.from == initiator {
                    return Err(BatchError::Unauthorized);
                }
                
                Ok(())
            }();

            match result {
                Ok(_) => {
                    // Execute transfer
                    let from_key = crate::types::DataKey::Balance(op.from.clone(), op.token_id);
                    let to_key = crate::types::DataKey::Balance(op.to.clone(), op.token_id);
                    
                    let from_balance: i128 = env.storage().persistent().get(&from_key).unwrap();
                    let to_balance: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);
                    
                    env.storage().persistent().set(&from_key, &(from_balance - op.amount));
                    env.storage().persistent().set(&to_key, &(to_balance + op.amount));
                    
                    rollback_ops.push((idx as u32, op.clone()));
                    successful += 1;
                    
                    results.push(OperationResult {
                        index: idx as u32,
                        status: OperationStatus::Success,
                        error: None,
                        gas_used: env.gas_used() - op_gas_start,
                    });
                }
                Err(e) => {
                    failed += 1;
                    let error_str = String::from_str(&env, &format!("{:?}", e));
                    results.push(OperationResult {
                        index: idx as u32,
                        status: OperationStatus::Failed,
                        error: Some(error_str),
                        gas_used: env.gas_used() - op_gas_start,
                    });
                }
            }
        }

        // Rollback if failure threshold exceeded
        let failure_rate = failed as f32 / operations.len() as f32;
        let mut rolled_back = false;
        
        if failure_rate > 0.5 {
            if let Err(_) = Self::rollback_transfer(&env, rollback_ops) {
                log!(&env, "Rollback failed for batch transfer");
            } else {
                rolled_back = true;
                successful = 0;
                for result in results.iter_mut() {
                    if result.status == OperationStatus::Success {
                        result.status = OperationStatus::RolledBack;
                    }
                }
            }
        }

        // Store execution record
        let batch_id = env.ledger().sequence() as u64;
        let execution = BatchExecution {
            batch_id,
            operation_count: operations.len() as u32,
            successful_ops: successful,
            failed_ops: failed,
            rolled_back,
            total_gas_used: env.gas_used() - start_gas,
            timestamp: start_time,
            initiator: initiator.clone(),
        };

        env.storage().persistent().set(&crate::types::DataKey::Execution(batch_id), &execution);
        env.events().publish(("batch_transfer_completed", batch_id), execution);

        Ok(results)
    }

    // Batch burn multiple tokens
    pub fn batch_burn(
        env: Env,
        operations: Vec<BurnOperation>,
        initiator: Address,
    ) -> Result<Vec<OperationResult>, BatchError> {
        initiator.require_auth();
        let start_gas = env.gas_used();
        let start_time = env.ledger().timestamp();

        if operations.len() > 100 {
            return Err(BatchError::BatchTooLarge);
        }

        let mut results = Vec::new(&env);
        let mut successful = 0;
        let mut failed = 0;
        let mut rollback_ops = Vec::new(&env);

        for (idx, op) in operations.iter().enumerate() {
            let op_gas_start = env.gas_used();
            let result = || -> Result<(), BatchError> {
                validate_address!(&op.from);
                validate_amount!(op.amount, 1, i128::MAX);
                
                let balance_key = crate::types::DataKey::Balance(op.from.clone(), op.token_id);
                let balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
                if balance < op.amount {
                    return Err(BatchError::InsufficientBalance);
                }
                
                if !op.from == initiator {
                    return Err(BatchError::Unauthorized);
                }
                
                Ok(())
            }();

            match result {
                Ok(_) => {
                    // Execute burn
                    let balance_key = crate::types::DataKey::Balance(op.from.clone(), op.token_id);
                    let current_balance: i128 = env.storage().persistent().get(&balance_key).unwrap();
                    env.storage().persistent().set(&balance_key, &(current_balance - op.amount));
                    
                    rollback_ops.push((idx as u32, op.clone()));
                    successful += 1;
                    
                    results.push(OperationResult {
                        index: idx as u32,
                        status: OperationStatus::Success,
                        error: None,
                        gas_used: env.gas_used() - op_gas_start,
                    });
                }
                Err(e) => {
                    failed += 1;
                    let error_str = String::from_str(&env, &format!("{:?}", e));
                    results.push(OperationResult {
                        index: idx as u32,
                        status: OperationStatus::Failed,
                        error: Some(error_str),
                        gas_used: env.gas_used() - op_gas_start,
                    });
                }
            }
        }

        let failure_rate = failed as f32 / operations.len() as f32;
        let mut rolled_back = false;
        
        if failure_rate > 0.5 {
            if let Err(_) = Self::rollback_burn(&env, rollback_ops) {
                log!(&env, "Rollback failed for batch burn");
            } else {
                rolled_back = true;
                successful = 0;
                for result in results.iter_mut() {
                    if result.status == OperationStatus::Success {
                        result.status = OperationStatus::RolledBack;
                    }
                }
            }
        }

        let batch_id = env.ledger().sequence() as u64;
        let execution = BatchExecution {
            batch_id,
            operation_count: operations.len() as u32,
            successful_ops: successful,
            failed_ops: failed,
            rolled_back,
            total_gas_used: env.gas_used() - start_gas,
            timestamp: start_time,
            initiator,
        };

        env.storage().persistent().set(&crate::types::DataKey::Execution(batch_id), &execution);
        env.events().publish(("batch_burn_completed", batch_id), execution);

        Ok(results)
    }

    // Rollback implementation for mint operations
    fn rollback_mint(env: &Env, operations: Vec<(u32, MintOperation)>) -> Result<(), BatchError> {
        for (_, op) in operations.iter() {
            let balance_key = crate::types::DataKey::Balance(op.to.clone(), op.token_id);
            let current_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
            if current_balance >= op.amount {
                env.storage().persistent().set(&balance_key, &(current_balance - op.amount));
            }
        }
        Ok(())
    }

    // Rollback implementation for transfer operations
    fn rollback_transfer(env: &Env, operations: Vec<(u32, TransferOperation)>) -> Result<(), BatchError> {
        for (_, op) in operations.iter() {
            let from_key = crate::types::DataKey::Balance(op.from.clone(), op.token_id);
            let to_key = crate::types::DataKey::Balance(op.to.clone(), op.token_id);
            
            let from_balance: i128 = env.storage().persistent().get(&from_key).unwrap_or(0);
            let to_balance: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);
            
            if to_balance >= op.amount {
                env.storage().persistent().set(&from_key, &(from_balance + op.amount));
                env.storage().persistent().set(&to_key, &(to_balance - op.amount));
            }
        }
        Ok(())
    }

    // Rollback implementation for burn operations
    fn rollback_burn(env: &Env, operations: Vec<(u32, BurnOperation)>) -> Result<(), BatchError> {
        for (_, op) in operations.iter() {
            let balance_key = crate::types::DataKey::Balance(op.from.clone(), op.token_id);
            let current_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
            env.storage().persistent().set(&balance_key, &(current_balance + op.amount));
        }
        Ok(())
    }

    // Get execution record for tracking
    pub fn get_execution(env: Env, batch_id: u64) -> Option<BatchExecution> {
        env.storage().persistent().get(&crate::types::DataKey::Execution(batch_id))
    }

    // Get batch results for partial success inspection
    pub fn get_balance(env: Env, address: Address, token_id: u128) -> i128 {
        env.storage().persistent().get(&crate::types::DataKey::Balance(address, token_id)).unwrap_or(0)
    }
}

// Storage data keys
mod types {
    use super::*;

    #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[soroban_sdk::Serialize, soroban_sdk::Deserialize]
    pub enum DataKey {
        Admin,
        Balance(Address, u128),
        Execution(u64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;
    use soroban_sdk::arbitrary::Arbitrary;

    #[test]
    fn test_batch_mint_success() {
        let env = Env::default();
        let admin = Address::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF");
        
        // Initialize contract
        BatchMinting::__constructor(env.clone(), admin.clone());
        
        // Create test mint operations
        let mut ops = Vec::new(&env);
        ops.push(MintOperation {
            to: Address::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"),
            token_id: 1,
            amount: 100,
        });
        
        // Execute batch mint
        let result = BatchMinting::batch_mint(env.clone(), ops, admin.clone());
        assert!(result.is_ok());
        
        let results = result.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, OperationStatus::Success);
        
        // Check balance was updated
        let balance = BatchMinting::get_balance(
            env.clone(), 
            Address::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"), 
            1
        );
        assert_eq!(balance, 100);
    }

    #[test]
    fn test_batch_mint_rollback() {
        let env = Env::default();
        let admin = Address::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF");
        
        BatchMinting::__constructor(env.clone(), admin.clone());
        
        // Create mix of valid and invalid operations to trigger rollback
        let mut ops = Vec::new(&env);
        // Valid operation
        ops.push(MintOperation {
            to: Address::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"),
            token_id: 1,
            amount: 100,
        });
        // Invalid operation (token_id 0)
        ops.push(MintOperation {
            to: Address::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"),
            token_id: 0,
            amount: 100,
        });
        // Another invalid operation
        ops.push(MintOperation {
            to: Address::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"), 
            token_id: 0,
            amount: 100,
        });
        
        let result = BatchMinting::batch_mint(env.clone(), ops, admin.clone());
        assert!(result.is_ok());
        
        let results = result.unwrap();
        // Check that successful operations were rolled back
        let success_ops: Vec<_> = results.iter().filter(|r| r.status == OperationStatus::Success).collect();
        let rolled_back_ops: Vec<_> = results.iter().filter(|r| r.status == OperationStatus::RolledBack).collect();
        
        assert_eq!(rolled_back_ops.len(), 1);
        assert_eq!(success_ops.len(), 0);
        
        // Balance should be 0 due to rollback
        let balance = BatchMinting::get_balance(
            env.clone(),
            Address::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"),
            1
        );
        assert_eq!(balance, 0);
    }

    #[test]
    fn test_batch_transfer() {
        let env = Env::default();
        let admin = Address::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF");
        let recipient = Address::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHX");
        
        BatchMinting::__constructor(env.clone(), admin.clone());
        
        // First mint some tokens to admin
        let mut mint_ops = Vec::new(&env);
        mint_ops.push(MintOperation {
            to: admin.clone(),
            token_id: 1,
            amount: 1000,
        });
        BatchMinting::batch_mint(env.clone(), mint_ops, admin.clone()).unwrap();
        
        // Now transfer some tokens
        let mut transfer_ops = Vec::new(&env);
        transfer_ops.push(TransferOperation {
            from: admin.clone(),
            to: recipient.clone(),
            token_id: 1,
            amount: 500,
        });
        
        let result = BatchMinting::batch_transfer(env.clone(), transfer_ops, admin.clone());
        assert!(result.is_ok());
        
        let results = result.unwrap();
        assert_eq!(results[0].status, OperationStatus::Success);
        
        // Check balances
        let admin_balance = BatchMinting::get_balance(env.clone(), admin.clone(), 1);
        let recipient_balance = BatchMinting::get_balance(env.clone(), recipient.clone(), 1);
        assert_eq!(admin_balance, 500);
        assert_eq!(recipient_balance, 500);
    }

    #[test]
    fn test_batch_burn() {
        let env = Env::default();
        let admin = Address::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF");
        
        BatchMinting::__constructor(env.clone(), admin.clone());
        
        // Mint tokens first
        let mut mint_ops = Vec::new(&env);
        mint_ops.push(MintOperation {
            to: admin.clone(),
            token_id: 1,
            amount: 1000,
        });
        BatchMinting::batch_mint(env.clone(), mint_ops, admin.clone()).unwrap();
        
        // Burn tokens
        let mut burn_ops = Vec::new(&env);
        burn_ops.push(BurnOperation {
            from: admin.clone(),
            token_id: 1,
            amount: 300,
        });
        
        let result = BatchMinting::batch_burn(env.clone(), burn_ops, admin.clone());
        assert!(result.is_ok());
        
        let results = result.unwrap();
        assert_eq!(results[0].status, OperationStatus::Success);
        
        // Check remaining balance
        let balance = BatchMinting::get_balance(env.clone(), admin.clone(), 1);
        assert_eq!(balance, 700);
    }

    #[test]
    fn test_execution_tracking() {
        let env = Env::default();
        let admin = Address::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF");
        
        BatchMinting::__constructor(env.clone(), admin.clone());
        
        let mut mint_ops = Vec::new(&env);
        mint_ops.push(MintOperation {
            to: admin.clone(),
            token_id: 1,
            amount: 100,
        });
        
        let result = BatchMinting::batch_mint(env.clone(), mint_ops, admin.clone()).unwrap();
        
        // Get the execution record that was created
        let batch_id = env.ledger().sequence() as u64;
        let execution = BatchMinting::get_execution(env.clone(), batch_id);
        assert!(execution.is_some());
        
        let exec = execution.unwrap();
        assert_eq!(exec.successful_ops, 1);
        assert_eq!(exec.failed_ops, 0);
        assert_eq!(exec.operation_count, 1);
        assert!(!exec.rolled_back);
    }
}