//! `CFF ` — the PostScript-flavoured half of OpenType.
//!
//! An `.otf` in the colloquial sense (`OTTO` sfnt tag) does not store outlines
//! in `glyf`. It stores them in a `CFF ` table as **Type 2 charstrings**: not a
//! point list but a program for a small stack machine, with subroutine calls,
//! hint operators, and operand encodings that vary in width. This module reads
//! that table and runs those programs, producing the same [`Outline`] that
//! [`sfnt`](crate::sfnt) produces from `glyf`.
//!
//! # Shape of the table
//!
//! A CFF table is a header followed by four INDEXes (Name, Top DICT, String,
//! Global Subr) laid out back to back, and then a scattering of structures
//! that are found only by following offsets out of the Top DICT: the
//! CharStrings INDEX, the Private DICT (which itself points at the local Subr
//! INDEX), the charset, and — for CID-keyed fonts — an FDArray of several
//! Private DICTs with an FDSelect saying which glyph uses which.
//!
//! An **INDEX** is a count, an offset size, `count + 1` offsets of that size,
//! and the data those offsets slice. The offsets are 1-based from the byte
//! *before* the data, which is the one detail that makes an off-by-one here
//! silent rather than loud.
//!
//! A **DICT** is operands-then-operator, the reverse of a PostScript dict, with
//! integers in five different encodings and reals in packed BCD.
//!
//! # What is supported
//!
//! * All the path operators: the `moveto`/`lineto`/`curveto` families
//!   including the alternating `hlineto`/`vlineto`/`hvcurveto`/`vhcurveto`
//!   forms, the four flex operators, and `endchar`.
//! * Local and global subroutines, with the count-dependent bias.
//! * Hint operators, to the extent outlines need them: the stem counters are
//!   tracked solely so that `hintmask` skips the right number of mask bytes.
//!   The hints themselves are discarded — see the hinting note in
//!   [`sfnt`](crate::sfnt).
//! * `seac`-style accented composition through the deprecated four-argument
//!   `endchar`, resolved through StandardEncoding and the charset.
//! * CID-keyed fonts: FDSelect formats 0 and 3 pick the Private DICT, and so
//!   the local subroutines, per glyph.
//! * A non-default `FontMatrix`, scaled into the `head` table's units per em so
//!   that a caller cannot tell a CFF face from a TrueType one.
//! * **`CFF2`**, the variable-font revision ([`Cff::parse2`]): the same
//!   charstrings in a leaner container -- a header giving the Top DICT's
//!   length, INDEXes counted in 32 bits, always an FDArray (FDSelect optional,
//!   and with a format 4 of 32-bit ranges), no charset, no widths, no
//!   `endchar` or `return` -- plus an item variation store and two operators,
//!   `vsindex` and `blend`, by which a glyph varies.
//!
//! # `blend`, twice
//!
//! A `blend` turns `n` values into their defaults plus one delta per region
//! of the variation subtable in force, each weighed at the instance -- and the
//! two libraries this crate follows weigh them with different arithmetic, so
//! a glyph is blended as the reader of it needs (`Instance`, design-decisions
//! §1328): as HarfBuzz does for the outline drawn and the box measured (`f32`
//! region scalars at its `F2Dot14` coordinates, the weighed deltas summed in
//! `f64`), and as FreeType does for the points the auto-hinter is given (a
//! blend vector of `FT_DivFix`ed factors multiplied by `FT_MulFix`, at its
//! 16.16 coordinates, each delta `FT_MulFix`ed into a 32-bit 16.16 sum).
//! Both are checked against the libraries on real `CFF2` fonts
//! (`tools/outline_oracle.py`, `tools/hint_oracle.py --var`).
//!
//! # What is not
//! * **The Type 2 arithmetic and storage operators** (`add`, `div`, `random`,
//!   `put`/`get`, the conditionals). No shipping font uses them — they exist
//!   for procedural outlines that no design tool emits — and guessing at them
//!   would risk drawing a wrong glyph rather than reporting a missing feature.
//!
//! # Robustness
//!
//! As in [`sfnt`](crate::sfnt), the input is untrusted. Every offset is
//! bounds-checked against the table it indexes, subroutine recursion is
//! depth-limited, and the number of path commands one glyph may emit is
//! capped — a charstring that calls a subroutine which draws in a loop is
//! otherwise a denial of service in a font file.

extern crate alloc;

use alloc::vec::Vec;

use crate::ftcalc::{div_fix, f2dot14_to_fixed, mul_fix};
use crate::sfnt::{
    CffPen, CffPoints, Exact, Outline, PathCmd, SfntError, TaggedOutline, Transform,
};
use crate::varstore::VarStore;

/// Every structural complaint about this table reads the same way.
const ERR: SfntError = SfntError::MalformedTable("CFF ");

/// How deep `callsubr`/`callgsubr` may nest. The Type 2 specification sets
/// the limit at 10; a file exceeding it is malformed, not merely unusual.
const MAX_SUBR_DEPTH: u8 = 10;

/// The Type 2 operand stack is 48 entries in the specification.
const STACK_LIMIT: usize = 48;

/// CFF2's operand stack, in charstrings and DICTs alike: 513 entries, the
/// most a `maxstack` may declare -- enough for a `blend` over many regions.
const CFF2_STACK_LIMIT: usize = 513;

/// Ceiling on the drawing operations one charstring may perform -- per
/// charstring, so a `seac` glyph, which runs three, may draw three times it.
///
/// A charstring is a program, so "how much can one glyph draw" is not bounded
/// by the file's size the way a `glyf` entry is: a short subroutine invoked
/// from a short charstring can emit unboundedly many segments. The most
/// elaborate real glyphs (CJK ideographs, script capitals) run to a few
/// thousand commands, so this is orders of magnitude above legitimate use.
const MAX_COMMANDS: usize = 65_536;

// ---------------------------------------------------------------------------
// Bounds-checked primitive reads
// ---------------------------------------------------------------------------

fn add(a: usize, b: usize) -> Result<usize, SfntError> {
    a.checked_add(b).ok_or(SfntError::TooShort)
}

fn mul(a: usize, b: usize) -> Result<usize, SfntError> {
    a.checked_mul(b).ok_or(SfntError::TooShort)
}

fn u8_at(d: &[u8], off: usize) -> Result<u8, SfntError> {
    d.get(off).copied().ok_or(ERR)
}

fn u16_at(d: &[u8], off: usize) -> Result<u16, SfntError> {
    let b: [u8; 2] = d
        .get(off..add(off, 2)?)
        .ok_or(ERR)?
        .try_into()
        .map_err(|_| ERR)?;
    Ok(u16::from_be_bytes(b))
}

/// Read an `n`-byte big-endian unsigned integer, `n` in `1..=4`.
///
/// CFF stores offsets at whatever width the font needs, declared per INDEX,
/// so this width is data rather than a constant.
fn uint_at(d: &[u8], off: usize, n: usize) -> Result<u32, SfntError> {
    if !(1..=4).contains(&n) {
        return Err(ERR);
    }
    let mut v: u32 = 0;
    for i in 0..n {
        v = (v << 8) | u32::from(u8_at(d, add(off, i)?)?);
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// INDEX
// ---------------------------------------------------------------------------

/// A CFF INDEX: a count-prefixed array of variable-length byte strings.
#[derive(Clone, Copy, Debug, Default)]
struct Index {
    count: usize,
    off_size: usize,
    /// Offset of the offset array.
    offsets: usize,
    /// The offsets are 1-based from here, not from the start of the data.
    origin: usize,
    /// First byte after the whole INDEX, so the next one can be found.
    end: usize,
}

impl Index {
    /// A CFF INDEX, whose count is 16 bits.
    fn parse(d: &[u8], at: usize) -> Result<Self, SfntError> {
        Self::parse_counted(d, at, 2)
    }

    /// A CFF2 INDEX: the same but for a 32-bit count.
    fn parse2(d: &[u8], at: usize) -> Result<Self, SfntError> {
        Self::parse_counted(d, at, 4)
    }

    /// An INDEX whose count is `count_size` bytes.
    fn parse_counted(d: &[u8], at: usize, count_size: usize) -> Result<Self, SfntError> {
        let count = usize::try_from(uint_at(d, at, count_size)?).map_err(|_| ERR)?;
        if count == 0 {
            // An empty INDEX is exactly its count and nothing else — in
            // particular it has no offSize byte to read.
            return Ok(Self {
                end: add(at, count_size)?,
                ..Self::default()
            });
        }
        let off_size = usize::from(u8_at(d, add(at, count_size)?)?);
        if !(1..=4).contains(&off_size) {
            return Err(ERR);
        }
        let offsets = add(at, add(count_size, 1)?)?;
        // `count + 1` offsets, then the data they slice.
        let origin = add(offsets, mul(add(count, 1)?, off_size)?)?
            .checked_sub(1)
            .ok_or(ERR)?;
        let last = uint_at(d, add(offsets, mul(count, off_size)?)?, off_size)?;
        let end = add(origin, usize::try_from(last).map_err(|_| ERR)?)?;
        if end > d.len() {
            return Err(ERR);
        }
        Ok(Self {
            count,
            off_size,
            offsets,
            origin,
            end,
        })
    }

    fn get<'a>(&self, d: &'a [u8], i: usize) -> Result<&'a [u8], SfntError> {
        if i >= self.count {
            return Err(ERR);
        }
        let a = uint_at(d, add(self.offsets, mul(i, self.off_size)?)?, self.off_size)?;
        let b = uint_at(
            d,
            add(self.offsets, mul(add(i, 1)?, self.off_size)?)?,
            self.off_size,
        )?;
        // Offsets are 1-based; a zero offset is malformed, and a decreasing
        // pair would slice backwards.
        if a < 1 || b < a {
            return Err(ERR);
        }
        let start = add(self.origin, usize::try_from(a).map_err(|_| ERR)?)?;
        let stop = add(self.origin, usize::try_from(b).map_err(|_| ERR)?)?;
        d.get(start..stop).ok_or(ERR)
    }

    /// The subroutine number bias: Type 2 numbers subroutines from the middle
    /// of the INDEX outwards so that the common small ones encode in one byte.
    fn bias(self) -> i32 {
        if self.count < 1240 {
            107
        } else if self.count < 33900 {
            1131
        } else {
            32768
        }
    }
}

// ---------------------------------------------------------------------------
// DICT
// ---------------------------------------------------------------------------

/// Two-byte DICT and charstring operators are `12 <b>`; folding them into a
/// single number keeps every `match` on operators flat.
const fn esc(b: u8) -> u16 {
    // Saturation can never trigger — the largest escape is 1200 + 255 — but
    // saying so in the arithmetic keeps the function free of a panic path.
    1200_u16.saturating_add(b as u16)
}

/// Walk a DICT, calling `f` with each operator and its operands.
///
/// Operands accumulate until an operator ends the entry, which is why the
/// callback shape (rather than a returned map) is the natural one: a DICT is
/// a stream, and the operand list is only meaningful at the operator.
fn parse_dict(
    d: &[u8],
    f: impl FnMut(u16, &[f64]) -> Result<(), SfntError>,
) -> Result<(), SfntError> {
    parse_dict_as(false, d, f)
}

/// [`parse_dict`] for a CFF2 DICT (`cff2`), which adds four operators --
/// `vsindex` (22), `blend` (23), `vstore` (24) and `maxstack` (25) -- and a
/// deeper operand stack, room for a `blend`'s deltas.
///
/// A `blend` is handed to `f` like any other operator, ending its entry: the
/// values it blends are ones this module never reads (a Private DICT's
/// hinting zones), so the operator that follows sees no operands rather
/// than blended ones.
fn parse_dict_as(
    cff2: bool,
    d: &[u8],
    mut f: impl FnMut(u16, &[f64]) -> Result<(), SfntError>,
) -> Result<(), SfntError> {
    let limit = if cff2 { CFF2_STACK_LIMIT } else { STACK_LIMIT };
    let mut operands: [f64; CFF2_STACK_LIMIT] = [0.0; CFF2_STACK_LIMIT];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < d.len() {
        let b0 = u8_at(d, i)?;
        match b0 {
            // Operators.
            0..=25 if b0 <= 21 || cff2 => {
                let op = if b0 == 12 {
                    i = add(i, 1)?;
                    esc(u8_at(d, i)?)
                } else {
                    u16::from(b0)
                };
                i = add(i, 1)?;
                f(op, operands.get(..n).ok_or(ERR)?)?;
                n = 0;
            }
            // Operands.
            28 | 29 | 30 | 32..=254 => {
                let (v, len) = dict_operand(d, i)?;
                if n >= limit {
                    return Err(ERR);
                }
                *operands.get_mut(n).ok_or(ERR)? = v;
                n = add(n, 1)?;
                i = add(i, len)?;
            }
            // 22..=27 (in a CFF DICT; 26 and 27 in a CFF2 one), 31 and 255
            // are reserved.
            _ => return Err(ERR),
        }
    }
    Ok(())
}

/// One DICT operand, returning its value and its encoded length.
fn dict_operand(d: &[u8], at: usize) -> Result<(f64, usize), SfntError> {
    let b0 = u8_at(d, at)?;
    match b0 {
        28 => Ok((
            f64::from(i16::from_be_bytes([
                u8_at(d, add(at, 1)?)?,
                u8_at(d, add(at, 2)?)?,
            ])),
            3,
        )),
        29 => {
            let v = i32::from_be_bytes([
                u8_at(d, add(at, 1)?)?,
                u8_at(d, add(at, 2)?)?,
                u8_at(d, add(at, 3)?)?,
                u8_at(d, add(at, 4)?)?,
            ]);
            Ok((f64::from(v), 5))
        }
        30 => real_operand(d, at),
        // The match arms bound `b0`, so none of the three encodings below can
        // leave the range the format defines (-1131..=1131 for the two-byte
        // forms); the saturating spellings only make that visible to the
        // compiler.
        32..=246 => Ok((f64::from(i32::from(b0).saturating_sub(139)), 1)),
        247..=250 => {
            let b1 = i32::from(u8_at(d, add(at, 1)?)?);
            let v = i32::from(b0)
                .saturating_sub(247)
                .saturating_mul(256)
                .saturating_add(b1)
                .saturating_add(108);
            Ok((f64::from(v), 2))
        }
        251..=254 => {
            let b1 = i32::from(u8_at(d, add(at, 1)?)?);
            let v = i32::from(b0)
                .saturating_sub(251)
                .saturating_mul(-256)
                .saturating_sub(b1)
                .saturating_sub(108);
            Ok((f64::from(v), 2))
        }
        _ => Err(ERR),
    }
}

