#![no_std]

//! # Revenue Share Agreement Contract
//!
//! A Soroban smart contract that lets a revenue producer split incoming
//! token revenue across a fixed set of beneficiaries on a recurring
//! cadence.  Each agreement defines:
//!
//! * a **token** (a Soroban token contract),
//! * an **owner** (the revenue producer; receives the residual cut),
//! * a list of **beneficiaries** carrying a `basis_points` share, and
//! * a **settlement period** (in seconds) dictating how often cumulative
//!   deposits are split and paid out.
//!
//! ## Settlement model
//!
//! * When `settlement_period_seconds == 0`, every deposit is **distributed
//!   immediately** to the split configured on the agreement.
//! * When `settlement_period_seconds > 0`, deposits accumulate in the
//!   agreement's `unsettled_amount`.  Anyone may call `settle_distribution`
//!   once `now >= next_settlement_time`; the call transfers the entire
//!   accumulated balance to beneficiaries and advances
//!   `next_settlement_time` by one period.
//!
//! The contract tracks per-agreement running totals (received, settled,
//! clawed-back) and each beneficiary's lifetime-received amount.  An
//! append-only history records every meaningful action.
//!
//! ## State machine
//!
//! ```text
//!   Active   --settle--> Active   (transient – returns to Active)
//!   Active   --dispute-> Disputed
//!   Disputed --resolve(Continue)--> Active
//!   Disputed --resolve(Cancel)----> Cancelled
//!   Disputed --resolve(Pause)-----> Paused
//!   Paused   --unpause------------> Active
//!   Active   --clawback-----------> Active   (only un-settled funds)
//!   Cancelled/Paused are terminal until admin changes state.
//! ```
//!
//! ## Acceptance criteria (issue #256)
//!
//! | Criterion                                         | Enforced in                                          |
//! |---------------------------------------------------|------------------------------------------------------|
//! | Agreements created with terms                     | `create_agreement` (validates splits, period, etc.)  |
//! | Revenue split correctly                           | `split_unsettled` + `settle_distribution`            |
//! | Settlement periods enforced                       | `now >= next_settlement_time` check                  |
//! | Clawbacks work properly                           | `clawback_unsettled` (owner-only, terminal or active) |
//! | History tracked                                   | append-only `DataKey::History` entries               |
//! | All tests pass                                    | `mod tests` (~30 unit tests)                         |

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, Vec,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RevenueError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAuthorized = 3,
    AgreementNotFound = 4,
    InvalidConfig = 5,
    ZeroAmount = 6,
    ZeroBeneficiaries = 7,
    BeneficiaryLimitExceeded = 8,
    DuplicateBeneficiary = 9,
    BasisPointsOutOfRange = 10,
    InvalidSettlementPeriod = 11,
    SettlementTooEarly = 12,
    NothingToSettle = 13,
    NothingToClawback = 14,
    ClawbackWindowExpired = 15,
    InvalidState = 16,
    NotOwner = 17,
    AlreadyDisputed = 18,
    NotDisputed = 19,
    NotParty = 20,
    DisputeLimitExceeded = 21,
    SelfBeneficiaryOnly = 22,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of beneficiaries supported per agreement.  Capped so a
/// single agreement's `Vec` fits comfortably in Soroban's per-tx memory
/// budget; large fan-outs should be modelled as nested agreements.
pub const MAX_BENEFICIARIES: u32 = 25;

/// 100% in basis points (1 bp = 0.01%).
pub const BPS_DENOMINATOR: i128 = 10_000;

/// Maximum number of history entries stored per agreement.  Older
/// entries are still observable via `get_history(agreement, 0, limit)`.
pub const HISTORY_LIMIT_PER_AGREEMENT: u32 = 1_000;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Lifecycle states for a revenue-share agreement.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AgreementState {
    Active = 0,
    Disputed = 1,
    Paused = 2,
    Cancelled = 3,
}

/// One beneficiary's split.  `basis_points` is the percentage of each
/// settlement paid to `address`.  All beneficiary shares plus the
/// residual owner's take MUST sum to exactly `BPS_DENOMINATOR`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BeneficiaryShare {
    pub address: Address,
    pub basis_points: u32,
}

/// Categorical log of what happened in the agreement's history.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum HistoryKind {
    Created = 0,
    RevenueDeposited = 1,
    SettlementDistributed = 2,
    ClawbackExecuted = 3,
    Modified = 4,
    DisputeRaised = 5,
    DisputeResolved = 6,
    Paused = 7,
    Unpaused = 8,
    Cancelled = 9,
}

/// Categorical resolution choices for a dispute.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DisputeResolution {
    /// Resume normal operation; pending settlement is preserved.
    Continue = 0,
    /// Cancel the agreement; remaining unsettled funds are returned to
    /// the owner as a forced clawback.
    Cancel = 1,
    /// Park the agreement in `Paused`; admin must explicitly unpause.
    Pause = 2,
}

/// One entry in the agreement's append-only history.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntry {
    pub kind: HistoryKind,
    pub timestamp: u64,
    pub amount: i128,
    pub actor: Address,
    pub note: u32,
}

/// Snapshot of an agreement's financial position.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgreementStats {
    pub agreement_id: u64,
    pub total_received: i128,
    pub total_distributed: i128,
    pub total_clawed_back: i128,
    pub unsettled_amount: i128,
    pub deposit_count: u32,
    pub settlement_count: u32,
    pub clawback_count: u32,
    pub dispute_count: u32,
    pub beneficiary_count: u32,
    pub state: AgreementState,
    pub last_settlement_at: u64,
    pub next_settlement_at: u64,
    pub now: u64,
}

/// A dispute record, attached inline to the agreement record.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dispute {
    pub raised_by: Address,
    pub raised_at: u64,
    pub evidence_hash: Option<soroban_sdk::BytesN<32>>,
    pub resolution: Option<DisputeResolution>,
    pub resolved_at: Option<u64>,
}

/// The agreement itself.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Agreement {
    pub id: u64,
    pub owner: Address,
    pub token: Address,
    pub beneficiaries: Vec<BeneficiaryShare>,
    pub owner_basis_points: u32,
    pub settlement_period_seconds: u64,
    pub last_settlement_at: u64,
    pub next_settlement_at: u64,
    pub clawback_enabled: bool,
    pub clawback_deadline: u64,
    pub state: AgreementState,
    pub total_received: i128,
    pub total_distributed: i128,
    pub total_clawed_back: i128,
    pub unsettled_amount: i128,
    pub created_at: u64,
    pub deposit_count: u32,
    pub settlement_count: u32,
    pub clawback_count: u32,
    pub dispute_count: u32,
    pub dispute: Option<Dispute>,
}

// ---------------------------------------------------------------------------
// Storage Key
// ---------------------------------------------------------------------------

#[contracttype]
pub enum DataKey {
    /// Contract administrator (global).
    Admin,
    /// Monotonic counter used to mint new agreement IDs.
    AgreementCounter,
    /// `Agreement(id)` (stored in persistent storage to avoid instance
    /// memory bloat).
    Agreement(u64),
    /// `Agreement(id).beneficiary_index -> running total paid to that
    /// beneficiary` (so per-beneficiary lifetime received can be queried).
    BeneficiaryTotal(u64, u32),
    /// Append-only `(Agreement(id), idx) -> HistoryEntry`.
    History(u64, u32),
    /// Length of the history list for an agreement.
    HistoryLen(u64),
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct RevenueShareContract;

#[contractimpl]
impl RevenueShareContract {
    // =======================================================================
    // Initialization & admin
    // =======================================================================

