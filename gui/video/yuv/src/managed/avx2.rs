//! The HDR conversion's table passes, eight pixels at a time with AVX2:
//! the transfer curves, HLG's OOTF power, the tone map's gain and the 8-bit
//! encoding -- the six lookups a pixel that kept the scalar passes at some
//! 25 ns a pixel. Each function computes what its scalar twin in the parent
//! module computes, operation for operation and in the same order (no fused
//! multiply-add), so the two give the same bits; a test holds them to it.
//! The arithmetic passes between them stay the parent's, which the compiler
//! already runs four pixels at a time.
//!
//! Chosen at run time: the processor must have AVX2 and the system must
//! save the AVX registers (CPUID's OSXSAVE and AVX bits, then XCR0's SSE
//! and AVX state), which is what lets this crate stay built for the baseline
//! x86-64 (design-decisions §1373). The answer is an [`Avx2`], which only
//! [`detect`] makes: every function here takes one, so none can run where
//! AVX2 does not. Any part of a row short of eight pixels goes through the
//! scalar code itself.

use core::arch::x86_64::{
    __m256, __m256i, _CMP_GE_OQ, _CMP_LE_OQ, _mm256_add_epi32, _mm256_add_ps, _mm256_and_si256,
    _mm256_blendv_ps, _mm256_castps_si256, _mm256_castsi256_ps, _mm256_cmp_ps, _mm256_cmpgt_epi32,
    _mm256_cvtepi32_ps, _mm256_cvttps_epi32, _mm256_div_ps, _mm256_i32gather_epi32,
    _mm256_i32gather_ps, _mm256_loadu_ps, _mm256_max_epi32, _mm256_max_ps, _mm256_min_epi32,
    _mm256_min_ps, _mm256_movemask_ps, _mm256_mul_ps, _mm256_or_si256, _mm256_set1_epi32,
    _mm256_set1_ps, _mm256_setzero_ps, _mm256_setzero_si256, _mm256_slli_epi32, _mm256_srli_epi32,
    _mm256_storeu_ps, _mm256_storeu_si256, _mm256_sub_epi32, _mm256_sub_ps,
};
use core::sync::atomic::{AtomicU8, Ordering};

use super::{CELLS, Cells, Curve, Power, SEGMENTS, Table, code, hlg, interpolate};

/// Lanes a vector holds.
const LANES: usize = 8;

// The casts of the tables' sizes below take them to be exact in `f32` and
// in `i32`.
const _: () = assert!(SEGMENTS < 1 << 24 && CELLS < 1 << 24);

/// The last segment a lane's index can start: [`interpolate`]'s top.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "SEGMENTS is under 2^24, exact in i32"
)]
const LAST_SEGMENT: i32 = SEGMENTS as i32 - 1;

/// The last cell a lane's light can be in: [`code`]'s top.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "CELLS is under 2^24, exact in i32"
)]
const LAST_CELL: i32 = CELLS as i32 - 1;

/// Proof that AVX2 runs here: made only by [`detect`].
#[derive(Clone, Copy, Debug)]
pub(super) struct Avx2(());

/// An [`Avx2`] if the processor runs AVX2 and the system saves its
/// registers: asked of the processor once, then kept.
pub(super) fn detect() -> Option<Avx2> {
    /// 0 not yet asked, 1 no, 2 yes.
    static KNOWN: AtomicU8 = AtomicU8::new(0);
    let yes = match KNOWN.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let yes = ask();
            KNOWN.store(if yes { 2 } else { 1 }, Ordering::Relaxed);
            yes
        }
    };
    yes.then_some(Avx2(()))
}

/// CPUID's AVX2, and the system's saving of the registers AVX uses.
fn ask() -> bool {
    use core::arch::x86_64::{__cpuid, __cpuid_count};
    const OSXSAVE: u32 = 1 << 27;
    const AVX: u32 = 1 << 28;
    const AVX2: u32 = 1 << 5;
    // XCR0's SSE (bit 1) and AVX (bit 2) state.
    const SAVED: u64 = 0b110;
    if __cpuid(0).eax < 7 {
        return false;
    }
    let one = __cpuid(1);
    if one.ecx & (OSXSAVE | AVX) != OSXSAVE | AVX {
        return false;
    }
    // SAFETY: CPUID's OSXSAVE bit, just checked, says the system has turned
    // XSAVE on, which is what XGETBV needs.
    let xcr0 = unsafe { xgetbv0() };
    xcr0 & SAVED == SAVED && __cpuid_count(7, 0).ebx & AVX2 != 0
}

