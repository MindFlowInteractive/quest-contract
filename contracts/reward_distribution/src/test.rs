#![cfg(test)]

extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, Bytes, BytesN, Env, String, Vec,
};

use crate::{
    types::{DistributionKind, DistributionStatus, RewardDistributionError},
    RewardDistributionContract, RewardDistributionContractClient,
};

// ─────────────────────────────────────────────────────────────────────────────
// Test helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Deploy a mock Soroban token, mint `supply` to `minter`, return token id.
fn create_token(env: &Env, admin: &Address, minter: &Address, supply: i128) -> Address {
    let token_id = env.register_stellar_asset_contract_v2(admin.clone());
    let token_address = token_id.address();
    let sac = token::StellarAssetClient::new(env, &token_address);
    sac.mint(minter, &supply);
    token_address
}

/// Register + initialize contract, return (client, admin).
fn setup(env: &Env) -> (RewardDistributionContractClient, Address) {
    let admin = Address::generate(env);
    let cid = env.register_contract(None, RewardDistributionContract);
    let client = RewardDistributionContractClient::new(env, &cid);
    client.initialize(&admin);
    (client, admin)
}

/// Build a trivial single-leaf Merkle tree for (claimer, amount).
/// Leaf = sha256(claimer_xdr || amount_xdr).
/// With only one leaf the root IS the leaf hash, and proof is empty.
fn single_leaf_root(env: &Env, claimer: &Address, amount: i128) -> (BytesN<32>, Vec<BytesN<32>>) {
    let mut data = Bytes::new(env);
    for b in claimer.to_xdr(env).iter() {
        data.push_back(b);
    }
    for b in amount.to_xdr(env).iter() {
        data.push_back(b);
    }
    let root: BytesN<32> = env.crypto().sha256(&data).into();
    (root, Vec::new(env))
}

