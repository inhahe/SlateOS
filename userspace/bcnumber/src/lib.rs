//! GNU bc 1.07.1's `lib/number.c`, ported: the arbitrary-precision decimal
//! numbers GNU `bc` and `dc` compute with, as Ubuntu 24.04 builds them.
//!
//! A number is a sign, `len` digits before the point and `scale` after,
//! one decimal digit per byte. Every operation is upstream's own algorithm
//! -- Karatsuba above 80 digits, Knuth's division, Newton's square root
//! with its widening precisions, `bc_raise`'s squarings at their own
//! scales -- because the digits a result keeps past its exact value
//! depend on the algorithm, and those digits are what `dc` prints.
//!
//! Upstream shares numbers by reference count and changes a few in place;
//! every number it changes in place is one it has just made, so values
//! here are plain values, copied where upstream counts a reference.
//!
//! # What upstream's build makes of `LONG_MAX`
//!
//! `number.c` reaches `LONG_MAX` through `number.h`, which defines it as
//! `0x7fffffff` when nothing has -- and on glibc nothing has, since
//! `number.c` includes no `<limits.h>`. So [`num2long`] stops converting at
//! a 32-bit bound although `long` is 64 bits: measured, `dc` takes
//! 2147483648 as a scale (and wraps it negative) but calls 9999999999 an
//! overflow.

#![allow(
    // A transcription of upstream's digit-array arithmetic: indexes are
    // the C's pointers, bounded by the lengths it allocates, and the
    // integer types follow the C's, `int` and `unsigned` alike.
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::similar_names,
    clippy::many_single_char_names,
    clippy::too_many_lines
)]

/// The base digits are kept in.
const BASE: i32 = 10;

/// `number.h`'s fallback `LONG_MAX`, which is the one `number.c` sees.
pub const LONG_MAX: i64 = 0x7fff_ffff;

/// Numbers longer than this (in digits, the two factors together) are
/// multiplied by divide and conquer.
const MUL_BASE_DIGITS: usize = 80;
/// Neither factor may be shorter than this for divide and conquer.
const MUL_SMALL_DIGITS: usize = MUL_BASE_DIGITS / 4;

/// A number: `bc_struct`, without the reference count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Num {
    /// `n_sign == MINUS`.
    pub neg: bool,
    /// Digits before the point (`n_len`).
    pub len: usize,
    /// Digits after the point (`n_scale`).
    pub scale: usize,
    /// The `len + scale` digits, most significant first (`n_value`).
    pub v: Vec<u8>,
}

/// A run-time diagnostic upstream prints from inside the arithmetic: its
/// `rt_warn` and `rt_error`, which the program decides how to show.
pub trait Diag {
    /// `rt_warn (msg)`.
    fn warn(&mut self, msg: &str);
    /// `rt_error (msg)`. The arithmetic goes on.
    fn error(&mut self, msg: &str);
}

impl Num {
    /// `bc_new_num (len, scale)`: zero, with that many digits.
    #[must_use]
    pub fn new(len: usize, scale: usize) -> Num {
        Num {
            neg: false,
            len,
            scale,
            v: vec![0; len + scale],
        }
    }

    /// `_zero_`.
    #[must_use]
    pub fn zero() -> Num {
        Num::new(1, 0)
    }

    /// `_one_`.
    #[must_use]
    pub fn one() -> Num {
        let mut n = Num::new(1, 0);
        n.v[0] = 1;
        n
    }

    /// `_two_`.
    #[must_use]
    pub fn two() -> Num {
        let mut n = Num::new(1, 0);
        n.v[0] = 2;
        n
    }

    /// Cut the number to `scale` digits after the point, as upstream does
    /// by lowering `n_scale` in place.
    pub fn truncate_scale(&mut self, scale: usize) {
        if scale < self.scale {
            self.scale = scale;
            self.v.truncate(self.len + scale);
        }
    }

    /// `_bc_rm_leading_zeros`.
    fn rm_leading_zeros(&mut self) {
        let mut k = 0;
        while self.len - k > 1 && self.v[k] == 0 {
            k += 1;
        }
        if k > 0 {
            self.v.drain(..k);
            self.len -= k;
        }
    }

    /// An integer from a slice of digits: `new_sub_num (len, 0, value)`.
    fn sub_num(digits: &[u8]) -> Num {
        Num {
            neg: false,
            len: digits.len(),
            scale: 0,
            v: digits.to_vec(),
        }
    }
}

