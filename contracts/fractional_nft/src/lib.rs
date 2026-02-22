#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, token, Address, Env, IntoVal, Symbol, Vec,
};

const BPS_DENOMINATOR: i128 = 10_000;
const ACC_SCALE: i128 = 1_000_000_000_000;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Vault {
    pub id: u64,
    pub nft_contract: Address,
    pub nft_id: u32,
    pub total_shares: i128,
    pub min_ownership_bps: u32,
    pub rental_token: Option<Address>,
    pub acc_rental_per_share: i128,
    pub active: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Listing {
    pub id: u64,
    pub vault_id: u64,
    pub seller: Address,
    pub payment_token: Address,
    pub shares: i128,
    pub price_per_share: i128,
    pub expiration: Option<u64>,
    pub active: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Buyout {
    pub vault_id: u64,
    pub buyer: Address,
    pub payment_token: Address,
    pub price_total: i128,
    pub escrow_remaining: i128,
    pub end_time: u64,
    pub active: bool,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProposalKind {
    SetRentalToken = 1,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    pub id: u64,
    pub vault_id: u64,
    pub proposer: Address,
    pub kind: ProposalKind,
    pub new_rental_token: Address,
    pub start_time: u64,
    pub end_time: u64,
    pub for_votes: i128,
    pub against_votes: i128,
    pub executed: bool,
}

#[contracttype]
pub enum DataKey {
    Admin,
    VaultCount,
    Vault(u64),

    Balance(u64, Address),
    Allowance(u64, Address, Address),

    ListingCount,
    Listing(u64),

    Buyout(u64),

    ProposalCount,
    Proposal(u64),
    Voted(u64, Address),

    RewardDebt(u64, Address),
    Claimable(u64, Address),
}

#[contract]
pub struct FractionalNftContract;

#[contractimpl]
impl FractionalNftContract {
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::VaultCount, &0u64);
        env.storage().instance().set(&DataKey::ListingCount, &0u64);
        env.storage().instance().set(&DataKey::ProposalCount, &0u64);
    }

    pub fn distribute_shares(
        env: Env,
        vault_id: u64,
        from: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
    ) {
        from.require_auth();
        Self::require_active_vault(&env, vault_id);

        if recipients.len() != amounts.len() {
            panic!("length_mismatch");
        }

        for i in 0..recipients.len() {
            let to = recipients.get(i).unwrap();
            let amt = amounts.get(i).unwrap();
            if amt <= 0 {
                panic!("invalid_amount");
            }
            Self::transfer_shares_internal(&env, vault_id, &from, &to, amt);
        }
    }

    pub fn fractionalize(
        env: Env,
        owner: Address,
        nft_contract: Address,
        nft_id: u32,
        total_shares: i128,
        min_ownership_bps: u32,
        rental_token: Option<Address>,
    ) -> u64 {
        owner.require_auth();

        if total_shares <= 0 {
            panic!("invalid_total_shares");
        }
        if min_ownership_bps > 10_000 {
            panic!("invalid_min_ownership_bps");
        }

        let owner_of_args = (nft_id,).into_val(&env);
        let current_owner: Address = env.invoke_contract(
            &nft_contract,
            &Symbol::new(&env, "owner_of"),
            owner_of_args,
        );
        if current_owner != owner {
            panic!("not_nft_owner");
        }

        let transfer_args = (owner.clone(), env.current_contract_address(), nft_id).into_val(&env);
        env.invoke_contract::<()>(
            &nft_contract,
            &Symbol::new(&env, "transfer"),
            transfer_args,
        );

        let id = Self::next_vault_id(&env);

        let vault = Vault {
            id,
            nft_contract,
            nft_id,
            total_shares,
            min_ownership_bps,
            rental_token,
            acc_rental_per_share: 0,
            active: true,
        };

        env.storage().persistent().set(&DataKey::Vault(id), &vault);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Vault(id), 100_000, 500_000);

        Self::set_balance(&env, id, &owner, total_shares);
        Self::set_reward_debt(&env, id, &owner, 0);

        env.events().publish((symbol_short!("fraction"), id), ());

        id
    }

    pub fn get_vault(env: Env, vault_id: u64) -> Option<Vault> {
        env.storage().persistent().get(&DataKey::Vault(vault_id))
    }

    pub fn balance_of(env: Env, vault_id: u64, owner: Address) -> i128 {
        Self::balance(&env, vault_id, &owner)
    }

    pub fn allowance(env: Env, vault_id: u64, owner: Address, spender: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Allowance(vault_id, owner, spender))
            .unwrap_or(0)
    }

    pub fn approve_shares(env: Env, vault_id: u64, owner: Address, spender: Address, amount: i128) {
        owner.require_auth();
        if amount < 0 {
            panic!("invalid_amount");
        }
        env.storage()
            .persistent()
            .set(&DataKey::Allowance(vault_id, owner, spender), &amount);
    }

    pub fn transfer_shares(env: Env, vault_id: u64, from: Address, to: Address, amount: i128) {
        from.require_auth();
        if amount <= 0 {
            panic!("invalid_amount");
        }
        if from == to {
            panic!("cannot_transfer_to_self");
        }

        Self::before_balance_change(&env, vault_id, &from);
        Self::before_balance_change(&env, vault_id, &to);

        let from_bal = Self::balance(&env, vault_id, &from);
        if from_bal < amount {
            panic!("insufficient_balance");
        }

        let to_bal = Self::balance(&env, vault_id, &to);
        Self::set_balance(&env, vault_id, &from, from_bal - amount);
        Self::set_balance(&env, vault_id, &to, to_bal + amount);

        Self::enforce_min_threshold(&env, vault_id, &from);
        Self::enforce_min_threshold(&env, vault_id, &to);

        Self::after_balance_change(&env, vault_id, &from);
        Self::after_balance_change(&env, vault_id, &to);

        env.events()
            .publish((symbol_short!("xfer"), vault_id, from, to), amount);
    }

    pub fn transfer_shares_from(
        env: Env,
        vault_id: u64,
        spender: Address,
        from: Address,
        to: Address,
        amount: i128,
    ) {
        spender.require_auth();
        if amount <= 0 {
            panic!("invalid_amount");
        }

        let key = DataKey::Allowance(vault_id, from.clone(), spender.clone());
        let allow: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if allow < amount {
            panic!("insufficient_allowance");
        }
        env.storage().persistent().set(&key, &(allow - amount));

        Self::before_balance_change(&env, vault_id, &from);
        Self::before_balance_change(&env, vault_id, &to);

        let from_bal = Self::balance(&env, vault_id, &from);
        if from_bal < amount {
            panic!("insufficient_balance");
        }
        let to_bal = Self::balance(&env, vault_id, &to);

        Self::set_balance(&env, vault_id, &from, from_bal - amount);
        Self::set_balance(&env, vault_id, &to, to_bal + amount);

        Self::enforce_min_threshold(&env, vault_id, &from);
        Self::enforce_min_threshold(&env, vault_id, &to);

        Self::after_balance_change(&env, vault_id, &from);
        Self::after_balance_change(&env, vault_id, &to);
    }

    pub fn create_listing(
        env: Env,
        seller: Address,
        vault_id: u64,
        shares: i128,
        payment_token: Address,
        price_per_share: i128,
        expiration: Option<u64>,
    ) -> u64 {
        seller.require_auth();
        Self::require_active_vault(&env, vault_id);

        if shares <= 0 || price_per_share <= 0 {
            panic!("invalid_params");
        }

        if let Some(t) = expiration {
            if t <= env.ledger().timestamp() {
                panic!("invalid_expiration");
            }
        }

        Self::transfer_shares_internal(&env, vault_id, &seller, &env.current_contract_address(), shares);

        let id = Self::next_listing_id(&env);
        let listing = Listing {
            id,
            vault_id,
            seller,
            payment_token,
            shares,
            price_per_share,
            expiration,
            active: true,
        };
        env.storage().persistent().set(&DataKey::Listing(id), &listing);
        id
    }

    pub fn buy_listing(env: Env, buyer: Address, listing_id: u64) {
        buyer.require_auth();

        let mut listing: Listing = env
            .storage()
            .persistent()
            .get(&DataKey::Listing(listing_id))
            .unwrap_or_else(|| panic!("listing_not_found"));

        if !listing.active {
            panic!("listing_inactive");
        }
        if listing.seller == buyer {
            panic!("cannot_buy_own_listing");
        }
        if let Some(t) = listing.expiration {
            if env.ledger().timestamp() > t {
                panic!("listing_expired");
            }
        }

        let total_price = listing
            .price_per_share
            .checked_mul(listing.shares)
            .unwrap_or_else(|| panic!("overflow"));

        let token_client = token::Client::new(&env, &listing.payment_token);
        token_client.transfer(&buyer, &listing.seller, &total_price);

        Self::transfer_shares_internal(
            &env,
            listing.vault_id,
            &env.current_contract_address(),
            &buyer,
            listing.shares,
        );

        listing.active = false;
        env.storage().persistent().set(&DataKey::Listing(listing_id), &listing);

        env.events()
            .publish((symbol_short!("sold"), listing_id), total_price);
    }

    pub fn cancel_listing(env: Env, seller: Address, listing_id: u64) {
        seller.require_auth();

        let mut listing: Listing = env
            .storage()
            .persistent()
            .get(&DataKey::Listing(listing_id))
            .unwrap_or_else(|| panic!("listing_not_found"));

        if !listing.active {
            panic!("listing_inactive");
        }
        if listing.seller != seller {
            panic!("not_seller");
        }

        Self::transfer_shares_internal(
            &env,
            listing.vault_id,
            &env.current_contract_address(),
            &seller,
            listing.shares,
        );

        listing.active = false;
        env.storage().persistent().set(&DataKey::Listing(listing_id), &listing);
    }

    pub fn start_buyout(
        env: Env,
        buyer: Address,
        vault_id: u64,
        payment_token: Address,
        price_total: i128,
        end_time: u64,
    ) {
        buyer.require_auth();
        let vault = Self::require_active_vault(&env, vault_id);

        if price_total <= 0 {
            panic!("invalid_price");
        }
        if end_time <= env.ledger().timestamp() {
            panic!("invalid_end_time");
        }

        if env.storage().persistent().has(&DataKey::Buyout(vault_id)) {
            let existing: Buyout = env.storage().persistent().get(&DataKey::Buyout(vault_id)).unwrap();
            if existing.active {
                panic!("buyout_active");
            }
        }

        let token_client = token::Client::new(&env, &payment_token);
        token_client.transfer(&buyer, &env.current_contract_address(), &price_total);

        let buyout = Buyout {
            vault_id,
            buyer,
            payment_token,
            price_total,
            escrow_remaining: price_total,
            end_time,
            active: true,
        };
        env.storage().persistent().set(&DataKey::Buyout(vault_id), &buyout);

        env.events()
            .publish((symbol_short!("buyout"), vault_id), price_total);

        let _ = vault;
    }

    pub fn get_buyout(env: Env, vault_id: u64) -> Option<Buyout> {
        env.storage().persistent().get(&DataKey::Buyout(vault_id))
    }

    pub fn tender_shares(env: Env, holder: Address, vault_id: u64, shares: i128) {
        holder.require_auth();
        if shares <= 0 {
            panic!("invalid_amount");
        }

        let vault = Self::require_active_vault(&env, vault_id);

        let mut buyout: Buyout = env
            .storage()
            .persistent()
            .get(&DataKey::Buyout(vault_id))
            .unwrap_or_else(|| panic!("no_buyout"));

        if !buyout.active {
            panic!("buyout_inactive");
        }
        if env.ledger().timestamp() > buyout.end_time {
            panic!("buyout_ended");
        }
        if holder == buyout.buyer {
            panic!("buyer_cannot_tender");
        }

        let payout = buyout
            .price_total
            .checked_mul(shares)
            .unwrap_or_else(|| panic!("overflow"))
            / vault.total_shares;

        if payout > buyout.escrow_remaining {
            panic!("insufficient_buyout_escrow");
        }

        Self::transfer_shares_internal(&env, vault_id, &holder, &buyout.buyer, shares);

        let token_client = token::Client::new(&env, &buyout.payment_token);
        token_client.transfer(&env.current_contract_address(), &holder, &payout);

        buyout.escrow_remaining -= payout;
        env.storage().persistent().set(&DataKey::Buyout(vault_id), &buyout);

        env.events()
            .publish((symbol_short!("tender"), vault_id, holder), payout);
    }

    pub fn reclaim_buyout_escrow(env: Env, buyer: Address, vault_id: u64) {
        buyer.require_auth();
        let mut buyout: Buyout = env
            .storage()
            .persistent()
            .get(&DataKey::Buyout(vault_id))
            .unwrap_or_else(|| panic!("no_buyout"));

        if !buyout.active {
            panic!("buyout_inactive");
        }
        if buyout.buyer != buyer {
            panic!("not_buyer");
        }
        if env.ledger().timestamp() <= buyout.end_time {
            panic!("buyout_not_ended");
        }

        let token_client = token::Client::new(&env, &buyout.payment_token);
        if buyout.escrow_remaining > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &buyer,
                &buyout.escrow_remaining,
            );
            buyout.escrow_remaining = 0;
        }

        buyout.active = false;
        env.storage().persistent().set(&DataKey::Buyout(vault_id), &buyout);
    }

    pub fn recombine(env: Env, owner: Address, vault_id: u64, to: Address) {
        owner.require_auth();
        let mut vault = Self::require_active_vault(&env, vault_id);

        let bal = Self::balance(&env, vault_id, &owner);
        if bal != vault.total_shares {
            panic!("not_full_owner");
        }

        if let Some(buyout) = env.storage().persistent().get::<DataKey, Buyout>(&DataKey::Buyout(vault_id)) {
            if buyout.active {
                panic!("buyout_active");
            }
        }

        Self::before_balance_change(&env, vault_id, &owner);
        Self::set_balance(&env, vault_id, &owner, 0);
        Self::after_balance_change(&env, vault_id, &owner);

        let transfer_args = (env.current_contract_address(), to, vault.nft_id).into_val(&env);
        env.invoke_contract::<()>(
            &vault.nft_contract,
            &Symbol::new(&env, "transfer"),
            transfer_args,
        );

        vault.active = false;
        env.storage().persistent().set(&DataKey::Vault(vault_id), &vault);

        env.events().publish((symbol_short!("merge"), vault_id), ());
    }

    pub fn deposit_rental_income(env: Env, payer: Address, vault_id: u64, amount: i128) {
        payer.require_auth();
        let mut vault = Self::require_active_vault(&env, vault_id);

        if amount <= 0 {
            panic!("invalid_amount");
        }
        let token_addr = vault.rental_token.clone().unwrap_or_else(|| panic!("rental_token_not_set"));

        let token_client = token::Client::new(&env, &token_addr);
        token_client.transfer(&payer, &env.current_contract_address(), &amount);

        vault.acc_rental_per_share += amount
            .checked_mul(ACC_SCALE)
            .unwrap_or_else(|| panic!("overflow"))
            / vault.total_shares;

        env.storage().persistent().set(&DataKey::Vault(vault_id), &vault);
    }

    pub fn claim_rental_profit(env: Env, claimer: Address, vault_id: u64) {
        claimer.require_auth();
        let vault = Self::require_active_vault(&env, vault_id);

        Self::before_balance_change(&env, vault_id, &claimer);

        let amount = env
            .storage()
            .persistent()
            .get(&DataKey::Claimable(vault_id, claimer.clone()))
            .unwrap_or(0);
        if amount <= 0 {
            panic!("no_claimable");
        }
        env.storage()
            .persistent()
            .set(&DataKey::Claimable(vault_id, claimer.clone()), &0i128);

        let token_addr = vault.rental_token.unwrap_or_else(|| panic!("rental_token_not_set"));
        let token_client = token::Client::new(&env, &token_addr);
        token_client.transfer(&env.current_contract_address(), &claimer, &amount);

        Self::after_balance_change(&env, vault_id, &claimer);
    }

    pub fn create_proposal_set_rental_token(
        env: Env,
        proposer: Address,
        vault_id: u64,
        new_rental_token: Address,
        voting_period_secs: u64,
    ) -> u64 {
        proposer.require_auth();
        let vault = Self::require_active_vault(&env, vault_id);

        if voting_period_secs == 0 {
            panic!("invalid_voting_period");
        }

        let proposer_shares = Self::balance(&env, vault_id, &proposer);
        if proposer_shares <= 0 {
            panic!("no_shares");
        }

        let id = Self::next_proposal_id(&env);
        let start_time = env.ledger().timestamp();
        let end_time = start_time + voting_period_secs;

        let proposal = Proposal {
            id,
            vault_id,
            proposer,
            kind: ProposalKind::SetRentalToken,
            new_rental_token,
            start_time,
            end_time,
            for_votes: 0,
            against_votes: 0,
            executed: false,
        };

        env.storage().persistent().set(&DataKey::Proposal(id), &proposal);

        let _ = vault;

        id
    }

    pub fn vote(env: Env, voter: Address, proposal_id: u64, support: bool) {
        voter.require_auth();

        let mut proposal: Proposal = env
            .storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
            .unwrap_or_else(|| panic!("proposal_not_found"));

        let now = env.ledger().timestamp();
        if now < proposal.start_time {
            panic!("voting_not_started");
        }
        if now > proposal.end_time {
            panic!("voting_ended");
        }

        if env
            .storage()
            .persistent()
            .has(&DataKey::Voted(proposal_id, voter.clone()))
        {
            panic!("already_voted");
        }

        let weight = Self::balance(&env, proposal.vault_id, &voter);
        if weight <= 0 {
            panic!("no_shares");
        }

        if support {
            proposal.for_votes += weight;
        } else {
            proposal.against_votes += weight;
        }

        env.storage().persistent().set(&DataKey::Proposal(proposal_id), &proposal);
        env.storage()
            .persistent()
            .set(&DataKey::Voted(proposal_id, voter), &true);
    }

    pub fn execute_proposal(env: Env, proposal_id: u64) {
        let mut proposal: Proposal = env
            .storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
            .unwrap_or_else(|| panic!("proposal_not_found"));

        if proposal.executed {
            panic!("already_executed");
        }

        let now = env.ledger().timestamp();
        if now <= proposal.end_time {
            panic!("voting_not_ended");
        }

        if proposal.for_votes <= proposal.against_votes {
            panic!("proposal_defeated");
        }

        let mut vault: Vault = env
            .storage()
            .persistent()
            .get(&DataKey::Vault(proposal.vault_id))
            .unwrap_or_else(|| panic!("vault_not_found"));

        if !vault.active {
            panic!("vault_inactive");
        }

        match proposal.kind {
            ProposalKind::SetRentalToken => {
                vault.rental_token = Some(proposal.new_rental_token.clone());
                env.storage().persistent().set(&DataKey::Vault(vault.id), &vault);
            }
        }

        proposal.executed = true;
        env.storage().persistent().set(&DataKey::Proposal(proposal_id), &proposal);
    }

    fn next_vault_id(env: &Env) -> u64 {
        let mut id: u64 = env.storage().instance().get(&DataKey::VaultCount).unwrap_or(0);
        id += 1;
        env.storage().instance().set(&DataKey::VaultCount, &id);
        id
    }

    fn next_listing_id(env: &Env) -> u64 {
        let mut id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ListingCount)
            .unwrap_or(0);
        id += 1;
        env.storage().instance().set(&DataKey::ListingCount, &id);
        id
    }

    fn next_proposal_id(env: &Env) -> u64 {
        let mut id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ProposalCount)
            .unwrap_or(0);
        id += 1;
        env.storage().instance().set(&DataKey::ProposalCount, &id);
        id
    }

    fn require_active_vault(env: &Env, vault_id: u64) -> Vault {
        let vault: Vault = env
            .storage()
            .persistent()
            .get(&DataKey::Vault(vault_id))
            .unwrap_or_else(|| panic!("vault_not_found"));
        if !vault.active {
            panic!("vault_inactive");
        }
        vault
    }

    fn balance(env: &Env, vault_id: u64, owner: &Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(vault_id, owner.clone()))
            .unwrap_or(0)
    }

    fn set_balance(env: &Env, vault_id: u64, owner: &Address, amount: i128) {
        env.storage()
            .persistent()
            .set(&DataKey::Balance(vault_id, owner.clone()), &amount);
    }

    fn set_reward_debt(env: &Env, vault_id: u64, owner: &Address, amount: i128) {
        env.storage()
            .persistent()
            .set(&DataKey::RewardDebt(vault_id, owner.clone()), &amount);
    }

    fn before_balance_change(env: &Env, vault_id: u64, owner: &Address) {
        if let Some(vault) = env.storage().persistent().get::<DataKey, Vault>(&DataKey::Vault(vault_id)) {
            if !vault.active {
                return;
            }
            let bal = Self::balance(env, vault_id, owner);
            let debt = env
                .storage()
                .persistent()
                .get(&DataKey::RewardDebt(vault_id, owner.clone()))
                .unwrap_or(0);

            let accumulated = bal
                .checked_mul(vault.acc_rental_per_share)
                .unwrap_or_else(|| panic!("overflow"))
                / ACC_SCALE;

            let pending = accumulated - debt;
            if pending > 0 {
                let key = DataKey::Claimable(vault_id, owner.clone());
                let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
                env.storage().persistent().set(&key, &(current + pending));
            }
        }
    }

    fn after_balance_change(env: &Env, vault_id: u64, owner: &Address) {
        if let Some(vault) = env.storage().persistent().get::<DataKey, Vault>(&DataKey::Vault(vault_id)) {
            if !vault.active {
                return;
            }
            let bal = Self::balance(env, vault_id, owner);
            let new_debt = bal
                .checked_mul(vault.acc_rental_per_share)
                .unwrap_or_else(|| panic!("overflow"))
                / ACC_SCALE;
            Self::set_reward_debt(env, vault_id, owner, new_debt);
        }
    }

    fn transfer_shares_internal(env: &Env, vault_id: u64, from: &Address, to: &Address, amount: i128) {
        if amount <= 0 {
            panic!("invalid_amount");
        }

        Self::before_balance_change(env, vault_id, from);
        Self::before_balance_change(env, vault_id, to);

        let from_bal = Self::balance(env, vault_id, from);
        if from_bal < amount {
            panic!("insufficient_balance");
        }
        let to_bal = Self::balance(env, vault_id, to);

        Self::set_balance(env, vault_id, from, from_bal - amount);
        Self::set_balance(env, vault_id, to, to_bal + amount);

        Self::enforce_min_threshold(env, vault_id, from);
        Self::enforce_min_threshold(env, vault_id, to);

        Self::after_balance_change(env, vault_id, from);
        Self::after_balance_change(env, vault_id, to);
    }

    fn enforce_min_threshold(env: &Env, vault_id: u64, owner: &Address) {
        let vault: Vault = env
            .storage()
            .persistent()
            .get(&DataKey::Vault(vault_id))
            .unwrap_or_else(|| panic!("vault_not_found"));

        if vault.min_ownership_bps == 0 {
            return;
        }

        let bal = Self::balance(env, vault_id, owner);
        if bal == 0 {
            return;
        }

        let min_shares = (vault.total_shares
            .checked_mul(vault.min_ownership_bps as i128)
            .unwrap_or_else(|| panic!("overflow"))
            + (BPS_DENOMINATOR - 1))
            / BPS_DENOMINATOR;

        if bal < min_shares {
            panic!("below_min_ownership_threshold");
        }
    }
}

mod test;
