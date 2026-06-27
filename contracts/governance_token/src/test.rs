#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, IntoVal, String, Symbol, Val, Vec,
};

// ───────────────────────────────────────────────────────
// Mock target contract used as the proposal action target
// ───────────────────────────────────────────────────────

#[contract]
pub struct MockTarget;

#[contractimpl]
impl MockTarget {
    pub fn do_something(env: Env, value: u64) -> u64 {
        value
    }
}

// ───────────────────────────────────────────────────────
// Helper: set up a fresh governance token contract
// ───────────────────────────────────────────────────────

struct Setup {
    env: Env,
    contract_id: Address,
    client: GovernanceTokenClient<'static>,
    admin: Address,
}

fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, GovernanceToken);
    let client = GovernanceTokenClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(
        &admin,
        &String::from_str(&env, "Governance Token"),
        &String::from_str(&env, "GOV"),
        &6,
        &100,  // voting_delay
        &1000, // voting_period
        &100,  // proposal_threshold
        &10,   // quorum_numerator (10%)
        &200,  // timelock_delay
        &600,  // grace_period
    );

    Setup {
        env,
        contract_id,
        client,
        admin,
    }
}

// ═══════════════════════════════════════════════════════
// 1. Token basics
// ═══════════════════════════════════════════════════════

#[test]
fn test_metadata() {
    let s = setup();
    assert_eq!(s.client.name(), String::from_str(&s.env, "Governance Token"));
    assert_eq!(s.client.symbol(), String::from_str(&s.env, "GOV"));
    assert_eq!(s.client.decimals(), 6);
    assert_eq!(s.client.total_supply(), 0);
}

#[test]
fn test_mint() {
    let s = setup();
    let user = Address::generate(&s.env);

    s.client.mint(&s.admin, &user, &1000);

    assert_eq!(s.client.balance(&user), 1000);
    assert_eq!(s.client.total_supply(), 1000);
}

#[test]
fn test_burn() {
    let s = setup();
    let user = Address::generate(&s.env);

    s.client.mint(&s.admin, &user, &1000);
    s.client.burn(&user, &300);

    assert_eq!(s.client.balance(&user), 700);
    assert_eq!(s.client.total_supply(), 700);
}

#[test]
fn test_transfer() {
    let s = setup();
    let u1 = Address::generate(&s.env);
    let u2 = Address::generate(&s.env);

    s.client.mint(&s.admin, &u1, &1000);
    s.client.transfer(&u1, &u2, &400);

    assert_eq!(s.client.balance(&u1), 600);
    assert_eq!(s.client.balance(&u2), 400);
    assert_eq!(s.client.total_supply(), 1000);
}

#[test]
fn test_approve_and_transfer_from() {
    let s = setup();
    let owner = Address::generate(&s.env);
    let spender = Address::generate(&s.env);
    let recipient = Address::generate(&s.env);

    s.client.mint(&s.admin, &owner, &1000);
    s.client.approve(&owner, &spender, &500);
    assert_eq!(s.client.allowance(&owner, &spender), 500);

    s.client.transfer_from(&spender, &owner, &recipient, &200);

    assert_eq!(s.client.balance(&owner), 800);
    assert_eq!(s.client.balance(&recipient), 200);
    assert_eq!(s.client.allowance(&owner, &spender), 300);
}

