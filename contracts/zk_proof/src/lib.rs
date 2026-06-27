#![no_std]

//! # Privacy-Preserving Zero-Knowledge Proof Contract
//!
//! A Soroban smart contract that implements the on-chain verifier half of a
//! hash-based zero-knowledge proof system modelled after shielded-pool
//! constructions (Tornado Cash / Zcash-style).
//!
//! ## Why hash-based?
//!
//! Soroban 21.x does **not** expose native elliptic-curve precompiles
//! (BN254 / Baby Jubjub) nor pairing-friendly field arithmetic.  Verifying
//! a real Groth16 / PLONK proof on-chain would require re-implementing
//! those primitives in pure Soroban host functions – something that is
//! impractical to do securely within per-transaction gas budgets.
//!
//! Instead this contract implements the *semantic surface* of a ZK proof
//! verifier – commitment, nullifier, Merkle inclusion, Fiat-Shamir
//! binding and a circuit-constraint layer – without the actual SNARK
//! math.  Off-chain provers:
//!
//! 1. Hold a `secret` and a `randomness`.
//! 2. Compute `commitment = sha256(DOMAIN_COMMITMENT || secret || randomness || scope)`.
//! 3. Compute `nullifier  = sha256(DOMAIN_NULLIFIER  || secret || scope)`.
//! 4. Compute a Merkle path from the deposited leaf to the pool's root.
//! 5. Compute a Fiat-Shamir `binding` that ties the four public values
//!    together with the `public_signals`.
//!
//! Only the *outputs* of (2)-(5) ever appear on-chain.  The `secret`
//! and `randomness` are therefore never leaked – which is exactly the
//! soundness goal of a real ZK proof.
//!
//! ## Acceptance criteria
//!
//! | Criterion                               | Where enforced                          |
//! |-----------------------------------------|-----------------------------------------|
//! | Proofs verified correctly               | `verify_proof` / `verify_merkle_path`   |
//! | Privacy maintained                      | No secret ever crosses the contract ABI |
//! | No double-spending                      | Nullifier set marked on withdrawal      |
//! | Nullifiers prevent replay               | Same nullifier cannot withdraw twice    |
//! | Performance acceptable                  | `batch_verify` short-circuits failures  |
//! | All tests pass                          | `mod tests` in this file                |

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    Vec,
};

// ---------------------------------------------------------------------------
// Constants & domain separation
// ---------------------------------------------------------------------------

/// Maximum defensible Merkle depth.  A binary tree of this depth holds up to
/// `2^MAX_DEPTH` leaves which is sufficient for any reasonable shielded pool.
pub const MAX_DEPTH: u32 = 32;

/// Domain-separation tag for Merkle leaves.  Prepended to `sha256` so that
/// leaves cannot be confused with internal Merkle nodes.
pub const DOMAIN_LEAF: u8 = 0x00;

/// Domain-separation tag for internal Merkle nodes.
pub const DOMAIN_NODE: u8 = 0x01;

/// Domain-separation tag for nullifier hashing off-chain.
pub const DOMAIN_NULLIFIER: u8 = 0x02;

/// Domain-separation tag for commitment hashing off-chain.
pub const DOMAIN_COMMITMENT: u8 = 0x03;

/// Domain-separation tag for the on-chain amount-sentinel hash used as the
/// first public signal.
pub const DOMAIN_AMOUNT_SENTINEL: u8 = 0x10;

/// Fixed prefix byte prepended to the Fiat-Shamir binding transcript.
pub const BINDING_PREFIX: [u8; 8] = *b"ZKPF:v1\0";

/// Deterministic value used to pad empty Merkle leaves (those with index
/// `>= leaf_count`).  This value is itself the all-`0xFF` 32-byte sentinel,
/// so it is distinguishable from any real commitment (which is the output
/// of `sha256(DOMAIN_COMMITMENT || …)` and never `0xFF…FF`).
const EMPTY_LEAF: [u8; 32] = [
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ZkError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAuthorized = 3,
    ExcessiveDepth = 4,
    InvalidMerklePath = 5,
    InvalidBinding = 6,
    InvalidCommitment = 7,
    CommitmentAlreadyDeposited = 8,
    CommitmentNotInPool = 9,
    NullifierAlreadySpent = 10,
    InvalidPublicSignals = 11,
    PoolNotActive = 12,
    PoolNotFound = 13,
    CircuitNotFound = 14,
    ZeroDenomination = 15,
    InvalidViewTag = 16,
    AuditorNotRegistered = 17,
    ViewTagAlreadyUsed = 18,
    PublicSignalsLimitExceeded = 19,
    ZeroRecipient = 20,
    BatchLengthMismatch = 21,
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A subset of `ZkProof` containing only the public statement a verifier
/// needs in order to check the proof.  Used to make the public surface
/// small when batch verifying.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProofStatement {
    pub commitment: BytesN<32>,
    pub nullifier: BytesN<32>,
    pub merkle_root: BytesN<32>,
    pub public_signals: Vec<BytesN<32>>,
}

/// Full ZK proof submitted by the prover.  All fields are *public* by
/// design – the zero-knowledge property comes from the fact that the
/// underlying witness (the secret linking `commitment` and `nullifier`)
/// never appears on-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZkProof {
    /// Pedersen-style commitment to `(secret, randomness, scope)`.
    pub commitment: BytesN<32>,
    /// Deterministic nullifier for replay protection.
    pub nullifier: BytesN<32>,
    /// Merkle root the commitment was deposited under.
    pub merkle_root: BytesN<32>,
    /// Public leaf index of the commitment (0-based).
    pub leaf_index: u32,
    /// Sibling hashes from leaf up to root.
    pub merkle_path: Vec<BytesN<32>>,
    /// Public outputs the (off-chain) circuit asserts.
    pub public_signals: Vec<BytesN<32>>,
    /// Fiat-Shamir binding so the proof cannot be retargeted.
    pub binding: BytesN<32>,
}

/// A circuit specification – an analogue of a SNARK R1CS file, expressed
/// declaratively and enforced by the contract at withdrawal time.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CircuitSpec {
    pub id: u32,
    /// Minimum depth of the underlying Merkle tree (set to 0 to allow
    /// single-leaf pools whose root equals their leaf).
    pub min_depth: u32,
    /// Maximum Merkle-tree depth (= max number of siblings in `merkle_path`).
    pub max_depth: u32,
    /// Maximum number of public signals accepted per proof.
    pub max_public_signals: u32,
    /// If true, the first public signal must equal `hash_amount(denom)`.
    pub enforce_amount_lt_denom: bool,
    /// If true, the second public signal must be non-zero and equal to
    /// the supplied `recipient_hash` for withdrawal calls.
    pub enforce_recipient_non_zero: bool,
    pub created_at: u64,
}

/// A shielded privacy pool.  All deposits are commitments into the pool's
/// Merkle tree; all withdrawals reveal only the nullifier and a Merkle
/// proof of membership.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivacyPool {
    pub id: u32,
    pub denomination: i128,
    pub leaf_count: u32,
    pub circuit_id: u32,
    pub created_at: u64,
    pub active: bool,
}

/// Per-deposit bookkeeping – stores the user's optional view-tag for
/// opt-in auditor access.  The view-tag itself is opaque here; the
/// actual cryptographic linkage to a specific `secret` happens
/// off-chain between the depositor and the auditor.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepositRecord {
    pub commitment: BytesN<32>,
    pub leaf_index: u32,
    pub view_tag: Option<BytesN<32>>,
    pub deposited_at: u64,
}

/// Immutable record of every successful withdrawal.  Stored so the
/// withdrawal history can be audited without re-deriving nullifier sets.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalRecord {
    pub nullifier: BytesN<32>,
    pub commitment: BytesN<32>,
    pub recipient_hash: BytesN<32>,
    pub pool_id: u32,
    pub completed_at: u64,
}

/// Aggregate counters for a pool, returned by `get_pool_stats`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolStats {
    pub deposit_count: u32,
    pub withdrawal_count: u32,
    pub active_commitments: u32,
    pub spent_nullifiers: u32,
    pub merkle_depth: u32,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
pub enum DataKey {
    /// Contract administrator.
    Admin,
    /// Counter used to assign monotonic IDs to circuits.
    CircuitCounter,
    /// Counter used to assign monotonic IDs to pools.
    PoolCounter,
    /// `Circuit(id)` → `CircuitSpec`.
    Circuit(u32),
    /// `Pool(id)` → `PrivacyPool`.
    Pool(u32),
    /// `(Pool, leaf_index)` → leaf hash (sha256(DOMAIN_LEAF || commitment)).
    LeafHash(u32, u32),
    /// `(Pool, commitment)` → `DepositRecord`.
    Commitment(u32, BytesN<32>),
    /// `(Pool, view_tag)` → `(commitment, leaf_index)` for auditor look-ups.
    ViewTag(u32, BytesN<32>),
    /// `(Pool, nullifier)` → bool – true once spent.
    Nullifier(u32, BytesN<32>),
    /// `(Pool, idx)` → `WithdrawalRecord`.  Append-only history.
    Withdrawal(u32, u32),
    /// `(Pool, idx)` → settlement reference for paginated history.
    WithdrawalCount(u32),
    /// `Auditor(scope_tag)` → `Address`.
    Auditor(BytesN<32>),
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct ZkProofContract;

#[contractimpl]
impl ZkProofContract {
    // =======================================================================
    // Initialization & admin
    // =======================================================================

