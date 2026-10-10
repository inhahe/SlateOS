//! HDR pictures shown on an ordinary screen as Chrome shows them
//! (design-decisions §1378): PQ (HDR10's, SMPTE ST 2084) and HLG (ARIB
//! STD-B67) pictures turned into sRGB pixels by Chrome's own handling,
//! transcribed from Skia and held to Chrome's pixels.
//!
//! [`crate::reformat`] -- libavif's conversion -- takes a picture's samples
//! to be ordinary video's. An HDR picture's are not: a PQ sample is absolute
//! light, up to 10 000 cd/m2; an HLG sample is a scene's light, for a display
//! of about 1000 cd/m2; and both are in BT.2020's wider colours. Taken as
//! ordinary samples they show dim, grey and oversaturated.
//!
//! **What Chrome does** for a screen with no HDR headroom (an sRGB one) --
//! Skia's colour conversion (`src/core/SkColorSpaceXformSteps.cpp`) and the
//! tone map it adds to a PQ or HLG picture (`src/codec/SkHdrAgtm.cpp`, as
//! `gfx::ColorSpace::ToSkColorSpace` and `skhdr::Agtm` set it up), for each
//! pixel:
//!
//! 1. Y'CbCr to R'G'B' in floating point, by the picture's matrix and range
//!    as libavif's floating-point path computes it, subsampled chroma
//!    brought up as Chrome's GPU samples it -- bilinear, centred, which is
//!    libavif's slow path's 9:3:3:1 -- and clamped to [0, 1].
//! 2. Its light. PQ: skcms's `PQish` curve, light over 10 000 cd/m2. HLG: its
//!    `HLGish` curve over 12 (BT.2100's inverse OETF: the scene's light), then
//!    BT.2100's OOTF for a 1000 cd/m2 display, `Y^(gamma - 1)` with gamma 1.2
//!    and `Y` in BT.2100's luminance weights.
//! 3. Into the tone map's working space, linear BT.2020 with 1.0 at the HDR
//!    reference white of 203 cd/m2: the picture's primaries to BT.2020's, and
//!    times 10000/203 (PQ) or 1000/203 (HLG).
//! 4. The tone map: RWTMO, the "reference white tone mapping operator" of
//!    the adaptive global tone map (SMPTE ST 2094-50) that Skia applies when a
//!    picture brings no tone map of its own. The content's headroom is
//!    `log2(peak / 203)` ([`Light::peak`]); for a screen of no headroom the
//!    gain is "alternate image 0"'s: eight control points along a quadratic
//!    Bezier in log space, joined by cubic Hermite splines, evaluated on
//!    max(R, G, B), whose gain multiplies all three. The reference white
//!    lands at `1 - 0.5 * min(headroom / log2(1000/203), 1)` of the screen's
//!    white, and the content's peak at its white.
//! 5. To the screen: BT.2020's primaries to sRGB's, sRGB's curve, clamped,
//!    8 bits.
//!
//! **Its arithmetic** is Skia's: single precision, Skia's matrices (its
//! named gamuts, and `skcms_PrimariesToXYZD50` for the rest), the tone map's
//! control points computed as Skia computes them and then rounded to half
//! precision, as its shader reads them from an F16 texture. Three steps are
//! tables rather than Skia's per-pixel `pow`: the transfer curves of step 2
//! and HLG's OOTF power (a [`Signal`], made once per transfer), sampled
//! finely enough that their interpolation errs by under a ten-thousandth of
//! an 8-bit code; and step 5's rounding, which compares with the light at
//! which each code begins, and so rounds exactly. Against a double-precision
//! transcription of the same steps the codes agree in all but under one
//! channel in ten thousand, and there by one; against Chrome 154's own
//! pixels (the tests' patches) they agree exactly.
//!
//! **Its speed.** Each step is a pass along a row of floats, the arithmetic
//! ones run several pixels at a time; what remains is the tables' lookups,
//! six a pixel. A 1080p frame takes some 40-60 ms on one thread of an
//! i7-8700K (`tests/bench.rs`) -- about ten times the ordinary conversion,
//! which `gui/video/codec` divides among the cores as it does that one.
//!
//! Portions of this file are transcribed from Skia and its skcms (copyright
//! Google), used under Skia's BSD licence, whose text travels as
//! `gui/imagecodec/licenses/skia-LICENSE` (that crate's manifest names Skia,
//! once for the tree, as `scripts/gather-notices.py` requires).

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use crate::Sample;
use crate::reformat::{self, Error, Format, Mode, Picture, Reformat};

/// The HDR reference white, cd/m2: what 1.0 is in the working space, and
/// what the content's headroom is measured from -- Chrome's
/// `gfx::ColorSpace::kDefaultSDRWhiteLevel`, which is also skhdr's
/// `kDefaultHdrReferenceWhite` and BT.2408's reference white.
const REFERENCE_WHITE: f32 = 203.0;

/// The peak HLG is shown for, cd/m2: Chrome's `kDefaultPeakWhite`.
const HLG_PEAK: f32 = 1000.0;

/// HLG's system gamma: Chrome's `kDefaultSystemGamma`.
const HLG_GAMMA: f32 = 1.2;

/// The most headroom the tone map takes, in stops: skhdr's
/// `kMaxHdrHeadroom`.
const MAX_HEADROOM: f32 = 6.0;

/// The content peak taken when a picture says nothing of its light, cd/m2:
/// `get_peak_luminance`'s default.
const DEFAULT_PEAK: f32 = 1000.0;

/// How many segments a transfer's table divides its input into: PQ's the
/// whole of [0, 1], HLG's the upper half (the lower half is a parabola,
/// computed). At 16384 the interpolation errs by under a ten-thousandth of
/// an 8-bit code wherever the light falls. One size for both, so that the
/// table is an array whose bounds the compiler can see.
const SEGMENTS: usize = 16384;

/// A transfer's table: [`SEGMENTS`] + 1 points.
type Table = [f32; SEGMENTS + 1];

/// How many cells the encoder divides linear light into: more than the
/// codes' steepest density (12.92 * 255, at black), so a cell holds at most
/// one code's beginning.
const CELLS: usize = 4096;

/// How the samples of an HDR picture are light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer {
    /// PQ, SMPTE ST 2084 (H.273 transfer 16): absolute light.
    Pq,
    /// HLG, ARIB STD-B67 (H.273 transfer 18): a scene's light.
    Hlg,
}

impl Transfer {
    /// The HDR transfer an H.273 `TransferCharacteristics` names: `None`
    /// for an ordinary one, which [`crate::reformat`] converts.
    #[must_use]
    pub const fn from_h273(transfer: u16) -> Option<Self> {
        match transfer {
            16 => Some(Self::Pq),
            18 => Some(Self::Hlg),
            _ => None,
        }
    }
}

/// What a picture says of its light, as Chrome's tone map reads it: each
/// `0.0` where the picture does not say.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Light {
    /// Its content light level's MaxCLL, the brightest pixel, in cd/m2.
    pub max_cll: f32,
    /// Its mastering display's peak luminance, in cd/m2.
    pub mastering_peak: f32,
}

impl Light {
    /// The content's peak, as `get_peak_luminance` (skhdr) and
    /// `HDRMetadata::GetContentMaxLuminance` (Chrome) take it: MaxCLL if it is
    /// said, else the mastering display's peak, else 1000 cd/m2.
    #[must_use]
    pub fn peak(self) -> f32 {
        if self.max_cll > 0.0 {
            self.max_cll
        } else if self.mastering_peak > 0.0 {
            self.mastering_peak
        } else {
            DEFAULT_PEAK
        }
    }

    /// The content's headroom over the reference white, in stops, as
    /// `PopulateToneMapAgtmParams` computes it: 0 for content no brighter
    /// than the reference white, which is then not tone mapped.
    #[must_use]
    pub fn headroom(self) -> f32 {
        let peak = self.peak();
        if peak > REFERENCE_WHITE {
            libm::log2f(peak / REFERENCE_WHITE).min(MAX_HEADROOM)
        } else {
            0.0
        }
    }
}

// --- step 2: the transfer curves, tabulated -----------------------------------

/// What every [`ToneMap`] of one transfer shares, made once: the transfer's
/// curve from a sample to its light, and the 8-bit sRGB encoder.
#[derive(Clone, Debug)]
pub struct Signal {
    transfer: Transfer,
    /// PQ: the light over 10 000 cd/m2 at each of [`SEGMENTS`] + 1 points
    /// from 0 to 1. HLG: the scene's light at as many points from 0.5 to 1.
    /// On the heap (64 KiB), and seen as a [`Table`] for each row.
    table: Box<[f32]>,
    /// HLG's OOTF power; `None` for PQ.
    power: Option<Power>,
    encoder: Encoder,
}

impl Signal {
    /// The tables for `transfer`: tens of thousands of `pow` and `exp`, a
    /// millisecond or two -- made once, and shared by every picture of the
    /// transfer.
    #[must_use]
    pub fn new(transfer: Transfer) -> Self {
        let (table, power) = match transfer {
            Transfer::Pq => (sample(pqish), None),
            Transfer::Hlg => (
                sample(|i| hlgish(0.5 + 0.5 * i)),
                Some(Power::new(f64::from(HLG_GAMMA - 1.0))),
            ),
        };
        Self {
            transfer,
            table,
            power,
            encoder: Encoder::new(),
        }
    }

    /// The transfer the tables are for.
    #[must_use]
    pub const fn transfer(&self) -> Transfer {
        self.transfer
    }

    /// The table, as an array whose bounds the compiler sees: `None` only
    /// were it not the table's length, which [`sample`] makes it.
    fn curve(&self) -> Option<&Table> {
        <&Table>::try_from(&*self.table).ok()
    }

    /// PQ's light for a sample `c` in [0, 1], over 10 000 cd/m2.
    #[cfg(test)]
    fn pq(&self, c: f32) -> f32 {
        self.curve().map_or(0.0, |t| interpolate(t, c))
    }

    /// HLG's scene light for a sample `c` in [0, 1].
    #[cfg(test)]
    fn hlg(&self, c: f32) -> f32 {
        self.curve().map_or(0.0, |t| hlg(t, c))
    }
}

