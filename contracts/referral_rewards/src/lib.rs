#![no_std]

//! Multi-level referral program with tiered rewards.
//!
//! The contract keeps a referral forest, a deterministic referral code per
//! user, an internal reward ledger and a set of caps, and pays every ancestor
//! of a referred user when volume is distributed.
//!
//! Rules:
//!
//! * **Codes** - every user owns at most one code. `generate_code` derives it
//!   as `SHA-256(xdr(user) || counter)` rendered with a 32 character alphabet
//!   behind the `R` marker, so a code is deterministic, always a valid symbol
//!   and unique: the counter is bumped on every mint and a candidate that is
//!   already taken is re-derived (at most `CODE_ATTEMPTS` times).
//! * **Tracking** - `register_referral` binds a user to exactly one referrer.
//!   Self referral, re-registration and any binding that would close a cycle
//!   are rejected, so the referrer links always form a forest. Level 1 is the
//!   direct referrer, level 2 the referrer of the referrer, and so on.
//! * **Rewards** - for a distribution of `volume` to `source`, the ancestor at
//!   level `l` receives
//!
//!   ```text
//!   base   = base_reward * level_shares_bps[l] / 10_000
//!   bonus  = base * tier_bonus_bps(tier) / 10_000
//!   payout = base + bonus
//!   ```
//!
//!   A level with a zero share earns nothing, and levels past `max_levels` (or
//!   past the end of the share vector) are simply not walked. Tiers are read
//!   from the state at the *start* of the call, so a distribution never changes
//!   the bonus it pays out; crossing a threshold therefore pays the higher
//!   bonus from the next distribution on.
//! * **Tiers** - a `TierRule` is satisfied when the referrer has at least
//!   `min_referrals` direct referrals and at least `min_volume` cumulative
//!   referred volume. Rules are validated to run from the weakest to the
//!   strongest requirement and the strongest satisfied rule wins; a referrer
//!   that satisfies none is `Bronze` with no bonus.
//! * **Caps** - `per_payout_cap`, `per_user_cap` and `global_cap` are applied
//!   in that order and **clamp** the payout rather than reverting it, so a
//!   distribution always succeeds and simply pays less. `distribute` and
//!   `quote` report the amount that was actually credited per level plus a
//!   `capped` flag. A cap of `0` disables that limit.
//! * **Ledger** - credited rewards are held internally and claimable with
//!   `claim_reward`, which moves the whole pending balance to the claimed
//!   counters and is therefore not repeatable: a second claim without new
//!   credits fails with `NothingToClaim`. An optional `claim_interval` adds a
//!   cooldown between claims. `reward_token` names the SEP-41 token the
//!   accounting is denominated in; no tokens move until a rail is added, so the
//!   ledger can be settled by a future entry point.
//! * **History** - every credited payout is appended to the history of its
//!   recipient, retaining at most `max_history` entries (hard cap
//!   `MAX_HISTORY`). The retained direct referral list of a referrer is capped
//!   at `MAX_REFERRALS` addresses.

use soroban_sdk::{
    contract, contractimpl, symbol_short, xdr::ToXdr, Address, Bytes, Env, Symbol, Vec,
};

mod storage;
mod types;

use storage::Storage;
use types::{
    Analytics, Config, Distribution, PayoutRecord, ReferralError, RewardLimits, Tier, TierRule,
    UserStats, ALPHABET, CODE_ATTEMPTS, CODE_LEN, CODE_PREFIX, MAX_BPS, MAX_CHAIN_DEPTH,
    MAX_HISTORY, MAX_LEVELS,
};

#[contract]
pub struct ReferralRewards;