/// `_bc_do_compare (n1, n2, use_sign, ignore_last)`: -1, 0 or 1.
fn do_compare(n1: &Num, n2: &Num, use_sign: bool, ignore_last: bool) -> i32 {
    let positive = |n: &Num| !use_sign || !n.neg;
    if use_sign && n1.neg != n2.neg {
        return if n1.neg { -1 } else { 1 };
    }
    if n1.len != n2.len {
        return if n1.len > n2.len {
            if positive(n1) { 1 } else { -1 }
        } else if positive(n1) {
            -1
        } else {
            1
        };
    }
    let mut count = n1.len + n1.scale.min(n2.scale);
    let mut i = 0;
    while count > 0 && n1.v[i] == n2.v[i] {
        i += 1;
        count -= 1;
    }
    if ignore_last && count == 1 && n1.scale == n2.scale {
        return 0;
    }
    if count != 0 {
        return if n1.v[i] > n2.v[i] {
            if positive(n1) { 1 } else { -1 }
        } else if positive(n1) {
            -1
        } else {
            1
        };
    }
    if n1.scale != n2.scale {
        if n1.scale > n2.scale {
            for k in 0..n1.scale - n2.scale {
                if n1.v[i + k] != 0 {
                    return if positive(n1) { 1 } else { -1 };
                }
            }
        } else {
            for k in 0..n2.scale - n1.scale {
                if n2.v[i + k] != 0 {
                    return if positive(n1) { -1 } else { 1 };
                }
            }
        }
    }
    0
}

/// `bc_compare (n1, n2)`: -1, 0 or 1.
#[must_use]
pub fn compare(n1: &Num, n2: &Num) -> i32 {
    do_compare(n1, n2, true, false)
}

/// `bc_is_neg`.
#[must_use]
pub fn is_neg(n: &Num) -> bool {
    n.neg
}

/// `bc_is_zero`.
#[must_use]
pub fn is_zero(n: &Num) -> bool {
    n.v[..n.len + n.scale].iter().all(|&d| d == 0)
}

/// `bc_is_near_zero (num, scale)`: every digit through `scale` places is
/// zero but the last, which is 0 or 1.
#[must_use]
pub fn is_near_zero(n: &Num, scale: usize) -> bool {
    let scale = scale.min(n.scale);
    let mut count = n.len + scale;
    let mut i = 0;
    while count > 0 {
        let d = n.v[i];
        i += 1;
        if d != 0 {
            break;
        }
        count -= 1;
    }
    !(count != 0 && (count != 1 || n.v[i - 1] != 1))
}

/// `_bc_do_add (n1, n2, scale_min)`: the magnitudes added.
fn do_add(n1: &Num, n2: &Num, scale_min: usize) -> Num {
    let sum_scale = n1.scale.max(n2.scale);
    let sum_digits = n1.len.max(n2.len) + 1;
    let mut sum = Num::new(sum_digits, sum_scale.max(scale_min));

    let mut n1bytes = n1.scale as isize;
    let mut n2bytes = n2.scale as isize;
    let mut n1p = (n1.len + n1.scale) as isize - 1;
    let mut n2p = (n2.len + n2.scale) as isize - 1;
    let mut sp = (sum_scale + sum_digits) as isize - 1;

    if n1bytes != n2bytes {
        if n1bytes > n2bytes {
            while n1bytes > n2bytes {
                sum.v[sp as usize] = n1.v[n1p as usize];
                sp -= 1;
                n1p -= 1;
                n1bytes -= 1;
            }
        } else {
            while n2bytes > n1bytes {
                sum.v[sp as usize] = n2.v[n2p as usize];
                sp -= 1;
                n2p -= 1;
                n2bytes -= 1;
            }
        }
    }

    n1bytes += n1.len as isize;
    n2bytes += n2.len as isize;
    let mut carry = 0;
    while n1bytes > 0 && n2bytes > 0 {
        let mut d = n1.v[n1p as usize] + n2.v[n2p as usize] + carry;
        n1p -= 1;
        n2p -= 1;
        if d > (BASE - 1) as u8 {
            carry = 1;
            d -= BASE as u8;
        } else {
            carry = 0;
        }
        sum.v[sp as usize] = d;
        sp -= 1;
        n1bytes -= 1;
        n2bytes -= 1;
    }

    let (mut rest, mut rp, rv) = if n1bytes == 0 {
        (n2bytes, n2p, &n2.v)
    } else {
        (n1bytes, n1p, &n1.v)
    };
    while rest > 0 {
        rest -= 1;
        let mut d = rv[rp as usize] + carry;
        rp -= 1;
        if d > (BASE - 1) as u8 {
            carry = 1;
            d -= BASE as u8;
        } else {
            carry = 0;
        }
        sum.v[sp as usize] = d;
        sp -= 1;
    }

    if carry == 1 {
        sum.v[sp as usize] += 1;
    }
    sum.rm_leading_zeros();
    sum
}

