#![no_std]

mod events;
mod storage;
mod types;

#[cfg(test)]
mod test;

use soroban_sdk::{
    contract, contractimpl, panic_with_error, token, Address, Bytes, BytesN, Env, String, Vec,
};

use crate::events::{
    emit_batch_created, emit_cancelled, emit_claimed, emit_dist_created, emit_expired,
    emit_initialized, emit_recovered,
};
use crate::storage::{
    append_claim_history, get_admin, get_claim_history as storage_get_history,
    get_claim_record as storage_get_claim_record, get_distribution, is_claimed, mark_claimed,
    next_dist_id, set_admin, set_claim_record, set_distribution,
};
use crate::types::{
    ClaimHistoryEntry, ClaimRecord, Distribution, DistributionKind, DistributionStatus,
    RewardDistributionError,
};

// ─────────────────────────────────────────────────────────────────────────────
// Contract
// ─────────────────────────────────────────────────────────────────────────────

#[contract]
pub struct RewardDistributionContract;

#[contractimpl]
impl RewardDistributionContract {
    // ─── Initialization ───────────────────────────────────────────────────────

    /// Initialize the contract. Must be called once before any other function.
    /// `admin` is the address that can create distributions and recover funds.
    pub fn initialize(env: Env, admin: Address) {
        if get_admin(&env).is_some() {
            panic_with_error!(&env, RewardDistributionError::AlreadyInitialized);
        }
        admin.require_auth();
        set_admin(&env, &admin);
        emit_initialized(&env, &admin);
    }

    // ─── Distribution creation ────────────────────────────────────────────────

    /// Create a new reward distribution campaign.
    ///
    /// The caller must be the admin. `total_allocation` tokens are pulled from
    /// `creator` into the contract immediately.
    ///
    /// Returns the new distribution ID.
    pub fn create_distribution(
        env: Env,
        creator: Address,
        label: String,
        kind: DistributionKind,
        merkle_root: BytesN<32>,
        token: Address,
        total_allocation: i128,
        expiry: u64,
    ) -> u32 {
        Self::require_admin(&env, &creator);

        if total_allocation <= 0 {
            panic_with_error!(&env, RewardDistributionError::InvalidAmount);
        }
        let now = env.ledger().timestamp();
        if expiry <= now {
            panic_with_error!(&env, RewardDistributionError::InvalidExpiry);
        }

        // Pull tokens from creator into the contract
        token::Client::new(&env, &token).transfer(
            &creator,
            &env.current_contract_address(),
            &total_allocation,
        );

        let id = next_dist_id(&env);
        let dist = Distribution {
            id,
            label,
            kind,
            merkle_root,
            token,
            total_allocation,
            claimed_amount: 0,
            claimed_count: 0,
            expiry,
            created_at: now,
            status: DistributionStatus::Active,
            creator: creator.clone(),
        };
        set_distribution(&env, id, &dist);

        emit_dist_created(&env, id, kind, &dist.token, total_allocation, expiry, &creator);
        id
    }

