//! SGI LogLuv compression (34676, and 34677 for its 24-bit form): libtiff
//! 4.7.1's `tif_luv.c`, as libtiff's RGBA reader drives it.
//!
//! Greg Ward Larson's high-dynamic-range encodings: luminance as a
//! logarithm -- `LogL`, 16 bits: a sign, then log2 in 1/256 steps -- and
//! colour as CIE (u', v'), either two bytes (`LogLuv32`) or an index into a
//! table of the gamut's squares (`LogLuv24`, whose luminance has 10 bits).
//! The RGBA reader asks the codec for 8-bit samples (`SGILOGDATAFMT_8BIT`),
//! so the codec turns each pixel into grey or RGB itself: luminance through
//! `exp`, colour through CCIR 709 primaries, both through a gamma of 2.
//!
//! A row of `LogL` or `LogLuv32` is stored as byte planes, most significant
//! first, each run-length coded: a byte of 128 or more is a run of
//! `byte - 126` copies of the byte after it, a smaller one that many bytes
//! as they are. `LogLuv24` is three bytes a pixel. libtiff's decoding is kept
//! to the byte, including where a literal run is longer than the row: the
//! bytes left over are read as the next plane's first codes.
//!
//! # Exactly libtiff's numbers
//!
//! The conversions are `double` arithmetic, which comes out the same
//! everywhere, but for `exp` and `sqrt`. `sqrt` is IEEE's, correctly
//! rounded; this crate has no maths library, so it is done here in integers.
//! `exp` is glibc's (2.39), which is correctly rounded on all but 21 of the
//! 33,790 arguments these conversions can pass it: here a correctly rounded
//! `exp` in double-double arithmetic, and glibc's 21 answers where they
//! differ -- checked against glibc for every argument (a test holds the
//! hash of all of glibc's results).

use alloc::vec::Vec;
use core::f64::consts::LN_2;

use super::dir::{Directory, compression, photometric};

/// What a row turns into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// `LogL16Decode`, then `L16toGry`.
    Grey,
    /// `LogLuvDecode24`, then `Luv24toRGB`.
    Luv24,
    /// `LogLuvDecode32`, then `Luv32toRGB`.
    Luv32,
}

/// The codec's state (`LogLuvState`) for 8-bit samples.
pub(super) struct Luv {
    kind: Kind,
    /// Bytes a pixel hands out (`pixel_size`).
    pixel_size: usize,
    /// Pixels the translation buffer holds (`tbuflen`).
    tbuflen: usize,
    /// A row's pixels as stored (`tbuf`).
    row: Vec<u32>,
    /// Luminances worked out so far, by their code (0 for not yet): the
    /// 16-bit codes' magnitudes, or the 10-bit codes.
    luminance: Vec<u64>,
}

/// `LogLuvSetupDecode` for `SGILOGDATAFMT_8BIT`, the format the RGBA reader
/// sets: `None` where libtiff's setup fails.
pub(super) fn setup(dir: &Directory, max_bytes: usize) -> Option<Luv> {
    let (kind, pixel_size) = match dir.photometric_or_zero() {
        // `LogLuvInitState`.
        photometric::LOGLUV => {
            if dir.planar_config != 1 {
                return None;
            }
            if dir.compression == compression::SGILOG24 {
                (Kind::Luv24, 3)
            } else {
                (Kind::Luv32, 3)
            }
        }
        // `LogL16InitState`.
        photometric::LOGL => {
            if dir.samples_per_pixel != 1 {
                return None;
            }
            (Kind::Grey, 1)
        }
        _ => return None,
    };
    // `multiply_ms`: 0 for a factor of 0 or a product past `tmsize_t`, and
    // then no translation buffer, and no setup.
    let product = |a: u32, b: u32| {
        u64::from(a)
            .checked_mul(u64::from(b))
            .filter(|&n| n != 0 && n <= i64::MAX as u64 / 4)
    };
    let tbuflen = if dir.tiled {
        product(dir.tile_width, dir.tile_length)
    } else if dir.rows_per_strip < dir.length {
        product(dir.width, dir.rows_per_strip)
    } else {
        product(dir.width, dir.length)
    }?;
    // Its `malloc`, held to the decode's limit.
    let bytes = usize::try_from(tbuflen.checked_mul(4)?).ok()?;
    if bytes > max_bytes {
        return None;
    }
    Some(Luv {
        kind,
        pixel_size,
        tbuflen: usize::try_from(tbuflen).ok()?,
        row: Vec::new(),
        luminance: Vec::new(),
    })
}

impl Luv {
    /// `LogLuvDecodeStrip` and `LogLuvDecodeTile`: `out` decoded a row of
    /// `row_len` bytes at a time from `data`; false at the first row that
    /// fails, as libtiff stops there.
    pub(super) fn decode(&mut self, data: &[u8], out: &mut [u8], row_len: usize) -> bool {
        if row_len == 0 {
            return false;
        }
        let mut raw = data;
        out.chunks_mut(row_len)
            .all(|row| self.decode_row(&mut raw, row))
    }