/// `_bc_do_sub (n1, n2, scale_min)`: the magnitude of `n2` from that of
/// `n1`, which is the larger.
fn do_sub(n1: &Num, n2: &Num, scale_min: usize) -> Num {
    let diff_len = n1.len.max(n2.len);
    let diff_scale = n1.scale.max(n2.scale);
    let min_len = n1.len.min(n2.len);
    let min_scale = n1.scale.min(n2.scale);
    let mut diff = Num::new(diff_len, diff_scale.max(scale_min));

    let mut n1p = (n1.len + n1.scale) as isize - 1;
    let mut n2p = (n2.len + n2.scale) as isize - 1;
    let mut dp = (diff_len + diff_scale) as isize - 1;

    let mut borrow: i32 = 0;
    if n1.scale == min_scale {
        for _ in 0..n2.scale - min_scale {
            let mut val = -i32::from(n2.v[n2p as usize]) - borrow;
            n2p -= 1;
            if val < 0 {
                val += BASE;
                borrow = 1;
            } else {
                borrow = 0;
            }
            diff.v[dp as usize] = val as u8;
            dp -= 1;
        }
    } else {
        for _ in 0..n1.scale - min_scale {
            diff.v[dp as usize] = n1.v[n1p as usize];
            dp -= 1;
            n1p -= 1;
        }
    }

    for _ in 0..min_len + min_scale {
        let mut val = i32::from(n1.v[n1p as usize]) - i32::from(n2.v[n2p as usize]) - borrow;
        n1p -= 1;
        n2p -= 1;
        if val < 0 {
            val += BASE;
            borrow = 1;
        } else {
            borrow = 0;
        }
        diff.v[dp as usize] = val as u8;
        dp -= 1;
    }

    if diff_len != min_len {
        for _ in 0..diff_len - min_len {
            let mut val = i32::from(n1.v[n1p as usize]) - borrow;
            n1p -= 1;
            if val < 0 {
                val += BASE;
                borrow = 1;
            } else {
                borrow = 0;
            }
            diff.v[dp as usize] = val as u8;
            dp -= 1;
        }
    }

    diff.rm_leading_zeros();
    diff
}

/// `bc_sub (n1, n2, &result, scale_min)`: `n1 - n2`, with at least
/// `scale_min` digits after the point.
#[must_use]
pub fn sub(n1: &Num, n2: &Num, scale_min: usize) -> Num {
    if n1.neg == n2.neg {
        match do_compare(n1, n2, false, false) {
            -1 => {
                let mut d = do_sub(n2, n1, scale_min);
                d.neg = !n2.neg;
                d
            }
            0 => {
                let res_scale = scale_min.max(n1.scale.max(n2.scale));
                Num::new(1, res_scale)
            }
            _ => {
                let mut d = do_sub(n1, n2, scale_min);
                d.neg = n1.neg;
                d
            }
        }
    } else {
        let mut d = do_add(n1, n2, scale_min);
        d.neg = n1.neg;
        d
    }
}

/// `bc_add (n1, n2, &result, scale_min)`: `n1 + n2`, with at least
/// `scale_min` digits after the point.
#[must_use]
pub fn add(n1: &Num, n2: &Num, scale_min: usize) -> Num {
    if n1.neg == n2.neg {
        let mut s = do_add(n1, n2, scale_min);
        s.neg = n1.neg;
        s
    } else {
        match do_compare(n1, n2, false, false) {
            -1 => {
                let mut s = do_sub(n2, n1, scale_min);
                s.neg = n2.neg;
                s
            }
            0 => {
                let res_scale = scale_min.max(n1.scale.max(n2.scale));
                Num::new(1, res_scale)
            }
            _ => {
                let mut s = do_sub(n1, n2, scale_min);
                s.neg = n1.neg;
                s
            }
        }
    }
}

/// `_bc_simp_mul`: the product of two digit strings, `u.len() + v.len()
/// + 1` digits long.
fn simp_mul(n1: &[u8], n2: &[u8]) -> Vec<u8> {
    let (n1len, n2len) = (n1.len() as isize, n2.len() as isize);
    let prodlen = (n1len + n2len + 1) as usize;
    let mut prod = vec![0u8; prodlen];
    let n1end = n1len - 1;
    let n2end = n2len - 1;
    let mut pv = prodlen as isize - 1;
    let mut sum: i64 = 0;
    for indx in 0..(prodlen as isize - 1) {
        let mut a = n1end - 0.max(indx - n2len + 1);
        let mut b = n2end - indx.min(n2len - 1);
        while a >= 0 && b <= n2end {
            sum += i64::from(n1[a as usize]) * i64::from(n2[b as usize]);
            a -= 1;
            b += 1;
        }
        prod[pv as usize] = (sum % i64::from(BASE)) as u8;
        pv -= 1;
        sum /= i64::from(BASE);
    }
    prod[pv as usize] = sum as u8;
    prod
}

