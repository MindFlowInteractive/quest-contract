#![no_std]

mod storage;
mod types;

use soroban_sdk::{
    contract, contractimpl, xdr::ToXdr, Address, BytesN, Env, Map, Symbol, Vec,
};


use storage::Storage;
use types::{
    AssetSourceConfig, CachedPrice, Config, EmergencyConfig, IntegrationError, OracleSource,
    PriceSnapshot, SignedPayload,
};

#[contract]
pub struct OracleIntegration;

#[contractimpl]
impl OracleIntegration {
    pub fn initialize(
        env: Env,
        admin: Address,
        signers: Vec<BytesN<32>>,
        threshold: u32,
        stale_threshold_secs: u64,
        max_deviation_bps: u32,
        cache_ttl_secs: u64,
    ) -> Result<(), IntegrationError> {
        if Storage::has_config(&env) {
            return Err(IntegrationError::AlreadyInitialized);
        }

        let mut signers_map: Map<BytesN<32>, bool> = Map::new(&env);
        for signer in signers.iter() {
            signers_map.set(signer, true);
        }

        let cfg = Config {
            admin,
            signers: signers_map,
            threshold,
            paused: false,
            stale_threshold_secs,
            max_deviation_bps,
            cache_ttl_secs,
        };
        Storage::set_config(&env, &cfg);

        Storage::set_emergency(
            &env,
            &EmergencyConfig {
                active: false,
                price: 0,
                timestamp: 0,
                round_id: 0,
            },
        );

        Ok(())
    }

    pub fn pause(env: Env) -> Result<(), IntegrationError> {
        let mut cfg = Storage::get_config(&env)?;
        cfg.admin.require_auth();
        cfg.paused = true;
        Storage::set_config(&env, &cfg);
        Ok(())
    }

    pub fn unpause(env: Env) -> Result<(), IntegrationError> {
        let mut cfg = Storage::get_config(&env)?;
        cfg.admin.require_auth();
        cfg.paused = false;
        Storage::set_config(&env, &cfg);
        Ok(())
    }

    pub fn set_asset_sources(
        env: Env,
        asset: Symbol,
        sources: Vec<OracleSource>,
    ) -> Result<(), IntegrationError> {
        let cfg = Storage::get_config(&env)?;
        cfg.admin.require_auth();
        Storage::set_asset_sources(&env, &asset, &AssetSourceConfig { sources });
        Ok(())
    }

    pub fn set_emergency_price(
        env: Env,
        price: i128,
        timestamp: u64,
        round_id: u64,
    ) -> Result<(), IntegrationError> {
        let cfg = Storage::get_config(&env)?;
        cfg.admin.require_auth();
        if price <= 0 {
            return Err(IntegrationError::InvalidPrice);
        }

        Storage::set_emergency(
            &env,
            &EmergencyConfig {
                active: true,
                price,
                timestamp,
                round_id,
            },
        );
        Ok(())
    }

    pub fn activate_emergency(env: Env) -> Result<(), IntegrationError> {
        let cfg = Storage::get_config(&env)?;
        cfg.admin.require_auth();
        let mut e = Storage::get_emergency(&env);
        e.active = true;
        Storage::set_emergency(&env, &e);
        Ok(())
    }

    pub fn deactivate_emergency(env: Env) -> Result<(), IntegrationError> {
        let cfg = Storage::get_config(&env)?;
        cfg.admin.require_auth();
        let mut e = Storage::get_emergency(&env);
        e.active = false;
        Storage::set_emergency(&env, &e);
        Ok(())
    }

    /// Direct signed submission to cache (off-chain signature verification requirement).
    pub fn submit_signed_price(
        env: Env,
        asset: Symbol,
        price: i128,
        timestamp: u64,
        round_id: u64,
        signatures: Vec<(BytesN<32>, BytesN<64>)>,
    ) -> Result<(), IntegrationError> {
        let cfg = Storage::get_config(&env)?;
        if cfg.paused {
            return Err(IntegrationError::Paused);
        }
        if price <= 0 {
            return Err(IntegrationError::InvalidPrice);
        }

        let payload = SignedPayload {
            asset: asset.clone(),
            price,
            timestamp,
            round_id,
            contract_address: env.current_contract_address(),
        };
        let payload_bytes = payload.to_xdr(&env);

        let mut valid = 0u32;
        let mut used: Map<BytesN<32>, bool> = Map::new(&env);

        for (pub_key, sig) in signatures.iter() {
            if !cfg.signers.contains_key(pub_key.clone()) {
                continue;
            }
            if used.contains_key(pub_key.clone()) {
                continue;
            }

            // NOTE: ed25519_verify returns () but will trap on invalid? We rely on contract behavior.
            env.crypto().ed25519_verify(&pub_key, &payload_bytes, &sig);
            used.set(pub_key, true);
            valid += 1;
        }

        if valid < cfg.threshold {
            return Err(IntegrationError::InsufficientSignatures);
        }

        let now = env.ledger().timestamp();
        let expires_at = now + cfg.cache_ttl_secs;

        let snapshot = PriceSnapshot {
            price,
            timestamp,
            round_id,
        };

        let cached = CachedPrice {
            price: snapshot.price,
            timestamp: snapshot.timestamp,
            round_id: snapshot.round_id,
            expires_at,
        };
        Storage::set_cached_price(&env, &asset, &cached);
        Ok(())
    }

