//! Arbitrary-precision signed integer. Base 2^32 little-endian limbs.
//! Zero external dependencies. Normalized invariant: `mag` has no leading
//! zero limbs (empty iff zero) and `sign` is 0 iff the value is zero.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug)]
pub struct BigInt {
    /// -1, 0, or 1
    pub sign: i8,
    /// little-endian base-2^32 limbs, trimmed (empty iff zero)
    pub mag: Vec<u32>,
}

const BASE: u64 = 1 << 32;

pub fn trim_mag(v: &[u32]) -> Vec<u32> {
    let mut end = v.len();
    while end > 0 && v[end - 1] == 0 {
        end -= 1;
    }
    v[..end].to_vec()
}

pub fn cmp_mag(a: &[u32], b: &[u32]) -> Ordering {
    if a.len() != b.len() {
        return a.len().cmp(&b.len());
    }
    for i in (0..a.len()).rev() {
        if a[i] != b[i] {
            return a[i].cmp(&b[i]);
        }
    }
    Ordering::Equal
}

pub fn add_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let mut out = Vec::with_capacity(long.len() + 1);
    let mut carry = 0u64;
    for i in 0..long.len() {
        let mut t = long[i] as u64 + carry;
        if i < short.len() {
            t += short[i] as u64;
        }
        out.push(t as u32);
        carry = t >> 32;
    }
    if carry > 0 {
        out.push(carry as u32);
    }
    out
}

/// a >= b required.
pub fn sub_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    debug_assert!(cmp_mag(a, b) != Ordering::Less);
    let mut out = Vec::with_capacity(a.len());
    let mut borrow = 0i64;
    for i in 0..a.len() {
        let mut t = a[i] as i64 - borrow;
        if i < b.len() {
            t -= b[i] as i64;
        }
        if t < 0 {
            t += 1 << 32;
            borrow = 1;
        } else {
            borrow = 0;
        }
        out.push(t as u32);
    }
    trim_mag(&out)
}

pub fn mul_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut c = vec![0u32; a.len() + b.len()];
    for i in 0..a.len() {
        let ai = a[i] as u64;
        let mut carry = 0u64;
        for j in 0..b.len() {
            let t = ai * b[j] as u64 + c[i + j] as u64 + carry;
            c[i + j] = t as u32;
            carry = t >> 32;
        }
        let mut k = i + b.len();
        while carry > 0 {
            let t = c[k] as u64 + carry;
            c[k] = t as u32;
            carry = t >> 32;
            k += 1;
        }
    }
    trim_mag(&c)
}

pub fn bit_len_mag(a: &[u32]) -> usize {
    if a.is_empty() {
        0
    } else {
        (a.len() - 1) * 32 + (32 - a[a.len() - 1].leading_zeros() as usize)
    }
}

fn bit_get(a: &[u32], i: usize) -> bool {
    let limb = i / 32;
    if limb >= a.len() {
        false
    } else {
        (a[limb] >> (i % 32)) & 1 == 1
    }
}

fn bit_set(v: &mut Vec<u32>, i: usize) {
    let limb = i / 32;
    while v.len() <= limb {
        v.push(0);
    }
    v[limb] |= 1u32 << (i % 32);
}

fn shl1_inplace(v: &mut Vec<u32>) {
    let mut carry = 0u32;
    for limb in v.iter_mut() {
        let nc = *limb >> 31;
        *limb = (*limb << 1) | carry;
        carry = nc;
    }
    if carry != 0 {
        v.push(carry);
    }
}

/// Binary restoring long division on magnitudes. b must be non-zero.
/// Returns (quotient, remainder) magnitudes.
pub fn div_rem_mag(a: &[u32], b: &[u32]) -> (Vec<u32>, Vec<u32>) {
    debug_assert!(!b.is_empty());
    match cmp_mag(a, b) {
        Ordering::Less => return (Vec::new(), a.to_vec()),
        Ordering::Equal => return (vec![1], Vec::new()),
        Ordering::Greater => {}
    }
    let abits = bit_len_mag(a);
    let mut q: Vec<u32> = vec![0; a.len()];
    let mut r: Vec<u32> = Vec::new();
    for i in (0..abits).rev() {
        shl1_inplace(&mut r);
        if bit_get(a, i) {
            if r.is_empty() {
                r.push(1);
            } else {
                r[0] |= 1;
            }
        }
        if cmp_mag(&r, b) != Ordering::Less {
            r = sub_mag(&r, b);
            bit_set(&mut q, i);
        }
    }
    (trim_mag(&q), trim_mag(&r))
}

