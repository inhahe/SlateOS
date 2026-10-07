//! A band's shape as a PVQ codeword (RFC 6716 §4.3.4.2): `K` unit pulses in
//! `N` dimensions, one of `V(N, K)` such vectors, coded as its index and
//! decoded here back into the pulses.
//!
//! Translated into Rust from libopus 1.5.2's `celt/cwrs.c` (the table-driven
//! `cwrsi` and `decode_pulses`), copyright Xiph.Org, Timothy B. Terriberry
//! and the contributors named in its `COPYING`, used under libopus's BSD
//! licence (`licenses/libopus-COPYING`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "codeword arithmetic in u32 as libopus's: the index is below V(N, K), which the table keeps within 32 bits for every N and K a standard mode allocates, and the pulse counts below 128"
)]

use super::pvq_table::{PVQ_U_DATA, PVQ_U_ROW};
use crate::entdec::Decoder;
use crate::fixed::mac16_16;

/// `CELT_PVQ_U_ROW[row][col]`.
fn row(row: i32, col: i32) -> u32 {
    let start = usize::try_from(row)
        .ok()
        .and_then(|r| PVQ_U_ROW.get(r))
        .copied()
        .unwrap_or(0);
    usize::try_from(col)
        .ok()
        .and_then(|c| PVQ_U_DATA.get(start + c))
        .copied()
        .unwrap_or(0)
}

/// `CELT_PVQ_U(n, k)`: vectors of `k` pulses in `n` dimensions with a
/// positive first coordinate, roughly.
fn pvq_u(n: i32, k: i32) -> u32 {
    row(n.min(k), n.max(k))
}

/// `CELT_PVQ_V(n, k)`: the number of codewords of `k` pulses in `n`
/// dimensions.
pub(crate) fn pvq_v(n: i32, k: i32) -> u32 {
    pvq_u(n, k).wrapping_add(pvq_u(n, k + 1))
}

/// `cwrsi`: codeword `i` of `k` pulses in `n` dimensions into `y`; its
/// squared norm.
fn cwrsi(mut n: i32, mut k: i32, mut i: u32, y: &mut [i32]) -> i32 {
    let mut yy = 0i32;
    let mut at = 0usize;
    let mut put = |y: &mut [i32], v: i32, yy: &mut i32| {
        // `val` is an int16 in libopus.
        let v = v as i16 as i32;
        if let Some(slot) = y.get_mut(at) {
            *slot = v;
        }
        at += 1;
        *yy = mac16_16(*yy, v, v);
    };
    while n > 2 {
        if k >= n {
            // Many pulses.
            let mut p = row(n, k + 1);
            let s = -i32::from(i >= p);
            i -= p & s as u32;
            // How many pulses fell in this dimension.
            let k0 = k;
            let q = row(n, n);
            if q > i {
                k = n;
                loop {
                    k -= 1;
                    p = row(k, n);
                    if p <= i {
                        break;
                    }
                }
            } else {
                p = row(n, k);
                while p > i {
                    k -= 1;
                    p = row(n, k);
                }
            }
            i -= p;
            put(y, (k0 - k + s) ^ s, &mut yy);
        } else {
            // Many dimensions.
            let p = row(k, n);
            let q = row(k + 1, n);
            if p <= i && i < q {
                i -= p;
                put(y, 0, &mut yy);
            } else {
                let s = -i32::from(i >= q);
                i -= q & s as u32;
                let k0 = k;
                let mut p;
                loop {
                    k -= 1;
                    p = row(k, n);
                    if p <= i {
                        break;
                    }
                }
                i -= p;
                put(y, (k0 - k + s) ^ s, &mut yy);
            }
        }
        n -= 1;
    }
    // n == 2.
    let p = (2 * k + 1) as u32;
    let s = -i32::from(i >= p);
    i -= p & s as u32;
    let k0 = k;
    k = ((i + 1) >> 1) as i32;
    if k != 0 {
        i -= (2 * k - 1) as u32;
    }
    put(y, (k0 - k + s) ^ s, &mut yy);
    // n == 1.
    let s = -(i as i32);
    put(y, (k + s) ^ s, &mut yy);
    yy
}

/// `decode_pulses`: a band's `k` pulses in `n` dimensions into `y`; their
/// squared norm.
pub(crate) fn decode_pulses(y: &mut [i32], n: i32, k: i32, dec: &mut Decoder<'_>) -> i32 {
    let index = dec.uint(pvq_v(n, k));
    cwrsi(n, k, index, y)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]
mod tests {
    use super::*;

    #[test]
    fn the_counts_follow_their_recurrence() {
        // V(N, 1) = 2N; V(2, K) = 4K; V(N, K) = V(N-1, K) + V(N, K-1) +
        // V(N-1, K-1).
        for n in 2..10 {
            assert_eq!(pvq_v(n, 1), 2 * n as u32);
        }
        for k in 1..20 {
            assert_eq!(pvq_v(2, k), 4 * k as u32);
        }
        for n in 3..12 {
            for k in 2..12 {
                assert_eq!(
                    pvq_v(n, k),
                    pvq_v(n - 1, k) + pvq_v(n, k - 1) + pvq_v(n - 1, k - 1),
                    "V({n}, {k})"
                );
            }
        }
    }

    #[test]
    fn every_codeword_has_k_pulses_and_its_own_vector() {
        for (n, k) in [(2, 3), (3, 2), (4, 4), (5, 1)] {
            let mut seen = std::collections::HashSet::new();
            for i in 0..pvq_v(n, k) {
                let mut y = vec![0; n as usize];
                let yy = cwrsi(n, k, i, &mut y);
                assert_eq!(y.iter().map(|v: &i32| v.abs()).sum::<i32>(), k);
                assert_eq!(yy, y.iter().map(|v| v * v).sum::<i32>());
                assert!(seen.insert(y), "codeword {i} of V({n}, {k}) repeated");
            }
        }
    }
}
