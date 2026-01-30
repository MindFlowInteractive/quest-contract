#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, IntoVal, Symbol, Val,
};

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::test_utils::{Contract, TestEnvBuilder};

    #[test]
    fn test_escrow_lifecycle() {
        let mut env = TestEnvBuilder::new().build();
        let escrow_contract = Contract::new(&mut env, "escrow");

        // Create test accounts
        let creator = env.new_tester();
        let party1 = env.new_tester();
        let party2 = env.new_tester();
        let arbitrator = env.new_tester();

        // Create escrow
        let escrow_id = escrow_contract
            .call(&mut env, "create_escrow", (
                creator.address(),
                vec![party1.address(), party2.address()],
                arbitrator.address(),
                300, // 5 minute timeout
                vec![
                    "Condition 1: Deliver product".to_string(),
                    "Condition 2: Verify quality".to_string(),
                ],
            ))
            .unwrap();

        // Verify escrow creation
        let escrow: EscrowAgreement = escrow_contract
            .call(&mut env, "get_escrow", (escrow_id,))
            .unwrap();
        assert_eq!(escrow.status, EscrowStatus::Created);
        assert_eq!(escrow.parties.len(), 2);
        assert_eq!(escrow.release_conditions.len(), 2);

        // Approve escrow
        escrow_contract
            .call(&mut env, "approve_escrow", (escrow_id,))
            .with_caller(party1.address());
        escrow_contract
            .call(&mut env, "approve_escrow", (escrow_id,))
            .with_caller(party2.address());

        // Verify approval
        let escrow: EscrowAgreement = escrow_contract
            .call(&mut env, "get_escrow", (escrow_id,))
            .unwrap();
        assert!(escrow.parties.iter().all(|p| p.approved));
        assert_eq!(escrow.status, EscrowStatus::Active);

        // Make deposits
        escrow_contract
            .call(&mut env, "deposit", (escrow_id, 100))
            .with_caller(party1.address());
        escrow_contract
            .call(&mut env, "deposit", (escrow_id, 100))
            .with_caller(party2.address());

        // Verify deposits
        let escrow: EscrowAgreement = escrow_contract
            .call(&mut env, "get_escrow", (escrow_id,))
            .unwrap();
        assert_eq!(escrow.total_deposit, 200);
        assert!(escrow.parties.iter().all(|p| p.deposit > 0));

        // Fulfill conditions
        escrow_contract
            .call(&mut env, "fulfill_condition", (escrow_id, 1, "Product delivered".to_string()));
        escrow_contract
            .call(&mut env, "fulfill_condition", (escrow_id, 2, "Quality verified".to_string()));

        // Verify conditions and release
        let escrow: EscrowAgreement = escrow_contract
            .call(&mut env, "get_escrow", (escrow_id,))
            .unwrap();
        assert!(escrow.release_conditions.iter().all(|c| c.fulfilled));
        assert_eq!(escrow.status, EscrowStatus::Released);

        // Check balances
        let party1_balance = env.get_balance(party1.address());
        let party2_balance = env.get_balance(party2.address());
        assert!(party1_balance > 0);
        assert!(party2_balance > 0);
    }

    #[test]
    fn test_dispute_resolution() {
        let mut env = TestEnvBuilder::new().build();
        let escrow_contract = Contract::new(&mut env, "escrow");

        let creator = env.new_tester();
        let party1 = env.new_tester();
        let party2 = env.new_tester();
        let arbitrator = env.new_tester();

        let escrow_id = escrow_contract
            .call(&mut env, "create_escrow", (
                creator.address(),
                vec![party1.address(), party2.address()],
                arbitrator.address(),
                300,
                vec!["Deliver product".to_string()],
            ))
            .unwrap();

        // Approve and deposit
        escrow_contract
            .call(&mut env, "approve_escrow", (escrow_id,))
            .with_caller(party1.address());
        escrow_contract
            .call(&mut env, "approve_escrow", (escrow_id,))
            .with_caller(party2.address());
        escrow_contract
            .call(&mut env, "deposit", (escrow_id, 100))
            .with_caller(party1.address());
        escrow_contract
            .call(&mut env, "deposit", (escrow_id, 100))
            .with_caller(party2.address());

        // Initiate dispute
        escrow_contract
            .call(&mut env, "initiate_dispute", (escrow_id, "Product not as described".to_string()))
            .with_caller(party1.address());

        // Verify dispute
        let escrow: EscrowAgreement = escrow_contract
            .call(&mut env, "get_escrow", (escrow_id,))
            .unwrap();
        assert_eq!(escrow.status, EscrowStatus::Disputed);
        assert_eq!(escrow.dispute_reason, Some("Product not as described".to_string()));

        // Arbitrator resolves
        escrow_contract
            .call(&mut env, "resolve_dispute", (escrow_id, "Full refund issued".to_string()))
            .with_caller(arbitrator.address());

        // Verify resolution
        let escrow: EscrowAgreement = escrow_contract
            .call(&mut env, "get_escrow", (escrow_id,))
            .unwrap();
        assert_eq!(escrow.status, EscrowStatus::Resolved);
        assert_eq!(escrow.resolution, Some("Full refund issued".to_string()));
    }

    #[test]
    fn test_timeout_refund() {
        let mut env = TestEnvBuilder::new().build();
        let escrow_contract = Contract::new(&mut env, "escrow");

        let creator = env.new_tester();
        let party1 = env.new_tester();
        let party2 = env.new_tester();
        let arbitrator = env.new_tester();

        let escrow_id = escrow_contract
            .call(&mut env, "create_escrow", (
                creator.address(),
                vec![party1.address(), party2.address()],
                arbitrator.address(),
                1, // 1 second timeout for test
                vec!["Deliver product".to_string()],
            ))
            .unwrap();

        // Approve and deposit
        escrow_contract
            .call(&mut env, "approve_escrow", (escrow_id,))
            .with_caller(party1.address());
        escrow_contract
            .call(&mut env, "approve_escrow", (escrow_id,))
            .with_caller(party2.address());
        escrow_contract
            .call(&mut env, "deposit", (escrow_id, 100))
            .with_caller(party1.address());
        escrow_contract
            .call(&mut env, "deposit", (escrow_id, 100))
            .with_caller(party2.address());

        // Fast forward time
        env.set_timestamp(env.current_timestamp() + 2);

        // Check timeout
        escrow_contract.call(&mut env, "check_timeout", (escrow_id,));

        // Verify refund
        let escrow: EscrowAgreement = escrow_contract
            .call(&mut env, "get_escrow", (escrow_id,))
            .unwrap();
        assert_eq!(escrow.status, EscrowStatus::Refunded);
        assert!(escrow.parties.iter().all(|p| p.deposit == 0));
    }

    #[test]
    fn test_partial_release() {
        let mut env = TestEnvBuilder::new().build();
        let escrow_contract = Contract::new(&mut env, "escrow");

        let creator = env.new_tester();
        let party1 = env.new_tester();
        let party2 = env.new_tester();
        let arbitrator = env.new_tester();

        let escrow_id = escrow_contract
            .call(&mut env, "create_escrow", (
                creator.address(),
                vec![party1.address(), party2.address()],
                arbitrator.address(),
                300,
                vec![
                    "Condition 1: Partial delivery".to_string(),
                    "Condition 2: Full delivery".to_string(),
                ],
            ))
            .unwrap();

        // Approve and deposit
        escrow_contract
            .call(&mut env, "approve_escrow", (escrow_id,))
            .with_caller(party1.address());
        escrow_contract
            .call(&mut env, "approve_escrow", (escrow_id,))
            .with_caller(party2.address());
        escrow_contract
            .call(&mut env, "deposit", (escrow_id, 200))
            .with_caller(party1.address());
        escrow_contract
            .call(&mut env, "deposit", (escrow_id, 200))
            .with_caller(party2.address());

        // Fulfill first condition only
        escrow_contract
            .call(&mut env, "fulfill_condition", (escrow_id, 1, "Partial delivery complete".to_string()));

        // Partial release
        escrow_contract.call(&mut env, "partial_release", (escrow_id,));

        // Verify partial release
        let escrow: EscrowAgreement = escrow_contract
            .call(&mut env, "get_escrow", (escrow_id,))
            .unwrap();
        assert!(escrow.parties.iter().any(|p| p.deposit < 200));
    }
}