#[contractimpl]
impl ReferralRewards {
    /// Initialises the protocol. Can only be called once.
    ///
    /// * `reward_token` - SEP-41 token the reward accounting is denominated in.
    /// * `base_reward` - reward paid to an ancestor with a 100% share, must be
    ///   `>= 0`.
    /// * `level_shares_bps` - share of `base_reward` per level in bps, must be
    ///   non empty and every entry `<= 10_000`.
    /// * `max_levels` - levels a distribution may pay, `1..=MAX_LEVELS`.
    /// * `limits` - per payout, per user and global caps (`0` disables a cap).
    /// * `tiers` - tier ladder ordered from the weakest to the strongest
    ///   requirement, every `bonus_bps <= 10_000`.
    /// * `max_history` - retained payouts per user, `1..=MAX_HISTORY`.
    /// * `claim_interval` - seconds between two claims, `0` disables it.
    pub fn initialize(
        env: Env,
        admin: Address,
        reward_token: Address,
        base_reward: i128,
        level_shares_bps: Vec<u32>,
        max_levels: u32,
        limits: RewardLimits,
        tiers: Vec<TierRule>,
        max_history: u32,
        claim_interval: u64,
    ) -> Result<(), ReferralError> {
        if Storage::has_config(&env) {
            return Err(ReferralError::AlreadyInitialized);
        }
        if base_reward < 0 {
            return Err(ReferralError::InvalidConfig);
        }
        if max_levels == 0 || max_levels > MAX_LEVELS {
            return Err(ReferralError::InvalidConfig);
        }
        if level_shares_bps.is_empty() {
            return Err(ReferralError::InvalidConfig);
        }
        for share in level_shares_bps.iter() {
            if share > MAX_BPS {
                return Err(ReferralError::InvalidConfig);
            }
        }
        if limits.per_payout_cap < 0 || limits.per_user_cap < 0 || limits.global_cap < 0 {
            return Err(ReferralError::InvalidConfig);
        }
        if max_history == 0 || max_history > MAX_HISTORY {
            return Err(ReferralError::InvalidConfig);
        }
        Self::validate_tiers(&tiers)?;

        let config = Config {
            admin: admin.clone(),
            reward_token: reward_token.clone(),
            base_reward,
            level_shares_bps,
            max_levels,
            limits,
            tiers,
            max_history,
            claim_interval,
        };
        Storage::set_config(&env, &config);
        Storage::set_stats(&env, &Analytics::empty());
        Storage::set_code_counter(&env, 0);

        env.events().publish(
            (symbol_short!("init"),),
            (admin, reward_token, base_reward, max_levels),
        );

        Ok(())
    }

    /// Returns the protocol configuration.
    pub fn get_config(env: Env) -> Result<Config, ReferralError> {
        Storage::get_config(&env)
    }

    /// Points the reward accounting at another SEP-41 token. Admin only.
    pub fn set_reward_token(env: Env, reward_token: Address) -> Result<(), ReferralError> {
        let mut config = Storage::get_config(&env)?;
        config.admin.require_auth();
        config.reward_token = reward_token.clone();
        Storage::set_config(&env, &config);

        env.events()
            .publish((symbol_short!("token"), config.admin.clone()), reward_token);

        Ok(())
    }

    // ------------------------------------------------------------ code minting

    /// Mints the referral code of `user`.
    ///
    /// The code is derived from the user address and a monotonically increasing
    /// counter, so it is deterministic for a given state and can never collide
    /// with a code that is already in the index.
    pub fn generate_code(env: Env, user: Address) -> Result<Symbol, ReferralError> {
        Storage::get_config(&env)?;
        user.require_auth();

        if Storage::has_code(&env, &user) {
            return Err(ReferralError::CodeAlreadyExists);
        }

        let mut counter = Storage::get_code_counter(&env);
        let mut attempts: u32 = 0;
        loop {
            let candidate = Self::derive_code(&env, &user, counter);
            if !Storage::has_code_owner(&env, &candidate) {
                counter = counter.saturating_add(1);
                Storage::set_code_counter(&env, counter);
                Storage::set_code(&env, &user, &candidate);
                Storage::set_code_owner(&env, &candidate, &user);
                Storage::add_to_registry(&env, &user);

                let mut stats = Storage::get_stats(&env);
                stats.users = stats.users.saturating_add(1);
                Storage::set_stats(&env, &stats);

                env.events()
                    .publish((symbol_short!("code_new"), user.clone()), candidate);

                return Ok(candidate);
            }

            counter = counter.saturating_add(1);
            attempts = attempts.saturating_add(1);
            if attempts >= CODE_ATTEMPTS {
                return Err(ReferralError::CodeCollision);
            }
        }
    }

    /// Returns the referral code of `user`.
    pub fn get_code(env: Env, user: Address) -> Result<Symbol, ReferralError> {
        Storage::get_config(&env)?;
        Storage::get_code(&env, &user)
    }

    /// Resolves a referral code to the address that owns it.
    pub fn resolve_code(env: Env, code: Symbol) -> Result<Address, ReferralError> {
        Storage::get_config(&env)?;
        Storage::get_code_owner(&env, &code)
    }