/// `10^n` for a small `n`, by repeated multiplication.
///
/// `f64::powi` lives in `std`, and this crate is written to be `no_std`-ready
/// (see the crate docs). The exponents a real DICT operand can carry are
/// single digits in practice, so a loop costs nothing and keeps the
/// dependency out.
fn pow10(n: i32) -> f64 {
    let mut v = 1.0_f64;
    for _ in 0..n.abs().min(60) {
        v *= 10.0;
    }
    if n < 0 { 1.0 / v } else { v }
}

/// A real number, stored as packed BCD nibbles terminated by `0xf`.
///
/// Only `FontMatrix` uses this in practice, but a font is free to write any
/// numeric operand this way.
fn real_operand(d: &[u8], at: usize) -> Result<(f64, usize), SfntError> {
    let mut mantissa = 0.0_f64;
    let mut frac_digits = 0i32;
    let mut exponent = 0i32;
    let mut exp_sign = 1i32;
    let mut in_exponent = false;
    let mut in_fraction = false;
    let mut negative = false;
    let mut i = add(at, 1)?;
    // A real is at most a couple of dozen nibbles; the cap stops a file that
    // simply omits the terminator from running to the end of the table.
    for _ in 0..64 {
        let byte = u8_at(d, i)?;
        i = add(i, 1)?;
        for nibble in [byte >> 4, byte & 0x0f] {
            match nibble {
                0..=9 => {
                    let digit = f64::from(u32::from(nibble));
                    if in_exponent {
                        exponent = exponent
                            .saturating_mul(10)
                            .saturating_add(i32::from(nibble));
                    } else {
                        mantissa = mantissa * 10.0 + digit;
                        if in_fraction {
                            frac_digits = frac_digits.saturating_add(1);
                        }
                    }
                }
                0x0a => in_fraction = true,
                0x0b => in_exponent = true,
                0x0c => {
                    in_exponent = true;
                    exp_sign = -1;
                }
                0x0e => negative = true,
                0x0f => {
                    let sign = if negative { -1.0 } else { 1.0 };
                    let scale = pow10(
                        exp_sign
                            .saturating_mul(exponent)
                            .saturating_sub(frac_digits),
                    );
                    return Ok((sign * mantissa * scale, i.checked_sub(at).ok_or(ERR)?));
                }
                // 0x0d is reserved.
                _ => return Err(ERR),
            }
        }
    }
    Err(ERR)
}

// ---------------------------------------------------------------------------
// Charset — glyph id to SID, for `seac`
// ---------------------------------------------------------------------------

/// How a glyph id maps to a string id.
#[derive(Clone, Copy, Debug)]
enum Charset {
    /// A predefined charset. All three assign SID `n` to glyph `n` over the
    /// range that `seac` can name, so they need no table.
    Predefined,
    /// A charset stored in the file, at this offset from the table start.
    Custom(usize),
}

impl Charset {
    /// The glyph that carries `sid`, or `None` if the font has no such glyph.
    ///
    /// This is the reverse of the direction the table is stored in, so it is a
    /// scan. That is deliberate: the only caller is `seac`, which fires for a
    /// handful of accented glyphs in a handful of old fonts, and building a
    /// reverse map at parse time would cost every font a table that almost
    /// none of them use.
    fn gid_for_sid(self, d: &[u8], sid: u16, num_glyphs: usize) -> Result<Option<u16>, SfntError> {
        let off = match self {
            Self::Predefined => {
                return Ok(if usize::from(sid) < num_glyphs {
                    Some(sid)
                } else {
                    None
                });
            }
            Self::Custom(off) => off,
        };
        // Glyph 0 is `.notdef` and is never listed.
        if sid == 0 {
            return Ok(Some(0));
        }
        let format = u8_at(d, off)?;
        let mut gid = 1usize;
        match format {
            0 => {
                let mut at = add(off, 1)?;
                while gid < num_glyphs {
                    if u16_at(d, at)? == sid {
                        return Ok(Some(u16::try_from(gid).map_err(|_| ERR)?));
                    }
                    at = add(at, 2)?;
                    gid = add(gid, 1)?;
                }
            }
            1 | 2 => {
                // Ranges of consecutive SIDs: a first SID and a count of how
                // many follow it. Format 2 differs only in the width of that
                // count, which is why the two share this arm.
                let n_left_size = if format == 1 { 1 } else { 2 };
                let mut at = add(off, 1)?;
                while gid < num_glyphs {
                    let first = u16_at(d, at)?;
                    let n_left = uint_at(d, add(at, 2)?, n_left_size)?;
                    let span = usize::try_from(n_left).map_err(|_| ERR)?;
                    if sid >= first {
                        let delta = usize::from(sid.saturating_sub(first));
                        if delta <= span {
                            let g = add(gid, delta)?;
                            return Ok(if g < num_glyphs {
                                Some(u16::try_from(g).map_err(|_| ERR)?)
                            } else {
                                None
                            });
                        }
                    }
                    at = add(at, add(2, n_left_size)?)?;
                    gid = add(gid, add(span, 1)?)?;
                }
            }
            _ => return Err(ERR),
        }
        Ok(None)
    }
}

/// The codes in StandardEncoding above ASCII that name a glyph.
///
/// Their SIDs run consecutively from 96 (`exclamdown`) to 149 (`germandbls`),
/// so the SID is the index into this table plus 96 and no second table is
/// needed. Codes 32..=126 are handled arithmetically (SID = code - 31); every
/// code not covered either way is unassigned.
const STANDARD_HIGH_CODES: [u8; 54] = [
    161, 162, 163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173, 174, 175, 177, 178, 179, 180,
    182, 183, 184, 185, 186, 187, 188, 189, 191, 193, 194, 195, 196, 197, 198, 199, 200, 202, 203,
    205, 206, 207, 208, 225, 227, 232, 233, 234, 235, 241, 245, 248, 249, 250, 251,
];

/// The SID that StandardEncoding assigns to `code`, or `None` if unassigned.
fn standard_encoding_sid(code: u8) -> Option<u16> {
    if (32..=126).contains(&code) {
        // `space` is SID 1 at code 32, and the run is unbroken to `asciitilde`.
        return Some(u16::from(code).saturating_sub(31));
    }
    let idx = STANDARD_HIGH_CODES.iter().position(|c| *c == code)?;
    u16::try_from(idx).ok()?.checked_add(96)
}

// ---------------------------------------------------------------------------
// The table
// ---------------------------------------------------------------------------

/// Which Private DICT — and so which local subroutines — a glyph uses.
#[derive(Clone, Debug)]
enum Locals {
    /// The ordinary case: one Private DICT for the whole font.
    Single(Option<Index>),
    /// CID-keyed: FDSelect maps a glyph to one of `fds`.
    Cid {
        /// Offset of the FDSelect structure from the table start.
        fd_select: usize,
        fds: Vec<Option<Index>>,
    },
}

/// Where a CFF2 charstring's `blend` operators take their weights from --
/// which instance of a variable face, read the way which library reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Instance<'c> {
    /// The default instance: every blended value is its default.
    Default,
    /// HarfBuzz's reading (`cff2_cs_interp_env_t`), at its `F2Dot14`
    /// coordinates: each region's scalar as [`VarStore`] evaluates it, an
    /// `f32`, and a value's deltas weighed and summed in `f64` before its
    /// default is added -- what a glyph is drawn and measured from.
    HarfBuzz(&'c [i16]),
    /// FreeType's reading (`cf2_doBlend`), at its 16.16 coordinates: a blend
    /// vector of `FT_DivFix`ed axis factors multiplied by `FT_MulFix`
    /// (`cff_blend_build_vector`), and each delta weighed by `FT_MulFix` and
    /// added to its default in 32-bit 16.16 -- what the auto-hinter is given.
    FreeType(&'c [i32]),
}

/// What a `CFF2` table's charstrings blend with.
#[derive(Clone, Debug)]
struct Cff2 {
    /// Each Font DICT's `vsindex` (from its Private DICT; 0 without one), in
    /// FDArray order: which variation subtable a glyph's blends start from.
    fd_vsindex: Vec<u16>,
    /// The item variation store, as HarfBuzz reads it.
    store: Option<VarStore>,
    /// The same store as FreeType's CFF driver keeps it.
    ft_store: Option<CffVStore>,
}

/// An item variation store as FreeType's CFF driver loads one
/// (`cff_vstore_load`): only what a blend needs, since a `CFF2` store's
/// subtables have no delta rows -- the deltas are in the charstrings.
#[derive(Clone, Debug, Default)]
struct CffVStore {
    axis_count: usize,
    region_count: usize,
    /// Region `r`'s axis `a` at `r * axis_count + a`: start, peak and end,
    /// 16.16.
    regions: Vec<[i64; 3]>,
    /// Each subtable's region indices.
    data: Vec<Vec<u16>>,
}

impl CffVStore {
    /// The store at `at` -- past the length word `CFF2` puts in front of it
    /// -- as FreeType loads it. `None` where FreeType's load fails: a format
    /// other than 1, or any part not there.
    fn parse(d: &[u8], at: usize) -> Option<Self> {
        if u16_at(d, at).ok()? != 1 {
            return None;
        }
        let regions_at = add(at, off_usize(uint_at(d, add(at, 2).ok()?, 4).ok()?).ok()?).ok()?;
        let data_count = usize::from(u16_at(d, add(at, 6).ok()?).ok()?);
        let mut offsets = Vec::with_capacity(data_count);
        for i in 0..data_count {
            let rec = add(at, add(8, mul(i, 4).ok()?).ok()?).ok()?;
            offsets.push(off_usize(uint_at(d, rec, 4).ok()?).ok()?);
        }
        let axis_count = usize::from(u16_at(d, regions_at).ok()?);
        let region_count = usize::from(u16_at(d, add(regions_at, 2).ok()?).ok()?);
        let fields = mul(region_count, axis_count).ok()?;
        // Every record must be there before any is kept, which also bounds
        // the allocation by the table's length.
        if add(add(regions_at, 4).ok()?, mul(fields, 6).ok()?).ok()? > d.len() {
            return None;
        }
        let mut regions = Vec::with_capacity(fields);
        for k in 0..fields {
            let axis = add(add(regions_at, 4).ok()?, mul(k, 6).ok()?).ok()?;
            let coord = |o: usize| -> Option<i64> {
                let raw = u16_at(d, add(axis, o).ok()?).ok()?;
                Some(f2dot14_to_fixed(i16::from_be_bytes(raw.to_be_bytes())))
            };
            regions.push([coord(0)?, coord(2)?, coord(4)?]);
        }
        let mut data = Vec::with_capacity(data_count);
        for off in offsets {
            // Past `itemCount` and `wordDeltaCount`, which a CFF2 subtable
            // has no use for.
            let sub = add(add(at, off).ok()?, 4).ok()?;
            let count = usize::from(u16_at(d, sub).ok()?);
            let mut indices = Vec::with_capacity(count.min(d.len()));
            for i in 0..count {
                indices.push(u16_at(d, add(sub, add(2, mul(i, 2).ok()?).ok()?).ok()?).ok()?);
            }
            data.push(indices);
        }
        Some(Self {
            axis_count,
            region_count,
            regions,
            data,
        })
    }

    /// FreeType's blend vector for subtable `vsindex` at `ndv`, less its
    /// leading 1.0 for the default (`cff_blend_build_vector`): one weight per
    /// region the subtable names, 16.16.
    ///
    /// Each axis contributes a factor -- 1 where the region's range on it is
    /// malformed or it peaks at 0 or `ndv` is at the peak, 0 outside the
    /// range, `FT_DivFix` of the distances inside it -- and the factors are
    /// multiplied by `FT_MulFix`, carrying on through a 0. `None` where
    /// FreeType refuses the blend: a `vsindex` past the subtables, a region
    /// past the regions, or coordinates for another number of axes.
    fn weights(&self, vsindex: u16, ndv: &[i32]) -> Option<Vec<i64>> {
        if !ndv.is_empty() && ndv.len() != self.axis_count {
            return None;
        }
        let indices = self.data.get(usize::from(vsindex))?;
        indices
            .iter()
            .map(|&r| {
                let r = usize::from(r);
                if r >= self.region_count {
                    return None;
                }
                if ndv.is_empty() {
                    return Some(0);
                }
                let start = mul(r, self.axis_count).ok()?;
                let region = self.regions.get(start..add(start, self.axis_count).ok()?)?;
                let mut weight: i64 = 0x1_0000;
                for (&[s, p, e], &c) in region.iter().zip(ndv) {
                    let c = i64::from(c);
                    // FreeType's straddle test also asks for a non-zero
                    // peak, which the zero-peak test makes redundant.
                    let factor = if s > p || p > e || p == 0 || (s < 0 && e > 0) {
                        0x1_0000
                    } else if c < s || c > e {
                        0
                    } else if c == p {
                        0x1_0000
                    } else if c < p {
                        div_fix(c.saturating_sub(s), p.saturating_sub(s))
                    } else {
                        div_fix(e.saturating_sub(c), e.saturating_sub(p))
                    };
                    weight = mul_fix(weight, factor);
                }
                Some(weight)
            })
            .collect()
    }
}

/// A `CFF2` blend's weights for the `vsindex` in force, as an [`Instance`]
/// reads them.
#[derive(Clone, Debug)]
enum Weights {
    /// The default instance: this many regions' deltas, weighed at nothing.
    Default(usize),
    /// HarfBuzz's region scalars.
    HarfBuzz(Vec<f32>),
    /// FreeType's blend vector, 16.16.
    FreeType(Vec<i64>),
}

impl Weights {
    /// How many deltas each blended value carries.
    fn len(&self) -> usize {
        match self {
            Self::Default(k) => *k,
            Self::HarfBuzz(s) => s.len(),
            Self::FreeType(bv) => bv.len(),
        }
    }

    /// `default` with `deltas` blended in, as the reading this is does it.
    fn apply(&self, default: f64, deltas: &[f64]) -> f64 {
        match self {
            Self::Default(_) => default,
            // `blend_deltas` sums the weighed deltas first, from zero, and
            // adds the default after.
            Self::HarfBuzz(scalars) => {
                let sum = deltas
                    .iter()
                    .zip(scalars)
                    .fold(0.0_f64, |v, (&d, &s)| v + f64::from(s) * d);
                default + sum
            }
            // `cf2_doBlend`: the default, then each `FT_MulFix`ed delta added
            // in turn, in a 32-bit 16.16 accumulator that wraps.
            Self::FreeType(bv) => {
                let mut sum = fixed32(default);
                for (&d, &w) in deltas.iter().zip(bv) {
                    #[allow(
                        clippy::cast_possible_truncation,
                        reason = "FreeType's `CF2_Fixed` is 32 bits; `ADD_INT32` wraps"
                    )]
                    let term = mul_fix(w, i64::from(fixed32(d))) as i32;
                    sum = sum.wrapping_add(term);
                }
                f64::from(sum) / 65536.0
            }
        }
    }
}

