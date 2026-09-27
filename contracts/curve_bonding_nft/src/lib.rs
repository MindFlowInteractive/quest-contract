#![no_std]

//! Curve-bonding NFT issuance.
//!
//! Buyers purchase sequentially numbered NFTs from a linear bonding curve:
//!
//! ```text
//! price(supply) = base_price + slope * supply
//! ```
//!
//! Every purchase is split between a **reserve** (backs the holders and funds
//! exit liquidity) and a **treasury** according to `reserve_bps`. Holders can
//! burn their NFT at any time to claim their pro-rata share of the reserve
//! (`reserve / supply`), which is the exit-liquidity mechanism. Every mint
//! records its `(supply_before, price)` in a bounded price-history ring, and
//! both entrypoints carry slippage guards (`max_price` on buy,
//! `min_proceeds` on exit).

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Vec};

/// Basis-point denominator for the reserve split.
pub const BPS: i128 = 10_000;

/// Maximum number of price-history points retained. Once the ring is full
/// the oldest point is dropped, keeping the stored history bounded.
pub const MAX_HISTORY: u32 = 256;

/// Curve parameters set at initialization.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurveParams {
    /// Price of the first mint (must be > 0).
    pub base_price: i128,
    /// Linear slope added per minted unit (must be >= 0).
    pub slope: i128,
    /// Share of every purchase routed to the reserve, in basis points.
    pub reserve_bps: u32,
    /// Hard cap on the number of NFTs that can ever be minted.
    pub max_supply: u32,
}

/// One recorded point of the price history.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PricePoint {
    /// Circulating supply immediately before the mint that set this price.
    pub supply_before: u32,
    /// Price paid for that mint.
    pub price: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Nft {
    pub token_id: u32,
    pub owner: Address,
    pub minted_price: i128,
}

#[contracttype]
pub enum DataKey {
    Params,
    Treasury,
    PaymentToken,
    Supply,
    Reserve,
    NextId,
    Owner(u32),
    Nft(u32),
    History,
}

// ── Storage helpers (borrow &Env so entrypoints keep ownership of `env`) ────

fn load_params(env: &Env) -> CurveParams {
    env.storage()
        .instance()
        .get(&DataKey::Params)
        .expect("not initialized")
}

fn load_supply(env: &Env) -> u32 {
    env.storage().instance().get(&DataKey::Supply).unwrap_or(0)
}

fn load_reserve(env: &Env) -> i128 {
    env.storage().instance().get(&DataKey::Reserve).unwrap_or(0)
}

fn load_next_id(env: &Env) -> u32 {
    env.storage().instance().get(&DataKey::NextId).unwrap_or(1)
}

fn load_payment_token(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::PaymentToken)
        .expect("not initialized")
}

fn load_treasury(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Treasury)
        .expect("not initialized")
}

fn load_owner(env: &Env, token_id: u32) -> Address {
    env.storage()
        .persistent()
        .get(&DataKey::Owner(token_id))
        .expect("token does not exist")
}

fn load_nft(env: &Env, token_id: u32) -> Nft {
    env.storage()
        .persistent()
        .get(&DataKey::Nft(token_id))
        .expect("token does not exist")
}

fn load_history(env: &Env) -> Vec<PricePoint> {
    env.storage()
        .instance()
        .get(&DataKey::History)
        .unwrap_or_else(|| Vec::new(env))
}

/// Linear bonding-curve formula: the price of the next unit given
/// circulating supply.
pub fn price_at(params: &CurveParams, supply: u32) -> i128 {
    params.base_price + params.slope * supply as i128
}

#[contract]
pub struct CurveBondingNFT;

#[contractimpl]
impl CurveBondingNFT {
    /// One-time setup of the curve, payment token, and treasury.
    pub fn initialize(
        env: Env,
        payment_token: Address,
        treasury: Address,
        base_price: i128,
        slope: i128,
        reserve_bps: u32,
        max_supply: u32,
    ) {
        if env.storage().instance().has(&DataKey::Params) {
            panic!("already initialized");
        }
        if base_price <= 0 {
            panic!("base_price must be positive");
        }
        if slope < 0 {
            panic!("slope must be non-negative");
        }
        if reserve_bps > BPS as u32 {
            panic!("reserve_bps cannot exceed 10000");
        }
        if max_supply == 0 {
            panic!("max_supply must be positive");
        }

        env.storage().instance().set(
            &DataKey::Params,
            &CurveParams {
                base_price,
                slope,
                reserve_bps,
                max_supply,
            },
        );
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        env.storage()
            .instance()
            .set(&DataKey::PaymentToken, &payment_token);
        env.storage().instance().set(&DataKey::Supply, &0u32);
        env.storage().instance().set(&DataKey::Reserve, &0i128);
        env.storage().instance().set(&DataKey::NextId, &1u32);
        env.storage()
            .instance()
            .set(&DataKey::History, &Vec::<PricePoint>::new(&env));
    }

    // ── Buying ──────────────────────────────────────────────────────────────