/// Build a two-leaf Merkle tree for entries [(a0,m0), (a1,m1)].
/// Returns (root, proof_for_leaf_0, proof_for_leaf_1).
fn two_leaf_tree(
    env: &Env,
    a0: &Address, m0: i128,
    a1: &Address, m1: i128,
) -> (BytesN<32>, Vec<BytesN<32>>, Vec<BytesN<32>>) {
    let leaf0 = {
        let mut d = Bytes::new(env);
        for b in a0.to_xdr(env).iter() { d.push_back(b); }
        for b in m0.to_xdr(env).iter() { d.push_back(b); }
        let h: BytesN<32> = env.crypto().sha256(&d).into();
        h
    };
    let leaf1 = {
        let mut d = Bytes::new(env);
        for b in a1.to_xdr(env).iter() { d.push_back(b); }
        for b in m1.to_xdr(env).iter() { d.push_back(b); }
        let h: BytesN<32> = env.crypto().sha256(&d).into();
        h
    };

    // sorted pair hash
    let (left, right) = if leaf0.to_array() <= leaf1.to_array() {
        (leaf0.clone(), leaf1.clone())
    } else {
        (leaf1.clone(), leaf0.clone())
    };
    let mut combined = Bytes::new(env);
    for b in left.to_array().iter() { combined.push_back(*b); }
    for b in right.to_array().iter() { combined.push_back(*b); }
    let root: BytesN<32> = env.crypto().sha256(&combined).into();

    let mut proof0 = Vec::new(env);
    proof0.push_back(leaf1.clone());

    let mut proof1 = Vec::new(env);
    proof1.push_back(leaf0.clone());

    (root, proof0, proof1)
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 1 — initialize happy path
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    assert_eq!(client.admin(), admin);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 2 — double initialize fails
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_initialize_twice_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    client.initialize(&admin); // AlreadyInitialized = 1
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 3 — create distribution happy path
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_create_distribution() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let token = create_token(&env, &admin, &admin, 1_000_000);
    let claimer = Address::generate(&env);
    let (root, _) = single_leaf_root(&env, &claimer, 500_000);
    let expiry = env.ledger().timestamp() + 86_400;

    let id = client.create_distribution(
        &admin,
        &String::from_str(&env, "Season 1 Airdrop"),
        &DistributionKind::Airdrop,
        &root,
        &token,
        &1_000_000i128,
        &expiry,
    );
    assert_eq!(id, 1u32);

    let dist = client.get_distribution(&id);
    assert_eq!(dist.id, 1);
    assert_eq!(dist.total_allocation, 1_000_000);
    assert_eq!(dist.claimed_amount, 0);
    assert_eq!(dist.claimed_count, 0);
    assert_eq!(dist.status, DistributionStatus::Active);
    assert_eq!(dist.merkle_root, root);
    assert_eq!(dist.creator, admin);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 4 — non-admin cannot create distribution
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_create_distribution_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let imposter = Address::generate(&env);
    let token = create_token(&env, &admin, &imposter, 1_000_000);
    let (root, _) = single_leaf_root(&env, &imposter, 500_000);

    client.create_distribution(
        &imposter,
        &String::from_str(&env, "Fake"),
        &DistributionKind::Airdrop,
        &root,
        &token,
        &1_000_000i128,
        &(env.ledger().timestamp() + 1000),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 5 — create distribution with past expiry fails
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_create_distribution_past_expiry() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let token = create_token(&env, &admin, &admin, 1_000_000);
    let (root, _) = single_leaf_root(&env, &admin, 500_000);

    // expiry == now → invalid
    client.create_distribution(
        &admin,
        &String::from_str(&env, "Bad"),
        &DistributionKind::Airdrop,
        &root,
        &token,
        &1_000_000i128,
        &env.ledger().timestamp(),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 6 — single-leaf claim happy path
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_claim_single_leaf() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let claimer = Address::generate(&env);
    let amount = 500_000i128;
    let token = create_token(&env, &admin, &admin, 1_000_000);
    let (root, proof) = single_leaf_root(&env, &claimer, amount);
    let expiry = env.ledger().timestamp() + 86_400;

    let id = client.create_distribution(
        &admin,
        &String::from_str(&env, "Rewards"),
        &DistributionKind::PlayerReward,
        &root,
        &token,
        &1_000_000i128,
        &expiry,
    );

    assert!(!client.has_claimed(&id, &claimer));

    client.claim(&id, &claimer, &amount, &proof);

    assert!(client.has_claimed(&id, &claimer));
    let dist = client.get_distribution(&id);
    assert_eq!(dist.claimed_amount, amount);
    assert_eq!(dist.claimed_count, 1);

    let token_client = token::Client::new(&env, &token);
    assert_eq!(token_client.balance(&claimer), amount);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 7 — two-leaf tree, both claimers succeed
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_claim_two_leaf_tree() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let amount_a = 300_000i128;
    let amount_b = 700_000i128;

    let token = create_token(&env, &admin, &admin, 1_000_000);
    let (root, proof_a, proof_b) = two_leaf_tree(&env, &alice, amount_a, &bob, amount_b);
    let expiry = env.ledger().timestamp() + 86_400;

    let id = client.create_distribution(
        &admin,
        &String::from_str(&env, "Two Leaf"),
        &DistributionKind::Airdrop,
        &root,
        &token,
        &1_000_000i128,
        &expiry,
    );

    client.claim(&id, &alice, &amount_a, &proof_a);
    client.claim(&id, &bob, &amount_b, &proof_b);

    let token_client = token::Client::new(&env, &token);
    assert_eq!(token_client.balance(&alice), amount_a);
    assert_eq!(token_client.balance(&bob), amount_b);

    let dist = client.get_distribution(&id);
    assert_eq!(dist.claimed_amount, 1_000_000);
    assert_eq!(dist.claimed_count, 2);
    assert_eq!(dist.status, DistributionStatus::Exhausted);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 8 — double claim rejected
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_double_claim_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let claimer = Address::generate(&env);
    let amount = 200_000i128;
    let token = create_token(&env, &admin, &admin, 1_000_000);
    let (root, proof) = single_leaf_root(&env, &claimer, amount);
    let expiry = env.ledger().timestamp() + 86_400;

    let id = client.create_distribution(
        &admin,
        &String::from_str(&env, "Test"),
        &DistributionKind::Airdrop,
        &root,
        &token,
        &1_000_000i128,
        &expiry,
    );

    client.claim(&id, &claimer, &amount, &proof);
    client.claim(&id, &claimer, &amount, &proof); // AlreadyClaimed = 9
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 9 — invalid Merkle proof rejected
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_invalid_merkle_proof_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let claimer = Address::generate(&env);
    let token = create_token(&env, &admin, &admin, 1_000_000);
    let (root, _) = single_leaf_root(&env, &claimer, 500_000);
    let expiry = env.ledger().timestamp() + 86_400;

    let id = client.create_distribution(
        &admin,
        &String::from_str(&env, "Test"),
        &DistributionKind::Airdrop,
        &root,
        &token,
        &1_000_000i128,
        &expiry,
    );

    // Wrong amount → proof won't match root
    let mut bad_proof = Vec::new(&env);
    bad_proof.push_back(BytesN::from_array(&env, &[0xFFu8; 32]));
    client.claim(&id, &claimer, &999_999i128, &bad_proof); // InvalidMerkleProof = 10
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 10 — claim on expired distribution fails
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_claim_expired_distribution() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let claimer = Address::generate(&env);
    let amount = 500_000i128;
    let token = create_token(&env, &admin, &admin, 1_000_000);
    let (root, proof) = single_leaf_root(&env, &claimer, amount);
    let expiry = env.ledger().timestamp() + 100;

    let id = client.create_distribution(
        &admin,
        &String::from_str(&env, "Short"),
        &DistributionKind::Airdrop,
        &root,
        &token,
        &1_000_000i128,
        &expiry,
    );

    // Advance past expiry
    env.ledger().with_timestamp(expiry + 1);
    client.claim(&id, &claimer, &amount, &proof); // DistributionExpired = 8
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 11 — claim history tracked per claimer
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_claim_history() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let claimer = Address::generate(&env);
    let token = create_token(&env, &admin, &admin, 2_000_000);

    // Distribution 1
    let amt1 = 300_000i128;
    let (root1, proof1) = single_leaf_root(&env, &claimer, amt1);
    let exp = env.ledger().timestamp() + 86_400;
    let id1 = client.create_distribution(
        &admin,
        &String::from_str(&env, "Dist 1"),
        &DistributionKind::Airdrop,
        &root1,
        &token,
        &1_000_000i128,
        &exp,
    );

    // Distribution 2
    let amt2 = 700_000i128;
    let (root2, proof2) = single_leaf_root(&env, &claimer, amt2);
    let id2 = client.create_distribution(
        &admin,
        &String::from_str(&env, "Dist 2"),
        &DistributionKind::Incentive,
        &root2,
        &token,
        &1_000_000i128,
        &exp,
    );

    client.claim(&id1, &claimer, &amt1, &proof1);
    client.claim(&id2, &claimer, &amt2, &proof2);

    let history = client.get_claim_history(&claimer);
    assert_eq!(history.len(), 2);
    assert_eq!(history.get(0).unwrap().distribution_id, id1);
    assert_eq!(history.get(0).unwrap().amount, amt1);
    assert_eq!(history.get(1).unwrap().distribution_id, id2);
    assert_eq!(history.get(1).unwrap().amount, amt2);

    // Claim record for dist1
    let rec = client.get_claim_record(&id1, &claimer).unwrap();
    assert_eq!(rec.distribution_id, id1);
    assert_eq!(rec.claimer, claimer);
    assert_eq!(rec.amount, amt1);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 12 — mark_expired by anyone once past expiry
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_mark_expired() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let token = create_token(&env, &admin, &admin, 1_000_000);
    let (root, _) = single_leaf_root(&env, &admin, 500_000);
    let expiry = env.ledger().timestamp() + 200;

    let id = client.create_distribution(
        &admin,
        &String::from_str(&env, "Soon"),
        &DistributionKind::Airdrop,
        &root,
        &token,
        &1_000_000i128,
        &expiry,
    );

    assert_eq!(client.get_distribution(&id).status, DistributionStatus::Active);

    env.ledger().with_timestamp(expiry + 1);
    let anyone = Address::generate(&env);
    client.mark_expired(&id);

    assert_eq!(client.get_distribution(&id).status, DistributionStatus::Expired);
    let _ = anyone; // suppress unused warning
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 13 — mark_expired before expiry fails
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn test_mark_expired_too_early() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let token = create_token(&env, &admin, &admin, 1_000_000);
    let (root, _) = single_leaf_root(&env, &admin, 500_000);
    let expiry = env.ledger().timestamp() + 9_999;

    let id = client.create_distribution(
        &admin,
        &String::from_str(&env, "Active"),
        &DistributionKind::Airdrop,
        &root,
        &token,
        &1_000_000i128,
        &expiry,
    );

    client.mark_expired(&id); // NotExpiredYet = 12
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 14 — recover unclaimed after expiry
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_recover_unclaimed() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let claimer = Address::generate(&env);
    let amount = 400_000i128;
    let total = 1_000_000i128;
    let token = create_token(&env, &admin, &admin, total);
    let (root, proof) = single_leaf_root(&env, &claimer, amount);
    let expiry = env.ledger().timestamp() + 500;

    let id = client.create_distribution(
        &admin,
        &String::from_str(&env, "Recover Test"),
        &DistributionKind::Airdrop,
        &root,
        &token,
        &total,
        &expiry,
    );

    // Claimer claims their portion
    client.claim(&id, &claimer, &amount, &proof);

    // Advance past expiry
    env.ledger().with_timestamp(expiry + 1);

    let token_client = token::Client::new(&env, &token);
    let admin_balance_before = token_client.balance(&admin);

    let recovered = client.recover_unclaimed(&admin, &id, &admin);
    assert_eq!(recovered, total - amount);
    assert_eq!(token_client.balance(&admin), admin_balance_before + (total - amount));

    // Distribution is now Exhausted
    assert_eq!(client.get_distribution(&id).status, DistributionStatus::Exhausted);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 15 — recover nothing when fully claimed
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_recover_nothing_when_fully_claimed() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let claimer = Address::generate(&env);
    let total = 1_000_000i128;
    let token = create_token(&env, &admin, &admin, total);
    let (root, proof) = single_leaf_root(&env, &claimer, total);
    let expiry = env.ledger().timestamp() + 500;

    let id = client.create_distribution(
        &admin,
        &String::from_str(&env, "Full"),
        &DistributionKind::Airdrop,
        &root,
        &token,
        &total,
        &expiry,
    );

    client.claim(&id, &claimer, &total, &proof);
    env.ledger().with_timestamp(expiry + 1);
    client.recover_unclaimed(&admin, &id, &admin); // NothingToRecover = 13
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 16 — cancel distribution returns unclaimed to creator
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cancel_distribution() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let total = 1_000_000i128;
    let token = create_token(&env, &admin, &admin, total);
    let (root, _) = single_leaf_root(&env, &admin, 500_000);
    let expiry = env.ledger().timestamp() + 86_400;

    let id = client.create_distribution(
        &admin,
        &String::from_str(&env, "Cancel Me"),
        &DistributionKind::Incentive,
        &root,
        &token,
        &total,
        &expiry,
    );

    let token_client = token::Client::new(&env, &token);
    let admin_before = token_client.balance(&admin);

    let refunded = client.cancel_distribution(&admin, &id);
    assert_eq!(refunded, total);
    assert_eq!(token_client.balance(&admin), admin_before + total);
    assert_eq!(client.get_distribution(&id).status, DistributionStatus::Cancelled);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 17 — cancel already-cancelled distribution fails
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_cancel_twice_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let token = create_token(&env, &admin, &admin, 1_000_000);
    let (root, _) = single_leaf_root(&env, &admin, 500_000);

    let id = client.create_distribution(
        &admin,
        &String::from_str(&env, "Double Cancel"),
        &DistributionKind::Airdrop,
        &root,
        &token,
        &1_000_000i128,
        &(env.ledger().timestamp() + 86_400),
    );

    client.cancel_distribution(&admin, &id);
    client.cancel_distribution(&admin, &id); // DistributionNotActive = 7
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 18 — batch_create distributes across multiple campaigns
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_batch_create() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let c1 = Address::generate(&env);
    let c2 = Address::generate(&env);
    let token = create_token(&env, &admin, &admin, 3_000_000);

    let (root1, _) = single_leaf_root(&env, &c1, 1_000_000);
    let (root2, _) = single_leaf_root(&env, &c2, 2_000_000);

    let now = env.ledger().timestamp();
    let expiry = now + 86_400;

    let mut labels: Vec<String> = Vec::new(&env);
    labels.push_back(String::from_str(&env, "Batch A"));
    labels.push_back(String::from_str(&env, "Batch B"));

    let mut kinds: Vec<u32> = Vec::new(&env);
    kinds.push_back(0u32); // Airdrop
    kinds.push_back(1u32); // Incentive

    let mut roots: Vec<BytesN<32>> = Vec::new(&env);
    roots.push_back(root1);
    roots.push_back(root2);

    let mut tokens: Vec<Address> = Vec::new(&env);
    tokens.push_back(token.clone());
    tokens.push_back(token.clone());

    let mut allocs: Vec<i128> = Vec::new(&env);
    allocs.push_back(1_000_000i128);
    allocs.push_back(2_000_000i128);

    let mut expiries: Vec<u64> = Vec::new(&env);
    expiries.push_back(expiry);
    expiries.push_back(expiry);

    let ids = client.batch_create(&admin, &labels, &kinds, &roots, &tokens, &allocs, &expiries);

    assert_eq!(ids.len(), 2);
    let id1 = ids.get(0).unwrap();
    let id2 = ids.get(1).unwrap();

    assert_eq!(client.get_distribution(&id1).total_allocation, 1_000_000);
    assert_eq!(client.get_distribution(&id2).total_allocation, 2_000_000);
    assert_eq!(client.get_distribution(&id1).kind, DistributionKind::Airdrop);
    assert_eq!(client.get_distribution(&id2).kind, DistributionKind::Incentive);
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 19 — verify_proof public function
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_verify_proof() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let claimer = Address::generate(&env);
    let amount = 123_456i128;
    let token = create_token(&env, &admin, &admin, 1_000_000);
    let (root, proof) = single_leaf_root(&env, &claimer, amount);
    let expiry = env.ledger().timestamp() + 86_400;

    let id = client.create_distribution(
        &admin,
        &String::from_str(&env, "Verify"),
        &DistributionKind::Grant,
        &root,
        &token,
        &1_000_000i128,
        &expiry,
    );

    // Correct proof returns true
    assert!(client.verify_proof(&id, &claimer, &amount, &proof));

    // Wrong amount returns false
    let bad_proof: Vec<BytesN<32>> = Vec::new(&env);
    assert!(!client.verify_proof(&id, &claimer, &999i128, &bad_proof));
}

// ─────────────────────────────────────────────────────────────────────────────
// TEST 20 — supply tracking: claimed_amount & claimed_count correct
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_supply_tracking() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let amt_a = 200_000i128;
    let amt_b = 300_000i128;

    let token = create_token(&env, &admin, &admin, 1_000_000);
    let (root, pa, pb) = two_leaf_tree(&env, &a, amt_a, &b, amt_b);
    let expiry = env.ledger().timestamp() + 86_400;

    let id = client.create_distribution(
        &admin,
        &String::from_str(&env, "Track"),
        &DistributionKind::Airdrop,
        &root,
        &token,
        &1_000_000i128,
        &expiry,
    );

    client.claim(&id, &a, &amt_a, &pa);
    let dist = client.get_distribution(&id);
    assert_eq!(dist.claimed_amount, amt_a);
    assert_eq!(dist.claimed_count, 1);

    client.claim(&id, &b, &amt_b, &pb);
    let dist = client.get_distribution(&id);
    assert_eq!(dist.claimed_amount, amt_a + amt_b);
    assert_eq!(dist.claimed_count, 2);
    assert_eq!(dist.status, DistributionStatus::Active); // not yet exhausted
}