/// XCR0.
///
/// # Safety
///
/// The system must have turned XSAVE on (CPUID's OSXSAVE).
#[target_feature(enable = "xsave")]
unsafe fn xgetbv0() -> u64 {
    // SAFETY: the caller's guarantee.
    unsafe { core::arch::x86_64::_xgetbv(0) }
}

/// The PQ table at every value of `values`, in place.
pub(super) fn interpolate_all(_: Avx2, table: &Table, values: &mut [f32]) {
    // SAFETY: the `Avx2` says AVX2 runs here.
    unsafe { interpolate_rows(table, values) }
}

/// The HLG table at every value of `values`, in place.
pub(super) fn hlg_all(_: Avx2, table: &Table, values: &mut [f32]) {
    // SAFETY: the `Avx2` says AVX2 runs here.
    unsafe { hlg_rows(table, values) }
}

/// HLG's OOTF factor for each pixel into `k`: `power` of its luminance by
/// `weights`, as the scalar pass computes it.
pub(super) fn power_all(
    _: Avx2,
    power: &Power,
    weights: [f32; 3],
    [r, g, b]: [&[f32]; 3],
    k: &mut [f32],
) {
    if power.exponents.len() != 256 || power.mantissas.len() != 1025 {
        // Not the tables `Power::new` makes: the scalar way, which checks
        // every index.
        let [wr, wg, wb] = weights;
        for (((&r, &g), &b), k) in r.iter().zip(g).zip(b).zip(k.iter_mut()) {
            *k = power.of(wr * r + wg * g + wb * b);
        }
        return;
    }
    // SAFETY: the `Avx2` says AVX2 runs here, and the tables are the lengths
    // `power_rows` needs, just checked.
    unsafe { power_rows(power, weights, [r, g, b], k) }
}

/// [`Curve::gain`] of every value of `k`, in place.
pub(super) fn gain_all(_: Avx2, curve: &Curve, k: &mut [f32]) {
    // SAFETY: the `Avx2` says AVX2 runs here.
    unsafe { gain_rows(curve, k) }
}

/// The 8-bit codes of each pixel's three channels into `out`, as
/// `0x00RRGGBB`.
pub(super) fn code_all(_: Avx2, cells: &Cells, [r, g, b]: [&[f32]; 3], out: &mut [u32]) {
    // SAFETY: the `Avx2` says AVX2 runs here.
    unsafe { code_rows(cells, [r, g, b], out) }
}

/// [`interpolate`] of eight values at once.
///
/// # Safety
///
/// AVX2 must run here.
#[target_feature(enable = "avx2")]
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "SEGMENTS is under 2^24, exact in f32"
)]
unsafe fn interpolate8(table: &Table, x: __m256) -> __m256 {
    let at = _mm256_mul_ps(x, _mm256_set1_ps(SEGMENTS as f32));
    // The scalar cast saturates: a negative or NaN index is 0, a large one
    // the last segment. Here the large are cut to the last in floating point
    // first -- the bound *first* in `min`, which returns its second operand
    // when either is NaN, so that NaN stays NaN -- the truncation then makes
    // NaN and negatives negative or 0, and `max` with 0 takes them to 0.
    let capped = _mm256_min_ps(_mm256_set1_ps(LAST_SEGMENT as f32), at);
    let i = _mm256_max_epi32(_mm256_cvttps_epi32(capped), _mm256_setzero_si256());
    let i = _mm256_min_epi32(i, _mm256_set1_epi32(LAST_SEGMENT));
    let frac = _mm256_sub_ps(at, _mm256_cvtepi32_ps(i));
    // SAFETY: every index is within [0, SEGMENTS - 1] by the clamps above,
    // and the one after it within [1, SEGMENTS]: inside the table of
    // SEGMENTS + 1. AVX2 is the caller's guarantee.
    let (a, b) = unsafe {
        (
            _mm256_i32gather_ps::<4>(table.as_ptr(), i),
            _mm256_i32gather_ps::<4>(table.as_ptr(), _mm256_add_epi32(i, _mm256_set1_epi32(1))),
        )
    };
    _mm256_add_ps(a, _mm256_mul_ps(frac, _mm256_sub_ps(b, a)))
}

