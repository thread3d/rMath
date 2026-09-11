//! Exact rational numbers with BigInt numerator/denominator.
//! Normalized invariant: den > 0, gcd(|num|, den) = 1, and zero is 0/1.

use crate::bigint::BigInt;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug)]
pub struct Rat {
    pub num: BigInt,
    pub den: BigInt,
}

impl Rat {
    pub fn new(num: BigInt, den: BigInt) -> Rat {
        if den.is_zero() {
            panic!("Rat with zero denominator");
        }
        let (num, den) = if den.sign < 0 { (num.neg(), den.neg()) } else { (num, den) };
        if num.is_zero() {
            return Rat { num: BigInt::zero(), den: BigInt::one() };
        }
        let g = num.gcd(&den);
        let num = num.div_rem(&g).0;
        let den = den.div_rem(&g).0;
        Rat { num, den }
    }
    pub fn from_i64(n: i64) -> Rat {
        Rat { num: BigInt::from_i64(n), den: BigInt::one() }
    }

    pub fn is_int(&self) -> bool {
        self.den.is_one()
    }
    pub fn is_zero(&self) -> bool {
        self.num.is_zero()
    }
    pub fn is_neg(&self) -> bool {
        self.num.sign < 0
    }
    pub fn is_pos(&self) -> bool {
        self.num.sign > 0
    }

    pub fn neg(self) -> Rat {
        Rat { num: self.num.neg(), den: self.den }
    }
    pub fn abs(self) -> Rat {
        Rat { num: self.num.abs(), den: self.den }
    }
    pub fn add(&self, o: &Rat) -> Rat {
        Rat::new(self.num.mul(&o.den).add(&o.num.mul(&self.den)), self.den.mul(&o.den))
    }
    pub fn sub(&self, o: &Rat) -> Rat {
        self.add(&o.clone().neg())
    }
    pub fn mul(&self, o: &Rat) -> Rat {
        Rat::new(self.num.mul(&o.num), self.den.mul(&o.den))
    }
    pub fn div(&self, o: &Rat) -> Rat {
        if o.is_zero() {
            panic!("division by zero rational");
        }
        // a / (n/d) = a * d / n; Rat::new normalizes sign.
        Rat::new(self.num.mul(&o.den), self.den.mul(&o.num))
    }
    pub fn pow_i64(&self, e: i64) -> Rat {
        if e >= 0 {
            Rat::new(self.num.pow(e as u32), self.den.pow(e as u32))
        } else if self.is_zero() {
            panic!("0 to a negative power");
        } else {
            Rat::new(self.den.pow((-e) as u32), self.num.pow((-e) as u32))
        }
    }

    pub fn cmp(&self, o: &Rat) -> Ordering {
        self.num.mul(&o.den).cmp(&o.num.mul(&self.den))
    }
    pub fn to_f64(&self) -> f64 {
        // Scale so the quotient lands in f64 range with decent rounding.
        let a = self.num.clone().abs();
        let b = self.den.clone();
        if a.is_zero() {
            return 0.0;
        }
        let shift = 64i64 - (a.bit_len() as i64 - b.bit_len() as i64);
        let f = if shift > 0 {
            let q = a.shl(shift as usize).div_rem(&b).0;
            q.to_f64() / 2f64.powi(shift as i32)
        } else {
            let q = a.div_rem(&b.shl((-shift) as usize)).0;
            q.to_f64() * 2f64.powi((-shift) as i32)
        };
        if self.num.sign < 0 {
            -f
        } else {
            f
        }
    }
}



impl PartialEq for Rat {
    fn eq(&self, o: &Rat) -> bool {
        self.num == o.num && self.den == o.den
    }
}
impl Eq for Rat {}
impl Hash for Rat {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.num.hash(h);
        self.den.hash(h);
    }
}
impl PartialOrd for Rat {
    fn partial_cmp(&self, o: &Rat) -> Option<Ordering> {
        Some(Rat::cmp(self, o))
    }
}
impl Ord for Rat {
    fn cmp(&self, o: &Rat) -> Ordering {
        Rat::cmp(self, o)
    }
}
impl fmt::Display for Rat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_int() {
            fmt::Display::fmt(&self.num, f)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(n: i64, d: i64) -> Rat {
        Rat::new(BigInt::from_i64(n), BigInt::from_i64(d))
    }

    #[test]
    fn normalization() {
        assert_eq!(r(2, 4).to_string(), "1/2");
        assert_eq!(r(4, 2).to_string(), "2");
        assert_eq!(r(-3, 6).to_string(), "-1/2");
        assert_eq!(r(3, -6).to_string(), "-1/2");
        assert_eq!(r(0, 5).to_string(), "0");
    }

    #[test]
    fn arithmetic() {
        assert_eq!(r(1, 2).add(&r(1, 3)).to_string(), "5/6");
        assert_eq!(r(1, 2).sub(&r(1, 2)).to_string(), "0");
        assert_eq!(r(2, 3).mul(&r(3, 4)).to_string(), "1/2");
        assert_eq!(r(1, 2).div(&r(3, 4)).to_string(), "2/3");
        assert_eq!(r(2, 3).pow_i64(-2).to_string(), "9/4");
    }

    #[test]
    fn compare_and_float() {
        assert!(r(1, 3) < r(1, 2));
        assert_eq!(r(1, 3).to_f64(), 1.0 / 3.0);
        assert_eq!(r(-1, 8).to_f64(), -0.125);
    }

    #[test]
    fn big_values() {
        let big = Rat::new(
            BigInt::from_str_dec("10000000000000000000000000000").unwrap(),
            BigInt::from_str_dec("3").unwrap(),
        );
        assert_eq!(
            big.mul(&r(3, 1)).num.to_string(),
            "10000000000000000000000000000"
        );
        assert!((big.to_f64() - 3.3333333333333333e27).abs() < 1e13);
    }
}