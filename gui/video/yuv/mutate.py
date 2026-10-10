"""Mutation test for yuv's HDR conversion (src/managed.rs): Chrome's handling of
a PQ or HLG picture on an sRGB screen, and the chroma upsampling it shares
with libavif's slow path (src/reformat.rs).

Each row puts back one way of not showing what Chrome shows -- a constant of
Skia's misremembered, a step of its tone map dropped, a rounding done
otherwise -- and names the tests that have to notice: the module's own, which
hold it to Chrome 154's pixels for 40 patches, to Skia's control points, and
to a double-precision transcription of the whole conversion.  The AVX2
passes (src/managed/avx2.rs) have rows of their own, each a slip of vector code
that the tests holding them to the scalar passes' bits have to notice.

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"
# The unit tests: managed.rs's and reformat.rs's.
TARGETS = ("--lib",)

PQ = "pq_is_chrome_s_pixel_for_pixel"
HLG = "hlg_is_chrome_s_pixel_for_pixel"
POINTS = "control_points_are_skia_s"
PEAK = "the_peak_is_max_cll_then_mastering_then_1000"
HALF = "half_rounds_as_f16c_does"
ENCODER = "the_encoder_rounds_exactly"
TABLES = "the_tables_err_by_a_ten_thousandth_of_a_code"
REFERENCE = "single_precision_is_the_reference_to_the_last_bit_nearly_always"
CURVE = "the_curve_ends_at_white_and_keeps_the_dark_s_proportions"
ALPHA = "alpha_is_carried_and_premultiplication_undone"
UPSAMPLED = "chroma_is_upsampled_nine_three_three_one"
PRIMARIES = "other_primaries_take_their_own_matrix"

HDR = [
    (
        "the reference white is 200 cd/m2",
        "const REFERENCE_WHITE: f32 = 203.0;",
        "const REFERENCE_WHITE: f32 = 200.0;",
        [PQ, HLG, POINTS],
    ),
    (
        "PQ's light is not scaled to the reference white",
        "            Transfer::Pq => 10_000.0 / REFERENCE_WHITE,",
        "            Transfer::Pq => 1.0,",
        [PQ],
    ),
    (
        "the Bezier's kappa is 0.6",
        "        let kappa = 0.65f32;",
        "        let kappa = 0.6f32;",
        [PQ, HLG, POINTS],
    ),
    (
        "the control points are not rounded to half precision",
        "(x * m - y) / (ln2 * x * y)].map(half);",
        "(x * m - y) / (ln2 * x * y)];",
        [POINTS],
    ),
    (
        "half precision truncates",
        "        f32::from_bits((bits + 0x0fff + keep_lsb) & !0x1fff)",
        "        f32::from_bits(bits & !0x1fff)",
        [HALF, POINTS],
    ),
    (
        "below the reference white there is no gain",
        "        if x <= p[0][0] {\n            return self.low;",
        "        if x <= p[0][0] {\n            return 1.0;",
        [PQ, HLG, CURVE],
    ),
    (
        "past the peak the gain stays the peak's",
        "            return self.top / x;",
        "            return self.top;",
        [CURVE],
    ),
    (
        "the cubic's parameter is not over the segment",
        "        let t = (x - x_i) / h;",
        "        let t = x - x_i;",
        [PQ, HLG, REFERENCE, CURVE],
    ),
    (
        "HLG's system gamma is 1.25",
        "                Some(Power::new(f64::from(HLG_GAMMA - 1.0))),",
        "                Some(Power::new(0.25)),",
        [HLG],
    ),
    (
        "HLG's lower half is over 12 without the doubling",
        "        twice * twice / 12.0",
        "        c * c / 12.0",
        [HLG, TABLES],
    ),
    (
        "HLG's OOTF weighs by BT.709",
        "        let weights = [0.262_700f32, 0.678_000, 0.059_300];",
        "        let weights = [0.2126f32, 0.7152, 0.0722];",
        [HLG],
    ),
    (
        "8 bits round down",
        "        *bound = at_least(srgb_decode((f64::from(k) + 0.5) / 255.0));",
        "        *bound = at_least(srgb_decode((f64::from(k) + 1.0) / 255.0));",
        [ENCODER, PQ, HLG],
    ),
    (
        "a cell's next code is never reached",
        "    code + u32::from(l >= bound)",
        "    code",
        [ENCODER, PQ, HLG],
    ),
    (
        "BT.2020's colours are shown as sRGB's",
        "            to_srgb: gamut_transform(&working, &SRGB),",
        "            to_srgb: diagonal([1.0; 3]),",
        [PQ, HLG, PRIMARIES],
    ),
    (
        "the mastering display's peak outranks MaxCLL",
        "        if self.max_cll > 0.0 {",
        "        if self.max_cll > 0.0 && self.mastering_peak <= 0.0 {",
        [PEAK],
    ),
    (
        "the headroom has no ceiling",
        "            libm::log2f(peak / REFERENCE_WHITE).min(MAX_HEADROOM)",
        "            libm::log2f(peak / REFERENCE_WHITE)",
        [POINTS],
    ),
    (
        "a sample's fraction leaves out the black",
        "        *o = (code.min(max) as f32 - bias) / range;",
        "        *o = code.min(max) as f32 / range;",
        [PQ, HLG],
    ),
    (
        "premultiplied colour is left multiplied",
        "        if let (true, Some(a)) = (self.unmultiply, a) {",
        "        if let (true, Some(a)) = (false, a) {",
        [ALPHA],
    ),
]

# The chroma upsampling shared with libavif's slow path (reformat.rs).
CHROMA = [
    (
        "the nearest chroma sample weighs 10/16",
        "            (at(near, uv_i) * (9.0 / 16.0))",
        "            (at(near, uv_i) * (10.0 / 16.0))",
        [UPSAMPLED],
    ),
    (
        "the diagonal sample is the nearest's row",
        "                + (at(far, adj_col) * (1.0 / 16.0))",
        "                + (at(near, adj_col) * (1.0 / 16.0))",
        [UPSAMPLED],
    ),
]

# The AVX2 passes (managed/avx2.rs), which must give the scalar passes' bits.
# Each row is a slip easy to make in vector code -- an operand order, an
# ordered or unordered comparison, a rounding mode, an association, the
# tail of a row -- that changes some value's bits.  The intrinsics a row
# brings in are named by their paths, as the module does not import them.
PASSES = "avx2_passes_are_the_scalar_passes_bit_for_bit"
ROWS = "avx2_rows_are_the_scalar_rows_bit_for_bit"
DETECTED = "avx2_is_detected_as_the_standard_library_detects_it"
X86 = "core::arch::x86_64::"

AVX = [
    (
        "AVX2 is never detected",
        "    yes.then_some(Avx2(()))",
        "    yes.then_some(Avx2(())).filter(|_| false)",
        [DETECTED],
    ),
    (
        "the AVX2 table index rounds rather than truncates",
        "    let i = _mm256_max_epi32(_mm256_cvttps_epi32(capped), _mm256_setzero_si256());",
        f"    let i = _mm256_max_epi32({X86}_mm256_cvtps_epi32(capped), _mm256_setzero_si256());",
        [PASSES],
    ),
    (
        "the AVX2 interpolation's fraction is from the capped index",
        "    let frac = _mm256_sub_ps(at, _mm256_cvtepi32_ps(i));",
        "    let frac = _mm256_sub_ps(capped, _mm256_cvtepi32_ps(i));",
        [PASSES],
    ),
    (
        "the AVX2 interpolation weighs its two points",
        "\n    _mm256_add_ps(a, _mm256_mul_ps(frac, _mm256_sub_ps(b, a)))\n}",
        "\n    _mm256_add_ps(\n"
        "        _mm256_mul_ps(a, _mm256_sub_ps(_mm256_set1_ps(1.0), frac)),\n"
        "        _mm256_mul_ps(b, frac),\n"
        "    )\n}",
        [PASSES],
    ),
    (
        "the AVX2 table passes leave a row's remainder",
        "    for x in chunks.into_remainder() {\n        *x = interpolate(table, *x);\n    }",
        "    let _ = chunks.into_remainder();",
        [PASSES, ROWS],
    ),
    (
        "the AVX2 HLG square is multiplied by a twelfth",
        "_mm256_div_ps(_mm256_mul_ps(twice, twice), _mm256_set1_ps(12.0));",
        "_mm256_mul_ps(_mm256_mul_ps(twice, twice), _mm256_set1_ps(1.0 / 12.0));",
        [PASSES],
    ),
    (
        "the AVX2 HLG blend is the wrong way round",
        "_mm256_blendv_ps(high, low, is_low)",
        "_mm256_blendv_ps(low, high, is_low)",
        [PASSES, ROWS],
    ),
    (
        "the AVX2 power takes infinity and NaN for normal numbers",
        "        _mm256_cmpgt_epi32(_mm256_set1_epi32(255), exponent),",
        "        _mm256_cmpgt_epi32(_mm256_set1_epi32(256), exponent),",
        [PASSES],
    ),
    (
        "the AVX2 power's fraction takes a bit of its index",
        "_mm256_and_si256(mantissa, _mm256_set1_epi32(0x1fff))",
        "_mm256_and_si256(mantissa, _mm256_set1_epi32(0x3fff))",
        [PASSES],
    ),
    (
        "the AVX2 power passes NaN as infinity",
        "_mm256_cmp_ps::<_CMP_GE_OQ>(y, _mm256_set1_ps(f32::INFINITY))",
        f"_mm256_cmp_ps::<{{ {X86}_CMP_NLT_UQ }}>(y, _mm256_set1_ps(f32::INFINITY))",
        [PASSES],
    ),
    (
        "the AVX2 luminance adds green and blue first",
        "                _mm256_add_ps(_mm256_mul_ps(w_r, r), _mm256_mul_ps(w_g, g)),\n"
        "                _mm256_mul_ps(w_b, b),",
        "                _mm256_mul_ps(w_r, r),\n"
        "                _mm256_add_ps(_mm256_mul_ps(w_g, g), _mm256_mul_ps(w_b, b)),",
        [PASSES],
    ),
    (
        "the AVX2 gain's middle lanes are taken in the wrong order",
        "                    if between & (1 << lane) != 0 {",
        "                    if between & (0x80 >> lane) != 0 {",
        [PASSES],
    ),
    (
        "the AVX2 gain past the last point multiplies by a reciprocal",
        "_mm256_blendv_ps(_mm256_div_ps(top, x), low, at_low)",
        "_mm256_blendv_ps(_mm256_mul_ps(top, _mm256_div_ps(_mm256_set1_ps(1.0), x)), low, at_low)",
        [PASSES],
    ),
    (
        "the AVX2 encoder makes NaN white",
        "_mm256_min_ps(_mm256_max_ps(l, _mm256_setzero_ps()), _mm256_set1_ps(1.0))",
        "_mm256_min_ps(_mm256_max_ps(_mm256_setzero_ps(), l), _mm256_set1_ps(1.0))",
        [PASSES, ROWS],
    ),
    (
        "the AVX2 encoder gives a code's least light the code before",
        "_mm256_cmp_ps::<_CMP_GE_OQ>(l, bound)",
        f"_mm256_cmp_ps::<{{ {X86}_CMP_GT_OQ }}>(l, bound)",
        [PASSES],
    ),
    (
        "the AVX2 pixel's red and blue are swapped",
        "_mm256_slli_epi32::<16>(red), _mm256_slli_epi32::<8>(green)),\n                blue,",
        "_mm256_slli_epi32::<16>(blue), _mm256_slli_epi32::<8>(green)),\n                red,",
        [PASSES, ROWS],
    ),
    (
        "the AVX2 encoder leaves a row's remainder",
        "        .take(n)\n        .skip(whole);",
        "        .take(n)\n        .skip(n);",
        [PASSES, ROWS],
    ),
]

if __name__ == "__main__":
    # A filter goes to the tables it names a row of, and only those: the
    # harness refuses a filter that selects nothing.
    only = sys.argv[1:]
    tables = [
        (SRC / "managed.rs", HDR),
        (SRC / "reformat.rs", CHROMA),
        (SRC / "managed" / "avx2.rs", AVX),
    ]
    names = [name for _, rows in tables for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    results = [0]
    for src, rows in tables:
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        results.append(sweep(src, rows, "yuv", timeout=600, only=mine or None, targets=TARGETS))
    raise SystemExit(max(results))
