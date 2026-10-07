//! The one CELT mode Opus uses: 48 kHz, 20 ms frames of 960 samples, 21
//! bands, a 120-sample overlap (libopus's `mode48000_960_120`), and the
//! lookups into its tables the decoder makes.
//!
//! Translated into Rust from libopus 1.5.2's `celt/modes.h`,
//! `celt/static_modes_fixed.h` and `celt/rate.h`, copyright Xiph.Org and the
//! contributors named in its `COPYING`, used under libopus's BSD licence
//! (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "band numbers are below NB_EBANDS, LM below 4 and a cache entry's own first byte bounds its lookups, as libopus's do"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "small integers: band edges below 100, LM below 4, bit counts in eighths of a frame's at most 10,200 bits"
)]

use super::tables::{
    BAND_ALLOCATION, CACHE_BITS50, CACHE_CAPS50, CACHE_INDEX50, EBAND5MS, LOGN400, WINDOW120,
};

/// Samples the windows of consecutive frames overlap by.
pub(crate) const OVERLAP: usize = 120;
/// Bands in a frame.
pub(crate) const NB_EBANDS: usize = 21;
/// Bands whose energy is coded.
pub(crate) const EFF_EBANDS: usize = 21;
/// The pre-emphasis filter: its coefficient and its inverse's terms, Q15
/// and Q12.
pub(crate) const PREEMPH: [i32; 4] = [27853, 0, 4096, 8192];
/// The largest frame size, as a shift of the shortest (2.5 ms): 20 ms.
pub(crate) const MAX_LM: i32 = 3;
/// The short MDCT's size.
pub(crate) const SHORT_MDCT_SIZE: usize = 120;
/// Rows of the allocation table.
pub(crate) const NB_ALLOC_VECTORS: usize = 11;

/// `eBands[i]`: band `i`'s first bin at the shortest frame.
#[inline]
pub(crate) fn ebands(i: usize) -> i32 {
    i32::from(EBAND5MS[i])
}

/// `allocVectors[i]`.
#[inline]
pub(crate) fn alloc_vector(i: usize) -> i32 {
    i32::from(BAND_ALLOCATION[i])
}

/// `logN[i]`.
#[inline]
pub(crate) fn log_n(i: usize) -> i32 {
    i32::from(LOGN400[i])
}

/// The overlap window.
pub(crate) fn window() -> &'static [i16] {
    &WINDOW120
}

/// `cache.caps[i]`.
#[inline]
pub(crate) fn cache_cap(i: usize) -> i32 {
    i32::from(CACHE_CAPS50[i])
}

/// The pulse cache for `band` at frame size `lm` (`m->cache.bits +
/// m->cache.index[(LM+1)*nbEBands+band]`): its first byte is the largest
/// pseudo-pulse count, then the bits each count costs.
fn cache(band: usize, lm: i32) -> &'static [u8] {
    let at = CACHE_INDEX50[(lm + 1) as usize * NB_EBANDS + band];
    CACHE_BITS50
        .get(usize::try_from(at).unwrap_or(0)..)
        .unwrap_or(&[])
}

/// `cache[cache[0]]`: what the most pseudo-pulses `band` can take at frame
/// size `lm` cost, in eighths of a bit -- above which (and 12 more) a
/// partition is split. Only asked for `lm` of 0 or more, where every band
/// has a cache.
pub(crate) fn cache_top(band: usize, lm: i32) -> i32 {
    let cache = cache(band, lm);
    let top = cache.first().copied().unwrap_or(0);
    i32::from(cache.get(usize::from(top)).copied().unwrap_or(0))
}

/// `get_pulses`: the pulse count of pseudo-pulse count `i`.
#[inline]
pub(crate) const fn get_pulses(i: i32) -> i32 {
    if i < 8 {
        i
    } else {
        (8 + (i & 7)) << ((i >> 3) - 1)
    }
}

/// `bits2pulses`: the pseudo-pulse count `bits` (in eighths) buy in `band`.
pub(crate) fn bits2pulses(band: usize, lm: i32, bits: i32) -> i32 {
    let cache = cache(band, lm);
    let at = |i: i32| i32::from(cache.get(i as usize).copied().unwrap_or(0));
    let mut lo = 0;
    let mut hi = at(0);
    let bits = bits - 1;
    for _ in 0..6 {
        let mid = (lo + hi + 1) >> 1;
        if at(mid) >= bits {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let below = if lo == 0 { -1 } else { at(lo) };
    if bits - below <= at(hi) - bits {
        lo
    } else {
        hi
    }
}

/// `pulses2bits`: what `pulses` pseudo-pulses cost in `band`, in eighths.
pub(crate) fn pulses2bits(band: usize, lm: i32, pulses: i32) -> i32 {
    if pulses == 0 {
        0
    } else {
        i32::from(cache(band, lm).get(pulses as usize).copied().unwrap_or(0)) + 1
    }
}
