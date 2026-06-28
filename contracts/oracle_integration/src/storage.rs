use soroban_sdk::{symbol_short, Env, Symbol};

use crate::types::{AssetSourceConfig, CachedPrice, Config, EmergencyConfig};


pub struct Storage;

impl Storage {
    pub fn has_config(env: &Env) -> bool {
        env.storage().instance().has(&symbol_short!("config"))
    }

    pub fn get_config(env: &Env) -> Result<Config, crate::types::IntegrationError> {
        env.storage()
            .instance()
            .get(&symbol_short!("config"))
            .ok_or(crate::types::IntegrationError::NotInitialized)
    }

    pub fn set_config(env: &Env, config: &Config) {
        env.storage()
            .instance()
            .set(&symbol_short!("config"), config);
    }

    pub fn set_asset_sources(env: &Env, asset: &Symbol, cfg: &AssetSourceConfig) {
        env.storage().persistent().set(asset, cfg);
    }

    pub fn get_asset_sources(
        env: &Env,
        asset: &Symbol,
    ) -> Result<AssetSourceConfig, crate::types::IntegrationError> {
        env.storage()
            .persistent()
            .get(asset)
            .ok_or(crate::types::IntegrationError::SourceNotConfigured)
    }

    pub fn set_cached_price(env: &Env, asset: &Symbol, price: &CachedPrice) {
        // Use different key namespace
        let key = (symbol_short!("cache"), asset.clone());
        env.storage().persistent().set(&key, price);
    }

    pub fn get_cached_price(env: &Env, asset: &Symbol) -> Option<CachedPrice> {
        let key = (symbol_short!("cache"), asset.clone());
        env.storage().persistent().get(&key)
    }

    pub fn has_cached_price(env: &Env, asset: &Symbol) -> bool {
        let key = (symbol_short!("cache"), asset.clone());
        env.storage().persistent().has(&key)
    }

    pub fn get_cached_price_entry(env: &Env, asset: &Symbol) -> Option<CachedPrice> {
        let key = (symbol_short!("cache"), asset.clone());
        env.storage().persistent().get(&key)
    }

    pub fn set_emergency(env: &Env, cfg: &EmergencyConfig) {
        env.storage()
            .instance()
            .set(&symbol_short!("emergency"), cfg);
    }

    pub fn get_emergency(env: &Env) -> EmergencyConfig {
        env.storage()
            .instance()
            .get(&symbol_short!("emergency"))
            .unwrap_or(EmergencyConfig {
                active: false,
                price: 0,
                timestamp: 0,
                round_id: 0,
            })
    }
}
