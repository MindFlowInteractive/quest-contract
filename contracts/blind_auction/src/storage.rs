use crate::types::{Auction, BlindAuctionError, Commitment, Config, GlobalStats};
use soroban_sdk::{symbol_short, Address, BytesN, Env, Symbol, Vec};

/// Storage layout for the blind auction contract.
///
/// * `instance()`  - config, the auction id counter and the global analytics.
/// * `persistent()` - one entry per auction, one bidder list per auction and
///   one commitment per `(auction, bidder)` pair.
pub struct Storage;

impl Storage {
    // ----- instance storage -------------------------------------------------

    pub fn has_config(env: &Env) -> bool {
        env.storage().instance().has(&symbol_short!("config"))
    }

    pub fn set_config(env: &Env, config: &Config) {
        env.storage()
            .instance()
            .set(&symbol_short!("config"), config);
    }

    pub fn get_config(env: &Env) -> Result<Config, BlindAuctionError> {
        env.storage()
            .instance()
            .get(&symbol_short!("config"))
            .ok_or(BlindAuctionError::NotInitialized)
    }

    pub fn get_auction_counter(env: &Env) -> u64 {
        env.storage()
            .instance()
            .get(&symbol_short!("auc_cnt"))
            .unwrap_or(0)
    }

    pub fn set_auction_counter(env: &Env, counter: &u64) {
        env.storage()
            .instance()
            .set(&symbol_short!("auc_cnt"), counter);
    }

    /// Returns a fresh, monotonically increasing auction id.
    pub fn next_auction_id(env: &Env) -> u64 {
        let next = Self::get_auction_counter(env) + 1;
        Self::set_auction_counter(env, &next);
        next
    }

    pub fn has_stats(env: &Env) -> bool {
        env.storage().instance().has(&symbol_short!("stats"))
    }

    pub fn set_stats(env: &Env, stats: &GlobalStats) {
        env.storage().instance().set(&symbol_short!("stats"), stats);
    }

    pub fn get_stats(env: &Env) -> Result<GlobalStats, BlindAuctionError> {
        env.storage()
            .instance()
            .get(&symbol_short!("stats"))
            .ok_or(BlindAuctionError::NotInitialized)
    }

    // ----- persistent storage: auctions ------------------------------------

    pub fn auction_key(auction_id: u64) -> (Symbol, u64) {
        (symbol_short!("auction"), auction_id)
    }

    pub fn set_auction(env: &Env, auction: &Auction) {
        env.storage()
            .persistent()
            .set(&Self::auction_key(auction.auction_id), auction);
    }

    pub fn get_auction(env: &Env, auction_id: u64) -> Result<Auction, BlindAuctionError> {
        env.storage()
            .persistent()
            .get(&Self::auction_key(auction_id))
            .ok_or(BlindAuctionError::AuctionNotFound)
    }

    // ----- persistent storage: bidder roster -------------------------------

    pub fn bidders_key(auction_id: u64) -> (Symbol, u64) {
        (symbol_short!("bidders"), auction_id)
    }

    pub fn init_bidders(env: &Env, auction_id: u64) {
        let empty: Vec<Address> = Vec::new(env);
        env.storage()
            .persistent()
            .set(&Self::bidders_key(auction_id), &empty);
    }

    /// Bidders in commit order. Iteration order is what makes the tie-break
    /// rule ("earliest commit wins") deterministic.
    pub fn get_bidders(env: &Env, auction_id: u64) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&Self::bidders_key(auction_id))
            .unwrap_or_else(|| Vec::new(env))
    }

    pub fn push_bidder(env: &Env, auction_id: u64, bidder: &Address) {
        let mut bidders = Self::get_bidders(env, auction_id);
        bidders.push_back(bidder.clone());
        env.storage()
            .persistent()
            .set(&Self::bidders_key(auction_id), &bidders);
    }

    // ----- persistent storage: commitments ---------------------------------

    pub fn commitment_key(auction_id: u64, bidder: &Address) -> (Symbol, u64, Address) {
        (symbol_short!("commit"), auction_id, bidder.clone())
    }

    pub fn has_commitment(env: &Env, auction_id: u64, bidder: &Address) -> bool {
        env.storage()
            .persistent()
            .has(&Self::commitment_key(auction_id, bidder))
    }

    pub fn set_commitment(env: &Env, auction_id: u64, bidder: &Address, record: &Commitment) {
        env.storage()
            .persistent()
            .set(&Self::commitment_key(auction_id, bidder), record);
    }

    pub fn get_commitment(
        env: &Env,
        auction_id: u64,
        bidder: &Address,
    ) -> Result<Commitment, BlindAuctionError> {
        env.storage()
            .persistent()
            .get(&Self::commitment_key(auction_id, bidder))
            .ok_or(BlindAuctionError::CommitmentMissing)
    }

    pub fn try_get_commitment(
        env: &Env,
        auction_id: u64,
        bidder: &Address,
    ) -> Option<Commitment> {
        env.storage()
            .persistent()
            .get(&Self::commitment_key(auction_id, bidder))
    }

    // ----- persistent storage: one live auction per NFT -------------------

    pub fn nft_key(nft_id: &BytesN<32>) -> (Symbol, BytesN<32>) {
        (symbol_short!("nft_auc"), nft_id.clone())
    }

    /// True while an auction for this NFT has neither been closed nor
    /// cancelled, which keeps a token from being listed twice.
    pub fn has_active_nft(env: &Env, nft_id: &BytesN<32>) -> bool {
        env.storage().persistent().has(&Self::nft_key(nft_id))
    }

    pub fn set_active_nft(env: &Env, nft_id: &BytesN<32>, auction_id: &u64) {
        env.storage()
            .persistent()
            .set(&Self::nft_key(nft_id), auction_id);
    }

    pub fn clear_active_nft(env: &Env, nft_id: &BytesN<32>) {
        env.storage().persistent().remove(&Self::nft_key(nft_id));
    }
}
