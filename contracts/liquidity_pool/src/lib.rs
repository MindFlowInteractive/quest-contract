#![no_std]

mod storage;
mod events;
#[cfg(test)]
mod test;

use soroban_sdk::{
    contract, contractimpl, Address, Env, Symbol, String, panic_with_error,
    token::Client as TokenClient,
};
use crate::storage::*;
use crate::events::*;

#[contract]
pub struct LiquidityPoolContract;

#[contractimpl]
impl LiquidityPoolContract {
    /// Initialize the liquidity pool with two tokens and fee configuration
    pub fn initialize(
        env: Env,
        admin: Address,
        token_a: Address,
        token_b: Address,
        fee_bps: u32,
    ) {
        if env.storage().instance().has(&DataKey::Initialized) {
            panic!("Pool already initialized");
        }

        if fee_bps > 1000 {
            panic_with_error!(&env, 1); // Max 10% fee
        }

        // Ensure token addresses are ordered to prevent duplicate pools
        let (token0, token1) = if token_a < token_b {
            (token_a, token_b)
        } else {
            (token_b, token_a)
        };

        set_tokens(&env, &token0, &token1);
        set_admin(&env, &admin);
        set_fee_bps(&env, &fee_bps);
        set_fee_recipient(&env, &admin); // Default to admin initially
        env.storage().instance().set(&DataKey::Initialized, &true);

        emit_pool_created(&env, token0, token1, fee_bps);
    }

    /// Add liquidity to the pool
    pub fn add_liquidity(
        env: Env,
        provider: Address,
        amount_a_desired: i128,
        amount_b_desired: i128,
        amount_a_min: i128,
        amount_b_min: i128,
        to: Address,
    ) -> i128 {
        provider.require_auth();

        let (token0, token1) = get_tokens(&env);
        let mut reserves = get_reserves(&env);
        let total_supply = get_total_supply(&env);

        // Calculate optimal amounts
        let (amount_a, amount_b, liquidity) = if reserves.reserve_a == 0 && reserves.reserve_b == 0 {
            // First liquidity provision
            let amount_a = amount_a_desired;
            let amount_b = amount_b_desired;
            let liquidity = (amount_a as u128 * amount_b as u128).sqrt() as i128;
            (amount_a, amount_b, liquidity)
        } else {
            let amount_b_optimal = reserves.reserve_b as u128 * amount_a_desired as u128
                / reserves.reserve_a as u128;
            let amount_a_optimal = reserves.reserve_a as u128 * amount_b_desired as u128
                / reserves.reserve_b as u128;

            let (amount_a, amount_b) = if amount_b_optimal <= amount_b_desired as u128 {
                if amount_a_desired < amount_a_min {
                    panic_with_error!(&env, 2); // Insufficient amountA
                }
                (amount_a_desired, amount_b_optimal as i128)
            } else {
                if amount_b_desired < amount_b_min {
                    panic_with_error!(&env, 3); // Insufficient amountB
                }
                (amount_a_optimal as i128, amount_b_desired)
            };

            let liquidity = core::cmp::min(
                amount_a as u128 * total_supply as u128 / reserves.reserve_a as u128,
                amount_b as u128 * total_supply as u128 / reserves.reserve_b as u128,
            ) as i128;

            (amount_a, amount_b, liquidity)
        };

        if liquidity <= 0 {
            panic_with_error!(&env, 4); // Insufficient liquidity minted
        }

        // Transfer tokens from provider
        let client_a = TokenClient::new(&env, &token0);
        let client_b = TokenClient::new(&env, &token1);
        client_a.transfer(&provider, &env.current_contract_address(), &amount_a);
        client_b.transfer(&provider, &env.current_contract_address(), &amount_b);

        // Update reserves and mint LP tokens
        reserves.reserve_a += amount_a;
        reserves.reserve_b += amount_b;
        set_reserves(&env, &reserves);

        let new_total_supply = total_supply + liquidity;
        set_total_supply(&env, &new_total_supply);

        // Update user's LP balance
        let mut user_balance = get_balance(&env, &to);
        user_balance += liquidity;
        set_balance(&env, &to, &user_balance);

        emit_liquidity_added(&env, provider, to, amount_a, amount_b, liquidity);

        liquidity
    }

    /// Remove liquidity from the pool
    pub fn remove_liquidity(
        env: Env,
        owner: Address,
        liquidity: i128,
        amount_a_min: i128,
        amount_b_min: i128,
        to: Address,
    ) -> (i128, i128) {
        owner.require_auth();

        let (token0, token1) = get_tokens(&env);
        let mut reserves = get_reserves(&env);
        let total_supply = get_total_supply(&env);

        let mut user_balance = get_balance(&env, &owner);
        if user_balance < liquidity {
            panic_with_error!(&env, 5); // Insufficient LP balance
        }

        // Calculate amounts to return
        let amount_a = liquidity as u128 * reserves.reserve_a as u128 / total_supply as u128;
        let amount_b = liquidity as u128 * reserves.reserve_b as u128 / total_supply as u128;

        if amount_a < amount_a_min as u128 || amount_b < amount_b_min as u128 {
            panic_with_error!(&env, 6); // Slippage exceeded
        }

        // Update state
        user_balance -= liquidity;
        set_balance(&env, &owner, &user_balance);

        let new_total_supply = total_supply - liquidity;
        set_total_supply(&env, &new_total_supply);

        reserves.reserve_a -= amount_a as i128;
        reserves.reserve_b -= amount_b as i128;
        set_reserves(&env, &reserves);

        // Transfer tokens back to user
        let client_a = TokenClient::new(&env, &token0);
        let client_b = TokenClient::new(&env, &token1);
        client_a.transfer(&env.current_contract_address(), &to, &amount_a as i128);
        client_b.transfer(&env.current_contract_address(), &to, &amount_b as i128);

        emit_liquidity_removed(&env, owner, to, amount_a as i128, amount_b as i128, liquidity);

        (amount_a as i128, amount_b as i128)
    }