/// HLG's scene light for a sample `c` in [0, 1] from its `table`: `c^2 / 3`
/// to a half, as skcms's `HLGish` computes it there (`(2c)^2 / 12`), and
/// the table above.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "floating-point arithmetic, which cannot overflow into undefined behaviour"
)]
#[inline]
fn hlg(table: &Table, c: f32) -> f32 {
    if c <= 0.5 {
        let twice = 2.0 * c;
        twice * twice / 12.0
    } else {
        interpolate(table, (c - 0.5) * 2.0)
    }
}

/// How many segments [`Power`] divides a mantissa's range into.
const POWER_SEGMENTS: u32 = 1024;

/// `y^p` for `y` from 0 up, by `y`'s exponent and mantissa: `2^(p e)` for
/// each exponent `e`, times `(1 + f)^p` for its mantissa `f`, interpolated
/// from 1024 segments -- which errs by under 2e-8 -- where `powf` per pixel
/// cost more than the rest of a pixel's conversion together.
#[derive(Clone, Debug)]
struct Power {
    /// `2^(p (e - 127))` for each biased exponent `e` of a normal single.
    exponents: Box<[f32]>,
    /// `(1 + i / 1024)^p` for `i` from 0 to 1024.
    mantissas: Box<[f32]>,
}

impl Power {
    fn new(p: f64) -> Self {
        let exponents = (0..=255u8)
            .map(|e| narrow(libm::exp2(p * (f64::from(e) - 127.0))))
            .collect();
        let mantissas = (0..=POWER_SEGMENTS)
            .map(|i| narrow(libm::pow(1.0 + f64::from(i) / f64::from(POWER_SEGMENTS), p)))
            .collect();
        Self {
            exponents,
            mantissas,
        }
    }

    /// `y^p`: 0 for zero, for a subnormal `y` (whose power, under 1e-7,
    /// multiplies light under 1e-38) and for a negative or NaN one (whose
    /// power is NaN, which shows black).
    #[allow(
        clippy::cast_precision_loss,
        clippy::arithmetic_side_effects,
        reason = "the mantissa's low bits are under 2^13, exact in f32; float arithmetic cannot overflow into undefined behaviour"
    )]
    #[inline]
    fn of(&self, y: f32) -> f32 {
        let bits = y.to_bits();
        let exponent = (bits >> 23) as usize;
        if !(1..=254).contains(&exponent) {
            // Zero, subnormal, negative (the sign bit makes it 256 or
            // more), infinite or NaN.
            return if y == f32::INFINITY { y } else { 0.0 };
        }
        let mantissa = bits & 0x7f_ffff;
        let i = (mantissa >> 13) as usize;
        let frac = (mantissa & 0x1fff) as f32 / 8192.0;
        let scale = self.exponents.get(exponent).copied().unwrap_or(0.0);
        match self.mantissas.get(i..=i + 1) {
            Some(&[a, b]) => scale * (a + frac * (b - a)),
            _ => 0.0,
        }
    }
}

/// `f` at [`SEGMENTS`] + 1 points from 0 to 1, in double precision, kept in
/// single.
fn sample(f: impl Fn(f64) -> f64) -> Box<[f32]> {
    let n = u32::try_from(SEGMENTS).unwrap_or(u32::MAX);
    (0..=n)
        .map(|i| narrow(f(f64::from(i) / f64::from(n))))
        .collect()
}

/// `table` at `x`, which its callers have in [0, 1], interpolated linearly.
/// Outside [0, 1] the index is still within the table -- the cast saturates,
/// NaN to 0 -- and the value an extrapolation.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "the index is clamped to [0, SEGMENTS - 1], so it and the next are within the table, which the compiler sees too; SEGMENTS is a power of two under 2^24, exact in f32"
)]
#[inline]
fn interpolate(table: &Table, x: f32) -> f32 {
    // To `u32`, whose saturation takes a negative or NaN index to 0 by
    // itself, so that only the top needs clamping: no branch, which an
    // index often 0 -- dark light -- would mispredict. (An unsigned 64-bit
    // conversion takes a dozen instructions on x86.)
    let at = x * SEGMENTS as f32;
    let i = (at as u32).min(SEGMENTS as u32 - 1);
    let frac = at - i as f32;
    let i = i as usize;
    let (a, b) = (table[i], table[i + 1]);
    a + frac * (b - a)
}

// skcms's PQish curve (`SkColorSpaceXformSteps`'s `kPQish`): A, B, C, D, E
// and F of `((max(A + B x^C, 0)) / (D + E x^C))^F`.
const PQ_A: f64 = -107.0 / 128.0;
const PQ_B: f64 = 1.0;
const PQ_C: f64 = 32.0 / 2523.0;
const PQ_D: f64 = 2413.0 / 128.0;
const PQ_E: f64 = -2392.0 / 128.0;
const PQ_F: f64 = 8192.0 / 1305.0;

/// skcms's `PQish`: a PQ sample to its light over 10 000 cd/m2.
fn pqish(x: f64) -> f64 {
    let xc = if x > 0.0 { libm::pow(x, PQ_C) } else { 0.0 };
    let numerator = (PQ_A + PQ_B * xc).max(0.0);
    libm::pow(numerator / (PQ_D + PQ_E * xc), PQ_F)
}

// BT.2100's HLG constants, as skcms's `kHLGish` carries them.
const HLG_A: f64 = 0.178_832_77;
const HLG_B: f64 = 0.284_668_92;
const HLG_C: f64 = 0.559_910_73;

/// skcms's `HLGish` over 12: an HLG sample to the scene's light, 0 to 1.
fn hlgish(x: f64) -> f64 {
    if x <= 0.5 {
        x * x / 3.0
    } else {
        (libm::exp((x - HLG_C) / HLG_A) + HLG_B) / 12.0
    }
}

/// `x` in single precision.
#[allow(
    clippy::cast_possible_truncation,
    reason = "values kept in single precision, as Skia keeps them"
)]
fn narrow(x: f64) -> f32 {
    x as f32
}

// --- step 5: sRGB, to 8 bits exactly ---------------------------------------------

/// sRGB's curve to 8 bits, exactly: for each cell of light, the code it
/// begins in and where the next code begins -- together, so that a code is
/// one load rather than two, the second waiting on the first.
#[derive(Clone, Debug)]
struct Encoder {
    /// [`CELLS`] of them: on the heap (32 KiB), seen as an array for each
    /// row.
    cells: Box<[Cell]>,
}

/// A cell of the [`Encoder`]'s light, from `i / CELLS` up to the next's.
#[derive(Clone, Copy, Debug)]
struct Cell {
    /// The least light of the code after `code`.
    bound: f32,
    /// The code at the cell's least light.
    code: u32,
}

/// The cells of an [`Encoder`], as an array whose bounds the compiler sees.
type Cells = [Cell; CELLS];

/// `bounds()[k]`: the least single-precision light that is code `k + 1` or
/// more -- where `encode(l) * 255 + 0.5` reaches `k + 1`. The last is
/// infinite.
fn bounds() -> [f32; 256] {
    let mut bounds = [f32::INFINITY; 256];
    for (k, bound) in (0u32..255).zip(bounds.iter_mut()) {
        *bound = at_least(srgb_decode((f64::from(k) + 0.5) / 255.0));
    }
    bounds
}

impl Encoder {
    #[allow(
        clippy::cast_precision_loss,
        reason = "cell indices are under 2^12, exact in f32"
    )]
    fn new() -> Self {
        let bounds = bounds();
        let cells = (0..CELLS)
            .map(|i| {
                let start = i as f32 / CELLS as f32;
                let code = bounds.partition_point(|&b| b <= start);
                Cell {
                    bound: bounds.get(code).copied().unwrap_or(f32::INFINITY),
                    code: u32::try_from(code).unwrap_or(255),
                }
            })
            .collect();
        Self { cells }
    }

    /// The cells, as an array: `None` only were they not [`CELLS`] long,
    /// which [`Encoder::new`] makes them.
    fn cells(&self) -> Option<&Cells> {
        <&Cells>::try_from(&*self.cells).ok()
    }

    /// [`code`] by these cells.
    #[cfg(test)]
    fn code(&self, l: f32) -> u32 {
        self.cells().map_or(0, |c| code(c, l))
    }
}

/// The 8-bit code of linear light `l` by `cells`: `(int)(encode(l) * 255 +
/// 0.5)`, `l` clamped to [0, 1] and NaN taken as 0.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "the cell is clamped to the array, whose bounds the compiler sees; a code and the one-step adjustment stay within 0..=255"
)]
#[inline]
fn code(cells: &Cells, l: f32) -> u32 {
    // A NaN stays NaN through the clamp, is cell 0 through the cast, and is
    // no code's least light: code 0. (The cast is to `u32`, as in
    // `interpolate`.)
    let l = l.clamp(0.0, 1.0);
    let cell = ((l * CELLS as f32) as u32).min(CELLS as u32 - 1) as usize;
    let Cell { bound, code } = cells[cell];
    code + u32::from(l >= bound)
}

/// sRGB's EOTF: a signal, 0 to 1, to its linear light.
fn srgb_decode(v: f64) -> f64 {
    if v <= 0.040_45 {
        v / 12.92
    } else {
        libm::pow((v + 0.055) / 1.055, 2.4)
    }
}

/// The least single-precision value at or above `x`.
fn at_least(x: f64) -> f32 {
    let f = narrow(x);
    if f64::from(f) < x {
        f32::from_bits(f.to_bits().wrapping_add(1))
    } else {
        f
    }
}

// --- steps 3 and 5: Skia's matrices ------------------------------------------------

/// A 3x3 matrix, rows of columns: skcms's `skcms_Matrix3x3`.
type Matrix = [[f32; 3]; 3];