    /// Batch-create multiple distributions in one transaction.
    ///
    /// Each element of the input arrays corresponds to one distribution.
    /// All arrays must have the same length (> 0, ≤ 20).
    ///
    /// Returns the Vec of new distribution IDs.
    pub fn batch_create(
        env: Env,
        creator: Address,
        labels: Vec<String>,
        kinds: Vec<u32>,           // DistributionKind as u32 values
        merkle_roots: Vec<BytesN<32>>,
        tokens: Vec<Address>,
        allocations: Vec<i128>,
        expiries: Vec<u64>,
    ) -> Vec<u32> {
        Self::require_admin(&env, &creator);

        let len = labels.len();
        if len == 0
            || len > 20
            || kinds.len() != len
            || merkle_roots.len() != len
            || tokens.len() != len
            || allocations.len() != len
            || expiries.len() != len
        {
            panic_with_error!(&env, RewardDistributionError::InvalidBatchInput);
        }

        let now = env.ledger().timestamp();
        let mut ids: Vec<u32> = Vec::new(&env);

        for i in 0..len {
            let allocation = allocations.get(i).unwrap();
            let expiry = expiries.get(i).unwrap();
            let token_addr = tokens.get(i).unwrap();
            let root = merkle_roots.get(i).unwrap();
            let label = labels.get(i).unwrap();
            let kind_u32 = kinds.get(i).unwrap();

            if allocation <= 0 {
                panic_with_error!(&env, RewardDistributionError::InvalidAmount);
            }
            if expiry <= now {
                panic_with_error!(&env, RewardDistributionError::InvalidExpiry);
            }

            let kind = match kind_u32 {
                0 => DistributionKind::Airdrop,
                1 => DistributionKind::Incentive,
                2 => DistributionKind::PlayerReward,
                3 => DistributionKind::Grant,
                _ => panic_with_error!(&env, RewardDistributionError::InvalidBatchInput),
            };

            // Pull tokens for this distribution
            token::Client::new(&env, &token_addr).transfer(
                &creator,
                &env.current_contract_address(),
                &allocation,
            );

            let id = next_dist_id(&env);
            let dist = Distribution {
                id,
                label,
                kind,
                merkle_root: root,
                token: token_addr.clone(),
                total_allocation: allocation,
                claimed_amount: 0,
                claimed_count: 0,
                expiry,
                created_at: now,
                status: DistributionStatus::Active,
                creator: creator.clone(),
            };
            set_distribution(&env, id, &dist);
            emit_dist_created(&env, id, kind, &token_addr, allocation, expiry, &creator);
            ids.push_back(id);
        }

        emit_batch_created(&env, &ids);
        ids
    }

    // ─── Claiming ─────────────────────────────────────────────────────────────

    /// Claim tokens from a distribution using a Merkle proof.
    ///
    /// The leaf that was committed to is: `sha256(claimer_xdr || amount_xdr)`
    /// and the proof traverses up to the stored `merkle_root`.
    ///
    /// Each (distribution_id, claimer) pair can only be claimed once.
    pub fn claim(
        env: Env,
        distribution_id: u32,
        claimer: Address,
        amount: i128,
        proof: Vec<BytesN<32>>,
    ) {
        claimer.require_auth();

        if amount <= 0 {
            panic_with_error!(&env, RewardDistributionError::InvalidAmount);
        }

        let mut dist = Self::require_distribution(&env, distribution_id);

        // Status check
        if dist.status != DistributionStatus::Active {
            panic_with_error!(&env, RewardDistributionError::DistributionNotActive);
        }

        // Expiry check
        let now = env.ledger().timestamp();
        if now > dist.expiry {
            // Lazily mark expired
            dist.status = DistributionStatus::Expired;
            set_distribution(&env, distribution_id, &dist);
            emit_expired(&env, distribution_id);
            panic_with_error!(&env, RewardDistributionError::DistributionExpired);
        }

        // Double-claim check
        if is_claimed(&env, distribution_id, &claimer) {
            panic_with_error!(&env, RewardDistributionError::AlreadyClaimed);
        }

        // Merkle proof verification
        if !Self::verify_merkle_proof(&env, &dist.merkle_root, &claimer, amount, &proof) {
            panic_with_error!(&env, RewardDistributionError::InvalidMerkleProof);
        }

        // Allocation check
        let remaining = dist
            .total_allocation
            .checked_sub(dist.claimed_amount)
            .unwrap_or(0);
        if amount > remaining {
            panic_with_error!(&env, RewardDistributionError::InsufficientAllocation);
        }

        // State updates
        mark_claimed(&env, distribution_id, &claimer);
        dist.claimed_amount = dist
            .claimed_amount
            .checked_add(amount)
            .expect("overflow");
        dist.claimed_count += 1;

        // Check if fully exhausted
        if dist.claimed_amount >= dist.total_allocation {
            dist.status = DistributionStatus::Exhausted;
        }
        set_distribution(&env, distribution_id, &dist);

        // Persist claim record
        let record = ClaimRecord {
            distribution_id,
            claimer: claimer.clone(),
            amount,
            claimed_at: now,
        };
        set_claim_record(&env, distribution_id, &claimer, &record);

        // Append to per-claimer history
        append_claim_history(
            &env,
            &claimer,
            ClaimHistoryEntry {
                distribution_id,
                amount,
                claimed_at: now,
            },
        );

        // Transfer tokens to claimer
        token::Client::new(&env, &dist.token).transfer(
            &env.current_contract_address(),
            &claimer,
            &amount,
        );

        emit_claimed(&env, distribution_id, &claimer, amount);
    }