/// [`interpolate_all`]'s work.
///
/// # Safety
///
/// AVX2 must run here.
#[target_feature(enable = "avx2")]
unsafe fn interpolate_rows(table: &Table, values: &mut [f32]) {
    let mut chunks = values.chunks_exact_mut(LANES);
    for chunk in &mut chunks {
        // SAFETY: `chunk` is eight floats, read and written in place; AVX2
        // is the caller's guarantee.
        unsafe {
            let x = _mm256_loadu_ps(chunk.as_ptr());
            _mm256_storeu_ps(chunk.as_mut_ptr(), interpolate8(table, x));
        }
    }
    for x in chunks.into_remainder() {
        *x = interpolate(table, *x);
    }
}

/// [`hlg_all`]'s work: both of [`hlg`]'s halves for eight values at once,
/// the one each value is in kept.
///
/// # Safety
///
/// AVX2 must run here.
#[target_feature(enable = "avx2")]
unsafe fn hlg_rows(table: &Table, values: &mut [f32]) {
    let mut chunks = values.chunks_exact_mut(LANES);
    for chunk in &mut chunks {
        // SAFETY: `chunk` is eight floats, read and written in place; AVX2
        // is the caller's guarantee.
        unsafe {
            let c = _mm256_loadu_ps(chunk.as_ptr());
            let twice = _mm256_mul_ps(_mm256_set1_ps(2.0), c);
            let low = _mm256_div_ps(_mm256_mul_ps(twice, twice), _mm256_set1_ps(12.0));
            let upper = _mm256_mul_ps(_mm256_sub_ps(c, _mm256_set1_ps(0.5)), _mm256_set1_ps(2.0));
            let high = interpolate8(table, upper);
            // NaN is not at or below a half: the table's, as the scalar's.
            let is_low = _mm256_cmp_ps::<_CMP_LE_OQ>(c, _mm256_set1_ps(0.5));
            _mm256_storeu_ps(chunk.as_mut_ptr(), _mm256_blendv_ps(high, low, is_low));
        }
    }
    for c in chunks.into_remainder() {
        *c = hlg(table, *c);
    }
}

/// [`Power::of`] of eight values at once.
///
/// # Safety
///
/// AVX2 must run here, and `power`'s tables be 256 exponents and 1025
/// mantissa points long, as [`Power::new`] makes them.
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn power8(power: &Power, y: __m256) -> __m256 {
    let bits = _mm256_castps_si256(y);
    // The exponent with the sign above it: a negative value's is 256 or more.
    let exponent = _mm256_srli_epi32::<23>(bits);
    let normal = _mm256_and_si256(
        _mm256_cmpgt_epi32(exponent, _mm256_setzero_si256()),
        _mm256_cmpgt_epi32(_mm256_set1_epi32(255), exponent),
    );
    // Gathered at 0 where it is not normal, and thrown away there.
    let exponent = _mm256_and_si256(exponent, normal);
    let mantissa = _mm256_and_si256(bits, _mm256_set1_epi32(0x7f_ffff));
    let i = _mm256_srli_epi32::<13>(mantissa);
    let frac = _mm256_div_ps(
        _mm256_cvtepi32_ps(_mm256_and_si256(mantissa, _mm256_set1_epi32(0x1fff))),
        _mm256_set1_ps(8192.0),
    );
    // SAFETY: the exponents gathered are 0 or within [1, 254], inside the 256;
    // `i` is a mantissa's top ten bits, within [0, 1023], and `i + 1` within
    // [1, 1024], inside the 1025 points -- the lengths are the caller's
    // guarantee, as AVX2 is.
    let (scale, a, b) = unsafe {
        (
            _mm256_i32gather_ps::<4>(power.exponents.as_ptr(), exponent),
            _mm256_i32gather_ps::<4>(power.mantissas.as_ptr(), i),
            _mm256_i32gather_ps::<4>(
                power.mantissas.as_ptr(),
                _mm256_add_epi32(i, _mm256_set1_epi32(1)),
            ),
        )
    };
    let normal_power = _mm256_mul_ps(
        scale,
        _mm256_add_ps(a, _mm256_mul_ps(frac, _mm256_sub_ps(b, a))),
    );
    // Not normal: infinity is itself, anything else 0.
    let infinite = _mm256_cmp_ps::<_CMP_GE_OQ>(y, _mm256_set1_ps(f32::INFINITY));
    let other = _mm256_blendv_ps(_mm256_setzero_ps(), y, infinite);
    _mm256_blendv_ps(other, normal_power, _mm256_castsi256_ps(normal))
}