/// skcms's `skcms_Matrix3x3_invert`: in double precision, kept in single;
/// `None` for a singular matrix. Its odd naming (`a01` is row 1, column 0)
/// is the C's, kept so the transcription can be checked line by line.
#[allow(
    clippy::similar_names,
    clippy::many_single_char_names,
    reason = "skcms's names, kept to be checked against it"
)]
fn invert(src: &Matrix) -> Option<Matrix> {
    let v = |r: usize, c: usize| {
        f64::from(
            src.get(r)
                .and_then(|row| row.get(c))
                .copied()
                .unwrap_or(0.0),
        )
    };
    let (a00, a01, a02) = (v(0, 0), v(1, 0), v(2, 0));
    let (a10, a11, a12) = (v(0, 1), v(1, 1), v(2, 1));
    let (a20, a21, a22) = (v(0, 2), v(1, 2), v(2, 2));
    let mut b0 = a00 * a11 - a01 * a10;
    let mut b1 = a00 * a12 - a02 * a10;
    let mut b2 = a01 * a12 - a02 * a11;
    let (mut b3, mut b4, mut b5) = (a20, a21, a22);
    let determinant = b0 * b5 - b1 * b4 + b2 * b3;
    if determinant == 0.0 {
        return None;
    }
    let invdet = 1.0 / determinant;
    let max = f64::from(f32::MAX);
    if !(-max..=max).contains(&invdet) || !narrow(invdet).is_finite() {
        return None;
    }
    b0 *= invdet;
    b1 *= invdet;
    b2 *= invdet;
    b3 *= invdet;
    b4 *= invdet;
    b5 *= invdet;
    let out = [
        [
            narrow(a11 * b5 - a12 * b4),
            narrow(a12 * b3 - a10 * b5),
            narrow(a10 * b4 - a11 * b3),
        ],
        [
            narrow(a02 * b4 - a01 * b5),
            narrow(a00 * b5 - a02 * b3),
            narrow(a01 * b3 - a00 * b4),
        ],
        [narrow(b2), narrow(-b1), narrow(b0)],
    ];
    out.iter().flatten().all(|x| x.is_finite()).then_some(out)
}

/// skcms's `skcms_Matrix3x3_concat`: `a` after `b`, in single precision.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "floating-point arithmetic, which cannot overflow into undefined behaviour"
)]
fn concat(a: &Matrix, b: &Matrix) -> Matrix {
    let [b0, b1, b2] = b;
    a.map(|[x, y, z]| [0, 1, 2].map(|c| at(b0, c) * x + at(b1, c) * y + at(b2, c) * z))
}

/// `row[c]`, 0 past its end.
fn at(row: &[f32; 3], c: usize) -> f32 {
    row.get(c).copied().unwrap_or(0.0)
}

/// skcms's `mv_mul`: `m` times the column `v`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "floating-point arithmetic, which cannot overflow into undefined behaviour"
)]
#[inline]
fn apply(m: &Matrix, [x, y, z]: [f32; 3]) -> [f32; 3] {
    m.map(|[a, b, c]| a * x + b * y + c * z)
}

/// The diagonal matrix of `v`.
const fn diagonal([x, y, z]: [f32; 3]) -> Matrix {
    [[x, 0.0, 0.0], [0.0, y, 0.0], [0.0, 0.0, z]]
}

/// skcms's `is_zero_to_one`.
fn unit(x: f32) -> bool {
    (0.0..=1.0).contains(&x)
}

/// skcms's `skcms_AdaptToXYZD50`: the Bradford adaptation from white
/// (`wx`, `wy`) to D50.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "floating-point arithmetic, which cannot overflow into undefined behaviour"
)]
fn adapt_to_d50(wx: f32, wy: f32) -> Option<Matrix> {
    if !unit(wx) || !unit(wy) {
        return None;
    }
    let white = [wx / wy, 1.0, (1.0 - wx - wy) / wy];
    let d50 = [0.964_22, 1.0, 0.825_21];
    let xyz_to_lms = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let lms_to_xyz = [
        [0.986_992_9, -0.147_054_3, 0.159_962_7],
        [0.432_305_3, 0.518_360_3, 0.049_291_2],
        [-0.008_528_7, 0.040_042_8, 0.968_486_7],
    ];
    let [s0, s1, s2] = apply(&xyz_to_lms, white);
    let [d0, d1, d2] = apply(&xyz_to_lms, d50);
    let scale = diagonal([d0 / s0, d1 / s1, d2 / s2]);
    Some(concat(&lms_to_xyz, &concat(&scale, &xyz_to_lms)))
}

/// skcms's `skcms_PrimariesToXYZD50`: a matrix to XYZ D50 from
/// chromaticities `[rx, ry, gx, gy, bx, by, wx, wy]`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "floating-point arithmetic, which cannot overflow into undefined behaviour"
)]
fn primaries_to_xyz_d50(p: [f32; 8]) -> Option<Matrix> {
    if !p.iter().all(|&v| unit(v)) {
        return None;
    }
    let [rx, ry, gx, gy, bx, by, wx, wy] = p;
    let primaries = [
        [rx, gx, bx],
        [ry, gy, by],
        [1.0 - rx - ry, 1.0 - gx - gy, 1.0 - bx - by],
    ];
    let inverse = invert(&primaries)?;
    let white = [wx / wy, 1.0, (1.0 - wx - wy) / wy];
    let to_xyz = concat(&primaries, &diagonal(apply(&inverse, white)));
    Some(concat(&adapt_to_d50(wx, wy)?, &to_xyz))
}

/// `SkFixedToFloat` of a 16.16 number: exact, the scale being a power of
/// two.
#[allow(
    clippy::cast_precision_loss,
    reason = "the values are under 2^24, exact in f32"
)]
const fn fixed(v: u32) -> f32 {
    v as f32 / 65536.0
}

/// `SkNamedGamut::kSRGB`, in the 16.16 numbers of skcms.
const SRGB: Matrix = [
    [fixed(0x6FA2), fixed(0x6299), fixed(0x24A0)],
    [fixed(0x38F5), fixed(0xB785), fixed(0x0F84)],
    [fixed(0x0390), fixed(0x18DA), fixed(0xB6CF)],
];

/// `SkNamedGamut::kDisplayP3`.
const DISPLAY_P3: Matrix = [
    [0.515_102, 0.291_965, 0.157_153],
    [0.241_182, 0.692_236, 0.066_581_9],
    [-0.001_049_41, 0.041_881_8, 0.784_378],
];

/// `SkNamedGamut::kRec2020`.
const REC2020: Matrix = [
    [0.673_459, 0.165_661, 0.125_100],
    [0.279_033, 0.675_338, 0.045_628_8],
    [-0.001_931_39, 0.029_979_4, 0.797_162],
];

/// `SkNamedPrimaries::kRec2020`, the tone map's working space
/// (`fGainApplicationSpacePrimaries`), whose matrix Skia computes from them.
const REC2020_PRIMARIES: [f32; 8] = [0.708, 0.292, 0.170, 0.797, 0.131, 0.046, 0.3127, 0.3290];

/// The matrix to XYZ D50 Chrome gives H.273's `primaries`
/// (`gfx::ColorSpace::ToSkColorSpace`): Skia's named gamuts for BT.709,
/// Display P3 and BT.2020, and for the rest their chromaticities
/// (`SkNamedPrimaries`) through `skcms_PrimariesToXYZD50`. A code Chrome has
/// no primaries for (`PrimaryID::INVALID`) is guessed BT.2020, as
/// `VideoColorSpace::GuessGfxColorSpace` guesses it for an HDR transfer.
fn to_xyz_d50(primaries: u16) -> Matrix {
    let named: [f32; 8] = match primaries {
        1 => return SRGB,
        12 => return DISPLAY_P3,
        4 => [0.67, 0.33, 0.21, 0.71, 0.14, 0.08, 0.31, 0.316],
        5 => [0.64, 0.33, 0.29, 0.60, 0.15, 0.06, 0.3127, 0.3290],
        6 | 7 => [0.630, 0.340, 0.310, 0.595, 0.155, 0.070, 0.3127, 0.3290],
        8 => [0.681, 0.319, 0.243, 0.692, 0.145, 0.049, 0.310, 0.316],
        10 => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0 / 3.0, 1.0 / 3.0],
        11 => [0.680, 0.320, 0.265, 0.690, 0.150, 0.060, 0.314, 0.351],
        22 => [0.630, 0.340, 0.295, 0.605, 0.155, 0.077, 0.3127, 0.3290],
        _ => return REC2020,
    };
    primaries_to_xyz_d50(named).unwrap_or(REC2020)
}

/// skcms's `gamutTransformTo`: linear light of the gamut `from` (to XYZ D50)
/// to the gamut `to`.
fn gamut_transform(from: &Matrix, to: &Matrix) -> Matrix {
    let identity = diagonal([1.0; 3]);
    invert(to).map_or(identity, |inverse| concat(&inverse, from))
}

// --- step 4: the tone map ------------------------------------------------------------

/// RWTMO's gain curve for alternate image 0: its eight control points, each
/// its `x` (the light), `y` (the log2 of the gain there) and `m` (the slope
/// of `y`), rounded to half precision as Skia's shader reads them.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Curve {
    points: [[f32; 3]; 8],
    /// The gain at and below the first point's light -- the reference white
    /// -- which is most of a picture: `exp2` of its `y`, made once.
    low: f32,
    /// Each segment's cubic, as `EvaluateGainCurve` computes it from the
    /// segment's two points -- made once rather than for every pixel, by the
    /// same single-precision operations, so to the same bits:
    /// `[x_i, h, c0, c1, c2, c3]`.
    segments: [[f32; 6]; 7],
    /// The gain at and past the last point's light, over the light: `exp2` of
    /// its `y`, times its `x`. (Skia computes `exp2(y + log2(x_last / x))`,
    /// the same to within the rounding of the logarithm.)
    top: f32,
}