    // ─── Expiry & recovery ────────────────────────────────────────────────────

    /// Mark a distribution as expired (callable by anyone once past expiry).
    /// This is a housekeeping function; it does not move tokens.
    pub fn mark_expired(env: Env, distribution_id: u32) {
        let mut dist = Self::require_distribution(&env, distribution_id);

        if dist.status != DistributionStatus::Active {
            panic_with_error!(&env, RewardDistributionError::DistributionNotActive);
        }
        if env.ledger().timestamp() <= dist.expiry {
            panic_with_error!(&env, RewardDistributionError::NotExpiredYet);
        }

        dist.status = DistributionStatus::Expired;
        set_distribution(&env, distribution_id, &dist);
        emit_expired(&env, distribution_id);
    }

    /// Recover unclaimed tokens from an expired distribution.
    ///
    /// Only the distribution creator (or admin) can call this.
    /// The distribution must be in Expired or Exhausted status, OR past its
    /// expiry timestamp.  Tokens are sent back to `recipient`.
    ///
    /// Returns the amount recovered.
    pub fn recover_unclaimed(env: Env, caller: Address, distribution_id: u32, recipient: Address) -> i128 {
        caller.require_auth();

        let mut dist = Self::require_distribution(&env, distribution_id);

        // Only creator or admin may recover
        let admin = get_admin(&env).expect("not initialized");
        if caller != dist.creator && caller != admin {
            panic_with_error!(&env, RewardDistributionError::Unauthorized);
        }

        let now = env.ledger().timestamp();

        // Must be past expiry, or already marked Expired/Exhausted/Cancelled
        let is_past_expiry = now > dist.expiry;
        let is_terminal = dist.status == DistributionStatus::Expired
            || dist.status == DistributionStatus::Exhausted
            || dist.status == DistributionStatus::Cancelled;

        if !is_past_expiry && !is_terminal {
            panic_with_error!(&env, RewardDistributionError::NotExpiredYet);
        }

        let unclaimed = dist
            .total_allocation
            .checked_sub(dist.claimed_amount)
            .unwrap_or(0);

        if unclaimed <= 0 {
            panic_with_error!(&env, RewardDistributionError::NothingToRecover);
        }

        // Mark exhausted (all funds accounted for)
        dist.claimed_amount = dist.total_allocation;
        dist.status = DistributionStatus::Exhausted;
        set_distribution(&env, distribution_id, &dist);

        // Transfer remaining tokens to recipient
        token::Client::new(&env, &dist.token).transfer(
            &env.current_contract_address(),
            &recipient,
            &unclaimed,
        );

        emit_recovered(&env, distribution_id, &recipient, unclaimed);
        unclaimed
    }

    /// Admin cancels an active distribution before expiry and recovers tokens.
    ///
    /// Returns the unclaimed amount sent back to the creator.
    pub fn cancel_distribution(env: Env, admin: Address, distribution_id: u32) -> i128 {
        Self::require_admin(&env, &admin);

        let mut dist = Self::require_distribution(&env, distribution_id);

        if dist.status != DistributionStatus::Active {
            panic_with_error!(&env, RewardDistributionError::DistributionNotActive);
        }

        let unclaimed = dist
            .total_allocation
            .checked_sub(dist.claimed_amount)
            .unwrap_or(0);

        dist.status = DistributionStatus::Cancelled;
        dist.claimed_amount = dist.total_allocation; // prevent further recovery calls
        let creator = dist.creator.clone();
        set_distribution(&env, distribution_id, &dist);

        if unclaimed > 0 {
            token::Client::new(&env, &dist.token).transfer(
                &env.current_contract_address(),
                &creator,
                &unclaimed,
            );
        }

        emit_cancelled(&env, distribution_id, unclaimed);
        unclaimed
    }

    // ─── Query functions ──────────────────────────────────────────────────────

    /// Return the `Distribution` struct for `id`, panicking if not found.
    pub fn get_distribution(env: Env, id: u32) -> Distribution {
        Self::require_distribution(&env, id)
    }

