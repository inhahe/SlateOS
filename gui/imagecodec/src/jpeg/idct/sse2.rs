//! [`super::islow`] in SSE2: libjpeg-turbo's `jidctint-sse2` arrangement,
//! held to the plain C transform's answer rather than to the SIMD one's.
//!
//! The arithmetic is the C code's, regrouped so that every multiplication
//! is a pair of 16-bit values times a pair of constants summed into 32 bits
//! (`pmaddwd`): `z1 = (z2 + z3) * c1; tmp2 = z1 - z3 * c2` becomes
//! `tmp2 = z2 * c1 + z3 * (c1 - c2)`, and so on. Those are the same
//! integers, so where no value outgrows its lane the results are the C
//! code's to the bit. What keeps every value inside its lane is a bound the
//! caller does not have to know about: the function checks that each
//! dequantised coefficient, and each value between the two passes, is at
//! most 16,383 in magnitude -- so that the sum of two, which the
//! regrouping forms in 16 bits, still fits -- and hands the block back
//! (`None`) when one is not. Every picture an encoder makes is far inside
//! it (a coefficient of 8-bit samples is at most about 2,000); a block past
//! it is a corrupt file's, and [`super::islow`] does it in libjpeg's 64-bit
//! arithmetic, wrap-arounds and all.
//!
//! The output stage is exact for any input, not only inside the bound:
//! libjpeg keeps each sample's descaled value modulo 1024 and looks it up in
//! its range-limit table, and so does this -- a mask, then the table's
//! arithmetic -- where libjpeg-turbo's own SIMD saturates instead, and so
//! disagrees with its C code on corrupt files.
//!
//! Where 32-bit intermediates could overflow: the largest is an output of
//! either pass before its descale, at most 61,214 times the largest input
//! (31,521 from the even part and 29,693 from the odd), which for inputs
//! of at most 16,383 is about 1.0 * 2^30.

use core::arch::x86_64::{
    __m128i, _mm_add_epi16, _mm_add_epi32, _mm_and_si128, _mm_cmpeq_epi16, _mm_cmpgt_epi16,
    _mm_cvtsi128_si64, _mm_madd_epi16, _mm_movemask_epi8, _mm_mulhi_epi16, _mm_mullo_epi16,
    _mm_or_si128, _mm_packs_epi32, _mm_packus_epi16, _mm_set_epi16, _mm_set1_epi16, _mm_set1_epi32,
    _mm_srai_epi16, _mm_srai_epi32, _mm_srli_si128, _mm_sub_epi16, _mm_sub_epi32,
    _mm_unpackhi_epi16, _mm_unpackhi_epi32, _mm_unpackhi_epi64, _mm_unpacklo_epi16,
    _mm_unpacklo_epi32, _mm_unpacklo_epi64, _mm_xor_si128,
};

use super::{
    CONST_BITS, FIX_0_298631336, FIX_0_390180644, FIX_0_541196100, FIX_0_765366865,
    FIX_0_899976223, FIX_1_175875602, FIX_1_501321110, FIX_1_847759065, FIX_1_961570560,
    FIX_2_053119869, FIX_2_562915447, FIX_3_072711026, PASS1_BITS,
};

/// The largest magnitude a value may have going into either pass.
const BOUND: i16 = 16_383;

/// A pair of constants for `pmaddwd`: `(a, b)` in every even and odd
/// 16-bit lane. Each is under 2^15 in magnitude, as the `as` requires.
#[inline]
#[target_feature(enable = "sse2")]
fn pair(a: i64, b: i64) -> __m128i {
    let (a, b) = (a as i16, b as i16);
    _mm_set_epi16(b, a, b, a, b, a, b, a)
}

/// Eight 16-bit lanes from a row of eight.
#[inline]
#[target_feature(enable = "sse2")]
fn load(row: &[i16; 8]) -> __m128i {
    let [a, b, c, d, e, f, g, h] = *row;
    _mm_set_epi16(h, g, f, e, d, c, b, a)
}

/// Whether every 16-bit lane of `v` is within `-BOUND..=BOUND`, as an
/// all-ones or all-zeros mask per lane.
#[inline]
#[target_feature(enable = "sse2")]
fn within(v: __m128i) -> __m128i {
    let high = _mm_cmpgt_epi16(v, _mm_set1_epi16(BOUND));
    let low = _mm_cmpgt_epi16(_mm_set1_epi16(-BOUND), v);
    // All ones where neither is set.
    _mm_cmpeq_epi16(_mm_or_si128(high, low), _mm_set1_epi16(0))
}