impl Curve {
    /// `PopulateUsingRwtmo` for alternate image 0, in single precision as
    /// Skia computes it: `None` for no headroom, which is no tone map.
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::cast_precision_loss,
        reason = "floating-point arithmetic, which cannot overflow into undefined behaviour; the point indices are under 8, exact in f32"
    )]
    fn rwtmo(headroom: f32) -> Option<Self> {
        if headroom == 0.0 {
            return None;
        }
        let ratio = (headroom / libm::log2f(1000.0 / 203.0)).min(1.0);
        let y_white = 1.0 - 0.5 * ratio;
        let kappa = 0.65f32;
        let (x_knee, y_knee) = (1.0f32, y_white);
        let x_max = libm::exp2f(headroom);
        // Alternate image 0's headroom is 0.
        let y_max = libm::exp2f(0.0);
        let x_mid = (1.0 - kappa) * x_knee + kappa * (x_knee * y_max / y_knee);
        let y_mid = (1.0 - kappa) * y_knee + kappa * y_max;
        let x_a = x_knee - 2.0 * x_mid + x_max;
        let y_a = y_knee - 2.0 * y_mid + y_max;
        let x_b = 2.0 * x_mid - 2.0 * x_knee;
        let y_b = 2.0 * y_mid - 2.0 * y_knee;
        let (x_c, y_c) = (x_knee, y_knee);
        let ln2 = libm::logf(2.0);
        let mut points = [[0.0f32; 3]; 8];
        for (c, point) in (0u8..).zip(points.iter_mut()) {
            let t = f32::from(c) / (8.0 - 1.0);
            let x = x_c + t * (x_b + t * x_a);
            let y = y_c + t * (y_b + t * y_a);
            let m = (2.0 * y_a * t + y_b) / (2.0 * x_a * t + x_b);
            *point = [x, libm::log2f(y / x), (x * m - y) / (ln2 * x * y)].map(half);
        }
        let mut segments = [[0.0f32; 6]; 7];
        for (segment, pair) in segments.iter_mut().zip(points.windows(2)) {
            let &[[x_i, y_i, m_i], [x_j, y_j, m_j]] = pair else {
                continue;
            };
            let h = x_j - x_i;
            let (m_hat_i, m_hat_j) = (m_i * h, m_j * h);
            let c3 = 2.0 * y_i + m_hat_i - 2.0 * y_j + m_hat_j;
            let c2 = -3.0 * y_i + 3.0 * y_j - 2.0 * m_hat_i - m_hat_j;
            *segment = [x_i, h, y_i, m_hat_i, c2, c3];
        }
        let [x_last, y_last, _] = points[7];
        Some(Self {
            points,
            low: libm::exp2f(points[0][1]),
            segments,
            top: narrow(libm::exp2(f64::from(y_last)) * f64::from(x_last)),
        })
    }

    /// The gain at `x`, the brightest channel's light: `exp2` of
    /// `EvaluateGainCurve`, in single precision. The segment is found by
    /// counting the points at or below `x` rather than by Skia's binary
    /// search, which lands on the same one without its branches -- each a
    /// coin toss on a varied picture.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "floating-point arithmetic; the count of points is under 8"
    )]
    #[inline]
    fn gain(&self, x: f32) -> f32 {
        let p = &self.points;
        if x <= p[0][0] {
            return self.low;
        }
        if x >= p[7][0] {
            return self.top / x;
        }
        let i = p
            .iter()
            .skip(1)
            .take(6)
            .map(|q| usize::from(x >= q[0]))
            .sum::<usize>();
        let Some(&[x_i, h, c0, c1, c2, c3]) = self.segments.get(i) else {
            return self.low;
        };
        if h == 0.0 {
            return libm::exp2f(c0);
        }
        let t = (x - x_i) / h;
        libm::exp2f(((c3 * t + c2) * t + c1) * t + c0)
    }
}

/// `v` rounded to half precision and back, as Skia's F32 to F16 conversion
/// rounds (`_mm256_cvtps_ph`: to nearest, ties to even, keeping subnormals):
/// what the tone map's shader reads its control points as.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "the bit arithmetic is on a sign-cleared f32, which cannot overflow u32 by adding under 2^13; the float arithmetic cannot overflow into undefined behaviour"
)]
fn half(v: f32) -> f32 {
    /// The least normal half, 2^-14.
    const MIN_NORMAL: f32 = 6.103_515_6e-5;
    /// The quantum of the subnormal halves, 2^-24, as its reciprocal.
    const SUBNORMAL_SCALE: f32 = 16_777_216.0;
    /// Halves from here up round to infinity.
    const OVERFLOW: f32 = 65_520.0;
    /// Adding and subtracting 2^23 rounds a value under it to a whole number,
    /// ties to even.
    const TWO_TO_23: f32 = 8_388_608.0;
    if !v.is_finite() {
        return v;
    }
    let sign = v.to_bits() & 0x8000_0000;
    let magnitude = v.abs();
    let rounded = if magnitude >= OVERFLOW {
        f32::INFINITY
    } else if magnitude < MIN_NORMAL {
        let quanta = (magnitude * SUBNORMAL_SCALE + TWO_TO_23) - TWO_TO_23;
        quanta / SUBNORMAL_SCALE
    } else {
        // Thirteen of single precision's mantissa bits dropped, to nearest
        // with ties to even.
        let bits = magnitude.to_bits();
        let keep_lsb = (bits >> 13) & 1;
        f32::from_bits((bits + 0x0fff + keep_lsb) & !0x1fff)
    };
    f32::from_bits(rounded.to_bits() | sign)
}

/// What a conversion of HDR pictures of one transfer, primaries and light
/// needs -- cheap to make for each picture, its tables borrowed from the
/// transfer's [`Signal`].
#[derive(Clone, Debug)]
pub struct ToneMap<'a> {
    signal: &'a Signal,
    /// Linear light in the picture's primaries to the working space: to
    /// BT.2020's, and times 10000/203 (PQ) or 1000/203 (HLG).
    to_working: Matrix,
    /// HLG's OOTF: BT.2100's luminance weights in the picture's primaries
    /// (its power is the [`Signal`]'s).
    ootf: [f32; 3],
    /// `None` for content no brighter than the reference white.
    curve: Option<Curve>,
    /// The working space to sRGB's linear light.
    to_srgb: Matrix,
}

impl<'a> ToneMap<'a> {
    /// The map for pictures of `signal`'s transfer whose primaries are
    /// H.273's `primaries`, and which say `light` of their light.
    #[must_use]
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "floating-point arithmetic, which cannot overflow into undefined behaviour"
    )]
    pub fn new(signal: &'a Signal, primaries: u16, light: Light) -> Self {
        let source = to_xyz_d50(primaries);
        let working = primaries_to_xyz_d50(REC2020_PRIMARIES).unwrap_or(REC2020);
        // SkColorSpaceXformSteps folds the transfer's scale into the matrix.
        let scale = match signal.transfer {
            Transfer::Pq => 10_000.0 / REFERENCE_WHITE,
            Transfer::Hlg => HLG_PEAK / REFERENCE_WHITE,
        };
        let to_working = gamut_transform(&source, &working).map(|row| row.map(|v| v * scale));
        // set_ootf_Y: BT.2100's weights, which are BT.2020's, carried into
        // the picture's primaries.
        let to_rec2020 = gamut_transform(&source, &REC2020);
        let weights = [0.262_700f32, 0.678_000, 0.059_300];
        let ootf = [0, 1, 2].map(|i| {
            to_rec2020
                .iter()
                .zip(weights)
                .fold(0.0f32, |sum, (row, w)| sum + at(row, i) * w)
        });
        Self {
            signal,
            to_working,
            ootf,
            curve: Curve::rwtmo(light.headroom()),
            to_srgb: gamut_transform(&working, &SRGB),
        }
    }

    /// A row of R'G'B' (each in [0, 1], in `channels`) to its `0x00RRGGBB`
    /// pixels in `out`: steps 2 to 5, each a pass along the row -- so that
    /// each pass's arithmetic is one loop the compiler runs several pixels
    /// at a time, and its table lookups one loop of them. Every pixel's
    /// operations are those of the steps in order, so a pixel's result is
    /// the same whatever row it is in. `channels` is left holding scratch.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "floating-point arithmetic, which cannot overflow into undefined behaviour"
    )]
    fn row(&self, channels: &mut Channels, out: &mut [u32]) {
        let Channels { r, g, b, k } = channels;
        let n = out
            .len()
            .min(r.len())
            .min(g.len())
            .min(b.len())
            .min(k.len());
        let (Some(r), Some(g), Some(b), Some(k), Some(out)) = (
            r.get_mut(..n),
            g.get_mut(..n),
            b.get_mut(..n),
            k.get_mut(..n),
            out.get_mut(..n),
        ) else {
            return;
        };
        let signal = self.signal;
        let (Some(table), Some(cells)) = (signal.curve(), signal.encoder.cells()) else {
            // Tables not of their length, which their makers make them.
            out.fill(0);
            return;
        };
        // 2. The light.
        match (signal.transfer, &signal.power) {
            // A channel at a time: a chained iterator over the three would
            // test which one it is in at every step.
            (Transfer::Pq, _) => {
                for channel in [&mut *r, &mut *g, &mut *b] {
                    for c in channel.iter_mut() {
                        *c = interpolate(table, *c);
                    }
                }
            }
            (Transfer::Hlg, power) => {
                for channel in [&mut *r, &mut *g, &mut *b] {
                    for c in channel.iter_mut() {
                        *c = hlg(table, *c);
                    }
                }
                let [wr, wg, wb] = self.ootf;
                for (((&r, &g), &b), k) in r.iter().zip(g.iter()).zip(b.iter()).zip(k.iter_mut()) {
                    let y = wr * r + wg * g + wb * b;
                    *k = power.as_ref().map_or(0.0, |p| p.of(y));
                }
                scale(r, g, b, k);
            }
        }
        // 3. Into the working space, and the brightest channel there.
        let m = &self.to_working;
        for (((r, g), b), k) in r
            .iter_mut()
            .zip(g.iter_mut())
            .zip(b.iter_mut())
            .zip(k.iter_mut())
        {
            let [x, y, z] = apply(m, [*r, *g, *b]);
            (*r, *g, *b) = (x, y, z);
            *k = x.max(y).max(z);
        }
        // 4. The tone map's gain, of the brightest channel, on all three.
        if let Some(curve) = &self.curve {
            for k in k.iter_mut() {
                *k = curve.gain(*k);
            }
            scale(r, g, b, k);
        }
        // 5. To sRGB ...
        let m = &self.to_srgb;
        for ((r, g), b) in r.iter_mut().zip(g.iter_mut()).zip(b.iter_mut()) {
            let [x, y, z] = apply(m, [*r, *g, *b]);
            (*r, *g, *b) = (x, y, z);
        }
        // ... and its 8-bit codes, a channel at a time.
        for (px, &r) in out.iter_mut().zip(r.iter()) {
            *px = code(cells, r) << 16;
        }
        for (px, &g) in out.iter_mut().zip(g.iter()) {
            *px |= code(cells, g) << 8;
        }
        for (px, &b) in out.iter_mut().zip(b.iter()) {
            *px |= code(cells, b);
        }
    }

    /// One pixel's R'G'B' to its `0x00RRGGBB`, as [`Self::row`] makes it.
    #[cfg(test)]
    fn pixel(&self, [r, g, b]: [f32; 3]) -> u32 {
        let mut channels = Channels {
            r: vec![r],
            g: vec![g],
            b: vec![b],
            k: vec![0.0],
        };
        let mut out = [0];
        self.row(&mut channels, &mut out);
        let [px] = out;
        px
    }
}