    /// Return whether `claimer` has already claimed from `distribution_id`.
    pub fn has_claimed(env: Env, distribution_id: u32, claimer: Address) -> bool {
        is_claimed(&env, distribution_id, &claimer)
    }

    /// Return the full claim record for a (distribution, claimer) pair.
    pub fn get_claim_record(env: Env, distribution_id: u32, claimer: Address) -> Option<ClaimRecord> {
        storage_get_claim_record(&env, distribution_id, &claimer)
    }

    /// Return the full claim history for `claimer` across all distributions.
    pub fn get_claim_history(env: Env, claimer: Address) -> Vec<ClaimHistoryEntry> {
        storage_get_history(&env, &claimer)
    }

    /// Return the current admin address.
    pub fn admin(env: Env) -> Address {
        get_admin(&env).expect("not initialized")
    }

    /// Verify a Merkle proof externally (useful for off-chain tooling and tests).
    pub fn verify_proof(
        env: Env,
        distribution_id: u32,
        claimer: Address,
        amount: i128,
        proof: Vec<BytesN<32>>,
    ) -> bool {
        let dist = Self::require_distribution(&env, distribution_id);
        Self::verify_merkle_proof(&env, &dist.merkle_root, &claimer, amount, &proof)
    }

    // ─── Private helpers ──────────────────────────────────────────────────────

    /// Panic if the caller is not the admin (or if the contract is not initialized).
    fn require_admin(env: &Env, caller: &Address) {
        caller.require_auth();
        let admin = match get_admin(env) {
            Some(a) => a,
            None => panic_with_error!(env, RewardDistributionError::NotInitialized),
        };
        if *caller != admin {
            panic_with_error!(env, RewardDistributionError::Unauthorized);
        }
    }

    /// Load a distribution or panic with `DistributionNotFound`.
    fn require_distribution(env: &Env, id: u32) -> Distribution {
        match get_distribution(env, id) {
            Some(d) => d,
            None => panic_with_error!(env, RewardDistributionError::DistributionNotFound),
        }
    }

    /// Verify a standard binary Merkle proof.
    ///
    /// Leaf preimage: `sha256( claimer.to_xdr(env) || amount.to_xdr(env) )`
    ///
    /// Each proof step: hash the sorted pair `(current, sibling)` in
    /// lexicographic order to produce the parent.  This matches the
    /// standard off-chain tree construction (OpenZeppelin-style sorted
    /// pair hashing).
    fn verify_merkle_proof(
        env: &Env,
        root: &BytesN<32>,
        claimer: &Address,
        amount: i128,
        proof: &Vec<BytesN<32>>,
    ) -> bool {
        // Build leaf: sha256(claimer_xdr || amount_xdr)
        let mut leaf_data = Bytes::new(env);
        let addr_xdr = claimer.to_xdr(env);
        for byte in addr_xdr.iter() {
            leaf_data.push_back(byte);
        }
        let amount_xdr = amount.to_xdr(env);
        for byte in amount_xdr.iter() {
            leaf_data.push_back(byte);
        }
        let mut current: BytesN<32> = env.crypto().sha256(&leaf_data).into();

        // Traverse proof
        for i in 0..proof.len() {
            let sibling = proof.get(i).unwrap();

            // Sort pair lexicographically so off-chain and on-chain agree
            let (left, right) = if Self::bytes_lte(&current, &sibling) {
                (current.clone(), sibling.clone())
            } else {
                (sibling.clone(), current.clone())
            };

            let mut combined = Bytes::new(env);
            for byte in left.to_array().iter() {
                combined.push_back(*byte);
            }
            for byte in right.to_array().iter() {
                combined.push_back(*byte);
            }
            current = env.crypto().sha256(&combined).into();
        }

        current == *root
    }

    /// Lexicographic comparison of two BytesN<32>: returns true if `a <= b`.
    fn bytes_lte(a: &BytesN<32>, b: &BytesN<32>) -> bool {
        let a_arr = a.to_array();
        let b_arr = b.to_array();
        for i in 0..32usize {
            if a_arr[i] < b_arr[i] {
                return true;
            }
            if a_arr[i] > b_arr[i] {
                return false;
            }
        }
        true // equal
    }
}