    // --------------------------------------------------------- referral tracking

    /// Binds `user` to the owner of `code` as its direct referrer.
    ///
    /// Rejects an unknown (including empty or malformed) code, a self
    /// referral, a user that is already bound and any binding that would close
    /// a cycle in the referral forest.
    pub fn register_referral(
        env: Env,
        code: Symbol,
        user: Address,
    ) -> Result<(), ReferralError> {
        Storage::get_config(&env)?;
        user.require_auth();

        if Storage::has_referrer(&env, &user) {
            return Err(ReferralError::AlreadyRegistered);
        }

        let referrer = Storage::get_code_owner(&env, &code)?;
        if referrer == user {
            return Err(ReferralError::SelfReferral);
        }

        // Walk up from the candidate referrer: the chain is a forest, so `user`
        // must never appear on it. The walk is bounded by `MAX_CHAIN_DEPTH`.
        let mut cursor = Some(referrer.clone());
        let mut depth: u32 = 0;
        while let Some(node) = cursor {
            if node == user {
                return Err(ReferralError::CycleDetected);
            }
            if depth >= MAX_CHAIN_DEPTH {
                return Err(ReferralError::CycleDetected);
            }
            depth = depth.saturating_add(1);
            cursor = Storage::get_referrer(&env, &node);
        }

        Storage::set_referrer(&env, &user, &referrer);
        Storage::add_referral(&env, &referrer, &user);

        let mut stats = Storage::get_user(&env, &referrer);
        let is_new_referrer = stats.direct_referrals == 0;
        stats.direct_referrals = stats.direct_referrals.saturating_add(1);
        Storage::set_user(&env, &referrer, &stats);

        let mut analytics = Storage::get_stats(&env);
        analytics.referrals = analytics.referrals.saturating_add(1);
        if is_new_referrer {
            analytics.referrers = analytics.referrers.saturating_add(1);
        }
        Storage::set_stats(&env, &analytics);

        env.events()
            .publish((symbol_short!("ref_reg"), user), (referrer, code));

        Ok(())
    }

    /// `true` when `user` is bound to the owner of `code`.
    ///
    /// The code has to exist; an unknown code is rejected with `CodeNotFound`.
    pub fn is_referral(env: Env, code: Symbol, user: Address) -> Result<bool, ReferralError> {
        Storage::get_config(&env)?;
        let owner = Storage::get_code_owner(&env, &code)?;
        Ok(Storage::get_referrer(&env, &user) == Some(owner))
    }

    /// Returns the direct referrer of `user`.
    pub fn get_referrer(env: Env, user: Address) -> Result<Address, ReferralError> {
        Storage::get_config(&env)?;
        Storage::get_referrer(&env, &user).ok_or(ReferralError::NoReferrer)
    }

    /// Number of users bound directly to `user`.
    pub fn get_referral_count(env: Env, user: Address) -> Result<u32, ReferralError> {
        Storage::get_config(&env)?;
        Ok(Storage::get_user(&env, &user).direct_referrals)
    }

    /// Retained list of the users `user` referred directly, oldest first.
    ///
    /// The list is capped at `MAX_REFERRALS` entries; `get_referral_count`
    /// always reports the true total.
    pub fn get_referral_history(env: Env, user: Address) -> Result<Vec<Address>, ReferralError> {
        Storage::get_config(&env)?;
        Ok(Storage::get_referrals(&env, &user))
    }

    /// Returns up to `limit` ancestors of `user`, level 1 first.
    ///
    /// `limit == 0` walks the whole chain.
    pub fn get_ancestors(
        env: Env,
        user: Address,
        limit: u32,
    ) -> Result<Vec<Address>, ReferralError> {
        Storage::get_config(&env)?;
        let mut result = Vec::new(&env);
        let mut cursor = Storage::get_referrer(&env, &user);
        let mut level: u32 = 1;
        while let Some(node) = cursor {
            if limit != 0 && level > limit {
                break;
            }
            result.push_back(node.clone());
            cursor = Storage::get_referrer(&env, &node);
            level = level.saturating_add(1);
        }
        Ok(result)
    }

    // --------------------------------------------------------- reward accounting