/// `_bc_shift_addsub (accum, val, shift, sub)`: add `val` (or take it
/// away) at `shift` digits from the right of `accum`.
fn shift_addsub(accum: &mut [u8], val: &Num, shift: usize, subtract: bool) {
    let mut count = val.len;
    if val.v.first().copied().unwrap_or(0) == 0 {
        count = count.saturating_sub(1);
    }
    let mut acc = accum.len() as isize - shift as isize - 1;
    let mut vp = val.len as isize - 1;
    let mut carry: i32 = 0;
    if subtract {
        while count > 0 {
            count -= 1;
            let mut a = i32::from(accum[acc as usize]) - (i32::from(val.v[vp as usize]) + carry);
            vp -= 1;
            if a < 0 {
                carry = 1;
                a += BASE;
            } else {
                carry = 0;
            }
            accum[acc as usize] = a as u8;
            acc -= 1;
        }
        while carry != 0 {
            let mut a = i32::from(accum[acc as usize]) - carry;
            if a < 0 {
                a += BASE;
                accum[acc as usize] = a as u8;
                acc -= 1;
            } else {
                accum[acc as usize] = a as u8;
                carry = 0;
            }
        }
    } else {
        while count > 0 {
            count -= 1;
            let mut a = i32::from(accum[acc as usize]) + i32::from(val.v[vp as usize]) + carry;
            vp -= 1;
            if a > BASE - 1 {
                carry = 1;
                a -= BASE;
            } else {
                carry = 0;
            }
            accum[acc as usize] = a as u8;
            acc -= 1;
        }
        while carry != 0 {
            let mut a = i32::from(accum[acc as usize]) + carry;
            if a > BASE - 1 {
                a -= BASE;
                accum[acc as usize] = a as u8;
                acc -= 1;
            } else {
                accum[acc as usize] = a as u8;
                carry = 0;
            }
        }
    }
}

/// `_bc_rec_mul (u, ulen, v, vlen, &prod)`: the product of two digit
/// strings by divide and conquer, `u.len() + v.len() + 1` digits long.
fn rec_mul(u: &[u8], v: &[u8]) -> Vec<u8> {
    let (ulen, vlen) = (u.len(), v.len());
    if ulen + vlen < MUL_BASE_DIGITS || ulen < MUL_SMALL_DIGITS || vlen < MUL_SMALL_DIGITS {
        return simp_mul(u, v);
    }
    let n = ulen.max(vlen).div_ceil(2);
    let (mut u1, mut u0) = if ulen < n {
        (Num::zero(), Num::sub_num(u))
    } else {
        (Num::sub_num(&u[..ulen - n]), Num::sub_num(&u[ulen - n..]))
    };
    let (mut v1, mut v0) = if vlen < n {
        (Num::zero(), Num::sub_num(v))
    } else {
        (Num::sub_num(&v[..vlen - n]), Num::sub_num(&v[vlen - n..]))
    };
    u1.rm_leading_zeros();
    u0.rm_leading_zeros();
    v1.rm_leading_zeros();
    v0.rm_leading_zeros();

    let m1zero = is_zero(&u1) || is_zero(&v1);
    let d1 = sub(&u1, &u0, 0);
    let d2 = sub(&v0, &v1, 0);

    let product = |a: &Num, b: &Num| -> Num {
        let p = rec_mul(&a.v[..a.len], &b.v[..b.len]);
        Num::sub_num(&p)
    };
    let m1 = if m1zero {
        Num::zero()
    } else {
        product(&u1, &v1)
    };
    let m2 = if is_zero(&d1) || is_zero(&d2) {
        Num::zero()
    } else {
        product(&d1, &d2)
    };
    let m3 = if is_zero(&u0) || is_zero(&v0) {
        Num::zero()
    } else {
        product(&u0, &v0)
    };

    let mut prod = vec![0u8; ulen + vlen + 1];
    if !m1zero {
        shift_addsub(&mut prod, &m1, 2 * n, false);
        shift_addsub(&mut prod, &m1, n, false);
    }
    shift_addsub(&mut prod, &m3, n, false);
    shift_addsub(&mut prod, &m3, 0, false);
    shift_addsub(&mut prod, &m2, n, d1.neg != d2.neg);
    prod
}

/// `bc_multiply (n1, n2, &prod, scale)`: the product, with the scale
/// `MIN (n1 scale + n2 scale, MAX (scale, n1 scale, n2 scale))`.
#[must_use]
pub fn multiply(n1: &Num, n2: &Num, scale: i64) -> Num {
    let len1 = n1.len + n1.scale;
    let len2 = n2.len + n2.scale;
    let full_scale = n1.scale + n2.scale;
    let prod_scale = (full_scale as i64).min(scale.max(n1.scale.max(n2.scale) as i64)) as usize;

    let digits = rec_mul(&n1.v[..len1], &n2.v[..len2]);
    let len = len2 + len1 + 1 - full_scale;
    let mut pval = Num {
        neg: n1.neg != n2.neg,
        len,
        scale: full_scale,
        v: digits,
    };
    pval.truncate_scale(prod_scale);
    pval.rm_leading_zeros();
    if is_zero(&pval) {
        pval.neg = false;
    }
    pval
}

/// `_one_mult`: `src` times one digit, and the carry out of its top.
fn one_mult(src: &[u8], digit: u32) -> (Vec<u8>, u32) {
    match digit {
        0 => (vec![0; src.len()], 0),
        1 => (src.to_vec(), 0),
        _ => {
            let mut out = vec![0u8; src.len()];
            let mut carry = 0;
            for i in (0..src.len()).rev() {
                let value = u32::from(src[i]) * digit + carry;
                out[i] = (value % BASE as u32) as u8;
                carry = value / BASE as u32;
            }
            (out, carry)
        }
    }
}