/// `v`, a whole number of 65536ths, as FreeType's 32-bit 16.16 `CF2_Fixed`
/// holds it.
fn fixed32(v: f64) -> i32 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a charstring operand is 16.16 by format; a larger one wraps as \
                  FreeType's 32-bit arithmetic does, and `as` saturates the rest"
    )]
    {
        (v * 65536.0).round() as i64 as i32
    }
}

/// A parsed `CFF ` table.
///
/// Every offset inside a CFF table is measured from the table's own start, so
/// this type never sees the font file: it is handed the table's slice and all
/// of its stored offsets are relative to that slice's byte zero. That removes
/// a whole class of "which base is this relative to" mistake, at the cost of
/// the caller having to re-slice on each call — which is what
/// `table` is for.
#[derive(Clone, Debug)]
pub struct Cff {
    /// Where the `CFF ` table sits in the font file.
    base: usize,
    len: usize,
    char_strings: Index,
    global_subrs: Index,
    locals: Locals,
    charset: Charset,
    /// Set only when `FontMatrix` disagrees with the `head` table's units per
    /// em, which is rare enough that the common path should not pay for it.
    matrix: Option<Transform>,
    /// `Some` for a `CFF2` table: what its charstrings blend with.
    cff2: Option<Cff2>,
}

impl Cff {
    /// The table's bytes, out of the font file.
    fn table<'a>(&self, data: &'a [u8]) -> Result<&'a [u8], SfntError> {
        data.get(self.base..add(self.base, self.len)?).ok_or(ERR)
    }

    /// Parse the `CFF ` table occupying `base..base + len` of `data`.
    ///
    /// `units_per_em` comes from `head` and is used to reconcile a non-default
    /// `FontMatrix`, so that outlines leave here in the same units a `glyf`
    /// face would produce.
    ///
    /// # Errors
    ///
    /// [`SfntError::MalformedTable`] when the table is truncated or
    /// self-inconsistent, and [`SfntError::CffUnsupported`] for a construct
    /// this module deliberately does not guess at.
    pub fn parse(
        data: &[u8],
        base: usize,
        len: usize,
        units_per_em: u16,
    ) -> Result<Self, SfntError> {
        let d = data.get(base..add(base, len)?).ok_or(ERR)?;
        // Header: major, minor, hdrSize, offSize. Everything starts at hdrSize
        // rather than at 4, because a future minor revision may grow it.
        let major = u8_at(d, 0)?;
        if major != 1 {
            return Err(SfntError::CffUnsupported("CFF major version"));
        }
        let hdr_size = usize::from(u8_at(d, 2)?);

        let names = Index::parse(d, hdr_size)?;
        let top_dicts = Index::parse(d, names.end)?;
        let strings = Index::parse(d, top_dicts.end)?;
        let global_subrs = Index::parse(d, strings.end)?;
        // The String INDEX is only needed to resolve glyph names, which
        // nothing here does; it is parsed because the Global Subr INDEX is
        // found by walking past it.
        let _ = strings;

        let top = top_dicts.get(d, 0)?;

        let mut char_strings_off = None;
        let mut private = None;
        let mut charset_off = 0u32;
        let mut font_matrix: Option<[f64; 6]> = None;
        let mut charstring_type = 2.0_f64;
        let mut fd_array_off = None;
        let mut fd_select_off = None;
        let mut is_cid = false;
        parse_dict(top, |op, args| {
            match op {
                15 => charset_off = dict_u32(args.first())?,
                17 => char_strings_off = Some(dict_u32(args.first())?),
                18 => {
                    let size = dict_u32(args.first())?;
                    let off = dict_u32(args.get(1))?;
                    private = Some((off, size));
                }
                esc if esc == self::esc(6) => {
                    charstring_type = args.first().copied().unwrap_or(2.0);
                }
                esc if esc == self::esc(7) => {
                    let mut m = [0.0; 6];
                    for (slot, v) in m.iter_mut().zip(args.iter()) {
                        *slot = *v;
                    }
                    font_matrix = Some(m);
                }
                esc if esc == self::esc(30) => is_cid = true,
                esc if esc == self::esc(36) => fd_array_off = Some(dict_u32(args.first())?),
                esc if esc == self::esc(37) => fd_select_off = Some(dict_u32(args.first())?),
                _ => {}
            }
            Ok(())
        })?;

        // Type 1 charstrings in a CFF wrapper exist in theory. Their operator
        // set overlaps Type 2's but means different things, so running one as
        // the other would draw a plausible-looking wrong glyph.
        // DICT operands are floats even when the value is an integer, so this
        // compares within a tolerance rather than for equality: a font that
        // writes `2` as a real must not be mistaken for a Type 1 font.
        if (charstring_type - 2.0).abs() > 0.5 {
            return Err(SfntError::CffUnsupported("Type 1 charstrings"));
        }

        let char_strings = Index::parse(d, off_usize(char_strings_off.ok_or(ERR)?)?)?;
        if char_strings.count == 0 {
            return Err(ERR);
        }

        let locals = if is_cid {
            let fd_array = Index::parse(d, off_usize(fd_array_off.ok_or(ERR)?)?)?;
            let mut fds = Vec::with_capacity(fd_array.count);
            for i in 0..fd_array.count {
                fds.push(local_subrs_of(d, fd_array.get(d, i)?)?);
            }
            Locals::Cid {
                fd_select: off_usize(fd_select_off.ok_or(ERR)?)?,
                fds,
            }
        } else {
            let subrs = match private {
                Some((off, size)) => private_subrs(d, off, size)?,
                None => None,
            };
            Locals::Single(subrs)
        };

        let charset = match charset_off {
            // 0, 1 and 2 are the predefined charsets rather than offsets.
            0..=2 => Charset::Predefined,
            off => Charset::Custom(off_usize(off)?),
        };

        Ok(Self {
            base,
            len,
            char_strings,
            global_subrs,
            locals,
            charset,
            matrix: font_matrix.and_then(|m| em_transform(m, units_per_em)),
            cff2: None,
        })
    }

    /// Parse the `CFF2` table occupying `base..base + len` of `data` -- the
    /// variable-font revision of CFF.
    ///
    /// The same charstrings in a leaner container: a header giving the Top
    /// DICT's length rather than a Name, Top DICT and String INDEX; INDEXes
    /// counted in 32 bits; always an FDArray of Font DICTs, each with a
    /// Private DICT that may name a default `vsindex`; FDSelect optional (one
    /// Font DICT without it) and in a third format; no charset, no widths in
    /// the charstrings, no `endchar`; and an item variation store whose
    /// regions the charstrings' `blend` operators weigh their deltas by.
    ///
    /// # Errors
    ///
    /// As [`parse`](Self::parse).
    pub fn parse2(
        data: &[u8],
        base: usize,
        len: usize,
        units_per_em: u16,
    ) -> Result<Self, SfntError> {
        let d = data.get(base..add(base, len)?).ok_or(ERR)?;
        // Header: major, minor, headerSize, topDictLength.
        if u8_at(d, 0)? != 2 {
            return Err(SfntError::CffUnsupported("CFF2 major version"));
        }
        let hdr_size = usize::from(u8_at(d, 2)?);
        let top_len = usize::from(u16_at(d, 3)?);
        let top = d.get(hdr_size..add(hdr_size, top_len)?).ok_or(ERR)?;
        let global_subrs = Index::parse2(d, add(hdr_size, top_len)?)?;

        let mut char_strings_off = None;
        let mut font_matrix: Option<[f64; 6]> = None;
        let mut fd_array_off = None;
        let mut fd_select_off = None;
        let mut vstore_off = None;
        parse_dict_as(true, top, |op, args| {
            match op {
                17 => char_strings_off = Some(dict_u32(args.first())?),
                24 => vstore_off = Some(dict_u32(args.first())?),
                esc if esc == self::esc(7) => {
                    let mut m = [0.0; 6];
                    for (slot, v) in m.iter_mut().zip(args.iter()) {
                        *slot = *v;
                    }
                    font_matrix = Some(m);
                }
                esc if esc == self::esc(36) => fd_array_off = Some(dict_u32(args.first())?),
                esc if esc == self::esc(37) => fd_select_off = Some(dict_u32(args.first())?),
                _ => {}
            }
            Ok(())
        })?;

        let char_strings = Index::parse2(d, off_usize(char_strings_off.ok_or(ERR)?)?)?;
        if char_strings.count == 0 {
            return Err(ERR);
        }
        let fd_array = Index::parse2(d, off_usize(fd_array_off.ok_or(ERR)?)?)?;
        let mut fds = Vec::with_capacity(fd_array.count);
        let mut fd_vsindex = Vec::with_capacity(fd_array.count);
        for i in 0..fd_array.count {
            let (subrs, vsindex) = cff2_private_of(d, fd_array.get(d, i)?)?;
            fds.push(subrs);
            fd_vsindex.push(vsindex);
        }
        let locals = match fd_select_off {
            Some(off) => Locals::Cid {
                fd_select: off_usize(off)?,
                fds,
            },
            // One Font DICT, and no FDSelect to choose among them.
            None => Locals::Single(fds.first().copied().flatten()),
        };

        // The store is preceded by a length word, and its own offsets are
        // measured from itself. Read with the axis count its region list
        // declares: HarfBuzz evaluates regions over that many axes whatever
        // `fvar` says, and FreeType refuses a blend at coordinates for
        // another number.
        let (store, ft_store) = match vstore_off {
            Some(off) if off != 0 => {
                let at = add(off_usize(off)?, 2)?;
                let axes = uint_at(d, at.saturating_add(2), 4)
                    .ok()
                    .and_then(|rel| usize::try_from(rel).ok())
                    .and_then(|rel| u16_at(d, at.checked_add(rel)?).ok())
                    .map_or(0, usize::from);
                (VarStore::parse(d, at, axes), CffVStore::parse(d, at))
            }
            _ => (None, None),
        };

        Ok(Self {
            base,
            len,
            char_strings,
            global_subrs,
            locals,
            charset: Charset::Predefined,
            matrix: font_matrix.and_then(|m| em_transform(m, units_per_em)),
            cff2: Some(Cff2 {
                fd_vsindex,
                store,
                ft_store,
            }),
        })
    }

    /// Whether this is a `CFF2` table, whose glyphs vary with the instance.
    #[must_use]
    pub(crate) fn is_cff2(&self) -> bool {
        self.cff2.is_some()
    }

    /// How many glyphs the CharStrings INDEX holds.
    #[must_use]
    pub fn num_glyphs(&self) -> usize {
        self.char_strings.count
    }

    /// Extract a glyph's outline in font units.
    ///
    /// # Errors
    ///
    /// [`SfntError::GlyphOutOfRange`] for an unknown glyph id,
    /// [`SfntError::MalformedTable`] when the charstring or the structures it
    /// reaches are inconsistent, and [`SfntError::CffUnsupported`] when it
    /// uses an operator this module does not implement.
    pub fn outline(&self, data: &[u8], gid: u16) -> Result<Outline, SfntError> {
        self.outline_at(data, gid, Instance::Default)
    }

    /// [`outline`](Self::outline) at a variable face's instance -- which only
    /// a `CFF2` glyph's `blend`s read.
    ///
    /// # Errors
    ///
    /// As [`outline`](Self::outline).
    pub(crate) fn outline_at(
        &self,
        data: &[u8],
        gid: u16,
        instance: Instance<'_>,
    ) -> Result<Outline, SfntError> {
        let d = self.table(data)?;
        let mut out = Outline::default();
        self.draw(d, gid, &mut out, Exact::default(), false, instance)?;
        if let Some(t) = self.matrix {
            let mut scaled = Outline::default();
            scaled.commands.reserve(out.commands.len());
            for cmd in &out.commands {
                scaled.commands.push(match *cmd {
                    PathCmd::MoveTo(p) => PathCmd::MoveTo(t.apply(p)),
                    PathCmd::LineTo(p) => PathCmd::LineTo(t.apply(p)),
                    PathCmd::QuadTo(c, p) => PathCmd::QuadTo(t.apply(c), t.apply(p)),
                    PathCmd::CurveTo(a, b, p) => {
                        PathCmd::CurveTo(t.apply(a), t.apply(b), t.apply(p))
                    }
                    PathCmd::Close => PathCmd::Close,
                });
            }
            return Ok(scaled);
        }
        Ok(out)
    }

    /// The box HarfBuzz measures `gid` by, at `instance`: `[min_x, min_y,
    /// max_x, max_y]` in charstring units, or `None` for a glyph that draws
    /// nothing. See [`Bounds`].
    ///
    /// # Errors
    ///
    /// As [`outline`](Self::outline).
    pub(crate) fn bounds(
        &self,
        data: &[u8],
        gid: u16,
        instance: Instance<'_>,
    ) -> Result<Option<[f64; 4]>, SfntError> {
        let d = self.table(data)?;
        let mut bounds = Bounds::default();
        self.draw(d, gid, &mut bounds, Exact::default(), false, instance)?;
        Ok(bounds.finish())
    }

    /// A glyph's points as FreeType's CFF loader stores them, for the
    /// auto-hinter: exact, where [`outline`](Self::outline)'s are narrowed to
    /// `f32`, and without the points FreeType drops (see [`CffPoints`]).
    ///
    /// # Errors
    ///
    /// As [`outline`](Self::outline).
    pub(crate) fn tagged_outline(
        &self,
        data: &[u8],
        gid: u16,
        instance: Instance<'_>,
    ) -> Result<TaggedOutline, SfntError> {
        let d = self.table(data)?;
        let mut points = CffPoints::default();
        self.draw(d, gid, &mut points, Exact::default(), false, instance)?;
        let mut out = points.finish();
        if let Some(t) = self.matrix {
            out.transform(&t);
        }
        Ok(out)
    }

    /// Draw `gid` on `pen`, its pen starting at `origin`. `d` is the table
    /// slice; `component` says this glyph is itself part of a `seac` glyph.
    fn draw<P: CffPen>(
        &self,
        d: &[u8],
        gid: u16,
        pen: &mut P,
        origin: Exact,
        component: bool,
        instance: Instance<'_>,
    ) -> Result<(), SfntError> {
        if usize::from(gid) >= self.char_strings.count {
            return Err(SfntError::GlyphOutOfRange);
        }
        let local = self.local_subrs(d, gid)?;
        let blend = match &self.cff2 {
            Some(cff2) => Some(BlendState {
                cff2,
                instance,
                vsindex: cff2
                    .fd_vsindex
                    .get(self.fd_index(d, gid)?)
                    .copied()
                    .unwrap_or(0),
                weights: None,
                blended: false,
            }),
            None => None,
        };
        let mut interp = Interp::new(self, d, local, pen, origin, blend);
        interp.run(self.char_strings.get(d, usize::from(gid))?, 0)?;
        interp.close_contour();
        let Some([adx, ady, bchar, achar]) = interp.seac else {
            return Ok(());
        };
        // The deprecated four-argument `endchar`: a base glyph and an accent
        // from StandardEncoding, the accent's pen starting at (adx, ady) --
        // as FreeType draws it. That is not the same as drawing the accent at
        // the origin and moving it: where each point lands in device space,
        // and so which of its lines have no length there, depends on where
        // the accent is (see `CffPoints`). A component composed in turn is
        // malformed, as FreeType has it, which also bounds the recursion.
        if component {
            return Err(ERR);
        }
        let base = self.seac_gid(d, bchar)?;
        let accent = self.seac_gid(d, achar)?;
        self.draw(d, base, pen, Exact::default(), true, instance)?;
        self.draw(d, accent, pen, Exact::new(adx, ady), true, instance)
    }

    /// Resolve a `seac` StandardEncoding code to a glyph id.
    fn seac_gid(&self, d: &[u8], code: f64) -> Result<u16, SfntError> {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        // A charstring operand is an integer in this position; anything
        // outside a byte is not a StandardEncoding code and is rejected below.
        let code = {
            let c = code.round();
            if !(0.0..=255.0).contains(&c) {
                return Err(ERR);
            }
            c as u8
        };
        let sid = standard_encoding_sid(code).ok_or(ERR)?;
        self.charset
            .gid_for_sid(d, sid, self.char_strings.count)?
            .ok_or(ERR)
    }

    /// The local subroutine INDEX in force for `gid`.
    fn local_subrs(&self, d: &[u8], gid: u16) -> Result<Option<Index>, SfntError> {
        match &self.locals {
            Locals::Single(idx) => Ok(*idx),
            Locals::Cid { fd_select, fds } => {
                let fd = Self::fd_for_gid(d, *fd_select, gid)?;
                Ok(fds.get(usize::from(fd)).copied().flatten())
            }
        }
    }

    /// Which Font DICT `gid` uses: its FDArray index.
    fn fd_index(&self, d: &[u8], gid: u16) -> Result<usize, SfntError> {
        match &self.locals {
            Locals::Single(_) => Ok(0),
            Locals::Cid { fd_select, .. } => Ok(usize::from(Self::fd_for_gid(d, *fd_select, gid)?)),
        }
    }

    /// FDSelect: which entry of the FDArray glyph `gid` uses.
    fn fd_for_gid(d: &[u8], at: usize, gid: u16) -> Result<u16, SfntError> {
        match u8_at(d, at)? {
            // Format 0: one byte per glyph, in glyph order.
            0 => u8_at(d, add(at, add(1, usize::from(gid))?)?).map(u16::from),
            // Format 4 (`CFF2`): as format 3, with 32-bit glyph ids and
            // 16-bit Font DICT numbers.
            4 => {
                let n_ranges = off_usize(uint_at(d, add(at, 1)?, 4)?)?;
                let sentinel = uint_at(d, add(at, add(5, mul(n_ranges, 6)?)?)?, 4)?;
                let gid = u32::from(gid);
                if gid >= sentinel {
                    return Err(ERR);
                }
                for i in 0..n_ranges {
                    let rec = add(at, add(5, mul(i, 6)?)?)?;
                    let first = uint_at(d, rec, 4)?;
                    let next = uint_at(d, add(rec, 6)?, 4)?;
                    if gid >= first && gid < next {
                        return u16_at(d, add(rec, 4)?);
                    }
                }
                Err(ERR)
            }
            // Format 3: ranges. A binary search would be possible but the
            // array is short (one entry per *font*, not per glyph) and this
            // runs once per glyph outline, not per pixel.
            3 => {
                let n_ranges = usize::from(u16_at(d, add(at, 1)?)?);
                let sentinel = u16_at(d, add(at, add(3, mul(n_ranges, 3)?)?)?)?;
                if gid >= sentinel {
                    return Err(ERR);
                }
                for i in 0..n_ranges {
                    let rec = add(at, add(3, mul(i, 3)?)?)?;
                    let first = u16_at(d, rec)?;
                    let next = u16_at(d, add(rec, 3)?)?;
                    if gid >= first && gid < next {
                        return u8_at(d, add(rec, 2)?).map(u16::from);
                    }
                }
                Err(ERR)
            }
            _ => Err(ERR),
        }
    }
}