pub fn gcd_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut a = a.to_vec();
    let mut b = b.to_vec();
    while !b.is_empty() {
        let (_, r) = div_rem_mag(&a, &b);
        a = b;
        b = r;
    }
    a
}

impl BigInt {
    pub fn zero() -> Self {
        BigInt { sign: 0, mag: Vec::new() }
    }
    pub fn one() -> Self {
        BigInt::from_i64(1)
    }
    pub fn from_i64(v: i64) -> Self {
        if v == 0 {
            return BigInt::zero();
        }
        let sign = if v < 0 { -1 } else { 1 };
        let uv = (v as i128).unsigned_abs() as u128;
        let mag = if uv < BASE as u128 {
            vec![uv as u32]
        } else {
            vec![uv as u32, (uv >> 32) as u32]
        };
        BigInt { sign, mag: trim_mag(&mag) }
    }
    pub fn from_i128(v: i128) -> Self {
        if v == 0 {
            return BigInt::zero();
        }
        let sign = if v < 0 { -1 } else { 1 };
        let uv = v.unsigned_abs();
        let mut mag = Vec::new();
        let mut x = uv;
        while x > 0 {
            mag.push(x as u32);
            x >>= 32;
        }
        BigInt { sign, mag }
    }
    pub fn from_str_dec(s: &str) -> Option<BigInt> {
        let (sign, digits) = if let Some(rest) = s.strip_prefix('-') {
            (-1i8, rest)
        } else if let Some(rest) = s.strip_prefix('+') {
            (1i8, rest)
        } else {
            (1i8, s)
        };
        if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let mut n = BigInt::zero();
        for chunk in digits.as_bytes().chunks(9) {
            let cs = std::str::from_utf8(chunk).ok()?;
            let c: u64 = cs.parse().ok()?;
            n = n.mul_small(10u32.pow(chunk.len() as u32)).add(&BigInt::from_i64(c as i64));
        }
        n.sign = if n.mag.is_empty() { 0 } else { sign };
        Some(n)
    }
    pub fn from_radix(s: &str, base: u32) -> Option<BigInt> {
        let (sign, digits) = if let Some(rest) = s.strip_prefix('-') {
            (-1i8, rest)
        } else {
            (1i8, s)
        };
        if digits.is_empty() {
            return None;
        }
        let mut n = BigInt::zero();
        let b = BigInt::from_i64(base as i64);
        for ch in digits.chars() {
            let d = ch.to_digit(36)?;
            if d >= base {
                return None;
            }
            n = n.mul(&b).add(&BigInt::from_i64(d as i64));
        }
        n.sign = if n.mag.is_empty() { 0 } else { sign };
        Some(n)
    }

    pub fn is_zero(&self) -> bool {
        self.sign == 0 || self.mag.is_empty()
    }
    pub fn is_one(&self) -> bool {
        self.sign == 1 && self.mag.len() == 1 && self.mag[0] == 1
    }
    pub fn is_neg(&self) -> bool {
        self.sign < 0
    }
    pub fn is_even(&self) -> bool {
        self.mag.is_empty() || self.mag[0] & 1 == 0
    }
    pub fn is_odd(&self) -> bool {
        !self.is_even()
    }
    fn normalized(mut self) -> BigInt {
        self.mag = trim_mag(&self.mag);
        if self.mag.is_empty() {
            self.sign = 0;
        } else if self.sign == 0 {
            self.sign = 1;
        }
        self
    }

    pub fn neg(self) -> Self {
        BigInt { sign: -self.sign, mag: self.mag }
    }
    pub fn abs(self) -> Self {
        BigInt { sign: self.sign.abs(), mag: self.mag }
    }