/// `bc_divide (n1, n2, &quot, scale)`: the quotient, truncated to `scale`
/// places; `None` for a division by zero.
#[must_use]
pub fn divide(n1: &Num, n2: &Num, scale: usize) -> Option<Num> {
    if is_zero(n2) {
        return None;
    }
    // Upstream's shortcut for a divisor of 1 is overwritten by the general
    // division below, which it does not skip.

    let mut scale2 = n2.scale;
    let mut p = n2.len + scale2;
    while scale2 > 0 {
        p -= 1;
        if n2.v[p] != 0 {
            break;
        }
        scale2 -= 1;
    }

    let len1 = n1.len + scale2;
    let scale1 = n1.scale as i64 - scale2 as i64;
    let extra = if scale1 < scale as i64 {
        (scale as i64 - scale1) as usize
    } else {
        0
    };
    let mut num1 = vec![0u8; n1.len + n1.scale + extra + 2];
    num1[1..=n1.len + n1.scale].copy_from_slice(&n1.v[..n1.len + n1.scale]);

    let mut len2 = n2.len + scale2;
    let mut num2 = vec![0u8; len2 + 1];
    num2[..len2].copy_from_slice(&n2.v[..len2]);
    let mut n2p = 0usize;
    while num2[n2p] == 0 {
        n2p += 1;
        len2 -= 1;
    }

    let (qdigits, zero) = if len2 > len1 + scale {
        (scale + 1, true)
    } else if len2 > len1 {
        (scale + 1, false)
    } else {
        (len1 - len2 + scale + 1, false)
    };

    let mut qval = Num::new(qdigits - scale, scale);
    let mut mval = vec![0u8; len2 + 1];

    if !zero {
        let norm = 10 / (u32::from(num2[n2p]) + 1);
        if norm != 1 {
            let span = (len1 as i64 + scale1 + extra as i64 + 1) as usize;
            let (m, _) = one_mult(&num1[..span], norm);
            num1[..span].copy_from_slice(&m);
            let (m, _) = one_mult(&num2[n2p..n2p + len2], norm);
            num2[n2p..n2p + len2].copy_from_slice(&m);
        }

        let mut qdig = 0usize;
        let mut qp = len2.saturating_sub(len1);
        while qdig <= len1 + scale - len2 {
            let d = u32::from(num2[n2p]);
            let top = u32::from(num1[qdig]) * 10 + u32::from(num1[qdig + 1]);
            let mut qguess = if d == u32::from(num1[qdig]) {
                9
            } else {
                top / d
            };
            let test = |q: u32| -> bool {
                u32::from(num2[n2p + 1]) * q
                    > (top.wrapping_sub(d * q)).wrapping_mul(10) + u32::from(num1[qdig + 2])
            };
            if test(qguess) {
                qguess -= 1;
                if test(qguess) {
                    qguess -= 1;
                }
            }

            let mut borrow = 0;
            if qguess != 0 {
                mval[0] = 0;
                let (m, carry) = one_mult(&num2[n2p..n2p + len2], qguess);
                mval[1..=len2].copy_from_slice(&m);
                if carry != 0 {
                    mval[0] = carry as u8;
                }
                let mut p1 = qdig + len2;
                let mut p2 = len2 as isize;
                for _ in 0..=len2 {
                    let mut val = i32::from(num1[p1]) - i32::from(mval[p2 as usize]) - borrow;
                    p2 -= 1;
                    if val < 0 {
                        val += 10;
                        borrow = 1;
                    } else {
                        borrow = 0;
                    }
                    num1[p1] = val as u8;
                    p1 = p1.wrapping_sub(1);
                }
            }

            if borrow == 1 {
                qguess -= 1;
                let mut p1 = qdig + len2;
                let mut p2 = (n2p + len2) as isize - 1;
                let mut carry = 0;
                for _ in 0..len2 {
                    let mut val = i32::from(num1[p1]) + i32::from(num2[p2 as usize]) + carry;
                    p2 -= 1;
                    if val > 9 {
                        val -= 10;
                        carry = 1;
                    } else {
                        carry = 0;
                    }
                    num1[p1] = val as u8;
                    p1 -= 1;
                }
                if carry == 1 {
                    num1[p1] = (num1[p1] + 1) % 10;
                }
            }

            qval.v[qp] = qguess as u8;
            qp += 1;
            qdig += 1;
        }
    }

    qval.neg = n1.neg != n2.neg;
    if is_zero(&qval) {
        qval.neg = false;
    }
    qval.rm_leading_zeros();
    Some(qval)
}

/// `bc_divmod (num1, num2, &quot, &rem, scale)`: the quotient (to
/// `scale` places) and the remainder; `None` for a division by zero.
#[must_use]
pub fn divmod(num1: &Num, num2: &Num, scale: usize) -> Option<(Num, Num)> {
    if is_zero(num2) {
        return None;
    }
    let rscale = num1.scale.max(num2.scale + scale);
    let temp = divide(num1, num2, scale)?;
    let quotient = temp.clone();
    let temp = multiply(&temp, num2, rscale as i64);
    let rem = sub(num1, &temp, rscale);
    Some((quotient, rem))
}

