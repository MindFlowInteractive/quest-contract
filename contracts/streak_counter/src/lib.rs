#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, Map,
    Vec,
};

const DAY: u64 = 86_400;
/// A freeze can be spent at most twice in any rolling 30-day window.
const FREEZE_PERIOD_DAYS: u64 = 30;
const MAX_FREEZES_PER_PERIOD: u32 = 2;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    InvalidConfig = 3,
    InvalidAmount = 4,
    Expired = 5,
    NotExpired = 6,
    UnknownMilestone = 7,
    Ineligible = 8,
    AlreadyClaimed = 9,
    InsufficientFunds = 10,
    InvalidDay = 11,
    Overflow = 12,
}

#[contracttype]
#[derive(Clone)]
pub struct Config {
    pub admin: Address,
    pub verifier: Address,
    pub token: Address,
    pub expires_at: u64,
    pub milestones: Map<u32, i128>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Streak {
    pub last_day: u64,
    pub current: u32,
    pub best: u32,
}

#[contracttype]
#[derive(Clone)]
enum Key {
    Config,
    Streak(Address),
    Claimed(Address, u32),
    Freezes(Address),
    FreezeUses(Address),
}

#[contract]
pub struct StreakCounter;

fn config(env: &Env) -> Result<Config, Error> {
    let config = env
        .storage()
        .instance()
        .get(&Key::Config)
        .ok_or(Error::NotInitialized)?;
    let ttl = env.storage().max_ttl();
    env.storage().instance().extend_ttl(ttl / 2, ttl);
    Ok(config)
}

fn retain(env: &Env, key: &Key) {
    let ttl = env.storage().max_ttl();
    env.storage().persistent().extend_ttl(key, ttl / 2, ttl);
}

fn active(env: &Env, config: &Config) -> Result<(), Error> {
    if env.ledger().timestamp() >= config.expires_at {
        return Err(Error::Expired);
    }
    Ok(())
}

fn freeze_balance(env: &Env, player: &Address) -> u32 {
    env.storage()
        .persistent()
        .get(&Key::Freezes(player.clone()))
        .unwrap_or(0)
}

fn freeze_uses(env: &Env, player: &Address) -> Vec<u64> {
    env.storage()
        .persistent()
        .get(&Key::FreezeUses(player.clone()))
        .unwrap_or(Vec::new(env))
}

/// Spend one freeze if the player has one and is under the rolling limit.
/// Returns the remaining balance, or `None` when no freeze can be spent.
/// `day` is the check-in day the freeze is spent on.
fn try_consume_freeze(env: &Env, player: &Address, day: u64) -> Result<Option<u32>, Error> {
    let balance = freeze_balance(env, player);
    if balance == 0 {
        return Ok(None);
    }
    let uses = freeze_uses(env, player);
    let mut fresh = Vec::new(env);
    for use_day in uses.iter() {
        if day < use_day + FREEZE_PERIOD_DAYS {
            fresh.push_back(use_day);
        }
    }
    if fresh.len() >= MAX_FREEZES_PER_PERIOD {
        return Ok(None);
    }
    let remaining = balance - 1;
    let freeze_key = Key::Freezes(player.clone());
    env.storage().persistent().set(&freeze_key, &remaining);
    fresh.push_back(day);
    let uses_key = Key::FreezeUses(player.clone());
    env.storage().persistent().set(&uses_key, &fresh);
    retain(env, &freeze_key);
    retain(env, &uses_key);
    Ok(Some(remaining))
}

#[contractimpl]
impl StreakCounter {
    /// Immutable pool terms. The verifier is a trusted puzzle-completion authority.
    pub fn initialize(
        env: Env,
        admin: Address,
        verifier: Address,
        token: Address,
        expires_at: u64,
        milestones: Map<u32, i128>,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&Key::Config) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        if expires_at <= env.ledger().timestamp() || milestones.is_empty() || milestones.len() > 32
        {
            return Err(Error::InvalidConfig);
        }
        for (days, amount) in milestones.iter() {
            if days == 0 || amount <= 0 {
                return Err(Error::InvalidConfig);
            }
        }
        env.storage().instance().set(
            &Key::Config,
            &Config {
                admin,
                verifier,
                token,
                expires_at,
                milestones,
            },
        );
        config(&env)?;
        Ok(())
    }

    /// At most one verified completion per UTC ledger day; no caller-supplied time.
    pub fn check_in(env: Env, player: Address) -> Result<Streak, Error> {
        let cfg = config(&env)?;
        active(&env, &cfg)?;
        cfg.verifier.require_auth();
        let day = env.ledger().timestamp() / DAY;
        let key = Key::Streak(player.clone());
        let mut streak: Streak = env.storage().persistent().get(&key).unwrap_or(Streak {
            last_day: day,
            current: 0,
            best: 0,
        });
        if streak.current > 0 && day < streak.last_day {
            return Err(Error::InvalidDay);
        }
        if streak.current == 0 || day != streak.last_day {
            let gap = if streak.current > 0 {
                day - streak.last_day
            } else {
                0
            };
            if gap == 1 {
                streak.current = streak.current.checked_add(1).ok_or(Error::Overflow)?;
            } else if gap == 2 {
                // Exactly one missed day: spend a freeze to keep the streak
                // alive instead of resetting it. No freeze, or the rolling
                // limit reached, means the streak resets as before.
                match try_consume_freeze(&env, &player, day)? {
                    Some(remaining) => {
                        streak.current = streak.current.checked_add(1).ok_or(Error::Overflow)?;
                        env.events()
                            .publish((symbol_short!("frz_used"), player.clone()), remaining);
                    }
                    None => {
                        streak.current = 1;
                    }
                }
            } else {
                // A freeze only covers one missed day; longer gaps reset.
                streak.current = 1;
            }
            streak.last_day = day;
            streak.best = streak.best.max(streak.current);
            env.storage().persistent().set(&key, &streak);
            env.events()
                .publish((symbol_short!("check_in"), player), streak.clone());
        }
        retain(&env, &key);
        Ok(streak)
    }