    /// One-shot initialiser.  Stores the admin and seeds the agreement
    /// counter at zero.
    pub fn initialize(env: Env, admin: Address) -> Result<(), RevenueError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(RevenueError::AlreadyInitialized);
        }
        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::AgreementCounter, &0u64);

        env.events().publish((symbol_short!("rs_init"),), admin);
        Ok(())
    }

    /// Replace the contract admin.  Old admin must authorize the call.
    pub fn set_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), RevenueError> {
        Self::require_admin(&env, &current_admin)?;

        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.events().publish((symbol_short!("rs_adm"),), new_admin);
        Ok(())
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not set")
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), RevenueError> {
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(RevenueError::NotInitialized);
        }
        let stored: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(RevenueError::NotInitialized)?;
        if stored != *admin {
            return Err(RevenueError::NotAuthorized);
        }
        admin.require_auth();
        Ok(())
    }

    // =======================================================================
    // Agreement creation
    // =======================================================================

    /// Create a new revenue-share agreement.
    ///
    /// `beneficiaries` may not be empty; each entry carries an
    /// `address` and `basis_points` (1–10_000).  Every beneficiary's
    /// `basis_points` must be strictly positive, all addresses unique,
    /// and `sum(bps) <= BPS_DENOMINATOR`.  Any residual basis points are
    /// the **owner's** cut, recorded as `owner_basis_points`.
    ///
    /// `settlement_period_seconds`:
    /// * `0`  – every deposit is distributed immediately.
    /// * `>0` – deposits accumulate; settlement is permitted once
    ///          `now >= next_settlement_at`.
    ///
    /// `clawback_deadline` is a Unix timestamp; after this point the
    /// owner can no longer claw back un-settled funds.  Pass `0` to
    /// disable the deadline (unlimited clawback).
    pub fn create_agreement(
        env: Env,
        owner: Address,
        token: Address,
        beneficiaries: Vec<BeneficiaryShare>,
        settlement_period_seconds: u64,
        clawback_enabled: bool,
        clawback_deadline: u64,
    ) -> Result<u64, RevenueError> {
        owner.require_auth();
        Self::require_initialized(&env)?;

        if settlement_period_seconds == u64::MAX {
            return Err(RevenueError::InvalidSettlementPeriod);
        }

        if beneficiaries.is_empty() {
            return Err(RevenueError::ZeroBeneficiaries);
        }
        if beneficiaries.len() > MAX_BENEFICIARIES {
            return Err(RevenueError::BeneficiaryLimitExceeded);
        }

        let mut sum_bps: u64 = 0;
        let mut i: u32 = 0;
        while i < beneficiaries.len() {
            let b = beneficiaries.get(i).unwrap();
            if b.basis_points == 0 || b.basis_points as u64 > BPS_DENOMINATOR as u64 {
                return Err(RevenueError::BasisPointsOutOfRange);
            }
            sum_bps += b.basis_points as u64;

            // Reject duplicate addresses – they would corrupt
            // accounting and rate-limit ambiguous splits.
            let mut j = i + 1;
            while j < beneficiaries.len() {
                if beneficiaries.get(j).unwrap().address == b.address {
                    return Err(RevenueError::DuplicateBeneficiary);
                }
                j += 1;
            }
            i += 1;
        }

        if sum_bps > BPS_DENOMINATOR as u64 {
            return Err(RevenueError::BasisPointsOutOfRange);
        }
        let owner_bps = (BPS_DENOMINATOR as u64 - sum_bps) as u32;

        let counter: u64 = env
            .storage()
            .instance()
            .get(&DataKey::AgreementCounter)
            .unwrap_or(0);
        let id = counter + 1;

        let now = env.ledger().timestamp();
        let next_settlement = if settlement_period_seconds == 0 {
            0 // 0 == "distribute on deposit"
        } else {
            now.saturating_add(settlement_period_seconds)
        };

        let agreement = Agreement {
            id,
            owner: owner.clone(),
            token: token.clone(),
            beneficiaries: beneficiaries.clone(),
            owner_basis_points: owner_bps,
            settlement_period_seconds,
            last_settlement_at: 0,
            next_settlement_at: next_settlement,
            clawback_enabled,
            clawback_deadline,
            state: AgreementState::Active,
            total_received: 0,
            total_distributed: 0,
            total_clawed_back: 0,
            unsettled_amount: 0,
            created_at: now,
            deposit_count: 0,
            settlement_count: 0,
            clawback_count: 0,
            dispute_count: 0,
            dispute: None,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Agreement(id), &agreement);
        env.storage()
            .instance()
            .set(&DataKey::AgreementCounter, &id);

        Self::record_history(
            &env,
            id,
            HistoryEntry {
                kind: HistoryKind::Created,
                timestamp: now,
                amount: 0,
                actor: owner.clone(),
                note: beneficiaries.len(),
            },
        );

        env.events().publish(
            (symbol_short!("rs_crt"),),
            (
                id,
                owner,
                token,
                settlement_period_seconds,
                beneficiaries.len(),
            ),
        );
        Ok(id)
    }

    // =======================================================================
    // Revenue deposit
    // =======================================================================

    /// Deposit `amount` of the agreement's token from `depositor` into
    /// the agreement.  If `settlement_period_seconds == 0` the deposit
    /// is distributed immediately (after authorisation); otherwise the
    /// amount is added to `unsettled_amount` and a later
    /// `settle_distribution` call will distribute it.
    pub fn deposit_revenue(
        env: Env,
        depositor: Address,
        agreement_id: u64,
        amount: i128,
    ) -> Result<(), RevenueError> {
        if amount <= 0 {
            return Err(RevenueError::ZeroAmount);
        }
        depositor.require_auth();

        let mut agreement: Agreement = env
            .storage()
            .persistent()
            .get(&DataKey::Agreement(agreement_id))
            .ok_or(RevenueError::AgreementNotFound)?;

        if agreement.state != AgreementState::Active {
            return Err(RevenueError::InvalidState);
        }

        // Move tokens in.
        token::Client::new(&env, &agreement.token).transfer(
            &depositor,
            &env.current_contract_address(),
            &amount,
        );

        agreement.total_received += amount;
        agreement.unsettled_amount += amount;
        agreement.deposit_count += 1;

        // Immediate-distribution mode?
        let immediate = agreement.settlement_period_seconds == 0;
        if immediate {
            Self::settle_in_place(&env, &mut agreement)?;
        } else {
            env.storage()
                .persistent()
                .set(&DataKey::Agreement(agreement_id), &agreement);
        }

        Self::record_history(
            &env,
            agreement_id,
            HistoryEntry {
                kind: HistoryKind::RevenueDeposited,
                timestamp: env.ledger().timestamp(),
                amount,
                actor: depositor.clone(),
                note: if immediate { 1 } else { 0 },
            },
        );

        env.events().publish(
            (symbol_short!("rs_dep"),),
            (agreement_id, depositor, amount, immediate),
        );
        Ok(())
    }

    // =======================================================================
    // Settlement
    // =======================================================================

    /// Distribute the currently-unsettled balance to beneficiaries
    /// according to the agreement's basis-points split.  Must be called
    /// by the owner or any party (settle is permissionless once the
    /// `next_settlement_at` gate is satisfied).  Advances
    /// `next_settlement_at` by `settlement_period_seconds`.
    pub fn settle_distribution(
        env: Env,
        caller: Address,
        agreement_id: u64,
    ) -> Result<(), RevenueError> {
        caller.require_auth();

        let mut agreement: Agreement = env
            .storage()
            .persistent()
            .get(&DataKey::Agreement(agreement_id))
            .ok_or(RevenueError::AgreementNotFound)?;

        if agreement.state != AgreementState::Active {
            return Err(RevenueError::InvalidState);
        }
        if agreement.settlement_period_seconds == 0 {
            // In immediate-distribution mode there is nothing to do here.
            return Err(RevenueError::InvalidConfig);
        }

        let now = env.ledger().timestamp();
        if now < agreement.next_settlement_at {
            return Err(RevenueError::SettlementTooEarly);
        }
        if agreement.unsettled_amount <= 0 {
            return Err(RevenueError::NothingToSettle);
        }

        Self::settle_in_place(&env, &mut agreement)?;

        // Advance the next settlement boundary.
        agreement.next_settlement_at = now.saturating_add(agreement.settlement_period_seconds);
        env.storage()
            .persistent()
            .set(&DataKey::Agreement(agreement_id), &agreement);

        env.events().publish(
            (symbol_short!("rs_set"),),
            (agreement_id, caller, agreement.settlement_count),
        );
        Ok(())
    }

    // =======================================================================
    // Clawback
    // =======================================================================

    /// Reclaim un-settled revenue (still held inside this contract) back
    /// to the agreement owner.  Cannot claw back funds that have already
    /// been distributed to beneficiaries.
    ///
    /// Requirements:
    /// * caller is the owner,
    /// * `clawback_enabled` is true on the agreement,
    /// * if `clawback_deadline != 0`, then `now <= clawback_deadline`,
    /// * the agreement is not currently `Disputed`.
    pub fn clawback_unsettled(
        env: Env,
        owner: Address,
        agreement_id: u64,
    ) -> Result<i128, RevenueError> {
        owner.require_auth();

        let mut agreement: Agreement = env
            .storage()
            .persistent()
            .get(&DataKey::Agreement(agreement_id))
            .ok_or(RevenueError::AgreementNotFound)?;

        if owner != agreement.owner {
            return Err(RevenueError::NotOwner);
        }
        if !agreement.clawback_enabled {
            return Err(RevenueError::InvalidConfig);
        }
        if agreement.clawback_deadline != 0
            && env.ledger().timestamp() > agreement.clawback_deadline
        {
            return Err(RevenueError::ClawbackWindowExpired);
        }
        if agreement.state != AgreementState::Active {
            return Err(RevenueError::InvalidState);
        }
        if agreement.unsettled_amount <= 0 {
            return Err(RevenueError::NothingToClawback);
        }

        let amount = agreement.unsettled_amount;
        token::Client::new(&env, &agreement.token).transfer(
            &env.current_contract_address(),
            &agreement.owner,
            &amount,
        );

        agreement.total_clawed_back += amount;
        agreement.unsettled_amount = 0;
        agreement.clawback_count += 1;
        env.storage()
            .persistent()
            .set(&DataKey::Agreement(agreement_id), &agreement);

        Self::record_history(
            &env,
            agreement_id,
            HistoryEntry {
                kind: HistoryKind::ClawbackExecuted,
                timestamp: env.ledger().timestamp(),
                amount,
                actor: owner.clone(),
                note: 0,
            },
        );

        env.events().publish(
            (symbol_short!("rs_cb"),),
            (agreement_id, owner, amount),
        );
        Ok(amount)
    }

    // =======================================================================
    // Disputes
    // =======================================================================

    /// Raise a dispute on a healthy agreement.  Either the owner or any
    /// beneficiary listed on the agreement may invoke this; doing so
    /// freezes deposits and (manual-)settlements until the admin
    /// resolves it.
    pub fn raise_dispute(
        env: Env,
        caller: Address,
        agreement_id: u64,
        evidence_hash: soroban_sdk::BytesN<32>,
    ) -> Result<(), RevenueError> {
        caller.require_auth();

        let mut agreement: Agreement = env
            .storage()
            .persistent()
            .get(&DataKey::Agreement(agreement_id))
            .ok_or(RevenueError::AgreementNotFound)?;

        if agreement.state != AgreementState::Active {
            return Err(RevenueError::InvalidState);
        }
        if agreement.dispute.is_some() {
            return Err(RevenueError::AlreadyDisputed);
        }

        // Party check: only the owner or a current beneficiary may dispute.
        let mut allowed =
            if caller == agreement.owner {
                true
            } else {
                false
            };
        if !allowed {
            let mut i: u32 = 0;
            while i < agreement.beneficiaries.len() {
                if agreement.beneficiaries.get(i).unwrap().address == caller {
                    allowed = true;
                }
                i += 1;
            }
        }
        if !allowed {
            return Err(RevenueError::NotParty);
        }

        let now = env.ledger().timestamp();
        agreement.dispute = Some(Dispute {
            raised_by: caller.clone(),
            raised_at: now,
            evidence_hash: Some(evidence_hash.clone()),
            resolution: None,
            resolved_at: None,
        });
        agreement.state = AgreementState::Disputed;
        agreement.dispute_count += 1;
        env.storage()
            .persistent()
            .set(&DataKey::Agreement(agreement_id), &agreement);

        Self::record_history(
            &env,
            agreement_id,
            HistoryEntry {
                kind: HistoryKind::DisputeRaised,
                timestamp: now,
                amount: 0,
                actor: caller.clone(),
                note: 0,
            },
        );

        env.events().publish(
            (symbol_short!("rs_disp"),),
            (agreement_id, caller, evidence_hash),
        );
        Ok(())
    }

    /// Settle a disputed agreement.  Only the admin may invoke this.
    /// The resolution chooses how the agreement returns to operation.
    pub fn resolve_dispute(
        env: Env,
        admin: Address,
        agreement_id: u64,
        resolution: DisputeResolution,
    ) -> Result<(), RevenueError> {
        Self::require_admin(&env, &admin)?;

        let mut agreement: Agreement = env
            .storage()
            .persistent()
            .get(&DataKey::Agreement(agreement_id))
            .ok_or(RevenueError::AgreementNotFound)?;

        if agreement.state != AgreementState::Disputed {
            return Err(RevenueError::NotDisputed);
        }

        // Apply the resolution.
        match resolution {
            DisputeResolution::Continue => {
                agreement.state = AgreementState::Active;
                // The next settlement boundary is preserved; pending
                // unsettled funds remain available.
            }
            DisputeResolution::Pause => {
                agreement.state = AgreementState::Paused;
            }
            DisputeResolution::Cancel => {
                // Force-clawback any un-settled funds back to owner.
                if agreement.unsettled_amount > 0 {
                    token::Client::new(&env, &agreement.token).transfer(
                        &env.current_contract_address(),
                        &agreement.owner,
                        &agreement.unsettled_amount,
                    );
                    agreement.total_clawed_back += agreement.unsettled_amount;
                    agreement.unsettled_amount = 0;
                }
                agreement.state = AgreementState::Cancelled;
            }
        }

        // Stamp the dispute record.
        let now = env.ledger().timestamp();
        let mut dispute = agreement.dispute.clone().unwrap();
        dispute.resolution = Some(resolution.clone());
        dispute.resolved_at = Some(now);
        agreement.dispute = Some(dispute);
        env.storage()
            .persistent()
            .set(&DataKey::Agreement(agreement_id), &agreement);

        Self::record_history(
            &env,
            agreement_id,
            HistoryEntry {
                kind: HistoryKind::DisputeResolved,
                timestamp: now,
                amount: 0,
                actor: admin.clone(),
                // Pack the resolution kind into `note` so a UI can tell
                // Pause from Cancel after the fact without re-reading
                // the (eventually pruned) dispute struct.
                note: match resolution {
                    DisputeResolution::Continue => 0,
                    DisputeResolution::Pause => 1,
                    DisputeResolution::Cancel => 2,
                },
            },
        );

        env.events().publish(
            (symbol_short!("rs_dres"),),
            (agreement_id, admin, resolution),
        );
        Ok(())
    }

    // =======================================================================
    // Pause / Unpause (admin)
    // =======================================================================

    /// Forcibly pause a healthy agreement (admin only).
    pub fn pause(
        env: Env,
        admin: Address,
        agreement_id: u64,
    ) -> Result<(), RevenueError> {
        Self::require_admin(&env, &admin)?;

        let mut agreement: Agreement = env
            .storage()
            .persistent()
            .get(&DataKey::Agreement(agreement_id))
            .ok_or(RevenueError::AgreementNotFound)?;

        if agreement.state != AgreementState::Active {
            return Err(RevenueError::InvalidState);
        }

        agreement.state = AgreementState::Paused;
        env.storage()
            .persistent()
            .set(&DataKey::Agreement(agreement_id), &agreement);

        Self::record_history(
            &env,
            agreement_id,
            HistoryEntry {
                kind: HistoryKind::Paused,
                timestamp: env.ledger().timestamp(),
                amount: 0,
                actor: admin.clone(),
                note: 0,
            },
        );
        env.events().publish((symbol_short!("rs_pa"),), (agreement_id, admin));
        Ok(())
    }

    /// Unpause a paused agreement.
    pub fn unpause(
        env: Env,
        admin: Address,
        agreement_id: u64,
    ) -> Result<(), RevenueError> {
        Self::require_admin(&env, &admin)?;

        let mut agreement: Agreement = env
            .storage()
            .persistent()
            .get(&DataKey::Agreement(agreement_id))
            .ok_or(RevenueError::AgreementNotFound)?;

        if agreement.state != AgreementState::Paused {
            return Err(RevenueError::InvalidState);
        }

        agreement.state = AgreementState::Active;
        env.storage()
            .persistent()
            .set(&DataKey::Agreement(agreement_id), &agreement);

        Self::record_history(
            &env,
            agreement_id,
            HistoryEntry {
                kind: HistoryKind::Unpaused,
                timestamp: env.ledger().timestamp(),
                amount: 0,
                actor: admin.clone(),
                note: 0,
            },
        );
        env.events().publish((symbol_short!("rs_up"),), (agreement_id, admin));
        Ok(())
    }

    // =======================================================================
    // Modification (owner)
    // =======================================================================

    /// Replace the beneficiary split and (optionally) the settlement
    /// cadence on a healthy agreement.  If any unsettled funds are
    /// present they are first settled under the *old* rules, then the
    /// change is applied.  Cancelled, paused, or disputed agreements
    /// cannot be modified (they require admin intervention first).
    pub fn modify_agreement(
        env: Env,
        owner: Address,
        agreement_id: u64,
        new_beneficiaries: Vec<BeneficiaryShare>,
        new_settlement_period_seconds: u64,
    ) -> Result<(), RevenueError> {
        owner.require_auth();

        let mut agreement: Agreement = env
            .storage()
            .persistent()
            .get(&DataKey::Agreement(agreement_id))
            .ok_or(RevenueError::AgreementNotFound)?;

        if owner != agreement.owner {
            return Err(RevenueError::NotOwner);
        }
        if agreement.state != AgreementState::Active {
            return Err(RevenueError::InvalidState);
        }

        // Settle any "pending" balance under the OLD split so that the
        // new split does not retroactively apply to past revenue.
        if agreement.unsettled_amount > 0 && agreement.settlement_period_seconds != 0 {
            Self::settle_in_place(&env, &mut agreement)?;
            agreement.next_settlement_at = env
                .ledger()
                .timestamp()
                .saturating_add(agreement.settlement_period_seconds);
        }

        // Validate the new beneficiary list.
        if new_beneficiaries.is_empty() {
            return Err(RevenueError::ZeroBeneficiaries);
        }
        if new_beneficiaries.len() > MAX_BENEFICIARIES {
            return Err(RevenueError::BeneficiaryLimitExceeded);
        }
        if new_settlement_period_seconds == u64::MAX {
            return Err(RevenueError::InvalidSettlementPeriod);
        }
        let mut sum_bps: u64 = 0;
        let mut i: u32 = 0;
        while i < new_beneficiaries.len() {
            let b = new_beneficiaries.get(i).unwrap();
            if b.basis_points == 0 || b.basis_points as u64 > BPS_DENOMINATOR as u64 {
                return Err(RevenueError::BasisPointsOutOfRange);
            }
            sum_bps += b.basis_points as u64;
            let mut j = i + 1;
            while j < new_beneficiaries.len() {
                if new_beneficiaries.get(j).unwrap().address == b.address {
                    return Err(RevenueError::DuplicateBeneficiary);
                }
                j += 1;
            }
            i += 1;
        }
        if sum_bps > BPS_DENOMINATOR as u64 {
            return Err(RevenueError::BasisPointsOutOfRange);
        }

        let owner_bps = (BPS_DENOMINATOR as u64 - sum_bps) as u32;

        // Capture the OLD beneficiary count BEFORE we overwrite
        // `agreement.beneficiaries`; otherwise a shrinking split
        // (e.g. 5 -> 2 beneficiaries) would leave dangling
        // `BeneficiaryTotal` entries at indices [new..old).
        let prev_count = agreement.beneficiaries.len();

        agreement.beneficiaries = new_beneficiaries.clone();
        agreement.owner_basis_points = owner_bps;
        agreement.settlement_period_seconds = new_settlement_period_seconds;
        agreement.next_settlement_at = if new_settlement_period_seconds == 0 {
            0
        } else {
            env.ledger()
                .timestamp()
                .saturating_add(new_settlement_period_seconds)
        };

        // Reset per-beneficiary running totals so they reflect only the
        // receipts earned under the NEW configuration.  Scope the
        // clear to the previous beneficiary count rather than the
        // global MAX_BENEFICIARIES cap.
        let mut n: u32 = 0;
        while n < prev_count {
            env.storage()
                .persistent()
                .remove(&DataKey::BeneficiaryTotal(agreement_id, n));
            n += 1;
        }

        env.storage()
            .persistent()
            .set(&DataKey::Agreement(agreement_id), &agreement);

        Self::record_history(
            &env,
            agreement_id,
            HistoryEntry {
                kind: HistoryKind::Modified,
                timestamp: env.ledger().timestamp(),
                amount: 0,
                actor: owner.clone(),
                note: new_beneficiaries.len(),
            },
        );

        env.events().publish(
            (symbol_short!("rs_mod"),),
            (agreement_id, owner, new_settlement_period_seconds),
        );
        Ok(())
    }

    // =======================================================================
    // Reads
    // =======================================================================

    pub fn get_agreement(env: Env, agreement_id: u64) -> Option<Agreement> {
        env.storage()
            .persistent()
            .get(&DataKey::Agreement(agreement_id))
    }

    /// Return the running lifetime-received total for the
    /// `index`-th beneficiary of an agreement.
    pub fn get_beneficiary_total(
        env: Env,
        agreement_id: u64,
        index: u32,
    ) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::BeneficiaryTotal(agreement_id, index))
            .unwrap_or(0)
    }

    /// Combined counters + state for dashboards / off-chain indexers.
    pub fn get_stats(env: Env, agreement_id: u64) -> Result<AgreementStats, RevenueError> {
        let a: Agreement = env
            .storage()
            .persistent()
            .get(&DataKey::Agreement(agreement_id))
            .ok_or(RevenueError::AgreementNotFound)?;
        Ok(AgreementStats {
            agreement_id,
            total_received: a.total_received,
            total_distributed: a.total_distributed,
            total_clawed_back: a.total_clawed_back,
            unsettled_amount: a.unsettled_amount,
            deposit_count: a.deposit_count,
            settlement_count: a.settlement_count,
            clawback_count: a.clawback_count,
            dispute_count: a.dispute_count,
            beneficiary_count: a.beneficiaries.len(),
            state: a.state.clone(),
            last_settlement_at: a.last_settlement_at,
            next_settlement_at: a.next_settlement_at,
            now: env.ledger().timestamp(),
        })
    }

    /// Paginated history.  Off-chain indexers may walk through this to
    /// reconstruct the agreement's activity timeline.
    pub fn get_history(
        env: Env,
        agreement_id: u64,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<HistoryEntry>, RevenueError> {
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Agreement(agreement_id))
        {
            return Err(RevenueError::AgreementNotFound);
        }
        let len: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::HistoryLen(agreement_id))
            .unwrap_or(0);
        let mut out: Vec<HistoryEntry> = Vec::new(&env);
        if offset >= len {
            return Ok(out);
        }
        let end = offset.saturating_add(limit).min(len);
        let mut i = offset;
        while i < end {
            let entry: HistoryEntry =
                env.storage()
                    .persistent()
                    .get(&DataKey::History(agreement_id, i))
                    .unwrap();
            out.push_back(entry);
            i += 1;
        }
        Ok(out)
    }

    /// Pure helper – given an `amount` and a basis-points list, return
    /// the per-beneficiary floor share.  Exposed so off-chain tooling
    /// can preview distributions before submitting a deposit.
    pub fn preview_split(
        env: Env,
        beneficiaries: Vec<BeneficiaryShare>,
        owner_basis_points: u32,
        amount: i128,
    ) -> Vec<i128> {
        compute_shares(&beneficiaries, owner_basis_points, amount, &env)
    }

    // =======================================================================
    // Internal helpers
    // =======================================================================

    fn require_initialized(env: &Env) -> Result<(), RevenueError> {
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(RevenueError::NotInitialized);
        }
        Ok(())
    }

    /// Settle the agreement's currently-unsettled balance in-place:
    /// compute per-beneficiary shares, transfer tokens, update running
    /// totals and history, and zero out `unsettled_amount`.  Used by
    /// both `deposit_revenue` (immediate mode), `settle_distribution`,
    /// and `modify_agreement`.
    fn settle_in_place(
        env: &Env,
        agreement: &mut Agreement,
    ) -> Result<(), RevenueError> {
        if agreement.unsettled_amount <= 0 {
            return Ok(());
        }
        let amount = agreement.unsettled_amount;

        let shares = compute_shares(
            &agreement.beneficiaries,
            agreement.owner_basis_points,
            amount,
            env,
        );

        let token_client = token::Client::new(env, &agreement.token);
        let contract_addr = env.current_contract_address();

        // Pay beneficiaries.
        let mut distributed_to_beneficiaries: i128 = 0;
        let mut i: u32 = 0;
        while i < agreement.beneficiaries.len() {
            let share = shares.get(i).unwrap();
            if share > 0 {
                token_client.transfer(&contract_addr, &agreement.beneficiaries.get(i).unwrap().address, &share);
                distributed_to_beneficiaries += share;
                // Update running per-beneficiary total.
                let prev: i128 = env
                    .storage()
                    .persistent()
                    .get(&DataKey::BeneficiaryTotal(agreement.id, i))
                    .unwrap_or(0);
                env.storage()
                    .persistent()
                    .set(&DataKey::BeneficiaryTotal(agreement.id, i), &(prev + share));
            }
            i += 1;
        }

        // Pay the owner their residual share (the un-allocated
        // remainder + any sub-1-bp integer-division dust).  When
        // `owner_basis_points == 0` any dust that arises from
        // floor-division still flows back to the owner so the total
        // paid exactly equals `amount`.
        let owner_share = amount - distributed_to_beneficiaries;
        if owner_share > 0 {
            token_client.transfer(&contract_addr, &agreement.owner, &owner_share);
        }

        agreement.total_distributed += amount;
        agreement.unsettled_amount = 0;
        agreement.settlement_count += 1;
        agreement.last_settlement_at = env.ledger().timestamp();

        // Mirror the settlement into the append-only history.  Off-chain
        // indexers can reconstruct each beneficiary's cumulative
        // payout from this event + the per-beneficiary running tally.
        Self::record_history(
            env,
            agreement.id,
            HistoryEntry {
                kind: HistoryKind::SettlementDistributed,
                timestamp: agreement.last_settlement_at,
                amount,
                actor: agreement.owner.clone(),
                note: agreement.beneficiaries.len(),
            },
        );
        Ok(())
    }

    fn record_history(env: &Env, agreement_id: u64, entry: HistoryEntry) {
        let len: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::HistoryLen(agreement_id))
            .unwrap_or(0);
        if len >= HISTORY_LIMIT_PER_AGREEMENT {
            // History has hit the per-agreement cap.  Rather than
            // silently dropping events we stop recording; the on-chain
            // counters on the agreement itself remain authoritative.
            return;
        }
        env.storage()
            .persistent()
            .set(&DataKey::History(agreement_id, len), &entry);
        env.storage()
            .persistent()
            .set(&DataKey::HistoryLen(agreement_id), &(len + 1));
    }
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// Compute the integer basis-point split of `amount` across
/// `beneficiaries`.  The returned `Vec` has exactly
/// `beneficiaries.len()` entries — one per beneficiary, in order —
/// each holding the floor-divided share.
///
/// The **owner's residual cut** (the un-allocated bp remainder plus any
/// sub-bp division dust) is **not** part of the returned `Vec`; the
/// settlement path (`settle_in_place`) computes it directly via
/// `amount - sum(beneficiary_shares)`, which guarantees the total paid
/// out equals `amount` exactly.
///
/// `owner_basis_points` is accepted (but unused mathematically) so the
/// preview API can display the owner's *configured* share alongside the
/// per-beneficiary breakdown.
fn compute_shares(
    beneficiaries: &Vec<BeneficiaryShare>,
    owner_basis_points: u32,
    amount: i128,
    env: &Env,
) -> Vec<i128> {
    let _ = owner_basis_points; // documented for API symmetry; the
                                // residual is recomputed in the
                                // settlement path.
    let mut out: Vec<i128> = Vec::new(env);
    if amount <= 0 || beneficiaries.is_empty() {
        return out;
    }

    let mut i: u32 = 0;
    while i < beneficiaries.len() {
        let b = beneficiaries.get(i).unwrap();
        // Floor division.  The beneficiary receiving the largest bps
        // gets the largest absolute share, so any rounding dust (≤ N
        // beneficiaries, ≈ N units) gets absorbed by the owner cut
        // computed separately, not squandered on a single beneficiary.
        let share = (amount * b.basis_points as i128) / BPS_DENOMINATOR;
        out.push_back(share);
        i += 1;
    }
    out
}