    /// Credits the ancestors of `source` with the rewards generated by
    /// `volume`.
    ///
    /// Every ancestor within `max_levels` receives its level share of
    /// `base_reward` plus the bonus of its tier, clamped by the per payout, per
    /// user and global caps. Credits are booked on the internal ledger and are
    /// claimable with `claim_reward`.
    pub fn distribute(
        env: Env,
        source: Address,
        volume: i128,
    ) -> Result<Distribution, ReferralError> {
        let config = Storage::get_config(&env)?;
        source.require_auth();

        if volume <= 0 {
            return Err(ReferralError::InvalidAmount);
        }

        let result = Self::plan(&env, &config, &source, volume, true)?;

        env.events().publish(
            (symbol_short!("reward"), source.clone()),
            (result.total_credited, result.levels_paid),
        );
        if result.capped {
            env.events().publish(
                (symbol_short!("cap_hit"), source.clone()),
                (volume, result.total_credited),
            );
        }

        Ok(result)
    }

    /// Simulates `distribute` without touching any state.
    ///
    /// The result is identical to what `distribute` would return for the same
    /// state and inputs.
    pub fn quote(env: Env, source: Address, volume: i128) -> Result<Distribution, ReferralError> {
        let config = Storage::get_config(&env)?;
        if volume <= 0 {
            return Err(ReferralError::InvalidAmount);
        }
        Self::plan(&env, &config, &source, volume, false)
    }

    /// Claims the whole pending reward balance of `user` and returns the amount
    /// paid out.
    ///
    /// The claim moves the pending balance to the claimed counters, so it is not
    /// repeatable: claiming again without new credits fails with
    /// `NothingToClaim`.
    pub fn claim_reward(env: Env, user: Address) -> Result<i128, ReferralError> {
        let config = Storage::get_config(&env)?;
        user.require_auth();

        let mut stats = Storage::get_user(&env, &user);
        if stats.pending <= 0 {
            return Err(ReferralError::NothingToClaim);
        }

        let now = env.ledger().timestamp();
        if config.claim_interval > 0
            && stats.last_claim_at > 0
            && now < stats.last_claim_at.saturating_add(config.claim_interval)
        {
            return Err(ReferralError::ClaimCooldown);
        }

        let amount = stats.pending;
        stats.pending = 0;
        stats.rewards_claimed = stats.rewards_claimed.saturating_add(amount);
        stats.claim_count = stats.claim_count.saturating_add(1);
        stats.last_claim_at = now;
        Storage::set_user(&env, &user, &stats);

        let mut analytics = Storage::get_stats(&env);
        analytics.claims = analytics.claims.saturating_add(1);
        analytics.total_claimed = analytics.total_claimed.saturating_add(amount);
        Storage::set_stats(&env, &analytics);

        env.events().publish((symbol_short!("claim"), user), amount);

        Ok(amount)
    }

    /// Pending, still claimable reward balance of `user`.
    pub fn get_balance(env: Env, user: Address) -> Result<i128, ReferralError> {
        Storage::get_config(&env)?;
        Ok(Storage::get_user(&env, &user).pending)
    }

    /// Referral and reward counters of `user`.
    pub fn get_stats(env: Env, user: Address) -> Result<UserStats, ReferralError> {
        Storage::get_config(&env)?;
        Ok(Storage::get_user(&env, &user))
    }

    /// Tier `user` currently sits in, given its direct referral count and its
    /// cumulative referred volume.
    pub fn tier_for(env: Env, user: Address) -> Result<Tier, ReferralError> {
        let config = Storage::get_config(&env)?;
        Ok(Self::tier_of(&config, &Storage::get_user(&env, &user)))
    }

    /// Tier bonus of `user`, in bps.
    pub fn tier_bonus(env: Env, user: Address) -> Result<u32, ReferralError> {
        let config = Storage::get_config(&env)?;
        let tier = Self::tier_of(&config, &Storage::get_user(&env, &user));
        Ok(Self::tier_bonus_bps(&config, tier))
    }

    // ----------------------------------------------------------------- history

    /// Returns up to `limit` of the most recent payouts credited to `user`,
    /// oldest first. Only credited payouts are retained, at most
    /// `Config::max_history` of them.
    pub fn get_history(
        env: Env,
        user: Address,
        limit: u32,
    ) -> Result<Vec<PayoutRecord>, ReferralError> {
        Storage::get_config(&env)?;
        Ok(Storage::get_history(&env, &user, limit))
    }