    /// Any authenticated sponsor may deposit; this also permits admin top-ups.
    pub fn fund(env: Env, from: Address, amount: i128) -> Result<(), Error> {
        let cfg = config(&env)?;
        active(&env, &cfg)?;
        if amount <= 0 || from == env.current_contract_address() {
            return Err(Error::InvalidAmount);
        }
        from.require_auth();
        token::Client::new(&env, &cfg.token).transfer(
            &from,
            &env.current_contract_address(),
            &amount,
        );
        env.events().publish((symbol_short!("fund"), from), amount);
        Ok(())
    }

    /// Only the admin can grant freeze tokens, which a player spends
    /// automatically to protect a streak from one missed day.
    pub fn grant_freeze(env: Env, player: Address, amount: u32) -> Result<u32, Error> {
        let cfg = config(&env)?;
        active(&env, &cfg)?;
        cfg.admin.require_auth();
        if amount == 0 {
            return Err(Error::InvalidAmount);
        }
        let key = Key::Freezes(player.clone());
        let balance = freeze_balance(&env, &player)
            .checked_add(amount)
            .ok_or(Error::Overflow)?;
        env.storage().persistent().set(&key, &balance);
        retain(&env, &key);
        env.events()
            .publish((symbol_short!("frz_grant"), player), balance);
        Ok(balance)
    }

    pub fn claim(env: Env, player: Address, milestone: u32) -> Result<i128, Error> {
        let cfg = config(&env)?;
        active(&env, &cfg)?;
        player.require_auth();
        let amount = cfg
            .milestones
            .get(milestone)
            .ok_or(Error::UnknownMilestone)?;
        let key = Key::Claimed(player.clone(), milestone);
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyClaimed);
        }
        let streak = Self::get_streak(env.clone(), player.clone())?;
        if streak.map(|s| s.best).unwrap_or(0) < milestone {
            return Err(Error::Ineligible);
        }
        let client = token::Client::new(&env, &cfg.token);
        if client.balance(&env.current_contract_address()) < amount {
            return Err(Error::InsufficientFunds);
        }
        // Set before the external call; Soroban rolls this back if transfer fails.
        env.storage().persistent().set(&key, &true);
        retain(&env, &key);
        client.transfer(&env.current_contract_address(), &player, &amount);
        env.events()
            .publish((symbol_short!("claim"), player, milestone), amount);
        Ok(amount)
    }

    /// Claims close at expiry; only the admin can recover the remaining balance.
    pub fn withdraw(env: Env, amount: i128) -> Result<(), Error> {
        let cfg = config(&env)?;
        cfg.admin.require_auth();
        if env.ledger().timestamp() < cfg.expires_at {
            return Err(Error::NotExpired);
        }
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let client = token::Client::new(&env, &cfg.token);
        if client.balance(&env.current_contract_address()) < amount {
            return Err(Error::InsufficientFunds);
        }
        client.transfer(&env.current_contract_address(), &cfg.admin, &amount);
        env.events()
            .publish((symbol_short!("withdraw"), cfg.admin), amount);
        Ok(())
    }

    pub fn get_config(env: Env) -> Result<Config, Error> {
        config(&env)
    }

    /// Recorded streak; `best` preserves earned eligibility even after a gap.
    pub fn get_streak(env: Env, player: Address) -> Result<Option<Streak>, Error> {
        config(&env)?;
        let key = Key::Streak(player);
        let value = env.storage().persistent().get(&key);
        if value.is_some() {
            retain(&env, &key);
        }
        Ok(value)
    }

    pub fn get_freezes(env: Env, player: Address) -> Result<u32, Error> {
        config(&env)?;
        let key = Key::Freezes(player);
        let value: Option<u32> = env.storage().persistent().get(&key);
        if value.is_some() {
            retain(&env, &key);
        }
        Ok(value.unwrap_or(0))
    }

    pub fn has_claimed(env: Env, player: Address, milestone: u32) -> Result<bool, Error> {
        config(&env)?;
        let key = Key::Claimed(player, milestone);
        let claimed = env.storage().persistent().has(&key);
        if claimed {
            retain(&env, &key);
        }
        Ok(claimed)
    }

    pub fn balance(env: Env) -> Result<i128, Error> {
        let cfg = config(&env)?;
        Ok(token::Client::new(&env, &cfg.token).balance(&env.current_contract_address()))
    }
}

#[cfg(test)]
mod test;
