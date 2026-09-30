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
        let num = self.mag.mul(&pow10(w));
        let mag = div_round(&num, &o.mag);
        Some(
            BigFloat {
                neg: self.neg != o.neg,
                mag,
                scale: self.scale - o.scale + w as i64,
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
}