/// Coerce a DICT operand that is meant to be an offset.
fn dict_u32(v: Option<&f64>) -> Result<u32, SfntError> {
    let v = *v.ok_or(ERR)?;
    if !(0.0..=f64::from(u32::MAX)).contains(&v) {
        return Err(ERR);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // Range-checked immediately above; DICT offsets are integers by format.
    Ok(v as u32)
}

fn off_usize(v: u32) -> Result<usize, SfntError> {
    usize::try_from(v).map_err(|_| SfntError::TooShort)
}

/// The local Subr INDEX named by a Private DICT at `off`, length `size`.
fn private_subrs(d: &[u8], off: u32, size: u32) -> Result<Option<Index>, SfntError> {
    let off = off_usize(off)?;
    let size = off_usize(size)?;
    let dict = d.get(off..add(off, size)?).ok_or(ERR)?;
    let mut subrs_rel = None;
    parse_dict(dict, |op, args| {
        if op == 19 {
            subrs_rel = Some(dict_u32(args.first())?);
        }
        Ok(())
    })?;
    match subrs_rel {
        // The Subrs offset is relative to the Private DICT, not the table —
        // the one offset in CFF that is not measured from the table start.
        Some(rel) => Ok(Some(Index::parse(d, add(off, off_usize(rel)?)?)?)),
        None => Ok(None),
    }
}

/// The local subroutines of one FDArray entry (a Font DICT).
fn local_subrs_of(d: &[u8], font_dict: &[u8]) -> Result<Option<Index>, SfntError> {
    let mut private = None;
    parse_dict(font_dict, |op, args| {
        if op == 18 {
            private = Some((dict_u32(args.first())?, dict_u32(args.get(1))?));
        }
        Ok(())
    })?;
    match private {
        Some((size, off)) => private_subrs(d, off, size),
        None => Ok(None),
    }
}

/// A `CFF2` Font DICT's local subroutines and default `vsindex`, from its
/// Private DICT.
///
/// The Subrs INDEX is a `CFF2` one (32-bit count), and like CFF's is placed
/// relative to the Private DICT. A Font DICT with no Private DICT has
/// neither.
fn cff2_private_of(d: &[u8], font_dict: &[u8]) -> Result<(Option<Index>, u16), SfntError> {
    let mut private = None;
    parse_dict_as(true, font_dict, |op, args| {
        if op == 18 {
            private = Some((dict_u32(args.first())?, dict_u32(args.get(1))?));
        }
        Ok(())
    })?;
    let Some((size, off)) = private else {
        return Ok((None, 0));
    };
    let off = off_usize(off)?;
    let dict = d.get(off..add(off, off_usize(size)?)?).ok_or(ERR)?;
    let mut subrs_rel = None;
    let mut vsindex = 0u16;
    parse_dict_as(true, dict, |op, args| {
        match op {
            19 => subrs_rel = Some(dict_u32(args.first())?),
            22 => vsindex = u16::try_from(dict_u32(args.first())?).map_err(|_| ERR)?,
            _ => {}
        }
        Ok(())
    })?;
    let subrs = match subrs_rel {
        Some(rel) => Some(Index::parse2(d, add(off, off_usize(rel)?)?)?),
        None => None,
    };
    Ok((subrs, vsindex))
}

/// The transform that takes charstring units to `units_per_em` font units,
/// or `None` when that is the identity.
///
/// `FontMatrix` maps charstring units into the em square, where the em is 1.0;
/// `head`'s units per em says how many font units that square is. The product
/// is therefore the identity for the overwhelmingly common pairing of
/// `[0.001 0 0 0.001 0 0]` with 1000 units per em, and only fonts that depart
/// from it pay anything.
fn em_transform(m: [f64; 6], units_per_em: u16) -> Option<Transform> {
    let upem = f64::from(units_per_em);
    let scaled = [
        m[0] * upem,
        m[1] * upem,
        m[2] * upem,
        m[3] * upem,
        m[4] * upem,
        m[5] * upem,
    ];
    let identity = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    // A hair of slack: 0.001 * 1000 is not exactly 1.0 in binary floating
    // point, and rebuilding every outline through a transform to correct an
    // error of one part in 10^15 would be pure cost.
    if scaled
        .iter()
        .zip(identity.iter())
        .all(|(a, b)| (a - b).abs() < 1e-6)
    {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    // Outlines are f32 throughout; a font matrix has at most a few significant
    // digits, so the narrowing is exact for every value a real font carries.
    Some(Transform {
        a: scaled[0] as f32,
        b: scaled[1] as f32,
        c: scaled[2] as f32,
        d: scaled[3] as f32,
        e: scaled[4] as f32,
        f: scaled[5] as f32,
    })
}

// ---------------------------------------------------------------------------
// The Type 2 charstring interpreter
// ---------------------------------------------------------------------------

/// The box HarfBuzz measures a CFF or `CFF2` glyph by
/// (`cff1_extents_param_t`, `cff2_extents_param_t`): every point a line or
/// curve is drawn through, a curve's control points included, and a
/// contour's starting point once something is drawn from it -- in `f64`, as
/// HarfBuzz's charstring numbers are, and in charstring units, because
/// HarfBuzz reads no `FontMatrix`.
#[derive(Debug, Default)]
struct Bounds {
    current: Exact,
    /// Whether something has been drawn since the last `moveto`.
    open: bool,
    /// `[min_x, min_y, max_x, max_y]`, once any point is in.
    b: Option<[f64; 4]>,
}

impl Bounds {
    fn add(&mut self, p: Exact) {
        self.b = Some(match self.b {
            None => [p.x, p.y, p.x, p.y],
            Some([x0, y0, x1, y1]) => [x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)],
        });
    }

    /// Something is drawn: the contour's start counts, the first time.
    fn draw_from_current(&mut self) {
        if !self.open {
            self.open = true;
            self.add(self.current);
        }
    }

    fn finish(self) -> Option<[f64; 4]> {
        self.b
    }
}

impl CffPen for Bounds {
    fn start(&mut self, at: Exact) {
        self.current = at;
    }

    fn move_to(&mut self, p: Exact) {
        self.open = false;
        self.current = p;
    }

    fn line_to(&mut self, p: Exact) {
        self.draw_from_current();
        self.add(p);
        self.current = p;
    }

    fn curve_to(&mut self, c1: Exact, c2: Exact, p: Exact) {
        self.draw_from_current();
        self.add(c1);
        self.add(c2);
        self.add(p);
        self.current = p;
    }

    fn close(&mut self) {
        self.open = false;
    }
}

/// A `CFF2` glyph's blending: the instance, the `vsindex` in force, and --
/// once the first `blend` has asked -- that subtable's weights.
struct BlendState<'a> {
    cff2: &'a Cff2,
    instance: Instance<'a>,
    vsindex: u16,
    weights: Option<Weights>,
    /// Whether a `blend` has run, after which `vsindex` is malformed.
    blended: bool,
}

