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
//!
//! # What is not
//!
//! * **CFF2** (`CFF2` table). That is the variable-font revision: no header
//!   Name INDEX, blend operators, and an item-variation store. It is a
//!   separate body of work and is reported as such rather than misparsed.
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

use crate::sfnt::{Outline, PathCmd, Point, SfntError, Transform};

/// Every structural complaint about this table reads the same way.
const ERR: SfntError = SfntError::MalformedTable("CFF ");

/// How deep `callsubr`/`callgsubr` may nest. The Type 2 specification sets
/// the limit at 10; a file exceeding it is malformed, not merely unusual.
const MAX_SUBR_DEPTH: u8 = 10;

/// The Type 2 operand stack is 48 entries in the specification.
const STACK_LIMIT: usize = 48;

/// Ceiling on the path commands one glyph may emit.
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
    let b: [u8; 2] = d.get(off..add(off, 2)?).ok_or(ERR)?.try_into().map_err(|_| ERR)?;
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
    fn parse(d: &[u8], at: usize) -> Result<Self, SfntError> {
        let count = usize::from(u16_at(d, at)?);
        if count == 0 {
            // An empty INDEX is exactly its two-byte count and nothing else —
            // in particular it has no offSize byte to read.
            return Ok(Self {
                end: add(at, 2)?,
                ..Self::default()
            });
        }
        let off_size = usize::from(u8_at(d, add(at, 2)?)?);
        if !(1..=4).contains(&off_size) {
            return Err(ERR);
        }
        let offsets = add(at, 3)?;
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
    mut f: impl FnMut(u16, &[f64]) -> Result<(), SfntError>,
) -> Result<(), SfntError> {
    let mut operands: [f64; 48] = [0.0; 48];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < d.len() {
        let b0 = u8_at(d, i)?;
        match b0 {
            // Operators.
            0..=21 => {
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
                if n >= operands.len() {
                    return Err(ERR);
                }
                *operands.get_mut(n).ok_or(ERR)? = v;
                n = add(n, 1)?;
                i = add(i, len)?;
            }
            // 22..=27, 31, 255 are reserved.
            _ => return Err(ERR),
        }
    }
    Ok(())
}