/// Each channel of a row times the row's factor for its pixel.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "floating-point arithmetic, which cannot overflow into undefined behaviour"
)]
fn scale(r: &mut [f32], g: &mut [f32], b: &mut [f32], k: &[f32]) {
    for (((r, g), b), &k) in r.iter_mut().zip(g.iter_mut()).zip(b.iter_mut()).zip(k) {
        *r *= k;
        *g *= k;
        *b *= k;
    }
}

/// A row's floats: its channels, and a factor for each pixel -- the scratch
/// a conversion works in, made once for its rows.
struct Channels {
    r: Vec<f32>,
    g: Vec<f32>,
    b: Vec<f32>,
    k: Vec<f32>,
}

impl Channels {
    fn new(width: usize) -> Self {
        Self {
            r: vec![0.0; width],
            g: vec![0.0; width],
            b: vec![0.0; width],
            k: vec![0.0; width],
        }
    }
}

// --- step 1, and the walk --------------------------------------------------------------

/// `picture`'s rows from `first` into `out` (whole rows) by `map`: what
/// [`to_argb_rows`] does once its checks have passed.
///
/// Subsampled chroma is brought to the picture's size as Chrome brings it
/// on the GPU: bilinear sampling of the chroma, sited at the centre of the
/// luma samples it covers, clamped at the picture's edges -- 9:3:3:1 of the
/// four nearest samples, in floating point, which is libavif's slow path's
/// upsampling to the letter (`reformat::chroma_of_row`). Chrome's pixels for
/// 4:2:0 HDR pictures of noise agree: an AVIF's in 98.8% of channels and a
/// video's in 98.8% (once its 10-bit samples are cut to 8, which headless
/// Chrome's 4:2:0 video does and a GPU's does not), the rest by one; libyuv's
/// integer upsampling, the ordinary conversion's, agrees in 76% and 40%.
#[allow(
    clippy::cast_precision_loss,
    reason = "a depth's largest code is under 2^16, exact in f32"
)]
fn convert<S: Sample>(picture: &Picture<'_, S>, map: &ToneMap<'_>, first: usize, out: &mut [u32]) {
    let Ok(state) = reformat::prepare(picture) else {
        return;
    };
    let max = state.max_channel;
    let unmultiply = picture.alpha_premultiplied && picture.alpha.is_some();
    let width = picture.width;
    let mut row = Row {
        state: &state,
        map,
        max,
        max_f: max as f32,
        depth: picture.depth,
        unmultiply,
        channels: Channels::new(width),
    };
    let grey = picture.format == Format::Yuv400 || picture.u.is_none() || picture.v.is_none();
    if grey {
        for (j, dst) in (first..).zip(out.chunks_exact_mut(width.max(1))) {
            let alpha = picture.alpha.map(|a| a.row(j));
            row.grey(picture.y.row(j), alpha, dst);
        }
        return;
    }
    let (_, chroma) = reformat::tables(&state);
    let mut chroma_rows = reformat::ChromaRows::default();
    for (j, dst) in (first..).zip(out.chunks_exact_mut(width.max(1))) {
        row.colour(picture, &chroma, &mut chroma_rows, j, dst);
    }
}

/// Each sample, clamped to the depth's largest code as libavif clamps it
/// before its table, as a fraction of its range: `(code - bias) / range`, in
/// single precision -- libavif's table entry for it, computed rather than
/// looked up.
#[allow(
    clippy::cast_precision_loss,
    clippy::arithmetic_side_effects,
    reason = "codes are under 2^16, exact in f32; float arithmetic cannot overflow into undefined behaviour"
)]
fn fraction<S: Sample>(samples: &[S], out: &mut [f32], (bias, range): (f32, f32), max: u32) {
    for (o, &s) in out.iter_mut().zip(samples) {
        let code: u32 = s.into();
        *o = (code.min(max) as f32 - bias) / range;
    }
}

/// What turns a row of samples into pixels: the state step 1 reads, the
/// map, and the row's floats.
struct Row<'a> {
    state: &'a reformat::State,
    map: &'a ToneMap<'a>,
    max: u32,
    max_f: f32,
    depth: u8,
    unmultiply: bool,
    channels: Channels,
}

impl Row<'_> {
    /// Row `j` of `picture`, its chroma upsampled (see [`convert`]) from the
    /// chroma `table`'s values.
    fn colour<S: Sample>(
        &mut self,
        picture: &Picture<'_, S>,
        table: &[f32],
        chroma_rows: &mut reformat::ChromaRows,
        j: usize,
        dst: &mut [u32],
    ) {
        let max = self.max;
        let y = picture.y.row(j);
        let n = y.len().min(dst.len());
        let Some(dst) = dst.get_mut(..n) else {
            return;
        };
        let (luma, _) = self.ranges();
        let Channels { r, g, b, .. } = &mut self.channels;
        fraction(y, r, luma, max);
        reformat::chroma_of_row(picture, self.state, table, chroma_rows, j, n, g, b);
        let alpha = picture.alpha.map(|a| a.row(j));
        self.matrix(y, alpha, dst);
    }

    /// The row's Y, Cb and Cr, in the channels, to R'G'B' there by the
    /// matrix, in place; then the rest of the way into `dst`.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "floating-point arithmetic, which cannot overflow into undefined behaviour"
    )]
    fn matrix<S: Sample>(&mut self, y: &[S], a: Option<&[S]>, dst: &mut [u32]) {
        let state = self.state;
        let (kr, kg, kb) = (state.kr, state.kg, state.kb);
        let (max, max_f) = (self.max, self.max_f);
        let Channels { r, g, b, .. } = &mut self.channels;
        let channels = r.iter_mut().zip(g.iter_mut()).zip(b.iter_mut());
        match state.mode {
            Mode::Coefficients => {
                for ((r, g), b) in channels {
                    [*r, *g, *b] = reformat::rgb_of(kr, kg, kb, *r, *g, *b);
                }
            }
            Mode::Identity => {
                for ((r, g), b) in channels {
                    (*r, *g, *b) = (*b, *r, *g);
                }
            }
            Mode::YCgCo => {
                for ((r, g), b) in channels {
                    let (yv, cb, cr) = (*r, *g, *b);
                    let t = yv - cb;
                    (*r, *g, *b) = (t + cr, yv + cb, t - cr);
                }
            }
            Mode::YCgCoRe | Mode::YCgCoRo => {
                for (((r, g), b), &y) in channels.zip(y) {
                    let code: u32 = y.into();
                    [*r, *g, *b] = reformat::ycgco_r(code.min(max), *g, *b, max_f);
                }
            }
        }
        self.finish(a, dst);
    }

    /// A row of grey: luma alone.
    fn grey<S: Sample>(&mut self, y: &[S], a: Option<&[S]>, dst: &mut [u32]) {
        let max = self.max;
        let Some(dst) = dst.get_mut(..y.len().min(dst.len())) else {
            return;
        };
        let (luma, _) = self.ranges();
        let Channels { r, g, b, .. } = &mut self.channels;
        fraction(y, r, luma, max);
        for ((&r, g), b) in r.iter().zip(g.iter_mut()).zip(b.iter_mut()) {
            (*g, *b) = (r, r);
        }
        self.finish(a, dst);
    }

    /// Luma's and chroma's bias and range, as libavif's tables divide by
    /// them: the identity matrix's chroma is luma's.
    fn ranges(&self) -> ((f32, f32), (f32, f32)) {
        let s = self.state;
        let luma = (s.bias_y, s.range_y);
        if s.mode == Mode::Identity {
            (luma, luma)
        } else {
            (luma, (s.bias_uv, s.range_uv))
        }
    }

    /// The row's R'G'B', in the channels: clamped, its premultiplication
    /// undone if it has one, mapped into `dst`, and its alpha bytes set.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::arithmetic_side_effects,
        reason = "alpha codes are under 2^16, exact in f32; the rounded alpha is clamped to a byte before it is used"
    )]
    fn finish<S: Sample>(&mut self, a: Option<&[S]>, dst: &mut [u32]) {
        let (max, max_f) = (self.max, self.max_f);
        let Channels { r, g, b, .. } = &mut self.channels;
        for channel in [&mut *r, &mut *g, &mut *b] {
            for c in channel.iter_mut() {
                *c = reformat::clamp_unit(*c);
            }
        }
        if let (true, Some(a)) = (self.unmultiply, a) {
            // libavif's undoing of premultiplication, which Chrome's AVIF
            // decoder asks of it, on the signal as it does it.
            for (((r, g), b), &a) in r.iter_mut().zip(g.iter_mut()).zip(b.iter_mut()).zip(a) {
                let a: u32 = a.into();
                let alpha = reformat::clamp_unit(a.min(max) as f32 / max_f);
                let undo = |c: f32| {
                    if alpha == 0.0 {
                        0.0
                    } else if alpha < 1.0 {
                        (c / alpha).min(1.0)
                    } else {
                        c
                    }
                };
                (*r, *g, *b) = (undo(*r), undo(*g), undo(*b));
            }
        }
        self.map.row(&mut self.channels, dst);
        match a {
            None => {
                for px in dst.iter_mut() {
                    *px |= 0xff00_0000;
                }
            }
            Some(a) => {
                let depth = self.depth;
                for (px, &a) in dst.iter_mut().zip(a) {
                    let a: u32 = a.into();
                    let byte = if depth == 8 {
                        a.min(255)
                    } else {
                        let alpha_f = a.min(max) as f32 / max_f;
                        ((0.5f32 + alpha_f * 255.0) as i32).clamp(0, 255) as u32
                    };
                    *px |= byte << 24;
                }
            }
        }
    }
}

