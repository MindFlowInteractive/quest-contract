#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror,
    panic_with_error, symbol_short,
    Address, Env, String, Symbol, token,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    LeaseNotFound = 1,
    LeaseAlreadyActive = 2,
    LeaseExpired = 3,
    UnauthorizedCaller = 4,
    InvalidTerms = 5,
    InsufficientCollateral = 6,
    ListingNotFound = 7,
    DisputeNotFound = 8,
    LeaseNotTerminable = 9,
    AlreadyDisputed = 10,
    InvalidSplit = 11,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LeaseStatus {
    Pending,
    Active,
    Expired,
    Terminated,
    Disputed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisputeStatus {
    Open,
    ResolvedForLessor,
    ResolvedForLessee,
}

#[contracttype]
#[derive(Clone)]
pub struct LeaseAgreement {
    pub lease_id: u64,
    pub lessor: Address,
    pub lessee: Address,
    pub nft_id: u64,
    pub start_time: u64,
    pub duration: u64,
    pub lessor_share: u32,
    pub collateral: i128,
    pub collateral_deposited: bool,
    pub status: LeaseStatus,
    pub total_revenue: i128,
    pub lessor_earned: i128,
    pub lessee_earned: i128,
    pub renewable: bool,
    pub renewal_count: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct MarketplaceListing {
    pub listing_id: u64,
    pub lessor: Address,
    pub nft_id: u64,
    pub lessor_share: u32,
    pub duration: u64,
    pub collateral_required: i128,
    pub active: bool,
    pub renewable: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct DisputeCase {
    pub dispute_id: u64,
    pub lease_id: u64,
    pub claimant: Address,
    pub reason: String,
    pub status: DisputeStatus,
    pub created_at: u64,
}

const LEASE_COUNTER: Symbol = symbol_short!("L_CNT");
const LISTING_COUNTER: Symbol = symbol_short!("LS_CNT");
const DISPUTE_COUNTER: Symbol = symbol_short!("D_CNT");
const ADMIN: Symbol = symbol_short!("ADMIN");
const TOKEN: Symbol = symbol_short!("TOKEN");

#[contracttype]
pub enum DataKey {
    Lease(u64),
    Listing(u64),
    Dispute(u64),
}

#[contract]
pub struct NFTLeasingContract;

#[contractimpl]
impl NFTLeasingContract {
    pub fn initialize(env: Env, admin: Address, token: Address) {
        env.storage().instance().set(&ADMIN, &admin);
        env.storage().instance().set(&TOKEN, &token);
        env.storage().instance().set(&LEASE_COUNTER, &0u64);
        env.storage().instance().set(&LISTING_COUNTER, &0u64);
        env.storage().instance().set(&DISPUTE_COUNTER, &0u64);
    }

    pub fn create_lease(
        env: Env,
        lessor: Address,
        lessee: Address,
        nft_id: u64,
        duration: u64,
        lessor_share: u32,
        collateral: i128,
        renewable: bool,
    ) -> u64 {
        lessor.require_auth();

        if lessor_share > 100 {
            panic_with_error!(&env, Error::InvalidSplit);
        }
        if duration == 0 {
            panic_with_error!(&env, Error::InvalidTerms);
        }

        let lease_id: u64 = env.storage().instance().get(&LEASE_COUNTER).unwrap_or(0) + 1;
        env.storage().instance().set(&LEASE_COUNTER, &lease_id);

        let lease = LeaseAgreement {
            lease_id,
            lessor,
            lessee,
            nft_id,
            start_time: 0,
            duration,
            lessor_share,
            collateral,
            collateral_deposited: collateral == 0,
            status: LeaseStatus::Pending,
            total_revenue: 0,
            lessor_earned: 0,
            lessee_earned: 0,
            renewable,
            renewal_count: 0,
        };

        env.storage().persistent().set(&DataKey::Lease(lease_id), &lease);
        lease_id
    }

    pub fn deposit_collateral(env: Env, lease_id: u64) {
        let mut lease = Self::get_lease(env.clone(), lease_id);
        lease.lessee.require_auth();

        if lease.collateral_deposited {
            return;
        }

        let token_addr: Address = env.storage().instance().get(&TOKEN).unwrap();
        let lessee = lease.lessee.clone();
        let collateral = lease.collateral;

        token::Client::new(&env, &token_addr).transfer(
            &lessee,
            &env.current_contract_address(),
            &collateral,
        );

        lease.collateral_deposited = true;
        env.storage().persistent().set(&DataKey::Lease(lease_id), &lease);
    }

    pub fn activate_lease(env: Env, lease_id: u64) {
        let mut lease = Self::get_lease(env.clone(), lease_id);
        lease.lessor.require_auth();

        if lease.status != LeaseStatus::Pending {
            panic_with_error!(&env, Error::LeaseAlreadyActive);
        }
        if !lease.collateral_deposited {
            panic_with_error!(&env, Error::InsufficientCollateral);
        }

        lease.start_time = env.ledger().timestamp();
        lease.status = LeaseStatus::Active;
        env.storage().persistent().set(&DataKey::Lease(lease_id), &lease);
    }

    pub fn record_reward(env: Env, lease_id: u64, amount: i128) {
        let mut lease = Self::get_lease(env.clone(), lease_id);

        if lease.status != LeaseStatus::Active {
            panic_with_error!(&env, Error::LeaseExpired);
        }

        if env.ledger().timestamp() > lease.start_time + lease.duration {
            lease.status = LeaseStatus::Expired;
            env.storage().persistent().set(&DataKey::Lease(lease_id), &lease);
            panic_with_error!(&env, Error::LeaseExpired);
        }

        lease.total_revenue += amount;
        env.storage().persistent().set(&DataKey::Lease(lease_id), &lease);
    }

    pub fn distribute_revenue(env: Env, lease_id: u64) {
        let mut lease = Self::get_lease(env.clone(), lease_id);

        if lease.status != LeaseStatus::Active {
            panic_with_error!(&env, Error::LeaseExpired);
        }

        let distributable = lease.total_revenue - (lease.lessor_earned + lease.lessee_earned);
        if distributable <= 0 {
            return;
        }

        let token_addr: Address = env.storage().instance().get(&TOKEN).unwrap();
        let token_client = token::Client::new(&env, &token_addr);
        let contract_addr = env.current_contract_address();

        let lessor_amount = (distributable * lease.lessor_share as i128) / 100;
        let lessee_amount = distributable - lessor_amount;
        let lessor = lease.lessor.clone();
        let lessee = lease.lessee.clone();

        if lessor_amount > 0 {
            token_client.transfer(&contract_addr, &lessor, &lessor_amount);
            lease.lessor_earned += lessor_amount;
        }
        if lessee_amount > 0 {
            token_client.transfer(&contract_addr, &lessee, &lessee_amount);
            lease.lessee_earned += lessee_amount;
        }

        env.storage().persistent().set(&DataKey::Lease(lease_id), &lease);
    }

    pub fn renew_lease(env: Env, lease_id: u64, new_duration: u64) {
        let mut lease = Self::get_lease(env.clone(), lease_id);
        lease.lessee.require_auth();

        if !lease.renewable {
            panic_with_error!(&env, Error::InvalidTerms);
        }
        if lease.status != LeaseStatus::Active && lease.status != LeaseStatus::Expired {
            panic_with_error!(&env, Error::LeaseNotTerminable);
        }
        if new_duration == 0 {
            panic_with_error!(&env, Error::InvalidTerms);
        }

        lease.start_time = env.ledger().timestamp();
        lease.duration = new_duration;
        lease.status = LeaseStatus::Active;
        lease.renewal_count += 1;
        env.storage().persistent().set(&DataKey::Lease(lease_id), &lease);
    }

    pub fn terminate_lease(env: Env, lease_id: u64, caller: Address) {
        caller.require_auth();

        let mut lease = Self::get_lease(env.clone(), lease_id);

        if caller != lease.lessor && caller != lease.lessee {
            panic_with_error!(&env, Error::UnauthorizedCaller);
        }
        if lease.status == LeaseStatus::Terminated {
            panic_with_error!(&env, Error::LeaseNotTerminable);
        }

        let token_addr: Address = env.storage().instance().get(&TOKEN).unwrap();
        let token_client = token::Client::new(&env, &token_addr);
        let contract_addr = env.current_contract_address();

        let distributable = lease.total_revenue - (lease.lessor_earned + lease.lessee_earned);
        if distributable > 0 {
            let lessor_amount = (distributable * lease.lessor_share as i128) / 100;
            let lessee_amount = distributable - lessor_amount;
            let lessor = lease.lessor.clone();
            let lessee = lease.lessee.clone();

            if lessor_amount > 0 {
                token_client.transfer(&contract_addr, &lessor, &lessor_amount);
                lease.lessor_earned += lessor_amount;
            }
            if lessee_amount > 0 {
                token_client.transfer(&contract_addr, &lessee, &lessee_amount);
                lease.lessee_earned += lessee_amount;
            }
        }

        if lease.collateral > 0 && lease.collateral_deposited {
            let lessee = lease.lessee.clone();
            let collateral = lease.collateral;
            token_client.transfer(&contract_addr, &lessee, &collateral);
        }

        lease.status = LeaseStatus::Terminated;
        env.storage().persistent().set(&DataKey::Lease(lease_id), &lease);
    }

    pub fn list_on_marketplace(
        env: Env,
        lessor: Address,
        nft_id: u64,
        lessor_share: u32,
        duration: u64,
        collateral_required: i128,
        renewable: bool,
    ) -> u64 {
        lessor.require_auth();

        if lessor_share > 100 {
            panic_with_error!(&env, Error::InvalidSplit);
        }

        let listing_id: u64 = env.storage().instance().get(&LISTING_COUNTER).unwrap_or(0) + 1;
        env.storage().instance().set(&LISTING_COUNTER, &listing_id);

        let listing = MarketplaceListing {
            listing_id,
            lessor,
            nft_id,
            lessor_share,
            duration,
            collateral_required,
            active: true,
            renewable,
        };

        env.storage().persistent().set(&DataKey::Listing(listing_id), &listing);
        listing_id
    }

    pub fn take_marketplace_listing(env: Env, listing_id: u64, lessee: Address) -> u64 {
        lessee.require_auth();

        let mut listing: MarketplaceListing = env
            .storage()
            .persistent()
            .get(&DataKey::Listing(listing_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::ListingNotFound));

        if !listing.active {
            panic_with_error!(&env, Error::ListingNotFound);
        }

        listing.active = false;
        env.storage().persistent().set(&DataKey::Listing(listing_id), &listing);

        let lease_id: u64 = env.storage().instance().get(&LEASE_COUNTER).unwrap_or(0) + 1;
        env.storage().instance().set(&LEASE_COUNTER, &lease_id);

        let collateral_deposited = listing.collateral_required == 0;
        let lease = LeaseAgreement {
            lease_id,
            lessor: listing.lessor.clone(),
            lessee,
            nft_id: listing.nft_id,
            start_time: 0,
            duration: listing.duration,
            lessor_share: listing.lessor_share,
            collateral: listing.collateral_required,
            collateral_deposited,
            status: LeaseStatus::Pending,
            total_revenue: 0,
            lessor_earned: 0,
            lessee_earned: 0,
            renewable: listing.renewable,
            renewal_count: 0,
        };

        env.storage().persistent().set(&DataKey::Lease(lease_id), &lease);
        lease_id
    }

    pub fn open_dispute(env: Env, lease_id: u64, claimant: Address, reason: String) -> u64 {
        claimant.require_auth();

        let mut lease = Self::get_lease(env.clone(), lease_id);

        if claimant != lease.lessor && claimant != lease.lessee {
            panic_with_error!(&env, Error::UnauthorizedCaller);
        }
        if lease.status == LeaseStatus::Disputed {
            panic_with_error!(&env, Error::AlreadyDisputed);
        }

        let dispute_id: u64 = env.storage().instance().get(&DISPUTE_COUNTER).unwrap_or(0) + 1;
        env.storage().instance().set(&DISPUTE_COUNTER, &dispute_id);

        let dispute = DisputeCase {
            dispute_id,
            lease_id,
            claimant,
            reason,
            status: DisputeStatus::Open,
            created_at: env.ledger().timestamp(),
        };

        lease.status = LeaseStatus::Disputed;
        env.storage().persistent().set(&DataKey::Lease(lease_id), &lease);
        env.storage().persistent().set(&DataKey::Dispute(dispute_id), &dispute);
        dispute_id
    }

    pub fn resolve_dispute(env: Env, dispute_id: u64, favor_lessor: bool) {
        let admin: Address = env.storage().instance().get(&ADMIN).unwrap();
        admin.require_auth();

        let mut dispute: DisputeCase = env
            .storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::DisputeNotFound));

        let lease_id = dispute.lease_id;
        let mut lease = Self::get_lease(env.clone(), lease_id);

        let token_addr: Address = env.storage().instance().get(&TOKEN).unwrap();
        let token_client = token::Client::new(&env, &token_addr);
        let contract_addr = env.current_contract_address();

        if lease.collateral > 0 && lease.collateral_deposited {
            let collateral = lease.collateral;
            if favor_lessor {
                let lessor = lease.lessor.clone();
                token_client.transfer(&contract_addr, &lessor, &collateral);
            } else {
                let lessee = lease.lessee.clone();
                token_client.transfer(&contract_addr, &lessee, &collateral);
            }
        }

        dispute.status = if favor_lessor {
            DisputeStatus::ResolvedForLessor
        } else {
            DisputeStatus::ResolvedForLessee
        };

        lease.status = LeaseStatus::Terminated;
        env.storage().persistent().set(&DataKey::Dispute(dispute_id), &dispute);
        env.storage().persistent().set(&DataKey::Lease(lease_id), &lease);
    }

    pub fn get_lease(env: Env, lease_id: u64) -> LeaseAgreement {
        env.storage()
            .persistent()
            .get(&DataKey::Lease(lease_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::LeaseNotFound))
    }

    pub fn get_listing(env: Env, listing_id: u64) -> MarketplaceListing {
        env.storage()
            .persistent()
            .get(&DataKey::Listing(listing_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::ListingNotFound))
    }

    pub fn get_dispute(env: Env, dispute_id: u64) -> DisputeCase {
        env.storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::DisputeNotFound))
    }
}

#[cfg(test)]
mod test;
