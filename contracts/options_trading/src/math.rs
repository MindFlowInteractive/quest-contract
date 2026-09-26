//! Fixed-point (Q9) math helpers used by the options pricing model.
//!
//! The contract targets `wasm32-unknown-unknown` with `#![no_std]`, so no
//! floating point primitives are available. Every value handled here is an
//! `i128` holding a real number multiplied by `SCALE` (10^9). The Q9 grid gives
//! roughly nine significant decimal digits, which is far more precision than an
//! on-chain option premium needs.

/// Number of fractional digits carried by every value in this module.
pub const SCALE: i128 = 1_000_000_000;

/// ln(2) in Q9.
const LN2: i128 = 693_147_180;
/// sqrt(2*pi) in Q9.
const SQRT_2PI: i128 = 2_506_628_274;
/// Abramowitz & Stegun 26.2.17 polynomial coefficient `p`.
const NORM_P: i128 = 231_641_900;

const B1: i128 = 319_381_530;
const B2: i128 = -356_563_782;
const B3: i128 = 1_781_477_937;
const B4: i128 = -1_821_255_978;
const B5: i128 = 1_330_274_429;

pub fn clamp(x: i128, lo: i128, hi: i128) -> i128 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

/// Integer square root using Newton-Raphson.
///
/// The seed is derived from the bit length of `n`, which guarantees
/// `seed >= sqrt(n)` on the first iteration and therefore keeps `x + n / x`
/// well inside the `u128` range at every step.
pub fn isqrt(n: u128) -> u128 {
    if n == 0 {
        return 0;
    }
    let bits = 128 - n.leading_zeros();
    let mut x: u128 = 1u128 << ((bits + 1) / 2);
    loop {
        let y = (x + n / x) / 2;
        if y >= x {
            break;
        }
        x = y;
    }
    x
}

/// Square root of a Q9 value, returned in Q9.
pub fn sqrt_q9(v: i128) -> i128 {
    if v <= 0 {
        return 0;
    }
    isqrt((v as u128) * (SCALE as u128)) as i128
}

/// Natural logarithm of a Q9 value, returned in Q9.
///
/// The argument is range-reduced into `[1, 2)` and `atanh` is evaluated through
/// its (rapidly converging) odd-power series, so no floating point is needed.
pub fn ln_q9(v: i128) -> i128 {
    if v <= 0 {
        return 0;
    }

    let mut x = v;
    let mut k: i32 = 0;
    while x > 2 * SCALE {
        x /= 2;
        k += 1;
    }
    while x < SCALE {
        x *= 2;
        k -= 1;
    }

    // t = (x - 1) / (x + 1), so that |t| <= 1/3.
    let t = ((x - SCALE) * SCALE) / (x + SCALE);
    let t2 = (t * t) / SCALE;

    // atanh(t) = t + t^3/3 + t^5/5 + ...
    let mut term = t;
    let mut sum = t;
    let mut d: i128 = 3;
    for _ in 0..6 {
        term = (term * t2) / SCALE;
        sum += term / d;
        d += 2;
    }

    2 * sum + (k as i128) * LN2
}

fn pow2(n: i32) -> i128 {
    let mut r = SCALE;
    let mut k = 0;
    while k < n {
        r *= 2;
        k += 1;
    }
    r
}

/// Exponential of a Q9 value, returned in Q9.
///
/// `x` is split into an integer power of two and a remainder in
/// `[-ln(2)/2, ln(2)/2]`; the remainder is evaluated with the Taylor series of
/// `e^r`, which converges in a handful of terms over that range.
pub fn exp_q9(x: i128) -> i128 {
    let x = clamp(x, -40 * SCALE, 20 * SCALE);

    // Reduce to e^r * 2^n with |r| as small as possible. Integer division
    // truncates towards zero, so the remainder is nudged into range by hand.
    let mut n = x / LN2;
    let mut r = x - n * LN2;
    if r > LN2 / 2 {
        n += 1;
        r -= LN2;
    } else if r < -LN2 / 2 {
        n -= 1;
        r += LN2;
    }

    let mut term = SCALE;
    let mut sum = SCALE;
    let mut i: i128 = 1;
    while i <= 18 {
        term = (term * r) / SCALE / i;
        sum += term;
        i += 1;
    }

    // `sum` and `pow2` are both Q9, so the rescale is explicit.
    if n >= 0 {
        (sum * pow2(n as i32)) / SCALE
    } else {
        (sum * SCALE) / pow2((-n) as i32)
    }
}