/// `bc_modulo (num1, num2, &result, scale)`.
#[must_use]
pub fn modulo(num1: &Num, num2: &Num, scale: usize) -> Option<Num> {
    divmod(num1, num2, scale).map(|(_, r)| r)
}

/// `bc_raisemod (base, expo, mod, &result, scale)`: `base` to the `expo`
/// modulo `modulus`; `None` for a zero modulus or a negative exponent.
#[must_use]
pub fn raisemod(
    base: &Num,
    expo: &Num,
    modulus: &Num,
    scale: usize,
    diag: &mut dyn Diag,
) -> Option<Num> {
    if is_zero(modulus) || expo.neg {
        return None;
    }
    let mut power = base.clone();
    let mut exponent = expo.clone();
    let mut temp = Num::one();

    if base.scale != 0 {
        diag.warn("non-zero scale in base");
    }
    if exponent.scale != 0 {
        diag.warn("non-zero scale in exponent");
        exponent = divide(&exponent, &Num::one(), 0)?;
    }
    if modulus.scale != 0 {
        diag.warn("non-zero scale in modulus");
    }

    let rscale = scale.max(base.scale) as i64;
    while !is_zero(&exponent) {
        let (q, parity) = divmod(&exponent, &Num::two(), 0)?;
        exponent = q;
        if !is_zero(&parity) {
            temp = multiply(&temp, &power, rscale);
            temp = modulo(&temp, modulus, scale)?;
        }
        power = multiply(&power, &power, rscale);
        power = modulo(&power, modulus, scale)?;
    }
    Some(temp)
}

/// `bc_raise (num1, num2, &result, scale)`: `num1` to the integer part of
/// `num2`. `result` is what upstream's result already held, which it keeps
/// when a negative power of zero cannot be divided.
#[must_use]
pub fn raise(num1: &Num, num2: &Num, result: Num, scale: i64, diag: &mut dyn Diag) -> Num {
    if num2.scale != 0 {
        diag.warn("non-zero scale in exponent");
    }
    let exponent = num2long(num2);
    if exponent == 0 && (num2.len > 1 || num2.v[0] != 0) {
        diag.error("exponent too large in raise");
    }
    if exponent == 0 {
        return Num::one();
    }

    let neg = exponent < 0;
    let mut uexponent = exponent.unsigned_abs();
    let rscale: i64 = if neg {
        scale
    } else {
        let a = (num1.scale as u64).wrapping_mul(uexponent);
        let b = scale.max(num1.scale as i64) as u64;
        i64::from(a.min(b) as i32)
    };

    let mut power = num1.clone();
    let mut pwrscale = num1.scale as i32;
    while uexponent & 1 == 0 {
        pwrscale = pwrscale.wrapping_shl(1);
        power = multiply(&power, &power, i64::from(pwrscale));
        uexponent >>= 1;
    }
    let mut temp = power.clone();
    let mut calcscale = pwrscale;
    uexponent >>= 1;

    while uexponent > 0 {
        pwrscale = pwrscale.wrapping_shl(1);
        power = multiply(&power, &power, i64::from(pwrscale));
        if uexponent & 1 == 1 {
            calcscale = pwrscale.wrapping_add(calcscale);
            temp = multiply(&temp, &power, i64::from(calcscale));
        }
        uexponent >>= 1;
    }

    if neg {
        match divide(&Num::one(), &temp, rscale.max(0) as usize) {
            Some(q) => q,
            None => result,
        }
    } else {
        if (temp.scale as i64) > rscale {
            temp.truncate_scale(rscale.max(0) as usize);
        }
        temp
    }
}

/// `bc_sqrt (&num, scale)`: the square root to `MAX (scale, num's
/// scale)` places; `None` for a negative number.
#[must_use]
pub fn sqrt(num: &Num, scale: usize, diag: &mut dyn Diag) -> Option<Num> {
    let cmp = compare(num, &Num::zero());
    if cmp < 0 {
        return None;
    }
    if cmp == 0 {
        return Some(Num::zero());
    }
    let cmp = compare(num, &Num::one());
    if cmp == 0 {
        return Some(Num::one());
    }

    let rscale = scale.max(num.scale);
    let point5 = Num {
        neg: false,
        len: 1,
        scale: 1,
        v: vec![0, 5],
    };

    let mut guess;
    let mut cscale;
    if cmp < 0 {
        guess = Num::one();
        cscale = num.scale;
    } else {
        guess = int2num(10);
        let mut guess1 = int2num(num.len as i32);
        guess1 = multiply(&guess1, &point5, 0);
        guess1.truncate_scale(0);
        guess = raise(&guess, &guess1, guess.clone(), 0, diag);
        cscale = 3;
    }

    loop {
        let guess1 = guess.clone();
        guess = divide(num, &guess, cscale)?;
        guess = add(&guess, &guess1, 0);
        guess = multiply(&guess, &point5, cscale as i64);
        let diff = sub(&guess, &guess1, cscale + 1);
        if is_near_zero(&diff, cscale) {
            if cscale < rscale + 1 {
                cscale = (cscale * 3).min(rscale + 1);
            } else {
                break;
            }
        }
    }
    divide(&guess, &Num::one(), rscale)
}