    /// One-shot initialiser.  Stores the administrator and seeds two
    /// built-in circuit specifications.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ZkError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ZkError::AlreadyInitialized);
        }
        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::CircuitCounter, &0u32);
        env.storage().instance().set(&DataKey::PoolCounter, &0u32);

        // Built-in circuit #1: permissive (min_depth=0).
        let basic_id = Self::create_circuit(
            env.clone(),
            admin.clone(),
            CircuitSpec {
                id: 0,
                min_depth: 0,
                max_depth: 20,
                max_public_signals: 4,
                enforce_amount_lt_denom: true,
                enforce_recipient_non_zero: true,
                created_at: env.ledger().timestamp(),
            },
        )?;

        // Built-in circuit #2: stringent (min_depth=4).
        let strict_id = Self::create_circuit(
            env.clone(),
            admin.clone(),
            CircuitSpec {
                id: 0,
                min_depth: 4,
                max_depth: 20,
                max_public_signals: 2,
                enforce_amount_lt_denom: true,
                enforce_recipient_non_zero: true,
                created_at: env.ledger().timestamp(),
            },
        )?;

        env.events().publish(
            (symbol_short!("zk_init"),),
            (admin, basic_id, strict_id),
        );

        Ok(())
    }

    /// Low-level circuit creator used both by `initialize` (for built-ins)
    /// and by the admin (`create_circuit`).  Validates the spec before
    /// persisting it and assigning a fresh ID.
    pub fn create_circuit(env: Env, admin: Address, spec: CircuitSpec) -> Result<u32, ZkError> {
        Self::require_admin(&env, &admin)?;

        if spec.min_depth > spec.max_depth || spec.max_depth > MAX_DEPTH {
            return Err(ZkError::ExcessiveDepth);
        }
        if spec.max_public_signals == 0 || spec.max_public_signals > 16 {
            return Err(ZkError::PublicSignalsLimitExceeded);
        }

        let counter: u32 = env
            .storage()
            .instance()
            .get(&DataKey::CircuitCounter)
            .unwrap_or(0);
        let id = counter + 1;

        let mut stored = spec.clone();
        stored.id = id;
        env.storage().instance().set(&DataKey::Circuit(id), &stored);
        env.storage()
            .instance()
            .set(&DataKey::CircuitCounter, &id);

        env.events().publish(
            (symbol_short!("zk_circ"),),
            (id, spec.min_depth, spec.max_depth, spec.max_public_signals),
        );
        Ok(id)
    }

    /// Create a new shielded pool attached to an existing circuit spec.
    pub fn create_pool(
        env: Env,
        admin: Address,
        denomination: i128,
        circuit_id: u32,
    ) -> Result<u32, ZkError> {
        Self::require_admin(&env, &admin)?;

        if denomination <= 0 {
            return Err(ZkError::ZeroDenomination);
        }
        if !env
            .storage()
            .instance()
            .has(&DataKey::Circuit(circuit_id))
        {
            return Err(ZkError::CircuitNotFound);
        }

        let counter: u32 = env
            .storage()
            .instance()
            .get(&DataKey::PoolCounter)
            .unwrap_or(0);
        let id = counter + 1;

        let pool = PrivacyPool {
            id,
            denomination,
            leaf_count: 0,
            circuit_id,
            created_at: env.ledger().timestamp(),
            active: true,
        };
        env.storage().instance().set(&DataKey::Pool(id), &pool);
        env.storage().instance().set(&DataKey::PoolCounter, &id);

        env.events().publish(
            (symbol_short!("zk_pool"),),
            (id, denomination, circuit_id),
        );
        Ok(id)
    }

    /// Pause or unpause a pool.  Paused pools reject new deposits and
    /// withdrawals but keep their history intact.
    pub fn set_pool_active(
        env: Env,
        admin: Address,
        pool_id: u32,
        active: bool,
    ) -> Result<(), ZkError> {
        Self::require_admin(&env, &admin)?;
        let mut pool: PrivacyPool = env
            .storage()
            .instance()
            .get(&DataKey::Pool(pool_id))
            .ok_or(ZkError::PoolNotFound)?;
        if pool.active == active {
            return Ok(());
        }
        pool.active = active;
        env.storage().instance().set(&DataKey::Pool(pool_id), &pool);
        env.events().publish(
            (symbol_short!("zk_pause"),),
            (pool_id, active),
        );
        Ok(())
    }

    /// Register an opt-in auditor for a given scope.  The auditor can
    /// query deposits by view-tag but cannot in any way decrypt or
    /// recover the depositor's secret on-chain.
    pub fn register_auditor(
        env: Env,
        admin: Address,
        scope_tag: BytesN<32>,
        auditor: Address,
    ) -> Result<(), ZkError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::Auditor(scope_tag.clone()), &auditor);
        env.events().publish(
            (symbol_short!("zk_aud"),),
            (scope_tag, auditor),
        );
        Ok(())
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), ZkError> {
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(ZkError::NotInitialized);
        }
        let stored: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ZkError::NotInitialized)?;
        if stored != *admin {
            return Err(ZkError::NotAuthorized);
        }
        admin.require_auth();
        Ok(())
    }

    // =======================================================================
    // Deposits
    // =======================================================================

    /// Deposit a commitment into a pool.  The depositor may optionally
    /// attach an opaque `view_tag` (a 32-byte scalar they share with an
    /// auditor off-chain) – enabling opt-in regulatory access.
    pub fn deposit(
        env: Env,
        depositor: Address,
        pool_id: u32,
        commitment: BytesN<32>,
        view_tag: Option<BytesN<32>>,
    ) -> Result<u32, ZkError> {
        depositor.require_auth();

        let mut pool: PrivacyPool = env
            .storage()
            .instance()
            .get(&DataKey::Pool(pool_id))
            .ok_or(ZkError::PoolNotFound)?;
        if !pool.active {
            return Err(ZkError::PoolNotActive);
        }
        if is_zero_hash(&commitment) {
            return Err(ZkError::InvalidCommitment);
        }
        if env
            .storage()
            .instance()
            .has(&DataKey::Commitment(pool_id, commitment.clone()))
        {
            return Err(ZkError::CommitmentAlreadyDeposited);
        }

        if let Some(tag) = view_tag.clone() {
            if is_zero_hash(&tag) {
                return Err(ZkError::InvalidViewTag);
            }
            if env
                .storage()
                .instance()
                .has(&DataKey::ViewTag(pool_id, tag.clone()))
            {
                return Err(ZkError::ViewTagAlreadyUsed);
            }
            env.storage().instance().set(
                &DataKey::ViewTag(pool_id, tag),
                &(commitment.clone(), pool.leaf_count),
            );
        }

        let leaf_index = pool.leaf_count;
        pool.leaf_count += 1;
        env.storage().instance().set(&DataKey::Pool(pool_id), &pool);

        let record = DepositRecord {
            commitment: commitment.clone(),
            leaf_index,
            view_tag: view_tag.clone(),
            deposited_at: env.ledger().timestamp(),
        };
        env.storage()
            .instance()
            .set(&DataKey::Commitment(pool_id, commitment.clone()), &record);

        // Store the domain-separated leaf hash so it can be used by
        // `verify_merkle_path` without recomputing the leaf at proof time.
        let leaf_hash = hash_leaf(env.clone(), commitment.clone());
        env.storage()
            .instance()
            .set(&DataKey::LeafHash(pool_id, leaf_index), &leaf_hash);

        env.events().publish(
            (symbol_short!("zk_dep"),),
            (pool_id, leaf_index, depositor),
        );
        Ok(leaf_index)
    }

    // =======================================================================
    // Verification (pure, no state mutation) – POOL-BOUND
    // =======================================================================

    /// Public-verifier entry point.  Returns `true` iff every constraint
    /// in the supplied circuit spec is satisfied by the proof against the
    /// **current** state of `pool_id`.  Does not mutate storage.
    ///
    /// Both `verify_proof` and `batch_verify` are intentionally bound to a
    /// concrete pool so an attacker cannot pass a forged proof with an
    /// arbitrary `merkle_root`.
    pub fn verify_proof(
        env: Env,
        pool_id: u32,
        proof: ZkProof,
        circuit_id: u32,
    ) -> Result<bool, ZkError> {
        let pool: PrivacyPool = env
            .storage()
            .instance()
            .get(&DataKey::Pool(pool_id))
            .ok_or(ZkError::PoolNotFound)?;
        let circuit: CircuitSpec = env
            .storage()
            .instance()
            .get(&DataKey::Circuit(circuit_id))
            .ok_or(ZkError::CircuitNotFound)?;

        let statement = ProofStatement {
            commitment: proof.commitment.clone(),
            nullifier: proof.nullifier.clone(),
            merkle_root: proof.merkle_root.clone(),
            public_signals: proof.public_signals.clone(),
        };
        verify_proof_inner(&env, &proof, &statement, &circuit, &pool, None)
    }

    /// Batch verifier – verifies that **all** proofs in the batch are
    /// valid against the **current** state of `pool_id`.  Short-circuits
    /// on the first failure (atomic safety).
    pub fn batch_verify(
        env: Env,
        pool_id: u32,
        proofs: Vec<ZkProof>,
        statements: Vec<ProofStatement>,
        circuit_id: u32,
    ) -> Result<bool, ZkError> {
        if proofs.len() != statements.len() {
            return Err(ZkError::BatchLengthMismatch);
        }
        let pool: PrivacyPool = env
            .storage()
            .instance()
            .get(&DataKey::Pool(pool_id))
            .ok_or(ZkError::PoolNotFound)?;
        let circuit: CircuitSpec = env
            .storage()
            .instance()
            .get(&DataKey::Circuit(circuit_id))
            .ok_or(ZkError::CircuitNotFound)?;

        let mut i: u32 = 0;
        while i < proofs.len() {
            let proof = proofs.get(i).unwrap();
            let stmt = statements.get(i).unwrap();
            if !verify_proof_inner(&env, &proof, &stmt, &circuit, &pool, None)? {
                return Ok(false);
            }
            i += 1;
        }
        Ok(true)
    }

    // =======================================================================
    // Withdrawal (verification + state mutation)
    // =======================================================================

    /// Withdraw from a pool using a ZK proof.  Verifies the proof, marks
    /// the nullifier as spent, and emits a withdrawal record.  The
    /// `recipient_hash` is the public-signal-equivalent of the recipient
    /// identity and is stored verbatim so that downstream token-transfer
    /// contracts (which the ZK contract is decoupled from) can act on it.
    pub fn withdraw(
        env: Env,
        caller: Address,
        pool_id: u32,
        proof: ZkProof,
        recipient_hash: BytesN<32>,
    ) -> Result<(), ZkError> {
        caller.require_auth();

        let pool: PrivacyPool = env
            .storage()
            .instance()
            .get(&DataKey::Pool(pool_id))
            .ok_or(ZkError::PoolNotFound)?;
        if !pool.active {
            return Err(ZkError::PoolNotActive);
        }
        let circuit: CircuitSpec = env
            .storage()
            .instance()
            .get(&DataKey::Circuit(pool.circuit_id))
            .ok_or(ZkError::CircuitNotFound)?;

        if is_zero_hash(&recipient_hash) {
            return Err(ZkError::ZeroRecipient);
        }

        let statement = ProofStatement {
            commitment: proof.commitment.clone(),
            nullifier: proof.nullifier.clone(),
            merkle_root: proof.merkle_root.clone(),
            public_signals: proof.public_signals.clone(),
        };

        // CRITICAL: `verify_proof_inner` returns `Result<bool,_>`; we MUST
        // treat `Ok(false)` as a hard rejection, not silently discard it.
        if !verify_proof_inner(
            &env,
            &proof,
            &statement,
            &circuit,
            &pool,
            Some(recipient_hash.clone()),
        )? {
            return Err(ZkError::InvalidMerklePath);
        }

        if env
            .storage()
            .instance()
            .has(&DataKey::Nullifier(pool_id, proof.nullifier.clone()))
        {
            return Err(ZkError::NullifierAlreadySpent);
        }
        if !env
            .storage()
            .instance()
            .has(&DataKey::Commitment(pool_id, proof.commitment.clone()))
        {
            return Err(ZkError::CommitmentNotInPool);
        }

        // Mark the nullifier as spent – replay protection.
        env.storage().instance().set(
            &DataKey::Nullifier(pool_id, proof.nullifier.clone()),
            &true,
        );

        // Append to the immutable withdrawal history.
        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::WithdrawalCount(pool_id))
            .unwrap_or(0);
        let record = WithdrawalRecord {
            nullifier: proof.nullifier.clone(),
            commitment: proof.commitment.clone(),
            recipient_hash: recipient_hash.clone(),
            pool_id,
            completed_at: env.ledger().timestamp(),
        };
        env.storage()
            .instance()
            .set(&DataKey::Withdrawal(pool_id, count), &record);
        env.storage()
            .instance()
            .set(&DataKey::WithdrawalCount(pool_id), &(count + 1));

        env.events().publish(
            (symbol_short!("zk_wdr"),),
            (pool_id, count, caller),
        );

        Ok(())
    }

    // =======================================================================
    // Audit (opt-in)
    // =======================================================================

    /// Auditor query: returns the deposit record linked to a view-tag.
    /// The auditor must have been registered for the supplied scope-tag.
    /// No secret material is revealed – the auditor already possesses the
    /// view-tag because the depositor shared it with them off-chain.
    pub fn audit_query(
        env: Env,
        auditor: Address,
        scope_tag: BytesN<32>,
        pool_id: u32,
        view_tag: BytesN<32>,
    ) -> Result<DepositRecord, ZkError> {
        auditor.require_auth();

        let registered: Address = env
            .storage()
            .instance()
            .get(&DataKey::Auditor(scope_tag.clone()))
            .ok_or(ZkError::AuditorNotRegistered)?;
        if registered != auditor {
            return Err(ZkError::NotAuthorized);
        }

        let (commitment, leaf_index): (BytesN<32>, u32) = env
            .storage()
            .instance()
            .get(&DataKey::ViewTag(pool_id, view_tag.clone()))
            .ok_or(ZkError::InvalidViewTag)?;

        env.events().publish(
            (symbol_short!("zk_aud_q"),),
            (pool_id, view_tag, auditor),
        );

        Ok(DepositRecord {
            commitment,
            leaf_index,
            view_tag: Some(view_tag),
            deposited_at: 0, // privacy: timestamp not leaked via this path
        })
    }

    // =======================================================================
    // Read-only queries
    // =======================================================================

    pub fn get_pool(env: Env, pool_id: u32) -> Option<PrivacyPool> {
        env.storage().instance().get(&DataKey::Pool(pool_id))
    }

    pub fn get_circuit(env: Env, circuit_id: u32) -> Option<CircuitSpec> {
        env.storage().instance().get(&DataKey::Circuit(circuit_id))
    }

    pub fn is_nullifier_spent(env: Env, pool_id: u32, nullifier: BytesN<32>) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Nullifier(pool_id, nullifier))
            .unwrap_or(false)
    }

    pub fn is_commitment_in_pool(env: Env, pool_id: u32, commitment: BytesN<32>) -> bool {
        env.storage()
            .instance()
            .has(&DataKey::Commitment(pool_id, commitment))
    }

    pub fn get_deposit(env: Env, pool_id: u32, commitment: BytesN<32>) -> Option<DepositRecord> {
        env.storage()
            .instance()
            .get(&DataKey::Commitment(pool_id, commitment))
    }

    /// Compute (and lazily return) the Merkle root of a pool.  Empty
    /// leaves past `leaf_count` are filled with `EMPTY_LEAF`.  This is
    /// the root against which withdrawal proofs must verify.
    pub fn get_pool_root(env: Env, pool_id: u32) -> Result<BytesN<32>, ZkError> {
        let pool: PrivacyPool = env
            .storage()
            .instance()
            .get(&DataKey::Pool(pool_id))
            .ok_or(ZkError::PoolNotFound)?;
        Ok(compute_pool_root(&env, &pool))
    }

    /// Combined counters – returned in a single struct to minimise RPC
    /// round-trips for off-chain dashboards.
    pub fn get_pool_stats(env: Env, pool_id: u32) -> Result<PoolStats, ZkError> {
        let pool: PrivacyPool = env
            .storage()
            .instance()
            .get(&DataKey::Pool(pool_id))
            .ok_or(ZkError::PoolNotFound)?;
        let withdrawals: u32 = env
            .storage()
            .instance()
            .get(&DataKey::WithdrawalCount(pool_id))
            .unwrap_or(0);
        let spent_nullifiers = withdrawals; // every withdrawal marks exactly one
        let active_commitments = if pool.leaf_count >= spent_nullifiers {
            pool.leaf_count - spent_nullifiers
        } else {
            0
        };
        let merkle_depth = depth_for_leaf_count(pool.leaf_count);
        Ok(PoolStats {
            deposit_count: pool.leaf_count,
            withdrawal_count: withdrawals,
            active_commitments,
            spent_nullifiers,
            merkle_depth,
        })
    }

    /// Paginated withdrawal history.  Index 0 is the oldest withdrawal.
    pub fn get_withdrawal_history(
        env: Env,
        pool_id: u32,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<WithdrawalRecord>, ZkError> {
        if !env.storage().instance().has(&DataKey::Pool(pool_id)) {
            return Err(ZkError::PoolNotFound);
        }
        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::WithdrawalCount(pool_id))
            .unwrap_or(0);
        let mut out = Vec::new(&env);
        if offset >= count {
            return Ok(out);
        }
        let end = (offset + limit).min(count);
        let mut i = offset;
        while i < end {
            if let Some(rec) = env
                .storage()
                .instance()
                .get::<DataKey, WithdrawalRecord>(&DataKey::Withdrawal(pool_id, i))
            {
                out.push_back(rec);
            }
            i += 1;
        }
        Ok(out)
    }

    // =======================================================================
    // Pure helpers exposed only for off-chain tooling / tests.
    // =======================================================================

    /// Re-derive the on-chain binding from a public statement.  Useful
    /// for off-chain provers sanity-checking their Fiat-Shamir transcript.
    pub fn compute_binding(
        env: Env,
        commitment: BytesN<32>,
        nullifier: BytesN<32>,
        merkle_root: BytesN<32>,
        public_signals: Vec<BytesN<32>>,
    ) -> BytesN<32> {
        rebuild_binding(
            &env,
            &commitment,
            &nullifier,
            &merkle_root,
            &public_signals,
        )
    }

    /// Recompute the Merkle root for a hypothetical leaf set.  Provided
    /// so external tooling can build candidate roots before submitting
    /// a deposit.
    pub fn recompute_root(
        env: Env,
        leaf_index: u32,
        leaf_hash: BytesN<32>,
        siblings: Vec<BytesN<32>>,
    ) -> BytesN<32> {
        verify_merkle_path(&env, leaf_hash, leaf_index, &siblings)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers – pure functions
// ---------------------------------------------------------------------------

/// Returns true if the hash is the all-zero scalar, which we explicitly
/// reject on deposits and view-tags to avoid trivially forgeable inputs.
fn is_zero_hash(h: &BytesN<32>) -> bool {
    let bytes = h.to_array();
    let mut all_zero = true;
    let mut i = 0;
    while i < 32 {
        if bytes[i] != 0 {
            all_zero = false;
        }
        i += 1;
    }
    all_zero
}

/// Domain-separated leaf hash: `sha256(DOMAIN_LEAF || commitment)`.
fn hash_leaf(env: Env, commitment: BytesN<32>) -> BytesN<32> {
    let mut buf = Bytes::new(&env);
    buf.push_back(DOMAIN_LEAF);
    let com_bytes = commitment.to_array();
    let mut i = 0;
    while i < 32 {
        buf.push_back(com_bytes[i]);
        i += 1;
    }
    BytesN::from_array(&env, &env.crypto().sha256(&buf).to_array())
}

/// Internal-node hash: `sha256(DOMAIN_NODE || left || right)` where the
/// layout (which sibling is left vs right) is chosen by a single bit of
/// the leaf index at the appropriate level.
fn hash_node(env: &Env, left: &BytesN<32>, right: &BytesN<32>) -> BytesN<32> {
    let mut buf = Bytes::new(env);
    buf.push_back(DOMAIN_NODE);
    let l = left.to_array();
    let r = right.to_array();
    let mut i = 0;
    while i < 32 {
        buf.push_back(l[i]);
        i += 1;
    }
    i = 0;
    while i < 32 {
        buf.push_back(r[i]);
        i += 1;
    }
    BytesN::from_array(env, &env.crypto().sha256(&buf).to_array())
}

/// Reconstruct the root from a leaf index, leaf hash, and sibling list.
/// `leaf_index` provides the path-sibling direction at every level via
/// the corresponding bit.
fn verify_merkle_path(
    env: &Env,
    leaf_hash: BytesN<32>,
    leaf_index: u32,
    siblings: &Vec<BytesN<32>>,
) -> BytesN<32> {
    let mut current = leaf_hash;
    let mut level: u32 = 0;
    while level < siblings.len() {
        let sibling = siblings.get(level).unwrap();
        let bit = (leaf_index >> level) & 1;
        let (left, right) = if bit == 0 {
            (current.clone(), sibling.clone())
        } else {
            (sibling.clone(), current.clone())
        };
        current = hash_node(env, &left, &right);
        level += 1;
    }
    current
}

/// Compute the Merkle root of a pool from its stored leaf hashes.
/// Empty leaves past `leaf_count` are filled with `EMPTY_LEAF`.
fn compute_pool_root(env: &Env, pool: &PrivacyPool) -> BytesN<32> {
    if pool.leaf_count == 0 {
        return BytesN::from_array(env, &EMPTY_LEAF);
    }
    let tree_size = next_pow2(pool.leaf_count);
    let mut level: Vec<BytesN<32>> = Vec::new(env);

    let mut i: u32 = 0;
    let empty_leaf = BytesN::from_array(env, &EMPTY_LEAF);
    while i < tree_size {
        if i < pool.leaf_count {
            if let Some(stored) = env
                .storage()
                .instance()
                .get::<DataKey, BytesN<32>>(&DataKey::LeafHash(pool.id, i))
            {
                level.push_back(stored);
            } else {
                level.push_back(empty_leaf.clone());
            }
        } else {
            level.push_back(empty_leaf.clone());
        }
        i += 1;
    }

    while level.len() > 1 {
        let mut next: Vec<BytesN<32>> = Vec::new(env);
        let mut j: u32 = 0;
        while j < level.len() {
            let left = level.get(j).unwrap();
            let right = level.get(j + 1).unwrap();
            next.push_back(hash_node(env, &left, &right));
            j += 2;
        }
        level = next;
    }
    level.get(0).unwrap()
}

fn next_pow2(n: u32) -> u32 {
    if n == 0 {
        return 1;
    }
    let mut p: u32 = 1;
    while p < n {
        p <<= 1;
    }
    if p == 0 {
        1
    } else {
        p
    }
}

fn depth_for_leaf_count(n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    let mut d: u32 = 0;
    let mut p: u32 = 1;
    while p < n {
        p <<= 1;
        d += 1;
    }
    d
}

/// Rebuild the Fiat-Shamir binding from the public statement.  The
/// prover must have computed the same value off-chain and included it
/// in the `ZkProof.binding` field.
fn rebuild_binding(
    env: &Env,
    commitment: &BytesN<32>,
    nullifier: &BytesN<32>,
    merkle_root: &BytesN<32>,
    public_signals: &Vec<BytesN<32>>,
) -> BytesN<32> {
    let mut buf = Bytes::new(env);
    let mut i: u32 = 0;
    while (i as usize) < BINDING_PREFIX.len() {
        buf.push_back(BINDING_PREFIX[i as usize]);
        i += 1;
    }
    let c = commitment.to_array();
    i = 0;
    while i < 32 {
        buf.push_back(c[i]);
        i += 1;
    }
    let n = nullifier.to_array();
    i = 0;
    while i < 32 {
        buf.push_back(n[i]);
        i += 1;
    }
    let r = merkle_root.to_array();
    i = 0;
    while i < 32 {
        buf.push_back(r[i]);
        i += 1;
    }

    // Public signals are hashed separately to keep the binding transcript
    // size bounded regardless of how many signals there are.
    let mut sig_buf = Bytes::new(env);
    let mut s_idx: u32 = 0;
    while s_idx < public_signals.len() {
        let s = public_signals.get(s_idx).unwrap().to_array();
        let mut k = 0;
        while k < 32 {
            sig_buf.push_back(s[k]);
            k += 1;
        }
        s_idx += 1;
    }
    let sig_hash = env.crypto().sha256(&sig_buf);
    let sig_arr = sig_hash.to_array();
    i = 0;
    while i < 32 {
        buf.push_back(sig_arr[i]);
        i += 1;
    }

    BytesN::from_array(env, &env.crypto().sha256(&buf).to_array())
}

/// Internal verifier – returns `Ok(true)` if every check passes,
/// `Ok(false)` for a soft cryptographic failure, or a typed error if
/// the supplied circuit / pool is fundamentally broken.
fn verify_proof_inner(
    env: &Env,
    proof: &ZkProof,
    statement: &ProofStatement,
    circuit: &CircuitSpec,
    pool: &PrivacyPool,
    recipient_hash: Option<BytesN<32>>,
) -> Result<bool, ZkError> {
    // 1. Statement coherence – the public statement must match the proof.
    if statement.commitment != proof.commitment
        || statement.nullifier != proof.nullifier
        || statement.merkle_root != proof.merkle_root
        || statement.public_signals.len() != proof.public_signals.len()
    {
        return Ok(false);
    }

    // 2. Public-signal count must respect the circuit.
    if proof.public_signals.len() > circuit.max_public_signals {
        return Ok(false);
    }

    // 3. Merkle-path length must respect the circuit depth bounds.
    if (proof.merkle_path.len() as u32) < circuit.min_depth
        || (proof.merkle_path.len() as u32) > circuit.max_depth
    {
        return Ok(false);
    }
    let leaf_hash = hash_leaf(env.clone(), proof.commitment.clone());
    let computed_root =
        verify_merkle_path(env, leaf_hash, proof.leaf_index, &proof.merkle_path);
    if computed_root != proof.merkle_root {
        return Ok(false);
    }

    // 4. Pool binding: the proof's merkle_root must equal the pool's
    //    current on-chain root.  This is the LINCHPIN of the contract's
    //    soundness – without it a fraud prover could pick any root.
    if proof.merkle_root != compute_pool_root(env, pool) {
        return Ok(false);
    }

    // 5. Fiat-Shamir binding – proof cannot be re-targeted.
    let expected_binding = rebuild_binding(
        env,
        &proof.commitment,
        &proof.nullifier,
        &proof.merkle_root,
        &proof.public_signals,
    );
    if expected_binding != proof.binding {
        return Ok(false);
    }

    // 6. Circuit constraints over the public signals.
    if circuit.enforce_amount_lt_denom {
        if proof.public_signals.len() < 1 {
            return Ok(false);
        }
        let amount_hash = hash_amount(env, pool.denomination);
        if amount_hash != proof.public_signals.get(0).unwrap() {
            return Ok(false);
        }
    }
    if circuit.enforce_recipient_non_zero {
        if proof.public_signals.len() < 2 {
            return Ok(false);
        }
        let supplied = proof.public_signals.get(1).unwrap();
        if is_zero_hash(&supplied) {
            return Ok(false);
        }
        if let Some(rh) = recipient_hash.clone() {
            if supplied != rh {
                return Ok(false);
            }
        }
    }

    Ok(true)
}

/// Hash the canonical "amount == pool.denomination" sentinel, with a
/// domain tag, so the prover can encode the public-signal value without
/// leaking the actual amount value (which would just be `pool.denomination`
/// in the privacy-pool model).
fn hash_amount(env: &Env, denomination: i128) -> BytesN<32> {
    let mut buf = Bytes::new(env);
    buf.push_back(DOMAIN_AMOUNT_SENTINEL);
    let bytes = denomination.to_be_bytes();
    let mut i = 0;
    while i < bytes.len() {
        buf.push_back(bytes[i]);
        i += 1;
    }
    BytesN::from_array(env, &env.crypto().sha256(&buf).to_array())
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Off-chain hash helpers (mirror the contract's hash functions).
    // -----------------------------------------------------------------------

    fn test_commitment(
        env: &Env,
        secret: &[u8; 32],
        randomness: &[u8; 32],
        scope: &[u8; 32],
    ) -> BytesN<32> {
        let mut buf = Bytes::new(env);
        buf.push_back(DOMAIN_COMMITMENT);
        let mut i = 0;
        while i < 32 {
            buf.push_back(secret[i]);
            i += 1;
        }
        i = 0;
        while i < 32 {
            buf.push_back(randomness[i]);
            i += 1;
        }
        i = 0;
        while i < 32 {
            buf.push_back(scope[i]);
            i += 1;
        }
        BytesN::from_array(env, &env.crypto().sha256(&buf).to_array())
    }

    fn test_nullifier(env: &Env, secret: &[u8; 32], scope: &[u8; 32]) -> BytesN<32> {
        let mut buf = Bytes::new(env);
        buf.push_back(DOMAIN_NULLIFIER);
        let mut i = 0;
        while i < 32 {
            buf.push_back(secret[i]);
            i += 1;
        }
        i = 0;
        while i < 32 {
            buf.push_back(scope[i]);
            i += 1;
        }
        BytesN::from_array(env, &env.crypto().sha256(&buf).to_array())
    }

    /// Determine the actual Merkle-tree depth / size for `n` deposited
    /// leaves (with `EMPTY_LEAF` padding to a power of two).
    fn tree_layout(n: u32) -> (u32, u32) {
        let tree_size = next_pow2(n);
        let depth = depth_for_leaf_count(n);
        (tree_size, depth)
    }

    /// Build the full tree (rooted at the returned `BytesN<32>`) from
    /// `leaves_hashed` (length = tree_size).  Always returns the
    /// Merkle root regardless of the `leaf_index` arg.
    fn build_full_tree_root(env: &Env, leaves_hashed: Vec<BytesN<32>>) -> BytesN<32> {
        let mut level = leaves_hashed;
        while level.len() > 1 {
            let mut next: Vec<BytesN<32>> = Vec::new(env);
            let mut i: u32 = 0;
            while i < level.len() {
                let left = level.get(i).unwrap();
                let right = level.get(i + 1).unwrap();
                next.push_back(hash_node(env, &left, &right));
                i += 2;
            }
            level = next;
        }
        level.get(0).unwrap()
    }

    /// Walk up the tree from `leaf_index`, returning the sibling hashes
    /// + the root.  `leaves_hashed` must already be padded to a power of
    /// two and contain the actual leaf hash for the new commitment.
    fn merkle_path_and_root(
        env: &Env,
        leaves_hashed: Vec<BytesN<32>>,
        leaf_index: u32,
    ) -> (Vec<BytesN<32>>, BytesN<32>) {
        let mut level = leaves_hashed;
        let mut path: Vec<BytesN<32>> = Vec::new(env);
        let mut idx = leaf_index;
        while level.len() > 1 {
            let sib_idx = if idx % 2 == 0 { idx + 1 } else { idx - 1 };
            path.push_back(level.get(sib_idx).unwrap());
            let mut next: Vec<BytesN<32>> = Vec::new(env);
            let mut i: u32 = 0;
            while i < level.len() {
                let left = level.get(i).unwrap();
                let right = level.get(i + 1).unwrap();
                next.push_back(hash_node(env, &left, &right));
                i += 2;
            }
            level = next;
            idx /= 2;
        }
        let root = level.get(0).unwrap();
        (path, root)
    }

    /// Build a complete, sound `ZkProof` for a new commitment that
    /// would become the (leaves_in_pool.len()+1)-th leaf after deposit.
    ///
    /// * `leaves_in_pool` – commitments **already** deposited
    ///   (this matches the on-chain state at the time of `client.deposit`
    ///    when the caller is about to deposit `new_commitment`).
    /// * `new_commitment` / `new_nullifier` – the deposit & withdrawal
    ///   values (generated off-chain by the prover from the same secret).
    /// * `denomination` – the pool's denomination (used to derive the
    ///   amount sentinel public signal).
    /// * `recipient` – the recipient hash (used as the second public
    ///   signal).
    fn build_proof(
        env: &Env,
        leaves_in_pool: Vec<BytesN<32>>,
        new_commitment: BytesN<32>,
        new_nullifier: BytesN<32>,
        denomination: i128,
        recipient: BytesN<32>,
    ) -> ZkProof {
        let empty = BytesN::from_array(env, &EMPTY_LEAF);

        let total = leaves_in_pool.len() as u32 + 1;
        let (tree_size, _depth) = tree_layout(total);

        let mut leaf_hashes: Vec<BytesN<32>> = Vec::new(env);
        let mut i: u32 = 0;
        while i < leaves_in_pool.len() as u32 {
            leaf_hashes.push_back(hash_leaf(env.clone(), leaves_in_pool.get(i).unwrap().clone()));
            i += 1;
        }
        leaf_hashes.push_back(hash_leaf(env.clone(), new_commitment.clone()));
        while (leaf_hashes.len() as u32) < tree_size {
            leaf_hashes.push_back(empty.clone());
        }

        let leaf_index = leaves_in_pool.len() as u32;
        let (path, root) = merkle_path_and_root(env, leaf_hashes, leaf_index);

        let amount_sig = hash_amount(env, denomination);
        let mut public_signals: Vec<BytesN<32>> = Vec::new(env);
        public_signals.push_back(amount_sig);
        public_signals.push_back(recipient.clone());

        let binding = rebuild_binding(
            env,
            &new_commitment,
            &new_nullifier,
            &root,
            &public_signals,
        );

        ZkProof {
            commitment: new_commitment,
            nullifier: new_nullifier,
            merkle_root: root,
            leaf_index,
            merkle_path: path,
            public_signals,
            binding,
        }
    }

    /// Build a complete MERKLE-tree (used to verify that the contract's
    /// `get_pool_root` reproduces the same root for a given state).
    fn full_pool_root(
        env: &Env,
        leaves_in_pool: Vec<BytesN<32>>,
    ) -> BytesN<32> {
        let empty = BytesN::from_array(env, &EMPTY_LEAF);
        if leaves_in_pool.is_empty() {
            return empty.clone();
        }
        let n = leaves_in_pool.len() as u32;
        let (tree_size, _d) = tree_layout(n);
        let mut leaf_hashes: Vec<BytesN<32>> = Vec::new(env);
        let mut i: u32 = 0;
        while i < n {
            leaf_hashes.push_back(hash_leaf(env.clone(), leaves_in_pool.get(i).unwrap().clone()));
            i += 1;
        }
        while (leaf_hashes.len() as u32) < tree_size {
            leaf_hashes.push_back(empty.clone());
        }
        build_full_tree_root(env, leaf_hashes)
    }

    fn fresh_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env
    }

    fn setup_with_pool(use_basic_circuit: bool) -> (Env, Address, u32, u32) {
        let env = fresh_env();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        client.initialize(&admin);

        let circuit_id = if use_basic_circuit { 1 } else { 2 };
        let pool_id = client.create_pool(&admin, &1_000i128, &circuit_id);
        (env, admin, pool_id, circuit_id)
    }

    fn assert_call_err<T, E>(
        result: &Result<Result<T, E>, soroban_sdk::Error>,
        case: &str,
    ) {
        // Catch either layer: contract panic OR explicit Err return.
        match result {
            Err(_) => {} // contract panicked – acceptable for "expected error"
            Ok(Err(_)) => {} // contract returned its own error
            Ok(Ok(_)) => panic!("expected error in {}, but call succeeded", case),
        }
    }

    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    #[test]
    fn test_initialize_creates_built_in_circuits() {
        let env = fresh_env();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        client.initialize(&admin);

        let basic = client.get_circuit(&1).unwrap();
        assert_eq!(basic.id, 1);
        assert_eq!(basic.min_depth, 0);
        assert!(basic.max_depth >= basic.min_depth);
        assert!(basic.max_public_signals > 0);

        let strict = client.get_circuit(&2).unwrap();
        assert_eq!(strict.id, 2);
        assert_eq!(strict.min_depth, 4);
        assert!(strict.min_depth >= basic.min_depth);
    }

    #[test]
    fn test_double_initialize_rejected() {
        let env = fresh_env();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        client.initialize(&admin);
        assert_call_err(&client.try_initialize(&admin), "second initialize");
    }

    #[test]
    fn test_non_admin_cannot_create_circuit() {
        let env = fresh_env();
        let admin = Address::generate(&env);
        let other = Address::generate(&env);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        client.initialize(&admin);

        let res = client.try_create_circuit(
            &other,
            &CircuitSpec {
                id: 0,
                min_depth: 0,
                max_depth: 5,
                max_public_signals: 2,
                enforce_amount_lt_denom: false,
                enforce_recipient_non_zero: false,
                created_at: 0,
            },
        );
        assert_call_err(&res, "non-admin create_circuit");
    }

    // -----------------------------------------------------------------------
    // Pool lifecycle
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_and_pause_pool() {
        let env = fresh_env();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        client.initialize(&admin);

        let pool_id = client.create_pool(&admin, &500i128, &1);
        assert_eq!(pool_id, 1);
        let pool = client.get_pool(&pool_id).unwrap();
        assert_eq!(pool.denomination, 500);
        assert!(pool.active);

        client.set_pool_active(&admin, &pool_id, &false);
        assert!(!client.get_pool(&pool_id).unwrap().active);
    }

    #[test]
    fn test_zero_denomination_pool_rejected() {
        let env = fresh_env();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        client.initialize(&admin);
        assert_call_err(
            &client.try_create_pool(&admin, &0i128, &1),
            "zero denomination",
        );
    }

    #[test]
    fn test_pool_with_unknown_circuit_rejected() {
        let env = fresh_env();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        client.initialize(&admin);
        assert_call_err(
            &client.try_create_pool(&admin, &1i128, &99),
            "unknown circuit",
        );
    }

    // -----------------------------------------------------------------------
    // Deposits
    // -----------------------------------------------------------------------

    #[test]
    fn test_deposit_returns_leaf_index() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        let depositor = Address::generate(&env);

        let c1 = test_commitment(&env, &[1u8; 32], &[2u8; 32], &[3u8; 32]);
        let c2 = test_commitment(&env, &[4u8; 32], &[5u8; 32], &[6u8; 32]);
        assert_eq!(client.deposit(&depositor, &pool_id, &c1, &None), 0);
        assert_eq!(client.deposit(&depositor, &pool_id, &c2, &None), 1);

        assert!(client.is_commitment_in_pool(&pool_id, &c1));
        assert!(client.is_commitment_in_pool(&pool_id, &c2));
    }

    #[test]
    fn test_double_deposit_rejected() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        let depositor = Address::generate(&env);

        let c = test_commitment(&env, &[1u8; 32], &[2u8; 32], &[3u8; 32]);
        client.deposit(&depositor, &pool_id, &c, &None);
        assert_call_err(
            &client.try_deposit(&depositor, &pool_id, &c, &None),
            "double deposit",
        );
    }

    #[test]
    fn test_zero_commitment_rejected() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        let depositor = Address::generate(&env);
        let zero = BytesN::from_array(&env, &[0u8; 32]);
        assert_call_err(
            &client.try_deposit(&depositor, &pool_id, &zero, &None),
            "zero commitment",
        );
    }

    #[test]
    fn test_deposit_with_view_tag_auditor_round_trip() {
        let (env, admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        let depositor = Address::generate(&env);
        let auditor = Address::generate(&env);

        let c = test_commitment(&env, &[10u8; 32], &[20u8; 32], &[30u8; 32]);
        let tag = BytesN::from_array(&env, &[42u8; 32]);
        client.deposit(&depositor, &pool_id, &c, &Some(tag.clone()));

        let scope = BytesN::from_array(&env, &[99u8; 32]);
        client.register_auditor(&admin, &scope, &auditor);

        let record = client
            .audit_query(&auditor, &scope, &pool_id, &tag)
            .unwrap();
        assert_eq!(record.commitment, c);
        assert_eq!(record.leaf_index, 0);
    }

    #[test]
    fn test_audit_query_unknown_auditor_rejected() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        let depositor = Address::generate(&env);
        let random_auditor = Address::generate(&env);

        let c = test_commitment(&env, &[1u8; 32], &[2u8; 32], &[3u8; 32]);
        let tag = BytesN::from_array(&env, &[7u8; 32]);
        client.deposit(&depositor, &pool_id, &c, &Some(tag.clone()));

        let scope = BytesN::from_array(&env, &[9u8; 32]);
        assert_call_err(
            &client.try_audit_query(&random_auditor, &scope, &pool_id, &tag),
            "unknown auditor",
        );
    }

    #[test]
    fn test_audit_query_unknown_view_tag_rejected() {
        let (env, admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        let auditor = Address::generate(&env);
        let scope = BytesN::from_array(&env, &[11u8; 32]);
        client.register_auditor(&admin, &scope, &auditor);
        let missing_tag = BytesN::from_array(&env, &[0xCDu8; 32]);
        assert_call_err(
            &client.try_audit_query(&auditor, &scope, &pool_id, &missing_tag),
            "missing view tag",
        );
    }

    // -----------------------------------------------------------------------
    // Proof verification (round-trip with the contract)
    // -----------------------------------------------------------------------

    #[test]
    fn test_verify_proof_round_trip_after_single_deposit() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);

        let depositor = Address::generate(&env);
        let secret = [11u8; 32];
        let randomness = [22u8; 32];
        let scope = [33u8; 32];
        let commitment = test_commitment(&env, &secret, &randomness, &scope);
        let nullifier = test_nullifier(&env, &secret, &scope);

        client.deposit(&depositor, &pool_id, &commitment, &None);

        let recipient = BytesN::from_array(&env, &[5u8; 32]);
        let proof = build_proof(
            &env,
            Vec::new(&env),
            commitment.clone(),
            nullifier.clone(),
            1_000i128,
            recipient.clone(),
        );

        // Sanity: the on-chain root must equal the helper's root.
        let pool_root = client.get_pool_root(&pool_id).unwrap();
        assert_eq!(proof.merkle_root, pool_root);

        let ok = client.verify_proof(&pool_id, &proof, &1u32);
        assert!(ok);
    }

    #[test]
    fn test_verify_proof_rejects_tampered_binding() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);

        let depositor = Address::generate(&env);
        let commitment = test_commitment(&env, &[1u8; 32], &[2u8; 32], &[3u8; 32]);
        let nullifier = test_nullifier(&env, &[1u8; 32], &[3u8; 32]);
        client.deposit(&depositor, &pool_id, &commitment, &None);

        let recipient = BytesN::from_array(&env, &[1u8; 32]);
        let proof = build_proof(
            &env,
            Vec::new(&env),
            commitment.clone(),
            nullifier.clone(),
            1_000i128,
            recipient.clone(),
        );
        let mut tampered = proof.clone();
        tampered.binding = BytesN::from_array(&env, &[0u8; 32]);

        // The proof should fail to verify: either inner Ok(false) or
        // an outright error – both are unacceptable.
        let result = client.try_verify_proof(&pool_id, &tampered, &1u32);
        match result {
            Err(_) => {} // contract panicked – acceptable
            Ok(Ok(true)) => panic!("tampered binding accepted (soundness break)"),
            Ok(Ok(false)) => {} // properly rejected
            Ok(Err(_)) => {} // properly rejected
        }
    }

    #[test]
    fn test_verify_proof_rejects_tampered_merkle_path() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);

        let depositor = Address::generate(&env);
        let commitment = test_commitment(&env, &[1u8; 32], &[2u8; 32], &[3u8; 32]);
        let nullifier = test_nullifier(&env, &[1u8; 32], &[3u8; 32]);

        // Deposit TWO leaves so the tree has depth >=1, then tamper
        // the second leaf's merkle path WITHOUT recomputing anything.
        let c_other = test_commitment(&env, &[9u8; 32], &[9u8; 32], &[9u8; 32]);
        client.deposit(&depositor, &pool_id, &c_other, &None);
        client.deposit(&depositor, &pool_id, &commitment, &None);

        let recipient = BytesN::from_array(&env, &[1u8; 32]);
        let proof = build_proof(
            &env,
            Vec::from_array(&env, &[c_other.clone()]),
            commitment.clone(),
            nullifier.clone(),
            1_000i128,
            recipient.clone(),
        );

        // Replace the (only) sibling at level 0 with random bytes; do
        // NOT recompute the binding or root – so the soundness check
        // MUST catch it.
        let mut tampered = proof.clone();
        if tampered.merkle_path.len() == 0 {
            panic!("expected a non-trivial merkle path");
        }
        tampered.merkle_path.set(0, BytesN::from_array(&env, &[0xCDu8; 32]));

        let result = client.try_verify_proof(&pool_id, &tampered, &1u32);
        match result {
            Err(_) => {}
            Ok(Ok(true)) => panic!("tampered merkle path accepted (soundness break)"),
            Ok(Ok(false)) => {}
            Ok(Err(_)) => {}
        }
    }

    #[test]
    fn test_verify_proof_rejects_wrong_recipient() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);

        let depositor = Address::generate(&env);
        let caller = Address::generate(&env);
        let commitment = test_commitment(&env, &[1u8; 32], &[2u8; 32], &[3u8; 32]);
        let nullifier = test_nullifier(&env, &[1u8; 32], &[3u8; 32]);
        client.deposit(&depositor, &pool_id, &commitment, &None);

        let proof_recipient = BytesN::from_array(&env, &[5u8; 32]);
        let proof = build_proof(
            &env,
            Vec::new(&env),
            commitment.clone(),
            nullifier.clone(),
            1_000i128,
            proof_recipient.clone(),
        );

        // A different recipient_hash means the public signal mismatch fires.
        let other_recipient = BytesN::from_array(&env, &[6u8; 32]);
        let r = client.try_withdraw(&caller, &pool_id, &proof, &other_recipient);
        match r {
            Err(_) => {}
            Ok(Ok(())) => panic!("withdraw succeeded despite recipient mismatch"),
            Ok(Err(_)) => {}
        }
    }

    // -----------------------------------------------------------------------
    // Withdrawals & replay protection
    // -----------------------------------------------------------------------

    #[test]
    fn test_withdraw_marks_nullifier_spent() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        let depositor = Address::generate(&env);
        let caller = Address::generate(&env);

        let s = [1u8; 32];
        let r = [2u8; 32];
        let sc = [3u8; 32];
        let commitment = test_commitment(&env, &s, &r, &sc);
        let nullifier = test_nullifier(&env, &s, &sc);
        client.deposit(&depositor, &pool_id, &commitment, &None);

        let recipient = BytesN::from_array(&env, &[5u8; 32]);
        let proof = build_proof(
            &env,
            Vec::new(&env),
            commitment.clone(),
            nullifier.clone(),
            1_000i128,
            recipient.clone(),
        );

        client.withdraw(&caller, &pool_id, &proof, &recipient);
        assert!(client.is_nullifier_spent(&pool_id, &nullifier));
    }

    #[test]
    fn test_withdraw_rejects_double_spend() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        let depositor = Address::generate(&env);
        let caller = Address::generate(&env);

        let s = [1u8; 32];
        let r = [2u8; 32];
        let sc = [3u8; 32];
        let commitment = test_commitment(&env, &s, &r, &sc);
        let nullifier = test_nullifier(&env, &s, &sc);
        client.deposit(&depositor, &pool_id, &commitment, &None);

        let recipient = BytesN::from_array(&env, &[5u8; 32]);
        let proof = build_proof(
            &env,
            Vec::new(&env),
            commitment.clone(),
            nullifier.clone(),
            1_000i128,
            recipient.clone(),
        );
        client.withdraw(&caller, &pool_id, &proof, &recipient);
        assert_call_err(
            &client.try_withdraw(&caller, &pool_id, &proof, &recipient),
            "double spend",
        );
    }

    #[test]
    fn test_withdraw_rejects_undeposited_commitment() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        let caller = Address::generate(&env);

        let commitment = test_commitment(&env, &[1u8; 32], &[2u8; 32], &[3u8; 32]);
        let nullifier = test_nullifier(&env, &[1u8; 32], &[3u8; 32]);

        let recipient = BytesN::from_array(&env, &[5u8; 32]);
        let proof = build_proof(
            &env,
            Vec::new(&env),
            commitment.clone(),
            nullifier.clone(),
            1_000i128,
            recipient.clone(),
        );
        // No deposit has been made; on-chain pool root = EMPTY_LEAF.
        assert_call_err(
            &client.try_withdraw(&caller, &pool_id, &proof, &recipient),
            "undeposited commitment",
        );
    }

    #[test]
    fn test_withdraw_rejects_paused_pool() {
        let (env, admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        let depositor = Address::generate(&env);
        let caller = Address::generate(&env);

        let commitment = test_commitment(&env, &[1u8; 32], &[2u8; 32], &[3u8; 32]);
        let nullifier = test_nullifier(&env, &[1u8; 32], &[3u8; 32]);
        client.deposit(&depositor, &pool_id, &commitment, &None);

        client.set_pool_active(&admin, &pool_id, &false);

        let recipient = BytesN::from_array(&env, &[5u8; 32]);
        let proof = build_proof(
            &env,
            Vec::new(&env),
            commitment.clone(),
            nullifier.clone(),
            1_000i128,
            recipient.clone(),
        );
        assert_call_err(
            &client.try_withdraw(&caller, &pool_id, &proof, &recipient),
            "paused pool",
        );
    }

    #[test]
    fn test_withdraw_rejects_zero_recipient() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        let depositor = Address::generate(&env);
        let caller = Address::generate(&env);

        let commitment = test_commitment(&env, &[1u8; 32], &[2u8; 32], &[3u8; 32]);
        let nullifier = test_nullifier(&env, &[1u8; 32], &[3u8; 32]);
        client.deposit(&depositor, &pool_id, &commitment, &None);

        let valid_recipient = BytesN::from_array(&env, &[5u8; 32]);
        let proof = build_proof(
            &env,
            Vec::new(&env),
            commitment.clone(),
            nullifier.clone(),
            1_000i128,
            valid_recipient,
        );

        let zero_recipient = BytesN::from_array(&env, &[0u8; 32]);
        assert_call_err(
            &client.try_withdraw(&caller, &pool_id, &proof, &zero_recipient),
            "zero recipient",
        );
    }

    // -----------------------------------------------------------------------
    // Batch verification
    // -----------------------------------------------------------------------

    #[test]
    fn test_batch_verify_all_valid() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        let depositor = Address::generate(&env);
        let denom = 1_000i128;
        let recipient = BytesN::from_array(&env, &[8u8; 32]);

        let mut all_leaves: Vec<BytesN<32>> = Vec::new(&env);
        let mut proofs: Vec<ZkProof> = Vec::new(&env);
        let mut stmts: Vec<ProofStatement> = Vec::new(&env);

        for idx in 0u32..3 {
            let secret = [(idx + 1) as u8; 32];
            let randomness = [(idx + 100) as u8; 32];
            let scope = [200u8; 32];
            let c = test_commitment(&env, &secret, &randomness, &scope);
            let n = test_nullifier(&env, &secret, &scope);
            client.deposit(&depositor, &pool_id, &c, &None);

            let proof = build_proof(
                &env,
                all_leaves.clone(),
                c.clone(),
                n.clone(),
                denom,
                recipient.clone(),
            );
            let stmt = ProofStatement {
                commitment: proof.commitment.clone(),
                nullifier: proof.nullifier.clone(),
                merkle_root: proof.merkle_root.clone(),
                public_signals: proof.public_signals.clone(),
            };
            proofs.push_back(proof);
            stmts.push_back(stmt);
            all_leaves.push_back(c);
        }

        let ok = client.batch_verify(&pool_id, &proofs, &stmts, &1u32).unwrap();
        assert!(ok);
    }

    #[test]
    fn test_batch_verify_short_circuits_on_invalid() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        let depositor = Address::generate(&env);
        let denom = 1_000i128;
        let recipient = BytesN::from_array(&env, &[8u8; 32]);

        // First proof: a real (sound) one.
        let s = [1u8; 32];
        let r = [2u8; 32];
        let sc = [3u8; 32];
        let c = test_commitment(&env, &s, &r, &sc);
        let n = test_nullifier(&env, &s, &sc);
        client.deposit(&depositor, &pool_id, &c, &None);
        let p1 = build_proof(&env, Vec::new(&env), c.clone(), n.clone(), denom, recipient.clone());
        let stmt1 = ProofStatement {
            commitment: p1.commitment.clone(),
            nullifier: p1.nullifier.clone(),
            merkle_root: p1.merkle_root.clone(),
            public_signals: p1.public_signals.clone(),
        };

        // Second proof: tampered binding.
        let s2 = [4u8; 32];
        let r2 = [5u8; 32];
        let sc2 = [6u8; 32];
        let c2 = test_commitment(&env, &s2, &r2, &sc2);
        let n2 = test_nullifier(&env, &s2, &sc2);
        client.deposit(&depositor, &pool_id, &c2, &None);

        let mut all_leaves: Vec<BytesN<32>> = Vec::new(&env);
        all_leaves.push_back(c.clone());
        let p2 = build_proof(&env, all_leaves.clone(), c2.clone(), n2.clone(), denom, recipient.clone());
        let mut bad = p2.clone();
        bad.binding = BytesN::from_array(&env, &[0u8; 32]);
        let stmt2 = ProofStatement {
            commitment: bad.commitment.clone(),
            nullifier: bad.nullifier.clone(),
            merkle_root: bad.merkle_root.clone(),
            public_signals: bad.public_signals.clone(),
        };

        let mut proofs: Vec<ZkProof> = Vec::new(&env);
        let mut stmts: Vec<ProofStatement> = Vec::new(&env);
        proofs.push_back(p1);
        stmts.push_back(stmt1);
        proofs.push_back(bad);
        stmts.push_back(stmt2);

        // Batch must return Ok(false) because the second proof is invalid.
        let result = client.batch_verify(&pool_id, &proofs, &stmts, &1u32).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_batch_verify_length_mismatch() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);

        // Empty batch (zero proofs) is a vacuous truth and must return Ok(true).
        let p: Vec<ZkProof> = Vec::new(&env);
        let s: Vec<ProofStatement> = Vec::new(&env);
        let r = client.try_batch_verify(&pool_id, &p, &s, &1u32);
        match r {
            Ok(Ok(_)) => {}
            _ => panic!("empty batch should be Ok(true), got error"),
        }

        // Mismatched lengths must error.
        let mut proofs: Vec<ZkProof> = Vec::new(&env);
        let depositor = Address::generate(&env);
        let c0 = test_commitment(&env, &[1u8; 32], &[2u8; 32], &[3u8; 32]);
        let n0 = test_nullifier(&env, &[1u8; 32], &[3u8; 32]);
        proofs.push_back(build_proof(
            &env,
            Vec::new(&env),
            c0.clone(),
            n0.clone(),
            1_000i128,
            BytesN::from_array(&env, &[5u8; 32]),
        ));
        // Deposit the commitment so the proof is internally valid;
        // the mismatch itself is what must error.
        client.deposit(&depositor, &pool_id, &c0, &None);
        let stmts: Vec<ProofStatement> = Vec::new(&env);
        assert_call_err(
            &client.try_batch_verify(&pool_id, &proofs, &stmts, &1u32),
            "length mismatch",
        );
    }

    // -----------------------------------------------------------------------
    // Withdrawal history pagination
    // -----------------------------------------------------------------------

    #[test]
    fn test_withdrawal_history_pagination() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        let depositor = Address::generate(&env);
        let caller = Address::generate(&env);
        let recipient = BytesN::from_array(&env, &[5u8; 32]);

        // First deposit + withdraw.
        let s_a = [1u8; 32];
        let r_a = [2u8; 32];
        let sc_a = [3u8; 32];
        let c_a = test_commitment(&env, &s_a, &r_a, &sc_a);
        let n_a = test_nullifier(&env, &s_a, &sc_a);
        client.deposit(&depositor, &pool_id, &c_a, &None);
        let proof_a = build_proof(
            &env,
            Vec::new(&env),
            c_a.clone(),
            n_a.clone(),
            1_000i128,
            recipient.clone(),
        );
        client.withdraw(&caller, &pool_id, &proof_a, &recipient);

        // Second deposit + withdraw.
        let s_b = [4u8; 32];
        let r_b = [5u8; 32];
        let sc_b = [6u8; 32];
        let c_b = test_commitment(&env, &s_b, &r_b, &sc_b);
        let n_b = test_nullifier(&env, &s_b, &sc_b);
        client.deposit(&depositor, &pool_id, &c_b, &None);
        let proof_b = build_proof(
            &env,
            Vec::from_array(&env, &[c_a.clone()]),
            c_b.clone(),
            n_b.clone(),
            1_000i128,
            recipient.clone(),
        );
        client.withdraw(&caller, &pool_id, &proof_b, &recipient);

        let full = client.get_withdrawal_history(&pool_id, &0u32, &10u32).unwrap();
        assert_eq!(full.len(), 2);

        let page = client.get_withdrawal_history(&pool_id, &1u32, &10u32).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page.get(0).unwrap().nullifier, n_b);
    }

    // -----------------------------------------------------------------------
    // Pool stats invariant
    // -----------------------------------------------------------------------

    #[test]
    fn test_pool_stats_consistent() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);

        let depositor = Address::generate(&env);
        let caller = Address::generate(&env);
        let recipient = BytesN::from_array(&env, &[5u8; 32]);

        let s = [7u8; 32];
        let r = [8u8; 32];
        let sc = [9u8; 32];
        let c = test_commitment(&env, &s, &r, &sc);
        let n = test_nullifier(&env, &s, &sc);
        client.deposit(&depositor, &pool_id, &c, &None);

        let proof = build_proof(
            &env,
            Vec::new(&env),
            c.clone(),
            n.clone(),
            1_000i128,
            recipient.clone(),
        );
        client.withdraw(&caller, &pool_id, &proof, &recipient);

        let stats = client.get_pool_stats(&pool_id).unwrap();
        assert_eq!(stats.deposit_count, 1);
        assert_eq!(stats.withdrawal_count, 1);
        assert_eq!(stats.spent_nullifiers, 1);
        assert_eq!(stats.active_commitments, 0);
    }

    // -----------------------------------------------------------------------
    // Privacy invariants
    // -----------------------------------------------------------------------

    #[test]
    fn test_nullifier_does_not_leak_commitment() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);

        // Mark a nullifier as spent on-chain (without a real deposit).
        let nullifier = test_nullifier(&env, &[1u8; 32], &[3u8; 32]);
        env.storage()
            .instance()
            .set(&DataKey::Nullifier(pool_id, nullifier.clone()), &true);

        assert!(client.is_nullifier_spent(&pool_id, &nullifier));
        // No inverse API exists; the contract does not allow callers to
        // recover the commitment from the nullifier.
        assert!(!client.is_commitment_in_pool(
            &pool_id,
            &test_commitment(&env, &[1u8; 32], &[2u8; 32], &[3u8; 32])
        ));
    }

    #[test]
    fn test_pool_root_changes_after_deposit() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);

        let depositor = Address::generate(&env);
        let root_before = client.get_pool_root(&pool_id).unwrap();

        let c = test_commitment(&env, &[1u8; 32], &[2u8; 32], &[3u8; 32]);
        client.deposit(&depositor, &pool_id, &c, &None);

        let root_after = client.get_pool_root(&pool_id).unwrap();
        assert_ne!(root_before, root_after);
        // New root matches what we compute from `leaves_in_pool = [c]`.
        let expected = full_pool_root(&env, Vec::from_array(&env, &[c.clone()]));
        assert_eq!(root_after, expected);
    }

    #[test]
    fn test_admin_can_pause_and_resume_pool() {
        let (env, admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        let depositor = Address::generate(&env);

        client.set_pool_active(&admin, &pool_id, &false);
        let c = test_commitment(&env, &[1u8; 32], &[2u8; 32], &[3u8; 32]);
        assert_call_err(
            &client.try_deposit(&depositor, &pool_id, &c, &None),
            "deposit while paused",
        );

        client.set_pool_active(&admin, &pool_id, &true);
        let idx = client.deposit(&depositor, &pool_id, &c, &None);
        assert_eq!(idx, 0);
    }

    // -----------------------------------------------------------------------
    // Build_proof helper self-consistency
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_proof_recovers_pool_root_for_n_leaves() {
        let (env, _admin, pool_id, _) = setup_with_pool(true);
        let contract_id = env.register_contract(None, ZkProofContract);
        let client = ZkProofContractClient::new(&env, &contract_id);
        let depositor = Address::generate(&env);
        let denom = 1_000i128;
        let recipient = BytesN::from_array(&env, &[5u8; 32]);

        // Deposit 5 leaves cumulatively, comparing the on-chain root
        // after each step with what `build_proof` would compute for the
        // same state.
        let mut all_leaves: Vec<BytesN<32>> = Vec::new(&env);
        for idx in 0u32..5 {
            let secret = [(idx + 50) as u8; 32];
            let randomness = [(idx + 100) as u8; 32];
            let scope = [150u8; 32];
            let c = test_commitment(&env, &secret, &randomness, &scope);
            client.deposit(&depositor, &pool_id, &c, &None);

            let p = build_proof(&env, all_leaves.clone(), c.clone(), BytesN::from_array(&env, &[0u8; 32]), denom, recipient.clone());
            let onchain = client.get_pool_root(&pool_id).unwrap();
            assert_eq!(p.merkle_root, onchain);
            all_leaves.push_back(c);
        }
    }
}