impl BlendState<'_> {
    /// The weights of the subtable in force, at the instance, as the
    /// instance's library computes them.
    fn weights(&self) -> Result<Weights, SfntError> {
        let store = self.cff2.store.as_ref();
        Ok(match self.instance {
            Instance::Default => {
                Weights::Default(store.map_or(0, |s| s.region_index_count(self.vsindex)))
            }
            Instance::HarfBuzz(coords) => Weights::HarfBuzz(
                store
                    .map(|s| s.region_scalars(self.vsindex, coords))
                    .unwrap_or_default(),
            ),
            // FreeType refuses a blend in a face with no store, as it does a
            // `vsindex` or region the store does not have.
            Instance::FreeType(ndv) => Weights::FreeType(
                self.cff2
                    .ft_store
                    .as_ref()
                    .and_then(|s| s.weights(self.vsindex, ndv))
                    .ok_or(ERR)?,
            ),
        })
    }
}

/// A charstring's arithmetic is done in `f64`, and only the pen narrows a
/// finished point to an outline's `f32` -- or, drawing the hinter's points,
/// does not (see [`CffPen`]).
///
/// Type 2 operands are integers or 16.16 fixed point, and a pen position is
/// their running sum -- all of which `f64` holds exactly, as FreeType's own
/// 16.16 arithmetic does. `f32` does not: a 16.16 value needs 32 bits and
/// `f32` has 24, so a font with fractional operands drifted as it drew, and a
/// contour that returns to its start in the font arrived a few millionths
/// away from it here. Harmless to a rasterizer; not to the auto-hinter, which
/// has to see the glyph's points exactly as FreeType's does (see
/// [`crate::hint`]).
struct Interp<'a, P: CffPen> {
    cff: &'a Cff,
    data: &'a [u8],
    local: Option<Index>,
    stack: [f64; CFF2_STACK_LIMIT],
    sp: usize,
    /// How deep the stack may go: 48 for CFF, 513 for `CFF2`.
    limit: usize,
    /// A `CFF2` glyph's blending; `None` for CFF, where `vsindex` and
    /// `blend` are not operators.
    blend: Option<BlendState<'a>>,
    /// Stem count, kept only so that `hintmask` skips the right number of
    /// mask bytes — one bit per stem, rounded up to a byte.
    n_stems: usize,
    x: f64,
    y: f64,
    /// Whether a contour is currently open, so that a `moveto` knows to close
    /// the previous one. CFF contours are implicitly closed; there is no
    /// `closepath` in Type 2.
    open: bool,
    pen: &'a mut P,
    /// Drawing operations so far, against [`MAX_COMMANDS`].
    drawn: usize,
    /// Set by a four-argument `endchar`; acted on by the caller, which is the
    /// only place that can recurse into another glyph.
    seac: Option<[f64; 4]>,
}

impl<'a, P: CffPen> Interp<'a, P> {
    /// An interpreter for one charstring, drawing on `pen` from `origin`,
    /// blending as `blend` says for a `CFF2` glyph.
    fn new(
        cff: &'a Cff,
        data: &'a [u8],
        local: Option<Index>,
        pen: &'a mut P,
        origin: Exact,
        blend: Option<BlendState<'a>>,
    ) -> Self {
        pen.start(origin);
        Self {
            cff,
            data,
            local,
            stack: [0.0; CFF2_STACK_LIMIT],
            sp: 0,
            limit: if blend.is_some() {
                CFF2_STACK_LIMIT
            } else {
                STACK_LIMIT
            },
            blend,
            n_stems: 0,
            x: origin.x,
            y: origin.y,
            open: false,
            pen,
            drawn: 0,
            seac: None,
        }
    }
}

impl<P: CffPen> Interp<'_, P> {
    fn push(&mut self, v: f64) -> Result<(), SfntError> {
        if self.sp >= self.limit {
            return Err(ERR);
        }
        *self.stack.get_mut(self.sp).ok_or(ERR)? = v;
        self.sp = self.sp.saturating_add(1);
        Ok(())
    }

    fn args(&self) -> Result<&[f64], SfntError> {
        self.stack.get(..self.sp).ok_or(ERR)
    }

    /// The last `N` operands.
    ///
    /// Reading fixed-arity operators from the *end* of the stack is what makes
    /// the width operand a non-issue. A charstring may prefix its first
    /// stack-clearing operator with one extra number, the glyph's advance
    /// width; advances come from `hmtx` here, so the width is not wanted, and
    /// taking arguments from the end drops it without a separate rule. The
    /// variable-arity operators need no such care: by the format, only the
    /// first stack-clearing operator can carry a width, and that is always one
    /// of the stem, `moveto` or `endchar` operators handled here.
    ///
    /// Returning an array rather than a slice lets each operator destructure
    /// its operands by name, so an arity mismatch is a compile error instead of
    /// an index that could be out of range at run time.
    fn last<const N: usize>(&self) -> Result<[f64; N], SfntError> {
        let start = self.sp.checked_sub(N).ok_or(ERR)?;
        let s = self.stack.get(start..self.sp).ok_or(ERR)?;
        s.try_into().map_err(|_| ERR)
    }

    /// `N` operands starting at position `k` from the *bottom* of the stack.
    ///
    /// The variable-arity operators walk their operands forwards, and each step
    /// both reads operands and emits a command — which needs `&mut self`.
    /// Copying the group out per step rather than holding a borrow of the stack
    /// is what keeps those loops borrow-checkable, and it bounds-checks the
    /// whole group in one place.
    fn run_of<const N: usize>(&self, k: usize) -> Result<[f64; N], SfntError> {
        let end = add(k, N)?;
        let s = self.args()?.get(k..end).ok_or(ERR)?;
        s.try_into().map_err(|_| ERR)
    }

    /// Count one drawing operation against the charstring's ceiling.
    fn spend(&mut self) -> Result<(), SfntError> {
        if self.drawn >= MAX_COMMANDS {
            return Err(ERR);
        }
        self.drawn = self.drawn.saturating_add(1);
        Ok(())
    }

    fn close_contour(&mut self) {
        if self.open {
            self.pen.close();
            self.open = false;
        }
    }

    fn move_to(&mut self, dx: f64, dy: f64) -> Result<(), SfntError> {
        self.spend()?;
        self.close_contour();
        self.x += dx;
        self.y += dy;
        self.pen.move_to(Exact::new(self.x, self.y));
        self.open = true;
        Ok(())
    }

    fn line_to(&mut self, dx: f64, dy: f64) -> Result<(), SfntError> {
        self.spend()?;
        self.x += dx;
        self.y += dy;
        self.pen.line_to(Exact::new(self.x, self.y));
        Ok(())
    }

    /// A cubic given as three successive deltas, which is how every Type 2
    /// curve operator ultimately expresses itself.
    fn curve_to(&mut self, d: [f64; 6]) -> Result<(), SfntError> {
        let c1 = (self.x + d[0], self.y + d[1]);
        let c2 = (c1.0 + d[2], c1.1 + d[3]);
        let p = (c2.0 + d[4], c2.1 + d[5]);
        self.curve_through(c1, c2, p)
    }

    /// A cubic to `p` with controls `c1` and `c2`, all absolute, leaving the
    /// pen at `p`.
    fn curve_through(
        &mut self,
        c1: (f64, f64),
        c2: (f64, f64),
        p: (f64, f64),
    ) -> Result<(), SfntError> {
        self.spend()?;
        (self.x, self.y) = p;
        self.pen.curve_to(
            Exact::new(c1.0, c1.1),
            Exact::new(c2.0, c2.1),
            Exact::new(p.0, p.1),
        );
        Ok(())
    }

    /// Count the stems an operator declares and clear the stack.
    ///
    /// Integer division absorbs a leading width operand: the stem operators
    /// take coordinate *pairs*, so an odd count means the first number is the
    /// width and the pairs start after it either way.
    fn count_stems(&mut self) {
        self.n_stems = self.n_stems.saturating_add(self.sp / 2);
        self.sp = 0;
    }

    /// `vsindex`: the variation subtable this glyph's blends use. Only
    /// before the first `blend`, which both libraries hold to; a negative
    /// one is left unused, as FreeType leaves it.
    fn vsindex(&mut self) -> Result<(), SfntError> {
        let [v] = self.last()?;
        self.sp = 0;
        let blend = self.blend.as_mut().ok_or(ERR)?;
        if blend.blended {
            return Err(ERR);
        }
        if v >= 0.0 {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "an integer operand, and non-negative here; one past \
                          u16 names no subtable and saturates to one that \
                          is refused as surely"
            )]
            {
                blend.vsindex = v.trunc() as u16;
            }
        }
        blend.weights = None;
        Ok(())
    }

    /// `blend`: `n` values, each followed further up the stack by one delta
    /// per region of the subtable in force, become `n` values blended at the
    /// instance -- the deltas popped, the results left where the defaults
    /// were for the operator that follows.
    fn blend(&mut self) -> Result<(), SfntError> {
        let [n] = self.last()?;
        self.sp = self.sp.checked_sub(1).ok_or(ERR)?;
        if !(0.0..=f64::from(u16::MAX)).contains(&n) {
            return Err(ERR);
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "range-checked on the line above"
        )]
        let n = n.trunc() as usize;
        let blend = self.blend.as_mut().ok_or(ERR)?;
        if blend.weights.is_none() {
            blend.weights = Some(blend.weights()?);
        }
        blend.blended = true;
        let weights = blend.weights.as_ref().ok_or(ERR)?;
        let k = weights.len();
        let total = mul(n, add(k, 1)?)?;
        let base = self.sp.checked_sub(total).ok_or(ERR)?;
        for i in 0..n {
            let at = add(base, i)?;
            let from = add(add(base, n)?, mul(i, k)?)?;
            let default = *self.stack.get(at).ok_or(ERR)?;
            let deltas = self.stack.get(from..add(from, k)?).ok_or(ERR)?;
            let blended = weights.apply(default, deltas);
            *self.stack.get_mut(at).ok_or(ERR)? = blended;
        }
        self.sp = add(base, n)?;
        Ok(())
    }

    fn run(&mut self, code: &[u8], depth: u8) -> Result<(), SfntError> {
        if depth > MAX_SUBR_DEPTH {
            return Err(ERR);
        }
        let mut i = 0usize;
        while i < code.len() {
            let b0 = u8_at(code, i)?;
            i = add(i, 1)?;
            match b0 {
                // --- operands ---------------------------------------------
                28 => {
                    let v = i16::from_be_bytes([u8_at(code, i)?, u8_at(code, add(i, 1)?)?]);
                    i = add(i, 2)?;
                    self.push(f64::from(v))?;
                }
                32..=246 => self.push(f64::from(i16::from(b0).saturating_sub(139)))?,
                247..=250 => {
                    let b1 = i32::from(u8_at(code, i)?);
                    i = add(i, 1)?;
                    let v = i32::from(b0)
                        .saturating_sub(247)
                        .saturating_mul(256)
                        .saturating_add(b1)
                        .saturating_add(108);
                    self.push(f64::from(v))?;
                }
                251..=254 => {
                    let b1 = i32::from(u8_at(code, i)?);
                    i = add(i, 1)?;
                    let v = i32::from(b0)
                        .saturating_sub(251)
                        .saturating_mul(-256)
                        .saturating_sub(b1)
                        .saturating_sub(108);
                    self.push(f64::from(v))?;
                }
                255 => {
                    // 16.16 fixed point — the only fractional operand form.
                    let v = i32::from_be_bytes([
                        u8_at(code, i)?,
                        u8_at(code, add(i, 1)?)?,
                        u8_at(code, add(i, 2)?)?,
                        u8_at(code, add(i, 3)?)?,
                    ]);
                    i = add(i, 4)?;
                    // Exact in f64, as every sum of such values is.
                    self.push(f64::from(v) / 65536.0)?;
                }

                // --- hints ------------------------------------------------
                1 | 3 | 18 | 23 => self.count_stems(),
                19 | 20 => {
                    // A `hintmask` before any explicit `vstem` carries the
                    // stem list on the stack, implicitly.
                    self.count_stems();
                    let bytes = self.n_stems.saturating_add(7) / 8;
                    i = add(i, bytes)?;
                    if i > code.len() {
                        return Err(ERR);
                    }
                }

                // --- moves ------------------------------------------------
                21 => {
                    let [dx, dy] = self.last()?;
                    self.sp = 0;
                    self.move_to(dx, dy)?;
                }
                22 => {
                    let [dx] = self.last()?;
                    self.sp = 0;
                    self.move_to(dx, 0.0)?;
                }
                4 => {
                    let [dy] = self.last()?;
                    self.sp = 0;
                    self.move_to(0.0, dy)?;
                }

                // --- lines ------------------------------------------------
                5 => {
                    // rlineto: any number of pairs.
                    let n = self.sp;
                    let mut k = 0usize;
                    while add(k, 2)? <= n {
                        let [dx, dy] = self.run_of(k)?;
                        self.line_to(dx, dy)?;
                        k = add(k, 2)?;
                    }
                    self.sp = 0;
                }
                6 | 7 => {
                    // hlineto / vlineto: single coordinates, alternating axis,
                    // starting horizontal for 6 and vertical for 7.
                    let mut horiz = b0 == 6;
                    for k in 0..self.sp {
                        let v = *self.args()?.get(k).ok_or(ERR)?;
                        if horiz {
                            self.line_to(v, 0.0)?;
                        } else {
                            self.line_to(0.0, v)?;
                        }
                        horiz = !horiz;
                    }
                    self.sp = 0;
                }

                // --- curves -----------------------------------------------
                8 => {
                    // rrcurveto: any number of six-tuples.
                    let mut k = 0usize;
                    while add(k, 6)? <= self.sp {
                        let d = self.run_of(k)?;
                        self.curve_to(d)?;
                        k = add(k, 6)?;
                    }
                    self.sp = 0;
                }
                24 => {
                    // rcurveline: curves, then one closing line.
                    let mut k = 0usize;
                    while add(k, 6)? <= self.sp.saturating_sub(2) {
                        let d = self.run_of(k)?;
                        self.curve_to(d)?;
                        k = add(k, 6)?;
                    }
                    let [dx, dy] = self.last()?;
                    self.sp = 0;
                    self.line_to(dx, dy)?;
                }
                25 => {
                    // rlinecurve: lines, then one closing curve.
                    let curve_at = self.sp.checked_sub(6).ok_or(ERR)?;
                    let mut k = 0usize;
                    while add(k, 2)? <= curve_at {
                        let [dx, dy] = self.run_of(k)?;
                        self.line_to(dx, dy)?;
                        k = add(k, 2)?;
                    }
                    let d = self.last()?;
                    self.sp = 0;
                    self.curve_to(d)?;
                }
                26 | 27 => {
                    // vvcurveto / hhcurveto: four-tuples whose first and last
                    // deltas are constrained to one axis, with an optional
                    // leading cross-axis delta applied to the first curve only.
                    let mut k = 0usize;
                    let mut cross = 0.0_f64;
                    if self.sp % 4 == 1 {
                        cross = *self.args()?.first().ok_or(ERR)?;
                        k = 1;
                    }
                    while add(k, 4)? <= self.sp {
                        let [p, q, r, s] = self.run_of(k)?;
                        let d = if b0 == 26 {
                            [cross, p, q, r, 0.0, s]
                        } else {
                            [p, cross, q, r, s, 0.0]
                        };
                        self.curve_to(d)?;
                        cross = 0.0;
                        k = add(k, 4)?;
                    }
                    self.sp = 0;
                }
                30 | 31 => {
                    // vhcurveto / hvcurveto: four-tuples that start on one axis
                    // and end on the other, alternating; the final tuple may
                    // carry a fifth value giving the otherwise-zero delta.
                    let mut horiz = b0 == 31;
                    let mut k = 0usize;
                    while add(k, 4)? <= self.sp {
                        let [p, q, r, s] = self.run_of(k)?;
                        // The fifth value only exists on the final tuple, so it
                        // is read separately rather than widening the group.
                        let extra = if self.sp.checked_sub(k) == Some(5) {
                            let [v] = self.run_of(add(k, 4)?)?;
                            v
                        } else {
                            0.0
                        };
                        let d = if horiz {
                            [p, 0.0, q, r, extra, s]
                        } else {
                            [0.0, p, q, r, s, extra]
                        };
                        self.curve_to(d)?;
                        horiz = !horiz;
                        k = add(k, 4)?;
                    }
                    self.sp = 0;
                }

                // --- CFF2 variations ----------------------------------------
                15 if self.blend.is_some() => self.vsindex()?,
                16 if self.blend.is_some() => self.blend()?,
                // CFF2 has no `return` and no `endchar`: a subroutine and a
                // glyph end where their charstrings do. An explicit one is
                // ignored and clears the stack, as FreeType treats it.
                11 | 14 if self.blend.is_some() => self.sp = 0,

                // --- control ----------------------------------------------
                10 | 29 => {
                    let subrs = if b0 == 10 {
                        self.local.ok_or(ERR)?
                    } else {
                        self.cff.global_subrs
                    };
                    let [n] = self.last()?;
                    self.sp = self.sp.checked_sub(1).ok_or(ERR)?;
                    #[allow(clippy::cast_possible_truncation)]
                    // A subroutine number is an integer operand; the index
                    // below rejects anything out of range.
                    let idx = (n.round() as i64).saturating_add(i64::from(subrs.bias()));
                    let idx = usize::try_from(idx).map_err(|_| ERR)?;
                    let body = subrs.get(self.data, idx)?;
                    self.run(body, depth.saturating_add(1))?;
                    // A subroutine that ended in `endchar` ends the glyph.
                    if self.seac.is_some() {
                        return Ok(());
                    }
                }
                11 => return Ok(()),
                14 => {
                    // endchar. Four trailing operands are the deprecated
                    // `seac` form; the caller composes the two glyphs, because
                    // only it can start a fresh interpreter.
                    if self.sp >= 4 {
                        self.seac = Some(self.last()?);
                    } else {
                        // A glyph with no seac still has to record that it
                        // finished, but `seac` is the caller's signal to
                        // recurse, so an empty marker would be wrong. Closing
                        // the contour is the whole of the work.
                        self.close_contour();
                    }
                    self.sp = 0;
                    return Ok(());
                }

                // --- two-byte operators -----------------------------------
                12 => {
                    let b1 = u8_at(code, i)?;
                    i = add(i, 1)?;
                    self.two_byte_op(b1)?;
                }

                _ => return Err(ERR),
            }
        }
        Ok(())
    }

    fn two_byte_op(&mut self, b1: u8) -> Result<(), SfntError> {
        match b1 {
            // hflex: a flex whose two curves share a baseline, so only one
            // vertical delta is stored and the second curve returns to the
            // starting y.
            34 => {
                let [dx1, dx2, dy2, dx3, dx4, dx5, dx6] = self.last()?;
                self.sp = 0;
                self.curve_to([dx1, 0.0, dx2, dy2, dx3, 0.0])?;
                self.curve_to([dx4, 0.0, dx5, -dy2, dx6, 0.0])?;
            }
            // flex: two ordinary curves plus a flex depth, which is a hinting
            // hint about when to flatten the pair and has no effect on the
            // outline.
            35 => {
                // The thirteenth operand is the flex depth, which is discarded.
                let [a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, _] = self.last()?;
                let first = [a0, a1, a2, a3, a4, a5];
                let second = [a6, a7, a8, a9, a10, a11];
                self.sp = 0;
                self.curve_to(first)?;
                self.curve_to(second)?;
            }
            // hflex1: as hflex, but the first curve may leave the baseline; the
            // second still returns to the starting y.
            36 => {
                let [dx1, dy1, dx2, dy2, dx3, dx4, dx5, dy5, dx6] = self.last()?;
                self.sp = 0;
                let start_y = self.y;
                self.curve_to([dx1, dy1, dx2, dy2, dx3, 0.0])?;
                let c1 = (self.x + dx4, self.y);
                let c2 = (c1.0 + dx5, c1.1 + dy5);
                self.curve_through(c1, c2, (c2.0 + dx6, start_y))?;
            }
            // flex1: the last delta is given on one axis only; which axis is
            // decided by whichever direction the flex travelled further in,
            // and the other coordinate returns to where the flex started.
            37 => {
                let v: [f64; 11] = self.last()?;
                self.sp = 0;
                let (start_x, start_y) = (self.x, self.y);
                let dx = v[0] + v[2] + v[4] + v[6] + v[8];
                let dy = v[1] + v[3] + v[5] + v[7] + v[9];
                self.curve_to([v[0], v[1], v[2], v[3], v[4], v[5]])?;
                let c1 = (self.x + v[6], self.y + v[7]);
                let c2 = (c1.0 + v[8], c1.1 + v[9]);
                let p = if dx.abs() > dy.abs() {
                    (c2.0 + v[10], start_y)
                } else {
                    (start_x, c2.1 + v[10])
                };
                self.curve_through(c1, c2, p)?;
            }
            _ => return Err(SfntError::CffUnsupported("Type 2 arithmetic operator")),
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    // A blended coordinate is compared exactly: which library's arithmetic
    // made it is decided in the last bit.
    clippy::float_cmp
)]
mod tests {
    use super::*;
    use crate::sfnt::Point;

