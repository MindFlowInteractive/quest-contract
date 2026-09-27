use soroban_sdk::{contracterror, contracttype, Address, Vec};

/// Basis point denominator used by every share and bonus in the protocol
/// (100% == 10_000 bps).
pub const MAX_BPS: u32 = 10_000;

/// Hard upper bound for `Config::max_levels`.
///
/// The payout walk is bounded by `max_levels`, so a single distribution can
/// never pay more than this many ancestors.
pub const MAX_LEVELS: u32 = 10;

/// Hard bound for the number of steps taken while walking a referral chain.
///
/// The referrer map is a forest (cycles are rejected on registration), so the
/// walk always terminates before this bound; it is kept as a defensive limit so
/// a corrupted chain can never spin forever.
pub const MAX_CHAIN_DEPTH: u32 = 64;

/// Hard upper bound for `Config::max_history` (retained payouts per user).
pub const MAX_HISTORY: u32 = 100;

/// Hard upper bound for the retained direct referral list of a single referrer.
///
/// The `direct_referrals` counter keeps growing past this value, only the
/// address list is truncated.
pub const MAX_REFERRALS: u32 = 200;

/// Number of candidates tried when deriving a code that collides with an
/// existing one.
pub const CODE_ATTEMPTS: u32 = 8;

/// Length of a generated referral code, including the leading marker character.
pub const CODE_LEN: usize = 10;

/// Crockford style alphabet (no `I`, `L`, `O`, `U`) used to render codes.
pub const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Marker that prefixes every generated code so codes are always valid symbols
/// and can never be confused with a caller supplied identifier.
pub const CODE_PREFIX: u8 = b'R';

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ReferralError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    InvalidConfig = 4,
    CodeAlreadyExists = 5,
    CodeNotFound = 6,
    /// Code derivation exhausted every candidate without finding a free code.
    CodeCollision = 7,
    SelfReferral = 8,
    AlreadyRegistered = 9,
    NoReferrer = 10,
    /// The registration would close a cycle in the referral forest.
    CycleDetected = 11,
    InvalidAmount = 12,
    /// The user has no referrer, so there is nobody to pay.
    NoChain = 13,
    NothingToClaim = 14,
    ClaimCooldown = 15,
}

/// Loyalty tier of a referrer.
///
/// The thresholds live in `Config::tiers`; the enum only names the ladder so
/// that on-chain tooling can index a tier without re-deriving it.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Tier {
    Bronze = 0,
    Silver = 1,
    Gold = 2,
}

/// Requirement and bonus of a single tier.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TierRule {
    pub tier: Tier,
    /// Minimum number of direct referrals the referrer must have collected.
    pub min_referrals: u32,
    /// Minimum cumulative referred volume the referrer must have generated.
    pub min_volume: i128,
    /// Bonus added on top of the base reward, in bps (`1_000` == +10%).
    pub bonus_bps: u32,
}

/// Cap applied to the reward accounting.
///
/// A cap of `0` disables that particular limit.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RewardLimits {
    /// Maximum credited to a single recipient within a single distribution.
    pub per_payout_cap: i128,
    /// Maximum lifetime amount credited to one recipient.
    pub per_user_cap: i128,
    /// Maximum lifetime amount credited by the whole protocol.
    pub global_cap: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
    /// SEP-41 token the reward accounting is denominated in.
    pub reward_token: Address,
    /// Reward paid to an ancestor whose level share is 100% (10_000 bps), before
    /// the tier bonus, on every distribution.
    pub base_reward: i128,
    /// Share of `base_reward` paid at level 1, 2, ... in bps.
    pub level_shares_bps: Vec<u32>,
    /// Number of ancestor levels a distribution may pay.
    pub max_levels: u32,
    pub limits: RewardLimits,
    /// Tier ladder, ordered from the weakest to the strongest requirement.
    pub tiers: Vec<TierRule>,
    /// Payout records retained per user.
    pub max_history: u32,
    /// Minimum number of seconds between two claims; `0` disables the cooldown.
    pub claim_interval: u64,
}

/// Per user referral and reward counters.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserStats {
    pub user: Address,
    /// Number of users bound directly to this referrer.
    pub direct_referrals: u32,
    /// Cumulative referred volume across every level.
    pub referred_volume: i128,
    /// Cumulative amount credited to this user.
    pub rewards_earned: i128,
    /// Cumulative amount claimed by this user.
    pub rewards_claimed: i128,
    /// Credited but not claimed yet.
    pub pending: i128,
    /// Number of payouts credited to this user.
    pub payout_count: u32,
    pub claim_count: u32,
    /// Ledger timestamp of the last successful claim.
    pub last_claim_at: u64,
}

/// One credited payout of a single distribution.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PayoutRecord {
    /// User whose volume triggered the distribution.
    pub source: Address,
    /// Ancestor the payout was credited to.
    pub recipient: Address,
    /// 1-based ancestor level, `1` being the direct referrer.
    pub level: u32,
    /// Volume that triggered the distribution.
    pub volume: i128,
    /// `base_reward * share_bps(level) / 10_000`.
    pub base_amount: i128,
    /// Tier bonus added on top of `base_amount`.
    pub bonus_amount: i128,
    /// Amount actually credited, i.e. after every cap was applied.
    pub amount: i128,
    /// Tier the bonus was derived from (state at the start of the call).
    pub tier: Tier,
    pub timestamp: u64,
}

/// Outcome of `distribute` / `quote`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Distribution {
    pub source: Address,
    pub volume: i128,
    /// Sum of every credited payout.
    pub total_credited: i128,
    /// Number of ancestors that actually received a non zero payout.
    pub levels_paid: u32,
    /// `true` when at least one payout was reduced by a cap.
    pub capped: bool,
    /// One entry per walked level, including levels that earned nothing.
    pub payouts: Vec<PayoutRecord>,
}

/// Protocol wide analytics, derived from the stored state.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Analytics {
    /// Number of referral codes minted.
    pub users: u32,
    /// Number of referral bindings created.
    pub referrals: u32,
    /// Number of users holding at least one direct referral.
    pub referrers: u32,
    pub distributions: u32,
    pub claims: u32,
    pub total_volume: i128,
    pub total_rewards: i128,
    pub total_claimed: i128,
    /// Number of code owners currently in each tier.
    pub bronze: u32,
    pub silver: u32,
    pub gold: u32,
    /// Code owner with the highest `rewards_earned`, ties broken by code age.
    pub top_referrer: Option<Address>,
    pub top_referrer_rewards: i128,
}

impl UserStats {
    /// Zeroed counters for `user`.
    pub fn new(user: &Address) -> Self {
        UserStats {
            user: user.clone(),
            direct_referrals: 0,
            referred_volume: 0,
            rewards_earned: 0,
            rewards_claimed: 0,
            pending: 0,
            payout_count: 0,
            claim_count: 0,
            last_claim_at: 0,
        }
    }
}

impl Analytics {
    /// Zeroed aggregate used before the first state changing call.
    pub fn empty() -> Self {
        Analytics {
            users: 0,
            referrals: 0,
            referrers: 0,
            distributions: 0,
            claims: 0,
            total_volume: 0,
            total_rewards: 0,
            total_claimed: 0,
            bronze: 0,
            silver: 0,
            gold: 0,
            top_referrer: None,
            top_referrer_rewards: 0,
        }
    }
}