    /// Swap tokens
    pub fn swap(
        env: Env,
        swapper: Address,
        amount_in: i128,
        amount_out_min: i128,
        token_in: Address,
        to: Address,
    ) -> i128 {
        swapper.require_auth();

        let (token0, token1) = get_tokens(&env);
        let mut reserves = get_reserves(&env);
        let fee_bps = get_fee_bps(&env);

        // Determine which token is being swapped in
        let (reserve_in, reserve_out) = if token_in == token0 {
            (reserves.reserve_a, reserves.reserve_b)
        } else if token_in == token1 {
            (reserves.reserve_b, reserves.reserve_a)
        } else {
            panic_with_error!(&env, 7); // Invalid token
        };

        if amount_in <= 0 {
            panic_with_error!(&env, 8); // Invalid input amount
        }

        // Calculate amount out using constant product formula with fees
        let amount_in_with_fee = amount_in as u128 * (10000 - fee_bps) as u128;
        let numerator = amount_in_with_fee * reserve_out as u128;
        let denominator = reserve_in as u128 * 10000 + amount_in_with_fee;
        let amount_out = numerator / denominator;

        if amount_out < amount_out_min as u128 {
            panic_with_error!(&env, 9); // Insufficient output amount (slippage)
        }

        // Calculate fee amount
        let fee_amount = amount_in * fee_bps as i128 / 10000;

        // Update reserves
        if token_in == token0 {
            reserves.reserve_a += amount_in;
            reserves.reserve_b -= amount_out as i128;
            reserves.fees_a += fee_amount;
        } else {
            reserves.reserve_b += amount_in;
            reserves.reserve_a -= amount_out as i128;
            reserves.fees_b += fee_amount;
        }
        set_reserves(&env, &reserves);

        // Transfer tokens
        let client_in = TokenClient::new(&env, &token_in);
        client_in.transfer(&swapper, &env.current_contract_address(), &amount_in);

        let token_out = if token_in == token0 { token1 } else { token0 };
        let client_out = TokenClient::new(&env, &token_out);
        client_out.transfer(&env.current_contract_address(), &to, &amount_out as i128);

        // Update price oracle
        update_price_oracle(&env, &reserves);

        emit_swap(&env, swapper, to, token_in, amount_in, token_out, amount_out as i128, fee_amount);

        amount_out as i128
    }

    /// Collect accumulated fees
    pub fn collect_fees(env: Env, to: Address) -> (i128, i128) {
        let admin = get_admin(&env);
        let fee_recipient = get_fee_recipient(&env);
        if env.invoker() != admin && env.invoker() != fee_recipient {
            panic_with_error!(&env, 10); // Unauthorized
        }

        let (token0, token1) = get_tokens(&env);
        let mut reserves = get_reserves(&env);

        let fees_a = reserves.fees_a;
        let fees_b = reserves.fees_b;

        if fees_a > 0 {
            let client0 = TokenClient::new(&env, &token0);
            client0.transfer(&env.current_contract_address(), &to, &fees_a);
        }
        if fees_b > 0 {
            let client1 = TokenClient::new(&env, &token1);
            client1.transfer(&env.current_contract_address(), &to, &fees_b);
        }

        reserves.fees_a = 0;
        reserves.fees_b = 0;
        set_reserves(&env, &reserves);

        emit_fees_collected(&env, to, fees_a, fees_b);

        (fees_a, fees_b)
    }

    /// Update fee recipient (only admin)
    pub fn set_fee_recipient(env: Env, new_recipient: Address) {
        let admin = get_admin(&env);
        admin.require_auth();
        set_fee_recipient(&env, &new_recipient);
    }

    // View functions
    pub fn get_reserves(env: Env) -> (i128, i128) {
        let reserves = get_reserves(&env);
        (reserves.reserve_a, reserves.reserve_b)
    }

    pub fn get_tokens(env: Env) -> (Address, Address) {
        get_tokens(&env)
    }

    pub fn balance_of(env: Env, owner: Address) -> i128 {
        get_balance(&env, &owner)
    }

    pub fn total_supply(env: Env) -> i128 {
        get_total_supply(&env)
    }

    pub fn get_price(env: Env) -> i128 {
        let reserves = get_reserves(&env);
        if reserves.reserve_b == 0 {
            0
        } else {
            (reserves.reserve_a as u128 * 1_000_000_000_000 / reserves.reserve_b as u128) as i128
        }
    }

    pub fn get_cumulative_price(env: Env) -> u128 {
        let oracle = get_price_oracle(&env);
        oracle.cumulative_price
    }

    pub fn calculate_impermanent_loss(env: Env, initial_price: i128, current_price: i128) -> i128 {
        // Calculate impermanent loss: IL = 2*sqrt(r) / (1+r) - 1, where r = current_price/initial_price
        if initial_price == 0 || current_price == 0 {
            return 0;
        }

        let r = current_price as u128 * 1_000_000_000_000 / initial_price as u128;
        let sqrt_r = (r as f64).sqrt() as u128;
        let numerator = 2 * sqrt_r * 1_000_000_000_000;
        let denominator = 1_000_000_000_000 + r;
        let il = (numerator / denominator) as i128 - 1_000_000_000_000;

        il
    }
}