#[test]
#[should_panic(expected = "Insufficient balance")]
fn test_transfer_insufficient() {
    let s = setup();
    let u1 = Address::generate(&s.env);
    let u2 = Address::generate(&s.env);

    s.client.mint(&s.admin, &u1, &100);
    s.client.transfer(&u1, &u2, &200);
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_double_initialize() {
    let s = setup();
    s.client.initialize(
        &s.admin,
        &String::from_str(&s.env, "X"),
        &String::from_str(&s.env, "X"),
        &6,
        &100,
        &1000,
        &100,
        &10,
        &200,
        &600,
    );
}

// ═══════════════════════════════════════════════════════
// 2. Delegation
// ═══════════════════════════════════════════════════════

#[test]
fn test_self_delegate() {
    let s = setup();
    let user = Address::generate(&s.env);

    s.client.mint(&s.admin, &user, &500);
    // Before explicit delegation the default delegate is the user
    assert_eq!(s.client.get_voting_power(&user), 500);

    // Self‑delegate explicitly — no change
    s.client.delegate(&user, &user);
    assert_eq!(s.client.get_voting_power(&user), 500);
}

#[test]
fn test_delegate_to_another() {
    let s = setup();
    let u1 = Address::generate(&s.env);
    let u2 = Address::generate(&s.env);

    s.client.mint(&s.admin, &u1, &500);
    assert_eq!(s.client.get_voting_power(&u1), 500);
    assert_eq!(s.client.get_voting_power(&u2), 0);

    // u1 delegates to u2
    s.client.delegate(&u1, &u2);

    assert_eq!(s.client.get_voting_power(&u1), 0);
    assert_eq!(s.client.get_voting_power(&u2), 500);
}

#[test]
fn test_re_delegate() {
    let s = setup();
    let u1 = Address::generate(&s.env);
    let u2 = Address::generate(&s.env);
    let u3 = Address::generate(&s.env);

    s.client.mint(&s.admin, &u1, &500);
    s.client.delegate(&u1, &u2);
    assert_eq!(s.client.get_voting_power(&u2), 500);

    // Re‑delegate from u2 to u3
    s.client.delegate(&u1, &u3);
    assert_eq!(s.client.get_voting_power(&u2), 0);
    assert_eq!(s.client.get_voting_power(&u3), 500);
}

#[test]
fn test_delegation_with_transfer() {
    let s = setup();
    let u1 = Address::generate(&s.env);
    let u2 = Address::generate(&s.env);
    let u3 = Address::generate(&s.env);

    s.client.mint(&s.admin, &u1, &1000);
    s.client.delegate(&u1, &u3); // u1's power goes to u3
    assert_eq!(s.client.get_voting_power(&u3), 1000);

    // u1 transfers 400 to u2 (u2 has no explicit delegate → self)
    s.client.transfer(&u1, &u2, &400);

    // u3 should lose 400, u2 gains 400 (self‑delegated)
    assert_eq!(s.client.get_voting_power(&u3), 600);
    assert_eq!(s.client.get_voting_power(&u2), 400);
}

#[test]
fn test_mint_after_delegation() {
    let s = setup();
    let u1 = Address::generate(&s.env);
    let u2 = Address::generate(&s.env);

    // u1 delegates to u2 first (with 0 balance)
    s.client.delegate(&u1, &u2);

    // Now mint to u1 — power should go to u2
    s.client.mint(&s.admin, &u1, &300);
    assert_eq!(s.client.balance(&u1), 300);
    assert_eq!(s.client.get_voting_power(&u1), 0);
    assert_eq!(s.client.get_voting_power(&u2), 300);
}

// ═══════════════════════════════════════════════════════
// 3. Vote weight snapshots
// ═══════════════════════════════════════════════════════

#[test]
fn test_checkpoints_written() {
    let s = setup();
    let user = Address::generate(&s.env);

    s.client.mint(&s.admin, &user, &500);
    assert_eq!(s.client.get_num_checkpoints(&user), 1);

    // Advance ledger sequence and mint more
    s.env.ledger().with_mut(|li| li.sequence_number += 1);
    s.client.mint(&s.admin, &user, &300);
    assert_eq!(s.client.get_num_checkpoints(&user), 2);
    assert_eq!(s.client.get_voting_power(&user), 800);
}

#[test]
fn test_get_past_votes() {
    let s = setup();
    let user = Address::generate(&s.env);

    // seq 0: mint 500
    s.client.mint(&s.admin, &user, &500);

    // seq 1: mint 300
    s.env.ledger().with_mut(|li| li.sequence_number += 1);
    s.client.mint(&s.admin, &user, &300);

    // seq 2: burn 200
    s.env.ledger().with_mut(|li| li.sequence_number += 1);
    s.client.burn(&user, &200);

    // Advance to seq 3 so we can query seq 0,1,2
    s.env.ledger().with_mut(|li| li.sequence_number += 1);

    assert_eq!(s.client.get_past_votes(&user, &0), 500);
    assert_eq!(s.client.get_past_votes(&user, &1), 800);
    assert_eq!(s.client.get_past_votes(&user, &2), 600);
}

#[test]
fn test_same_block_checkpoint_overwrite() {
    let s = setup();
    let user = Address::generate(&s.env);

    // Two mints in same ledger sequence → only one checkpoint
    s.client.mint(&s.admin, &user, &500);
    s.client.mint(&s.admin, &user, &300);

    assert_eq!(s.client.get_num_checkpoints(&user), 1);
    assert_eq!(s.client.get_voting_power(&user), 800);
}

// ═══════════════════════════════════════════════════════
// 4. Full proposal lifecycle (happy path)
// ═══════════════════════════════════════════════════════

#[test]
fn test_full_proposal_lifecycle() {
    let s = setup();
    let proposer = Address::generate(&s.env);
    let voter2 = Address::generate(&s.env);
    let target_id = s.env.register_contract(None, MockTarget);

    // Mint enough to meet proposal threshold
    s.client.mint(&s.admin, &proposer, &500);
    s.client.mint(&s.admin, &voter2, &200);

    // Need to be on a fresh sequence so get_past_votes works
    s.env.ledger().with_mut(|li| li.sequence_number += 1);

    let action = ProposalActionInput {
        contract_id: target_id.clone(),
        function_name: Symbol::new(&s.env, "do_something"),
        args: Vec::from_array(&s.env, [42u64.into_val(&s.env)]),
    };

    let pid = s.client.propose(
        &proposer,
        &String::from_str(&s.env, "Test Proposal"),
        &String::from_str(&s.env, "Do something"),
        &action,
        &0,
    );

    let p = s.client.get_proposal_info(&pid);
    assert_eq!(p.status, ProposalStatus::Pending);

    // Advance past voting_delay (100s)
    s.env.ledger().with_mut(|li| {
        li.timestamp += 200;
        li.sequence_number += 1;
    });

    // Vote
    s.client.vote(&proposer, &pid, &VoteType::For);
    s.client.vote(&voter2, &pid, &VoteType::For);

    let p = s.client.get_proposal_info(&pid);
    assert_eq!(p.for_votes, 700); // 500 + 200
    assert_eq!(p.status, ProposalStatus::Active);

    // Advance past voting_period
    s.env.ledger().with_mut(|li| {
        li.timestamp += 1100;
        li.sequence_number += 1;
    });

    // Queue
    s.client.queue(&pid);
    let p = s.client.get_proposal_info(&pid);
    assert_eq!(p.status, ProposalStatus::Queued);
    assert!(p.eta > 0);

    // Advance past timelock_delay (200s)
    s.env.ledger().with_mut(|li| {
        li.timestamp += 300;
        li.sequence_number += 1;
    });

    // Execute
    s.client.execute(&pid);
    let p = s.client.get_proposal_info(&pid);
    assert_eq!(p.status, ProposalStatus::Executed);
}

// ═══════════════════════════════════════════════════════
// 5. Proposal defeated (quorum not reached)
// ═══════════════════════════════════════════════════════

#[test]
#[should_panic(expected = "Quorum not reached")]
fn test_proposal_quorum_not_reached() {
    let s = setup();
    let proposer = Address::generate(&s.env);
    let target_id = s.env.register_contract(None, MockTarget);

    // Mint a lot to one user, but only the proposer votes
    s.client.mint(&s.admin, &proposer, &500);
    let whale = Address::generate(&s.env);
    s.client.mint(&s.admin, &whale, &10000); // huge supply

    s.env.ledger().with_mut(|li| li.sequence_number += 1);

    let action = ProposalActionInput {
        contract_id: target_id,
        function_name: Symbol::new(&s.env, "do_something"),
        args: Vec::from_array(&s.env, [1u64.into_val(&s.env)]),
    };

    let pid = s.client.propose(
        &proposer,
        &String::from_str(&s.env, "Low Quorum"),
        &String::from_str(&s.env, "Will fail quorum"),
        &action,
        &0,
    );

    s.env.ledger().with_mut(|li| {
        li.timestamp += 200;
        li.sequence_number += 1;
    });

    s.client.vote(&proposer, &pid, &VoteType::For);

    s.env.ledger().with_mut(|li| {
        li.timestamp += 1100;
        li.sequence_number += 1;
    });

    // Should panic with quorum not reached
    s.client.queue(&pid);
}

// ═══════════════════════════════════════════════════════
// 6. Proposal defeated (more against votes)
// ═══════════════════════════════════════════════════════

#[test]
#[should_panic(expected = "Proposal defeated")]
fn test_proposal_defeated_by_votes() {
    let s = setup();
    let proposer = Address::generate(&s.env);
    let opposer = Address::generate(&s.env);
    let target_id = s.env.register_contract(None, MockTarget);

    s.client.mint(&s.admin, &proposer, &500);
    s.client.mint(&s.admin, &opposer, &600);

    s.env.ledger().with_mut(|li| li.sequence_number += 1);

    let action = ProposalActionInput {
        contract_id: target_id,
        function_name: Symbol::new(&s.env, "do_something"),
        args: Vec::from_array(&s.env, [1u64.into_val(&s.env)]),
    };

    let pid = s.client.propose(
        &proposer,
        &String::from_str(&s.env, "Will Fail"),
        &String::from_str(&s.env, "Defeated"),
        &action,
        &0,
    );

    s.env.ledger().with_mut(|li| {
        li.timestamp += 200;
        li.sequence_number += 1;
    });

    s.client.vote(&proposer, &pid, &VoteType::For);
    s.client.vote(&opposer, &pid, &VoteType::Against);

    s.env.ledger().with_mut(|li| {
        li.timestamp += 1100;
        li.sequence_number += 1;
    });

    s.client.queue(&pid);
}

// ═══════════════════════════════════════════════════════
// 7. Timelock enforcement
// ═══════════════════════════════════════════════════════

#[test]
#[should_panic(expected = "Timelock not elapsed")]
fn test_execute_before_timelock() {
    let s = setup();
    let proposer = Address::generate(&s.env);
    let target_id = s.env.register_contract(None, MockTarget);

    s.client.mint(&s.admin, &proposer, &500);
    s.env.ledger().with_mut(|li| li.sequence_number += 1);

    let action = ProposalActionInput {
        contract_id: target_id,
        function_name: Symbol::new(&s.env, "do_something"),
        args: Vec::from_array(&s.env, [1u64.into_val(&s.env)]),
    };

    let pid = s.client.propose(
        &proposer,
        &String::from_str(&s.env, "Timelock Test"),
        &String::from_str(&s.env, "Desc"),
        &action,
        &0,
    );

    s.env.ledger().with_mut(|li| {
        li.timestamp += 200;
        li.sequence_number += 1;
    });
    s.client.vote(&proposer, &pid, &VoteType::For);

    s.env.ledger().with_mut(|li| {
        li.timestamp += 1100;
        li.sequence_number += 1;
    });
    s.client.queue(&pid);

    // Try to execute immediately — timelock not elapsed
    s.client.execute(&pid);
}

#[test]
#[should_panic(expected = "Grace period expired")]
fn test_execute_after_grace_period() {
    let s = setup();
    let proposer = Address::generate(&s.env);
    let target_id = s.env.register_contract(None, MockTarget);

    s.client.mint(&s.admin, &proposer, &500);
    s.env.ledger().with_mut(|li| li.sequence_number += 1);

    let action = ProposalActionInput {
        contract_id: target_id,
        function_name: Symbol::new(&s.env, "do_something"),
        args: Vec::from_array(&s.env, [1u64.into_val(&s.env)]),
    };

    let pid = s.client.propose(
        &proposer,
        &String::from_str(&s.env, "Grace Test"),
        &String::from_str(&s.env, "Desc"),
        &action,
        &0,
    );

    s.env.ledger().with_mut(|li| {
        li.timestamp += 200;
        li.sequence_number += 1;
    });
    s.client.vote(&proposer, &pid, &VoteType::For);

    s.env.ledger().with_mut(|li| {
        li.timestamp += 1100;
        li.sequence_number += 1;
    });
    s.client.queue(&pid);

    // Way past grace period (timelock_delay=200, grace_period=600 → expire after 800)
    s.env.ledger().with_mut(|li| {
        li.timestamp += 1000;
        li.sequence_number += 1;
    });
    s.client.execute(&pid);
}

// ═══════════════════════════════════════════════════════
// 8. Voting edge cases
// ═══════════════════════════════════════════════════════

#[test]
#[should_panic(expected = "Already voted")]
fn test_double_vote() {
    let s = setup();
    let proposer = Address::generate(&s.env);
    let target_id = s.env.register_contract(None, MockTarget);

    s.client.mint(&s.admin, &proposer, &500);
    s.env.ledger().with_mut(|li| li.sequence_number += 1);

    let action = ProposalActionInput {
        contract_id: target_id,
        function_name: Symbol::new(&s.env, "do_something"),
        args: Vec::from_array(&s.env, [1u64.into_val(&s.env)]),
    };

    let pid = s.client.propose(
        &proposer,
        &String::from_str(&s.env, "Dbl Vote"),
        &String::from_str(&s.env, "Desc"),
        &action,
        &0,
    );

    s.env.ledger().with_mut(|li| {
        li.timestamp += 200;
        li.sequence_number += 1;
    });

    s.client.vote(&proposer, &pid, &VoteType::For);
    s.client.vote(&proposer, &pid, &VoteType::Against); // should panic
}

#[test]
#[should_panic(expected = "Voting has not started")]
fn test_vote_before_start() {
    let s = setup();
    let proposer = Address::generate(&s.env);
    let target_id = s.env.register_contract(None, MockTarget);

    s.client.mint(&s.admin, &proposer, &500);
    s.env.ledger().with_mut(|li| li.sequence_number += 1);

    let action = ProposalActionInput {
        contract_id: target_id,
        function_name: Symbol::new(&s.env, "do_something"),
        args: Vec::from_array(&s.env, [1u64.into_val(&s.env)]),
    };

    let pid = s.client.propose(
        &proposer,
        &String::from_str(&s.env, "Early Vote"),
        &String::from_str(&s.env, "Desc"),
        &action,
        &0,
    );

    // Don't advance time — voting hasn't started
    s.client.vote(&proposer, &pid, &VoteType::For);
}

#[test]
#[should_panic(expected = "Voting has ended")]
fn test_vote_after_end() {
    let s = setup();
    let proposer = Address::generate(&s.env);
    let target_id = s.env.register_contract(None, MockTarget);

    s.client.mint(&s.admin, &proposer, &500);
    s.env.ledger().with_mut(|li| li.sequence_number += 1);

    let action = ProposalActionInput {
        contract_id: target_id,
        function_name: Symbol::new(&s.env, "do_something"),
        args: Vec::from_array(&s.env, [1u64.into_val(&s.env)]),
    };

    let pid = s.client.propose(
        &proposer,
        &String::from_str(&s.env, "Late Vote"),
        &String::from_str(&s.env, "Desc"),
        &action,
        &0,
    );

    // Way past voting end
    s.env.ledger().with_mut(|li| {
        li.timestamp += 5000;
        li.sequence_number += 1;
    });

    s.client.vote(&proposer, &pid, &VoteType::For);
}

#[test]
#[should_panic(expected = "Insufficient voting power to propose")]
fn test_propose_insufficient_power() {
    let s = setup();
    let user = Address::generate(&s.env);
    let target_id = s.env.register_contract(None, MockTarget);

    // Mint less than threshold (100)
    s.client.mint(&s.admin, &user, &50);

    let action = ProposalActionInput {
        contract_id: target_id,
        function_name: Symbol::new(&s.env, "do_something"),
        args: Vec::from_array(&s.env, [1u64.into_val(&s.env)]),
    };

    s.client.propose(
        &user,
        &String::from_str(&s.env, "Fail"),
        &String::from_str(&s.env, "Desc"),
        &action,
        &0,
    );
}

// ═══════════════════════════════════════════════════════
// 9. Cancel
// ═══════════════════════════════════════════════════════

#[test]
fn test_cancel_proposal() {
    let s = setup();
    let proposer = Address::generate(&s.env);
    let target_id = s.env.register_contract(None, MockTarget);

    s.client.mint(&s.admin, &proposer, &500);
    s.env.ledger().with_mut(|li| li.sequence_number += 1);

    let action = ProposalActionInput {
        contract_id: target_id,
        function_name: Symbol::new(&s.env, "do_something"),
        args: Vec::from_array(&s.env, [1u64.into_val(&s.env)]),
    };

    let pid = s.client.propose(
        &proposer,
        &String::from_str(&s.env, "Cancel Me"),
        &String::from_str(&s.env, "Desc"),
        &action,
        &0,
    );

    s.client.cancel(&proposer, &pid);
    let p = s.client.get_proposal_info(&pid);
    assert_eq!(p.status, ProposalStatus::Canceled);
}

#[test]
#[should_panic(expected = "Voting already started")]
fn test_cancel_after_voting_starts() {
    let s = setup();
    let proposer = Address::generate(&s.env);
    let target_id = s.env.register_contract(None, MockTarget);

    s.client.mint(&s.admin, &proposer, &500);
    s.env.ledger().with_mut(|li| li.sequence_number += 1);

    let action = ProposalActionInput {
        contract_id: target_id,
        function_name: Symbol::new(&s.env, "do_something"),
        args: Vec::from_array(&s.env, [1u64.into_val(&s.env)]),
    };

    let pid = s.client.propose(
        &proposer,
        &String::from_str(&s.env, "Late Cancel"),
        &String::from_str(&s.env, "Desc"),
        &action,
        &0,
    );

    s.env.ledger().with_mut(|li| {
        li.timestamp += 200;
        li.sequence_number += 1;
    });

    s.client.cancel(&proposer, &pid);
}

#[test]
#[should_panic(expected = "Not proposer")]
fn test_cancel_not_proposer() {
    let s = setup();
    let proposer = Address::generate(&s.env);
    let other = Address::generate(&s.env);
    let target_id = s.env.register_contract(None, MockTarget);

    s.client.mint(&s.admin, &proposer, &500);
    s.env.ledger().with_mut(|li| li.sequence_number += 1);

    let action = ProposalActionInput {
        contract_id: target_id,
        function_name: Symbol::new(&s.env, "do_something"),
        args: Vec::from_array(&s.env, [1u64.into_val(&s.env)]),
    };

    let pid = s.client.propose(
        &proposer,
        &String::from_str(&s.env, "Not Mine"),
        &String::from_str(&s.env, "Desc"),
        &action,
        &0,
    );

    s.client.cancel(&other, &pid);
}

// ═══════════════════════════════════════════════════════
// 10. Vote receipt
// ═══════════════════════════════════════════════════════

#[test]
fn test_vote_receipt() {
    let s = setup();
    let proposer = Address::generate(&s.env);
    let target_id = s.env.register_contract(None, MockTarget);

    s.client.mint(&s.admin, &proposer, &500);
    s.env.ledger().with_mut(|li| li.sequence_number += 1);

    let action = ProposalActionInput {
        contract_id: target_id,
        function_name: Symbol::new(&s.env, "do_something"),
        args: Vec::from_array(&s.env, [1u64.into_val(&s.env)]),
    };

    let pid = s.client.propose(
        &proposer,
        &String::from_str(&s.env, "Receipt Test"),
        &String::from_str(&s.env, "Desc"),
        &action,
        &0,
    );

    s.env.ledger().with_mut(|li| {
        li.timestamp += 200;
        li.sequence_number += 1;
    });

    s.client.vote(&proposer, &pid, &VoteType::Against);

    let receipt = s.client.get_vote_receipt(&pid, &proposer);
    assert_eq!(receipt.vote_type, VoteType::Against);
    assert_eq!(receipt.weight, 500);
}

// ═══════════════════════════════════════════════════════
// 11. Delegation with proposal voting (snapshot)
// ═══════════════════════════════════════════════════════

#[test]
fn test_delegation_snapshot_voting() {
    let s = setup();
    let u1 = Address::generate(&s.env);
    let u2 = Address::generate(&s.env);
    let target_id = s.env.register_contract(None, MockTarget);

    // u1 has 500, u2 has 200
    s.client.mint(&s.admin, &u1, &500);
    s.client.mint(&s.admin, &u2, &200);

    // u2 delegates to u1 → u1 has 700 power
    s.client.delegate(&u2, &u1);
    assert_eq!(s.client.get_voting_power(&u1), 700);

    s.env.ledger().with_mut(|li| li.sequence_number += 1);

    // Create proposal
    let action = ProposalActionInput {
        contract_id: target_id,
        function_name: Symbol::new(&s.env, "do_something"),
        args: Vec::from_array(&s.env, [1u64.into_val(&s.env)]),
    };

    let pid = s.client.propose(
        &u1,
        &String::from_str(&s.env, "Deleg Vote"),
        &String::from_str(&s.env, "Desc"),
        &action,
        &0,
    );

    s.env.ledger().with_mut(|li| {
        li.timestamp += 200;
        li.sequence_number += 1;
    });

    // u1 votes with 700 power (own 500 + u2's delegated 200)
    s.client.vote(&u1, &pid, &VoteType::For);

    let p = s.client.get_proposal_info(&pid);
    assert_eq!(p.for_votes, 700);
}

// ═══════════════════════════════════════════════════════
// 12. Abstain votes
// ═══════════════════════════════════════════════════════

#[test]
fn test_abstain_vote() {
    let s = setup();
    let proposer = Address::generate(&s.env);
    let abstainer = Address::generate(&s.env);
    let target_id = s.env.register_contract(None, MockTarget);

    s.client.mint(&s.admin, &proposer, &500);
    s.client.mint(&s.admin, &abstainer, &300);

    s.env.ledger().with_mut(|li| li.sequence_number += 1);

    let action = ProposalActionInput {
        contract_id: target_id,
        function_name: Symbol::new(&s.env, "do_something"),
        args: Vec::from_array(&s.env, [1u64.into_val(&s.env)]),
    };

    let pid = s.client.propose(
        &proposer,
        &String::from_str(&s.env, "Abstain Test"),
        &String::from_str(&s.env, "Desc"),
        &action,
        &0,
    );

    s.env.ledger().with_mut(|li| {
        li.timestamp += 200;
        li.sequence_number += 1;
    });

    s.client.vote(&proposer, &pid, &VoteType::For);
    s.client.vote(&abstainer, &pid, &VoteType::Abstain);

    let p = s.client.get_proposal_info(&pid);
    assert_eq!(p.for_votes, 500);
    assert_eq!(p.abstain_votes, 300);
    assert_eq!(p.against_votes, 0);
}

// ═══════════════════════════════════════════════════════
// 13. Config readback
// ═══════════════════════════════════════════════════════

#[test]
fn test_config_readback() {
    let s = setup();
    let config = s.client.get_config_info();
    assert_eq!(config.voting_delay, 100);
    assert_eq!(config.voting_period, 1000);
    assert_eq!(config.proposal_threshold, 100);
    assert_eq!(config.quorum_numerator, 10);
    assert_eq!(config.timelock_delay, 200);
    assert_eq!(config.grace_period, 600);
}
