//! [`super::ycc_argb`] in SSE2, eight pixels at a time: the integers
//! [`super::ycc_to_rgb`] computes, computed side by side.
//!
//! Each channel's correction is one `pmaddwd`: a pair of 16-bit values times
//! a pair of constants, summed exactly into 32 bits. Red's is
//! `R_CR * xr + 16384 * 2` -- the rounding term `ONE_HALF` is 2^15, one past
//! what a signed 16-bit lane holds, so it goes in as 2 times 2^14 -- then
//! shifted right by 16; green's is `G_CB * xb + G_CR * xr` plus `ONE_HALF`;
//! blue's is red's with `B_CB` and `xb`. Every step is exact, so the pixels
//! are [`super::ycc_to_rgb`]'s, clamped as libjpeg's range limit clamps, and
//! a test compares the two on every input.

use core::arch::x86_64::{
    __m128i, _mm_add_epi16, _mm_add_epi32, _mm_cvtsi128_si64, _mm_madd_epi16, _mm_packs_epi32,
    _mm_packus_epi16, _mm_set_epi16, _mm_set_epi64x, _mm_set1_epi16, _mm_set1_epi32,
    _mm_setzero_si128, _mm_slli_epi16, _mm_srai_epi32, _mm_srli_si128, _mm_sub_epi16,
    _mm_unpackhi_epi16, _mm_unpacklo_epi8, _mm_unpacklo_epi16,
};

use super::{B_CB, G_CB, G_CR, R_CR, SCALE};

// The shift below is `SCALE`, written as the number the instruction takes.
const _: () = assert!(SCALE == 16);

/// A pair of constants for `pmaddwd`: `(a, b)` in every even and odd lane.
#[inline]
#[target_feature(enable = "sse2")]
fn pair(a: i32, b: i32) -> __m128i {
    let (a, b) = (a as i16, b as i16);
    _mm_set_epi16(b, a, b, a, b, a, b, a)
}

/// Eight bytes as eight 16-bit lanes.
#[inline]
#[target_feature(enable = "sse2")]
fn widen(bytes: &[u8; 8]) -> __m128i {
    let low = _mm_set_epi64x(0, i64::from_le_bytes(*bytes));
    _mm_unpacklo_epi8(low, _mm_setzero_si128())
}

/// `(x * a + y * b + rounding) >> 16` in each of eight lanes, where the
/// 32-bit sum is exact, packed back to 16 bits (it is within +-400).
#[inline]
#[target_feature(enable = "sse2")]
fn term(x: __m128i, y: __m128i, constants: __m128i, rounding: __m128i) -> __m128i {
    let lo = _mm_madd_epi16(_mm_unpacklo_epi16(x, y), constants);
    let hi = _mm_madd_epi16(_mm_unpackhi_epi16(x, y), constants);
    let lo = _mm_srai_epi32::<16>(_mm_add_epi32(lo, rounding));
    let hi = _mm_srai_epi32::<16>(_mm_add_epi32(hi, rounding));
    _mm_packs_epi32(lo, hi)
}

/// Opaque `0xAARRGGBB` pixels from rows of Y, Cb and Cr, eight at a time;
/// the number of pixels written, a multiple of eight, is returned and the
/// rest left to the caller.
#[target_feature(enable = "sse2")]
pub(super) fn ycc_argb(y: &[u8], cb: &[u8], cr: &[u8], out: &mut [u32]) -> usize {
    let centre = _mm_set1_epi16(128);
    let two = _mm_set1_epi16(2);
    let none = _mm_setzero_si128();
    let red = pair(R_CR, 1 << 14);
    let green = pair(G_CB, G_CR);
    let blue = pair(B_CB, 1 << 14);
    let half = _mm_set1_epi32(1 << 15);
    let opaque = _mm_set1_epi16(-1);
    let mut done = 0usize;
    let rows = y
        .as_chunks::<8>()
        .0
        .iter()
        .zip(cb.as_chunks::<8>().0)
        .zip(cr.as_chunks::<8>().0)
        .zip(out.as_chunks_mut::<8>().0);
    for (((y, cb), cr), out) in rows {
        let y = widen(y);
        let xb = _mm_sub_epi16(widen(cb), centre);
        let xr = _mm_sub_epi16(widen(cr), centre);
        // R = y + xr + (R_CR * xr + 2^15) >> 16.
        let r = _mm_add_epi16(_mm_add_epi16(y, xr), term(xr, two, red, none));
        // G = y - xr + (G_CB * xb + G_CR * xr + 2^15) >> 16.
        let g = _mm_add_epi16(_mm_sub_epi16(y, xr), term(xb, xr, green, half));
        // B = y + 2 xb + (B_CB * xb + 2^15) >> 16.
        let b = _mm_add_epi16(
            _mm_add_epi16(y, _mm_slli_epi16::<1>(xb)),
            term(xb, two, blue, none),
        );
        // Saturate to bytes, then interleave B, G, R, A into pixels, which
        // in little-endian memory are 0xAARRGGBB.
        let (r, g, b) = (
            _mm_packus_epi16(r, none),
            _mm_packus_epi16(g, none),
            _mm_packus_epi16(b, none),
        );
        let bg = _mm_unpacklo_epi8(b, g);
        let ra = _mm_unpacklo_epi8(r, opaque);
        let first = _mm_unpacklo_epi16(bg, ra);
        let second = _mm_unpackhi_epi16(bg, ra);
        let words = [
            _mm_cvtsi128_si64(first) as u64,
            _mm_cvtsi128_si64(_mm_srli_si128::<8>(first)) as u64,
            _mm_cvtsi128_si64(second) as u64,
            _mm_cvtsi128_si64(_mm_srli_si128::<8>(second)) as u64,
        ];
        for (pixels, word) in out.as_chunks_mut::<2>().0.iter_mut().zip(words) {
            *pixels = [word as u32, (word >> 32) as u32];
        }
        done = done.saturating_add(8);
    }
    done
}