    /// One row (`tif_decoderow`): the stored pixels, then their samples.
    fn decode_row(&mut self, raw: &mut &[u8], out: &mut [u8]) -> bool {
        let npixels = out.len().checked_div(self.pixel_size).unwrap_or(0);
        // "Translation buffer too short".
        if self.tbuflen < npixels {
            return false;
        }
        self.row.clear();
        self.row.resize(npixels, 0);
        let decoded = match self.kind {
            Kind::Grey => planes(raw, &mut self.row, &[8, 0]),
            Kind::Luv32 => planes(raw, &mut self.row, &[24, 16, 8, 0]),
            Kind::Luv24 => triples(raw, &mut self.row),
        };
        if !decoded {
            return false;
        }
        match self.kind {
            Kind::Grey => {
                for (&p, slot) in self.row.iter().zip(out.iter_mut()) {
                    let y = log_l16_to_y(&mut self.luminance, p);
                    *slot = gamma8(y);
                }
            }
            Kind::Luv24 | Kind::Luv32 => {
                for (&p, rgb) in self.row.iter().zip(out.chunks_exact_mut(3)) {
                    let xyz = if self.kind == Kind::Luv24 {
                        luv24_to_xyz(&mut self.luminance, p)
                    } else {
                        luv32_to_xyz(&mut self.luminance, p)
                    };
                    let [r, g, b] = xyz_to_rgb24(xyz);
                    if let [rs, gs, bs] = rgb {
                        *rs = r;
                        *gs = g;
                        *bs = b;
                    }
                }
            }
        }
        true
    }
}

/// The run-length coded byte planes of `LogL16Decode` and `LogLuvDecode32`:
/// each plane ORed into `values` at its shift, the input consumed as far as
/// libtiff consumes it. False where a plane runs out before the row is
/// full ("Not enough data").
fn planes(raw: &mut &[u8], values: &mut [u32], shifts: &[u32]) -> bool {
    let n = values.len();
    for &shift in shifts {
        let mut i = 0;
        while i < n {
            let Some((&code, rest)) = raw.split_first() else {
                break;
            };
            if code >= 128 {
                // A run: `code - 126` copies of the next byte.
                let Some((&byte, rest)) = rest.split_first() else {
                    break;
                };
                *raw = rest;
                let b = u32::from(byte) << shift;
                let mut left = code.wrapping_sub(126);
                while left != 0 && i < n {
                    left = left.wrapping_sub(1);
                    if let Some(v) = values.get_mut(i) {
                        *v |= b;
                    }
                    i = i.wrapping_add(1);
                }
            } else {
                // `code` bytes as they are -- or as many as the row and the
                // data hold; what the row does not take is left for the
                // next code.
                let mut left = code;
                let mut rest = rest;
                while let Some((&byte, tail)) = rest.split_first() {
                    if left == 0 || i >= n {
                        break;
                    }
                    left = left.wrapping_sub(1);
                    if let Some(v) = values.get_mut(i) {
                        *v |= u32::from(byte) << shift;
                    }
                    i = i.wrapping_add(1);
                    rest = tail;
                }
                *raw = rest;
            }
        }
        if i != n {
            return false;
        }
    }
    true
}

/// `LogLuvDecode24`: three bytes a pixel, most significant first.
fn triples(raw: &mut &[u8], values: &mut [u32]) -> bool {
    for v in values.iter_mut() {
        let Some((&[a, b, c], rest)) = raw.split_first_chunk::<3>() else {
            return false;
        };
        *v = u32::from(a) << 16 | u32::from(b) << 8 | u32::from(c);
        *raw = rest;
    }
    true
}

/// `(uint8_t)` of a luminance or channel through a gamma of 2: 0 at or
/// below 0, 255 at or above 1, else `(int)(256. * sqrt(v))` -- and 0 for
/// NaN, which fails both tests and truncates to `INT_MIN` on x86.
fn gamma8(v: f64) -> u8 {
    if v.is_nan() || v <= 0.0 {
        0
    } else if v >= 1.0 {
        255
    } else {
        // Below 256, as `v` is below 1; the cast truncates, as C's does.
        (256.0 * sqrt(v)) as u8
    }
}

/// `f(code)`, kept in `cache` (sized `len`) once found: every luminance
/// here is positive, so 0 marks one not yet worked out.
fn cached(cache: &mut Vec<u64>, len: usize, code: u32, f: fn(u32) -> f64) -> f64 {
    if cache.len() != len {
        cache.clear();
        cache.resize(len, 0);
    }
    let at = usize::try_from(code).unwrap_or(usize::MAX);
    match cache.get(at).copied() {
        Some(bits) if bits != 0 => f64::from_bits(bits),
        _ => {
            let y = f(code);
            if let Some(slot) = cache.get_mut(at) {
                *slot = y.to_bits();
            }
            y
        }
    }
}

/// `LogL16toY`: the luminance of a 16-bit code -- its sign, then log2 in
/// 1/256 steps from 2^-64. `cache` keeps each magnitude's `exp` once found.
fn log_l16_to_y(cache: &mut Vec<u64>, p16: u32) -> f64 {
    let le = p16 & 0x7fff;
    if le == 0 {
        return 0.0;
    }
    let y = cached(cache, 0x8000, le, glibc_exp16);
    if p16 & 0x8000 == 0 { y } else { -y }
}