    /// Encode a charstring integer the way a real font would.
    fn int(v: i32) -> Vec<u8> {
        if (-107..=107).contains(&v) {
            alloc::vec![u8::try_from(v + 139).unwrap()]
        } else {
            let b = i16::try_from(v).unwrap().to_be_bytes();
            alloc::vec![28, b[0], b[1]]
        }
    }

    /// Assemble a charstring from integer operands and raw operator bytes.
    fn cs(parts: &[&[u8]]) -> Vec<u8> {
        parts.iter().flat_map(|p| p.iter().copied()).collect()
    }

    /// Run a charstring in isolation, with no subroutines and no font around
    /// it. Every operator this exercises is self-contained, so the table
    /// scaffolding a real font would supply is not needed to test them.
    fn run_bare(code: &[u8]) -> Outline {
        let cff = Cff {
            base: 0,
            len: 0,
            char_strings: Index::default(),
            global_subrs: Index::default(),
            locals: Locals::Single(None),
            charset: Charset::Predefined,
            matrix: None,
            cff2: None,
        };
        let mut out = Outline::default();
        let mut interp = Interp::new(&cff, &[], None, &mut out, Exact::default(), None);
        interp.run(code, 0).unwrap();
        interp.close_contour();
        out
    }

    fn pt(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    /// Encode a charstring's 16.16 fixed-point operand.
    fn fixed(v: f64) -> Vec<u8> {
        let mut out = vec![255u8];
        out.extend_from_slice(&((v * 65536.0).round() as i32).to_be_bytes());
        out
    }

    /// An INDEX of `entries`, with two-byte offsets.
    fn index(entries: &[Vec<u8>]) -> Vec<u8> {
        let mut out = (entries.len() as u16).to_be_bytes().to_vec();
        if entries.is_empty() {
            return out;
        }
        out.push(2);
        let mut off = 1u16;
        out.extend_from_slice(&off.to_be_bytes());
        for e in entries {
            off += e.len() as u16;
            out.extend_from_slice(&off.to_be_bytes());
        }
        for e in entries {
            out.extend_from_slice(e);
        }
        out
    }

    /// A whole `CFF ` table whose charstrings are `glyphs`, under the
    /// predefined charset: glyph `n` carries SID `n`.
    fn table(glyphs: &[Vec<u8>]) -> Vec<u8> {
        let header = [1u8, 0, 4, 1];
        let names = index(&[b"T".to_vec()]);
        let empty = index(&[]);
        // The Top DICT's one entry is the CharStrings offset, written as a
        // five-byte integer so that the DICT's size does not depend on it.
        let top_len = index(&[vec![0; 6]]).len();
        let at = header.len() + names.len() + top_len + 2 * empty.len();
        let mut top = vec![29u8];
        top.extend_from_slice(&(at as i32).to_be_bytes());
        top.push(17);
        let mut out = header.to_vec();
        out.extend(names);
        out.extend(index(&[top]));
        out.extend_from_slice(&empty); // strings
        out.extend_from_slice(&empty); // global subroutines
        assert_eq!(out.len(), at);
        out.extend(index(glyphs));
        out
    }

    /// Run a charstring in isolation, as [`run_bare`] does, recording the
    /// points FreeType would store.
    fn run_points(code: &[u8]) -> TaggedOutline {
        let cff = Cff {
            base: 0,
            len: 0,
            char_strings: Index::default(),
            global_subrs: Index::default(),
            locals: Locals::Single(None),
            charset: Charset::Predefined,
            matrix: None,
            cff2: None,
        };
        let mut points = CffPoints::default();
        let mut interp = Interp::new(&cff, &[], None, &mut points, Exact::default(), None);
        interp.run(code, 0).unwrap();
        interp.close_contour();
        points.finish()
    }

    #[test]
    fn the_hinters_points_keep_a_charstrings_fractions_exactly() {
        // 13107/65536 needs all sixteen of 16.16's fraction bits, and 500
        // plus it needs 25 bits of mantissa -- one more than f32 has. Three
        // lines that return to the start in 16.16 return to it exactly here,
        // so the closing point folds as FreeType folds it.
        let d = 13107.0 / 65536.0;
        let code = cs(&[
            &int(500),
            &int(500),
            &[21],
            &fixed(d),
            &int(100),
            &fixed(2.0 * d),
            &int(-50),
            &fixed(-3.0 * d),
            &int(-50),
            &[5],
        ]);
        let t = run_points(&code);
        assert_eq!(t.ends, [3]);
        assert_eq!(
            t.points,
            [
                Exact::new(500.0, 500.0),
                Exact::new(500.0 + d, 600.0),
                Exact::new(500.0 + 3.0 * d, 550.0)
            ]
        );
        // The path narrows the same points to f32.
        let o = run_bare(&code);
        assert_eq!(
            o.commands[1],
            PathCmd::LineTo(Point::new((500.0 + d) as f32, 600.0))
        );
    }

    // --- CFF2 -------------------------------------------------------------

    /// A `CFF2` INDEX: a four-byte count, then as a CFF one.
    fn index2(entries: &[Vec<u8>]) -> Vec<u8> {
        let mut out = (entries.len() as u32).to_be_bytes().to_vec();
        if entries.is_empty() {
            return out;
        }
        out.push(4);
        let mut off = 1u32;
        out.extend_from_slice(&off.to_be_bytes());
        for e in entries {
            off += e.len() as u32;
            out.extend_from_slice(&off.to_be_bytes());
        }
        for e in entries {
            out.extend_from_slice(e);
        }
        out
    }

    /// An item variation store as a `CFF2` table carries one (no delta rows):
    /// one axis, `regions` as (start, peak, end), and each subtable's region
    /// indices.
    fn store2(regions: &[(i16, i16, i16)], subtables: &[&[u16]]) -> Vec<u8> {
        let mut regions_part = Vec::new();
        regions_part.extend_from_slice(&1u16.to_be_bytes());
        regions_part.extend_from_slice(&(regions.len() as u16).to_be_bytes());
        for &(a, b, c) in regions {
            for v in [a, b, c] {
                regions_part.extend_from_slice(&v.to_be_bytes());
            }
        }
        let header = 8 + 4 * subtables.len();
        let mut out = Vec::new();
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&(header as u32).to_be_bytes());
        out.extend_from_slice(&(subtables.len() as u16).to_be_bytes());
        let mut at = header + regions_part.len();
        let bodies: Vec<Vec<u8>> = subtables
            .iter()
            .map(|indices| {
                let mut b = vec![0, 0, 0, 0];
                b.extend_from_slice(&(indices.len() as u16).to_be_bytes());
                for &r in *indices {
                    b.extend_from_slice(&r.to_be_bytes());
                }
                b
            })
            .collect();
        for b in &bodies {
            out.extend_from_slice(&(at as u32).to_be_bytes());
            at += b.len();
        }
        out.extend(regions_part);
        for b in bodies {
            out.extend(b);
        }
        out
    }

    /// A DICT integer in its fixed five-byte form, so a DICT's length does
    /// not depend on the offsets it holds.
    fn int5(v: usize) -> Vec<u8> {
        let mut out = vec![29u8];
        out.extend_from_slice(&(v as i32).to_be_bytes());
        out
    }