/// One DICT operand, returning its value and its encoded length.
fn dict_operand(d: &[u8], at: usize) -> Result<(f64, usize), SfntError> {
    let b0 = u8_at(d, at)?;
    match b0 {
        28 => Ok((f64::from(i16::from_be_bytes([
            u8_at(d, add(at, 1)?)?,
            u8_at(d, add(at, 2)?)?,
        ])), 3)),
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
                        exponent = exponent.saturating_mul(10).saturating_add(i32::from(nibble));
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
                    let scale =
                        pow10(exp_sign.saturating_mul(exponent).saturating_sub(frac_digits));
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

/// A parsed `CFF ` table.
///
/// Every offset inside a CFF table is measured from the table's own start, so
/// this type never sees the font file: it is handed the table's slice and all
/// of its stored offsets are relative to that slice's byte zero. That removes
/// a whole class of "which base is this relative to" mistake, at the cost of
/// the caller having to re-slice on each call — which is what
/// [`table`](Self::table) is for.
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
                esc if esc == self::esc(6) => charstring_type = args.first().copied().unwrap_or(2.0),
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
        })
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
        let d = self.table(data)?;
        let mut out = Outline::default();
        self.outline_into(d, gid, &mut out, 0)?;
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

    /// Draw `gid` into `out`. `d` is the table slice.
    fn outline_into(
        &self,
        d: &[u8],
        gid: u16,
        out: &mut Outline,
        depth: u8,
    ) -> Result<(), SfntError> {
        if usize::from(gid) >= self.char_strings.count {
            return Err(SfntError::GlyphOutOfRange);
        }
        let local = self.local_subrs(d, gid)?;
        let mut interp = Interp {
            cff: self,
            data: d,
            local,
            stack: [0.0; STACK_LIMIT],
            sp: 0,
            n_stems: 0,
            x: 0.0,
            y: 0.0,
            open: false,
            out,
            seac: None,
        };
        interp.run(self.char_strings.get(d, usize::from(gid))?, 0)?;
        interp.close_contour();
        let seac = interp.seac;

        if let Some([adx, ady, bchar, achar]) = seac {
            // The deprecated four-argument `endchar`: draw a base glyph and an
            // accent from StandardEncoding, the accent shifted by (adx, ady).
            // Composition can nest — an accent is a real glyph and could in
            // principle be composed itself — so it is depth-limited the same
            // way `glyf` composites are.
            if depth >= MAX_SUBR_DEPTH {
                return Err(ERR);
            }
            let base = self.seac_gid(d, bchar)?;
            let accent = self.seac_gid(d, achar)?;
            self.outline_into(d, base, out, depth.saturating_add(1))?;
            let mut acc = Outline::default();
            self.outline_into(d, accent, &mut acc, depth.saturating_add(1))?;
            let shift = Transform {
                e: adx,
                f: ady,
                ..Transform::IDENTITY
            };
            for cmd in &acc.commands {
                out.commands.push(match *cmd {
                    PathCmd::MoveTo(p) => PathCmd::MoveTo(shift.apply(p)),
                    PathCmd::LineTo(p) => PathCmd::LineTo(shift.apply(p)),
                    PathCmd::QuadTo(c, p) => PathCmd::QuadTo(shift.apply(c), shift.apply(p)),
                    PathCmd::CurveTo(a, b, p) => {
                        PathCmd::CurveTo(shift.apply(a), shift.apply(b), shift.apply(p))
                    }
                    PathCmd::Close => PathCmd::Close,
                });
            }
        }
        Ok(())
    }

    /// Resolve a `seac` StandardEncoding code to a glyph id.
    fn seac_gid(&self, d: &[u8], code: f32) -> Result<u16, SfntError> {
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

    /// FDSelect: which entry of the FDArray glyph `gid` uses.
    fn fd_for_gid(d: &[u8], at: usize, gid: u16) -> Result<u8, SfntError> {
        match u8_at(d, at)? {
            // Format 0: one byte per glyph, in glyph order.
            0 => u8_at(d, add(at, add(1, usize::from(gid))?)?),
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
                        return u8_at(d, add(rec, 2)?);
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

struct Interp<'a> {
    cff: &'a Cff,
    data: &'a [u8],
    local: Option<Index>,
    stack: [f32; STACK_LIMIT],
    sp: usize,
    /// Stem count, kept only so that `hintmask` skips the right number of
    /// mask bytes — one bit per stem, rounded up to a byte.
    n_stems: usize,
    x: f32,
    y: f32,
    /// Whether a contour is currently open, so that a `moveto` knows to close
    /// the previous one. CFF contours are implicitly closed; there is no
    /// `closepath` in Type 2.
    open: bool,
    out: &'a mut Outline,
    /// Set by a four-argument `endchar`; acted on by the caller, which is the
    /// only place that can recurse into another glyph.
    seac: Option<[f32; 4]>,
}

impl Interp<'_> {
    fn push(&mut self, v: f32) -> Result<(), SfntError> {
        if self.sp >= STACK_LIMIT {
            return Err(ERR);
        }
        *self.stack.get_mut(self.sp).ok_or(ERR)? = v;
        self.sp = self.sp.saturating_add(1);
        Ok(())
    }

    fn args(&self) -> Result<&[f32], SfntError> {
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
    fn last<const N: usize>(&self) -> Result<[f32; N], SfntError> {
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
    fn run_of<const N: usize>(&self, k: usize) -> Result<[f32; N], SfntError> {
        let end = add(k, N)?;
        let s = self.args()?.get(k..end).ok_or(ERR)?;
        s.try_into().map_err(|_| ERR)
    }

    fn emit(&mut self, cmd: PathCmd) -> Result<(), SfntError> {
        if self.out.commands.len() >= MAX_COMMANDS {
            return Err(ERR);
        }
        self.out.commands.push(cmd);
        Ok(())
    }

    fn close_contour(&mut self) {
        if self.open {
            self.out.commands.push(PathCmd::Close);
            self.open = false;
        }
    }

    fn move_to(&mut self, dx: f32, dy: f32) -> Result<(), SfntError> {
        self.close_contour();
        self.x += dx;
        self.y += dy;
        self.emit(PathCmd::MoveTo(Point::new(self.x, self.y)))?;
        self.open = true;
        Ok(())
    }

    fn line_to(&mut self, dx: f32, dy: f32) -> Result<(), SfntError> {
        self.x += dx;
        self.y += dy;
        self.emit(PathCmd::LineTo(Point::new(self.x, self.y)))
    }

    /// A cubic given as three successive deltas, which is how every Type 2
    /// curve operator ultimately expresses itself.
    fn curve_to(&mut self, d: [f32; 6]) -> Result<(), SfntError> {
        let c1 = Point::new(self.x + d[0], self.y + d[1]);
        let c2 = Point::new(c1.x + d[2], c1.y + d[3]);
        let p = Point::new(c2.x + d[4], c2.y + d[5]);
        self.x = p.x;
        self.y = p.y;
        self.emit(PathCmd::CurveTo(c1, c2, p))
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
                    self.push(f32::from(v))?;
                }
                32..=246 => self.push(f32::from(i16::from(b0).saturating_sub(139)))?,
                247..=250 => {
                    let b1 = i32::from(u8_at(code, i)?);
                    i = add(i, 1)?;
                    let v = i32::from(b0)
                        .saturating_sub(247)
                        .saturating_mul(256)
                        .saturating_add(b1)
                        .saturating_add(108);
                    #[allow(clippy::cast_precision_loss)]
                    // Bounded by 1131; exact in f32.
                    self.push(v as f32)?;
                }
                251..=254 => {
                    let b1 = i32::from(u8_at(code, i)?);
                    i = add(i, 1)?;
                    let v = i32::from(b0)
                        .saturating_sub(251)
                        .saturating_mul(-256)
                        .saturating_sub(b1)
                        .saturating_sub(108);
                    #[allow(clippy::cast_precision_loss)]
                    // Bounded by -1131; exact in f32.
                    self.push(v as f32)?;
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
                    #[allow(clippy::cast_precision_loss)]
                    // A 16.16 value's magnitude is within f32's exact range for
                    // every coordinate a font can express in an em square.
                    self.push(v as f32 / 65536.0)?;
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
                    let mut cross = 0.0_f32;
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
                let c1 = Point::new(self.x + dx4, self.y);
                let c2 = Point::new(c1.x + dx5, c1.y + dy5);
                let p = Point::new(c2.x + dx6, start_y);
                self.x = p.x;
                self.y = p.y;
                self.emit(PathCmd::CurveTo(c1, c2, p))?;
            }
            // flex1: the last delta is given on one axis only; which axis is
            // decided by whichever direction the flex travelled further in,
            // and the other coordinate returns to where the flex started.
            37 => {
                let v: [f32; 11] = self.last()?;
                self.sp = 0;
                let (start_x, start_y) = (self.x, self.y);
                let dx = v[0] + v[2] + v[4] + v[6] + v[8];
                let dy = v[1] + v[3] + v[5] + v[7] + v[9];
                self.curve_to([v[0], v[1], v[2], v[3], v[4], v[5]])?;
                let c1 = Point::new(self.x + v[6], self.y + v[7]);
                let c2 = Point::new(c1.x + v[8], c1.y + v[9]);
                let p = if dx.abs() > dy.abs() {
                    Point::new(c2.x + v[10], start_y)
                } else {
                    Point::new(start_x, c2.y + v[10])
                };
                self.x = p.x;
                self.y = p.y;
                self.emit(PathCmd::CurveTo(c1, c2, p))?;
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
    clippy::cast_possible_truncation
)]
mod tests {
    use super::*;

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
        };
        let mut out = Outline::default();
        let mut interp = Interp {
            cff: &cff,
            data: &[],
            local: None,
            stack: [0.0; STACK_LIMIT],
            sp: 0,
            n_stems: 0,
            x: 0.0,
            y: 0.0,
            open: false,
            out: &mut out,
            seac: None,
        };
        interp.run(code, 0).unwrap();
        interp.close_contour();
        out
    }

    fn pt(x: f32, y: f32) -> Point {
        Point::new(x, y)
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
        assert_eq!(plain.commands.first(), Some(&PathCmd::MoveTo(pt(100.0, 200.0))));
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
        assert_eq!(o.commands.iter().filter(|c| **c == PathCmd::Close).count(), 2);
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
        };
        let mut out = Outline::default();
        let mut interp = Interp {
            cff: &cff,
            data: &[],
            local: None,
            stack: [0.0; STACK_LIMIT],
            sp: 0,
            n_stems: 0,
            x: 0.0,
            y: 0.0,
            open: false,
            out: &mut out,
            seac: None,
        };
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
        };
        let mut out = Outline::default();
        let mut interp = Interp {
            cff: &cff,
            data: &[],
            local: None,
            stack: [0.0; STACK_LIMIT],
            sp: 0,
            n_stems: 0,
            x: 0.0,
            y: 0.0,
            open: false,
            out: &mut out,
            seac: None,
        };
        assert_eq!(interp.run(&prog, 0).unwrap_err(), ERR);
        assert!(out.commands.len() <= MAX_COMMANDS + 1);
    }
}