    // --------------------------------------------------------------- analytics

    /// Protocol wide counters.
    ///
    /// Everything but the per tier counts and the top referrer is maintained
    /// incrementally; those two are derived from the code registry so they
    /// always reflect the current state.
    pub fn get_analytics(env: Env) -> Result<Analytics, ReferralError> {
        let config = Storage::get_config(&env)?;
        let mut analytics = Storage::get_stats(&env);

        let mut bronze: u32 = 0;
        let mut silver: u32 = 0;
        let mut gold: u32 = 0;
        let mut top_referrer: Option<Address> = None;
        let mut top_rewards: i128 = 0;

        let registry = Storage::get_registry(&env);
        for user in registry.iter() {
            let stats = Storage::get_user(&env, &user);
            match Self::tier_of(&config, &stats) {
                Tier::Bronze => bronze = bronze.saturating_add(1),
                Tier::Silver => silver = silver.saturating_add(1),
                Tier::Gold => gold = gold.saturating_add(1),
            }
            // Strictly greater keeps the earliest code owner on a tie.
            if stats.rewards_earned > top_rewards {
                top_rewards = stats.rewards_earned;
                top_referrer = Some(user);
            }
        }

        analytics.bronze = bronze;
        analytics.silver = silver;
        analytics.gold = gold;
        analytics.top_referrer = top_referrer;
        analytics.top_referrer_rewards = top_rewards;

        Ok(analytics)
    }

    // ----------------------------------------------------------------- helpers

    /// Rejects a tier ladder that is not ordered from the weakest to the
    /// strongest requirement, or that carries an out of range bonus.
    fn validate_tiers(tiers: &Vec<TierRule>) -> Result<(), ReferralError> {
        let mut previous_volume: i128 = -1;
        let mut previous_referrals: u32 = 0;
        for rule in tiers.iter() {
            if rule.min_volume < 0 || rule.bonus_bps > MAX_BPS {
                return Err(ReferralError::InvalidConfig);
            }
            let stronger = rule.min_volume > previous_volume
                || (rule.min_volume == previous_volume && rule.min_referrals > previous_referrals);
            if !stronger {
                return Err(ReferralError::InvalidConfig);
            }
            previous_volume = rule.min_volume;
            previous_referrals = rule.min_referrals;
        }
        Ok(())
    }

    /// Strongest tier rule satisfied by `stats`, `Bronze` when none is.
    fn tier_of(config: &Config, stats: &UserStats) -> Tier {
        let mut tier = Tier::Bronze;
        for rule in config.tiers.iter() {
            let satisfied = stats.direct_referrals >= rule.min_referrals
                && stats.referred_volume >= rule.min_volume;
            if satisfied {
                tier = rule.tier;
            }
        }
        tier
    }

    /// Bonus configured for `tier`, `0` when the tier has no rule.
    fn tier_bonus_bps(config: &Config, tier: Tier) -> u32 {
        for rule in config.tiers.iter() {
            if rule.tier == tier {
                return rule.bonus_bps;
            }
        }
        0
    }

    /// Remaining global credit budget, `i128::MAX` when the cap is disabled.
    fn global_room(config: &Config, analytics: &Analytics) -> i128 {
        if config.limits.global_cap == 0 {
            i128::MAX
        } else {
            (config.limits.global_cap - analytics.total_rewards).max(0)
        }
    }

    /// Deterministic referral code for `user` at `counter`.
    ///
    /// The digest of `xdr(user) || counter` is rendered with `ALPHABET` behind
    /// the `R` marker, which keeps the result a valid symbol of `CODE_LEN`
    /// characters regardless of the digest.
    fn derive_code(env: &Env, user: &Address, counter: u32) -> Symbol {
        let mut seed = Bytes::new(env);
        seed.append(&user.clone().to_xdr(env));
        seed.append(&Bytes::from_array(env, &counter.to_be_bytes()));

        let digest = env.crypto().sha256(&seed).to_array();
        let mut rendered = [CODE_PREFIX; CODE_LEN];
        for (i, slot) in rendered.iter_mut().enumerate().skip(1) {
            *slot = ALPHABET[(digest[i - 1] as usize) % ALPHABET.len()];
        }

        // Every byte comes from `ALPHABET`, so this is always plain ASCII.
        let text = core::str::from_utf8(&rendered).unwrap_or("R00000000");
        Symbol::new(env, text)
    }