    /// A whole `CFF2` table: `glyphs` as its charstrings, one Font DICT whose
    /// Private DICT names `vsindex`, and `store` (after its length word).
    fn table2(glyphs: &[Vec<u8>], vsindex: u16, store: &[u8]) -> Vec<u8> {
        // Header (5) + Top DICT (3 offsets of 5 bytes + 1-byte ops, plus the
        // two-byte FDArray op) + empty global subroutines (4).
        let top_len = 3 * 5 + 1 + 1 + 2;
        let mut at = 5 + top_len + 4;
        let char_strings = index2(glyphs);
        let char_strings_at = at;
        at += char_strings.len();
        // The Private DICT: `vsindex` as a one-byte-or-more integer, op 22.
        let private = {
            let mut p = int(i32::from(vsindex));
            p.push(22);
            p
        };
        let private_at = at;
        at += private.len();
        let mut font_dict = int5(private.len());
        font_dict.extend(int5(private_at));
        font_dict.push(18);
        let fd_array = index2(&[font_dict]);
        let fd_array_at = at;
        at += fd_array.len();
        let vstore_at = at;
        let mut top = int5(char_strings_at);
        top.push(17);
        top.extend(int5(fd_array_at));
        top.extend_from_slice(&[12, 36]);
        top.extend(int5(vstore_at));
        top.push(24);
        assert_eq!(top.len(), top_len);
        let mut out = vec![2u8, 0, 5];
        out.extend_from_slice(&(top_len as u16).to_be_bytes());
        out.extend(top);
        out.extend(index2(&[]));
        out.extend(char_strings);
        out.extend(private);
        out.extend(fd_array);
        out.extend_from_slice(&(store.len() as u16).to_be_bytes());
        out.extend_from_slice(store);
        out
    }

    /// One glyph of a `table2` of `glyphs` over `store` with `vsindex`, drawn
    /// at `instance`.
    fn draw2(
        glyph: &[u8],
        vsindex: u16,
        store: &[u8],
        instance: Instance<'_>,
    ) -> Result<Outline, SfntError> {
        let t = table2(&[glyph.to_vec()], vsindex, store);
        let cff = Cff::parse2(&t, 0, t.len(), 1000)?;
        assert!(cff.is_cff2());
        cff.outline_at(&t, 0, instance)
    }

    /// A glyph whose one point is `blend`ed: `moveto` to x = 100 with deltas
    /// `deltas`, y = 0, then a line of 10 so something is drawn.
    fn blended_move(deltas: &[i32]) -> Vec<u8> {
        let mut parts = vec![int(100)];
        parts.extend(deltas.iter().map(|&d| int(d)));
        parts.push(int(1));
        cs(&[&parts.concat(), &[16], &int(0), &[21], &int(10), &[6]])
    }

    fn first_point(outline: &Outline) -> Point {
        match outline.commands[0] {
            PathCmd::MoveTo(p) => p,
            ref other => panic!("not a move: {other:?}"),
        }
    }

    #[test]
    fn a_cff2_index_counts_its_entries_in_four_bytes() {
        let d = index2(&[b"ab".to_vec(), b"c".to_vec()]);
        let idx = Index::parse2(&d, 0).unwrap();
        assert_eq!(idx.count, 2);
        assert_eq!(idx.get(&d, 1).unwrap(), b"c");
        assert_eq!(idx.end, d.len());
        // Empty, it is the count alone -- four bytes, not two.
        let empty = Index::parse2(&[0, 0, 0, 0, 9], 0).unwrap();
        assert_eq!((empty.count, empty.end), (0, 4));
    }

    #[test]
    fn fdselect_format_4_maps_ranges_to_sixteen_bit_font_dicts() {
        // Glyphs 0-9 use Font DICT 3, 10-19 Font DICT 300; sentinel 20.
        let mut d = vec![4u8];
        d.extend_from_slice(&2u32.to_be_bytes());
        for (first, fd) in [(0u32, 3u16), (10, 300)] {
            d.extend_from_slice(&first.to_be_bytes());
            d.extend_from_slice(&fd.to_be_bytes());
        }
        d.extend_from_slice(&20u32.to_be_bytes());
        assert_eq!(Cff::fd_for_gid(&d, 0, 9).unwrap(), 3);
        assert_eq!(Cff::fd_for_gid(&d, 0, 10).unwrap(), 300);
        assert!(Cff::fd_for_gid(&d, 0, 20).is_err());
    }

    #[test]
    fn a_blend_is_its_default_at_the_default_instance() {
        let store = store2(&[(0, 16384, 16384)], &[&[0]]);
        let out = draw2(&blended_move(&[50]), 0, &store, Instance::Default).unwrap();
        assert_eq!(first_point(&out), pt(100.0, 0.0));
    }

    #[test]
    fn each_library_blends_at_its_own_coordinates_and_precision() {
        // One region peaking at +1; the instance a third of the way there.
        let store = store2(&[(0, 16384, 16384)], &[&[0]]);
        let glyph = blended_move(&[1]);
        // HarfBuzz: 5461/16384 as an f32 scalar, times 1, in f64.
        let hb = draw2(&glyph, 0, &store, Instance::HarfBuzz(&[5461])).unwrap();
        let scalar = 5461.0_f32 / 16384.0;
        assert_eq!(first_point(&hb).x, (100.0 + f64::from(scalar)) as f32);
        // FreeType: FT_DivFix(21845, 65536) = 21845, FT_MulFix'ed by 1.0 --
        // 100 + 21845/65536.
        let ft = draw2(&glyph, 0, &store, Instance::FreeType(&[21845])).unwrap();
        assert_eq!(first_point(&ft).x, (100.0 + 21845.0 / 65536.0) as f32);
        assert_ne!(first_point(&hb).x, first_point(&ft).x);
    }

    #[test]
    fn vsindex_chooses_the_subtable_and_a_private_dict_its_default() {
        // Subtable 0 weighs region 0 (peak at +1); subtable 1 region 1 (peak
        // at -1), which is nothing at +1.
        let store = store2(&[(0, 16384, 16384), (-16384, -16384, 0)], &[&[0], &[1]]);
        let at_top = Instance::HarfBuzz(&[16384]);
        assert_eq!(
            first_point(&draw2(&blended_move(&[50]), 0, &store, at_top).unwrap()).x,
            150.0
        );
        // The Private DICT's `vsindex` 1: region 1, which does not apply.
        assert_eq!(
            first_point(&draw2(&blended_move(&[50]), 1, &store, at_top).unwrap()).x,
            100.0
        );
        // And the charstring's own `vsindex`, before the blend.
        let chosen = cs(&[&int(1), &[15], &blended_move(&[50])]);
        assert_eq!(
            first_point(&draw2(&chosen, 0, &store, at_top).unwrap()).x,
            100.0
        );
        // After a blend, a `vsindex` is malformed.
        let late = cs(&[
            &int(100),
            &int(50),
            &int(1),
            &[16],
            &int(0),
            &int(1),
            &[15],
            &[21],
        ]);
        assert!(draw2(&late, 0, &store, at_top).is_err());
    }

    #[test]
    fn freetype_refuses_a_blend_its_store_cannot_weigh() {
        // A `vsindex` past the subtables: FreeType fails the glyph; HarfBuzz
        // weighs no regions and keeps the default.
        let store = store2(&[(0, 16384, 16384)], &[&[0]]);
        let glyph = cs(&[
            &int(5),
            &[15],
            &int(100),
            &int(1),
            &[16],
            &int(0),
            &[21],
            &int(10),
            &[6],
        ]);
        assert!(draw2(&glyph, 0, &store, Instance::FreeType(&[0x1_0000])).is_err());
        let hb = draw2(&glyph, 0, &store, Instance::HarfBuzz(&[16384])).unwrap();
        assert_eq!(first_point(&hb).x, 100.0);
        // Coordinates for two axes against a one-axis store.
        let two = draw2(&blended_move(&[50]), 0, &store, Instance::FreeType(&[1, 2]));
        assert!(two.is_err());
    }

    #[test]
    fn cff2_has_no_return_or_endchar_and_ignores_one() {
        // An `endchar` mid-glyph does not end it: the line after it is drawn.
        let store = store2(&[(0, 16384, 16384)], &[&[0]]);
        let glyph = cs(&[
            &int(0),
            &int(0),
            &[21],
            &int(10),
            &[6],
            &[14],
            &int(20),
            &[7],
        ]);
        let out = draw2(&glyph, 0, &store, Instance::Default).unwrap();
        assert!(out.commands.contains(&PathCmd::LineTo(pt(10.0, 20.0))));
    }

    #[test]
    fn a_cff2_stack_holds_far_more_than_a_cff_one() {
        // Sixty blended values -- 120 operands and a count -- which CFF's 48
        // could not hold.
        let store = store2(&[(0, 16384, 16384)], &[&[0]]);
        let mut parts: Vec<Vec<u8>> = (0..60).map(|_| int(1)).collect();
        parts.extend((0..60).map(|_| int(2)));
        parts.push(int(60));
        let mut glyph = parts.concat();
        glyph.push(16);
        // 60 values of 1 + 2 at +1 = 3 each: an rlineto of 30 pairs.
        glyph.push(5);
        let full = cs(&[&int(0), &int(0), &[21], &glyph]);
        let out = draw2(&full, 0, &store, Instance::HarfBuzz(&[16384])).unwrap();
        assert!(out.commands.contains(&PathCmd::LineTo(pt(90.0, 90.0))));
    }

    #[test]
    fn freetype_weighs_a_region_by_div_fix_and_mul_fix_through_a_zero() {
        let store = store2(&[(0, 8192, 16384), (-16384, -8192, 0)], &[&[0, 1]]);
        let vs = CffVStore::parse(&store, 0).unwrap();
        // At +0.25: region 0 is FT_DivFix(0.25, 0.5) = 0.5; region 1 is out.
        assert_eq!(vs.weights(0, &[0x4000]).unwrap(), vec![0x8000, 0]);
        // At its peak a region is 1, at the default neither applies.
        assert_eq!(vs.weights(0, &[0x8000]).unwrap(), vec![0x1_0000, 0]);
        assert_eq!(vs.weights(0, &[0]).unwrap(), vec![0, 0]);
        // A subtable it does not have, and coordinates for no axes (the
        // default, weighed at nothing).
        assert!(vs.weights(1, &[0]).is_none());
        assert_eq!(vs.weights(0, &[]).unwrap(), vec![0, 0]);
    }