/// Standard normal probability density, Q9.
pub fn norm_pdf_q9(x: i128) -> i128 {
    let x = clamp(x, -12 * SCALE, 12 * SCALE);
    let x2 = (x * x) / SCALE;
    let e = exp_q9(-x2 / 2);
    (e * SCALE) / SQRT_2PI
}

/// `Phi(x)` for `x <= 0`.
///
/// Abramowitz & Stegun 26.2.17 evaluates
/// `U(y) = phi(y) * (b1 t + ... + b5 t^5)`, `t = 1 / (1 + p y)`, for `y >= 0`.
/// That approximation is the *lower* tail, i.e. `U(y) = Phi(-y) = 1 - Phi(y)`.
/// Since `phi` is even, feeding it `-x` makes it usable for the negative half
/// line directly, with no symmetry step.
fn cdf_negative_q9(x: i128) -> i128 {
    let y = -x;

    // t = 1 / (1 + p * y). `denom` is Q9 and the result has to be Q9 too, so
    // the numerator carries an extra SCALE.
    let denom = SCALE + (NORM_P * y) / SCALE;
    if denom <= 0 {
        return SCALE;
    }
    let t = (SCALE * SCALE) / denom;

    let t2 = (t * t) / SCALE;
    let t3 = (t2 * t) / SCALE;
    let t4 = (t3 * t) / SCALE;
    let t5 = (t4 * t) / SCALE;

    let poly = (t * B1
        + t2 * B2
        + t3 * B3
        + t4 * B4
        + t5 * B5)
        / SCALE;

    clamp((norm_pdf_q9(y) * poly) / SCALE, 0, SCALE)
}

/// Standard normal cumulative distribution function, Q9.
///
/// `Phi(x) = 1 - Phi(-x)` recovers the positive half line from the negative one.
pub fn norm_cdf_q9(x: i128) -> i128 {
    let x = clamp(x, -12 * SCALE, 12 * SCALE);
    if x >= 0 {
        clamp(SCALE - cdf_negative_q9(-x), 0, SCALE)
    } else {
        cdf_negative_q9(x)
    }
}

/// `d1` of the Black-Scholes model. All arguments and the result are Q9.
pub fn bs_d1(spot: i128, strike: i128, t_years: i128, vol: i128, rate: i128) -> i128 {
    if spot <= 0 || strike <= 0 {
        return 0;
    }
    let sigma_sqrt_t = (vol * sqrt_q9(t_years)) / SCALE;
    if sigma_sqrt_t <= 0 {
        return 0;
    }
    let vsq = (vol * vol) / SCALE;
    let rdt = (rate * t_years) / SCALE;
    let lns = ln_q9(spot) - ln_q9(strike);
    let numerator = lns + rdt + vsq / 2;
    clamp((numerator * SCALE) / sigma_sqrt_t, -12 * SCALE, 12 * SCALE)
}

/// Discount factor `exp(-rate * t)`, Q9.
pub fn discount_q9(rate: i128, t_years: i128) -> i128 {
    exp_q9(-((rate * t_years) / SCALE))
}