    /// `base * bps / 10_000` without risking an `i128` overflow.
    fn mul_bps(amount: i128, bps: u32) -> i128 {
        let divisor = MAX_BPS as i128;
        let factor = bps as i128;
        (amount / divisor) * factor + (amount % divisor) * factor / divisor
    }

    /// Walks the ancestor chain of `source` and computes the payout of every
    /// level. With `apply` set the computed state is written back, otherwise
    /// the walk is a pure simulation.
    fn plan(
        env: &Env,
        config: &Config,
        source: &Address,
        volume: i128,
        apply: bool,
    ) -> Result<Distribution, ReferralError> {
        let now = env.ledger().timestamp();

        // Pass 1: snapshot the chain and the state it is evaluated on, so the
        // tier of a level never depends on the volume booked by this very call.
        let mut chain: Vec<(Address, UserStats)> = Vec::new(env);
        let mut cursor = Storage::get_referrer(env, source);
        let mut level: u32 = 1;
        while let Some(parent) = cursor {
            if level > config.max_levels {
                break;
            }
            chain.push_back((parent.clone(), Storage::get_user(env, &parent)));
            cursor = Storage::get_referrer(env, &parent);
            level = level.saturating_add(1);
        }
        if chain.is_empty() {
            return Err(ReferralError::NoChain);
        }

        // Pass 2: price every level, clamping by the per payout, per user and
        // global caps in that order.
        let mut global_room = Self::global_room(config, &Storage::get_stats(env));
        let mut payouts: Vec<PayoutRecord> = Vec::new(env);
        let mut total_credited: i128 = 0;
        let mut levels_paid: u32 = 0;
        let mut capped = false;
        let mut index: u32 = 0;

        while index < chain.len() {
            let (recipient, mut stats) = chain.get(index).unwrap();
            let tier = Self::tier_of(config, &stats);
            let share_bps = config.level_shares_bps.get(index).unwrap_or(0);
            let base_amount = Self::mul_bps(config.base_reward, share_bps);
            let bonus_amount = Self::mul_bps(base_amount, Self::tier_bonus_bps(config, tier));
            let mut amount = base_amount.saturating_add(bonus_amount).max(0);

            if config.limits.per_payout_cap > 0 && amount > config.limits.per_payout_cap {
                amount = config.limits.per_payout_cap;
                capped = true;
            }
            if config.limits.per_user_cap > 0 {
                let room = (config.limits.per_user_cap - stats.rewards_earned).max(0);
                if amount > room {
                    amount = room;
                    capped = true;
                }
            }
            if amount > global_room {
                amount = global_room.max(0);
                capped = true;
            }

            stats.referred_volume = stats.referred_volume.saturating_add(volume);
            if amount > 0 {
                stats.rewards_earned = stats.rewards_earned.saturating_add(amount);
                stats.pending = stats.pending.saturating_add(amount);
                stats.payout_count = stats.payout_count.saturating_add(1);
                total_credited = total_credited.saturating_add(amount);
                global_room = global_room.saturating_sub(amount);
                levels_paid = levels_paid.saturating_add(1);
            }

            let record = PayoutRecord {
                source: source.clone(),
                recipient: recipient.clone(),
                level: index.saturating_add(1),
                volume,
                base_amount,
                bonus_amount,
                amount,
                tier,
                timestamp: now,
            };
            payouts.push_back(record.clone());

            if apply {
                Storage::set_user(env, &recipient, &stats);
                // Only credited payouts are worth retaining.
                if amount > 0 {
                    Storage::add_history(env, &recipient, &record, config.max_history);
                }
                let promoted = Self::tier_of(config, &stats);
                if promoted != tier {
                    env.events().publish(
                        (symbol_short!("tier_up"), recipient),
                        (promoted as u32, tier as u32),
                    );
                }
            }

            index = index.saturating_add(1);
        }

        if apply {
            let mut analytics = Storage::get_stats(env);
            analytics.distributions = analytics.distributions.saturating_add(1);
            analytics.total_volume = analytics.total_volume.saturating_add(volume);
            analytics.total_rewards = analytics.total_rewards.saturating_add(total_credited);
            Storage::set_stats(env, &analytics);
        }

        Ok(Distribution {
            source: source.clone(),
            volume,
            total_credited,
            levels_paid,
            capped,
            payouts,
        })
    }
}

mod test;
