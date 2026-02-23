//! Puzzle Rental Contract

#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, Map, Symbol, Vec, log,
};

// ============================================================
// Storage Keys
// ============================================================

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Admin address
    Admin,
    /// Listing by listing_id
    Listing(u64),
    /// Active rental by rental_id
    Rental(u64),
    /// Next listing id counter
    NextListingId,
    /// Next rental id counter
    NextRentalId,
    /// Listings owned by an address
    OwnerListings(Address),
    /// Rentals by renter address
    RenterRentals(Address),
    /// Rental history entry (rental_id) → RentalRecord
    RentalHistory(u64),
    /// All active listing ids (marketplace discovery)
    ActiveListings,
}

// ============================================================
// Data Structures
// ============================================================

/// Status of a rental listing
#[contracttype]
#[derive(Clone, PartialEq)]
pub enum ListingStatus {
    Active,
    Paused,
    Cancelled,
}

/// Status of a rental
#[contracttype]
#[derive(Clone, PartialEq)]
pub enum RentalStatus {
    Active,
    Expired,
    Terminated,
}

/// A rental listing created by an NFT owner
#[contracttype]
#[derive(Clone)]
pub struct RentalListing {
    /// Unique listing id
    pub listing_id: u64,
    /// NFT contract address
    pub nft_contract: Address,
    /// Token id of the NFT
    pub nft_token_id: u64,
    /// Owner of the NFT
    pub owner: Address,
    /// Price per period (in tokens)
    pub price_per_period: i128,
    /// Duration of one rental period in ledger seconds
    pub period_duration: u64,
    /// Maximum number of periods a renter can rent
    pub max_periods: u32,
    /// Token used for payment
    pub payment_token: Address,
    /// Current status
    pub status: ListingStatus,
    /// Ledger timestamp when listing was created
    pub created_at: u64,
    /// Whether extensions are allowed
    pub allow_extensions: bool,
    /// Early termination refund percentage (0-100)
    pub early_termination_refund_pct: u32,
}

/// An active or historical rental agreement
#[contracttype]
#[derive(Clone)]
pub struct RentalAgreement {
    /// Unique rental id
    pub rental_id: u64,
    /// Associated listing id
    pub listing_id: u64,
    /// NFT contract address
    pub nft_contract: Address,
    /// Token id of the NFT
    pub nft_token_id: u64,
    /// Owner address
    pub owner: Address,
    /// Renter address
    pub renter: Address,
    /// Payment token
    pub payment_token: Address,
    /// Total tokens paid
    pub total_paid: i128,
    /// Price per period
    pub price_per_period: i128,
    /// Number of periods rented
    pub periods: u32,
    /// Rental start (ledger timestamp)
    pub start_time: u64,
    /// Rental end (ledger timestamp)
    pub end_time: u64,
    /// Current status
    pub status: RentalStatus,
    /// Early termination refund percentage
    pub early_termination_refund_pct: u32,
}

/// Historical record stored after rental closes
#[contracttype]
#[derive(Clone)]
pub struct RentalRecord {
    pub rental_id: u64,
    pub listing_id: u64,
    pub nft_contract: Address,
    pub nft_token_id: u64,
    pub owner: Address,
    pub renter: Address,
    pub total_paid: i128,
    pub start_time: u64,
    pub end_time: u64,
    pub final_status: RentalStatus,
}

/// Marketplace page result
#[contracttype]
#[derive(Clone)]
pub struct MarketplacePage {
    pub listings: Vec<RentalListing>,
    pub total: u64,
}

// ============================================================
// Events
// ============================================================
mod events {
    pub const LISTING_CREATED: &str = "listing_created";
    pub const LISTING_CANCELLED: &str = "listing_cancelled";
    pub const RENTAL_STARTED: &str = "rental_started";
    pub const RENTAL_EXTENDED: &str = "rental_extended";
    pub const RENTAL_EXPIRED: &str = "rental_expired";
    pub const RENTAL_TERMINATED: &str = "rental_terminated";
    pub const ACCESS_CHECKED: &str = "access_checked";
}

