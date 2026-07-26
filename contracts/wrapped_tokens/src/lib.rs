#![no_std]

mod events;
mod storage;
mod types;

#[cfg(test)]
mod test;

use soroban_sdk::{contract, contractimpl, panic_with_error, Address, Bytes, BytesN, Env, String, Vec};

use crate::events::{
    emit_approval, emit_fee_collected, emit_initialized, emit_operator_added,
    emit_operator_removed, emit_paused, emit_tokens_burned, emit_tokens_minted,
    emit_transfer, emit_unpaused, emit_unwrap_completed, emit_unwrap_initiated,
    emit_wrap_confirmed, emit_wrap_requested,
};
use crate::storage::{
    get_allowance, get_balance, get_config, get_custody,
    get_operator_list, get_unwrap_request as storage_get_unwrap_request,
    get_wrap_request as storage_get_wrap_request, has_confirmed,
    increment_confirmation_count, increment_unwrap_nonce, increment_wrap_nonce, is_operator,
    is_tx_used, mark_tx_used, set_allowance, set_balance, set_config, set_confirmed, set_custody,
    set_operator, set_operator_list, set_unwrap_request, set_wrap_request,
};
use crate::types::{
    ChainId, CustodyInfo, OperationStatus, UnwrapRequest, WrapRequest, WrappedTokenConfig,
    WrappedTokenError,
};

/// Maximum number of bridge operators allowed.
const MAX_OPERATORS: u32 = 20;

#[contract]
pub struct WrappedTokensContract;

#[contractimpl]
impl WrappedTokensContract {
    // ─────────────────────────────────────────────────────────────────────────
    // INITIALIZATION
    // ─────────────────────────────────────────────────────────────────────────

