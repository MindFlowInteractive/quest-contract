#![no_std]
// Closes #308: futures/derivatives (starter: open position + margin
// tracking). Settlement, funding rates, and liquidation are follow-ups.

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

#[contracttype]
#[derive(Clone)]
pub struct Position {
    pub trader: Address,
    pub size: i128,
    pub margin: i128,
    pub entry_price: i128,
}

#[contracttype]
pub enum DataKey {
    Position(Address),
}

#[contract]
pub struct FuturesContract;

#[contractimpl]
impl FuturesContract {
    /// Opens a position for `trader`, requiring `margin` to be posted upfront.
    pub fn open_position(env: Env, trader: Address, size: i128, margin: i128, entry_price: i128) {
        trader.require_auth();
        if margin <= 0 {
            panic!("margin must be positive");
        }
        let position = Position { trader: trader.clone(), size, margin, entry_price };
        env.storage().instance().set(&DataKey::Position(trader), &position);
    }

    /// Returns `trader`'s open position, if any.
    pub fn get_position(env: Env, trader: Address) -> Option<Position> {
        env.storage().instance().get(&DataKey::Position(trader))
    }
}