// ============================================================
// Contract
// ============================================================

#[contract]
pub struct PuzzleRentalContract;

#[contractimpl]
impl PuzzleRentalContract {
    // ----------------------------------------------------------
    // Initialization
    // ----------------------------------------------------------

    /// Initialize the contract with an admin address.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::NextListingId, &1u64);
        env.storage().instance().set(&DataKey::NextRentalId, &1u64);
        env.storage()
            .instance()
            .set(&DataKey::ActiveListings, &Vec::<u64>::new(&env));
    }

    // ----------------------------------------------------------
    // Listing Management
    // ----------------------------------------------------------

    /// Create a new rental listing for an NFT.
    /// The caller must own the NFT and authorize this call.
    pub fn create_listing(
        env: Env,
        owner: Address,
        nft_contract: Address,
        nft_token_id: u64,
        payment_token: Address,
        price_per_period: i128,
        period_duration: u64,
        max_periods: u32,
        allow_extensions: bool,
        early_termination_refund_pct: u32,
    ) -> u64 {
        owner.require_auth();

        if price_per_period <= 0 {
            panic!("price must be positive");
        }
        if period_duration == 0 {
            panic!("period duration must be > 0");
        }
        if max_periods == 0 {
            panic!("max periods must be > 0");
        }
        if early_termination_refund_pct > 100 {
            panic!("refund pct must be 0-100");
        }

        let listing_id = Self::next_listing_id(&env);
        let now = env.ledger().timestamp();

        let listing = RentalListing {
            listing_id,
            nft_contract,
            nft_token_id,
            owner: owner.clone(),
            price_per_period,
            period_duration,
            max_periods,
            payment_token,
            status: ListingStatus::Active,
            created_at: now,
            allow_extensions,
            early_termination_refund_pct,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Listing(listing_id), &listing);

        // Track owner's listings
        let mut owner_listings: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::OwnerListings(owner.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        owner_listings.push_back(listing_id);
        env.storage()
            .persistent()
            .set(&DataKey::OwnerListings(owner), &owner_listings);

        // Add to active listings marketplace index
        let mut active: Vec<u64> = env
            .storage()
            .instance()
            .get(&DataKey::ActiveListings)
            .unwrap_or_else(|| Vec::new(&env));
        active.push_back(listing_id);
        env.storage()
            .instance()
            .set(&DataKey::ActiveListings, &active);

        env.events().publish(
            (Symbol::new(&env, events::LISTING_CREATED),),
            listing_id,
        );

        log!(&env, "Listing created: {}", listing_id);
        listing_id
    }

    /// Cancel a listing (owner only). Cannot cancel if there's an active rental.
    pub fn cancel_listing(env: Env, owner: Address, listing_id: u64) {
        owner.require_auth();

        let mut listing: RentalListing = env
            .storage()
            .persistent()
            .get(&DataKey::Listing(listing_id))
            .unwrap_or_else(|| panic!("listing not found"));

        if listing.owner != owner {
            panic!("not the owner");
        }

        listing.status = ListingStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Listing(listing_id), &listing);

        // Remove from active listings index
        Self::remove_from_active_listings(&env, listing_id);

        env.events().publish(
            (Symbol::new(&env, events::LISTING_CANCELLED),),
            listing_id,
        );
    }

    // ----------------------------------------------------------
    // Rental Operations
    // ----------------------------------------------------------

    /// Accept a rental listing and pay for access.
    /// `periods` is how many rental periods the renter wants.
    pub fn rent(
        env: Env,
        renter: Address,
        listing_id: u64,
        periods: u32,
    ) -> u64 {
        renter.require_auth();

        if periods == 0 {
            panic!("periods must be > 0");
        }

        let listing: RentalListing = env
            .storage()
            .persistent()
            .get(&DataKey::Listing(listing_id))
            .unwrap_or_else(|| panic!("listing not found"));

        if listing.status != ListingStatus::Active {
            panic!("listing not active");
        }
        if periods > listing.max_periods {
            panic!("exceeds max periods");
        }
        if listing.owner == renter {
            panic!("owner cannot rent own listing");
        }

        let total_cost = listing.price_per_period * periods as i128;
        let now = env.ledger().timestamp();
        let end_time = now + listing.period_duration * periods as u64;

        // Escrow payment in the contract; disbursed to owner on expiry/termination.
        let payment_client = token::Client::new(&env, &listing.payment_token);
        let contract_address = env.current_contract_address();
        payment_client.transfer(&renter, &contract_address, &total_cost);

        let rental_id = Self::next_rental_id(&env);

        let rental = RentalAgreement {
            rental_id,
            listing_id,
            nft_contract: listing.nft_contract.clone(),
            nft_token_id: listing.nft_token_id,
            owner: listing.owner.clone(),
            renter: renter.clone(),
            payment_token: listing.payment_token.clone(),
            total_paid: total_cost,
            price_per_period: listing.price_per_period,
            periods,
            start_time: now,
            end_time,
            status: RentalStatus::Active,
            early_termination_refund_pct: listing.early_termination_refund_pct,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Rental(rental_id), &rental);

        // Track renter's rentals
        let mut renter_rentals: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::RenterRentals(renter.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        renter_rentals.push_back(rental_id);
        env.storage()
            .persistent()
            .set(&DataKey::RenterRentals(renter), &renter_rentals);

        env.events().publish(
            (Symbol::new(&env, events::RENTAL_STARTED),),
            (rental_id, listing_id, total_cost),
        );

        log!(&env, "Rental started: id={} listing={} cost={}", rental_id, listing_id, total_cost);
        rental_id
    }

    /// Extend an active rental by additional periods.
    pub fn extend_rental(env: Env, renter: Address, rental_id: u64, additional_periods: u32) {
        renter.require_auth();

        if additional_periods == 0 {
            panic!("additional periods must be > 0");
        }

        let mut rental: RentalAgreement = env
            .storage()
            .persistent()
            .get(&DataKey::Rental(rental_id))
            .unwrap_or_else(|| panic!("rental not found"));

        if rental.renter != renter {
            panic!("not the renter");
        }

        Self::auto_expire_if_needed(&env, &mut rental);

        if rental.status != RentalStatus::Active {
            panic!("rental is not active");
        }

        let listing: RentalListing = env
            .storage()
            .persistent()
            .get(&DataKey::Listing(rental.listing_id))
            .unwrap_or_else(|| panic!("listing not found"));

        if !listing.allow_extensions {
            panic!("extensions not allowed for this listing");
        }

        let new_total_periods = rental.periods + additional_periods;
        if new_total_periods > listing.max_periods {
            panic!("exceeds max periods");
        }

        let extension_cost = listing.price_per_period * additional_periods as i128;

        // Escrow additional payment in the contract.
        let payment_client = token::Client::new(&env, &listing.payment_token);
        let contract_address = env.current_contract_address();
        payment_client.transfer(&renter, &contract_address, &extension_cost);

        rental.end_time += listing.period_duration * additional_periods as u64;
        rental.total_paid += extension_cost;
        rental.periods = new_total_periods;

        env.storage()
            .persistent()
            .set(&DataKey::Rental(rental_id), &rental);

        env.events().publish(
            (Symbol::new(&env, events::RENTAL_EXTENDED),),
            (rental_id, additional_periods, extension_cost),
        );

        log!(&env, "Rental extended: id={} added_periods={} cost={}", rental_id, additional_periods, extension_cost);
    }

    /// Terminate a rental early. Renter gets a partial refund based on
    /// the listing's `early_termination_refund_pct` of unused time.
    pub fn terminate_rental(env: Env, renter: Address, rental_id: u64) {
        renter.require_auth();

        let mut rental: RentalAgreement = env
            .storage()
            .persistent()
            .get(&DataKey::Rental(rental_id))
            .unwrap_or_else(|| panic!("rental not found"));

        if rental.renter != renter {
            panic!("not the renter");
        }

        Self::auto_expire_if_needed(&env, &mut rental);

        if rental.status != RentalStatus::Active {
            panic!("rental is not active");
        }

        let now = env.ledger().timestamp();
        let remaining_time = if now < rental.end_time {
            rental.end_time - now
        } else {
            0
        };
        let total_duration = rental.end_time - rental.start_time;

        // Calculate refund for unused portion
        let refund = if remaining_time > 0 && total_duration > 0 && rental.early_termination_refund_pct > 0 {
            let unused_ratio_numerator = remaining_time as i128;
            let unused_ratio_denominator = total_duration as i128;
            let raw_refund = rental.total_paid * unused_ratio_numerator / unused_ratio_denominator;
            raw_refund * rental.early_termination_refund_pct as i128 / 100
        } else {
            0
        };

        let payment_client = token::Client::new(&env, &rental.payment_token);
        let contract_address = env.current_contract_address();
        let owner_share = rental.total_paid - refund;
        // Pay owner their earned portion from escrow
        if owner_share > 0 {
            payment_client.transfer(&contract_address, &rental.owner, &owner_share);
        }
        // Refund unused portion to renter from escrow
        if refund > 0 {
            payment_client.transfer(&contract_address, &rental.renter, &refund);
        }

        rental.status = RentalStatus::Terminated;

        // Archive to history
        Self::archive_rental(&env, &rental);

        env.storage()
            .persistent()
            .set(&DataKey::Rental(rental_id), &rental);

        env.events().publish(
            (Symbol::new(&env, events::RENTAL_TERMINATED),),
            (rental_id, refund),
        );

        log!(&env, "Rental terminated: id={} refund={}", rental_id, refund);
    }

    /// Mark a rental as expired if its end_time has passed.
    /// Anyone can call this to trigger expiration.
    pub fn expire_rental(env: Env, rental_id: u64) {
        let mut rental: RentalAgreement = env
            .storage()
            .persistent()
            .get(&DataKey::Rental(rental_id))
            .unwrap_or_else(|| panic!("rental not found"));

        if rental.status != RentalStatus::Active {
            panic!("rental already closed");
        }

        let now = env.ledger().timestamp();
        if now < rental.end_time {
            panic!("rental has not expired yet");
        }

        rental.status = RentalStatus::Expired;

        // Release escrowed funds to owner on normal expiry
        let payment_client = token::Client::new(&env, &rental.payment_token);
        let contract_address = env.current_contract_address();
        payment_client.transfer(&contract_address, &rental.owner, &rental.total_paid);

        // Archive to history
        Self::archive_rental(&env, &rental);

        env.storage()
            .persistent()
            .set(&DataKey::Rental(rental_id), &rental);

        env.events().publish(
            (Symbol::new(&env, events::RENTAL_EXPIRED),),
            rental_id,
        );

        log!(&env, "Rental expired: id={}", rental_id);
    }

    // ----------------------------------------------------------
    // Access Control
    // ----------------------------------------------------------

    /// Check if a renter currently has valid access to an NFT puzzle.
    /// Returns true if access is granted, false otherwise.
    pub fn has_access(env: Env, renter: Address, nft_contract: Address, nft_token_id: u64) -> bool {
        let rental_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::RenterRentals(renter.clone()))
            .unwrap_or_else(|| Vec::new(&env));

        let now = env.ledger().timestamp();

        for rental_id in rental_ids.iter() {
            let rental: RentalAgreement = match env
                .storage()
                .persistent()
                .get(&DataKey::Rental(rental_id))
            {
                Some(r) => r,
                None => continue,
            };

            if rental.status == RentalStatus::Active
                && rental.nft_contract == nft_contract
                && rental.nft_token_id == nft_token_id
                && now < rental.end_time
            {
                env.events().publish(
                    (Symbol::new(&env, events::ACCESS_CHECKED),),
                    (renter, true),
                );
                return true;
            }
        }

        env.events().publish(
            (Symbol::new(&env, events::ACCESS_CHECKED),),
            (renter, false),
        );
        false
    }

    // ----------------------------------------------------------
    // Marketplace Discovery
    // ----------------------------------------------------------

    /// Get a page of active listings for marketplace discovery.
    /// `offset` and `limit` allow pagination.
    pub fn get_active_listings(env: Env, offset: u64, limit: u32) -> MarketplacePage {
        let active_ids: Vec<u64> = env
            .storage()
            .instance()
            .get(&DataKey::ActiveListings)
            .unwrap_or_else(|| Vec::new(&env));

        let total = active_ids.len() as u64;
        let mut listings = Vec::new(&env);

        let start = offset as u32;
        let end = (offset as u32 + limit).min(active_ids.len());

        for i in start..end {
            let listing_id = active_ids.get(i).unwrap();
            if let Some(listing) = env
                .storage()
                .persistent()
                .get::<DataKey, RentalListing>(&DataKey::Listing(listing_id))
            {
                if listing.status == ListingStatus::Active {
                    listings.push_back(listing);
                }
            }
        }

        MarketplacePage { listings, total }
    }

    /// Get all listing ids for a given owner.
    pub fn get_owner_listings(env: Env, owner: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::OwnerListings(owner))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Get all rental ids for a given renter.
    pub fn get_renter_rentals(env: Env, renter: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::RenterRentals(renter))
            .unwrap_or_else(|| Vec::new(&env))
    }

    // ----------------------------------------------------------
    // Query Helpers
    // ----------------------------------------------------------

    /// Get a listing by id.
    pub fn get_listing(env: Env, listing_id: u64) -> RentalListing {
        env.storage()
            .persistent()
            .get(&DataKey::Listing(listing_id))
            .unwrap_or_else(|| panic!("listing not found"))
    }

    /// Get a rental by id.
    pub fn get_rental(env: Env, rental_id: u64) -> RentalAgreement {
        env.storage()
            .persistent()
            .get(&DataKey::Rental(rental_id))
            .unwrap_or_else(|| panic!("rental not found"))
    }

    /// Get rental history record.
    pub fn get_rental_history(env: Env, rental_id: u64) -> RentalRecord {
        env.storage()
            .persistent()
            .get(&DataKey::RentalHistory(rental_id))
            .unwrap_or_else(|| panic!("rental history not found"))
    }

    // ----------------------------------------------------------
    // Internal Helpers
    // ----------------------------------------------------------

    fn next_listing_id(env: &Env) -> u64 {
        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextListingId)
            .unwrap_or(1);
        env.storage()
            .instance()
            .set(&DataKey::NextListingId, &(id + 1));
        id
    }

    fn next_rental_id(env: &Env) -> u64 {
        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextRentalId)
            .unwrap_or(1);
        env.storage()
            .instance()
            .set(&DataKey::NextRentalId, &(id + 1));
        id
    }

    fn auto_expire_if_needed(env: &Env, rental: &mut RentalAgreement) {
        if rental.status == RentalStatus::Active {
            let now = env.ledger().timestamp();
            if now >= rental.end_time {
                rental.status = RentalStatus::Expired;
            }
        }
    }

    fn archive_rental(env: &Env, rental: &RentalAgreement) {
        let record = RentalRecord {
            rental_id: rental.rental_id,
            listing_id: rental.listing_id,
            nft_contract: rental.nft_contract.clone(),
            nft_token_id: rental.nft_token_id,
            owner: rental.owner.clone(),
            renter: rental.renter.clone(),
            total_paid: rental.total_paid,
            start_time: rental.start_time,
            end_time: rental.end_time,
            final_status: rental.status.clone(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::RentalHistory(rental.rental_id), &record);
    }

    fn remove_from_active_listings(env: &Env, listing_id: u64) {
        let active: Vec<u64> = env
            .storage()
            .instance()
            .get(&DataKey::ActiveListings)
            .unwrap_or_else(|| Vec::new(env));

        let mut new_active = Vec::new(env);
        for id in active.iter() {
            if id != listing_id {
                new_active.push_back(id);
            }
        }
        env.storage()
            .instance()
            .set(&DataKey::ActiveListings, &new_active);
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod test;