    pub fn add(&self, o: &BigInt) -> BigInt {
        if self.sign == 0 {
            return o.clone();
        }
        if o.sign == 0 {
            return self.clone();
        }
        if self.sign == o.sign {
            let mag = add_mag(&self.mag, &o.mag);
            BigInt { sign: self.sign, mag }
        } else {
            match cmp_mag(&self.mag, &o.mag) {
                Ordering::Equal => BigInt::zero(),
                Ordering::Greater => BigInt { sign: self.sign, mag: sub_mag(&self.mag, &o.mag) },
                Ordering::Less => BigInt { sign: o.sign, mag: sub_mag(&o.mag, &self.mag) },
            }
        }
    }
    pub fn sub(&self, o: &BigInt) -> BigInt {
        self.add(&o.clone().neg())
    }
    pub fn mul(&self, o: &BigInt) -> BigInt {
        if self.sign == 0 || o.sign == 0 {
            return BigInt::zero();
        }
        let mag = mul_mag(&self.mag, &o.mag);
        BigInt { sign: self.sign * o.sign, mag }
    }
    pub fn mul_small(&self, m: u32) -> BigInt {
        if self.sign == 0 || m == 0 {
            return BigInt::zero();
        }
        let mut out = Vec::with_capacity(self.mag.len() + 1);
        let mut carry = 0u64;
        for &limb in &self.mag {
            let t = limb as u64 * m as u64 + carry;
            out.push(t as u32);
            carry = t >> 32;
        }
        if carry > 0 {
            out.push(carry as u32);
        }
        BigInt { sign: self.sign, mag: trim_mag(&out) }
    }
    /// Truncated division: q = trunc(a/b), r = a - q*b (sign of r follows a).
    pub fn div_rem(&self, o: &BigInt) -> (BigInt, BigInt) {
        if o.is_zero() {
            panic!("division by zero");
        }
        if self.is_zero() {
            return (BigInt::zero(), BigInt::zero());
        }
        // Single-limb operands: a plain u64 division is far cheaper than the
        // long-division loop, and polynomial coefficient reduction hammers this
        // path (Expand spends ~5% of its time in div_rem_mag).
        if self.mag.len() <= 2 && o.mag.len() <= 2 {
            let mag_u64 = |m: &[u32]| -> u64 {
                match m.len() {
                    0 => 0,
                    1 => m[0] as u64,
                    _ => ((m[1] as u64) << 32) | m[0] as u64,
                }
            };
            let (a, b) = (mag_u64(&self.mag), mag_u64(&o.mag));
            let (qm, rm) = (a / b, a % b);
            let limb = |v: u64| -> Vec<u32> {
                if v == 0 {
                    Vec::new()
                } else if v <= u32::MAX as u64 {
                    vec![v as u32]
                } else {
                    vec![v as u32, (v >> 32) as u32]
                }
            };
            let q = BigInt {
                sign: if qm == 0 { 0 } else { self.sign * o.sign },
                mag: limb(qm),
            };
            let r = BigInt {
                sign: if rm == 0 { 0 } else { self.sign },
                mag: limb(rm),
            };
            return (q, r);
        }
        let (qmag, rmag) = div_rem_mag(&self.mag, &o.mag);
        let qsign = self.sign * o.sign;
        let q = BigInt { sign: if qmag.is_empty() { 0 } else { qsign }, mag: qmag };
        let r = BigInt { sign: if rmag.is_empty() { 0 } else { self.sign }, mag: rmag };
        (q, r)
    }
    /// Floor division pair: (floor(a/b), a mod b) with remainder in [0,|b|).
    pub fn div_mod_floor(&self, o: &BigInt) -> (BigInt, BigInt) {
        let (q, r) = self.div_rem(o);
        if !r.is_zero() && (r.sign != o.sign) {
            (q.sub(&BigInt::one()), r.add(o))
        } else {
            (q, r)
        }
    }
    pub fn gcd(&self, o: &BigInt) -> BigInt {
        let mag = gcd_mag(&self.mag, &o.mag);
        BigInt { sign: if mag.is_empty() { 0 } else { 1 }, mag }
    }
    pub fn pow(&self, e: u32) -> BigInt {
        if e == 0 {
            return BigInt::one();
        }
        let mut base = self.clone();
        let mut acc = BigInt::one();
        let mut e = e;
        while e > 0 {
            if e & 1 == 1 {
                acc = acc.mul(&base);
            }
            base = base.mul(&base);
            e >>= 1;
        }
        acc
    }
    /// e must be non-negative; returns None if too large to attempt.
    pub fn pow_big(&self, e: &BigInt) -> Option<BigInt> {
        if e.sign < 0 {
            return None;
        }
        if e.bit_len() > 20 {
            return None;
        }
        let mut base = self.clone();
        let mut acc = BigInt::one();
        for i in 0..e.bit_len() {
            if e.get_bit(i) {
                acc = acc.mul(&base);
            }
            base = base.mul(&base);
        }
        Some(acc)
    }
    pub fn shl(&self, bits: usize) -> BigInt {
        if self.is_zero() {
            return BigInt::zero();
        }
        let limb_shift = bits / 32;
        let bit_shift = bits % 32;
        let mut out = vec![0u32; limb_shift];
        if bit_shift == 0 {
            out.extend_from_slice(&self.mag);
        } else {
            let mut carry = 0u32;
            for &limb in &self.mag {
                out.push((limb << bit_shift) | carry);
                carry = limb >> (32 - bit_shift);
            }
            if carry != 0 {
                out.push(carry);
            }
        }
        BigInt { sign: self.sign, mag: trim_mag(&out) }
    }
    pub fn get_bit(&self, i: usize) -> bool {
        let limb = i / 32;
        limb < self.mag.len() && (self.mag[limb] >> (i % 32)) & 1 == 1
    }
    pub fn bit_len(&self) -> usize {
        bit_len_mag(&self.mag)
    }
    pub fn bit(&self, i: usize) -> bool {
        self.get_bit(i)
    }