/// The digits of bases up to 16.
const REF_STR: &[u8; 16] = b"0123456789ABCDEF";

/// `bc_out_long (val, size, space, out_char)`: a "digit" of a base over
/// 16, as `size` decimal characters, a space first if `space`.
pub fn out_long(val: i64, size: usize, space: bool, out: &mut dyn FnMut(u8)) {
    if space {
        out(b' ');
    }
    let digits = val.to_string();
    let mut size = size;
    while size > digits.len() {
        out(b'0');
        size -= 1;
    }
    for &b in digits.as_bytes() {
        out(b);
    }
}

/// `bc_out_num (num, o_base, out_char, leading_zero)`: `num` in base
/// `o_base`, one character at a time.
pub fn out_num(num: &Num, o_base: i32, out: &mut dyn FnMut(u8), leading_zero: bool) {
    if num.neg {
        out(b'-');
    }
    if is_zero(num) {
        out(b'0');
        return;
    }
    if o_base == 10 {
        let mut p = 0;
        if num.len > 1 || num.v[0] != 0 {
            for _ in 0..num.len {
                out(b'0' + num.v[p]);
                p += 1;
            }
        } else {
            p += 1;
        }
        if leading_zero && is_zero(num) {
            out(b'0');
        }
        if num.scale > 0 {
            out(b'.');
            for _ in 0..num.scale {
                out(b'0' + num.v[p]);
                p += 1;
            }
        }
        return;
    }

    if leading_zero && is_zero(num) {
        out(b'0');
    }
    let one = Num::one();
    let mut int_part = divide(num, &one, 0).unwrap_or_else(Num::zero);
    let mut frac_part = sub(num, &int_part, 0);
    int_part.neg = false;
    frac_part.neg = false;
    let base = int2num(o_base);
    let max_o_digit = int2num(o_base - 1);

    let mut digits: Vec<i64> = Vec::new();
    while !is_zero(&int_part) {
        let cur_dig = modulo(&int_part, &base, 0).unwrap_or_else(Num::zero);
        digits.push(num2long(&cur_dig));
        int_part = divide(&int_part, &base, 0).unwrap_or_else(Num::zero);
    }
    for &d in digits.iter().rev() {
        if o_base <= 16 {
            out(REF_STR[d as usize]);
        } else {
            out_long(d, max_o_digit.len, true, out);
        }
    }

    if num.scale > 0 {
        out(b'.');
        let mut pre_space = false;
        let mut t_num = Num::one();
        while t_num.len <= num.scale {
            frac_part = multiply(&frac_part, &base, num.scale as i64);
            let fdigit = num2long(&frac_part);
            int_part = int2num(fdigit as i32);
            frac_part = sub(&frac_part, &int_part, 0);
            if o_base <= 16 {
                out(REF_STR[fdigit as usize]);
            } else {
                out_long(fdigit, max_o_digit.len, pre_space, out);
                pre_space = true;
            }
            t_num = multiply(&t_num, &base, 0);
        }
    }
}

/// `bc_num2long (num)`: the integer part, or 0 when it is out of reach --
/// which the caller tells from a real 0 by asking [`is_zero`]. The bound
/// is upstream's 32-bit `LONG_MAX`, the accumulator 64 bits.
#[must_use]
pub fn num2long(num: &Num) -> i64 {
    let mut val: i64 = 0;
    let mut i = num.len;
    let mut p = 0;
    while i > 0 && val <= LONG_MAX / i64::from(BASE) {
        val = val
            .wrapping_mul(i64::from(BASE))
            .wrapping_add(i64::from(num.v[p]));
        p += 1;
        i -= 1;
    }
    if i > 0 || val < 0 {
        val = 0;
    }
    if num.neg { -val } else { val }
}

/// `bc_int2num (&num, val)`.
#[must_use]
pub fn int2num(val: i32) -> Num {
    let neg = val < 0;
    let mut v = i64::from(val).abs();
    let mut digits = vec![(v % i64::from(BASE)) as u8];
    v /= i64::from(BASE);
    while v != 0 {
        digits.push((v % i64::from(BASE)) as u8);
        v /= i64::from(BASE);
    }
    digits.reverse();
    Num {
        neg,
        len: digits.len(),
        scale: 0,
        v: digits,
    }
}

/// `bc_num2str (num)`: base 10, as `bc` prints a number into a string.
#[must_use]
pub fn num2str(num: &Num) -> String {
    let mut s = String::new();
    if num.neg {
        s.push('-');
    }
    for &d in &num.v[..num.len] {
        s.push(char::from(b'0' + d));
    }
    if num.scale > 0 {
        s.push('.');
        for &d in &num.v[num.len..num.len + num.scale] {
            s.push(char::from(b'0' + d));
        }
    }
    s
}