/// A 16-bit 8x8 transpose: `rows[r]` lane `c` becomes the result's `[c]`
/// lane `r`.
#[inline]
#[target_feature(enable = "sse2")]
fn transpose(rows: [__m128i; 8]) -> [__m128i; 8] {
    let [r0, r1, r2, r3, r4, r5, r6, r7] = rows;
    let a0 = _mm_unpacklo_epi16(r0, r1);
    let a1 = _mm_unpackhi_epi16(r0, r1);
    let a2 = _mm_unpacklo_epi16(r2, r3);
    let a3 = _mm_unpackhi_epi16(r2, r3);
    let a4 = _mm_unpacklo_epi16(r4, r5);
    let a5 = _mm_unpackhi_epi16(r4, r5);
    let a6 = _mm_unpacklo_epi16(r6, r7);
    let a7 = _mm_unpackhi_epi16(r6, r7);
    let b0 = _mm_unpacklo_epi32(a0, a2);
    let b1 = _mm_unpackhi_epi32(a0, a2);
    let b2 = _mm_unpacklo_epi32(a1, a3);
    let b3 = _mm_unpackhi_epi32(a1, a3);
    let b4 = _mm_unpacklo_epi32(a4, a6);
    let b5 = _mm_unpackhi_epi32(a4, a6);
    let b6 = _mm_unpacklo_epi32(a5, a7);
    let b7 = _mm_unpackhi_epi32(a5, a7);
    [
        _mm_unpacklo_epi64(b0, b4),
        _mm_unpackhi_epi64(b0, b4),
        _mm_unpacklo_epi64(b1, b5),
        _mm_unpackhi_epi64(b1, b5),
        _mm_unpacklo_epi64(b2, b6),
        _mm_unpackhi_epi64(b2, b6),
        _mm_unpacklo_epi64(b3, b7),
        _mm_unpackhi_epi64(b3, b7),
    ]
}

/// The two halves of a 32-bit result: lanes 0-3 and 4-7 of an 8-lane row.
#[derive(Clone, Copy)]
struct Wide {
    lo: __m128i,
    hi: __m128i,
}

#[inline]
#[target_feature(enable = "sse2")]
fn wadd(a: Wide, b: Wide) -> Wide {
    Wide {
        lo: _mm_add_epi32(a.lo, b.lo),
        hi: _mm_add_epi32(a.hi, b.hi),
    }
}

#[inline]
#[target_feature(enable = "sse2")]
fn wsub(a: Wide, b: Wide) -> Wide {
    Wide {
        lo: _mm_sub_epi32(a.lo, b.lo),
        hi: _mm_sub_epi32(a.hi, b.hi),
    }
}

/// `x * a + y * b` in each of the eight lanes, 32-bit.
#[inline]
#[target_feature(enable = "sse2")]
fn madd(x: __m128i, y: __m128i, constants: __m128i) -> Wide {
    Wide {
        lo: _mm_madd_epi16(_mm_unpacklo_epi16(x, y), constants),
        hi: _mm_madd_epi16(_mm_unpackhi_epi16(x, y), constants),
    }
}

/// `DESCALE` by `N`: add half, shift right arithmetically.
#[inline]
#[target_feature(enable = "sse2")]
fn descale<const N: i32>(v: Wide) -> Wide {
    let half = _mm_set1_epi32(1i32.wrapping_shl(N.wrapping_sub(1) as u32));
    Wide {
        lo: _mm_srai_epi32::<N>(_mm_add_epi32(v.lo, half)),
        hi: _mm_srai_epi32::<N>(_mm_add_epi32(v.hi, half)),
    }
}

// The two descales, as numbers the shift instruction can take: the first
// pass's `CONST_BITS - PASS1_BITS` and the second's `CONST_BITS +
// PASS1_BITS + 3`.
const _: () = assert!(CONST_BITS - PASS1_BITS == 11 && CONST_BITS + PASS1_BITS + 3 == 18);

/// One pass: the eight-point transform on every lane, `x[k]` being input
/// `k` of each, giving the eight outputs before their descale.
#[inline]
#[target_feature(enable = "sse2")]
fn idct8(x: [__m128i; 8]) -> [Wide; 8] {
    let [x0, x1, x2, x3, x4, x5, x6, x7] = x;
    // Even part.
    let tmp3 = madd(
        x2,
        x6,
        pair(FIX_0_541196100 + FIX_0_765366865, FIX_0_541196100),
    );
    let tmp2 = madd(
        x2,
        x6,
        pair(FIX_0_541196100, FIX_0_541196100 - FIX_1_847759065),
    );
    // `LEFT_SHIFT(z2 +- z3, CONST_BITS)`, as multiplications by 2^13.
    let one = 1i64 << CONST_BITS;
    let tmp0 = madd(x0, x4, pair(one, one));
    let tmp1 = madd(x0, x4, pair(one, one.wrapping_neg()));
    let tmp10 = wadd(tmp0, tmp3);
    let tmp13 = wsub(tmp0, tmp3);
    let tmp11 = wadd(tmp1, tmp2);
    let tmp12 = wsub(tmp1, tmp2);
    // Odd part: inputs 7, 5, 3 and 1 are libjpeg's tmp0 to tmp3. Two sums
    // in 16 bits, which the bound keeps inside their lanes.
    let sum73 = _mm_add_epi16(x7, x3);
    let sum51 = _mm_add_epi16(x5, x1);
    let z3 = madd(
        sum73,
        sum51,
        pair(FIX_1_175875602 - FIX_1_961570560, FIX_1_175875602),
    );
    let z4 = madd(
        sum73,
        sum51,
        pair(FIX_1_175875602, FIX_1_175875602 - FIX_0_390180644),
    );
    let t0 = wadd(
        madd(
            x7,
            x1,
            pair(FIX_0_298631336 - FIX_0_899976223, -FIX_0_899976223),
        ),
        z3,
    );
    let t3 = wadd(
        madd(
            x7,
            x1,
            pair(-FIX_0_899976223, FIX_1_501321110 - FIX_0_899976223),
        ),
        z4,
    );
    let t1 = wadd(
        madd(
            x5,
            x3,
            pair(FIX_2_053119869 - FIX_2_562915447, -FIX_2_562915447),
        ),
        z4,
    );
    let t2 = wadd(
        madd(
            x5,
            x3,
            pair(-FIX_2_562915447, FIX_3_072711026 - FIX_2_562915447),
        ),
        z3,
    );
    [
        wadd(tmp10, t3),
        wadd(tmp11, t2),
        wadd(tmp12, t1),
        wadd(tmp13, t0),
        wsub(tmp13, t0),
        wsub(tmp12, t1),
        wsub(tmp11, t2),
        wsub(tmp10, t3),
    ]
}