/// Undiscounted Black-Scholes price of a call or put, Q9.
///
/// `is_call` selects the payoff; American options are priced with the European
/// formula, which is the standard lower bound for early-exercisable contracts.
pub fn bs_price(spot: i128, strike: i128, t_years: i128, vol: i128, rate: i128, is_call: bool) -> i128 {
    if spot <= 0 || strike <= 0 || t_years <= 0 {
        return 0;
    }
    let d1 = bs_d1(spot, strike, t_years, vol, rate);
    let sigma_sqrt_t = (vol * sqrt_q9(t_years)) / SCALE;
    let d2 = clamp(d1 - sigma_sqrt_t, -12 * SCALE, 12 * SCALE);
    let df = discount_q9(rate, t_years);

    let nd1 = norm_cdf_q9(d1);
    let nd2 = norm_cdf_q9(d2);

    let price = if is_call {
        (spot * nd1) / SCALE - ((strike * df) / SCALE) * nd2 / SCALE
    } else {
        ((strike * df) / SCALE) * (SCALE - nd2) / SCALE - (spot * (SCALE - nd1)) / SCALE
    };

    clamp(price, 0, spot + strike)
}

/// The five "Greeks" of the Black-Scholes model.
///
/// * `delta` — option value change per 1.00 move in spot
/// * `gamma` — delta change per 1.00 move in spot
/// * `vega`  — option value change per 1% move in volatility
/// * `theta` — option value change per day of elapsed time
/// * `rho`   — option value change per 1% move in the risk-free rate
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Greeks {
    pub delta: i128,
    pub gamma: i128,
    pub vega: i128,
    pub theta: i128,
    pub rho: i128,
}

/// Black-Scholes Greeks in Q9. Returns zeroes for a degenerate contract.
pub fn bs_greeks(
    spot: i128,
    strike: i128,
    t_years: i128,
    vol: i128,
    rate: i128,
    is_call: bool,
) -> Greeks {
    if spot <= 0 || strike <= 0 || t_years <= 0 {
        return Greeks { delta: 0, gamma: 0, vega: 0, theta: 0, rho: 0 };
    }

    let sqrt_t = sqrt_q9(t_years);
    let sigma_sqrt_t = (vol * sqrt_t) / SCALE;
    if sigma_sqrt_t <= 0 {
        return Greeks { delta: 0, gamma: 0, vega: 0, theta: 0, rho: 0 };
    }

    let d1 = bs_d1(spot, strike, t_years, vol, rate);
    let d2 = clamp(d1 - sigma_sqrt_t, -12 * SCALE, 12 * SCALE);
    let df = discount_q9(rate, t_years);

    let nd1 = norm_cdf_q9(d1);
    let pdf = norm_pdf_q9(d1);

    let delta = if is_call { nd1 } else { nd1 - SCALE };

    // gamma = pdf(d1) / (S * sigma * sqrt(T))
    let denom = (spot * sigma_sqrt_t) / SCALE;
    let gamma = if denom > 0 { (pdf * SCALE) / denom } else { 0 };

    // vega per 1% move in volatility: S * pdf(d1) * sqrt(T) / 100
    let vega = ((spot * pdf) / SCALE) * sqrt_t / SCALE / 100;

    // theta per day. The first term is the -S*phi(d1)*sigma/(2*sqrt(T)) decay,
    // the second is the risk-free rate carried by the option premium.
    let decay = ((spot * pdf) / SCALE) * vol / (2 * sqrt_t);
    let carry_q9 = (rate * df) / SCALE;
    let carry = (carry_q9 * strike) / SCALE;
    let theta_year = if is_call {
        -decay - carry * norm_cdf_q9(d2) / SCALE
    } else {
        -decay + carry * (SCALE - norm_cdf_q9(d2)) / SCALE
    };
    let theta = theta_year / 365;

    // rho per 1% move in the risk-free rate: K * T * exp(-rT) * N(+-d2) / 100.
    // The time factor is T itself, not r*T.
    let k_df = (strike * df) / SCALE;
    let rho_year = if is_call {
        (k_df * t_years / SCALE) * norm_cdf_q9(d2) / SCALE
    } else {
        -(k_df * t_years / SCALE) * (SCALE - norm_cdf_q9(d2)) / SCALE
    };
    let rho = rho_year / 100;

    Greeks { delta, gamma, vega, theta, rho }
}