/// `bc_str2num (&num, str, scale)`: a base-10 number, at most `scale`
/// places kept; zero for anything that is not one.
#[must_use]
pub fn str2num(text: &[u8], scale: usize) -> Num {
    let mut p = 0;
    if matches!(text.first(), Some(b'+' | b'-')) {
        p += 1;
    }
    while text.get(p) == Some(&b'0') {
        p += 1;
    }
    let mut digits = 0;
    while text.get(p).is_some_and(u8::is_ascii_digit) {
        p += 1;
        digits += 1;
    }
    if text.get(p) == Some(&b'.') {
        p += 1;
    }
    let mut strscale = 0;
    while text.get(p).is_some_and(u8::is_ascii_digit) {
        p += 1;
        strscale += 1;
    }
    if p != text.len() || digits + strscale == 0 {
        return Num::zero();
    }

    let strscale = strscale.min(scale);
    let zero_int = digits == 0;
    let mut num = Num::new(if zero_int { 1 } else { digits }, strscale);
    let mut p = 0;
    if text.first() == Some(&b'-') {
        num.neg = true;
        p += 1;
    } else if text.first() == Some(&b'+') {
        p += 1;
    }
    while text.get(p) == Some(&b'0') {
        p += 1;
    }
    let mut n = 0;
    if zero_int {
        num.v[0] = 0;
        n = 1;
    } else {
        for _ in 0..digits {
            num.v[n] = text[p] - b'0';
            n += 1;
            p += 1;
        }
    }
    if strscale > 0 {
        p += 1;
        for _ in 0..strscale {
            num.v[n] = text[p] - b'0';
            n += 1;
            p += 1;
        }
    }
    num
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    struct Quiet(Vec<String>);
    impl Diag for Quiet {
        fn warn(&mut self, msg: &str) {
            self.0.push(format!("warn: {msg}"));
        }
        fn error(&mut self, msg: &str) {
            self.0.push(format!("error: {msg}"));
        }
    }

    fn n(s: &str) -> Num {
        str2num(s.as_bytes(), 100)
    }

    fn show(num: &Num, base: i32) -> String {
        let mut out = Vec::new();
        out_num(num, base, &mut |c| out.push(c), false);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn arithmetic_as_bc_does_it() {
        assert_eq!(show(&add(&n("1.5"), &n("2.25"), 0), 10), "3.75");
        assert_eq!(show(&sub(&n("1"), &n("3"), 0), 10), "-2");
        assert_eq!(show(&sub(&n("2.50"), &n("2.5"), 0), 10), "0");
        assert_eq!(show(&multiply(&n("1.5"), &n("1.5"), 0), 10), "2.2");
        assert_eq!(show(&multiply(&n("1.5"), &n("1.5"), 5), 10), "2.25");
        assert_eq!(show(&divide(&n("1"), &n("3"), 5).unwrap(), 10), ".33333");
        assert_eq!(show(&divide(&n("-7"), &n("2"), 0).unwrap(), 10), "-3");
        assert!(divide(&n("1"), &n("0"), 5).is_none());
        let (q, r) = divmod(&n("17"), &n("5"), 0).unwrap();
        assert_eq!((show(&q, 10), show(&r, 10)), ("3".into(), "2".into()));
    }

    #[test]
    fn long_products_take_the_divide_and_conquer_path_and_agree() {
        let a = n(&"7".repeat(150));
        let b = n(&"3".repeat(120));
        let fast = multiply(&a, &b, 0);
        let slow = simp_mul(&a.v, &b.v);
        let mut slow = Num::sub_num(&slow);
        slow.rm_leading_zeros();
        assert_eq!(fast, slow);
    }

    #[test]
    fn powers_roots_and_other_bases() {
        let mut d = Quiet(Vec::new());
        assert_eq!(
            show(&raise(&n("2"), &n("100"), Num::zero(), 0, &mut d), 10),
            "1267650600228229401496703205376"
        );
        assert_eq!(
            show(&raise(&n("2"), &n("-2"), Num::zero(), 5, &mut d), 10),
            ".25000"
        );
        assert_eq!(
            show(&raise(&n("0"), &n("-1"), Num::zero(), 5, &mut d), 10),
            "0"
        );
        assert_eq!(
            show(&sqrt(&n("2"), 10, &mut d).unwrap(), 10),
            "1.4142135623"
        );
        assert!(sqrt(&n("-1"), 0, &mut d).is_none());
        assert_eq!(show(&n("255"), 16), "FF");
        assert_eq!(show(&n("255"), 100), " 02 55");
        assert_eq!(show(&n("10.5"), 2), "1010.1000");
        assert!(d.0.is_empty());
        let _ = raise(&n("2"), &n("1.5"), Num::zero(), 0, &mut d);
        assert_eq!(d.0, ["warn: non-zero scale in exponent"]);
    }

    #[test]
    fn longs_overflow_at_upstream_s_bound() {
        assert_eq!(num2long(&n("2147483647")), 2_147_483_647);
        assert_eq!(num2long(&n("2147483648")), 2_147_483_648);
        assert_eq!(num2long(&n("9999999999")), 0);
        assert_eq!(num2long(&n("-12.9")), -12);
    }
}