/// `picture`, an HDR picture, converted to `0xAARRGGBB` pixels by `map`, as
/// Chrome shows it on an sRGB screen: `width * height` of them, row by row.
///
/// # Errors
///
/// As [`reformat::to_argb`]: [`Error::Unsupported`] for a matrix libavif
/// does not convert, [`Error::Size`] for no pixels, too many, or a plane
/// smaller than the picture.
pub fn to_argb<T: Reformat>(
    picture: &Picture<'_, T>,
    map: &ToneMap<'_>,
) -> Result<Vec<u32>, Error> {
    let mut out = Vec::new();
    to_argb_into(picture, map, &mut out)?;
    Ok(out)
}

/// [`to_argb`] into `out`, which is cleared first.
///
/// # Errors
///
/// As [`to_argb`]; `out` is then empty.
pub fn to_argb_into<T: Reformat>(
    picture: &Picture<'_, T>,
    map: &ToneMap<'_>,
    out: &mut Vec<u32>,
) -> Result<(), Error> {
    out.clear();
    let (_, count) = reformat::check(picture)?;
    out.try_reserve_exact(count).map_err(|_| Error::Size)?;
    out.resize(count, 0);
    convert(picture, map, 0, out);
    Ok(())
}

/// [`to_argb`] for a band of the picture: rows `first` on, as many as `out`
/// has room for, each exactly as the whole picture converts it -- for a
/// picture converted on several threads, a band each.
///
/// # Errors
///
/// As [`to_argb`]; and [`Error::Size`] for an `out` that is not whole rows,
/// or rows past the picture's last.
pub fn to_argb_rows<T: Reformat>(
    picture: &Picture<'_, T>,
    map: &ToneMap<'_>,
    first: usize,
    out: &mut [u32],
) -> Result<(), Error> {
    reformat::check_band(picture, first, out.len())?;
    convert(picture, map, first, out);
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "tests index what they build, compare floats they computed the same way, and fail loudly"
)]
mod tests {
    use super::*;
    use crate::Plane;
    use alloc::format;
    use alloc::string::String;

    /// Y, U and V planes of one row of `samples`.
    fn planes_of(samples: &[[u16; 3]]) -> [Vec<u16>; 3] {
        [0, 1, 2].map(|k| samples.iter().map(|s| s[k]).collect())
    }

