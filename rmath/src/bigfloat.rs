//! Arbitrary-precision decimal reals (`N[expr, prec]`).
//!
//! The kernel's machine reals are `f64`; that caps `N[expr, prec]` at ~16
//! significant digits. This module adds an exact-length decimal float built on
//! the in-tree `BigInt`, so `N[Pi, 50]` can return 50 real digits.
//!
//! Representation: `(-1)^neg * mag * 10^-scale`, with `mag` a non-negative
//! `BigInt` and `scale` a signed decimal exponent. Trailing zeros are stripped
//! so a value has one canonical form, and `prec` records the significant
//! decimal digits the value was produced at (used for display padding).
//!
//! The elementary constants and functions are computed with a few guard digits
//! and then rounded to `prec`: `pi` by Machin's formula, `e`/`exp` by the
//! exponential series, `ln` by the `atanh` series with square-root reduction,
//! `sin`/`cos` by Taylor series after range reduction, and `atan` by its series
//! with the standard argument transformations.

use crate::bigint::BigInt;
use crate::rational::Rat;
use std::cmp::Ordering;

fn pow10(k: usize) -> BigInt {
    BigInt::from_u64(10).pow(k as u32)
}

/// Number of decimal digits of a non-negative magnitude (0 has one).
fn digits_of(n: &BigInt) -> usize {
    if n.is_zero() {
        return 1;
    }
    n.to_string_dec().len()
}

/// Round `a / b` to the nearest integer (a ≥ 0, b > 0).
fn div_round(a: &BigInt, b: &BigInt) -> BigInt {
    let (q, r) = a.div_rem(b);
    if r.mul_small(2).cmp(b) != Ordering::Less {
        q.add(&BigInt::one())
    } else {
        q
    }
}