    /// Mints the next NFT to `buyer` at the current curve price.
    ///
    /// Slippage guard: aborts if the price would exceed `max_price`.
    pub fn buy(env: Env, buyer: Address, max_price: i128) -> u32 {
        buyer.require_auth();
        let params = load_params(&env);
        let supply = load_supply(&env);
        if supply >= params.max_supply {
            panic!("sold out");
        }

        let price = price_at(&params, supply);
        if price > max_price {
            panic!("price exceeds slippage limit");
        }

        let token_client = token::TokenClient::new(&env, &load_payment_token(&env));
        token_client.transfer(&buyer, &env.current_contract_address(), &price);

        // Split the payment: the reserve backs holders, the rest goes to
        // the treasury immediately.
        let reserve_part = price * params.reserve_bps as i128 / BPS;
        let treasury_part = price - reserve_part;
        let mut reserve = load_reserve(&env);
        reserve += reserve_part;
        env.storage().instance().set(&DataKey::Reserve, &reserve);
        if treasury_part > 0 {
            let treasury = load_treasury(&env);
            token_client.transfer(&env.current_contract_address(), &treasury, &treasury_part);
        }

        // Mint.
        let token_id = load_next_id(&env);
        env.storage()
            .instance()
            .set(&DataKey::NextId, &(token_id + 1));
        env.storage()
            .instance()
            .set(&DataKey::Supply, &(supply + 1));
        env.storage()
            .persistent()
            .set(&DataKey::Owner(token_id), &buyer);
        env.storage().persistent().set(
            &DataKey::Nft(token_id),
            &Nft {
                token_id,
                owner: buyer,
                minted_price: price,
            },
        );

        // Price history: bounded ring, oldest point dropped when full.
        let mut history = load_history(&env);
        history.push_back(PricePoint {
            supply_before: supply,
            price,
        });
        if history.len() > MAX_HISTORY {
            history.pop_front();
        }
        env.storage().instance().set(&DataKey::History, &history);

        token_id
    }

    // ── Exiting ─────────────────────────────────────────────────────────────

    /// Burns `token_id` and pays its owner the pro-rata reserve share
    /// (`reserve / supply`) — the exit-liquidity path.
    ///
    /// Slippage guard: aborts if the payout would be below `min_proceeds`.
    pub fn burn_and_exit(env: Env, token_id: u32, min_proceeds: i128) -> i128 {
        let owner = load_owner(&env, token_id);
        owner.require_auth();

        let supply = load_supply(&env);
        let reserve = load_reserve(&env);
        let proceeds = reserve / supply as i128;
        if proceeds < min_proceeds {
            panic!("proceeds below slippage limit");
        }

        env.storage().persistent().remove(&DataKey::Owner(token_id));
        env.storage().persistent().remove(&DataKey::Nft(token_id));
        env.storage()
            .instance()
            .set(&DataKey::Supply, &(supply - 1));
        env.storage()
            .instance()
            .set(&DataKey::Reserve, &(reserve - proceeds));

        let payment_token = load_payment_token(&env);
        token::TokenClient::new(&env, &payment_token).transfer(
            &env.current_contract_address(),
            &owner,
            &proceeds,
        );
        proceeds
    }

    /// Plain burn for the current owner: no proceeds, the forfeited reserve
    /// share stays behind for the remaining holders.
    pub fn burn(env: Env, token_id: u32) {
        let owner = load_owner(&env, token_id);
        owner.require_auth();
        let supply = load_supply(&env);
        env.storage().persistent().remove(&DataKey::Owner(token_id));
        env.storage().persistent().remove(&DataKey::Nft(token_id));
        env.storage()
            .instance()
            .set(&DataKey::Supply, &(supply - 1));
    }

    // ── NFT transfers ───────────────────────────────────────────────────────

    pub fn transfer(env: Env, from: Address, to: Address, token_id: u32) {
        from.require_auth();
        let owner = load_owner(&env, token_id);
        if owner != from {
            panic!("not the token owner");
        }
        let mut nft = load_nft(&env, token_id);
        nft.owner = to.clone();
        env.storage()
            .persistent()
            .set(&DataKey::Owner(token_id), &to);
        env.storage()
            .persistent()
            .set(&DataKey::Nft(token_id), &nft);
    }

    // ── Views ───────────────────────────────────────────────────────────────

    pub fn params(env: Env) -> CurveParams {
        load_params(&env)
    }

    /// Price of the next mint: `base_price + slope * supply`.
    pub fn price(env: Env) -> i128 {
        price_at(&load_params(&env), load_supply(&env))
    }

    /// Price the curve would charge at an arbitrary supply level.
    pub fn quote(env: Env, supply: u32) -> i128 {
        price_at(&load_params(&env), supply)
    }

    pub fn supply(env: Env) -> u32 {
        load_supply(&env)
    }

    pub fn reserve(env: Env) -> i128 {
        load_reserve(&env)
    }

    pub fn next_token_id(env: Env) -> u32 {
        load_next_id(&env)
    }

    pub fn treasury(env: Env) -> Address {
        load_treasury(&env)
    }

    pub fn payment_token(env: Env) -> Address {
        load_payment_token(&env)
    }

    pub fn owner_of(env: Env, token_id: u32) -> Address {
        load_owner(&env, token_id)
    }

    pub fn nft(env: Env, token_id: u32) -> Nft {
        load_nft(&env, token_id)
    }

    pub fn price_history_len(env: Env) -> u32 {
        load_history(&env).len()
    }

    pub fn price_history(env: Env, index: u32) -> PricePoint {
        load_history(&env)
            .get(index)
            .expect("price history index out of bounds")
    }
}

#[cfg(test)]
mod test;
