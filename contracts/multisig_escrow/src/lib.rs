#![no_std]

//! # Multi-Signature Escrow Contract
//!
//! A Soroban smart contract implementing a held-amount escrow whose
//! release or refund requires **M-of-N** independent signatures from an
//! authorised list of signers, with an optional arbitrator that can
//! settle disputes.
//!
//! ## Use case
//!
//! A **depositor** (e.g. a buyer) and a **beneficiary** (e.g. a seller)
//! lock funds in the contract.  The release of those funds to the
//! beneficiary – or the refund back to the depositor – requires agreement
//! from at least `threshold` of the `signers`.  If a dispute is raised
//! (by depositor or beneficiary) the contract freezes votes and asks the
//! appointed `arbitrator` to choose between Release, Refund, or a 50/50
//! Split.
//!
//! ## State machine
//!
//! ```text
//!   Pending --fund--> Active
//!   Pending --cancel(depositor)-->     Cancelled
//!   Active  --M approve-release-->     Released
//!   Active  --M approve-refund-->      Refunded
//!   Active  --raise_dispute-->         Disputed
//!   Active  --expire(now > expires)--> Expired --> (auto) Refunded
//!   Disputed--arbitrator-->            Released / Refunded / Split
//! ```
//!
//! ## Acceptance criteria (issue #266)
//!
//! | Criterion                  | Where it is enforced                                  |
//! |---------------------------|-------------------------------------------------------|
//! | Funds held securely        | `token::Client` transfers only into the contract; balance only decreases on terminal actions. |
//! | Signatures validated       | `sign_approve` requires the signer to be in `escrow.signers` and rejects re-votes. |
//! | Release requires agreement | `release_funds` only executes once `ReleaseCount >= threshold` AND state=Active.    |
//! | Disputes handled           | `raise_dispute` (depositor or beneficiary); arbitrator must `resolve_dispute`.        |
//! | Refunds processed          | `refund_funds` (multisig), `resolve_dispute(Refund)`, `cancel_escrow`, `expire_escrow`. |
//! | All tests pass             | 25+ inline unit tests in `mod tests`.                |

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, BytesN, Env,
    Vec,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum EscrowError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAuthorized = 3,
    EscrowNotFound = 4,
    InvalidState = 5,
    InvalidConfig = 6,
    ZeroAmount = 7,
    EmptySigners = 8,
    InvalidThreshold = 9,
    DuplicateSigner = 10,
    AlreadyVoted = 11,
    NotASigner = 12,
    NotDepositor = 13,
    NoArbitrator = 14,
    NotArbitrator = 15,
    NotExpired = 16,
    ThresholdNotMet = 17,
    AlreadyFunded = 18,
    NotParty = 19,
    AlreadyDisputed = 20,
    InvalidDisputeCaster = 21,
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EscrowState {
    Pending = 0,
    Active = 1,
    Released = 2,
    Refunded = 3,
    Disputed = 4,
    Cancelled = 5,
    Expired = 6,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VoteAction {
    ApproveRelease = 0,
    ApproveRefund = 1,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisputeResolution {
    Release = 0,
    Refund = 1,
    Split = 2, // 50/50 floor split
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dispute {
    pub raised_by: Address,
    pub raised_at: u64,
    pub evidence_hash: Option<BytesN<32>>,
    pub resolution: Option<DisputeResolution>,
    pub resolved_at: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Escrow {
    pub id: u64,
    pub depositor: Address,
    pub beneficiary: Address,
    pub token: Address,
    pub amount: i128,
    pub signers: Vec<Address>,
    pub threshold: u32,
    pub arbitrator: Option<Address>,
    pub created_at: u64,
    pub expires_at: u64,
    pub funded: bool,
    pub state: EscrowState,
    pub dispute: Option<Dispute>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscrowStats {
    pub release_votes: u32,
    pub refund_votes: u32,
    pub voted: u32,
    pub threshold: u32,
    pub funded: bool,
    pub state: EscrowState,
    pub created_at: u64,
    pub expires_at: u64,
    pub now: u64,
}

#[contracttype]
pub enum DataKey {
    Admin,
    EscrowCounter,
    Escrow(u64),
    Vote(u64, Address),
    ReleaseCount(u64),
    RefundCount(u64),
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct MultisigEscrowContract;

#[contractimpl]
impl MultisigEscrowContract {
    // =======================================================================
    // Initialization
    // =======================================================================

    /// One-shot initialisation.  Stores the administrator.
    pub fn initialize(env: Env, admin: Address) -> Result<(), EscrowError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(EscrowError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::EscrowCounter, &0u64);
        env.events().publish((symbol_short!("ms_init"),), admin);
        Ok(())
    }

    // =======================================================================
    // Escrow creation & funding
    // =======================================================================

    /// Create a new escrow agreement.
    ///
    /// * `depositor`      – the wallet whose funds are held.
    /// * `beneficiary`    – the wallet that receives funds on release.
    /// * `token`          – the Soroban token contract to deposit.
    /// * `amount`         – the held amount (in the token's smallest unit).
    /// * `signers`        – the N addresses authorised to vote.
    /// * `threshold`      – the M value; must be `1 ≤ threshold ≤ signers.len()`.
    /// * `arbitrator`     – optional dispute-resolver.
    /// * `expires_at`     – Unix timestamp after which the escrow is force-refundable.
    pub fn create_escrow(
        env: Env,
        depositor: Address,
        beneficiary: Address,
        token: Address,
        amount: i128,
        signers: Vec<Address>,
        threshold: u32,
        arbitrator: Option<Address>,
        expires_at: u64,
    ) -> Result<u64, EscrowError> {
        depositor.require_auth();
        Self::require_initialized(&env)?;

        if amount <= 0 {
            return Err(EscrowError::ZeroAmount);
        }
        if signers.is_empty() {
            return Err(EscrowError::EmptySigners);
        }
        if threshold == 0 || threshold > signers.len() {
            return Err(EscrowError::InvalidThreshold);
        }
        if has_duplicates(&signers) {
            return Err(EscrowError::DuplicateSigner);
        }
        // Depositor and beneficiary must not also be signers – they are
        // *parties*, not neutral witnesses.
        for s in signers.iter() {
            if s == depositor || s == beneficiary {
                return Err(EscrowError::InvalidConfig);
            }
        }
        if expires_at <= env.ledger().timestamp() {
            return Err(EscrowError::InvalidConfig);
        }

        let counter: u64 = env
            .storage()
            .instance()
            .get(&DataKey::EscrowCounter)
            .unwrap_or(0);
        let id = counter + 1;

        let escrow = Escrow {
            id,
            depositor: depositor.clone(),
            beneficiary: beneficiary.clone(),
            token: token.clone(),
            amount,
            signers: signers.clone(),
            threshold,
            arbitrator: arbitrator.clone(),
            created_at: env.ledger().timestamp(),
            expires_at,
            funded: false,
            state: EscrowState::Pending,
            dispute: None,
        };
        env.storage().instance().set(&DataKey::Escrow(id), &escrow);
        env.storage()
            .instance()
            .set(&DataKey::EscrowCounter, &id);

        env.events().publish(
            (symbol_short!("ms_crt"),),
            (id, depositor, beneficiary, amount, threshold, expires_at),
        );
        Ok(id)
    }

    /// Move `amount` of `token` from the depositor into the contract.
    /// Transitions the escrow from `Pending` to `Active`.
    pub fn fund_escrow(env: Env, depositor: Address, escrow_id: u64) -> Result<(), EscrowError> {
        depositor.require_auth();

        let mut escrow: Escrow = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .ok_or(EscrowError::EscrowNotFound)?;
        if depositor != escrow.depositor {
            return Err(EscrowError::NotDepositor);
        }
        if escrow.funded {
            return Err(EscrowError::AlreadyFunded);
        }
        if escrow.state != EscrowState::Pending {
            return Err(EscrowError::InvalidState);
        }

        token::Client::new(&env, &escrow.token).transfer(
            &depositor,
            &env.current_contract_address(),
            &escrow.amount,
        );

        escrow.funded = true;
        escrow.state = EscrowState::Active;
        env.storage().instance().set(&DataKey::Escrow(escrow_id), &escrow);

        env.events().publish(
            (symbol_short!("ms_fund"),),
            (escrow_id, depositor, escrow.amount),
        );
        Ok(())
    }

    // =======================================================================
    // Signature voting
    // =======================================================================

    /// Cast a signed vote for either release or refund on `escrow_id`.
    ///
    /// Any caller that is in `escrow.signers` may submit a vote.  A
    /// signer cannot change their vote once submitted.
    pub fn sign_approve(
        env: Env,
        signer: Address,
        escrow_id: u64,
        action: VoteAction,
    ) -> Result<(), EscrowError> {
        signer.require_auth();

        let escrow: Escrow = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .ok_or(EscrowError::EscrowNotFound)?;

        // Vote can only be cast while the escrow is Active and not
        // disputed.  Once the case is in arbitration the matter is up
        // to the arbitrator.
        if escrow.state != EscrowState::Active {
            return Err(EscrowError::InvalidState);
        }
        if !contains_address(&escrow.signers, &signer) {
            return Err(EscrowError::NotASigner);
        }
        if env
            .storage()
            .instance()
            .has(&DataKey::Vote(escrow_id, signer.clone()))
        {
            return Err(EscrowError::AlreadyVoted);
        }

        env.storage()
            .instance()
            .set(&DataKey::Vote(escrow_id, signer.clone()), &action);

        match action {
            VoteAction::ApproveRelease => {
                let cur: u32 = env
                    .storage()
                    .instance()
                    .get(&DataKey::ReleaseCount(escrow_id))
                    .unwrap_or(0);
                env.storage()
                    .instance()
                    .set(&DataKey::ReleaseCount(escrow_id), &(cur + 1));
            }
            VoteAction::ApproveRefund => {
                let cur: u32 = env
                    .storage()
                    .instance()
                    .get(&DataKey::RefundCount(escrow_id))
                    .unwrap_or(0);
                env.storage()
                    .instance()
                    .set(&DataKey::RefundCount(escrow_id), &(cur + 1));
            }
        }

        env.events().publish(
            (symbol_short!("ms_sign"),),
            (escrow_id, signer, action),
        );
        Ok(())
    }

    // =======================================================================
    // Release & refund (multisig-driven)
    // =======================================================================

    /// Transfer the held funds to the **beneficiary**.  Anyone may call
    /// this once `M-of-N` have voted `ApproveRelease`; the action is
    /// gated entirely by vote-counting, not by who triggers it.
    pub fn release_funds(env: Env, _caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
        let mut escrow: Escrow = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .ok_or(EscrowError::EscrowNotFound)?;

        if escrow.state != EscrowState::Active {
            return Err(EscrowError::InvalidState);
        }
        if !escrow.funded {
            return Err(EscrowError::InvalidState);
        }

        let release_votes: u32 = env
            .storage()
            .instance()
            .get(&DataKey::ReleaseCount(escrow_id))
            .unwrap_or(0);
        if release_votes < escrow.threshold {
            return Err(EscrowError::ThresholdNotMet);
        }

        token::Client::new(&env, &escrow.token).transfer(
            &env.current_contract_address(),
            &escrow.beneficiary,
            &escrow.amount,
        );

        escrow.state = EscrowState::Released;
        env.storage().instance().set(&DataKey::Escrow(escrow_id), &escrow);

        env.events().publish(
            (symbol_short!("ms_rel"),),
            (escrow_id, escrow.beneficiary, escrow.amount),
        );
        Ok(())
    }

    /// Transfer the held funds back to the **depositor**.  Triggered
    /// when `M-of-N` have voted `ApproveRefund`.
    pub fn refund_funds(env: Env, _caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
        let mut escrow: Escrow = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .ok_or(EscrowError::EscrowNotFound)?;

        if escrow.state != EscrowState::Active {
            return Err(EscrowError::InvalidState);
        }
        if !escrow.funded {
            return Err(EscrowError::InvalidState);
        }

        let refund_votes: u32 = env
            .storage()
            .instance()
            .get(&DataKey::RefundCount(escrow_id))
            .unwrap_or(0);
        if refund_votes < escrow.threshold {
            return Err(EscrowError::ThresholdNotMet);
        }

        token::Client::new(&env, &escrow.token).transfer(
            &env.current_contract_address(),
            &escrow.depositor,
            &escrow.amount,
        );

        escrow.state = EscrowState::Refunded;
        env.storage().instance().set(&DataKey::Escrow(escrow_id), &escrow);

        env.events().publish(
            (symbol_short!("ms_rfd"),),
            (escrow_id, escrow.depositor, escrow.amount),
        );
        Ok(())
    }

    // =======================================================================
    // Dispute & arbitration
    // =======================================================================

    /// Raise a dispute.  Either the **depositor** or the **beneficiary**
    /// may do this while the escrow is `Active`.  An arbitrator must
    /// already have been designated at creation time.
    ///
    /// Disputing freezes new votes; the case becomes the arbitrator's.
    pub fn raise_dispute(
        env: Env,
        caller: Address,
        escrow_id: u64,
        evidence_hash: Option<BytesN<32>>,
    ) -> Result<(), EscrowError> {
        caller.require_auth();

        let mut escrow: Escrow = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .ok_or(EscrowError::EscrowNotFound)?;

        if escrow.state != EscrowState::Active {
            return Err(EscrowError::InvalidState);
        }
        if escrow.dispute.is_some() {
            return Err(EscrowError::AlreadyDisputed);
        }
        if escrow.arbitrator.is_none() {
            return Err(EscrowError::NoArbitrator);
        }
        if caller != escrow.depositor && caller != escrow.beneficiary {
            return Err(EscrowError::InvalidDisputeCaster);
        }

        escrow.dispute = Some(Dispute {
            raised_by: caller.clone(),
            raised_at: env.ledger().timestamp(),
            evidence_hash: evidence_hash.clone(),
            resolution: None,
            resolved_at: None,
        });
        escrow.state = EscrowState::Disputed;
        env.storage().instance().set(&DataKey::Escrow(escrow_id), &escrow);

        env.events().publish(
            (symbol_short!("ms_disp"),),
            (escrow_id, caller, evidence_hash),
        );
        Ok(())
    }

    /// Settle a disputed escrow.  Only the configured arbitrator may
    /// invoke this; the resolution chooses Release, Refund, or Split.
    /// After this call the escrow transitions to a terminal state.
    pub fn resolve_dispute(
        env: Env,
        arbitrator: Address,
        escrow_id: u64,
        resolution: DisputeResolution,
    ) -> Result<(), EscrowError> {
        arbitrator.require_auth();

        let mut escrow: Escrow = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .ok_or(EscrowError::EscrowNotFound)?;

        if escrow.state != EscrowState::Disputed {
            return Err(EscrowError::InvalidState);
        }
        if escrow.arbitrator.is_none() || escrow.arbitrator.clone().unwrap() != arbitrator {
            return Err(EscrowError::NotArbitrator);
        }
        if !escrow.funded {
            return Err(EscrowError::InvalidState);
        }

        let amount = escrow.amount;
        let token_client = token::Client::new(&env, &escrow.token);
        let contract_address = env.current_contract_address();

        match resolution {
            DisputeResolution::Release => {
                token_client.transfer(&contract_address, &escrow.beneficiary, &amount);
                escrow.state = EscrowState::Released;
            }
            DisputeResolution::Refund => {
                token_client.transfer(&contract_address, &escrow.depositor, &amount);
                escrow.state = EscrowState::Refunded;
            }
            DisputeResolution::Split => {
                // Floor division: beneficiary gets amount/2, depositor
                // gets the remainder so the two halves always sum to the
                // original total (avoids dust leakage).
                let half = amount / 2;
                let remainder = amount - half;
                token_client.transfer(&contract_address, &escrow.beneficiary, &half);
                token_client.transfer(&contract_address, &escrow.depositor, &remainder);
                escrow.state = EscrowState::Released; // both got funds; mark terminal
            }
        }

        // Stamp the dispute with the resolution.
        let mut dispute = escrow.dispute.clone().unwrap();
        dispute.resolution = Some(resolution.clone());
        dispute.resolved_at = Some(env.ledger().timestamp());
        escrow.dispute = Some(dispute);
        env.storage().instance().set(&DataKey::Escrow(escrow_id), &escrow);

        env.events().publish(
            (symbol_short!("ms_dres"),),
            (escrow_id, arbitrator, resolution),
        );
        Ok(())
    }

    // =======================================================================
    // Cancellation & expiry
    // =======================================================================

    /// Voluntary cancellation by the depositor.
    ///
    /// Available while:
    ///   * the escrow is `Pending` (no funds yet moved), OR
    ///   * the escrow is `Active` and **no votes have been cast**.
    ///
    /// Funds are transferred back to the depositor if they were already
    /// held by the contract.
    pub fn cancel_escrow(env: Env, depositor: Address, escrow_id: u64) -> Result<(), EscrowError> {
        depositor.require_auth();

        let mut escrow: Escrow = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .ok_or(EscrowError::EscrowNotFound)?;
        if depositor != escrow.depositor {
            return Err(EscrowError::NotDepositor);
        }

        let total_votes: u32 = env
            .storage()
            .instance()
            .get::<DataKey, u32>(&DataKey::ReleaseCount(escrow_id))
            .unwrap_or(0)
            + env
                .storage()
                .instance()
                .get::<DataKey, u32>(&DataKey::RefundCount(escrow_id))
                .unwrap_or(0);

        let allowed = match escrow.state {
            EscrowState::Pending => true,
            EscrowState::Active => total_votes == 0,
            _ => false,
        };
        if !allowed {
            return Err(EscrowError::InvalidState);
        }

        if escrow.funded {
            token::Client::new(&env, &escrow.token).transfer(
                &env.current_contract_address(),
                &escrow.depositor,
                &escrow.amount,
            );
        }
        escrow.state = EscrowState::Cancelled;
        escrow.funded = false;
        env.storage().instance().set(&DataKey::Escrow(escrow_id), &escrow);

        env.events().publish((symbol_short!("ms_cncl"),), (escrow_id, depositor));
        Ok(())
    }

    /// Anyone may invoke this once `now > expires_at` to force-refund an
    /// escrow that is `Pending` or `Active`.
    ///
    /// Disputed escrows are not affected – the arbitrator still owns
    /// them.  Refunded/Released/Cancelled/Expired escrows are no-ops (err).
    pub fn expire_escrow(env: Env, escrow_id: u64) -> Result<(), EscrowError> {
        let mut escrow: Escrow = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .ok_or(EscrowError::EscrowNotFound)?;

        match escrow.state {
            EscrowState::Pending | EscrowState::Active => {}
            _ => return Err(EscrowError::InvalidState),
        }
        if env.ledger().timestamp() <= escrow.expires_at {
            return Err(EscrowError::NotExpired);
        }

        if escrow.funded {
            token::Client::new(&env, &escrow.token).transfer(
                &env.current_contract_address(),
                &escrow.depositor,
                &escrow.amount,
            );
        }

        escrow.state = EscrowState::Expired;
        escrow.funded = false;
        env.storage().instance().set(&DataKey::Escrow(escrow_id), &escrow);

        env.events().publish((symbol_short!("ms_exp"),), (escrow_id, escrow.depositor));
        Ok(())
    }

    // =======================================================================
    // Reads
    // =======================================================================

    pub fn get_escrow(env: Env, escrow_id: u64) -> Option<Escrow> {
        env.storage().instance().get(&DataKey::Escrow(escrow_id))
    }

    pub fn get_vote(env: Env, escrow_id: u64, signer: Address) -> Option<VoteAction> {
        env.storage().instance().get(&DataKey::Vote(escrow_id, signer))
    }

    /// Combined counters for dashboards / off-chain indexing.
    pub fn get_stats(env: Env, escrow_id: u64) -> Result<EscrowStats, EscrowError> {
        let escrow: Escrow = env
            .storage()
            .instance()
            .get(&DataKey::Escrow(escrow_id))
            .ok_or(EscrowError::EscrowNotFound)?;
        let release: u32 = env
            .storage()
            .instance()
            .get(&DataKey::ReleaseCount(escrow_id))
            .unwrap_or(0);
        let refund: u32 = env
            .storage()
            .instance()
            .get(&DataKey::RefundCount(escrow_id))
            .unwrap_or(0);
        Ok(EscrowStats {
            release_votes: release,
            refund_votes: refund,
            voted: release + refund,
            threshold: escrow.threshold,
            funded: escrow.funded,
            state: escrow.state.clone(),
            created_at: escrow.created_at,
            expires_at: escrow.expires_at,
            now: env.ledger().timestamp(),
        })
    }

    // =======================================================================
    // Internal
    // =======================================================================

    fn require_initialized(env: &Env) -> Result<(), EscrowError> {
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(EscrowError::NotInitialized);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

fn contains_address(list: &Vec<Address>, who: &Address) -> bool {
    for a in list.iter() {
        if a == *who {
            return true;
        }
    }
    false
}

fn has_duplicates(list: &Vec<Address>) -> bool {
    let mut i: u32 = 0;
    while i < list.len() {
        let a = list.get(i).unwrap();
        let mut j: u32 = i + 1;
        while j < list.len() {
            let b = list.get(j).unwrap();
            if a == b {
                return true;
            }
            j += 1;
        }
        i += 1;
    }
    false
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Test fixtures
    // -----------------------------------------------------------------------

    fn fresh_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env
    }

    fn setup() -> (Env, Address) {
        let env = fresh_env();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, MultisigEscrowContract);
        let client = MultisigEscrowContractClient::new(&env, &contract_id);
        client.initialize(&admin);
        (env, admin)
    }

    /// Build a 2-of-3 escrow fixture with the given depositor /
    /// beneficiary / signers and the token contract pre-funded with
    /// `amount` for the depositor.
    fn make_basic(env: &Env, amount: i128) -> (MultisigEscrowContractClient, u64, Address, Address, Address, Address, Address) {
        let token_admin = Address::generate(env);
        let token_addr = env
            .register_stellar_asset_contract_v2(token_admin.clone())
            .address();
        let stellar = token::StellarAssetClient::new(env, &token_addr);

        let depositor = Address::generate(env);
        let beneficiary = Address::generate(env);
        let signer1 = Address::generate(env);
        let signer2 = Address::generate(env);
        let signer3 = Address::generate(env);
        stellar.mint(&depositor, &amount);

        let contract_id = env.register_contract(None, MultisigEscrowContract);
        let client = MultisigEscrowContractClient::new(env, &contract_id);
        client.initialize(&Address::generate(env));

        let mut signers: Vec<Address> = Vec::new(env);
        signers.push_back(signer1.clone());
        signers.push_back(signer2.clone());
        signers.push_back(signer3.clone());

        let exp = env.ledger().timestamp() + 86_400;
        let id = client.create_escrow(
            &depositor,
            &beneficiary,
            &token_addr,
            &amount,
            &signers,
            &2u32,
            &None,
            &exp,
        );
        (client, id, depositor, beneficiary, signer1, signer2, token_addr)
    }

    fn assert_call_err<T, E>(result: &Result<Result<T, E>, soroban_sdk::Error>, what: &str) {
        match result {
            Err(_) => {}
            Ok(Err(_)) => {}
            Ok(Ok(_)) => panic!("expected error in `{}`, but got success", what),
        }
    }

    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    #[test]