    /// Floor square root (exact integer sqrt).
    pub fn isqrt(self) -> BigInt {
        if self.sign <= 0 {
            return BigInt::zero();
        }
        let bits = self.bit_len();
        let mut x = BigInt::from_i64(1).shl((bits + 1) / 2);
        let n = self;
        loop {
            let (q, _) = n.div_rem(&x);
            let y = q.add(&x).div_small_i(2);
            if y >= x {
                x = y;
                break;
            }
            x = y;
        }
        // Correct to exact floor.
        while x.mul(&x).cmp(&n) == Ordering::Greater {
            x = x.sub(&BigInt::one());
        }
        let one = BigInt::one();
        while x.add(&one).mul(&x.add(&one)).cmp(&n) != Ordering::Greater {
            x = x.add(&one);
        }
        x
    }
    /// Floor k-th root for k >= 1 (negative n returns the negative real root).
    pub fn iroot(self, k: u32) -> BigInt {
        if k == 0 || self.sign < 0 {
            return BigInt::zero();
        }
        if self.is_zero() || k == 1 {
            return self;
        }
        let sign = self.sign;
        let n = self.abs();
        let bits = n.bit_len();
        let mut lo = BigInt::zero();
        let mut hi = BigInt::one().shl((bits + k as usize - 1) / k as usize + 1);
        while lo < hi {
            let mid = lo.add(&hi).add(&BigInt::one()).div_small_i(2);
            let p = mid.pow(k);
            if p.cmp(&n) != Ordering::Greater {
                lo = mid;
            } else {
                hi = mid.sub(&BigInt::one());
            }
        }
        if sign > 0 {
            lo
        } else {
            lo.neg()
        }
    }
    pub fn div_small_i(&self, d: i64) -> BigInt {
        let (q, _) = self.div_rem(&BigInt::from_i64(d));
        q
    }
    pub fn div_rem_small(&self, d: u32) -> (BigInt, u32) {
        debug_assert!(d != 0);
        if self.mag.is_empty() {
            return (BigInt::zero(), 0);
        }
        let mut rem = 0u64;
        let mut out = vec![0u32; self.mag.len()];
        for i in (0..self.mag.len()).rev() {
            let cur = (rem << 32) | self.mag[i] as u64;
            out[i] = (cur / d as u64) as u32;
            rem = cur % d as u64;
        }
        let mag = trim_mag(&out);
        (
            BigInt { sign: if mag.is_empty() { 0 } else { self.sign }, mag },
            rem as u32,
        )
    }