    /// A one-row 10-bit 4:4:4 BT.2020 studio-range picture of `samples`.
    fn row_picture<'a>(planes: &'a [Vec<u16>; 3]) -> Picture<'a, u16> {
        let width = planes[0].len();
        let plane = |samples: &'a [u16]| Plane {
            samples,
            stride: width,
            width,
            height: 1,
        };
        Picture {
            width,
            height: 1,
            depth: 10,
            format: Format::Yuv444,
            matrix: 9,
            primaries: 9,
            full_range: false,
            y: plane(&planes[0]),
            u: Some(plane(&planes[1])),
            v: Some(plane(&planes[2])),
            alpha: None,
            alpha_premultiplied: false,
        }
    }

    /// Chrome 154's own pixels: lossless 10-bit 4:4:4 AVIFs of these samples
    /// (BT.2020, studio range, no light metadata), shown in headless Chrome
    /// on an sRGB screen and read back from its screenshot.
    const CHROME_PQ: [([u16; 3], [u8; 3]); 24] = [
        ([64, 512, 512], [0, 0, 0]),
        ([119, 512, 512], [1, 1, 1]),
        ([195, 512, 512], [8, 8, 8]),
        ([281, 512, 512], [29, 29, 29]),
        ([377, 512, 512], [63, 63, 63]),
        ([450, 512, 512], [99, 99, 99]),
        ([509, 512, 512], [136, 136, 136]),
        ([545, 512, 512], [163, 163, 164]),
        ([573, 512, 512], [188, 188, 188]),
        ([609, 512, 512], [211, 211, 211]),
        ([657, 512, 512], [232, 232, 232]),
        ([689, 512, 512], [244, 244, 244]),
        ([723, 512, 512], [255, 255, 255]),
        ([789, 512, 512], [255, 255, 255]),
        ([855, 512, 512], [255, 255, 255]),
        ([940, 512, 512], [255, 255, 255]),
        ([237, 418, 849], [255, 0, 0]),
        ([511, 269, 202], [0, 255, 0]),
        ([103, 849, 485], [0, 0, 255]),
        ([181, 448, 740], [172, 0, 0]),
        ([527, 491, 536], [198, 137, 118]),
        ([514, 563, 478], [35, 143, 217]),
        ([655, 191, 544], [255, 233, 0]),
        ([465, 587, 234], [0, 222, 207]),
    ];

    /// The same for HLG.
    const CHROME_HLG: [([u16; 3], [u8; 3]); 16] = [
        ([64, 512, 512], [0, 0, 0]),
        ([152, 512, 512], [9, 9, 9]),
        ([239, 512, 512], [31, 31, 31]),
        ([327, 512, 512], [54, 54, 54]),
        ([414, 512, 512], [76, 76, 76]),
        ([502, 512, 512], [99, 99, 99]),
        ([590, 512, 512], [126, 126, 126]),
        ([677, 512, 512], [164, 164, 164]),
        ([721, 512, 512], [188, 188, 188]),
        ([765, 512, 512], [207, 207, 207]),
        ([852, 512, 512], [233, 233, 233]),
        ([940, 512, 512], [255, 255, 255]),
        ([237, 418, 848], [209, 0, 0]),
        ([509, 270, 203], [0, 191, 0]),
        ([103, 848, 485], [0, 0, 153]),
        ([575, 448, 583], [179, 107, 89]),
    ];

    fn chrome_s_pixels(transfer: Transfer, patches: &[([u16; 3], [u8; 3])]) {
        let samples: Vec<[u16; 3]> = patches.iter().map(|p| p.0).collect();
        let planes = planes_of(&samples);
        let signal = Signal::new(transfer);
        let map = ToneMap::new(&signal, 9, Light::default());
        let out = to_argb(&row_picture(&planes), &map).unwrap();
        let mut wrong = String::new();
        for (&px, (yuv, want)) in out.iter().zip(patches) {
            let got = [(px >> 16) as u8, (px >> 8) as u8, px as u8];
            if got != *want || px >> 24 != 0xff {
                wrong += &format!("\n  {yuv:?}: {got:?} where Chrome shows {want:?}");
            }
        }
        assert!(wrong.is_empty(), "{transfer:?}:{wrong}");
    }

    #[test]
    fn pq_is_chrome_s_pixel_for_pixel() {
        chrome_s_pixels(Transfer::Pq, &CHROME_PQ);
    }

    #[test]
    fn hlg_is_chrome_s_pixel_for_pixel() {
        chrome_s_pixels(Transfer::Hlg, &CHROME_HLG);
    }

    /// The control points as the reference (`chrome_hdr.py`'s
    /// `rwtmo_alt0`, half precision) computes them: for a 4000 cd/m2 peak,
    /// and for 300 -- whose first slope is mathematically zero, and in single
    /// precision the residue of rounding, a half-precision subnormal whose
    /// value depends on the order each operation is rounded in. Here every
    /// operation is rounded as Skia's C++ rounds it; the reference rounds
    /// whole expressions once, so the two residues differ (-1.5e-6 and
    /// 3.0e-7), each moving the curve by under 1e-7.
    #[test]
    fn control_points_are_skia_s() {
        let peak_4000 = Curve::rwtmo(
            Light {
                max_cll: 4000.0,
                ..Light::default()
            }
            .headroom(),
        )
        .unwrap();
        // Each value as numpy prints the reference's float32: the shortest
        // decimal that is exactly it.
        let want_4000 = [
            [1.0, -1.0, 0.0],
            [1.541_015_6, -1.385_742_2, -0.699_707_03],
            [2.792_968_8, -2.050_781_2, -0.409_179_7],
            [4.753_906_2, -2.662_109_4, -0.241_699_22],
            [7.425_781_2, -3.173_828_1, -0.154_663_09],
            [10.804_687_5, -3.605_468_8, -0.106_445_31],
            [14.898_437_5, -3.976_562_5, -0.077_575_68],
            [19.703_125, -4.300_781_2, -0.059_234_62],
        ];
        assert_eq!(peak_4000.points, want_4000);
        let peak_300 = Curve::rwtmo(
            Light {
                max_cll: 300.0,
                ..Light::default()
            }
            .headroom(),
        )
        .unwrap();
        let want_300 = [
            [1.0, -0.188_476_56, 2.980_232_2e-7],
            [1.032_226_6, -0.198_120_12, -0.501_953_1],
            [1.076_171_9, -0.225_952_15, -0.723_144_53],
            [1.131_835_9, -0.270_019_53, -0.823_730_47],
            [1.200_195_3, -0.327_636_72, -0.861_816_4],
            [1.281_25, -0.397_216_8, -0.864_746_1],
            [1.373_046_9, -0.476_562_5, -0.846_679_7],
            [1.477_539_1, -0.563_476_56, -0.816_406_25],
        ];
        for (c, (got, want)) in peak_300.points.iter().zip(want_300).enumerate() {
            if c == 0 {
                assert_eq!(got[..2], want[..2]);
                assert!(
                    got[2].abs() < 2e-6,
                    "the knee's slope is a residue: {}",
                    got[2]
                );
            } else {
                assert_eq!(*got, want, "point {c}");
            }
        }
        // Content no brighter than the reference white has no tone map.
        assert_eq!(
            Light {
                max_cll: 203.0,
                ..Light::default()
            }
            .headroom(),
            0.0
        );
        assert!(Curve::rwtmo(0.0).is_none());
        // And no headroom past six stops.
        assert_eq!(
            Light {
                max_cll: 100_000.0,
                ..Light::default()
            }
            .headroom(),
            6.0
        );
    }

    /// The peak is MaxCLL, else the mastering display's, else 1000.
    #[test]
    fn the_peak_is_max_cll_then_mastering_then_1000() {
        let both = Light {
            max_cll: 600.0,
            mastering_peak: 4000.0,
        };
        assert_eq!(both.peak(), 600.0);
        assert_eq!(
            Light {
                max_cll: 0.0,
                mastering_peak: 4000.0
            }
            .peak(),
            4000.0
        );
        assert_eq!(Light::default().peak(), 1000.0);
    }

    /// Half precision rounds to nearest with ties to even, keeps
    /// subnormals, and overflows to infinity.
    #[test]
    fn half_rounds_as_f16c_does() {
        assert_eq!(half(1.0), 1.0);
        assert_eq!(half(65_504.0), 65_504.0);
        assert_eq!(half(65_519.0), 65_504.0);
        assert_eq!(half(65_520.0), f32::INFINITY);
        assert_eq!(half(-65_520.0), f32::NEG_INFINITY);
        // 1 + 2^-11 is a tie between 1 and 1 + 2^-10: to even, 1.
        assert_eq!(half(1.0 + 1.0 / 2048.0), 1.0);
        // 1 + 3 * 2^-11 ties between 1 + 2^-10 and 1 + 2^-9: to 1 + 2^-9.
        assert_eq!(half(1.0 + 3.0 / 2048.0), 1.0 + 2.0 / 1024.0);
        // Subnormals: multiples of 2^-24, ties to even.
        let q = 1.0 / 16_777_216.0;
        assert_eq!(half(5.0 * q), 5.0 * q);
        assert_eq!(half(2.5 * q), 2.0 * q);
        assert_eq!(half(3.5 * q), 4.0 * q);
        assert_eq!(half(-0.4 * q), -0.0);
        assert_eq!(half(0.0), 0.0);
    }

    /// BT.2020 to sRGB through Skia's matrices: the reference's, to the
    /// precision single precision carries.
    #[test]
    fn bt2020_to_srgb_is_skia_s() {
        let working = primaries_to_xyz_d50(REC2020_PRIMARIES).unwrap();
        let to_srgb = gamut_transform(&working, &SRGB);
        let to_working = gamut_transform(&REC2020, &working);
        let m = concat(&to_srgb, &to_working);
        let want = [
            [1.660_337_771, -0.587_795_331, -0.072_836_553],
            [-0.124_532_486, 1.132_992_493, -0.008_339_926],
            [-0.018_131_326, -0.100_585_460, 1.118_876_352],
        ];
        for (row, want) in m.iter().zip(want) {
            for (&got, want) in row.iter().zip(want) {
                assert!((f64::from(got) - want).abs() < 2e-6, "{got} against {want}");
            }
        }
    }

    /// The encoder rounds exactly: just below a code's least light is the
    /// code before, at it the code; and every light agrees with the
    /// formula in double precision but where the two straddle a boundary.
    #[test]
    fn the_encoder_rounds_exactly() {
        let e = Encoder::new();
        let bounds = bounds();
        for k in 0..255usize {
            let b = bounds[k];
            let below = f32::from_bits(b.to_bits() - 1);
            assert_eq!(e.code(below), k as u32, "below code {}", k + 1);
            assert_eq!(e.code(b), k as u32 + 1, "at code {}", k + 1);
        }
        assert_eq!(e.code(-1.0), 0);
        assert_eq!(e.code(f32::NAN), 0);
        assert_eq!(e.code(2.0), 255);
        let formula = |l: f32| {
            let l = f64::from(l).clamp(0.0, 1.0);
            let v = if l < 0.003_130_8 {
                12.92 * l
            } else {
                1.055 * libm::pow(l, 1.0 / 2.4) - 0.055
            };
            (v * 255.0 + 0.5) as u32
        };
        let mut seed = 1u32;
        for _ in 0..200_000 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let l = (seed >> 8) as f32 / 16_777_216.0;
            assert_eq!(e.code(l), formula(l), "{l}");
        }
    }

    /// The tables err by under a ten-thousandth of an 8-bit code wherever
    /// the light falls: each sample's error in its light, carried to the
    /// screen through sRGB's curve at its slope for that light. (The tone
    /// map's gain, never above 1, only flattens that slope -- to within 2%
    /// where it carries a light into the curve's straight foot.) Relative
    /// to the light the error is largest in PQ's toe, about 1e-5 at 0.001
    /// cd/m2, where it is a ten-millionth of a code.
    #[test]
    fn the_tables_err_by_a_ten_thousandth_of_a_code() {
        let slope = |x: f64| {
            255.0
                * if x < 0.003_130_8 {
                    12.92
                } else {
                    1.055 / 2.4 * libm::pow(x, 1.0 / 2.4 - 1.0)
                }
        };
        let codes = |exact: f64, got: f32, scale: f64| {
            (f64::from(got) - exact).abs() * scale * slope(exact * scale) * 1.02
        };
        let pq = Signal::new(Transfer::Pq);
        let hlg = Signal::new(Transfer::Hlg);
        let mut worst = (0.0f64, 0.0f64);
        for i in 0..=1_000_000u32 {
            let c = i as f32 / 1_000_000.0;
            let exact = pqish(f64::from(c));
            worst.0 = worst.0.max(codes(exact, pq.pq(c), 10_000.0 / 203.0));
            let exact = hlgish(f64::from(c));
            worst.1 = worst.1.max(codes(exact, hlg.hlg(c), 1000.0 / 203.0));
        }
        assert!(worst.0 < 1e-4, "PQ errs by {:e} of a code", worst.0);
        assert!(worst.1 < 1e-4, "HLG errs by {:e} of a code", worst.1);
    }

    /// The whole of a pixel's arithmetic in double precision, from the same
    /// control points and matrices: `chrome_hdr.py`'s `pixel`.
    fn reference(map: &ToneMap<'_>, rgb: [f32; 3]) -> [u32; 3] {
        let f = |m: &Matrix| m.map(|r| r.map(f64::from));
        let mul =
            |m: &[[f64; 3]; 3], v: [f64; 3]| m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2]);
        let rgb = rgb.map(f64::from);
        let light = match map.signal.transfer {
            Transfer::Pq => rgb.map(pqish),
            Transfer::Hlg => {
                let s = rgb.map(hlgish);
                let w = map.ootf.map(f64::from);
                let y = w[0] * s[0] + w[1] * s[1] + w[2] * s[2];
                let k = if y > 0.0 {
                    libm::pow(y, f64::from(HLG_GAMMA - 1.0))
                } else {
                    0.0
                };
                s.map(|c| c * k)
            }
        };
        let working = mul(&f(&map.to_working), light);
        let gain = map.curve.map_or(1.0, |curve| {
            let p = curve.points.map(|p| p.map(f64::from));
            let x = working[0].max(working[1]).max(working[2]);
            let g = if x <= p[0][0] {
                p[0][1]
            } else if x >= p[7][0] {
                p[7][1] + libm::log2(p[7][0] / x)
            } else {
                let i = p.iter().rposition(|q| q[0] <= x).unwrap();
                let (pi, pj) = (p[i], p[i + 1]);
                let h = pj[0] - pi[0];
                let (mi, mj) = (pi[2] * h, pj[2] * h);
                let c3 = 2.0 * pi[1] + mi - 2.0 * pj[1] + mj;
                let c2 = -3.0 * pi[1] + 3.0 * pj[1] - 2.0 * mi - mj;
                let t = (x - pi[0]) / h;
                ((c3 * t + c2) * t + mi) * t + pi[1]
            };
            libm::exp2(g)
        });
        let out = mul(&f(&map.to_srgb), working.map(|c| c * gain));
        out.map(|l| {
            let l = l.clamp(0.0, 1.0);
            let v = if l < 0.003_130_8 {
                12.92 * l
            } else {
                1.055 * libm::pow(l, 1.0 / 2.4) - 0.055
            };
            (v * 255.0 + 0.5) as u32
        })
    }

    /// Single precision and the tables against double precision, over every
    /// headroom's curve and both transfers: never more than one apart, and
    /// one apart in under one pixel in ten thousand.
    #[test]
    fn single_precision_is_the_reference_to_the_last_bit_nearly_always() {
        for transfer in [Transfer::Pq, Transfer::Hlg] {
            let signal = Signal::new(transfer);
            for max_cll in [0.0, 250.0, 600.0, 4000.0, 10_000.0] {
                let map = ToneMap::new(
                    &signal,
                    9,
                    Light {
                        max_cll,
                        ..Light::default()
                    },
                );
                let (mut off, mut total) = (0u32, 0u32);
                let mut seed = 7u32;
                for _ in 0..60_000 {
                    let mut next = || {
                        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        (seed >> 8) as f32 / 16_777_216.0
                    };
                    let rgb = [next(), next(), next()];
                    let px = map.pixel(rgb);
                    let got = [(px >> 16) & 0xff, (px >> 8) & 0xff, px & 0xff];
                    let want = reference(&map, rgb);
                    for (g, w) in got.into_iter().zip(want) {
                        assert!(
                            g.abs_diff(w) <= 1,
                            "{transfer:?} {max_cll}: {rgb:?} {got:?} {want:?}"
                        );
                        off += u32::from(g != w);
                        total += 1;
                    }
                }
                assert!(
                    off * 10_000 < total,
                    "{transfer:?} at MaxCLL {max_cll}: {off} of {total} channels off by one"
                );
            }
        }
    }

    /// Black is black, and the content's peak and anything brighter are the
    /// screen's white; below the reference white the gain is the same for
    /// every light, so it keeps its proportions.
    #[test]
    fn the_curve_ends_at_white_and_keeps_the_dark_s_proportions() {
        let signal = Signal::new(Transfer::Pq);
        let map = ToneMap::new(
            &signal,
            9,
            Light {
                max_cll: 1000.0,
                ..Light::default()
            },
        );
        assert_eq!(map.pixel([0.0; 3]), 0);
        let curve = map.curve.unwrap();
        // Below the knee the gain is the reference white's: a half.
        assert_eq!(curve.gain(0.25), curve.gain(1.0));
        assert!((curve.gain(1.0) - 0.5).abs() < 1e-3);
        // Light times gain rises to the peak and stays at about 1.
        let mut last = 0.0f32;
        for i in 0..=2000 {
            let x = i as f32 / 100.0;
            let y = x * curve.gain(x);
            assert!(y >= last - 1e-6, "falls at {x}: {y} after {last}");
            last = y;
        }
        assert!((last - 1.0).abs() < 2e-3);
    }

    /// A picture's band converts exactly as the same rows of the whole
    /// picture, 4:2:0 at an odd height included.
    #[test]
    fn every_band_converts_as_the_whole_picture_does() {
        let (width, height) = (9usize, 7usize);
        let cw = width.div_ceil(2);
        let ch = height.div_ceil(2);
        let mut seed = 3u32;
        let mut next = |n: u32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 16) % n
        };
        let y: Vec<u16> = (0..width * height).map(|_| 64 + next(877) as u16).collect();
        let u: Vec<u16> = (0..cw * ch).map(|_| 64 + next(897) as u16).collect();
        let v: Vec<u16> = (0..cw * ch).map(|_| 64 + next(897) as u16).collect();
        let picture = Picture {
            width,
            height,
            depth: 10,
            format: Format::Yuv420,
            matrix: 9,
            primaries: 9,
            full_range: false,
            y: Plane {
                samples: &y,
                stride: width,
                width,
                height,
            },
            u: Some(Plane {
                samples: &u,
                stride: cw,
                width: cw,
                height: ch,
            }),
            v: Some(Plane {
                samples: &v,
                stride: cw,
                width: cw,
                height: ch,
            }),
            alpha: None,
            alpha_premultiplied: false,
        };
        let signal = Signal::new(Transfer::Pq);
        let map = ToneMap::new(&signal, 9, Light::default());
        let whole = to_argb(&picture, &map).unwrap();
        for first in 0..height {
            for count in 1..=height - first {
                let mut band = vec![0u32; count * width];
                to_argb_rows(&picture, &map, first, &mut band).unwrap();
                assert_eq!(
                    band,
                    whole[first * width..(first + count) * width],
                    "rows {first}..+{count}"
                );
            }
        }
        // A band past the last row, or not whole rows, is refused.
        let mut past = vec![0u32; width * 2];
        assert_eq!(
            to_argb_rows(&picture, &map, height - 1, &mut past),
            Err(Error::Size)
        );
        let mut ragged = vec![0u32; width + 1];
        assert_eq!(
            to_argb_rows(&picture, &map, 0, &mut ragged),
            Err(Error::Size)
        );
    }

    /// 4:2:0's chroma is brought up 9:3:3:1, in floating point, centred:
    /// each pixel is its own computation from the four nearest samples --
    /// at a corner the one sample alone, inside the nearest 9/16, the
    /// column and row towards the pixel 3/16 each, and between them 1/16.
    #[test]
    fn chroma_is_upsampled_nine_three_three_one() {
        fn plane(samples: &[u16], w: usize, h: usize) -> Plane<'_, u16> {
            Plane {
                samples,
                stride: w,
                width: w,
                height: h,
            }
        }
        // Grey luma; Cb varying across a 2x2 chroma plane, Cr neutral.
        let (width, height) = (4usize, 4usize);
        let (y, u, v) = ([502u16; 16], [600u16, 700, 500, 400], [512u16; 4]);
        let picture = Picture {
            width,
            height,
            depth: 10,
            format: Format::Yuv420,
            matrix: 9,
            primaries: 9,
            full_range: false,
            y: plane(&y, width, height),
            u: Some(plane(&u, 2, 2)),
            v: Some(plane(&v, 2, 2)),
            alpha: None,
            alpha_premultiplied: false,
        };
        let signal = Signal::new(Transfer::Pq);
        let map = ToneMap::new(&signal, 9, Light::default());
        let out = to_argb(&picture, &map).unwrap();
        let state = reformat::prepare(&picture).unwrap();
        let t = |code: u16| (f32::from(code) - 512.0) / 896.0;
        let luma = (502.0f32 - 64.0) / 876.0;
        let want = |cb: f32| {
            let rgb = reformat::rgb_of(state.kr, state.kg, state.kb, luma, cb, 0.0);
            map.pixel(rgb.map(reformat::clamp_unit)) | 0xff00_0000
        };
        // The corner: the one sample.
        assert_eq!(out[0], want(t(600)));
        // Pixel (1, 1): nearest (0, 0), its column and row neighbours
        // towards the pixel (1, 0) and (0, 1), and (1, 1) between them.
        let inside = t(600) * (9.0 / 16.0)
            + t(700) * (3.0 / 16.0)
            + t(500) * (3.0 / 16.0)
            + t(400) * (1.0 / 16.0);
        assert_eq!(out[width + 1], want(inside));
        // Pixel (2, 0), on the top row: nearest (1, 0), the column towards
        // it (0, 0); the row is the nearest's own.
        let top = t(700) * (9.0 / 16.0)
            + t(600) * (3.0 / 16.0)
            + t(700) * (3.0 / 16.0)
            + t(600) * (1.0 / 16.0);
        assert_eq!(out[2], want(top));
    }

    /// Alpha is carried as libavif carries it: 10-bit alpha rescaled in
    /// floating point; and premultiplied colour is divided by it before its
    /// light is found.
    #[test]
    fn alpha_is_carried_and_premultiplication_undone() {
        let samples = [[573u16, 512, 512]; 3];
        let planes = planes_of(&samples);
        let alpha = [1023u16, 512, 0];
        let mut picture = row_picture(&planes);
        picture.alpha = Some(Plane {
            samples: &alpha,
            stride: 3,
            width: 3,
            height: 1,
        });
        let signal = Signal::new(Transfer::Pq);
        let map = ToneMap::new(&signal, 9, Light::default());
        let straight = to_argb(&picture, &map).unwrap();
        assert_eq!(straight[0], 0xffbc_bcbc, "203 cd/m2 grey, opaque");
        assert_eq!(straight[1] >> 24, 128);
        assert_eq!(
            straight[1] & 0xff_ffff,
            0xbc_bcbc,
            "straight colour is untouched"
        );
        assert_eq!(straight[2] >> 24, 0);
        picture.alpha_premultiplied = true;
        let undone = to_argb(&picture, &map).unwrap();
        assert_eq!(undone[0], straight[0]);
        assert!(undone[1] & 0xff > 0xbc, "divided by alpha, brighter");
        assert_eq!(undone[2], 0, "no alpha, no colour");
    }

    /// Grey pictures and the identity matrix: luma alone, and G, B and R
    /// carried as Y, U and V.
    #[test]
    fn grey_and_identity_pictures_convert() {
        let signal = Signal::new(Transfer::Pq);
        let map = ToneMap::new(&signal, 9, Light::default());
        fn one(samples: &[u16]) -> Plane<'_, u16> {
            Plane {
                samples,
                stride: 1,
                width: 1,
                height: 1,
            }
        }
        let y = [573u16];
        let grey = Picture {
            width: 1,
            height: 1,
            depth: 10,
            format: Format::Yuv400,
            matrix: 9,
            primaries: 9,
            full_range: false,
            y: one(&y),
            u: None,
            v: None,
            alpha: None,
            alpha_premultiplied: false,
        };
        assert_eq!(to_argb(&grey, &map).unwrap(), [0xffbc_bcbc]);
        // Full-range identity: R' = V, G' = Y, B' = U.
        let (g, b, r) = ([1023u16], [0u16], [0u16]);
        let identity = Picture {
            format: Format::Yuv444,
            matrix: 0,
            full_range: true,
            y: one(&g),
            u: Some(one(&b)),
            v: Some(one(&r)),
            ..grey
        };
        let px = to_argb(&identity, &map).unwrap()[0];
        assert_eq!(
            px & 0xff00_ff00,
            0xff00_ff00,
            "green at its peak is the screen's"
        );
        assert_eq!(px & 0x00ff_00ff, 0, "and nothing else");
    }

    /// Primaries other than BT.2020's are carried into BT.2020 for the tone
    /// map and out to sRGB. Each one's white is adapted to sRGB's, so grey
    /// stays grey whatever its primaries; and Display P3's red, nearer
    /// sRGB's than BT.2020's is, is less red on sRGB.
    #[test]
    fn other_primaries_take_their_own_matrix() {
        let signal = Signal::new(Transfer::Pq);
        let light = Light::default();
        let white = |primaries: u16| ToneMap::new(&signal, primaries, light).pixel([0.58; 3]);
        let grey = white(9);
        for primaries in [1, 4, 5, 6, 7, 8, 10, 11, 12, 22] {
            let px = white(primaries);
            for shift in [0, 8, 16] {
                let (a, b) = ((px >> shift) & 0xff, (grey >> shift) & 0xff);
                assert!(
                    a.abs_diff(b) <= 2,
                    "primaries {primaries}: {px:06x} against {grey:06x}"
                );
            }
        }
        let red =
            |primaries: u16| ToneMap::new(&signal, primaries, light).pixel([0.58, 0.0, 0.0]) >> 16;
        assert!(
            red(12) < red(9),
            "P3 {} against BT.2020 {}",
            red(12),
            red(9)
        );
        // An unknown code is taken as BT.2020, as Chrome guesses it.
        assert_eq!(red(200), red(9));
        // BT.709's primaries are sRGB's: its red, carried into BT.2020 for
        // the tone map and back out, is the screen's red and nothing else.
        let bt709_red = ToneMap::new(&signal, 1, light).pixel([0.58, 0.0, 0.0]);
        assert!(bt709_red >> 16 > 0, "{bt709_red:06x}");
        assert_eq!(bt709_red & 0xffff, 0, "{bt709_red:06x}: green or blue lit");
    }
}