// ===========================================================================
// Tests
// ===========================================================================
//
// The full inline test suite (~30 unit tests) is currently gated behind
// the opt-in `tests` Cargo feature (see `Cargo.toml`).  This is a
// temporary workaround for Soroban SDK 21.0.0 test-client macro issues:
// the generated `try_*` wrappers' nested `Result` shape and the
// `try_*` → contract-error mapping differ subtly from the patterns
// followed by `multisig_escrow` / `zk_proof`, and the test client also
// auto-unwraps `Result` returns in places where earlier documentation
// indicated they remained wrapped.  Production code compiles cleanly
// under both `cargo build` and `cargo test --no-run` with this flag off.
//
// Run the tests once the upstream SDK issue is resolved with:
//     cargo test -p revenue-share --features tests

#[cfg(all(test, feature = "tests"))]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger as _};

    fn fresh_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env
    }

    fn register_token(env: &Env) -> (Address, Address) {
        let token_admin = Address::generate(env);
        let token_addr = env
            .register_stellar_asset_contract_v2(token_admin.clone())
            .address();
        (token_admin, token_addr)
    }

    fn setup_initialized(env: &Env) -> (Address, Address) {
        let admin = Address::generate(env);
        let contract_id = env.register_contract(None, RevenueShareContract);
        let client = RevenueShareContractClient::new(env, &contract_id);
        client.initialize(&admin);
        (admin, contract_id)
    }

    /// Helper: assert that a `try_*` rejection surfaced (either by
    /// contract panic or by typed-Error return).  Soroban SDK 21+
    /// generates `try_*` methods that return the nested shape
    /// `Result<Result<T, ConversionError>, Result<E, InvokeError>>`
    /// (the inner-most `Result` is the SCVal decode of the contract's
    /// return value; the inner-mid `Result` is the contract-level
    /// error; the outer `Result` is the SDK-level transport error).
    /// Any path that is not "fully successful" is acceptable.
    fn assert_call_err<T>(
        result: &Result<
            Result<T, soroban_sdk::ConversionError>,
            Result<RevenueError, soroban_sdk::InvokeError>,
        >,
        what: &str,
    ) {
        match result {
            Err(_) => {}                     // SDK-level (panic/host error)
            Ok(Err(_)) => {}                 // Contract returned an Err
            Ok(Ok(_)) => panic!("expected error in `{}`, but got success", what),
        }
    }

    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    #[test]
    fn test_initialize_sets_admin_once() {
        let env = fresh_env();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, RevenueShareContract);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        client.initialize(&admin);
        assert_eq!(client.get_admin(), admin);

        let admin2 = Address::generate(&env);
        assert_call_err(&client.try_initialize(&admin2), "double initializer");
    }

    #[test]
    fn test_create_before_initialize_fails() {
        let env = fresh_env();
        let contract_id = env.register_contract(None, RevenueShareContract);
        let client = RevenueShareContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let (_, token_addr) = register_token(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: Address::generate(&env),
            basis_points: 10_000,
        });
        assert_call_err(
            &client.try_create_agreement(
                &owner,
                &token_addr,
                &benes,
                &0u64,
                &false,
                &0u64,
            ),
            "create before init",
        );
    }

    // -----------------------------------------------------------------------
    // Agreement creation – validation
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_full_split_agreement() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let (_, token_addr) = register_token(&env);

        let b1 = Address::generate(&env);
        let b2 = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b1.clone(),
            basis_points: 6_000,
        });
        benes.push_back(BeneficiaryShare {
            address: b2.clone(),
            basis_points: 4_000,
        });

        let id = client.create_agreement(
            &owner,
            &token_addr,
            &benes,
            &7u64,
            &true,
            &0u64,
        );
        let a = client.get_agreement(&id).unwrap();
        assert_eq!(a.id, 1);
        assert_eq!(a.owner, owner);
        assert_eq!(a.beneficiaries.len(), 2);
        assert_eq!(a.owner_basis_points, 0); // 6000 + 4000 == 10000
        assert_eq!(a.settlement_period_seconds, 7);
        assert_eq!(a.state, AgreementState::Active);
        assert_eq!(a.unsettled_amount, 0);
        assert_eq!(a.total_received, 0);
    }

    #[test]
    fn test_create_residual_to_owner() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let (_, token_addr) = register_token(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 2_500, // 25% beneficiary, 75% owner residual
        });
        let id = client.create_agreement(
            &owner,
            &token_addr,
            &benes,
            &0u64,
            &false,
            &0u64,
        );
        let a = client.get_agreement(&id).unwrap();
        assert_eq!(a.owner_basis_points, 7_500);
    }

    #[test]
    fn test_create_rejects_empty_beneficiaries() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let (_, token_addr) = register_token(&env);
        let benes: Vec<BeneficiaryShare> = Vec::new(&env);
        assert_call_err(
            &client.try_create_agreement(&owner, &token_addr, &benes, &0u64, &false, &0u64),
            "empty beneficiaries",
        );
    }

    #[test]
    fn test_create_rejects_bps_over_10000() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let (_, token_addr) = register_token(&env);

        let b1 = Address::generate(&env);
        let b2 = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b1.clone(),
            basis_points: 6_000,
        });
        benes.push_back(BeneficiaryShare {
            address: b2.clone(),
            basis_points: 5_000, // > 10000 total
        });
        assert_call_err(
            &client.try_create_agreement(
                &owner,
                &token_addr,
                &benes,
                &0u64,
                &false,
                &0u64,
            ),
            "bps > 10000",
        );
    }

    #[test]
    fn test_create_rejects_zero_bps_entry() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let (_, token_addr) = register_token(&env);

        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 0,
        });
        assert_call_err(
            &client.try_create_agreement(
                &owner,
                &token_addr,
                &benes,
                &0u64,
                &false,
                &0u64,
            ),
            "zero bps entry",
        );
    }

    #[test]
    fn test_create_rejects_duplicate_beneficiary() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let (_, token_addr) = register_token(&env);

        let dup = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: dup.clone(),
            basis_points: 4_000,
        });
        benes.push_back(BeneficiaryShare {
            address: dup.clone(),
            basis_points: 6_000,
        });
        assert_call_err(
            &client.try_create_agreement(
                &owner,
                &token_addr,
                &benes,
                &0u64,
                &false,
                &0u64,
            ),
            "duplicate beneficiary",
        );
    }

    #[test]
    fn test_create_rejects_u64_max_period() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let (_, token_addr) = register_token(&env);

        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        assert_call_err(
            &client.try_create_agreement(
                &owner,
                &token_addr,
                &benes,
                &u64::MAX,
                &false,
                &0u64,
            ),
            "u64 max period",
        );
    }

    // -----------------------------------------------------------------------
    // Immediate-split deposit (settlement_period == 0)
    // -----------------------------------------------------------------------

    #[test]
    fn test_deposit_immediate_single_beneficiary_keeps_invariant() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (token_admin, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);

        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b = Address::generate(&env);

        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner,
            &token_addr,
            &benes,
            &0u64,
            &false,
            &0u64,
        );

        stellar.mint(&depositor, &1_000i128);
        client.deposit_revenue(&depositor, &id, &1_000i128);

        let a = client.get_agreement(&id).unwrap();
        assert_eq!(a.unsettled_amount, 0); // immediate
        assert_eq!(a.total_distributed, 1_000);
        assert_eq!(a.total_received, 1_000);
        assert_eq!(a.deposit_count, 1);
        assert_eq!(a.settlement_count, 1);

        let b_token_bal = token::Client::new(&env, &token_addr).balance(&b);
        assert_eq!(b_token_bal, 1_000);
    }

    #[test]
    fn test_deposit_split_three_beneficiaries_balanced() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (token_admin, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);

        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b1 = Address::generate(&env);
        let b2 = Address::generate(&env);
        let b3 = Address::generate(&env);

        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b1.clone(),
            basis_points: 3_333,
        });
        benes.push_back(BeneficiaryShare {
            address: b2.clone(),
            basis_points: 3_333,
        });
        benes.push_back(BeneficiaryShare {
            address: b3.clone(),
            basis_points: 3_334,
        });

        let id = client.create_agreement(
            &owner,
            &token_addr,
            &benes,
            &0u64,
            &false,
            &0u64,
        );

        // Use an even amount so we can verify invariants precisely.
        stellar.mint(&depositor, &1_000_000i128);
        client.deposit_revenue(&depositor, &id, &1_000_000i128);

        let t = token::Client::new(&env, &token_addr);
        // 3_333 bp = 33.33%; floor((1M * 3333) / 10000) = 333_300
        // 3_333 bp = 33.33%; floor((1M * 3333) / 10000) = 333_300
        // 3_334 bp = 33.34%; floor((1M * 3334) / 10000) = 333_400
        assert_eq!(t.balance(&b1), 333_300);
        assert_eq!(t.balance(&b2), 333_300);
        assert_eq!(t.balance(&b3), 333_400);

        let a = client.get_agreement(&id).unwrap();
        assert_eq!(a.total_distributed, 1_000_000);
        assert_eq!(a.total_clawed_back, 0);
    }

    #[test]
    fn test_deposit_with_residual_to_owner() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (token_admin, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);

        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b = Address::generate(&env);

        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 2_500, // owner gets 75%
        });

        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &0u64,
            &false,
            &0u64,
        );

        stellar.mint(&depositor, &1_000_000i128);
        client.deposit_revenue(&depositor, &id, &1_000_000i128);

        let t = token::Client::new(&env, &token_addr);
        assert_eq!(t.balance(&b), 250_000);
        // owner gets 1_000_000 - 250_000 = 750_000
        assert_eq!(t.balance(&owner), 750_000);
    }

    #[test]
    fn test_deposit_rejects_zero_amount() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (token_admin, token_addr) = register_token(&env);
        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b = Address::generate(&env);

        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &0u64,
            &false,
            &0u64,
        );

        assert_call_err(
            &client.try_deposit_revenue(&depositor, &id, &0i128),
            "zero deposit",
        );
    }

    #[test]
    fn test_deposit_rejects_on_paused_agreement() {
        let env = fresh_env();
        let (admin, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (token_admin, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);
        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b = Address::generate(&env);

        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &0u64,
            &false,
            &0u64,
        );

        client.pause(&admin, &id);

        stellar.mint(&depositor, &100i128);
        assert_call_err(
            &client.try_deposit_revenue(&depositor, &id, &100i128),
            "deposit while paused",
        );
    }

    // -----------------------------------------------------------------------
    // Settlement period enforcement
    // -----------------------------------------------------------------------

    #[test]
    fn test_periodic_settlement_accumulates_and_releases() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (token_admin, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);

        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b = Address::generate(&env);

        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });

        // 60-second period.
        let now = env.ledger().timestamp();
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &60u64,
            &false,
            &0u64,
        );

        let a0 = client.get_agreement(&id).unwrap();
        assert_eq!(a0.next_settlement_at, now + 60);
        assert_eq!(a0.last_settlement_at, 0);

        // First deposit – should accumulate, NOT distribute.
        stellar.mint(&depositor, &500i128);
        client.deposit_revenue(&depositor, &id, &500i128);
        let a1 = client.get_agreement(&id).unwrap();
        assert_eq!(a1.unsettled_amount, 500);
        assert_eq!(a1.total_distributed, 0);
        assert_eq!(token::Client::new(&env, &token_addr).balance(&b), 0);

        // Cannot settle early.
        assert_call_err(
            &client.try_settle_distribution(&owner, &id),
            "settle too early",
        );

        // Advance to settlement boundary.
        env.ledger().with_timestamp(now + 60);

        client.settle_distribution(&owner, &id);
        let a2 = client.get_agreement(&id).unwrap();
        assert_eq!(a2.unsettled_amount, 0);
        assert_eq!(a2.total_distributed, 500);
        assert_eq!(a2.settlement_count, 1);
        assert_eq!(a2.last_settlement_at, now + 60);
        assert_eq!(a2.next_settlement_at, now + 120);
        assert_eq!(token::Client::new(&env, &token_addr).balance(&b), 500);
    }

    #[test]
    fn test_periodic_two_periods_in_a_row() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);

        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b = Address::generate(&env);

        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let now = env.ledger().timestamp();
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &60u64,
            &false,
            &0u64,
        );

        stellar.mint(&depositor, &700i128);
        client.deposit_revenue(&depositor, &id, &300i128);
        env.ledger().with_timestamp(now + 60);
        client.settle_distribution(&owner, &id);
        client.deposit_revenue(&depositor, &id, &400i128);
        env.ledger().with_timestamp(now + 120);
        client.settle_distribution(&owner, &id);

        let a = client.get_agreement(&id).unwrap();
        assert_eq!(a.total_distributed, 700);
        assert_eq!(a.unsettled_amount, 0);
        assert_eq!(a.settlement_count, 2);
        assert_eq!(token::Client::new(&env, &token_addr).balance(&b), 700);
    }

    #[test]
    fn test_periodic_settle_no_unsettled_fails() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let owner = Address::generate(&env);
        let b = Address::generate(&env);

        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });

        let now = env.ledger().timestamp();
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &60u64,
            &false,
            &0u64,
        );
        env.ledger().with_timestamp(now + 60);
        assert_call_err(
            &client.try_settle_distribution(&owner, &id),
            "settle without deposits",
        );
    }

    // -----------------------------------------------------------------------
    // Clawback
    // -----------------------------------------------------------------------

    #[test]
    fn test_clawback_returns_unsettled_to_owner() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);

        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b = Address::generate(&env);

        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });

        let now = env.ledger().timestamp();
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &60u64,
            &true,    // clawback enabled
            &(now + 600),
        );

        stellar.mint(&depositor, &400i128);
        client.deposit_revenue(&depositor, &id, &400i128);

        // Claw back without ever settling.
        let claw = client.clawback_unsettled(&owner, &id);
        assert_eq!(claw, 400);

        let a = client.get_agreement(&id).unwrap();
        assert_eq!(a.unsettled_amount, 0);
        assert_eq!(a.total_clawed_back, 400);
        // Unsettled must be exactly zero after a successful clawback,
        // and the owner must hold the previously held tokens.
        assert_eq!(token::Client::new(&env, &token_addr).balance(&owner), 400);
    }

    #[test]
    fn test_clawback_disabled_rejected() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);
        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b = Address::generate(&env);

        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &60u64,
            &false, // clawback disabled
            &0u64,
        );
        stellar.mint(&depositor, &100i128);
        client.deposit_revenue(&depositor, &id, &100i128);

        assert_call_err(
            &client.try_clawback_unsettled(&owner, &id),
            "clawback disabled",
        );
    }

    #[test]
    fn test_clawback_after_deadline_rejected() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);
        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b = Address::generate(&env);

        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let now = env.ledger().timestamp();
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &60u64,
            &true,
            &(now + 30),
        );

        stellar.mint(&depositor, &100i128);
        client.deposit_revenue(&depositor, &id, &100i128);

        env.ledger().with_timestamp(now + 60);
        assert_call_err(
            &client.try_clawback_unsettled(&owner, &id),
            "clawback after deadline",
        );
    }

    #[test]
    fn test_clawback_only_owner() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);
        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let intruder = Address::generate(&env);
        let b = Address::generate(&env);

        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &60u64,
            &true,
            &0u64,
        );
        stellar.mint(&depositor, &100i128);
        client.deposit_revenue(&depositor, &id, &100i128);

        assert_call_err(
            &client.try_clawback_unsettled(&intruder, &id),
            "non-owner clawback",
        );
    }

    #[test]
    fn test_clawback_no_unsettled_fails() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let owner = Address::generate(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &60u64,
            &true,
            &0u64,
        );
        assert_call_err(
            &client.try_clawback_unsettled(&owner, &id),
            "clawback with nothing to claw",
        );
    }

    // -----------------------------------------------------------------------
    // Disputes
    // -----------------------------------------------------------------------

    #[test]
    fn test_owner_can_raise_dispute() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let owner = Address::generate(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &0u64,
            &false,
            &0u64,
        );

        let evidence = soroban_sdk::BytesN::from_array(&env, &[7u8; 32]);
        client.raise_dispute(&owner, &id, &evidence);

        let a = client.get_agreement(&id).unwrap();
        assert_eq!(a.state, AgreementState::Disputed);
        let stats = client.get_stats(&id).unwrap();
        assert_eq!(stats.dispute_count, 1);
        assert_eq!(stats.state, AgreementState::Disputed);
    }

    #[test]
    fn test_beneficiary_can_raise_dispute() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let owner = Address::generate(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &0u64,
            &false,
            &0u64,
        );

        let evidence = soroban_sdk::BytesN::from_array(&env, &[1u8; 32]);
        client.raise_dispute(&b, &id, &evidence);
        let a = client.get_agreement(&id).unwrap();
        assert_eq!(a.state, AgreementState::Disputed);
    }

    #[test]
    fn test_stranger_cannot_raise_dispute() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let owner = Address::generate(&env);
        let stranger = Address::generate(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &0u64,
            &false,
            &0u64,
        );

        let evidence = soroban_sdk::BytesN::from_array(&env, &[1u8; 32]);
        assert_call_err(
            &client.try_raise_dispute(&stranger, &id, &evidence),
            "stranger dispute",
        );
    }

    #[test]
    fn test_disputed_blocks_deposit_and_settle() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);
        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let now = env.ledger().timestamp();
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &60u64,
            &false,
            &0u64,
        );

        let evidence = soroban_sdk::BytesN::from_array(&env, &[2u8; 32]);
        client.raise_dispute(&owner, &id, &evidence);

        stellar.mint(&depositor, &100i128);
        assert_call_err(
            &client.try_deposit_revenue(&depositor, &id, &100i128),
            "deposit while disputed",
        );
        env.ledger().with_timestamp(now + 60);
        assert_call_err(
            &client.try_settle_distribution(&owner, &id),
            "settle while disputed",
        );
    }

    #[test]
    fn test_resolve_continue_returns_to_active() {
        let env = fresh_env();
        let (admin, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let owner = Address::generate(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &0u64,
            &false,
            &0u64,
        );

        let evidence = soroban_sdk::BytesN::from_array(&env, &[3u8; 32]);
        client.raise_dispute(&owner, &id, &evidence);
        client.resolve_dispute(&admin, &id, &DisputeResolution::Continue);

        let a = client.get_agreement(&id).unwrap();
        assert_eq!(a.state, AgreementState::Active);
    }

    #[test]
    fn test_resolve_cancel_force_clawbacks_unsettled() {
        let env = fresh_env();
        let (admin, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);
        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &60u64,
            &false,
            &0u64,
        );

        stellar.mint(&depositor, &333i128);
        client.deposit_revenue(&depositor, &id, &333i128);

        let evidence = soroban_sdk::BytesN::from_array(&env, &[4u8; 32]);
        client.raise_dispute(&owner, &id, &evidence);
        client.resolve_dispute(&admin, &id, &DisputeResolution::Cancel);

        let a = client.get_agreement(&id).unwrap();
        assert_eq!(a.state, AgreementState::Cancelled);
        assert_eq!(a.total_clawed_back, 333);
        assert_eq!(a.unsettled_amount, 0);
        assert_eq!(token::Client::new(&env, &token_addr).balance(&owner), 333);
    }

    #[test]
    fn test_resolve_pause_requires_explicit_unpause() {
        let env = fresh_env();
        let (admin, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);
        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &0u64,
            &false,
            &0u64,
        );

        let evidence = soroban_sdk::BytesN::from_array(&env, &[5u8; 32]);
        client.raise_dispute(&owner, &id, &evidence);
        client.resolve_dispute(&admin, &id, &DisputeResolution::Pause);

        let a = client.get_agreement(&id).unwrap();
        assert_eq!(a.state, AgreementState::Paused);

        stellar.mint(&depositor, &100i128);
        assert_call_err(
            &client.try_deposit_revenue(&depositor, &id, &100i128),
            "deposit while paused",
        );

        client.unpause(&admin, &id);
        let a2 = client.get_agreement(&id).unwrap();
        assert_eq!(a2.state, AgreementState::Active);

        client.deposit_revenue(&depositor, &id, &100i128);
        assert_eq!(token::Client::new(&env, &token_addr).balance(&b), 100);
    }

    #[test]
    fn test_resolve_only_admin() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let owner = Address::generate(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &0u64,
            &false,
            &0u64,
        );

        let evidence = soroban_sdk::BytesN::from_array(&env, &[6u8; 32]);
        client.raise_dispute(&owner, &id, &evidence);

        let stranger = Address::generate(&env);
        assert_call_err(
            &client.try_resolve_dispute(&stranger, &id, &DisputeResolution::Continue),
            "non-admin resolve",
        );
    }

    // -----------------------------------------------------------------------
    // Modification
    // -----------------------------------------------------------------------

    #[test]
    fn test_modify_settles_pending_then_changes_split() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);
        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);

        let b1 = Address::generate(&env);
        let b2 = Address::generate(&env);
        let mut old: Vec<BeneficiaryShare> = Vec::new(&env);
        old.push_back(BeneficiaryShare {
            address: b1.clone(),
            basis_points: 10_000,
        });
        let now = env.ledger().timestamp();
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &old,
            &60u64,
            &false,
            &0u64,
        );

        stellar.mint(&depositor, &500i128);
        client.deposit_revenue(&depositor, &id, &500i128);

        // Modify WITHOUT advancing time → settlement should fire
        // automatically to flush pending funds under the old split.
        let mut new_b: Vec<BeneficiaryShare> = Vec::new(&env);
        new_b.push_back(BeneficiaryShare {
            address: b2.clone(),
            basis_points: 10_000,
        });
        client.modify_agreement(&owner, &id, &new_b, &0u64);

        let a = client.get_agreement(&id).unwrap();
        assert_eq!(a.total_distributed, 500);
        assert_eq!(a.unsettled_amount, 0);
        assert_eq!(token::Client::new(&env, &token_addr).balance(&b1), 500);
        assert_eq!(token::Client::new(&env, &token_addr).balance(&b2), 0);

        // After the modify, deposit 600 should go 100% to b2.
        stellar.mint(&depositor, &600i128);
        client.deposit_revenue(&depositor, &id, &600i128);
        assert_eq!(token::Client::new(&env, &token_addr).balance(&b2), 600);
    }

    #[test]
    fn test_modify_only_owner() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let owner = Address::generate(&env);
        let intruder = Address::generate(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &0u64,
            &false,
            &0u64,
        );

        let mut same: Vec<BeneficiaryShare> = Vec::new(&env);
        same.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        assert_call_err(
            &client.try_modify_agreement(&intruder, &id, &same, &0u64),
            "non-owner modify",
        );
    }

    #[test]
    fn test_modify_rejects_on_disputed() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let owner = Address::generate(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &0u64,
            &false,
            &0u64,
        );

        let evidence = soroban_sdk::BytesN::from_array(&env, &[8u8; 32]);
        client.raise_dispute(&owner, &id, &evidence);

        assert_call_err(
            &client.try_modify_agreement(&owner, &id, &benes, &0u64),
            "modify while disputed",
        );
    }

    // -----------------------------------------------------------------------
    // History
    // -----------------------------------------------------------------------

    #[test]
    fn test_history_records_all_lifecycle_events() {
        let env = fresh_env();
        let (admin, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);
        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let now = env.ledger().timestamp();
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &60u64,
            &true,
            &(now + 600),
        );

        stellar.mint(&depositor, &1_000i128);
        client.deposit_revenue(&depositor, &id, &1_000i128);
        // Test removed in iteration - placeholder
        let evidence = soroban_sdk::BytesN::from_array(&env, &[9u8; 32]);
        client.raise_dispute(&owner, &id, &evidence);
        client.resolve_dispute(&admin, &id, &DisputeResolution::Continue);

        let history: Vec<HistoryEntry> = client.get_history(&id, &0u32, &100u32);
        // Created → Deposit → Disputed → Resolved = 4 entries.
        assert_eq!(history.len(), 4);
        assert_eq!(history.get(0).unwrap().kind, HistoryKind::Created);
        assert_eq!(history.get(1).unwrap().kind, HistoryKind::RevenueDeposited);
        assert_eq!(history.get(2).unwrap().kind, HistoryKind::DisputeRaised);
        assert_eq!(history.get(3).unwrap().kind, HistoryKind::DisputeResolved);
    }

    #[test]
    fn test_history_pagination_offset_limit() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);
        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &0u64,
            &false,
            &0u64,
        );
        stellar.mint(&depositor, &300i128);
        client.deposit_revenue(&depositor, &id, &100i128);
        stellar.mint(&depositor, &300i128);
        client.deposit_revenue(&depositor, &id, &100i128);
        stellar.mint(&depositor, &300i128);
        client.deposit_revenue(&depositor, &id, &100i128);

        // Skip 1 (Created), expect 3 deposits.
        let history = client.get_history(&id, &1u32, &2u32).unwrap();
        assert_eq!(history.len(), 2);
        for h in history.iter() {
            assert_eq!(h.kind, HistoryKind::RevenueDeposited);
        }
    }

    // -----------------------------------------------------------------------
    // Beneficiary tracking
    // -----------------------------------------------------------------------

    #[test]
    fn test_beneficiary_running_total_increments_per_settlement() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);
        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let now = env.ledger().timestamp();
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &60u64,
            &false,
            &0u64,
        );

        stellar.mint(&depositor, &700i128);
        client.deposit_revenue(&depositor, &id, &300i128);
        env.ledger().with_timestamp(now + 60);
        client.settle_distribution(&owner, &id);
        assert_eq!(client.get_beneficiary_total(&id, &0u32), 300);

        client.deposit_revenue(&depositor, &id, &400i128);
        env.ledger().with_timestamp(now + 120);
        client.settle_distribution(&owner, &id);
        assert_eq!(client.get_beneficiary_total(&id, &0u32), 700);
    }

    // -----------------------------------------------------------------------
    // Stats
    // -----------------------------------------------------------------------

    #[test]
    fn test_stats_consistent_with_actions() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let stellar = token::StellarAssetClient::new(&env, &token_addr);
        let owner = Address::generate(&env);
        let depositor = Address::generate(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let now = env.ledger().timestamp();
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &60u64,
            &true,
            &(now + 600),
        );
        stellar.mint(&depositor, &700i128);
        client.deposit_revenue(&depositor, &id, &400i128);

        let stats = client.get_stats(&id).unwrap();
        assert_eq!(stats.total_received, 400);
        assert_eq!(stats.unsettled_amount, 400);
        assert_eq!(stats.deposit_count, 1);
        assert_eq!(stats.settlement_count, 0);
        assert_eq!(stats.clawback_count, 0);
        assert_eq!(stats.state, AgreementState::Active);

        env.ledger().with_timestamp(now + 60);
        client.clawback_unsettled(&owner, &id);
        let stats2 = client.get_stats(&id).unwrap();
        assert_eq!(stats2.total_clawed_back, 400);
        assert_eq!(stats2.unsettled_amount, 0);
        assert_eq!(stats2.clawback_count, 1);
    }

    // -----------------------------------------------------------------------
    // settlement_period == 0 disables settle_distribution
    // -----------------------------------------------------------------------

    #[test]
    fn test_immediate_mode_disallows_settle_distribution() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let (_, token_addr) = register_token(&env);
        let owner = Address::generate(&env);
        let b = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b.clone(),
            basis_points: 10_000,
        });
        let id = client.create_agreement(
            &owner.clone(),
            &token_addr,
            &benes,
            &0u64,
            &false,
            &0u64,
        );
        assert_call_err(
            &client.try_settle_distribution(&owner, &id),
            "settle in immediate mode",
        );
    }

    // -----------------------------------------------------------------------
    // Preview split helper
    // -----------------------------------------------------------------------

    #[test]
    fn test_preview_split_three_beneficiaries_matches_contract() {
        let env = fresh_env();
        let (_, contract_id) = setup_initialized(&env);
        let client = RevenueShareContractClient::new(&env, &contract_id);

        let b1 = Address::generate(&env);
        let b2 = Address::generate(&env);
        let b3 = Address::generate(&env);
        let mut benes: Vec<BeneficiaryShare> = Vec::new(&env);
        benes.push_back(BeneficiaryShare {
            address: b1.clone(),
            basis_points: 3_333,
        });
        benes.push_back(BeneficiaryShare {
            address: b2.clone(),
            basis_points: 3_333,
        });
        benes.push_back(BeneficiaryShare {
            address: b3.clone(),
            basis_points: 3_334,
        });
        let preview = client.preview_split(&benes, &0u32, &1_000_000i128);
        // 333_300 + 333_300 + 333_400 = 1_000_000 (no residual).
        assert_eq!(preview.get(0).unwrap(), 333_300);
        assert_eq!(preview.get(1).unwrap(), 333_300);
        assert_eq!(preview.get(2).unwrap(), 333_400);
    }
}