    #[test]
    fn a_cff2_face_is_drawn_and_measured_as_harfbuzz_draws_it() {
        use crate::hint::fixture::{VAR_CFF2, VAR_CFF2_DRAWN};
        let face = crate::sfnt::Face::parse(VAR_CFF2.to_vec()).unwrap();
        let coords = face
            .variation_axes()
            .unwrap()
            .normalize_tags(&[(*b"wght", 610.0)]);
        let mut wrong = Vec::new();
        for &(gid, name, extents, ops, points) in &VAR_CFF2_DRAWN {
            let got = face.glyph_extents_at(gid, &coords).unwrap();
            if got != extents {
                wrong.push(alloc::format!("{name}: box {got:?}, HarfBuzz {extents:?}"));
            }
            let outline = face.outline_at(gid, &coords).unwrap();
            let (got_ops, got_points) = spelled(&outline.commands);
            if got_ops != ops || got_points != points {
                wrong.push(alloc::format!(
                    "{name}: path {got_ops} {got_points:?}, HarfBuzz {ops} {points:?}"
                ));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// An outline as the fixture spells HarfBuzz's drawing: `M`, `L`, `C`,
    /// `Q` and `Z`, with the points' x and y in turn -- a contour with
    /// nothing drawn left out, and a closing line back to its start dropped.
    fn spelled(commands: &[PathCmd]) -> (String, Vec<f32>) {
        let mut contours: Vec<(Point, Vec<PathCmd>)> = Vec::new();
        let mut open: Option<(Point, Vec<PathCmd>)> = None;
        for cmd in commands {
            match *cmd {
                PathCmd::MoveTo(p) => {
                    contours.extend(open.take());
                    open = Some((p, Vec::new()));
                }
                PathCmd::Close => contours.extend(open.take()),
                other => {
                    if let Some((_, segs)) = open.as_mut() {
                        segs.push(other);
                    }
                }
            }
        }
        contours.extend(open.take());
        let (mut ops, mut points) = (String::new(), Vec::new());
        for (start, mut segs) in contours {
            if segs.last() == Some(&PathCmd::LineTo(start)) {
                segs.pop();
            }
            if segs.is_empty() {
                continue;
            }
            ops.push('M');
            points.extend([start.x, start.y]);
            for seg in segs {
                match seg {
                    PathCmd::LineTo(p) => {
                        ops.push('L');
                        points.extend([p.x, p.y]);
                    }
                    PathCmd::QuadTo(c, p) => {
                        ops.push('Q');
                        points.extend([c.x, c.y, p.x, p.y]);
                    }
                    PathCmd::CurveTo(a, b, p) => {
                        ops.push('C');
                        points.extend([a.x, a.y, b.x, b.y, p.x, p.y]);
                    }
                    PathCmd::MoveTo(_) | PathCmd::Close => {}
                }
            }
            ops.push('Z');
        }
        (ops, points)
    }

    #[test]
    fn a_seac_accent_is_drawn_from_its_offset() {
        // StandardEncoding's `A` (65) is SID 34 and `acute` (194) SID 125,
        // which the predefined charset makes glyphs 34 and 125.
        let mut glyphs = vec![vec![14u8]; 126];
        let triangle = |size: i32| {
            cs(&[
                &int(0),
                &int(0),
                &[21],
                &int(size),
                &int(0),
                &int(0),
                &int(size),
                &[5],
                &[14],
            ])
        };
        glyphs[34] = triangle(100);
        glyphs[125] = triangle(10);
        // The accent 50.5 units right and 200 up: a fraction, so that where
        // its pen starts matters to the points.
        glyphs[1] = cs(&[&fixed(50.5), &int(200), &int(65), &int(194), &[14]]);
        let d = table(&glyphs);
        let cff = Cff::parse(&d, 0, d.len(), 1000).unwrap();
        let t = cff.tagged_outline(&d, 1, Instance::Default).unwrap();
        assert_eq!(t.ends, [3, 6]);
        assert_eq!(
            &t.points[3..],
            [
                Exact::new(50.5, 200.0),
                Exact::new(60.5, 200.0),
                Exact::new(60.5, 210.0)
            ]
        );
        // The path agrees: the base's move, two lines and close, then the
        // accent's.
        let o = cff.outline(&d, 1).unwrap();
        assert_eq!(o.commands[4], PathCmd::MoveTo(pt(50.5, 200.0)));
        assert_eq!(o.commands[6], PathCmd::LineTo(pt(60.5, 210.0)));

        // A component that is itself composed is malformed, as FreeType has
        // it -- here the base names itself, which would otherwise recurse.
        glyphs[34] = cs(&[&int(0), &int(0), &int(65), &int(194), &[14]]);
        let d = table(&glyphs);
        let cff = Cff::parse(&d, 0, d.len(), 1000).unwrap();
        assert_eq!(cff.outline(&d, 1).unwrap_err(), ERR);
        assert_eq!(
            cff.tagged_outline(&d, 1, Instance::Default).unwrap_err(),
            ERR
        );
    }

    #[test]
    fn a_rectangle_drawn_with_the_alternating_line_operators() {
        // 100 200 rmoveto  50 hlineto ... : hlineto alternates axes, so four
        // operands draw all four sides bar the implicit closing one.
        let code = cs(&[
            &int(100),
            &int(200),
            &[21],
            &int(50),
            &int(40),
            &int(-50),
            &[6],
            &[14],
        ]);
        let o = run_bare(&code);
        assert_eq!(
            o.commands,
            alloc::vec![
                PathCmd::MoveTo(pt(100.0, 200.0)),
                PathCmd::LineTo(pt(150.0, 200.0)),
                PathCmd::LineTo(pt(150.0, 240.0)),
                PathCmd::LineTo(pt(100.0, 240.0)),
                PathCmd::Close,
            ]
        );
    }

    #[test]
    fn a_leading_width_operand_is_not_mistaken_for_a_coordinate() {
        // The same move, with a width in front of it. Reading rmoveto's
        // operands from the end of the stack has to skip the width; reading
        // them from the front would put the glyph at (200, 100).
        let plain = run_bare(&cs(&[&int(100), &int(200), &[21], &[14]]));
        let with_width = run_bare(&cs(&[&int(555), &int(100), &int(200), &[21], &[14]]));
        assert_eq!(with_width.commands, plain.commands);
        assert_eq!(
            plain.commands.first(),
            Some(&PathCmd::MoveTo(pt(100.0, 200.0)))
        );
    }

    #[test]
    fn hintmask_consumes_one_byte_per_eight_stems() {
        // Four stems (eight operands) then hintmask: one mask byte. If the
        // skip were wrong the mask byte would be read as an operand and the
        // line would land somewhere else entirely.
        let code = cs(&[
            &int(0),
            &int(10),
            &int(20),
            &int(10),
            &int(40),
            &int(10),
            &int(60),
            &int(10),
            &[19],
            &[0b1111_0000],
            &int(5),
            &int(5),
            &[21],
            &int(10),
            &[6],
            &[14],
        ]);
        let o = run_bare(&code);
        assert_eq!(
            o.commands.first(),
            Some(&PathCmd::MoveTo(pt(5.0, 5.0))),
            "the mask byte leaked into the operand stack"
        );
    }

    #[test]
    fn hintmask_skips_two_bytes_past_eight_stems() {
        let mut parts: Vec<Vec<u8>> = Vec::new();
        // Nine stems: 18 operands, so two mask bytes.
        for k in 0..9 {
            parts.push(int(k * 20));
            parts.push(int(10));
        }
        let mut code: Vec<u8> = parts.into_iter().flatten().collect();
        code.push(19);
        code.push(0xff);
        code.push(0x80);
        code.extend(int(7));
        code.extend(int(9));
        code.push(21);
        code.push(14);
        let o = run_bare(&code);
        assert_eq!(o.commands.first(), Some(&PathCmd::MoveTo(pt(7.0, 9.0))));
    }

    #[test]
    fn vhcurveto_alternates_axes_and_takes_the_trailing_fifth_operand() {
        // One four-tuple starting vertical, plus a fifth operand supplying the
        // delta that would otherwise be zero.
        let code = cs(&[
            &int(0),
            &int(0),
            &[21],
            &int(10),
            &int(20),
            &int(30),
            &int(40),
            &int(7),
            &[30],
            &[14],
        ]);
        let o = run_bare(&code);
        // vertical start: c1 = (0, 0+10); c2 = c1 + (20, 30); end = c2 + (40, 7)
        assert_eq!(
            o.commands.get(1),
            Some(&PathCmd::CurveTo(
                pt(0.0, 10.0),
                pt(20.0, 40.0),
                pt(60.0, 47.0)
            ))
        );
    }

    #[test]
    fn hhcurveto_applies_its_odd_leading_delta_to_the_first_curve_only() {
        let code = cs(&[
            &int(0),
            &int(0),
            &[21],
            &int(5), // dy1, applied once
            &int(10),
            &int(20),
            &int(30),
            &int(40),
            &int(10),
            &int(20),
            &int(30),
            &int(40),
            &[27],
            &[14],
        ]);
        let o = run_bare(&code);
        let PathCmd::CurveTo(a1, _, _) = o.commands[1] else {
            panic!("expected a curve, got {:?}", o.commands[1]);
        };
        assert_eq!(a1, pt(10.0, 5.0), "the leading delta did not reach curve 1");
        let PathCmd::CurveTo(_, _, end1) = o.commands[1] else {
            panic!("expected a curve");
        };
        let PathCmd::CurveTo(a2, _, _) = o.commands[2] else {
            panic!("expected a second curve, got {:?}", o.commands[2]);
        };
        // The second curve's first control point stays on the line the first
        // curve ended on: hhcurveto's leading delta applies once, not once per
        // tuple.
        assert_eq!(a2, pt(end1.x + 10.0, end1.y), "the leading delta repeated");
    }

    #[test]
    fn flex_draws_the_two_curves_it_names() {
        let mut code = cs(&[&int(0), &int(0), &[21]]);
        for v in [10, 10, 10, 10, 10, -10, 10, -10, 10, 10, 10, 10, 50] {
            code.extend(int(v));
        }
        code.push(12);
        code.push(35);
        code.push(14);
        let o = run_bare(&code);
        assert_eq!(o.commands.len(), 4, "flex is two curves: {:?}", o.commands);
        assert!(matches!(o.commands[1], PathCmd::CurveTo(..)));
        assert!(matches!(o.commands[2], PathCmd::CurveTo(..)));
        // The flex depth (the 13th operand) is a hint, not a coordinate: it
        // must not move the pen.
        let PathCmd::CurveTo(_, _, end) = o.commands[2] else {
            panic!("expected a curve");
        };
        assert_eq!(end, pt(60.0, 20.0));
    }

    #[test]
    fn flex1_returns_to_the_axis_it_travelled_less_far_along() {
        let mut code = cs(&[&int(0), &int(0), &[21]]);
        // Mostly horizontal travel, so the last operand is a dx and the y
        // returns to where the flex started.
        for v in [20, 5, 20, 5, 20, -5, 20, -5, 20, 0, 20] {
            code.extend(int(v));
        }
        code.push(12);
        code.push(37);
        code.push(14);
        let o = run_bare(&code);
        let PathCmd::CurveTo(_, _, end) = o.commands[2] else {
            panic!("expected a curve, got {:?}", o.commands[2]);
        };
        // The return is exact in principle, but it is reached by summing five
        // deltas, so this allows for the rounding that sum can carry.
        assert!(
            end.y.abs() < 1e-4,
            "flex1 did not return to its starting y: {}",
            end.y
        );
    }

    #[test]
    fn a_moveto_closes_the_contour_before_it() {
        let code = cs(&[
            &int(0),
            &int(0),
            &[21],
            &int(10),
            &[6],
            &int(50),
            &int(50),
            &[21],
            &int(10),
            &[6],
            &[14],
        ]);
        let o = run_bare(&code);
        assert_eq!(
            o.commands.iter().filter(|c| **c == PathCmd::Close).count(),
            2
        );
        assert_eq!(o.commands[2], PathCmd::Close);
    }

    #[test]
    fn an_unimplemented_operator_is_reported_rather_than_guessed_at() {
        let cff = Cff {
            base: 0,
            len: 0,
            char_strings: Index::default(),
            global_subrs: Index::default(),
            locals: Locals::Single(None),
            charset: Charset::Predefined,
            matrix: None,
            cff2: None,
        };
        let mut out = Outline::default();
        let mut interp = Interp::new(&cff, &[], None, &mut out, Exact::default(), None);
        // 12 10 is `add`, which this module deliberately does not implement.
        let err = interp.run(&[12, 10], 0).unwrap_err();
        assert_eq!(err, SfntError::CffUnsupported("Type 2 arithmetic operator"));
    }

    #[test]
    fn the_subroutine_bias_follows_the_index_size() {
        let small = Index {
            count: 100,
            ..Index::default()
        };
        let medium = Index {
            count: 2000,
            ..Index::default()
        };
        let large = Index {
            count: 40000,
            ..Index::default()
        };
        assert_eq!(small.bias(), 107);
        assert_eq!(medium.bias(), 1131);
        assert_eq!(large.bias(), 32768);
    }

    #[test]
    fn standard_encoding_covers_ascii_and_the_named_high_codes() {
        assert_eq!(standard_encoding_sid(b' '), Some(1));
        assert_eq!(standard_encoding_sid(b'A'), Some(34));
        assert_eq!(standard_encoding_sid(b'~'), Some(95));
        assert_eq!(standard_encoding_sid(161), Some(96));
        assert_eq!(standard_encoding_sid(251), Some(149));
        assert_eq!(standard_encoding_sid(0), None);
        assert_eq!(standard_encoding_sid(160), None);
        assert_eq!(standard_encoding_sid(255), None);
    }

    #[test]
    fn a_real_dict_operand_decodes_its_packed_digits() {
        // 0.001 as CFF packs it: '0' '.' '0' '0' '1' end.
        let d = [30u8, 0x0a, 0x00, 0x1f];
        let (v, len) = real_operand(&d, 0).unwrap();
        assert!((v - 0.001).abs() < 1e-12, "got {v}");
        assert_eq!(len, 4);

        // -2.5e3, exercising the sign, the fraction and the exponent nibbles.
        let d = [30u8, 0xe2, 0xa5, 0xb3, 0xff];
        let (v, _) = real_operand(&d, 0).unwrap();
        assert!((v + 2500.0).abs() < 1e-9, "got {v}");
    }

    #[test]
    fn the_font_matrix_is_the_identity_at_a_thousand_units_per_em() {
        assert!(em_transform([0.001, 0.0, 0.0, 0.001, 0.0, 0.0], 1000).is_none());
        // A face whose charstrings are drawn on a 2048 grid but which declares
        // 1000 units per em has to be scaled, or every glyph is twice too big.
        let t = em_transform([1.0 / 2048.0, 0.0, 0.0, 1.0 / 2048.0, 0.0, 0.0], 1000).unwrap();
        assert!((t.a - 1000.0 / 2048.0).abs() < 1e-6);
    }

    #[test]
    fn a_truncated_index_is_rejected_rather_than_read_past() {
        // count = 1, offSize = 1, offsets say the data runs to byte 200.
        let d = [0u8, 1, 1, 1, 200];
        assert_eq!(Index::parse(&d, 0).unwrap_err(), ERR);
    }

    #[test]
    fn an_index_with_a_zero_offset_size_is_rejected() {
        let d = [0u8, 1, 0, 1, 2];
        assert_eq!(Index::parse(&d, 0).unwrap_err(), ERR);
    }

    #[test]
    fn an_empty_index_is_two_bytes_and_no_more() {
        let d = [0u8, 0, 0xaa];
        let idx = Index::parse(&d, 0).unwrap();
        assert_eq!(idx.count, 0);
        assert_eq!(idx.end, 2);
    }

    #[test]
    fn an_index_round_trips_its_entries() {
        // Two entries, "ab" and "cde", offsets 1, 3, 6 at one byte each.
        let d = [0u8, 2, 1, 1, 3, 6, b'a', b'b', b'c', b'd', b'e'];
        let idx = Index::parse(&d, 0).unwrap();
        assert_eq!(idx.count, 2);
        assert_eq!(idx.get(&d, 0).unwrap(), b"ab");
        assert_eq!(idx.get(&d, 1).unwrap(), b"cde");
        assert_eq!(idx.get(&d, 2).unwrap_err(), ERR);
        assert_eq!(idx.end, d.len());
    }

    #[test]
    fn a_glyph_cannot_draw_without_limit() {
        // A charstring that keeps drawing: `rlineto` with a full stack, run
        // enough times to pass the ceiling, must stop rather than allocate.
        let mut code = cs(&[&int(0), &int(0), &[21]]);
        for _ in 0..24 {
            code.extend(int(1));
        }
        let body = code.split_off(3);
        let mut prog = code;
        for _ in 0..7000 {
            prog.extend_from_slice(&body);
            prog.push(5);
        }
        let cff = Cff {
            base: 0,
            len: 0,
            char_strings: Index::default(),
            global_subrs: Index::default(),
            locals: Locals::Single(None),
            charset: Charset::Predefined,
            matrix: None,
            cff2: None,
        };
        let mut out = Outline::default();
        let mut interp = Interp::new(&cff, &[], None, &mut out, Exact::default(), None);
        assert_eq!(interp.run(&prog, 0).unwrap_err(), ERR);
        assert!(out.commands.len() <= MAX_COMMANDS + 1);
    }
}