/// `exp(M_LN2 / 256. * (Le + .5) - M_LN2 * 64.)`, as glibc answers it.
fn glibc_exp16(le: u32) -> f64 {
    if let Ok(at) = GLIBC_L16.binary_search_by_key(&le, |&(k, _)| u32::from(k)) {
        if let Some(&(_, bits)) = GLIBC_L16.get(at) {
            return f64::from_bits(bits);
        }
    }
    exp(LN_2 / 256.0 * (f64::from(le) + 0.5) - LN_2 * 64.0)
}

/// `LogL10toY`: the luminance of `LogLuv24`'s 10-bit code, log2 in 1/64
/// steps from 2^-12.
fn log_l10_to_y(cache: &mut Vec<u64>, p10: u32) -> f64 {
    if p10 == 0 {
        return 0.0;
    }
    cached(cache, 0x400, p10, glibc_exp10)
}

/// `exp(M_LN2 / 64. * (p10 + .5) - M_LN2 * 12.)`, as glibc answers it.
fn glibc_exp10(p10: u32) -> f64 {
    if let Ok(at) = GLIBC_L10.binary_search_by_key(&p10, |&(k, _)| u32::from(k)) {
        if let Some(&(_, bits)) = GLIBC_L10.get(at) {
            return f64::from_bits(bits);
        }
    }
    exp(LN_2 / 64.0 * (f64::from(p10) + 0.5) - LN_2 * 12.0)
}

/// `U_NEU` and `V_NEU`: the neutral chroma a code off the table decodes to.
const U_NEU: f64 = 0.210_526_316;
const V_NEU: f64 = 0.473_684_211;
/// `UVSCALE`: `LogLuv32`'s chroma steps.
const UVSCALE: f64 = 410.0;
/// `UV_NDIVS`: the squares of `LogLuv24`'s chroma table.
const UV_NDIVS: u32 = 16289;

/// `uv_decode`: the (u', v') at the middle of chroma square `c`, or `None`
/// for a code past the table.
fn uv_decode(c: u32) -> Option<(f64, f64)> {
    if c >= UV_NDIVS {
        return None;
    }
    let row = |vi: usize| UV_ROW.get(vi).copied().unwrap_or((0, 0, 0));
    let c = i64::from(c);
    // Binary search for the row whose squares hold `c`.
    let (mut lower, mut upper) = (0usize, UV_ROW.len());
    while upper.wrapping_sub(lower) > 1 {
        let vi = lower.wrapping_add(upper) >> 1;
        match c.cmp(&i64::from(row(vi).2)) {
            core::cmp::Ordering::Greater => lower = vi,
            core::cmp::Ordering::Less => upper = vi,
            core::cmp::Ordering::Equal => {
                lower = vi;
                break;
            }
        }
    }
    let (ustart, _, ncum) = row(lower);
    let ui = c.wrapping_sub(i64::from(ncum));
    let sq = f64::from(f32::from_bits(UV_SQSIZ));
    // `ui` is below 2^15 and `lower` below 163: exact in a `double`.
    let u = f64::from(f32::from_bits(ustart)) + (ui as f64 + 0.5) * sq;
    let v = f64::from(f32::from_bits(UV_VSTART)) + (lower as f64 + 0.5) * sq;
    Some((u, v))
}

/// XYZ from luminance `l` and chroma (u', v'), each to a `float`, as
/// `LogLuv24toXYZ` and `LogLuv32toXYZ` finish.
fn xyz(l: f64, u: f64, v: f64) -> [f32; 3] {
    let s = 1.0 / (6.0 * u - 16.0 * v + 12.0);
    let x = 9.0 * u * s;
    let y = 4.0 * v * s;
    // Rounded to `float`, as C's casts round.
    [(x / y * l) as f32, l as f32, ((1.0 - x - y) / y * l) as f32]
}

/// `LogLuv24toXYZ`.
fn luv24_to_xyz(cache: &mut Vec<u64>, p: u32) -> [f32; 3] {
    let l = log_l10_to_y(cache, p >> 14 & 0x3ff);
    if l <= 0.0 {
        return [0.0; 3];
    }
    let (u, v) = uv_decode(p & 0x3fff).unwrap_or((U_NEU, V_NEU));
    xyz(l, u, v)
}

/// `LogLuv32toXYZ`.
fn luv32_to_xyz(cache: &mut Vec<u64>, p: u32) -> [f32; 3] {
    let l = log_l16_to_y(cache, p >> 16);
    if l <= 0.0 {
        return [0.0; 3];
    }
    let u = 1.0 / UVSCALE * (f64::from(p >> 8 & 0xff) + 0.5);
    let v = 1.0 / UVSCALE * (f64::from(p & 0xff) + 0.5);
    xyz(l, u, v)
}

/// `XYZtoRGB24`: CCIR 709 primaries, then a gamma of 2.
fn xyz_to_rgb24(xyz: [f32; 3]) -> [u8; 3] {
    let [x, y, z] = xyz.map(f64::from);
    let r = 2.690 * x + -1.276 * y + -0.414 * z;
    let g = -1.022 * x + 1.978 * y + 0.044 * z;
    let b = 0.061 * x + -0.224 * y + 1.163 * z;
    [gamma8(r), gamma8(g), gamma8(b)]
}