    pub fn to_i64(&self) -> Option<i64> {
        if self.mag.len() > 2 {
            return None;
        }
        let uv: u128 = match self.mag.len() {
            0 => 0,
            1 => self.mag[0] as u128,
            _ => self.mag[0] as u128 | ((self.mag[1] as u128) << 32),
        };
        if self.sign >= 0 {
            if uv <= i64::MAX as u128 {
                Some(uv as i64)
            } else {
                None
            }
        } else {
            if uv <= (i64::MAX as u128) + 1 {
                Some((uv as i128 * -1) as i64)
            } else {
                None
            }
        }
    }
    pub fn to_u64_pub(&self) -> Option<u64> {
        if self.sign < 0 || self.mag.len() > 2 {
            return None;
        }
        match self.mag.len() {
            0 => Some(0),
            1 => Some(self.mag[0] as u64),
            _ => {
                let hi = self.mag[1] as u64;
                if hi > u32::MAX as u64 {
                    None
                } else {
                    Some(((hi as u64) << 32) | self.mag[0] as u64)
                }
            }
        }
    }
    pub fn from_u64(v: u64) -> BigInt {
        if v == 0 {
            return BigInt::zero();
        }
        BigInt {
            sign: 1,
            mag: vec![v as u32, (v >> 32) as u32],
        }
        .normalized()
    }
    pub fn to_i128(&self) -> Option<i128> {
        if self.bit_len() > 127 {
            return None;
        }
        let mut uv: u128 = 0;
        for (i, &limb) in self.mag.iter().enumerate().take(4) {
            uv |= (limb as u128) << (32 * i);
        }
        Some(if self.sign < 0 { -(uv as i128) } else { uv as i128 })
    }
    pub fn to_f64(&self) -> f64 {
        if let Some(v) = self.to_i128() {
            return v as f64;
        }
        let bits = self.bit_len();
        let take = bits.min(64);
        let mut q: u64 = 0;
        for i in 0..take {
            let b = bits - 1 - i;
            q = (q << 1) | self.get_bit(b) as u64;
        }
        let mut nonzero_below = false;
        for i in 0..(bits - take) {
            if self.get_bit(i) {
                nonzero_below = true;
                break;
            }
        }
        if nonzero_below && (q & 1 == 1) {
            q = q.wrapping_add(1);
        }
        let f = q as f64 * 2f64.powi((bits - take) as i32);
        if self.sign < 0 {
            -f
        } else {
            f
        }
    }

    pub fn to_string_dec(&self) -> String {
        if self.is_zero() {
            return "0".to_string();
        }
        let mut parts: Vec<String> = Vec::new();
        let mut cur = self.clone();
        let base = 1_000_000_000u32;
        while !cur.is_zero() {
            let (q, r) = cur.div_rem_small(base);
            cur = q;
            parts.push(format!("{:09}", r));
        }
        let mut s = parts.into_iter().rev().collect::<String>();
        while s.len() > 1 && s.starts_with('0') {
            s.remove(0);
        }
        if self.sign < 0 {
            s.insert(0, '-');
        }
        s
    }
    pub fn to_string_radix(&self, radix: u32) -> String {
        const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
        if self.is_zero() {
            return "0".to_string();
        }
        let mut out: Vec<u8> = Vec::new();
        let mut cur = self.clone();
        while !cur.is_zero() {
            let (q, r) = cur.div_rem_small(radix);
            cur = q;
            out.push(DIGITS[r as usize]);
        }
        if self.sign < 0 {
            out.push(b'-');
        }
        out.reverse();
        String::from_utf8(out).unwrap()
    }
}

impl PartialEq for BigInt {
    fn eq(&self, o: &BigInt) -> bool {
        self.sign == o.sign && self.mag == o.mag
    }
}
impl Eq for BigInt {}
impl Hash for BigInt {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.sign.hash(h);
        self.mag.hash(h);
    }
}
impl PartialOrd for BigInt {
    fn partial_cmp(&self, o: &BigInt) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for BigInt {
    fn cmp(&self, o: &BigInt) -> Ordering {
        match self.sign.cmp(&o.sign) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match self.sign {
            0 => Ordering::Equal,
            1 => cmp_mag(&self.mag, &o.mag),
            -1 => cmp_mag(&o.mag, &self.mag),
            _ => unreachable!(),
        }
    }
}
impl fmt::Display for BigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.to_string_dec(), f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(s: &str) -> BigInt {
        BigInt::from_str_dec(s).unwrap()
    }