/// [`power_all`]'s work.
///
/// # Safety
///
/// As [`power8`].
#[target_feature(enable = "avx2")]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "floating-point arithmetic, which cannot overflow into undefined behaviour"
)]
unsafe fn power_rows(power: &Power, [wr, wg, wb]: [f32; 3], [r, g, b]: [&[f32]; 3], k: &mut [f32]) {
    let n = k.len().min(r.len()).min(g.len()).min(b.len());
    // Never less than 0, so never saturated.
    let whole = n.saturating_sub(n % LANES);
    let (w_r, w_g, w_b) = (_mm256_set1_ps(wr), _mm256_set1_ps(wg), _mm256_set1_ps(wb));
    let rows = r
        .chunks_exact(LANES)
        .zip(g.chunks_exact(LANES))
        .zip(b.chunks_exact(LANES));
    for (((r, g), b), k) in rows.zip(k.chunks_exact_mut(LANES)) {
        // SAFETY: each slice is eight floats; AVX2 and the tables' lengths
        // are the caller's guarantees.
        unsafe {
            let (r, g, b) = (
                _mm256_loadu_ps(r.as_ptr()),
                _mm256_loadu_ps(g.as_ptr()),
                _mm256_loadu_ps(b.as_ptr()),
            );
            let y = _mm256_add_ps(
                _mm256_add_ps(_mm256_mul_ps(w_r, r), _mm256_mul_ps(w_g, g)),
                _mm256_mul_ps(w_b, b),
            );
            _mm256_storeu_ps(k.as_mut_ptr(), power8(power, y));
        }
    }
    let tail = r.iter().zip(g).zip(b).zip(k.iter_mut()).take(n).skip(whole);
    for (((&r, &g), &b), k) in tail {
        *k = power.of(wr * r + wg * g + wb * b);
    }
}

/// [`gain_all`]'s work: at or below the first point and at or past the last,
/// eight at a time as the scalar computes them; between, a pixel at a time by
/// the scalar itself, whose `exp2f` is libm's.
///
/// # Safety
///
/// AVX2 must run here.
#[target_feature(enable = "avx2")]
#[allow(clippy::arithmetic_side_effects, reason = "a lane's bit is under 8")]
unsafe fn gain_rows(curve: &Curve, k: &mut [f32]) {
    let first = _mm256_set1_ps(curve.points[0][0]);
    let last = _mm256_set1_ps(curve.points[7][0]);
    let (low, top) = (_mm256_set1_ps(curve.low), _mm256_set1_ps(curve.top));
    let mut chunks = k.chunks_exact_mut(LANES);
    for chunk in &mut chunks {
        // SAFETY: `chunk` is eight floats, and `xs` eight more, read and
        // written in place; AVX2 is the caller's guarantee.
        unsafe {
            let x = _mm256_loadu_ps(chunk.as_ptr());
            let at_low = _mm256_cmp_ps::<_CMP_LE_OQ>(x, first);
            let at_top = _mm256_cmp_ps::<_CMP_GE_OQ>(x, last);
            let gains = _mm256_blendv_ps(_mm256_div_ps(top, x), low, at_low);
            let between = !(_mm256_movemask_ps(at_low) | _mm256_movemask_ps(at_top)) & 0xff;
            _mm256_storeu_ps(chunk.as_mut_ptr(), gains);
            if between != 0 {
                let mut xs = [0.0f32; LANES];
                _mm256_storeu_ps(xs.as_mut_ptr(), x);
                for (lane, (out, &x)) in chunk.iter_mut().zip(&xs).enumerate() {
                    if between & (1 << lane) != 0 {
                        *out = curve.gain(x);
                    }
                }
            }
        }
    }
    for k in chunks.into_remainder() {
        *k = curve.gain(*k);
    }
}