/// The IEEE square root of a positive finite `x`, correctly rounded: the
/// integer square root of its significand, rounded to nearest.
fn sqrt(x: f64) -> f64 {
    let bits = x.to_bits();
    let biased = i32::try_from(bits >> 52 & 0x7ff).unwrap_or(0);
    let mut mant = bits & ((1 << 52) - 1);
    let mut e = if biased == 0 {
        // Subnormal: bring the leading bit up to bit 52.
        let shift = mant.leading_zeros().saturating_sub(11);
        mant <<= shift;
        mant &= (1 << 52) - 1;
        1i32.wrapping_sub(i32::try_from(shift).unwrap_or(0))
    } else {
        biased
    };
    // x = m * 2^e with m a 53-bit integer.
    let mut m = u128::from(mant | 1 << 52);
    e = e.wrapping_sub(1075);
    if e & 1 != 0 {
        m <<= 1;
        e = e.wrapping_sub(1);
    }
    // sqrt(m * 2^52) has 53 bits; x's root is it times 2^((e - 52) / 2).
    let big = m << 52;
    let mut q = isqrt(big);
    // Round to nearest: up when the root's fraction is past a half, which
    // for a remainder `r = big - q^2` is `r > q` (it is never exactly half).
    let r = big.wrapping_sub(q.wrapping_mul(q));
    if r > q {
        q = q.wrapping_add(1);
    }
    let mut half = e.wrapping_sub(52) / 2;
    if q == 1 << 53 {
        q = 1 << 52;
        half = half.wrapping_add(1);
    }
    let exponent = u64::try_from(half.wrapping_add(52 + 1023)).unwrap_or(0);
    let fraction = u64::try_from(q).unwrap_or(0) & ((1 << 52) - 1);
    f64::from_bits(exponent << 52 | fraction)
}

/// The integer square root, rounded down: digit by digit.
fn isqrt(n: u128) -> u128 {
    let mut rem = n;
    let mut root = 0u128;
    let mut bit = 1u128 << 126;
    while bit > n {
        bit >>= 2;
    }
    while bit != 0 {
        let trial = root.wrapping_add(bit);
        if rem >= trial {
            rem = rem.wrapping_sub(trial);
            root = (root >> 1).wrapping_add(bit);
        } else {
            root >>= 1;
        }
        bit >>= 2;
    }
    root
}

/// A number as an unevaluated sum of two `double`s, the second at most half
/// an ulp of the first: about 106 bits.
#[derive(Clone, Copy)]
struct Dd(f64, f64);

/// `a + b` exactly, as a rounded sum and its error (Knuth).
fn two_sum(a: f64, b: f64) -> Dd {
    let s = a + b;
    let bb = s - a;
    Dd(s, (a - (s - bb)) + (b - bb))
}

/// `a + b` exactly, for `|a| >= |b|` (Dekker).
fn fast_two_sum(a: f64, b: f64) -> Dd {
    let s = a + b;
    Dd(s, b - (s - a))
}

/// `a` split into two halves of 26 bits (Veltkamp).
fn split(a: f64) -> (f64, f64) {
    let c = 134_217_729.0 * a;
    let hi = c - (c - a);
    (hi, a - hi)
}

/// `a * b` exactly, as a rounded product and its error (Dekker).
fn two_prod(a: f64, b: f64) -> Dd {
    let p = a * b;
    let (ah, al) = split(a);
    let (bh, bl) = split(b);
    Dd(p, ((ah * bh - p) + ah * bl + al * bh) + al * bl)
}

fn dd_mul(a: Dd, b: Dd) -> Dd {
    let p = two_prod(a.0, b.0);
    fast_two_sum(p.0, p.1 + (a.0 * b.1 + a.1 * b.0))
}

fn dd_add(a: Dd, b: Dd) -> Dd {
    let s = two_sum(a.0, b.0);
    fast_two_sum(s.0, s.1 + (a.1 + b.1))
}

/// `exp(x)`, correctly rounded, for `|x|` below 700: `x = k ln 2 + r`,
/// with `ln 2` in three parts so that `r` is found to about 2^-140, then
/// `e^r` by its Taylor series in double-double arithmetic, then 2^k.
fn exp(x: f64) -> f64 {
    let t = x * f64::from_bits(INV_LN2);
    // Rounded half away from zero: any nearby `k` keeps `r` small.
    let k = if t >= 0.0 {
        (t + 0.5) as i32
    } else {
        (t - 0.5) as i32
    };
    let kf = f64::from(k);
    // Exact: `LN2_HI` has 21 significant bits, and `x` is within a factor
    // of two of `k LN2_HI` when `k` is not 0 (Sterbenz).
    let t0 = x - kf * f64::from_bits(LN2_HI);
    let p = two_prod(kf, f64::from_bits(LN2_MID));
    let r = two_sum(t0, -p.0);
    let r = fast_two_sum(r.0, (r.1 - p.1) - kf * f64::from_bits(LN2_LO));
    let mut sum = Dd(0.0, 0.0);
    for &(hi, lo) in INV_FACT.iter().rev() {
        sum = dd_add(dd_mul(sum, r), Dd(f64::from_bits(hi), f64::from_bits(lo)));
    }
    // The nearest double to the sum, then times 2^k, which is exact.
    let y = sum.0 + sum.1;
    let scale = u64::try_from(k.wrapping_add(1023)).unwrap_or(0);
    y * f64::from_bits(scale << 52)
}