/// Fixed-point multiply: `(a * b) / 10^w`, truncated. Series are evaluated at a
/// fixed scale so a term underflows to zero instead of shrinking forever.
fn fmul(a: &BigInt, b: &BigInt, w: usize) -> BigInt {
    a.mul(b).div_rem(&pow10(w)).0
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BigFloat {
    pub neg: bool,
    /// Non-negative magnitude.
    pub mag: BigInt,
    /// value = (-1)^neg * mag * 10^-scale
    pub scale: i64,
    /// Significant decimal digits (for display padding).
    pub prec: usize,
}

impl BigFloat {
    pub fn zero(prec: usize) -> BigFloat {
        BigFloat { neg: false, mag: BigInt::zero(), scale: 0, prec }
    }

    /// Strip trailing zeros and normalize the sign of zero.
    fn normalize(mut self) -> BigFloat {
        if self.mag.is_zero() {
            self.neg = false;
            self.scale = 0;
            return self;
        }
        loop {
            let (q, r) = self.mag.div_rem_small(10);
            if r != 0 {
                break;
            }
            self.mag = q;
            self.scale -= 1;
        }
        self
    }

    pub fn from_bigint(n: &BigInt, prec: usize) -> BigFloat {
        BigFloat { neg: n.is_neg(), mag: n.clone().abs(), scale: 0, prec }.normalize().round_to(prec)
    }

    pub fn from_i64(v: i64, prec: usize) -> BigFloat {
        BigFloat::from_bigint(&BigInt::from_i64(v), prec)
    }

    /// The decimal expansion of an exact rational, rounded to `prec` digits.
    pub fn from_rat(r: &Rat, prec: usize) -> BigFloat {
        if r.is_zero() {
            return BigFloat::zero(prec);
        }
        let w = prec + 10;
        let num = r.num.clone().abs().mul(&pow10(w));
        let mag = div_round(&num, &r.den.clone().abs());
        BigFloat {
            neg: r.num.is_neg() != r.den.is_neg(),
            mag,
            scale: w as i64,
            prec,
        }
        .normalize()
        .round_to(prec)
    }

    /// Round to `prec` significant decimal digits.
    pub fn round_to(mut self, prec: usize) -> BigFloat {
        self.prec = prec;
        if self.mag.is_zero() {
            return self;
        }
        let d = digits_of(&self.mag);
        if d <= prec {
            return self;
        }
        let drop = d - prec;
        self.mag = div_round(&self.mag, &pow10(drop));
        self.scale -= drop as i64;
        self.normalize()
    }

    pub fn is_zero(&self) -> bool {
        self.mag.is_zero()
    }

    pub fn to_f64(&self) -> f64 {
        let m = self.mag.to_f64();
        let e = (self.scale as i32).clamp(-300, 300);
        let v = m * 10f64.powi(-e);
        if self.neg {
            -v
        } else {
            v
        }
    }

    /// The value scaled by `10^w`, rounded to the nearest integer.
    pub fn to_scaled(&self, w: usize) -> BigInt {
        let n = if (w as i64) >= self.scale {
            self.mag.mul(&pow10((w as i64 - self.scale) as usize))
        } else {
            div_round(&self.mag, &pow10((self.scale - w as i64) as usize))
        };
        if self.neg {
            n.neg()
        } else {
            n
        }
    }

    pub fn from_scaled(n: &BigInt, w: usize, prec: usize) -> BigFloat {
        BigFloat { neg: n.is_neg(), mag: n.clone().abs(), scale: w as i64, prec }.normalize().round_to(prec)
    }

    /// Decimal string with `prec` significant digits (padded like Mathematica).
    pub fn to_string(&self) -> String {
        if self.mag.is_zero() {
            if self.prec <= 1 {
                return "0".to_string();
            }
            return format!("0.{}", "0".repeat(self.prec - 1));
        }
        let d = digits_of(&self.mag);
        let prec = self.prec.max(1);
        // Pad to `prec` significant digits without changing the value.
        let (mut mag, mut scale) = (self.mag.clone(), self.scale);
        if d < prec {
            mag = mag.mul(&pow10(prec - d));
            scale += (prec - d) as i64;
        }
        let s = mag.to_string_dec();
        let body = if scale <= 0 {
            format!("{}{}", s, "0".repeat((-scale) as usize))
        } else if (s.len() as i64) > scale {
            let cut = s.len() - scale as usize;
            format!("{}.{}", &s[..cut], &s[cut..])
        } else {
            format!("0.{}{}", "0".repeat(scale as usize - s.len()), s)
        };
        if self.neg {
            format!("-{}", body)
        } else {
            body
        }
    }

    pub fn neg(&self) -> BigFloat {
        let mut r = self.clone();
        if !r.mag.is_zero() {
            r.neg = !r.neg;
        }
        r
    }

    pub fn cmp(&self, o: &BigFloat) -> Ordering {
        if self.neg != o.neg {
            // A negative is smaller, unless both are zero.
            if self.is_zero() && o.is_zero() {
                return Ordering::Equal;
            }
            return if self.neg { Ordering::Less } else { Ordering::Greater };
        }
        let s = self.scale.max(o.scale);
        let a = self.mag.mul(&pow10((s - self.scale) as usize));
        let b = o.mag.mul(&pow10((s - o.scale) as usize));
        let ord = a.cmp(&b);
        if self.neg {
            ord.reverse()
        } else {
            ord
        }
    }

    fn add_signed(&self, o: &BigFloat, subtract: bool) -> BigFloat {
        let prec = self.prec.min(o.prec).max(1);
        let s = self.scale.max(o.scale);
        let a = self.mag.mul(&pow10((s - self.scale) as usize));
        let b = o.mag.mul(&pow10((s - o.scale) as usize));
        let (neg, mag) = match (self.neg, o.neg != subtract) {
            (false, false) => (false, a.add(&b)),
            (true, true) => (true, a.add(&b)),
            (false, true) => match a.cmp(&b) {
                Ordering::Less => (true, b.sub(&a)),
                _ => (false, a.sub(&b)),
            },
            (true, false) => match b.cmp(&a) {
                Ordering::Less => (true, a.sub(&b)),
                _ => (false, b.sub(&a)),
            },
        };
        BigFloat { neg, mag, scale: s, prec }.normalize().round_to(prec)
    }

    pub fn add(&self, o: &BigFloat) -> BigFloat {
        self.add_signed(o, false)
    }

    pub fn sub(&self, o: &BigFloat) -> BigFloat {
        self.add_signed(o, true)
    }

    pub fn mul(&self, o: &BigFloat) -> BigFloat {
        let prec = self.prec.min(o.prec).max(1);
        BigFloat {
            neg: self.neg != o.neg,
            mag: self.mag.mul(&o.mag),
            scale: self.scale + o.scale,
            prec,
        }
        .normalize()
        .round_to(prec)
    }

    pub fn div(&self, o: &BigFloat) -> Option<BigFloat> {
        if o.is_zero() {
            return None;
        }
        let prec = self.prec.min(o.prec).max(1);
        let w = prec + 10;
        // Align both operands to a common scale first: scaling the numerator by
        // 10^w alone loses every digit when the divisor carries more fractional
        // digits than the dividend (which is what atan's argument reduction does).
        let s = self.scale.max(o.scale);
        let a = self.mag.mul(&pow10((s - self.scale) as usize));
        let b = o.mag.mul(&pow10((s - o.scale) as usize));
        let mag = div_round(&a.mul(&pow10(w)), &b);
        Some(
            BigFloat {
                neg: self.neg != o.neg,
                mag,
                scale: w as i64,
                prec,
            }
            .normalize()
            .round_to(prec),
        )
    }

    /// Square root, rounded to `prec` digits. Negative values have no real root.
    pub fn sqrt(&self, prec: usize) -> Option<BigFloat> {
        if self.neg {
            return None;
        }
        if self.is_zero() {
            return Some(BigFloat::zero(prec));
        }
        let w = prec + 10;
        let d = digits_of(&self.mag);
        // Choose the scaling so the scaled integer has ~2w digits.
        let target = 2 * w;
        let mut total = target as i64 - d as i64;
        if (total + self.scale) % 2 != 0 {
            total += 1;
        }
        if total < -self.scale {
            total = -self.scale;
        }
        let k = (total + self.scale) / 2;
        let n = self.mag.mul(&pow10(total.max(0) as usize));
        let root = n.isqrt();
        Some(BigFloat { neg: false, mag: root, scale: k, prec }.normalize().round_to(prec))
    }

    /// The scale factor `10^w` as a `BigInt`.
    fn one_scaled(w: usize) -> BigInt {
        pow10(w)
    }

    /// `atan(1/n)` scaled by `10^w`.
    fn atan_inv_scaled(n: u64, w: usize) -> BigInt {
        let scale = BigFloat::one_scaled(w);
        let nn = BigInt::from_u64(n).mul(&BigInt::from_u64(n));
        let mut term = scale.div_rem(&BigInt::from_u64(n)).0;
        let mut sum = term.clone();
        let mut k: u64 = 1;
        loop {
            term = term.div_rem(&nn).0;
            let t = term.div_rem(&BigInt::from_u64(2 * k + 1)).0;
            if t.is_zero() {
                break;
            }
            sum = if k % 2 == 1 { sum.sub(&t) } else { sum.add(&t) };
            k += 1;
            if k > 200_000 {
                break;
            }
        }
        sum
    }

    /// π to `prec` significant digits (Machin's formula).
    pub fn pi(prec: usize) -> BigFloat {
        let w = prec + 15;
        let a = BigFloat::atan_inv_scaled(5, w);
        let b = BigFloat::atan_inv_scaled(239, w);
        let mag = a.mul_small(16).sub(&b.mul_small(4));
        BigFloat { neg: false, mag, scale: w as i64, prec }.normalize().round_to(prec)
    }

    /// e to `prec` significant digits (the exponential series at 1).
    pub fn e(prec: usize) -> BigFloat {
        let w = prec + 15;
        let mut term = BigFloat::one_scaled(w);
        let mut sum = term.clone();
        let mut k: u64 = 1;
        loop {
            term = term.div_rem(&BigInt::from_u64(k)).0;
            if term.is_zero() {
                break;
            }
            sum = sum.add(&term);
            k += 1;
            if k > 200_000 {
                break;
            }
        }
        BigFloat { neg: false, mag: sum, scale: w as i64, prec }.normalize().round_to(prec)
    }

    fn frac(n: i64, d: i64, prec: usize) -> BigFloat {
        BigFloat::from_rat(&Rat::new(BigInt::from_i64(n), BigInt::from_i64(d)), prec)
    }

    /// Round to the nearest integer (as a BigFloat).
    fn round_int(&self, w: usize) -> BigFloat {
        BigFloat::from_scaled(&self.to_scaled(0), 0, w)
    }

    pub fn abs(&self) -> BigFloat {
        let mut r = self.clone();
        r.neg = false;
        r
    }

    /// exp(self): halve the argument, Taylor at a fixed scale, then square back.
    pub fn exp(&self, prec: usize) -> BigFloat {
        let w = prec + 16;
        let scale = pow10(w);
        let half = scale.div_rem_small(2).0;
        let mut x = self.to_scaled(w);
        let mut m = 0u32;
        while x.clone().abs().cmp(&half) == Ordering::Greater {
            x = x.div_rem_small(2).0;
            m += 1;
            if m > 64 {
                break;
            }
        }
        let mut term = scale.clone();
        let mut sum = term.clone();
        let mut k: u64 = 1;
        loop {
            term = term.div_rem(&BigInt::from_u64(k)).0;
            term = fmul(&term, &x, w);
            if term.is_zero() {
                break;
            }
            sum = sum.add(&term);
            k += 1;
            if k > 100_000 {
                break;
            }
        }
        for _ in 0..m {
            sum = fmul(&sum, &sum, w);
        }
        BigFloat::from_scaled(&sum, w, prec)
    }

    /// ln(self) for self > 0: square-root reduction near 1, then the atanh series.
    pub fn ln(&self, prec: usize) -> Option<BigFloat> {
        if self.is_zero() || self.neg {
            return None;
        }
        let w = prec + 16;
        let one = BigFloat::from_i64(1, w);
        let lo = BigFloat::frac(4, 5, w);
        let hi = BigFloat::frac(5, 4, w);
        let mut x = self.clone().round_to(w);
        let mut k = 0u32;
        while x.cmp(&hi) == Ordering::Greater || x.cmp(&lo) == Ordering::Less {
            x = x.sqrt(w)?;
            k += 1;
            if k > 200 {
                break;
            }
        }
        let z = x.sub(&one).div(&x.add(&one))?;
        let zs = z.to_scaled(w);
        let z2 = fmul(&zs, &zs, w);
        let mut term = zs.clone();
        let mut sum = zs.clone();
        let mut i: u64 = 3;
        loop {
            term = fmul(&term, &z2, w);
            let t = term.div_rem(&BigInt::from_u64(i)).0;
            if t.is_zero() {
                break;
            }
            sum = sum.add(&t);
            i += 2;
            if i > 100_000 {
                break;
            }
        }
        // ln(x) = 2 * atanh(z), and each square root contributes another 2.
        let mut factor = BigInt::from_u64(2);
        for _ in 0..k {
            factor = factor.mul_small(2);
        }
        Some(BigFloat::from_scaled(&sum.mul(&factor), w, prec))
    }

    /// Reduce an angle to [-π, π].
    fn reduce_angle(&self, w: usize) -> BigFloat {
        let two_pi = BigFloat::pi(w).mul(&BigFloat::from_i64(2, w));
        match self.div(&two_pi) {
            Some(q) => self.sub(&q.round_int(w).mul(&two_pi)),
            None => self.clone(),
        }
    }

    pub fn sin(&self, prec: usize) -> BigFloat {
        let w = prec + 16;
        let x = self.reduce_angle(w).to_scaled(w);
        let x2 = fmul(&x, &x, w);
        let mut term = x.clone();
        let mut sum = x.clone();
        let mut k: u64 = 1;
        loop {
            let denom = BigInt::from_u64((2 * k) * (2 * k + 1));
            term = fmul(&term, &x2, w).div_rem(&denom).0.neg();
            if term.is_zero() {
                break;
            }
            sum = sum.add(&term);
            k += 1;
            if k > 100_000 {
                break;
            }
        }
        BigFloat::from_scaled(&sum, w, prec)
    }

    pub fn cos(&self, prec: usize) -> BigFloat {
        let w = prec + 16;
        let x = self.reduce_angle(w).to_scaled(w);
        let x2 = fmul(&x, &x, w);
        let mut term = pow10(w);
        let mut sum = term.clone();
        let mut k: u64 = 1;
        loop {
            let denom = BigInt::from_u64((2 * k - 1) * (2 * k));
            term = fmul(&term, &x2, w).div_rem(&denom).0.neg();
            if term.is_zero() {
                break;
            }
            sum = sum.add(&term);
            k += 1;
            if k > 100_000 {
                break;
            }
        }
        BigFloat::from_scaled(&sum, w, prec)
    }

    pub fn tan(&self, prec: usize) -> Option<BigFloat> {
        let w = prec + 16;
        self.sin(w).div(&self.cos(w)).map(|v| v.round_to(prec))
    }

    /// atan(self): invert |x| > 1, halve until small, then the series.
    pub fn atan(&self, prec: usize) -> BigFloat {
        let w = prec + 16;
        let one = BigFloat::from_i64(1, w);
        let neg = self.neg;
        let mut a = self.abs().round_to(w);
        let mut invert = false;
        if a.cmp(&one) == Ordering::Greater {
            invert = true;
            a = one.div(&a).unwrap();
        }
        let small = BigFloat::frac(1, 10, w);
        let mut k = 0u32;
        while a.cmp(&small) == Ordering::Greater {
            let s = one.add(&a.mul(&a)).sqrt(w).unwrap();
            a = a.div(&one.add(&s)).unwrap();
            k += 1;
            if k > 200 {
                break;
            }
        }
        let a2 = fmul(&a.to_scaled(w), &a.to_scaled(w), w);
        let av = a.to_scaled(w);
        let mut term = av.clone();
        let mut sum = av.clone();
        let mut i: u64 = 3;
        loop {
            term = fmul(&term, &a2, w).neg();
            let t = term.div_rem(&BigInt::from_u64(i)).0;
            if t.is_zero() {
                break;
            }
            sum = sum.add(&t);
            i += 2;
            if i > 100_000 {
                break;
            }
        }
        let mut factor = BigInt::from_u64(1);
        for _ in 0..k {
            factor = factor.mul_small(2);
        }
        let mut total = sum.mul(&factor);
        if invert {
            let half_pi = BigFloat::pi(w).div(&BigFloat::from_i64(2, w)).unwrap();
            total = half_pi.to_scaled(w).sub(&total);
        }
        if neg {
            total = total.neg();
        }
        BigFloat::from_scaled(&total, w, prec)
    }

    pub fn sinh(&self, prec: usize) -> BigFloat {
        let w = prec + 16;
        self.exp(w)
            .sub(&self.neg().exp(w))
            .div(&BigFloat::from_i64(2, w))
            .unwrap()
            .round_to(prec)
    }

    pub fn cosh(&self, prec: usize) -> BigFloat {
        let w = prec + 16;
        self.exp(w)
            .add(&self.neg().exp(w))
            .div(&BigFloat::from_i64(2, w))
            .unwrap()
            .round_to(prec)
    }

    pub fn tanh(&self, prec: usize) -> BigFloat {
        let w = prec + 16;
        self.sinh(w).div(&self.cosh(w)).unwrap().round_to(prec)
    }

    /// `self^o` for `self > 0`, via `exp(o ln self)`.
    pub fn powf(&self, o: &BigFloat, prec: usize) -> Option<BigFloat> {
        if self.is_zero() {
            return Some(if o.is_zero() {
                BigFloat::from_i64(1, prec)
            } else {
                BigFloat::zero(prec)
            });
        }
        if self.neg {
            return None;
        }
        let w = prec + 16;
        let l = self.ln(w)?;
        Some(o.clone().round_to(w).mul(&l).exp(prec))
    }
}

impl std::fmt::Display for BigFloat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_integers_and_rationals() {
        assert_eq!(BigFloat::from_i64(42, 20).to_string(), "42.000000000000000000");
        assert_eq!(BigFloat::from_i64(-7, 5).to_string(), "-7.0000");
        let third = BigFloat::from_rat(&Rat::new(BigInt::from_i64(1), BigInt::from_i64(3)), 20);
        assert_eq!(third.to_string(), "0.33333333333333333333");
        let half = BigFloat::from_rat(&Rat::new(BigInt::from_i64(1), BigInt::from_i64(2)), 20);
        assert_eq!(half.to_string(), "0.50000000000000000000");
    }

    #[test]
    fn pi_and_e_digits() {
        let pi = BigFloat::pi(50);
        assert_eq!(pi.to_string(), "3.1415926535897932384626433832795028841971693993751");
        let e = BigFloat::e(50);
        // 50 significant digits, rounded (decimal 50 is 5 followed by 9).
        assert_eq!(e.to_string(), "2.7182818284590452353602874713526624977572470937000");
    }

    #[test]
    fn sqrt_digits() {
        let two = BigFloat::from_i64(2, 60);
        let s = two.sqrt(40).unwrap();
        assert_eq!(s.to_string(), "1.414213562373095048801688724209698078570");
    }

    #[test]
    fn arithmetic_is_accurate() {
        let pi = BigFloat::pi(30);
        let sum = pi.add(&BigFloat::from_i64(1, 30));
        assert!(sum.to_string().starts_with("4.14159265358979323846264338328"));
        let sq = pi.mul(&pi);
        assert!(sq.to_string().starts_with("9.86960440108935861883449099988"));
        let one = BigFloat::from_i64(1, 30);
        let inv = one.div(&BigFloat::from_i64(7, 30)).unwrap();
        assert!(inv.to_string().starts_with("0.142857142857142857142857142857"));
    }

    #[test]
    fn comparison() {
        let a = BigFloat::from_i64(2, 20);
        let b = BigFloat::from_i64(3, 20);
        assert_eq!(a.cmp(&b), Ordering::Less);
        assert_eq!(b.cmp(&a), Ordering::Greater);
        assert_eq!(a.cmp(&a.clone()), Ordering::Equal);
        let neg = BigFloat::from_i64(-5, 20);
        assert_eq!(neg.cmp(&a), Ordering::Less);
    }

    #[test]
    fn transcendental_digits() {
        let one = BigFloat::from_i64(1, 50);
        assert_eq!(one.exp(40).to_string(), "2.718281828459045235360287471352662497757");
        let two = BigFloat::from_i64(2, 50);
        assert_eq!(
            two.ln(40).unwrap().to_string(),
            "0.6931471805599453094172321214581765680755"
        );
        assert_eq!(one.sin(40).to_string(), "0.8414709848078965066525023216302989996226");
        assert_eq!(one.cos(40).to_string(), "0.5403023058681397174009366074429766037323");
        assert_eq!(one.atan(40).to_string(), "0.7853981633974483096156608458198757210493");
        // ln is the inverse of exp, and sin^2 + cos^2 = 1, to working precision.
        let e2 = BigFloat::from_i64(2, 60).exp(50);
        let back = e2.ln(50).unwrap();
        assert!(back.sub(&BigFloat::from_i64(2, 50).round_to(50)).abs().to_string()
            .starts_with("0.000000000000000000000000000000000000000000000000"));
        let s = one.sin(50);
        let c = one.cos(50);
        let id = s.mul(&s).add(&c.mul(&c)).round_to(40);
        assert_eq!(id.to_string(), "1.000000000000000000000000000000000000000");
    }
}