/// The block's samples, a row of eight bytes at a time, or `None` if a
/// value falls outside the bound (see the module docs) -- `quant` included:
/// a quantiser over 32,767 does not fit a signed lane.
#[target_feature(enable = "sse2")]
pub(super) fn islow(coef: &[i16; 64], quant: &[u16; 64]) -> Option<[[u8; 8]; 8]> {
    let coef_rows = coef.as_chunks::<8>().0;
    let quant_rows = quant.as_chunks::<8>().0;
    // Dequantise: the exact 32-bit product is `hi:lo`, and it is in bounds
    // when `hi` is `lo`'s sign and `lo` is within the bound.
    let mut rows = [_mm_set1_epi16(0); 8];
    let mut ok = _mm_set1_epi16(-1);
    for ((slot, c), q) in rows.iter_mut().zip(coef_rows).zip(quant_rows) {
        let q: [i16; 8] = q.map(|v| v as i16);
        let (c, q) = (load(c), load(&q));
        // A quantiser read as negative is one past 32,767.
        let q_ok = _mm_cmpgt_epi16(q, _mm_set1_epi16(-1));
        let lo = _mm_mullo_epi16(c, q);
        let hi = _mm_mulhi_epi16(c, q);
        let fits = _mm_cmpeq_epi16(hi, _mm_srai_epi16::<15>(lo));
        ok = _mm_and_si128(ok, _mm_and_si128(q_ok, _mm_and_si128(fits, within(lo))));
        *slot = lo;
    }
    if _mm_movemask_epi8(ok) != 0xFFFF {
        return None;
    }
    // Pass 1: columns, a lane each.
    let pass1 = idct8(rows).map(|v| {
        let v = descale::<11>(v);
        _mm_packs_epi32(v.lo, v.hi)
    });
    // Between the passes: every value in bounds (a saturated one is out).
    let mut ok = _mm_set1_epi16(-1);
    for &v in &pass1 {
        ok = _mm_and_si128(ok, within(v));
    }
    if _mm_movemask_epi8(ok) != 0xFFFF {
        return None;
    }
    // Pass 2: rows, a lane each.
    let pass2 = idct8(transpose(pass1)).map(|v| {
        let v = descale::<18>(v);
        // `range_limit[x & 1023]`: ten bits, read as signed, level-shifted
        // by 128 and clamped to a byte.
        let mask = _mm_set1_epi32(1023);
        let ten = _mm_packs_epi32(_mm_and_si128(v.lo, mask), _mm_and_si128(v.hi, mask));
        let signed = _mm_sub_epi16(_mm_xor_si128(ten, _mm_set1_epi16(512)), _mm_set1_epi16(512));
        _mm_add_epi16(signed, _mm_set1_epi16(128))
    });
    // `pass2[c]` lane `r` is row `r`'s sample `c`: turn it back, then
    // saturate to bytes, two rows to a register.
    let [r0, r1, r2, r3, r4, r5, r6, r7] = transpose(pass2);
    let bytes = [
        _mm_packus_epi16(r0, r1),
        _mm_packus_epi16(r2, r3),
        _mm_packus_epi16(r4, r5),
        _mm_packus_epi16(r6, r7),
    ];
    let mut out = [[0u8; 8]; 8];
    for (pair_of_rows, v) in out.as_chunks_mut::<2>().0.iter_mut().zip(bytes) {
        let [first, second] = pair_of_rows;
        *first = (_mm_cvtsi128_si64(v) as u64).to_le_bytes();
        *second = (_mm_cvtsi128_si64(_mm_srli_si128::<8>(v)) as u64).to_le_bytes();
    }
    Some(out)
}