    pub fn get_price(env: Env, asset: Symbol) -> Result<PriceSnapshot, IntegrationError> {
        if Storage::get_emergency(&env).active {
            let e = Storage::get_emergency(&env);
            return Ok(PriceSnapshot {
                price: e.price,
                timestamp: e.timestamp,
                round_id: e.round_id,
            });
        }

        let cfg = Storage::get_config(&env)?;
        if cfg.paused {
            return Err(IntegrationError::Paused);
        }

        let now = env.ledger().timestamp();
        if Storage::has_cached_price(&env, &asset) {
            let cached = Storage::get_cached_price_entry(&env, &asset).unwrap();
            if now <= cached.expires_at {
                // cache-level freshness; also ensure source timestamp isn't stale
                Self::self_validate_stale(&env, &cfg, cached.timestamp)?;

                return Ok(PriceSnapshot {
                    price: cached.price,
                    timestamp: cached.timestamp,
                    round_id: cached.round_id,
                });
            }
        }

        // Refresh via sources (best-effort: aggregate across all valid sources).
        let sources_cfg = Storage::get_asset_sources(&env, &asset)?;
        let mut valid_prices: Vec<i128> = Vec::new(&env);
        let mut latest_snapshot: Option<PriceSnapshot> = None;

        for src in sources_cfg.sources.iter() {
            if let Some(snap) = Self::self_try_fetch_validate(&env, &cfg, &src, &asset) {

                valid_prices.push_back(snap.price);
                latest_snapshot = Some(snap);
            }
        }

        if valid_prices.len() == 0 {
            return Err(IntegrationError::InsufficientValidSources);
        }

        let agg = Self::self_median(&env, &valid_prices);


        // timestamp/round_id: take from most recent successful snapshot
        let snap = latest_snapshot.unwrap();
        Ok(PriceSnapshot {
            price: agg,
            timestamp: snap.timestamp,
            round_id: snap.round_id,
        })
    }

    fn self_try_fetch_validate(
        env: &Env,
        cfg: &Config,
        src: &OracleSource,
        asset: &Symbol,
    ) -> Option<PriceSnapshot> {
        // For median feed we use pair_id; for signed ed25519 we use asset.
        let _res: Result<PriceSnapshot, IntegrationError> = match src {


            OracleSource::SignedEd25519(oracle_contract, src_asset) => {

                if src_asset != asset {
                    return None;
                }


                // NOTE: In Soroban, cross-contract calls require a contract client.
                // This placeholder implementation treats the source as unavailable.
                Err(IntegrationError::SourceFetchFailed)

            }
            OracleSource::MedianFeed(_, _) => {

                // NOTE: Cross-contract calls require a contract client.
                // Placeholder: treat source as unavailable.
                Err(IntegrationError::SourceFetchFailed)
            }


        };

        match _res {

            Ok(snap) => {

                if snap.price <= 0 {
                    return None;
                }
                // staleness based on fetched timestamp
                if Self::self_validate_stale(env, cfg, snap.timestamp).is_err() {

                    return None;
                }

                // deviation check vs cached previous (if exists)

                if Storage::has_cached_price(env, asset) {
                    let cached = Storage::get_cached_price_entry(env, asset).unwrap();
                    if cached.price > 0 {
                        let diff = (snap.price - cached.price).abs();
                        let allowed = (cached.price.abs() as i128)
                            * (cfg.max_deviation_bps as i128)
                            / 10_000i128;
                        if diff > allowed {
                            return None;
                        }
                    }
                }

                Some(snap)
            }
            Err(_) => None,
        }
    }

    fn self_validate_stale(
        env: &Env,
        cfg: &Config,
        source_timestamp: u64,
    ) -> Result<(), IntegrationError> {
        let now = env.ledger().timestamp();
        if now > source_timestamp + cfg.stale_threshold_secs {
            return Err(IntegrationError::SourceStale);
        }
        Ok(())
    }

    fn self_median(env: &Env, prices: &Vec<i128>) -> i128 {
        let mut v: Vec<i128> = Vec::new(env);
        for p in prices.iter() {
            v.push_back(p);
        }
        let len = v.len();
        // simple insertion sort
        for i in 1..len {
            let mut j = i;
            while j > 0 {
                let a = v.get(j - 1).unwrap();
                let b = v.get(j).unwrap();
                if a <= b {
                    break;
                }
                v.set(j - 1, b);
                v.set(j, a);
                j -= 1;
            }
        }

        if len % 2 == 1 {
            v.get(len / 2).unwrap()
        } else {
            let mid1 = v.get(len / 2 - 1).unwrap();
            let mid2 = v.get(len / 2).unwrap();
            (mid1 + mid2) / 2
        }
    }
}

mod test;
