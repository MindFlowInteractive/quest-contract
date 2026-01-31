#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror, token, Address, Env, Map, Symbol, Vec,
};

// ──────────────────────────────────────────────────────────
// DATA STRUCTURES
// ──────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HintQuality {
    Poor = 1,
    Fair = 2,
    Good = 3,
    Excellent = 4,
    Perfect = 5,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListingStatus {
    Active = 1,
    Sold = 2,
    Cancelled = 3,
    Expired = 4,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hint {
    pub hint_id: u64,
    pub puzzle_id: u32,
    pub creator: Address,
    pub content_hash: [u8; 32], // Hash of hint content (stored off-chain)
    pub quality: HintQuality,
    pub created_at: u64,
    pub total_sales: u32,
    pub total_rating: u64, // Sum of all ratings
    pub rating_count: u32,  // Number of ratings
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HintListing {
    pub listing_id: u64,
    pub hint_id: u64,
    pub seller: Address,
    pub payment_token: Address,
    pub base_price: i128,
    pub current_price: i128,
    pub status: ListingStatus,
    pub created_time: u64,
    pub expiration_time: u64,
    pub creator: Address,
    pub royalty_bps: u32, // Royalty in basis points (10000 = 100%)
    pub quality: HintQuality,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HintPack {
    pub pack_id: u64,
    pub name: Symbol,
    pub hint_ids: Vec<u64>,
    pub pack_price: i128,
    pub discount_bps: u32, // Discount percentage in basis points
    pub creator: Address,
    pub created_at: u64,
    pub expiration_time: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rating {
    pub rater: Address,
    pub hint_id: u64,
    pub quality_rating: u32, // 1-5 scale
    pub helpfulness: u32,    // 1-5 scale
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarketplaceConfig {
    pub admin: Address,
    pub fee_recipient: Address,
    pub fee_bps: u32, // Marketplace fee in basis points
    pub min_listing_duration: u64,
    pub max_listing_duration: u64,
    pub price_adjustment_factor: u32, // For dynamic pricing (basis points)
    pub min_quality_for_listing: HintQuality,
}

#[contracttype]
pub enum DataKey {
    Config,                              // MarketplaceConfig
    Hint(u64),                          // Hint
    HintCounter,                        // u64
    Listing(u64),                       // HintListing
    ListingCounter,                     // u64
    Pack(u64),                          // HintPack
    PackCounter,                        // u64
    Rating(u64, Address),              // Rating (hint_id, rater)
    RatingsByHint(u64),                // Vec<Address> - raters for a hint
    ListingsByHint(u64),                // Vec<u64> - listing IDs for a hint
    ListingsBySeller(Address),          // Vec<u64> - listing IDs by seller
    ListingsByPuzzle(u32),              // Vec<u64> - listing IDs by puzzle
    ActiveListings,                     // Vec<u64> - all active listings
    PriceHistory(u64),                  // Vec<i128> - price history for a hint
    DemandMetrics(u64),                 // DemandMetrics
    PacksByCreator(Address),            // Vec<u64> - pack IDs by creator
    ActivePacks,                        // Vec<u64> - all active packs
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DemandMetrics {
    pub hint_id: u64,
    pub views: u32,
    pub purchases: u32,
    pub last_purchase_time: u64,
    pub average_time_to_sale: u64, // Average time between listing and sale
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MarketplaceError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    HintNotFound = 4,
    ListingNotFound = 5,
    ListingNotActive = 6,
    ListingExpired = 7,
    InvalidPrice = 8,
    InvalidDuration = 9,
    InsufficientBalance = 10,
    InvalidQuality = 11,
    InvalidRating = 12,
    PackNotFound = 13,
    PackExpired = 14,
    DuplicateRating = 15,
}

// ──────────────────────────────────────────────────────────
// CONTRACT IMPLEMENTATION
// ──────────────────────────────────────────────────────────

#[contract]
pub struct HintMarketplace;

#[contractimpl]
impl HintMarketplace {
    /// Initialize the hint marketplace contract
    pub fn initialize(
        env: Env,
        admin: Address,
        fee_recipient: Address,
        fee_bps: u32,
        min_listing_duration: u64,
        max_listing_duration: u64,
        price_adjustment_factor: u32,
        min_quality_for_listing: HintQuality,
    ) {
        if env.storage().instance().has(&DataKey::Config) {
            panic!("Already initialized");
        }

        if fee_bps > 10000 {
            panic!("Fee cannot exceed 100%");
        }

        if price_adjustment_factor > 10000 {
            panic!("Price adjustment factor cannot exceed 100%");
        }

        let config = MarketplaceConfig {
            admin,
            fee_recipient,
            fee_bps,
            min_listing_duration,
            max_listing_duration,
            price_adjustment_factor,
            min_quality_for_listing,
        };

        env.storage().instance().set(&DataKey::Config, &config);
        env.storage().instance().set(&DataKey::HintCounter, &0u64);
        env.storage().instance().set(&DataKey::ListingCounter, &0u64);
        env.storage().instance().set(&DataKey::PackCounter, &0u64);
    }

    /// Update marketplace configuration (admin only)
    pub fn update_config(
        env: Env,
        fee_recipient: Option<Address>,
        fee_bps: Option<u32>,
        min_listing_duration: Option<u64>,
        max_listing_duration: Option<u64>,
        price_adjustment_factor: Option<u32>,
        min_quality_for_listing: Option<HintQuality>,
    ) {
        let config: MarketplaceConfig = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .expect("Not initialized");

        config.admin.require_auth();

        let mut new_config = config.clone();

        if let Some(recipient) = fee_recipient {
            new_config.fee_recipient = recipient;
        }

        if let Some(bps) = fee_bps {
            if bps > 10000 {
                panic!("Fee cannot exceed 100%");
            }
            new_config.fee_bps = bps;
        }

        if let Some(min) = min_listing_duration {
            new_config.min_listing_duration = min;
        }

        if let Some(max) = max_listing_duration {
            new_config.max_listing_duration = max;
        }

        if let Some(factor) = price_adjustment_factor {
            if factor > 10000 {
                panic!("Price adjustment factor cannot exceed 100%");
            }
            new_config.price_adjustment_factor = factor;
        }

        if let Some(quality) = min_quality_for_listing {
            new_config.min_quality_for_listing = quality;
        }

        env.storage().instance().set(&DataKey::Config, &new_config);
    }

    /// Create a new hint
    pub fn create_hint(
        env: Env,
        creator: Address,
        puzzle_id: u32,
        content_hash: [u8; 32],
        quality: HintQuality,
    ) -> u64 {
        creator.require_auth();

        // Generate hint ID
        let mut hint_id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::HintCounter)
            .unwrap_or(0);
        hint_id += 1;
        env.storage().instance().set(&DataKey::HintCounter, &hint_id);

        let now = env.ledger().timestamp();

        let hint = Hint {
            hint_id,
            puzzle_id,
            creator: creator.clone(),
            content_hash,
            quality,
            created_at: now,
            total_sales: 0,
            total_rating: 0,
            rating_count: 0,
        };

        env.storage().instance().set(&DataKey::Hint(hint_id), &hint);

        // Initialize demand metrics
        let metrics = DemandMetrics {
            hint_id,
            views: 0,
            purchases: 0,
            last_purchase_time: 0,
            average_time_to_sale: 0,
        };
        env.storage()
            .instance()
            .set(&DataKey::DemandMetrics(hint_id), &metrics);

        hint_id
    }

    /// Create a listing for a hint
    pub fn create_listing(
        env: Env,
        seller: Address,
        hint_id: u64,
        payment_token: Address,
        base_price: i128,
        duration: u64,
        royalty_bps: u32,
    ) -> u64 {
        seller.require_auth();

        let config: MarketplaceConfig = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .expect("Not initialized");

        // Verify hint exists
        let hint: Hint = env
            .storage()
            .instance()
            .get(&DataKey::Hint(hint_id))
            .expect("Hint not found");

        // Verify quality meets minimum requirement
        if hint.quality < config.min_quality_for_listing {
            panic!("Hint quality below minimum requirement");
        }

        // Verify seller owns the hint (in a real implementation, this would check ownership)
        // For now, we assume the creator can list their hints

        if base_price <= 0 {
            panic!("Price must be positive");
        }

        if royalty_bps > 10000 {
            panic!("Royalty cannot exceed 100%");
        }

        if duration < config.min_listing_duration || duration > config.max_listing_duration {
            panic!("Invalid listing duration");
        }

        let now = env.ledger().timestamp();
        let expiration_time = now + duration;

        // Calculate initial price using dynamic pricing
        let current_price = Self::calculate_dynamic_price(&env, hint_id, base_price);

        // Generate listing ID
        let mut listing_id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ListingCounter)
            .unwrap_or(0);
        listing_id += 1;
        env.storage().instance().set(&DataKey::ListingCounter, &listing_id);

        let listing = HintListing {
            listing_id,
            hint_id,
            seller: seller.clone(),
            payment_token,
            base_price,
            current_price,
            status: ListingStatus::Active,
            created_time: now,
            expiration_time,
            creator: hint.creator.clone(),
            royalty_bps,
            quality: hint.quality,
        };

        // Save listing
        env.storage()
            .instance()
            .set(&DataKey::Listing(listing_id), &listing);

        // Update indexes
        let mut hint_listings = Self::get_listings_by_hint(&env, hint_id);
        hint_listings.push_back(listing_id);
        env.storage()
            .instance()
            .set(&DataKey::ListingsByHint(hint_id), &hint_listings);

        let mut seller_listings = Self::get_listings_by_seller(&env, &seller);
        seller_listings.push_back(listing_id);
        env.storage()
            .instance()
            .set(&DataKey::ListingsBySeller(seller.clone()), &seller_listings);

        let mut puzzle_listings = Self::get_listings_by_puzzle(&env, hint.puzzle_id);
        puzzle_listings.push_back(listing_id);
        env.storage()
            .instance()
            .set(&DataKey::ListingsByPuzzle(hint.puzzle_id), &puzzle_listings);

        let mut active_listings = Self::get_active_listings(&env);
        active_listings.push_back(listing_id);
        env.storage()
            .instance()
            .set(&DataKey::ActiveListings, &active_listings);

        listing_id
    }

    /// Buy a listed hint
    pub fn buy(env: Env, buyer: Address, listing_id: u64) {
        buyer.require_auth();

        let mut listing: HintListing = env
            .storage()
            .instance()
            .get(&DataKey::Listing(listing_id))
            .expect("Listing not found");

        // Check listing status
        if listing.status != ListingStatus::Active {
            panic!("Listing is not active");
        }

        // Check expiration
        let now = env.ledger().timestamp();
        if now > listing.expiration_time {
            listing.status = ListingStatus::Expired;
            env.storage()
                .instance()
                .set(&DataKey::Listing(listing_id), &listing);
            Self::remove_from_active_listings(&env, listing_id);
            panic!("Listing has expired");
        }

        if listing.seller == buyer {
            panic!("Cannot buy your own listing");
        }

        let config: MarketplaceConfig = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .expect("Not initialized");

        // Calculate fees and royalties
        let (seller_amount, fee_amount, royalty_amount) = Self::calculate_payouts(
            &env,
            listing.current_price,
            config.fee_bps,
            listing.royalty_bps,
        );

        // Transfer payment from buyer to contract
        let token_client = token::Client::new(&env, &listing.payment_token);
        token_client.transfer(&buyer, &env.current_contract_address(), &listing.current_price);

        // Distribute payments
        // 1. Pay seller (after fees and royalties)
        token_client.transfer(&env.current_contract_address(), &listing.seller, &seller_amount);

        // 2. Pay marketplace fee
        if fee_amount > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &config.fee_recipient,
                &fee_amount,
            );
        }

        // 3. Pay royalty to creator
        if royalty_amount > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &listing.creator,
                &royalty_amount,
            );
        }

        // Update hint statistics
        let mut hint: Hint = env
            .storage()
            .instance()
            .get(&DataKey::Hint(listing.hint_id))
            .expect("Hint not found");
        hint.total_sales += 1;
        env.storage()
            .instance()
            .set(&DataKey::Hint(listing.hint_id), &hint);

        // Update demand metrics
        Self::update_demand_metrics(&env, listing.hint_id, listing.created_time, now);

        // Update listing status
        listing.status = ListingStatus::Sold;
        env.storage()
            .instance()
            .set(&DataKey::Listing(listing_id), &listing);

        // Remove from active listings
        Self::remove_from_active_listings(&env, listing_id);

        // Record price in history
        Self::record_price_history(&env, listing.hint_id, listing.current_price);
    }

    /// Rate a hint after purchase
    pub fn rate_hint(
        env: Env,
        rater: Address,
        hint_id: u64,
        quality_rating: u32,
        helpfulness: u32,
    ) {
        rater.require_auth();

        // Verify hint exists
        let mut hint: Hint = env
            .storage()
            .instance()
            .get(&DataKey::Hint(hint_id))
            .expect("Hint not found");

        // Verify rating values
        if quality_rating < 1 || quality_rating > 5 || helpfulness < 1 || helpfulness > 5 {
            panic!("Rating must be between 1 and 5");
        }

        // Check if already rated
        if env
            .storage()
            .instance()
            .has(&DataKey::Rating(hint_id, rater.clone()))
        {
            panic!("Already rated this hint");
        }

        let now = env.ledger().timestamp();

        let rating = Rating {
            rater: rater.clone(),
            hint_id,
            quality_rating,
            helpfulness,
            timestamp: now,
        };

        // Save rating
        env.storage()
            .instance()
            .set(&DataKey::Rating(hint_id, rater.clone()), &rating);

        // Update ratings index
        let mut ratings = Self::get_ratings_by_hint(&env, hint_id);
        ratings.push_back(rater.clone());
        env.storage()
            .instance()
            .set(&DataKey::RatingsByHint(hint_id), &ratings);

        // Update hint statistics
        hint.total_rating += quality_rating as u64;
        hint.rating_count += 1;

        // Update hint quality based on average rating
        let avg_rating = hint.total_rating / hint.rating_count as u64;
        hint.quality = match avg_rating {
            1 => HintQuality::Poor,
            2 => HintQuality::Fair,
            3 => HintQuality::Good,
            4 => HintQuality::Excellent,
            5 => HintQuality::Perfect,
            _ => HintQuality::Good,
        };

        env.storage()
            .instance()
            .set(&DataKey::Hint(hint_id), &hint);
    }

    /// Create a hint pack bundle
    pub fn create_pack(
        env: Env,
        creator: Address,
        name: Symbol,
        hint_ids: Vec<u64>,
        pack_price: i128,
        discount_bps: u32,
        expiration_time: Option<u64>,
    ) -> u64 {
        creator.require_auth();

        if hint_ids.is_empty() {
            panic!("Pack must contain at least one hint");
        }

        if discount_bps > 10000 {
            panic!("Discount cannot exceed 100%");
        }

        // Verify all hints exist and belong to creator
        for hint_id in hint_ids.iter() {
            let hint: Hint = env
                .storage()
                .instance()
                .get(&DataKey::Hint(hint_id))
                .expect("Hint not found");
            if hint.creator != creator {
                panic!("Not all hints belong to creator");
            }
        }

        // Generate pack ID
        let mut pack_id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::PackCounter)
            .unwrap_or(0);
        pack_id += 1;
        env.storage().instance().set(&DataKey::PackCounter, &pack_id);

        let now = env.ledger().timestamp();

        let pack = HintPack {
            pack_id,
            name,
            hint_ids: hint_ids.clone(),
            pack_price,
            discount_bps,
            creator: creator.clone(),
            created_at: now,
            expiration_time,
        };

        // Save pack
        env.storage().instance().set(&DataKey::Pack(pack_id), &pack);

        // Update indexes
        let mut creator_packs = Self::get_packs_by_creator(&env, &creator);
        creator_packs.push_back(pack_id);
        env.storage()
            .instance()
            .set(&DataKey::PacksByCreator(creator), &creator_packs);

        let mut active_packs = Self::get_active_packs(&env);
        active_packs.push_back(pack_id);
        env.storage()
            .instance()
            .set(&DataKey::ActivePacks, &active_packs);

        pack_id
    }

    /// Buy a hint pack
    pub fn buy_pack(env: Env, buyer: Address, pack_id: u64, payment_token: Address) {
        buyer.require_auth();

        let pack: HintPack = env
            .storage()
            .instance()
            .get(&DataKey::Pack(pack_id))
            .expect("Pack not found");

        // Check expiration
        if let Some(exp_time) = pack.expiration_time {
            let now = env.ledger().timestamp();
            if now > exp_time {
                panic!("Pack has expired");
            }
        }

        let config: MarketplaceConfig = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .expect("Not initialized");

        // Calculate fees and royalties
        let (creator_amount, fee_amount, _royalty_amount) = Self::calculate_payouts(
            &env,
            pack.pack_price,
            config.fee_bps,
            0, // No royalty for packs
        );

        // Transfer payment from buyer to contract
        let token_client = token::Client::new(&env, &payment_token);
        token_client.transfer(&buyer, &env.current_contract_address(), &pack.pack_price);

        // Distribute payments
        token_client.transfer(&env.current_contract_address(), &pack.creator, &creator_amount);

        if fee_amount > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &config.fee_recipient,
                &fee_amount,
            );
        }

        // Update hint statistics for all hints in pack
        for hint_id in pack.hint_ids.iter() {
            let mut hint: Hint = env
                .storage()
                .instance()
                .get(&DataKey::Hint(hint_id))
                .expect("Hint not found");
            hint.total_sales += 1;
            env.storage()
                .instance()
                .set(&DataKey::Hint(hint_id), &hint);
        }
    }

    /// Cancel a listing
    pub fn cancel_listing(env: Env, seller: Address, listing_id: u64) {
        seller.require_auth();

        let mut listing: HintListing = env
            .storage()
            .instance()
            .get(&DataKey::Listing(listing_id))
            .expect("Listing not found");

        if listing.seller != seller {
            panic!("Not the listing seller");
        }

        if listing.status != ListingStatus::Active {
            panic!("Listing is not active");
        }

        // Update listing status
        listing.status = ListingStatus::Cancelled;
        env.storage()
            .instance()
            .set(&DataKey::Listing(listing_id), &listing);

        // Remove from active listings
        Self::remove_from_active_listings(&env, listing_id);
    }

    /// Check and expire old listings (can be called by anyone)
    pub fn expire_listings(env: Env, listing_ids: Vec<u64>) {
        let now = env.ledger().timestamp();

        for listing_id in listing_ids.iter() {
            if let Some(mut listing) = env
                .storage()
                .instance()
                .get::<DataKey, HintListing>(&DataKey::Listing(listing_id))
            {
                if listing.status == ListingStatus::Active && now > listing.expiration_time {
                    listing.status = ListingStatus::Expired;
                    env.storage()
                        .instance()
                        .set(&DataKey::Listing(listing_id), &listing);
                    Self::remove_from_active_listings(&env, listing_id);
                }
            }
        }
    }

    // ──────────────────────────────────────────────────────────
    // HELPER FUNCTIONS
    // ──────────────────────────────────────────────────────────

    /// Calculate dynamic price based on demand and quality
    fn calculate_dynamic_price(env: &Env, hint_id: u64, base_price: i128) -> i128 {
        let config: MarketplaceConfig = env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .expect("Not initialized");

        // Get demand metrics
        let metrics: DemandMetrics = env
            .storage()
            .instance()
            .get(&DataKey::DemandMetrics(hint_id))
            .unwrap_or(DemandMetrics {
                hint_id,
                views: 0,
                purchases: 0,
                last_purchase_time: 0,
                average_time_to_sale: 0,
            });

        // Get hint quality
        let hint: Hint = env
            .storage()
            .instance()
            .get(&DataKey::Hint(hint_id))
            .expect("Hint not found");

        // Base price adjustment based on quality
        let quality_multiplier = match hint.quality {
            HintQuality::Poor => 5000,   // 50%
            HintQuality::Fair => 7500,    // 75%
            HintQuality::Good => 10000,   // 100%
            HintQuality::Excellent => 12500, // 125%
            HintQuality::Perfect => 15000,   // 150%
        };

        let quality_adjusted_price = (base_price * quality_multiplier as i128) / 10000;

        // Demand-based adjustment
        let demand_factor = if metrics.purchases > 0 {
            // Higher demand = higher price
            let purchase_rate = (metrics.purchases as i128 * 10000)
                / (metrics.views as i128 + metrics.purchases as i128).max(1);
            // Adjust by up to 20% based on purchase rate
            10000 + (purchase_rate * config.price_adjustment_factor as i128) / 10000
        } else {
            10000 // No adjustment for new hints
        };

        let final_price = (quality_adjusted_price * demand_factor) / 10000;

        // Ensure price is at least base_price * 0.5 and at most base_price * 3.0
        let min_price = base_price / 2;
        let max_price = base_price * 3;
        final_price.max(min_price).min(max_price)
    }

    /// Calculate payouts (seller amount, fee amount, royalty amount)
    fn calculate_payouts(
        _env: &Env,
        price: i128,
        fee_bps: u32,
        royalty_bps: u32,
    ) -> (i128, i128, i128) {
        let fee_amount = (price * fee_bps as i128) / 10000;
        let royalty_amount = (price * royalty_bps as i128) / 10000;
        let seller_amount = price - fee_amount - royalty_amount;

        (seller_amount, fee_amount, royalty_amount)
    }

    /// Update demand metrics after a purchase
    fn update_demand_metrics(env: &Env, hint_id: u64, listing_created: u64, purchase_time: u64) {
        let mut metrics: DemandMetrics = env
            .storage()
            .instance()
            .get(&DataKey::DemandMetrics(hint_id))
            .unwrap_or(DemandMetrics {
                hint_id,
                views: 0,
                purchases: 0,
                last_purchase_time: 0,
                average_time_to_sale: 0,
            });

        metrics.purchases += 1;
        metrics.last_purchase_time = purchase_time;

        // Update average time to sale
        let time_to_sale = purchase_time - listing_created;
        if metrics.purchases == 1 {
            metrics.average_time_to_sale = time_to_sale;
        } else {
            metrics.average_time_to_sale =
                (metrics.average_time_to_sale * (metrics.purchases - 1) as u64 + time_to_sale)
                    / metrics.purchases as u64;
        }

        env.storage()
            .instance()
            .set(&DataKey::DemandMetrics(hint_id), &metrics);
    }

    /// Record price in history
    fn record_price_history(env: &Env, hint_id: u64, price: i128) {
        let mut history: Vec<i128> = env
            .storage()
            .instance()
            .get(&DataKey::PriceHistory(hint_id))
            .unwrap_or(Vec::new(env));

        history.push_back(price);

        // Keep only last 100 prices
        if history.len() > 100 {
            let mut new_history = Vec::new(env);
            let start_index = history.len() - 100;
            for i in start_index..history.len() {
                new_history.push_back(*history.get(i).unwrap());
            }
            history = new_history;
        }

        env.storage()
            .instance()
            .set(&DataKey::PriceHistory(hint_id), &history);
    }

    /// Remove listing from active listings
    fn remove_from_active_listings(env: &Env, listing_id: u64) {
        let mut active_listings = Self::get_active_listings(env);
        if let Some(index) = active_listings.first_index_of(listing_id) {
            active_listings.remove(index);
            env.storage()
                .instance()
                .set(&DataKey::ActiveListings, &active_listings);
        }
    }

    // ──────────────────────────────────────────────────────────
    // GETTER FUNCTIONS
    // ──────────────────────────────────────────────────────────

    /// Get hint details
    pub fn get_hint(env: Env, hint_id: u64) -> Option<Hint> {
        env.storage().instance().get(&DataKey::Hint(hint_id))
    }

    /// Get listing details
    pub fn get_listing(env: Env, listing_id: u64) -> Option<HintListing> {
        env.storage().instance().get(&DataKey::Listing(listing_id))
    }

    /// Get pack details
    pub fn get_pack(env: Env, pack_id: u64) -> Option<HintPack> {
        env.storage().instance().get(&DataKey::Pack(pack_id))
    }

    /// Get rating for a hint by a specific rater
    pub fn get_rating(env: Env, hint_id: u64, rater: Address) -> Option<Rating> {
        env.storage().instance().get(&DataKey::Rating(hint_id, rater))
    }

    /// Get all listings for a hint
    pub fn get_listings_by_hint(env: &Env, hint_id: u64) -> Vec<u64> {
        env.storage()
            .instance()
            .get(&DataKey::ListingsByHint(hint_id))
            .unwrap_or(Vec::new(env))
    }

    /// Get all listings by seller
    pub fn get_listings_by_seller(env: &Env, seller: &Address) -> Vec<u64> {
        env.storage()
            .instance()
            .get(&DataKey::ListingsBySeller(seller.clone()))
            .unwrap_or(Vec::new(env))
    }

    /// Get all listings for a puzzle
    pub fn get_listings_by_puzzle(env: &Env, puzzle_id: u32) -> Vec<u64> {
        env.storage()
            .instance()
            .get(&DataKey::ListingsByPuzzle(puzzle_id))
            .unwrap_or(Vec::new(env))
    }

    /// Get all active listings
    pub fn get_active_listings(env: &Env) -> Vec<u64> {
        env.storage()
            .instance()
            .get(&DataKey::ActiveListings)
            .unwrap_or(Vec::new(env))
    }

    /// Get all ratings for a hint
    pub fn get_ratings_by_hint(env: &Env, hint_id: u64) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::RatingsByHint(hint_id))
            .unwrap_or(Vec::new(env))
    }

    /// Get price history for a hint
    pub fn get_price_history(env: Env, hint_id: u64) -> Vec<i128> {
        env.storage()
            .instance()
            .get(&DataKey::PriceHistory(hint_id))
            .unwrap_or(Vec::new(&env))
    }

    /// Get demand metrics for a hint
    pub fn get_demand_metrics(env: Env, hint_id: u64) -> Option<DemandMetrics> {
        env.storage().instance().get(&DataKey::DemandMetrics(hint_id))
    }

    /// Get all packs by creator
    pub fn get_packs_by_creator(env: &Env, creator: &Address) -> Vec<u64> {
        env.storage()
            .instance()
            .get(&DataKey::PacksByCreator(creator.clone()))
            .unwrap_or(Vec::new(env))
    }

    /// Get all active packs
    pub fn get_active_packs(env: &Env) -> Vec<u64> {
        env.storage()
            .instance()
            .get(&DataKey::ActivePacks)
            .unwrap_or(Vec::new(env))
    }

    /// Get marketplace configuration
    pub fn get_config(env: Env) -> MarketplaceConfig {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .expect("Not initialized")
    }
}

#[cfg(test)]
mod test;
