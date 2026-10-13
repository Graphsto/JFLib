// bn -- unsigned big integers for checking signatures (RSA, ECDSA)
//
//   from_be / to_be                     big-endian bytes <-> little-endian 64-bit limbs
//   Mont::new(n)                        arithmetic modulo an odd n: mul (Montgomery), add, sub,
//                                       to_mont / from_mont, pow, inv (n prime)
//
// Variable time on purpose: everything here is public (keys, signatures, digests).  Never use
// it for secrets.

use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;

pub type Limbs = Vec<u64>;

/// big-endian bytes -> limbs (at least `k` of them)
pub fn from_be(b: &[u8], k: usize) -> Limbs {
    let mut v = vec![0u64; k.max(b.len().div_ceil(8))];
    for (i, &x) in b.iter().rev().enumerate() {
        v[i / 8] |= (x as u64) << (8 * (i % 8));
    }
    v
}

/// limbs -> exactly `len` big-endian bytes (higher bytes dropped)
pub fn to_be(a: &[u64], len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    for (i, o) in out.iter_mut().rev().enumerate() {
        if i / 8 < a.len() {
            *o = (a[i / 8] >> (8 * (i % 8))) as u8;
        }
    }
    out
}

pub fn cmp(a: &[u64], b: &[u64]) -> Ordering {
    let n = a.len().max(b.len());
    for i in (0..n).rev() {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x.cmp(&y);
        }
    }
    Ordering::Equal
}

pub fn is_zero(a: &[u64]) -> bool {
    a.iter().all(|&x| x == 0)
}

/// the number of significant bits
pub fn bits(a: &[u64]) -> usize {
    for i in (0..a.len()).rev() {
        if a[i] != 0 {
            return i * 64 + 64 - a[i].leading_zeros() as usize;
        }
    }
    0
}

/// a -= b (a >= b, b no longer than a) -> borrow
fn sub_in(a: &mut [u64], b: &[u64]) -> u64 {
    let mut borrow = 0u64;
    for (i, x) in a.iter_mut().enumerate() {
        let y = b.get(i).copied().unwrap_or(0);
        let (d1, b1) = x.overflowing_sub(y);
        let (d2, b2) = d1.overflowing_sub(borrow);
        *x = d2;
        borrow = (b1 | b2) as u64;
    }
    borrow
}

/// a += b -> carry
fn add_in(a: &mut [u64], b: &[u64]) -> u64 {
    let mut carry = 0u64;
    for (i, x) in a.iter_mut().enumerate() {
        let y = b.get(i).copied().unwrap_or(0);
        let (s1, c1) = x.overflowing_add(y);
        let (s2, c2) = s1.overflowing_add(carry);
        *x = s2;
        carry = (c1 | c2) as u64;
    }
    carry
}

/// arithmetic modulo an odd number
#[derive(Clone)]
pub struct Mont {
    pub n: Limbs,
    k: usize,
    /// -n^-1 mod 2^64
    n0: u64,
    /// R^2 mod n (R = 2^(64 k))
    r2: Limbs,
}

impl Mont {
    pub fn new(n: &[u64]) -> Option<Mont> {
        let mut n = Vec::from(n);
        while n.len() > 1 && *n.last()? == 0 {
            n.pop();
        }
        if n[0] & 1 == 0 || (n.len() == 1 && n[0] < 3) {
            return None;
        }
        let k = n.len();
        let mut inv = 1u64;
        for _ in 0..6 {
            inv = inv.wrapping_mul(2u64.wrapping_sub(n[0].wrapping_mul(inv)));
        }
        // R^2 mod n by doubling 1 two times 64 k times
        let mut r = vec![0u64; k + 1];
        r[0] = 1;
        for _ in 0..128 * k {
            let mut c = 0;
            for x in r.iter_mut() {
                let nc = *x >> 63;
                *x = *x << 1 | c;
                c = nc;
            }
            if cmp(&r, &n) != Ordering::Less {
                sub_in(&mut r, &n);
            }
        }
        r.truncate(k);
        Some(Mont { n, k, n0: inv.wrapping_neg(), r2: r })
    }