/// 1/n!, n = 0..=27, each as a double-double (high, low).
const INV_FACT: [(u64, u64); 28] = [
    (0x3ff0000000000000, 0x0000000000000000),
    (0x3ff0000000000000, 0x0000000000000000),
    (0x3fe0000000000000, 0x0000000000000000),
    (0x3fc5555555555555, 0x3c65555555555555),
    (0x3fa5555555555555, 0x3c45555555555555),
    (0x3f81111111111111, 0x3c01111111111111),
    (0x3f56c16c16c16c17, 0xbbef49f49f49f49f),
    (0x3f2a01a01a01a01a, 0x3b6a01a01a01a01a),
    (0x3efa01a01a01a01a, 0x3b3a01a01a01a01a),
    (0x3ec71de3a556c734, 0xbb6c154f8ddc6c00),
    (0x3e927e4fb7789f5c, 0x3b3cbbc05b4fa99a),
    (0x3e5ae64567f544e4, 0xbafc062e06d1f209),
    (0x3e21eed8eff8d898, 0xbac2aec959e14c06),
    (0x3de6124613a86d09, 0x3a8f28e0cc748ebe),
    (0x3da93974a8c07c9d, 0x3a305d6f8a2efd1f),
    (0x3d6ae7f3e733b81f, 0x39e1d8656b0ee8cb),
    (0x3d2ae7f3e733b81f, 0x39a1d8656b0ee8cb),
    (0x3ce952c77030ad4a, 0x398ac981465ddc6c),
    (0x3ca6827863b97d97, 0x394eec01221a8b0b),
    (0x3c62f49b46814157, 0x38f2650f61dbdcb4),
    (0x3c1e542ba4020225, 0x387ea72b4afe3c2f),
    (0x3bd71b8ef6dcf572, 0xb87d043ae40c4647),
    (0x3b90ce396db7f853, 0xb83aebcdbd20331c),
    (0x3b4761b41316381a, 0xb7d3423c7d91404f),
    (0x3aff2cf01972f578, 0xb789ada5fcc1ab14),
    (0x3ab3f3ccdd165fa9, 0xb7458ddadf344487),
    (0x3a688e85fc6a4e5a, 0xb7071c37ebd16540),
    (0x3a1d1ab1c2dccea3, 0x36a054d0c78aea14),
];
const LN2_HI: u64 = 0x3fe62e4200000000;
const LN2_MID: u64 = 0x3e9fdf473de6af28;
const LN2_LO: u64 = 0xbb3c4c67fc0d0951;
const INV_LN2: u64 = 0x3ff71547652b82fe;
/// glibc's `exp` where it is not correctly rounded: `LogL16toY`'s `Le`.
const GLIBC_L16: [(u16, u64); 21] = [
    (1446, 0x3c491d0dad829e66),
    (1731, 0x3c5b2a36f0cf3f2f),
    (3281, 0x3cbc36e26b34e08e),
    (4127, 0x3cf16cad3c92df6f),
    (7617, 0x3dcb04a868742eec),
    (10550, 0x3e828b4c0ea83f3a),
    (11288, 0x3eb118edb6db2dc4),
    (14113, 0x3f6184e5d23816d3),
    (15572, 0x3fbc71cb269e6007),
    (17888, 0x404d62461eec14c4),
    (18453, 0x4070f58503328e60),
    (19618, 0x40b8d7ccbc6c19e9),
    (22517, 0x416f1a61cbdf5bda),
    (27488, 0x42a4c70d0735379d),
    (29305, 0x43163b90532205dd),
    (29382, 0x431b62eeb6ddfc9f),
    (30029, 0x4343bc559212efa6),
    (30706, 0x436ed9f78d802dbd),
    (31690, 0x43abaf46ca7a677a),
    (31814, 0x43b35d7a577dd702),
    (32285, 0x43d15496238814b2),
];
/// The same for `LogL10toY`'s `p10`.
const GLIBC_L10: [(u16, u64); 0] = [];