    #[test]
    fn parse_format_roundtrip() {
        let n = b("123456789012345678901234567890123456789");
        assert_eq!(n.to_string(), "123456789012345678901234567890123456789");
        assert_eq!(BigInt::from_i64(-42).to_string(), "-42");
        assert_eq!(BigInt::zero().to_string(), "0");
    }

    #[test]
    fn arithmetic() {
        let a = b("987654321987654321987654321");
        let bb = b("123456789");
        assert_eq!(a.add(&bb).to_string(), "987654321987654322111111110");
        assert_eq!(a.sub(&bb).to_string(), "987654321987654321864197532");
        assert_eq!(
            a.mul(&bb).to_string(),
            "121932631234567900234567900112635269"
        );
        assert_eq!(BigInt::from_i64(-7).add(&BigInt::from_i64(3)).to_string(), "-4");
        assert_eq!(BigInt::from_i64(-7).mul(&BigInt::from_i64(6)).to_string(), "-42");
        assert_eq!(b("-5").abs().to_string(), "5");
        assert_eq!(b("-5").neg().to_string(), "5");
    }

    #[test]
    fn division() {
        let (q, r) = BigInt::from_i64(-7).div_rem(&BigInt::from_i64(3));
        assert_eq!(q.to_string(), "-2");
        assert_eq!(r.to_string(), "-1");
        let (q, r) = BigInt::from_i64(-7).div_mod_floor(&BigInt::from_i64(3));
        assert_eq!(q.to_string(), "-3");
        assert_eq!(r.to_string(), "2");
        let (q, r) = BigInt::from_i64(5).div_rem(&BigInt::from_i64(5));
        assert_eq!(q.to_string(), "1");
        assert!(r.is_zero());
        // Property checks on a big division.
        let a = b("1000000000000000000000000000");
        let d = b("99991");
        let (q, r) = a.div_rem(&d);
        assert_eq!(q.mul(&d).add(&r).to_string(), a.to_string());
        assert!(r.cmp(&d) == Ordering::Less && !r.is_neg());
        let (q, r) = a.div_mod_floor(&d);
        assert_eq!(q.mul(&d).add(&r).to_string(), a.to_string());
        assert!(r.cmp(&d) == Ordering::Less && !r.is_neg());
        let (q, r) = b("1000000").div_rem(&b("99991"));
        assert_eq!(q.to_string(), "10");
        assert_eq!(r.to_string(), "90");
    }

    #[test]
    fn pow_roots_gcd() {
        assert_eq!(BigInt::from_i64(2).pow(100).to_string(), "1267650600228229401496703205376");
        let sq = b("12345678901234567890").pow(2);
        assert_eq!(sq.to_string(), "152415787532388367501905199875019052100");
        assert_eq!(sq.clone().isqrt().to_string(), "12345678901234567890");
        // Property: isqrt(n)^2 <= n < (isqrt(n)+1)^2
        let n = b("147573952589676412927");
        let s = n.clone().isqrt();
        assert!(s.pow(2).cmp(&n) != Ordering::Greater);
        assert!(s.add(&BigInt::one()).pow(2).cmp(&n) == Ordering::Greater);
        // Cubic roots: (2^20)^3 = 2^60.
        assert_eq!(BigInt::from_i64(2).pow(60).iroot(3).to_string(), "1048576");
        assert_eq!(b("48").gcd(&b("18")).to_string(), "6");
        assert_eq!(b("0").gcd(&b("0")).to_string(), "0");
    }

    #[test]
    fn radix_and_bits() {
        assert_eq!(BigInt::from_i64(255).to_string_radix(16), "ff");
        assert_eq!(BigInt::from_radix("1010", 2).unwrap().to_string(), "10");
        assert_eq!(BigInt::from_i64(255).get_bit(7), true);
        assert_eq!(BigInt::from_i64(255).bit_len(), 8);
    }

    #[test]
    fn f64_conversion() {
        assert_eq!(b("1000000000000000000000").to_f64(), 1e21);
        assert_eq!(BigInt::from_i64(-3).to_f64(), -3.0);
    }
}