/// [`code`] of eight values at once.
///
/// # Safety
///
/// AVX2 must run here.
#[target_feature(enable = "avx2")]
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "CELLS is under 2^24, exact in f32"
)]
unsafe fn code8(cells: &Cells, l: __m256) -> __m256i {
    // The scalar's `clamp` keeps a NaN, which its cast takes to cell 0 and
    // its comparison to no code above that cell's; `max` here returns its
    // second operand, 0, for a NaN: the same cell and the same comparison.
    let l = _mm256_min_ps(_mm256_max_ps(l, _mm256_setzero_ps()), _mm256_set1_ps(1.0));
    let cell = _mm256_cvttps_epi32(_mm256_mul_ps(l, _mm256_set1_ps(CELLS as f32)));
    let cell = _mm256_min_epi32(cell, _mm256_set1_epi32(LAST_CELL));
    // A cell is two 32-bit words, its bound and then its code.
    let word = _mm256_slli_epi32::<1>(cell);
    let base = cells.as_ptr();
    // SAFETY: `cell` is within [0, CELLS - 1] (the light clamped to [0, 1]
    // before it is scaled, then the top cut), so words `2 cell` and
    // `2 cell + 1` are inside the cells' `2 CELLS`; `Cell` is `repr(C)` of a
    // `f32` and a `u32`, so those are its bound and its code. AVX2 is the
    // caller's guarantee.
    let (bound, code) = unsafe {
        (
            _mm256_i32gather_ps::<4>(base.cast::<f32>(), word),
            _mm256_i32gather_epi32::<4>(
                base.cast::<i32>(),
                _mm256_add_epi32(word, _mm256_set1_epi32(1)),
            ),
        )
    };
    // At or past the next code's least light: one more (a true comparison
    // is all ones, -1).
    let next = _mm256_castps_si256(_mm256_cmp_ps::<_CMP_GE_OQ>(l, bound));
    _mm256_sub_epi32(code, next)
}

/// [`code_all`]'s work.
///
/// # Safety
///
/// AVX2 must run here.
#[target_feature(enable = "avx2")]
#[allow(
    clippy::cast_ptr_alignment,
    reason = "`_mm256_storeu_si256` stores to any alignment"
)]
unsafe fn code_rows(cells: &Cells, [r, g, b]: [&[f32]; 3], out: &mut [u32]) {
    let n = out.len().min(r.len()).min(g.len()).min(b.len());
    // Never less than 0, so never saturated.
    let whole = n.saturating_sub(n % LANES);
    let rows = r
        .chunks_exact(LANES)
        .zip(g.chunks_exact(LANES))
        .zip(b.chunks_exact(LANES));
    for (((r, g), b), out) in rows.zip(out.chunks_exact_mut(LANES)) {
        // SAFETY: each slice is eight floats, and `out` eight 32-bit words
        // (`storeu` needs no alignment); AVX2 is the caller's guarantee.
        unsafe {
            let red = code8(cells, _mm256_loadu_ps(r.as_ptr()));
            let green = code8(cells, _mm256_loadu_ps(g.as_ptr()));
            let blue = code8(cells, _mm256_loadu_ps(b.as_ptr()));
            let pixels = _mm256_or_si256(
                _mm256_or_si256(_mm256_slli_epi32::<16>(red), _mm256_slli_epi32::<8>(green)),
                blue,
            );
            _mm256_storeu_si256(out.as_mut_ptr().cast::<__m256i>(), pixels);
        }
    }
    let tail = r
        .iter()
        .zip(g)
        .zip(b)
        .zip(out.iter_mut())
        .take(n)
        .skip(whole);
    for (((&r, &g), &b), px) in tail {
        *px = (code(cells, r) << 16) | (code(cells, g) << 8) | code(cells, b);
    }
}