/// `uv_row` (`uvcode.h`): each row of the (u', v') grid -- the u' it starts
/// at (C's `(float)` of a `double` literal, as bits), its squares, and the
/// squares in the rows before it.
const UV_ROW: [(u32, u16, u16); 163] = [
    (0x3e7d9b5f, 4, 0),
    (0x3e79a134, 6, 4),
    (0x3e777c03, 7, 10),
    (0x3e73953e, 9, 17),
    (0x3e719158, 10, 26),
    (0x3e6db984, 12, 36),
    (0x3e69d51b, 14, 48),
    (0x3e67b070, 15, 62),
    (0x3e63b539, 17, 77),
    (0x3e61b329, 18, 94),
    (0x3e5bdcf0, 21, 112),
    (0x3e59d1b3, 22, 133),
    (0x3e57c73f, 23, 155),
    (0x3e51e53b, 26, 178),
    (0x3e4fdb90, 27, 204),
    (0x3e4c06e2, 29, 231),
    (0x3e4837b5, 31, 260),
    (0x3e463498, 32, 291),
    (0x3e426f61, 34, 323),
    (0x3e3eaf68, 36, 357),
    (0x3e3eaf68, 36, 393),
    (0x3e3af966, 38, 429),
    (0x3e374c90, 40, 467),
    (0x3e33ad5c, 42, 507),
    (0x3e302108, 44, 549),
    (0x3e302108, 44, 593),
    (0x3e2ca8a0, 46, 637),
    (0x3e2ca8a0, 46, 683),
    (0x3e278034, 49, 729),
    (0x3e22730c, 52, 778),
    (0x3e22730c, 52, 830),
    (0x3e22730c, 52, 882),
    (0x3e1d81ae, 55, 934),
    (0x3e1d81ae, 55, 989),
    (0x3e18ace2, 58, 1044),
    (0x3e18ace2, 58, 1102),
    (0x3e122c02, 62, 1160),
    (0x3e122c02, 62, 1222),
    (0x3e122c02, 62, 1284),
    (0x3e0d96a7, 65, 1346),
    (0x3e0d96a7, 65, 1411),
    (0x3e0d96a7, 65, 1476),
    (0x3e075686, 69, 1541),
    (0x3e075686, 69, 1610),
    (0x3e013b9f, 73, 1679),
    (0x3e013b9f, 73, 1752),
    (0x3e013b9f, 73, 1825),
    (0x3df68a50, 77, 1898),
    (0x3df68a50, 77, 1975),
    (0x3df68a50, 77, 2052),
    (0x3df68a50, 77, 2129),
    (0x3de7589f, 82, 2206),
    (0x3de7589f, 82, 2288),
    (0x3de7589f, 82, 2370),
    (0x3ddc0ebf, 86, 2452),
    (0x3ddc0ebf, 86, 2538),
    (0x3ddc0ebf, 86, 2624),
    (0x3ddc0ebf, 86, 2710),
    (0x3dcd80a1, 91, 2796),
    (0x3dcd80a1, 91, 2887),
    (0x3dcd80a1, 91, 2978),
    (0x3dc2d16c, 95, 3069),
    (0x3dc2d16c, 95, 3164),
    (0x3dc2d16c, 95, 3259),
    (0x3dc2d16c, 95, 3354),
    (0x3db4ca0c, 100, 3449),
    (0x3db4ca0c, 100, 3549),
    (0x3db4ca0c, 100, 3649),
    (0x3db4ca0c, 100, 3749),
    (0x3da6f588, 105, 3849),
    (0x3da6f588, 105, 3954),
    (0x3da6f588, 105, 4059),
    (0x3da6f588, 105, 4164),
    (0x3d9950b9, 110, 4269),
    (0x3d9950b9, 110, 4379),
    (0x3d9950b9, 110, 4489),
    (0x3d9950b9, 110, 4599),
    (0x3d8bdba1, 115, 4709),
    (0x3d8bdba1, 115, 4824),
    (0x3d8bdba1, 115, 4939),
    (0x3d8bdba1, 115, 5054),
    (0x3d823290, 119, 5169),
    (0x3d823290, 119, 5288),
    (0x3d823290, 119, 5407),
    (0x3d823290, 119, 5526),
    (0x3d6a5e78, 124, 5645),
    (0x3d6a5e78, 124, 5769),
    (0x3d6a5e78, 124, 5893),
    (0x3d6a5e78, 124, 6017),
    (0x3d50d5a6, 129, 6141),
    (0x3d50d5a6, 129, 6270),
    (0x3d50d5a6, 129, 6399),
    (0x3d50d5a6, 129, 6528),
    (0x3d50d5a6, 129, 6657),
    (0x3d37be12, 134, 6786),
    (0x3d37be12, 134, 6920),
    (0x3d37be12, 134, 7054),
    (0x3d37be12, 134, 7188),
    (0x3d262dc7, 138, 7322),
    (0x3d262dc7, 138, 7460),
    (0x3d262dc7, 138, 7598),
    (0x3d262dc7, 138, 7736),
    (0x3d14d834, 142, 7874),
    (0x3d14d834, 142, 8016),
    (0x3d14d834, 142, 8158),
    (0x3d14d834, 142, 8300),
    (0x3d03a42f, 146, 8442),
    (0x3d03a42f, 146, 8588),
    (0x3d03a42f, 146, 8734),
    (0x3d03a42f, 146, 8880),
    (0x3ce4f11b, 150, 9026),
    (0x3ce4f11b, 150, 9176),
    (0x3ce4f11b, 150, 9326),
    (0x3cc2784b, 154, 9476),
    (0x3cc2784b, 154, 9630),
    (0x3cc2784b, 154, 9784),
    (0x3cc2784b, 154, 9938),
    (0x3c9fc6da, 158, 10092),
    (0x3c9fc6da, 158, 10250),
    (0x3c9fc6da, 158, 10408),
    (0x3c8b1141, 161, 10566),
    (0x3c8b1141, 161, 10727),
    (0x3c8b1141, 161, 10888),
    (0x3c8b1141, 161, 11049),
    (0x3c4f13cf, 165, 11210),
    (0x3c4f13cf, 165, 11375),
    (0x3c4f13cf, 165, 11540),
    (0x3c23b14b, 168, 11705),
    (0x3c23b14b, 168, 11873),
    (0x3c23b14b, 168, 12041),
    (0x3c13b7d8, 170, 12209),
    (0x3c13b7d8, 170, 12379),
    (0x3c13b7d8, 170, 12549),
    (0x3bcbb7fa, 173, 12719),
    (0x3bcbb7fa, 173, 12892),
    (0x3ba704bc, 175, 13065),
    (0x3ba704bc, 175, 13240),
    (0x3ba704bc, 175, 13415),
    (0x3b801712, 177, 13590),
    (0x3b801712, 177, 13767),
    (0x3b195aaf, 177, 13944),
    (0x3b1c90c5, 170, 14121),
    (0x3a8bfc22, 164, 14291),
    (0x3ad8a97a, 157, 14455),
    (0x3a3bf50e, 150, 14612),
    (0x3ad38cda, 143, 14762),
    (0x398d8ec9, 136, 14905),
    (0x39fdc161, 129, 15041),
    (0x3a90928a, 123, 15170),
    (0x3aa2ca9b, 115, 15293),
    (0x3a9bb6aa, 109, 15408),
    (0x3a848388, 103, 15517),
    (0x3a39dc2f, 97, 15620),
    (0x399dcf89, 89, 15717),
    (0x3b1e55c1, 82, 15806),
    (0x3b550ebb, 76, 15888),
    (0x3b54bad8, 69, 15964),
    (0x3b87b13a, 62, 16033),
    (0x3bc36545, 55, 16095),
    (0x3c10d174, 47, 16150),
    (0x3c2bde40, 40, 16197),
    (0x3c8b3700, 31, 16237),
    (0x3cc1d085, 21, 16268),
];
/// `UV_SQSIZ`, `(float)0.003500`.
const UV_SQSIZ: u32 = 0x3b656042;
/// `UV_VSTART`, `(float)0.016940`.
const UV_VSTART: u32 = 0x3c8ac5c1;

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp,
        reason = "tests of exact arithmetic compare exact values"
    )]

    use super::*;

    #[test]
    fn every_luminance_is_glibcs() {
        // FNV-1a of glibc 2.39's `exp` over every argument the conversions
        // can pass it -- `Le` 1 to 32767, then `p10` 1 to 1023 -- as
        // `target/tiff_wip/luv/exps.c` printed them.
        let values = (1..0x8000)
            .map(glibc_exp16)
            .chain((1..0x400).map(glibc_exp10));
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for y in values {
            for byte in y.to_bits().to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        }
        assert_eq!(hash, 0xfc3a_5685_6803_05a1);
    }

    #[test]
    fn exp_is_correctly_rounded_where_glibc_is_not() {
        // glibc's answer for `Le` 1446 is one ulp above the true value's
        // rounding; the correctly rounded one is kept apart from it.
        let a = LN_2 / 256.0 * (1446.0 + 0.5) - LN_2 * 64.0;
        assert_eq!(exp(a).to_bits(), 0x3c49_1d0d_ad82_9e65);
        assert_eq!(glibc_exp16(1446).to_bits(), 0x3c49_1d0d_ad82_9e66);
        assert_eq!(exp(0.0), 1.0);
        assert_eq!(exp(LN_2), 2.0);
    }

    #[test]
    fn sqrt_is_ieees() {
        let cases: [(u64, u64); 36] = [
            (0x4010000000000000, 0x4000000000000000),
            (0x4000000000000000, 0x3ff6a09e667f3bcd),
            (0x3fd0000000000000, 0x3fe0000000000000),
            (0x3fe0000000000000, 0x3fe6a09e667f3bcd),
            (0x0000000000000001, 0x1e60000000000000),
            (0x000fffffffffffff, 0x1fffffffffffffff),
            (0x0010000000000000, 0x2000000000000000),
            (0x3fefffffffffffff, 0x3fefffffffffffff),
            (0x3fe0000000000001, 0x3fe6a09e667f3bcd),
            (0x01a56e1fc2f8f359, 0x20ca2fe76a3f9475),
            (0x4008000000000000, 0x3ffbb67ae8584caa),
            (0x3fb999999999999a, 0x3fd43d136248490f),
            (0x3fe3eecf89059360, 0x3fe94176cab16094),
            (0x3fe7bcb8116f23ee, 0x3feb8f874aaf33b1),
            (0x3fe97239c6c3047f, 0x3fec891a7da0cc56),
            (0x3fee288d7f5db50c, 0x3fef10c89dd9e083),
            (0x3fe7ad3fc69bae32, 0x3feb868aba51aebc),
            (0x3fed83afb61ec2c4, 0x3feebb69a76d655a),
            (0x3f9db38beb9086e0, 0x3fc5ccb13b9a729e),
            (0x3fddccc2f63529c2, 0x3fe5d5f023f52bd2),
            (0x3fee2ffa6cff07a8, 0x3fef149b6f5b44cc),
            (0x3fe4c46648351904, 0x3fe9c763c66cdb3c),
            (0x3fecd42d44a09da2, 0x3fee5f80231921da),
            (0x3fbcfb10ebe5bb28, 0x3fd58893783ae683),
            (0x37949c407814e8a3, 0x3bc228d1ec507db4),
            (0x185e56733f1f65a9, 0x2c2608286535e414),
            (0x0686bfa18b33e969, 0x233afb09f0c4c758),
            (0x0ff508d692edcf46, 0x27f25869c922a045),
            (0x2ecbb6c8035b739a, 0x375dc7ad44c79f33),
            (0x1a1f259b377b9aa3, 0x2d0652e221deb55f),
            (0x0ba720ac478c281e, 0x25cb34594501b0b5),
            (0x37847327ea959c22, 0x3bb994c54acda181),
            (0x18ec8a7cc4069546, 0x2c6e3895b606f860),
            (0x30c362dd28dbd25f, 0x3858e8304851915d),
            (0x049a8792cc11d358, 0x22449a4ddfcdd451),
            (0x278c1a47238642eb, 0x33bdfcf295dc204f),
        ];
        for (x, want) in cases {
            assert_eq!(sqrt(f64::from_bits(x)).to_bits(), want, "sqrt of {x:#x}");
        }
    }

    #[test]
    fn a_luminance_through_the_gamma() {
        let mut cache = Vec::new();
        // No luminance, and a negative one, are black; 2^0 and above white.
        assert_eq!(gamma8(log_l16_to_y(&mut cache, 0)), 0);
        assert_eq!(gamma8(log_l16_to_y(&mut cache, 0x8000 | 0x4000)), 0);
        assert_eq!(gamma8(log_l16_to_y(&mut cache, 0x4000)), 255);
        // Codes land half a step above their power of two: Le 0x3E00 is
        // 2^-1.998, whose root is just over a half, and 0x3DFF just under.
        assert_eq!(gamma8(log_l16_to_y(&mut cache, 0x3e00)), 128);
        assert_eq!(gamma8(log_l16_to_y(&mut cache, 0x3dff)), 127);
        assert_eq!(gamma8(f64::NAN), 0);
    }

    #[test]
    fn runs_and_literals_fill_each_plane() {
        // High bytes: a run of 3 of 0x12, then 1 literal 0x34; low bytes: 4
        // literals.
        let data = [129, 0x12, 1, 0x34, 4, 1, 2, 3, 4];
        let mut raw = &data[..];
        let mut values = [0u32; 4];
        assert!(planes(&mut raw, &mut values, &[8, 0]));
        assert_eq!(values, [0x1201, 0x1202, 0x1203, 0x3404]);
        assert!(raw.is_empty());
    }

    #[test]
    fn a_literal_longer_than_the_row_leaves_its_rest_as_codes() {
        // Two pixels; the high plane's literal claims three bytes, so its
        // third (130) is read as the low plane's first code: a run of 4.
        let data = [3, 0xA0, 0xB0, 130, 0x07];
        let mut raw = &data[..];
        let mut values = [0u32; 2];
        assert!(planes(&mut raw, &mut values, &[8, 0]));
        assert_eq!(values, [0xA007, 0xB007]);
    }

    #[test]
    fn a_plane_short_of_data_fails() {
        let mut raw = &[129u8, 0x12][..];
        let mut values = [0u32; 4];
        assert!(!planes(&mut raw, &mut values, &[8, 0]));
        // A run code with no byte after it stops the plane there.
        let mut raw = &[2u8, 1, 2, 200][..];
        let mut values = [0u32; 3];
        assert!(!planes(&mut raw, &mut values, &[0]));
        assert_eq!(raw, [200]);
    }

    #[test]
    fn three_bytes_a_pixel_and_no_fewer() {
        let mut raw = &[1u8, 2, 3, 4, 5, 6, 7][..];
        let mut values = [0u32; 2];
        assert!(triples(&mut raw, &mut values));
        assert_eq!(values, [0x010203, 0x040506]);
        let mut values = [0u32; 1];
        assert!(!triples(&mut raw, &mut values));
    }

    #[test]
    fn chroma_squares_from_the_first_to_the_last() {
        let sq = f64::from(f32::from_bits(UV_SQSIZ));
        let v0 = f64::from(f32::from_bits(UV_VSTART));
        let (u, v) = uv_decode(0).unwrap();
        assert_eq!(u, f64::from(f32::from_bits(UV_ROW[0].0)) + 0.5 * sq);
        assert_eq!(v, v0 + 0.5 * sq);
        // The last square: row 162's 21st.
        let (u, v) = uv_decode(UV_NDIVS - 1).unwrap();
        assert_eq!(u, f64::from(f32::from_bits(UV_ROW[162].0)) + 20.5 * sq);
        assert_eq!(v, v0 + 162.5 * sq);
        assert_eq!(uv_decode(UV_NDIVS), None);
    }

    #[test]
    fn a_neutral_grey_pixel_is_grey() {
        // `LogLuv32` at Le 2^-2 with the neutral point's chroma bytes.
        let mut cache = Vec::new();
        let ue = (U_NEU * UVSCALE) as u32;
        let ve = (V_NEU * UVSCALE) as u32;
        let [r, g, b] = xyz_to_rgb24(luv32_to_xyz(&mut cache, 0x3e00 << 16 | ue << 8 | ve));
        assert!(r.abs_diff(g) <= 2 && g.abs_diff(b) <= 2, "{r} {g} {b}");
    }
}