    /// Initialize the wrapped token contract. Must be called exactly once.
    /// The `config.admin` address must authorize this call.
    pub fn initialize(env: Env, config: WrappedTokenConfig) {
        if get_config(&env).is_some() {
            panic_with_error!(&env, WrappedTokenError::AlreadyInitialized);
        }
        if config.fee_bps > 10_000 {
            panic_with_error!(&env, WrappedTokenError::InvalidFee);
        }
        if config.required_confirmations == 0 {
            panic_with_error!(&env, WrappedTokenError::InvalidAmount);
        }
        config.admin.require_auth();

        set_config(&env, &config);
        set_custody(
            &env,
            &CustodyInfo {
                total_supply: 0,
                total_fees_collected: 0,
                total_wraps: 0,
                total_unwraps: 0,
                last_operation_at: 0,
            },
        );
        emit_initialized(&env, &config.admin);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // OPERATOR MANAGEMENT
    // ─────────────────────────────────────────────────────────────────────────

    /// Add a new bridge operator. Only the admin can call this.
    pub fn add_operator(env: Env, operator: Address) {
        let config = Self::require_config(&env);
        config.admin.require_auth();

        if is_operator(&env, &operator) {
            panic_with_error!(&env, WrappedTokenError::OperatorAlreadyExists);
        }

        let mut list = get_operator_list(&env);
        if list.len() >= MAX_OPERATORS {
            panic_with_error!(&env, WrappedTokenError::MaxOperatorsReached);
        }

        set_operator(&env, &operator, true);
        list.push_back(operator.clone());
        set_operator_list(&env, &list);

        emit_operator_added(&env, &operator, &config.admin);
    }

    /// Remove a bridge operator. Only the admin can call this.
    pub fn remove_operator(env: Env, operator: Address) {
        let config = Self::require_config(&env);
        config.admin.require_auth();

        if !is_operator(&env, &operator) {
            panic_with_error!(&env, WrappedTokenError::OperatorNotFound);
        }

        set_operator(&env, &operator, false);

        let list = get_operator_list(&env);
        let mut new_list: Vec<Address> = Vec::new(&env);
        for i in 0..list.len() {
            let addr = list.get(i).unwrap();
            if addr != operator {
                new_list.push_back(addr);
            }
        }
        set_operator_list(&env, &new_list);

        emit_operator_removed(&env, &operator, &config.admin);
    }

    /// Return the list of all currently registered operators.
    pub fn get_operators(env: Env) -> Vec<Address> {
        get_operator_list(&env)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // WRAP (MINT) FLOW
    // ─────────────────────────────────────────────────────────────────────────

    /// Called by an operator to submit a wrap request after confirming that
    /// the corresponding assets have been locked on the source chain.
    ///
    /// `operator` is the calling operator's address (must authorize).
    ///
    /// If `required_confirmations == 1` the tokens are minted immediately.
    /// Otherwise the request stays Pending until enough operators confirm.
    ///
    /// Returns the nonce assigned to this wrap request.
    pub fn submit_wrap(
        env: Env,
        operator: Address,
        recipient: Address,
        gross_amount: i128,
        source_chain: ChainId,
        source_tx_id: BytesN<32>,
    ) -> u64 {
        let config = Self::require_config(&env);
        Self::require_not_paused(&env, &config);

        operator.require_auth();
        if !is_operator(&env, &operator) {
            panic_with_error!(&env, WrappedTokenError::Unauthorized);
        }

        if gross_amount <= 0 {
            panic_with_error!(&env, WrappedTokenError::InvalidAmount);
        }

        // Replay prevention — mark source tx as consumed before any state changes
        if is_tx_used(&env, &source_tx_id) {
            panic_with_error!(&env, WrappedTokenError::NonceAlreadyUsed);
        }
        mark_tx_used(&env, &source_tx_id);

        // fee = max(min_fee, gross * fee_bps / 10000)
        let (fee_amount, net_amount) = Self::calc_fee(&env, &config, gross_amount);

        let nonce = increment_wrap_nonce(&env);
        let now = env.ledger().timestamp();

        let req = WrapRequest {
            nonce,
            recipient: recipient.clone(),
            gross_amount,
            fee_amount,
            net_amount,
            source_chain,
            source_tx_id: source_tx_id.clone(),
            status: OperationStatus::Pending,
            created_at: now,
            operator: operator.clone(),
        };
        set_wrap_request(&env, nonce, &req);

        // Record the submitting operator's confirmation
        set_confirmed(&env, nonce, &operator);
        let count = increment_confirmation_count(&env, nonce);

        emit_wrap_requested(
            &env,
            nonce,
            &operator,
            &recipient,
            gross_amount,
            fee_amount,
            source_chain,
            &source_tx_id,
        );
        emit_wrap_confirmed(&env, nonce, &operator, count);

        // Mint immediately if threshold is already met
        if count >= config.required_confirmations {
            Self::mint_internal(&env, nonce);
        }

        nonce
    }

    /// Called by an additional operator to confirm a pending wrap request.
    /// When the confirmation count reaches `required_confirmations` the tokens
    /// are minted to the recipient.
    ///
    /// `operator` is the confirming operator's address (must authorize).
    pub fn confirm_wrap(env: Env, operator: Address, nonce: u64) {
        let config = Self::require_config(&env);
        Self::require_not_paused(&env, &config);

        operator.require_auth();
        if !is_operator(&env, &operator) {
            panic_with_error!(&env, WrappedTokenError::Unauthorized);
        }

        let req = match storage_get_wrap_request(&env, nonce) {
            Some(r) => r,
            None => panic_with_error!(&env, WrappedTokenError::RequestNotFound),
        };

        if req.status != OperationStatus::Pending {
            panic_with_error!(&env, WrappedTokenError::RequestAlreadyProcessed);
        }

        if has_confirmed(&env, nonce, &operator) {
            panic_with_error!(&env, WrappedTokenError::AlreadyConfirmed);
        }

        set_confirmed(&env, nonce, &operator);
        let count = increment_confirmation_count(&env, nonce);

        emit_wrap_confirmed(&env, nonce, &operator, count);

        if count >= config.required_confirmations {
            Self::mint_internal(&env, nonce);
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // UNWRAP (BURN) FLOW
    // ─────────────────────────────────────────────────────────────────────────

    /// Called by a user who wants to redeem their wrapped tokens for the
    /// underlying asset on the source chain.
    ///
    /// Burns `gross_amount` from the caller's balance and records an
    /// `UnwrapRequest`. The bridge operator will observe this on-chain event
    /// and release the underlying asset off-chain, then call `complete_unwrap`.
    ///
    /// Returns the nonce assigned to this unwrap request.
    pub fn initiate_unwrap(
        env: Env,
        user: Address,
        gross_amount: i128,
        target_chain: ChainId,
        target_recipient: Bytes,
    ) -> u64 {
        let config = Self::require_config(&env);
        Self::require_not_paused(&env, &config);

        user.require_auth();

        if gross_amount <= 0 {
            panic_with_error!(&env, WrappedTokenError::InvalidAmount);
        }

        let balance = get_balance(&env, &user);
        if balance < gross_amount {
            panic_with_error!(&env, WrappedTokenError::InsufficientBalance);
        }

        // Burn gross_amount from user immediately
        set_balance(&env, &user, balance - gross_amount);

        // Fee calculation
        let (fee_amount, net_amount) = Self::calc_fee(&env, &config, gross_amount);

        let nonce = increment_unwrap_nonce(&env);
        let now = env.ledger().timestamp();

        let req = UnwrapRequest {
            nonce,
            user: user.clone(),
            gross_amount,
            fee_amount,
            net_amount,
            target_chain,
            target_recipient: target_recipient.clone(),
            status: OperationStatus::Pending,
            created_at: now,
        };
        set_unwrap_request(&env, nonce, &req);

        // Update custody: supply decreases, fees accumulate, unwrap count increments
        let mut custody = get_custody(&env);
        custody.total_supply = custody
            .total_supply
            .checked_sub(gross_amount)
            .unwrap_or(0);
        custody.total_fees_collected = custody
            .total_fees_collected
            .checked_add(fee_amount)
            .expect("custody fee overflow");
        custody.total_unwraps = custody
            .total_unwraps
            .checked_add(1)
            .expect("unwrap count overflow");
        custody.last_operation_at = now;
        set_custody(&env, &custody);

        emit_tokens_burned(&env, nonce, &user, gross_amount);
        emit_unwrap_initiated(
            &env,
            nonce,
            &user,
            gross_amount,
            fee_amount,
            target_chain,
            &target_recipient,
        );

        nonce
    }

    /// Called by an operator to mark an unwrap request as completed once the
    /// underlying asset has been released on the target chain off-chain.
    ///
    /// `operator` is the completing operator's address (must authorize).
    pub fn complete_unwrap(env: Env, operator: Address, nonce: u64) {
        // Require config (ensures contract is initialized)
        Self::require_config(&env);

        operator.require_auth();
        if !is_operator(&env, &operator) {
            panic_with_error!(&env, WrappedTokenError::Unauthorized);
        }

        let mut req = match storage_get_unwrap_request(&env, nonce) {
            Some(r) => r,
            None => panic_with_error!(&env, WrappedTokenError::RequestNotFound),
        };

        if req.status != OperationStatus::Pending {
            panic_with_error!(&env, WrappedTokenError::RequestAlreadyProcessed);
        }

        req.status = OperationStatus::Completed;
        set_unwrap_request(&env, nonce, &req);

        emit_unwrap_completed(&env, nonce, &operator);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TOKEN OPERATIONS (SEP-41 compatible)
    // ─────────────────────────────────────────────────────────────────────────

    /// Transfer `amount` of wrapped tokens from `from` to `to`.
    /// `from` must authorize this call.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        let config = Self::require_config(&env);
        Self::require_not_paused(&env, &config);

        from.require_auth();

        if amount <= 0 {
            panic_with_error!(&env, WrappedTokenError::InvalidAmount);
        }

        let from_balance = get_balance(&env, &from);
        if from_balance < amount {
            panic_with_error!(&env, WrappedTokenError::InsufficientBalance);
        }

        set_balance(&env, &from, from_balance - amount);
        let to_balance = get_balance(&env, &to);
        set_balance(&env, &to, to_balance + amount);

        emit_transfer(&env, &from, &to, amount);
    }

    /// Approve `spender` to transfer up to `amount` tokens on behalf of `owner`.
    /// `owner` must authorize this call.
    pub fn approve(env: Env, owner: Address, spender: Address, amount: i128) {
        let config = Self::require_config(&env);
        Self::require_not_paused(&env, &config);

        owner.require_auth();

        if amount < 0 {
            panic_with_error!(&env, WrappedTokenError::InvalidAmount);
        }

        set_allowance(&env, &owner, &spender, amount);
        emit_approval(&env, &owner, &spender, amount);
    }

    /// Transfer `amount` tokens from `from` to `to` using the allowance
    /// granted to `spender`. `spender` must authorize this call.
    pub fn transfer_from(
        env: Env,
        spender: Address,
        from: Address,
        to: Address,
        amount: i128,
    ) {
        let config = Self::require_config(&env);
        Self::require_not_paused(&env, &config);

        spender.require_auth();

        if amount <= 0 {
            panic_with_error!(&env, WrappedTokenError::InvalidAmount);
        }

        let allowance = get_allowance(&env, &from, &spender);
        if allowance < amount {
            panic_with_error!(&env, WrappedTokenError::InsufficientBalance);
        }

        let from_balance = get_balance(&env, &from);
        if from_balance < amount {
            panic_with_error!(&env, WrappedTokenError::InsufficientBalance);
        }

        set_allowance(&env, &from, &spender, allowance - amount);
        set_balance(&env, &from, from_balance - amount);
        let to_balance = get_balance(&env, &to);
        set_balance(&env, &to, to_balance + amount);

        emit_transfer(&env, &from, &to, amount);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // QUERY FUNCTIONS
    // ─────────────────────────────────────────────────────────────────────────

    /// Return the wrapped token balance of `account`.
    pub fn balance(env: Env, account: Address) -> i128 {
        get_balance(&env, &account)
    }

    /// Return the allowance that `owner` has granted to `spender`.
    pub fn allowance(env: Env, owner: Address, spender: Address) -> i128 {
        get_allowance(&env, &owner, &spender)
    }

    /// Return the total outstanding wrapped token supply.
    pub fn total_supply(env: Env) -> i128 {
        get_custody(&env).total_supply
    }

    /// Return the token name.
    pub fn name(env: Env) -> String {
        Self::require_config(&env).name
    }

    /// Return the token symbol.
    pub fn symbol(env: Env) -> String {
        Self::require_config(&env).symbol
    }

    /// Return the token decimal places.
    pub fn decimals(env: Env) -> u32 {
        Self::require_config(&env).decimals
    }

    /// Return the admin address.
    pub fn admin(env: Env) -> Address {
        Self::require_config(&env).admin
    }

    /// Return whether the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        Self::require_config(&env).paused
    }

    /// Return the WrapRequest for the given nonce (panics if not found).
    pub fn get_wrap_request(env: Env, nonce: u64) -> WrapRequest {
        match storage_get_wrap_request(&env, nonce) {
            Some(r) => r,
            None => panic_with_error!(&env, WrappedTokenError::RequestNotFound),
        }
    }

    /// Return the UnwrapRequest for the given nonce (panics if not found).
    pub fn get_unwrap_request(env: Env, nonce: u64) -> UnwrapRequest {
        match storage_get_unwrap_request(&env, nonce) {
            Some(r) => r,
            None => panic_with_error!(&env, WrappedTokenError::RequestNotFound),
        }
    }

    /// Return aggregate custody and statistics info.
    pub fn get_custody_info(env: Env) -> CustodyInfo {
        get_custody(&env)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // PAUSE / UNPAUSE
    // ─────────────────────────────────────────────────────────────────────────

    /// Pause the contract. Admin or any registered operator can pause.
    /// `caller` is the address initiating the pause (must authorize).
    pub fn pause(env: Env, caller: Address) {
        let mut config = Self::require_config(&env);
        caller.require_auth();

        if config.admin != caller && !is_operator(&env, &caller) {
            panic_with_error!(&env, WrappedTokenError::Unauthorized);
        }

        if config.paused {
            panic_with_error!(&env, WrappedTokenError::ContractPaused);
        }

        config.paused = true;
        set_config(&env, &config);
        emit_paused(&env, &caller);
    }

    /// Unpause the contract. Only the admin can unpause.
    pub fn unpause(env: Env) {
        let mut config = Self::require_config(&env);
        config.admin.require_auth();

        if !config.paused {
            panic_with_error!(&env, WrappedTokenError::ContractNotPaused);
        }

        config.paused = false;
        let admin = config.admin.clone();
        set_config(&env, &config);
        emit_unpaused(&env, &admin);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ADMIN CONFIGURATION UPDATES
    // ─────────────────────────────────────────────────────────────────────────

    /// Update bridge fee parameters. Only admin.
    pub fn update_fee(env: Env, fee_bps: u32, min_fee: i128) {
        let mut config = Self::require_config(&env);
        config.admin.require_auth();

        if fee_bps > 10_000 {
            panic_with_error!(&env, WrappedTokenError::InvalidFee);
        }
        if min_fee < 0 {
            panic_with_error!(&env, WrappedTokenError::InvalidFee);
        }

        config.fee_bps = fee_bps;
        config.min_fee = min_fee;
        set_config(&env, &config);
    }

    /// Change the fee collector address. Only admin.
    pub fn update_fee_collector(env: Env, fee_collector: Address) {
        let mut config = Self::require_config(&env);
        config.admin.require_auth();

        config.fee_collector = fee_collector;
        set_config(&env, &config);
    }

    /// Change the required number of operator confirmations. Only admin.
    pub fn update_required_confirmations(env: Env, required: u32) {
        let mut config = Self::require_config(&env);
        config.admin.require_auth();

        if required == 0 {
            panic_with_error!(&env, WrappedTokenError::InvalidAmount);
        }

        config.required_confirmations = required;
        set_config(&env, &config);
    }

    /// Fee collector withdraws the accumulated protocol fees.
    ///
    /// Fees were never minted to the circulating supply — they are tracked
    /// separately and minted here to the fee_collector address.
    pub fn collect_fees(env: Env) {
        let config = Self::require_config(&env);
        config.fee_collector.require_auth();

        let mut custody = get_custody(&env);
        let fees = custody.total_fees_collected;
        if fees <= 0 {
            return;
        }

        // Mint fee amount to fee_collector
        let current = get_balance(&env, &config.fee_collector);
        set_balance(&env, &config.fee_collector, current + fees);

        custody.total_fees_collected = 0;
        set_custody(&env, &custody);

        emit_fee_collected(&env, &config.fee_collector, fees);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // PRIVATE HELPERS
    // ─────────────────────────────────────────────────────────────────────────

    /// Execute the actual mint for a wrap request that has reached the
    /// required confirmation threshold. Marks the request Completed, credits
    /// the recipient, and updates custody stats.
    fn mint_internal(env: &Env, nonce: u64) {
        let mut req = match storage_get_wrap_request(env, nonce) {
            Some(r) => r,
            None => panic_with_error!(env, WrappedTokenError::RequestNotFound),
        };

        // Guard against double-mint (should not happen, but be defensive)
        if req.status != OperationStatus::Pending {
            return;
        }

        req.status = OperationStatus::Completed;
        set_wrap_request(env, nonce, &req);

        // Credit net_amount to recipient
        let current = get_balance(env, &req.recipient);
        set_balance(env, &req.recipient, current + req.net_amount);

        // Update custody stats
        let mut custody = get_custody(env);
        custody.total_supply = custody
            .total_supply
            .checked_add(req.net_amount)
            .expect("supply overflow");
        custody.total_fees_collected = custody
            .total_fees_collected
            .checked_add(req.fee_amount)
            .expect("fee overflow");
        custody.total_wraps = custody
            .total_wraps
            .checked_add(1)
            .expect("wrap count overflow");
        custody.last_operation_at = env.ledger().timestamp();
        set_custody(env, &custody);

        emit_tokens_minted(env, nonce, &req.recipient, req.net_amount);
    }

    /// Load config or panic with `NotInitialized`.
    fn require_config(env: &Env) -> WrappedTokenConfig {
        match get_config(env) {
            Some(c) => c,
            None => panic_with_error!(env, WrappedTokenError::NotInitialized),
        }
    }

    /// Panic with `ContractPaused` if the contract is paused.
    fn require_not_paused(env: &Env, config: &WrappedTokenConfig) {
        if config.paused {
            panic_with_error!(env, WrappedTokenError::ContractPaused);
        }
    }

    /// Calculate (fee_amount, net_amount) from gross_amount and config.
    ///
    /// `fee = max(min_fee, gross_amount * fee_bps / 10000)`
    /// `net = gross - fee`
    fn calc_fee(
        env: &Env,
        config: &WrappedTokenConfig,
        gross_amount: i128,
    ) -> (i128, i128) {
        let proportional = gross_amount
            .checked_mul(config.fee_bps as i128)
            .expect("fee mul overflow")
            / 10_000;

        let fee_amount = if proportional > config.min_fee {
            proportional
        } else {
            config.min_fee
        };

        // Sanity: fee cannot equal or exceed the gross amount
        if fee_amount >= gross_amount {
            panic_with_error!(env, WrappedTokenError::InvalidFee);
        }

        let net_amount = gross_amount - fee_amount;
        (fee_amount, net_amount)
    }
}