    pub fn limbs(&self) -> usize {
        self.k
    }

    fn pad(&self, a: &[u64]) -> Limbs {
        let mut v = vec![0u64; self.k];
        for (i, &x) in a.iter().enumerate().take(self.k) {
            v[i] = x;
        }
        v
    }

    /// a b R^-1 mod n (a, b < n)
    pub fn mul(&self, a: &[u64], b: &[u64]) -> Limbs {
        let k = self.k;
        let (a, b) = (self.pad(a), self.pad(b));
        let n = &self.n;
        let mut t = vec![0u64; k + 2];
        for &bi in b.iter() {
            let mut c = 0u128;
            for j in 0..k {
                let x = t[j] as u128 + a[j] as u128 * bi as u128 + c;
                t[j] = x as u64;
                c = x >> 64;
            }
            let x = t[k] as u128 + c;
            t[k] = x as u64;
            t[k + 1] = (x >> 64) as u64;
            let m = t[0].wrapping_mul(self.n0);
            let x = t[0] as u128 + m as u128 * n[0] as u128;
            let mut c = x >> 64;
            for j in 1..k {
                let x = t[j] as u128 + m as u128 * n[j] as u128 + c;
                t[j - 1] = x as u64;
                c = x >> 64;
            }
            let x = t[k] as u128 + c;
            t[k - 1] = x as u64;
            t[k] = t[k + 1] + (x >> 64) as u64;
            t[k + 1] = 0;
        }
        t.truncate(k + 1);
        if cmp(&t, n) != Ordering::Less {
            sub_in(&mut t, n);
        }
        t.truncate(k);
        t
    }

    pub fn to_mont(&self, a: &[u64]) -> Limbs {
        self.mul(a, &self.r2)
    }

    pub fn from_mont(&self, a: &[u64]) -> Limbs {
        self.mul(a, &[1])
    }

    /// (a + b) mod n (a, b < n)
    pub fn add(&self, a: &[u64], b: &[u64]) -> Limbs {
        let mut r = self.pad(a);
        r.push(0);
        add_in(&mut r, b);
        if cmp(&r, &self.n) != Ordering::Less {
            sub_in(&mut r, &self.n);
        }
        r.truncate(self.k);
        r
    }

    /// (a - b) mod n (a, b < n)
    pub fn sub(&self, a: &[u64], b: &[u64]) -> Limbs {
        let mut r = self.pad(a);
        if sub_in(&mut r, b) != 0 {
            add_in(&mut r, &self.n);
        }
        r
    }

    /// a mod n for a < 2 n
    pub fn reduce_once(&self, a: &[u64]) -> Limbs {
        let mut r = Vec::from(a);
        r.resize(r.len().max(self.k) + 1, 0);
        if cmp(&r, &self.n) != Ordering::Less {
            sub_in(&mut r, &self.n);
        }
        r.truncate(self.k);
        r
    }

    /// base^exp mod n (normal form in and out; exp big-endian)
    pub fn pow(&self, base: &[u64], exp: &[u8]) -> Limbs {
        let b = self.to_mont(base);
        let mut acc = self.to_mont(&[1]);
        for &byte in exp {
            for i in (0..8).rev() {
                acc = self.mul(&acc, &acc);
                if byte >> i & 1 == 1 {
                    acc = self.mul(&acc, &b);
                }
            }
        }
        self.from_mont(&acc)
    }

    /// a^-1 mod n for a prime n (a != 0; Montgomery form in and out)
    pub fn inv_mont(&self, a: &[u64]) -> Limbs {
        let mut e = self.n.clone();
        sub_in(&mut e, &[2]);
        let ebytes = to_be(&e, self.k * 8);
        // pow in the Montgomery domain: start from R (= 1)
        let mut acc = self.to_mont(&[1]);
        for &byte in &ebytes {
            for i in (0..8).rev() {
                acc = self.mul(&acc, &acc);
                if byte >> i & 1 == 1 {
                    acc = self.mul(&acc, a);
                }
            }
        }
        acc
    }
}
