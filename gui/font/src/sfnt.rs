//! `sfnt` — a TrueType/OpenType font-file parser.
//!
//! This is the first half of real scalable text on SlateOS. Until now the
//! whole system drew text with one procedurally generated 8x16 bitmap face
//! (see the `Font::system_mono` family in this crate's root): every UI
//! rendered at exactly one size, in exactly one typeface, with no kerning
//! and no coverage beyond Latin-1 plus box drawing. `apps/fontmanager`
//! presents a font list that is entirely hard-coded metadata — there was no
//! code anywhere in the tree that could open a `.ttf`.
//!
//! This module opens one. It reads the `sfnt` container, resolves a
//! character to a glyph id through `cmap`, reads horizontal metrics from
//! `hmtx`, and turns a `glyf` entry into a resolution-independent outline
//! (`Outline`) in font units. Turning that outline into pixels is the
//! rasterizer's job — see `raster.rs`.
//!
//! # What is supported
//!
//! * Container: bare TrueType (`0x00010000`, `true`), OpenType with
//!   TrueType outlines (`OTTO` is *detected and rejected*, see below), and
//!   TrueType Collections (`ttcf` — the first face is used).
//! * Tables: `head`, `hhea`, `maxp`, `hmtx`, `loca`, `glyf`, `cmap`.
//! * `cmap` subtable formats 0 (byte), 4 (BMP segmented) and 12 (full
//!   UCS-4 groups), chosen in that order of preference: a format-12 Unicode
//!   subtable wins over format 4, which wins over format 0.
//! * Simple glyphs, including the implied on-curve midpoints between two
//!   consecutive off-curve points, and contours that begin off-curve.
//! * Composite glyphs, including nested composites, with the `WE_HAVE_A_SCALE`
//!   / `X_AND_Y_SCALE` / `TWO_BY_TWO` transforms.
//!
//! # What is not, and why that is an error rather than a silent wrong answer
//!
//! * **CFF/Type2 outlines** (`OTTO`-flavoured `.otf`). Those store outlines
//!   as Type 2 charstrings in a `CFF ` table — a completely different format
//!   (an interpreter with a stack machine, subroutines and hint operators),
//!   not a variant of `glyf`. Parsing it is a separate body of work. A face
//!   with CFF outlines therefore fails to open with
//!   `SfntError::CffOutlinesUnsupported` rather than opening and then
//!   returning empty outlines for every glyph, which would look like a
//!   rendering bug rather than a missing feature.
//! * **Hinting.** `fpgm`/`prep`/glyph instructions are skipped. Modern
//!   rendering at reasonable sizes with anti-aliasing does not need the
//!   TrueType interpreter, and running untrusted bytecode from a font file
//!   is a liability we have no reason to take on.
//! * **`kern`/`GPOS` kerning and `GSUB` shaping.** Advances come from
//!   `hmtx` only. Complex-script shaping is HarfBuzz's job (roadmap item
//!   "2D drawing library: Vello + HarfBuzz"); this module is the font-file
//!   layer that a shaper would sit on top of.
//!
//! # Robustness
//!
//! A font file is untrusted input: it can arrive from a downloaded document
//! or a USB stick. Every read is bounds-checked against the slice it comes
//! from, every offset arithmetic is checked, composite recursion is depth-
//! limited, and no code path panics on malformed input — a bad file yields
//! an `SfntError`, never a crash.

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why a font file could not be parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SfntError {
    /// The data ended before a structure that the format requires.
    TooShort,
    /// The file does not begin with a recognised `sfnt` version tag.
    BadMagic,
    /// A table required for outline rendering is absent.
    MissingTable(&'static str),
    /// A table is present but its contents are inconsistent or truncated.
    MalformedTable(&'static str),
    /// A glyph id was requested that is `>= numGlyphs`.
    GlyphOutOfRange,
    /// No `cmap` subtable in a format we can read.
    UnsupportedCmap,
    /// The face stores outlines as CFF/Type2 charstrings, not `glyf`.
    CffOutlinesUnsupported,
    /// Composite glyphs nest deeper than [`MAX_COMPOSITE_DEPTH`].
    CompositeTooDeep,
}

impl fmt::Display for SfntError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => f.write_str("font data ends prematurely"),
            Self::BadMagic => f.write_str("not an sfnt font (bad version tag)"),
            Self::MissingTable(t) => write!(f, "required table '{t}' is missing"),
            Self::MalformedTable(t) => write!(f, "table '{t}' is malformed"),
            Self::GlyphOutOfRange => f.write_str("glyph id is out of range"),
            Self::UnsupportedCmap => f.write_str("no readable cmap subtable"),
            Self::CffOutlinesUnsupported => {
                f.write_str("font uses CFF/Type2 outlines, which are not supported yet")
            }
            Self::CompositeTooDeep => f.write_str("composite glyph nests too deeply"),
        }
    }
}

/// How deep composite glyphs may nest before we call the file malicious.
///
/// Real fonts nest two levels at most (an accented letter referencing a base
/// letter and an accent, where the accent is itself occasionally composite).
/// The limit exists because a font can trivially be crafted so that glyph A
/// references glyph B which references glyph A.
pub const MAX_COMPOSITE_DEPTH: u8 = 8;

// ---------------------------------------------------------------------------
// Big-endian primitive reads, all bounds-checked
// ---------------------------------------------------------------------------

fn u16_at(d: &[u8], off: usize) -> Option<u16> {
    let end = off.checked_add(2)?;
    let b: [u8; 2] = d.get(off..end)?.try_into().ok()?;
    Some(u16::from_be_bytes(b))
}

fn i16_at(d: &[u8], off: usize) -> Option<i16> {
    let end = off.checked_add(2)?;
    let b: [u8; 2] = d.get(off..end)?.try_into().ok()?;
    Some(i16::from_be_bytes(b))
}

fn u32_at(d: &[u8], off: usize) -> Option<u32> {
    let end = off.checked_add(4)?;
    let b: [u8; 4] = d.get(off..end)?.try_into().ok()?;
    Some(u32::from_be_bytes(b))
}

fn tag_at(d: &[u8], off: usize) -> Option<[u8; 4]> {
    let end = off.checked_add(4)?;
    d.get(off..end)?.try_into().ok()
}

/// Convert a `F2Dot14` fixed-point value (composite-glyph scales) to `f32`.
fn f2dot14(raw: i16) -> f32 {
    f32::from(raw) / 16384.0
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// A point in font units (y grows upward, origin at the baseline pen position).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f32,
    /// Vertical coordinate, positive above the baseline.
    pub y: f32,
}

impl Point {
    /// A point at `(x, y)`.
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// The midpoint of `self` and `other` — the implied on-curve point
    /// between two consecutive off-curve control points.
    #[must_use]
    pub fn midpoint(self, other: Self) -> Self {
        Self {
            x: (self.x + other.x) * 0.5,
            y: (self.y + other.y) * 0.5,
        }
    }
}

/// One step of a glyph outline.
///
/// TrueType outlines are quadratic only; there is no cubic variant here
/// because `glyf` cannot express one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathCmd {
    /// Start a new contour at this point.
    MoveTo(Point),
    /// Straight segment to this point.
    LineTo(Point),
    /// Quadratic bezier: control point, then end point.
    QuadTo(Point, Point),
    /// Close the current contour back to its `MoveTo`.
    Close,
}

/// An affine transform, applied to a composite glyph's component.
///
/// Maps `(x, y)` to `(a*x + c*y + e, b*x + d*y + f)` — the same element
/// order as PostScript/SVG matrices.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// x scale.
    pub a: f32,
    /// y shear.
    pub b: f32,
    /// x shear.
    pub c: f32,
    /// y scale.
    pub d: f32,
    /// x translation.
    pub e: f32,
    /// y translation.
    pub f: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    /// The identity transform.
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// Apply this transform to a point.
    #[must_use]
    pub fn apply(&self, p: Point) -> Point {
        Point {
            x: self.a.mul_add(p.x, self.c.mul_add(p.y, self.e)),
            y: self.b.mul_add(p.x, self.d.mul_add(p.y, self.f)),
        }
    }

    /// `self` followed by `outer` — i.e. `outer ∘ self`.
    ///
    /// Used when a composite component is itself composite: the inner
    /// component's transform must be applied first, then the outer one.
    #[must_use]
    pub fn then(&self, outer: &Self) -> Self {
        Self {
            a: outer.a.mul_add(self.a, outer.c * self.b),
            b: outer.b.mul_add(self.a, outer.d * self.b),
            c: outer.a.mul_add(self.c, outer.c * self.d),
            d: outer.b.mul_add(self.c, outer.d * self.d),
            e: outer.a.mul_add(self.e, outer.c.mul_add(self.f, outer.e)),
            f: outer.b.mul_add(self.e, outer.d.mul_add(self.f, outer.f)),
        }
    }
}

/// An axis-aligned bounding box in font units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BBox {
    /// Left edge.
    pub x_min: f32,
    /// Bottom edge.
    pub y_min: f32,
    /// Right edge.
    pub x_max: f32,
    /// Top edge.
    pub y_max: f32,
}

/// A glyph outline: a list of contours expressed as path commands.
#[derive(Clone, Debug, Default)]
pub struct Outline {
    /// The path, in font units.
    pub commands: Vec<PathCmd>,
}

impl Outline {
    /// True when the glyph draws nothing (e.g. the space character).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// The tight bounding box of every point the path touches, or `None`
    /// for an empty outline.
    ///
    /// Control points are included, so the box can be slightly larger than
    /// the true ink extent — that is the conservative direction, which is
    /// what a rasterizer sizing its buffer needs.
    #[must_use]
    pub fn bbox(&self) -> Option<BBox> {
        let mut b: Option<BBox> = None;
        let mut extend = |p: Point| {
            b = Some(match b {
                None => BBox {
                    x_min: p.x,
                    y_min: p.y,
                    x_max: p.x,
                    y_max: p.y,
                },
                Some(cur) => BBox {
                    x_min: cur.x_min.min(p.x),
                    y_min: cur.y_min.min(p.y),
                    x_max: cur.x_max.max(p.x),
                    y_max: cur.y_max.max(p.y),
                },
            });
        };
        for cmd in &self.commands {
            match *cmd {
                PathCmd::MoveTo(p) | PathCmd::LineTo(p) => extend(p),
                PathCmd::QuadTo(c, p) => {
                    extend(c);
                    extend(p);
                }
                PathCmd::Close => {}
            }
        }
        b
    }

    /// Append `other`, transformed by `t`. Used to assemble composites.
    fn extend_transformed(&mut self, other: &Self, t: &Transform) {
        self.commands.reserve(other.commands.len());
        for cmd in &other.commands {
            self.commands.push(match *cmd {
                PathCmd::MoveTo(p) => PathCmd::MoveTo(t.apply(p)),
                PathCmd::LineTo(p) => PathCmd::LineTo(t.apply(p)),
                PathCmd::QuadTo(c, p) => PathCmd::QuadTo(t.apply(c), t.apply(p)),
                PathCmd::Close => PathCmd::Close,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// The face
// ---------------------------------------------------------------------------

/// Byte range of a table within the font file.
#[derive(Clone, Copy, Debug)]
struct Span {
    off: usize,
    len: usize,
}

/// A `cmap` subtable we know how to read.
#[derive(Clone, Copy, Debug)]
struct CmapSub {
    off: usize,
    format: u16,
}

/// Vertical metrics shared by every glyph in the face, in font units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceMetrics {
    /// Distance from the baseline to the top of the tallest glyph.
    pub ascender: i16,
    /// Distance from the baseline to the lowest descender. Negative, per spec.
    pub descender: i16,
    /// Extra space the designer wants between lines.
    pub line_gap: i16,
    /// The design grid size these values are expressed in.
    pub units_per_em: u16,
}

impl FaceMetrics {
    /// Baseline-to-baseline distance in font units.
    #[must_use]
    pub fn line_height(&self) -> i32 {
        i32::from(self.ascender)
            .saturating_sub(i32::from(self.descender))
            .saturating_add(i32::from(self.line_gap))
    }
}

/// A parsed font face, owning the file bytes it was built from.
///
/// The face owns its data rather than borrowing it because the natural
/// callers — a font cache, a `Font` in this crate — outlive whatever read
/// the file, and a borrowing parser would force every one of them to carry
/// a lifetime parameter for no benefit.
#[derive(Clone, Debug)]
pub struct Face {
    data: Vec<u8>,
    metrics: FaceMetrics,
    num_glyphs: u16,
    num_h_metrics: u16,
    /// `indexToLocFormat`: false = 16-bit `loca` (offsets halved), true = 32-bit.
    loca_long: bool,
    loca: Span,
    glyf: Span,
    hmtx: Span,
    cmap: Option<CmapSub>,
}

impl Face {
    /// Parse a font file.
    ///
    /// # Errors
    ///
    /// Returns [`SfntError`] when the data is not an sfnt font, when a table
    /// required for outline rendering is missing or malformed, or when the
    /// face stores CFF outlines (see the module docs).
    pub fn parse(data: Vec<u8>) -> Result<Self, SfntError> {
        let table_dir = Self::locate_table_directory(&data)?;
        let num_tables = u16_at(&data, table_dir.checked_add(4).ok_or(SfntError::TooShort)?)
            .ok_or(SfntError::TooShort)?;
        let records = table_dir.checked_add(12).ok_or(SfntError::TooShort)?;

        let mut head = None;
        let mut hhea = None;
        let mut maxp = None;
        let mut hmtx = None;
        let mut loca = None;
        let mut glyf = None;
        let mut cmap = None;
        let mut has_cff = false;

        for i in 0..usize::from(num_tables) {
            let rec = records
                .checked_add(i.checked_mul(16).ok_or(SfntError::TooShort)?)
                .ok_or(SfntError::TooShort)?;
            let tag = tag_at(&data, rec).ok_or(SfntError::TooShort)?;
            let off = u32_at(&data, rec.checked_add(8).ok_or(SfntError::TooShort)?)
                .ok_or(SfntError::TooShort)?;
            let len = u32_at(&data, rec.checked_add(12).ok_or(SfntError::TooShort)?)
                .ok_or(SfntError::TooShort)?;
            let off = usize::try_from(off).map_err(|_| SfntError::TooShort)?;
            let len = usize::try_from(len).map_err(|_| SfntError::TooShort)?;
            // A table whose declared extent runs past the file is not usable;
            // skip it rather than trusting the length later.
            if off.checked_add(len).is_none_or(|end| end > data.len()) {
                continue;
            }
            let span = Span { off, len };
            match &tag {
                b"head" => head = Some(span),
                b"hhea" => hhea = Some(span),
                b"maxp" => maxp = Some(span),
                b"hmtx" => hmtx = Some(span),
                b"loca" => loca = Some(span),
                b"glyf" => glyf = Some(span),
                b"cmap" => cmap = Some(span),
                b"CFF " => has_cff = true,
                _ => {}
            }
        }

        // Report the CFF case before the missing-glyf case: "this font uses a
        // format we don't read yet" is actionable, "glyf is missing" is not.
        if glyf.is_none() && has_cff {
            return Err(SfntError::CffOutlinesUnsupported);
        }

        let head = head.ok_or(SfntError::MissingTable("head"))?;
        let hhea = hhea.ok_or(SfntError::MissingTable("hhea"))?;
        let maxp = maxp.ok_or(SfntError::MissingTable("maxp"))?;
        let hmtx = hmtx.ok_or(SfntError::MissingTable("hmtx"))?;
        let loca = loca.ok_or(SfntError::MissingTable("loca"))?;
        let glyf = glyf.ok_or(SfntError::MissingTable("glyf"))?;

        let head_data = data
            .get(head.off..head.off.checked_add(head.len).ok_or(SfntError::TooShort)?)
            .ok_or(SfntError::MalformedTable("head"))?;
        let units_per_em = u16_at(head_data, 18).ok_or(SfntError::MalformedTable("head"))?;
        if units_per_em == 0 {
            // Every metric in the file is a ratio against this; zero would
            // make every scale computation a division by zero.
            return Err(SfntError::MalformedTable("head"));
        }
        let loca_format = i16_at(head_data, 50).ok_or(SfntError::MalformedTable("head"))?;
        let loca_long = match loca_format {
            0 => false,
            1 => true,
            _ => return Err(SfntError::MalformedTable("head")),
        };

        let hhea_data = data
            .get(hhea.off..hhea.off.checked_add(hhea.len).ok_or(SfntError::TooShort)?)
            .ok_or(SfntError::MalformedTable("hhea"))?;
        let ascender = i16_at(hhea_data, 4).ok_or(SfntError::MalformedTable("hhea"))?;
        let descender = i16_at(hhea_data, 6).ok_or(SfntError::MalformedTable("hhea"))?;
        let line_gap = i16_at(hhea_data, 8).ok_or(SfntError::MalformedTable("hhea"))?;
        let num_h_metrics = u16_at(hhea_data, 34).ok_or(SfntError::MalformedTable("hhea"))?;

        let maxp_data = data
            .get(maxp.off..maxp.off.checked_add(maxp.len).ok_or(SfntError::TooShort)?)
            .ok_or(SfntError::MalformedTable("maxp"))?;
        let num_glyphs = u16_at(maxp_data, 4).ok_or(SfntError::MalformedTable("maxp"))?;

        let cmap_sub = match cmap {
            Some(span) => Self::select_cmap(&data, span),
            None => None,
        };

        Ok(Self {
            metrics: FaceMetrics {
                ascender,
                descender,
                line_gap,
                units_per_em,
            },
            num_glyphs,
            num_h_metrics,
            loca_long,
            loca,
            glyf,
            hmtx,
            cmap: cmap_sub,
            data,
        })
    }

    /// Find the offset of the table directory, unwrapping a collection.
    fn locate_table_directory(data: &[u8]) -> Result<usize, SfntError> {
        let tag = tag_at(data, 0).ok_or(SfntError::TooShort)?;
        match &tag {
            // A collection shares glyph data between faces; face 0 is the
            // one a caller that did not ask for an index wants.
            b"ttcf" => {
                let num_fonts = u32_at(data, 8).ok_or(SfntError::TooShort)?;
                if num_fonts == 0 {
                    return Err(SfntError::BadMagic);
                }
                let off = u32_at(data, 12).ok_or(SfntError::TooShort)?;
                usize::try_from(off).map_err(|_| SfntError::TooShort)
            }
            // 0x00010000 is TrueType; 'true' is the old Apple spelling;
            // 'OTTO' is OpenType, whose outlines live in CFF (rejected at
            // table-scan time, with a specific error).
            b"\x00\x01\x00\x00" | b"true" | b"OTTO" => Ok(0),
            _ => Err(SfntError::BadMagic),
        }
    }

    /// Pick the most capable `cmap` subtable the file offers.
    ///
    /// Preference order is by *format*, not by platform: a format-12
    /// subtable maps the whole of Unicode, format 4 only the BMP, format 0
    /// only the first 256 code points. Platform id only breaks ties, where
    /// Windows (3) and Unicode (0) tables are preferred over Macintosh (1)
    /// because the Mac tables use legacy non-Unicode encodings.
    fn select_cmap(data: &[u8], span: Span) -> Option<CmapSub> {
        let num_tables = u16_at(data, span.off.checked_add(2)?)?;
        let mut best: Option<(u8, CmapSub)> = None;
        for i in 0..usize::from(num_tables) {
            let rec = span.off.checked_add(4)?.checked_add(i.checked_mul(8)?)?;
            let platform = u16_at(data, rec)?;
            let encoding = u16_at(data, rec.checked_add(2)?)?;
            let sub_off = u32_at(data, rec.checked_add(4)?)?;
            let sub_off = span.off.checked_add(usize::try_from(sub_off).ok()?)?;
            let format = u16_at(data, sub_off)?;
            let unicode = matches!(platform, 0 | 3);
            // Higher score wins.
            let score: u8 = match (format, unicode) {
                (12, true) => 5,
                (12, false) => 4,
                (4, true) => 3,
                (4, false) => 2,
                (0, _) => 1,
                _ => continue,
            };
            // Platform 3 encoding 0 is "symbol": legitimate, but it maps
            // code points into the F0xx private-use range, so prefer a real
            // Unicode table when both exist.
            let score = if platform == 3 && encoding == 0 {
                score.saturating_sub(1)
            } else {
                score
            };
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((
                    score,
                    CmapSub {
                        off: sub_off,
                        format,
                    },
                ));
            }
        }
        best.map(|(_, sub)| sub)
    }

    /// Vertical metrics for the face, in font units.
    #[must_use]
    pub fn metrics(&self) -> FaceMetrics {
        self.metrics
    }

    /// The design grid size — outline coordinates are in these units.
    #[must_use]
    pub fn units_per_em(&self) -> u16 {
        self.metrics.units_per_em
    }

    /// Number of glyphs in the face.
    #[must_use]
    pub fn num_glyphs(&self) -> u16 {
        self.num_glyphs
    }

    /// True when the face carries a character map we can read.
    #[must_use]
    pub fn has_cmap(&self) -> bool {
        self.cmap.is_some()
    }

    /// The scale factor from font units to pixels at `px_per_em`.
    #[must_use]
    pub fn scale_for_px(&self, px_per_em: f32) -> f32 {
        px_per_em / f32::from(self.metrics.units_per_em)
    }

    /// Map a character to a glyph id, or `None` when the face has no glyph
    /// for it (the caller should fall back to glyph 0, `.notdef`).
    #[must_use]
    pub fn glyph_index(&self, ch: char) -> Option<u16> {
        let sub = self.cmap?;
        let cp = ch as u32;
        let gid = match sub.format {
            0 => self.cmap_format0(sub.off, cp),
            4 => self.cmap_format4(sub.off, cp),
            12 => self.cmap_format12(sub.off, cp),
            _ => None,
        }?;
        // Glyph 0 is `.notdef` — a hit that resolves to it is a miss.
        if gid == 0 || gid >= self.num_glyphs {
            None
        } else {
            Some(gid)
        }
    }

    fn cmap_format0(&self, off: usize, cp: u32) -> Option<u16> {
        if cp > 0xFF {
            return None;
        }
        let idx = off.checked_add(6)?.checked_add(usize::try_from(cp).ok()?)?;
        self.data.get(idx).copied().map(u16::from)
    }

    fn cmap_format4(&self, off: usize, cp: u32) -> Option<u16> {
        if cp > 0xFFFF {
            return None;
        }
        let cp = u16::try_from(cp).ok()?;
        let seg_count_x2 = u16_at(&self.data, off.checked_add(6)?)?;
        let seg_count = usize::from(seg_count_x2.checked_div(2)?);
        let end_codes = off.checked_add(14)?;
        let start_codes = end_codes
            .checked_add(usize::from(seg_count_x2))?
            .checked_add(2)?; // reservedPad
        let id_deltas = start_codes.checked_add(usize::from(seg_count_x2))?;
        let id_range_offsets = id_deltas.checked_add(usize::from(seg_count_x2))?;

        // Segments are sorted by endCode, so a binary search finds the one
        // that can contain `cp` in log time. Fonts routinely have hundreds
        // of segments and this runs per character.
        let mut lo = 0usize;
        let mut hi = seg_count;
        let mut seg = None;
        while lo < hi {
            let mid = lo.checked_add(hi.checked_sub(lo)? / 2)?;
            let end = u16_at(&self.data, end_codes.checked_add(mid.checked_mul(2)?)?)?;
            if cp <= end {
                seg = Some(mid);
                hi = mid;
            } else {
                lo = mid.checked_add(1)?;
            }
        }
        let seg = seg?;
        let seg2 = seg.checked_mul(2)?;
        let start = u16_at(&self.data, start_codes.checked_add(seg2)?)?;
        if cp < start {
            return None;
        }
        let delta = i16_at(&self.data, id_deltas.checked_add(seg2)?)?;
        let range_off_pos = id_range_offsets.checked_add(seg2)?;
        let range_off = u16_at(&self.data, range_off_pos)?;
        let raw = if range_off == 0 {
            cp
        } else {
            // The spec's notorious "pointer arithmetic in a file format":
            // idRangeOffset is a byte offset measured from its own slot.
            let idx = range_off_pos
                .checked_add(usize::from(range_off))?
                .checked_add(usize::from(cp.checked_sub(start)?).checked_mul(2)?)?;
            let g = u16_at(&self.data, idx)?;
            if g == 0 {
                return None;
            }
            g
        };
        Some(raw.wrapping_add_signed(delta))
    }

    fn cmap_format12(&self, off: usize, cp: u32) -> Option<u16> {
        let num_groups = u32_at(&self.data, off.checked_add(12)?)?;
        let num_groups = usize::try_from(num_groups).ok()?;
        let groups = off.checked_add(16)?;
        let mut lo = 0usize;
        let mut hi = num_groups;
        while lo < hi {
            let mid = lo.checked_add(hi.checked_sub(lo)? / 2)?;
            let rec = groups.checked_add(mid.checked_mul(12)?)?;
            let start = u32_at(&self.data, rec)?;
            let end = u32_at(&self.data, rec.checked_add(4)?)?;
            if cp < start {
                hi = mid;
            } else if cp > end {
                lo = mid.checked_add(1)?;
            } else {
                let start_gid = u32_at(&self.data, rec.checked_add(8)?)?;
                let gid = start_gid.checked_add(cp.checked_sub(start)?)?;
                return u16::try_from(gid).ok();
            }
        }
        None
    }

    /// Horizontal advance for a glyph, in font units.
    ///
    /// # Errors
    ///
    /// [`SfntError::GlyphOutOfRange`] when `gid >= num_glyphs`, or
    /// [`SfntError::MalformedTable`] when `hmtx` is too short for the
    /// `numberOfHMetrics` that `hhea` declares.
    pub fn advance(&self, gid: u16) -> Result<u16, SfntError> {
        if gid >= self.num_glyphs {
            return Err(SfntError::GlyphOutOfRange);
        }
        if self.num_h_metrics == 0 {
            return Err(SfntError::MalformedTable("hhea"));
        }
        // Monospaced-tail encoding: the last full metric's advance applies
        // to every glyph past `numberOfHMetrics`, and only their left side
        // bearings are stored individually.
        let idx = gid.min(self.num_h_metrics.saturating_sub(1));
        let off = self
            .hmtx
            .off
            .checked_add(usize::from(idx).checked_mul(4).ok_or(SfntError::TooShort)?)
            .ok_or(SfntError::TooShort)?;
        u16_at(&self.data, off).ok_or(SfntError::MalformedTable("hmtx"))
    }

    /// Left side bearing for a glyph, in font units.
    ///
    /// # Errors
    ///
    /// As [`Face::advance`].
    pub fn left_side_bearing(&self, gid: u16) -> Result<i16, SfntError> {
        if gid >= self.num_glyphs {
            return Err(SfntError::GlyphOutOfRange);
        }
        if self.num_h_metrics == 0 {
            return Err(SfntError::MalformedTable("hhea"));
        }
        let off = if gid < self.num_h_metrics {
            self.hmtx
                .off
                .checked_add(usize::from(gid).checked_mul(4).ok_or(SfntError::TooShort)?)
                .and_then(|o| o.checked_add(2))
        } else {
            // The trailing array of bare bearings begins after the full metrics.
            let full = usize::from(self.num_h_metrics)
                .checked_mul(4)
                .ok_or(SfntError::TooShort)?;
            let extra = usize::from(gid.saturating_sub(self.num_h_metrics))
                .checked_mul(2)
                .ok_or(SfntError::TooShort)?;
            self.hmtx
                .off
                .checked_add(full)
                .and_then(|o| o.checked_add(extra))
        }
        .ok_or(SfntError::TooShort)?;
        i16_at(&self.data, off).ok_or(SfntError::MalformedTable("hmtx"))
    }

    /// The byte range of a glyph's entry in `glyf`, or `None` for a glyph
    /// with no outline (a space, for instance, has a zero-length entry).
    fn glyph_span(&self, gid: u16) -> Result<Option<Span>, SfntError> {
        if gid >= self.num_glyphs {
            return Err(SfntError::GlyphOutOfRange);
        }
        let i = usize::from(gid);
        let (start, end) = if self.loca_long {
            let a = self
                .loca
                .off
                .checked_add(i.checked_mul(4).ok_or(SfntError::TooShort)?)
                .ok_or(SfntError::TooShort)?;
            let s = u32_at(&self.data, a).ok_or(SfntError::MalformedTable("loca"))?;
            let e = u32_at(&self.data, a.checked_add(4).ok_or(SfntError::TooShort)?)
                .ok_or(SfntError::MalformedTable("loca"))?;
            (
                usize::try_from(s).map_err(|_| SfntError::TooShort)?,
                usize::try_from(e).map_err(|_| SfntError::TooShort)?,
            )
        } else {
            let a = self
                .loca
                .off
                .checked_add(i.checked_mul(2).ok_or(SfntError::TooShort)?)
                .ok_or(SfntError::TooShort)?;
            let s = u16_at(&self.data, a).ok_or(SfntError::MalformedTable("loca"))?;
            let e = u16_at(&self.data, a.checked_add(2).ok_or(SfntError::TooShort)?)
                .ok_or(SfntError::MalformedTable("loca"))?;
            // The short format stores halved offsets, which is why a
            // short-loca font can only address 128 KiB of glyph data.
            (usize::from(s).saturating_mul(2), usize::from(e).saturating_mul(2))
        };
        if end <= start {
            return Ok(None);
        }
        let len = end.checked_sub(start).ok_or(SfntError::MalformedTable("loca"))?;
        let off = self
            .glyf
            .off
            .checked_add(start)
            .ok_or(SfntError::MalformedTable("loca"))?;
        if off.checked_add(len).is_none_or(|e| e > self.data.len()) {
            return Err(SfntError::MalformedTable("loca"));
        }
        if len > self.glyf.len {
            return Err(SfntError::MalformedTable("loca"));
        }
        Ok(Some(Span { off, len }))
    }

    /// Extract a glyph's outline in font units.
    ///
    /// An empty outline is a valid result (space, and other blank glyphs).
    ///
    /// # Errors
    ///
    /// [`SfntError::GlyphOutOfRange`] for an unknown glyph id,
    /// [`SfntError::MalformedTable`] when the glyph's data is truncated or
    /// self-inconsistent, [`SfntError::CompositeTooDeep`] when composite
    /// components nest past [`MAX_COMPOSITE_DEPTH`].
    pub fn outline(&self, gid: u16) -> Result<Outline, SfntError> {
        let mut out = Outline::default();
        self.outline_into(gid, &mut out, 0)?;
        Ok(out)
    }

    fn outline_into(&self, gid: u16, out: &mut Outline, depth: u8) -> Result<(), SfntError> {
        if depth > MAX_COMPOSITE_DEPTH {
            return Err(SfntError::CompositeTooDeep);
        }
        let Some(span) = self.glyph_span(gid)? else {
            return Ok(());
        };
        let end = span.off.checked_add(span.len).ok_or(SfntError::TooShort)?;
        let g = self
            .data
            .get(span.off..end)
            .ok_or(SfntError::MalformedTable("glyf"))?;
        let num_contours = i16_at(g, 0).ok_or(SfntError::MalformedTable("glyf"))?;
        if num_contours >= 0 {
            let n = usize::try_from(num_contours).map_err(|_| SfntError::MalformedTable("glyf"))?;
            let body = g.get(10..).ok_or(SfntError::MalformedTable("glyf"))?;
            parse_simple_glyph(body, n, out)
        } else {
            let body = g.get(10..).ok_or(SfntError::MalformedTable("glyf"))?;
            self.parse_composite_glyph(body, out, depth)
        }
    }

    fn parse_composite_glyph(
        &self,
        data: &[u8],
        out: &mut Outline,
        depth: u8,
    ) -> Result<(), SfntError> {
        const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
        const ARGS_ARE_XY_VALUES: u16 = 0x0002;
        const WE_HAVE_A_SCALE: u16 = 0x0008;
        const MORE_COMPONENTS: u16 = 0x0020;
        const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
        const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;

        let mut pos = 0usize;
        loop {
            let flags = u16_at(data, pos).ok_or(SfntError::MalformedTable("glyf"))?;
            let component = u16_at(data, pos.checked_add(2).ok_or(SfntError::TooShort)?)
                .ok_or(SfntError::MalformedTable("glyf"))?;
            pos = pos.checked_add(4).ok_or(SfntError::TooShort)?;

            let (arg1, arg2) = if flags & ARG_1_AND_2_ARE_WORDS == 0 {
                let lo = *data.get(pos).ok_or(SfntError::MalformedTable("glyf"))?;
                let hi = *data
                    .get(pos.checked_add(1).ok_or(SfntError::TooShort)?)
                    .ok_or(SfntError::MalformedTable("glyf"))?;
                pos = pos.checked_add(2).ok_or(SfntError::TooShort)?;
                // Byte args are *signed* when they are offsets.
                #[allow(clippy::cast_possible_wrap)]
                (f32::from(lo as i8), f32::from(hi as i8))
            } else {
                let lo = i16_at(data, pos).ok_or(SfntError::MalformedTable("glyf"))?;
                let hi = i16_at(data, pos.checked_add(2).ok_or(SfntError::TooShort)?)
                    .ok_or(SfntError::MalformedTable("glyf"))?;
                pos = pos.checked_add(4).ok_or(SfntError::TooShort)?;
                (f32::from(lo), f32::from(hi))
            };

            // Point-matching placement (args are point indices, not offsets)
            // is vanishingly rare and needs the parent's point list, which we
            // have already collapsed into path commands. Treat it as no
            // translation rather than mis-placing the component.
            let (tx, ty) = if flags & ARGS_ARE_XY_VALUES == 0 {
                (0.0, 0.0)
            } else {
                (arg1, arg2)
            };

            let mut xform = Transform {
                e: tx,
                f: ty,
                ..Transform::IDENTITY
            };
            if flags & WE_HAVE_A_SCALE != 0 {
                let scale = f2dot14(i16_at(data, pos).ok_or(SfntError::MalformedTable("glyf"))?);
                pos = pos.checked_add(2).ok_or(SfntError::TooShort)?;
                xform.a = scale;
                xform.d = scale;
            } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
                xform.a = f2dot14(i16_at(data, pos).ok_or(SfntError::MalformedTable("glyf"))?);
                xform.d = f2dot14(
                    i16_at(data, pos.checked_add(2).ok_or(SfntError::TooShort)?)
                        .ok_or(SfntError::MalformedTable("glyf"))?,
                );
                pos = pos.checked_add(4).ok_or(SfntError::TooShort)?;
            } else if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
                let read = |k: usize| -> Result<f32, SfntError> {
                    let at = pos
                        .checked_add(k.checked_mul(2).ok_or(SfntError::TooShort)?)
                        .ok_or(SfntError::TooShort)?;
                    Ok(f2dot14(
                        i16_at(data, at).ok_or(SfntError::MalformedTable("glyf"))?,
                    ))
                };
                xform.a = read(0)?;
                xform.b = read(1)?;
                xform.c = read(2)?;
                xform.d = read(3)?;
                pos = pos.checked_add(8).ok_or(SfntError::TooShort)?;
            }

            // Recurse into the component and splice its (transformed) path in.
            // Building the child separately is what makes nested composites
            // work: the child's own component transforms compose naturally
            // because they were already applied when it was built.
            let mut child = Outline::default();
            self.outline_into(
                component,
                &mut child,
                depth.checked_add(1).ok_or(SfntError::CompositeTooDeep)?,
            )?;
            out.extend_transformed(&child, &xform);

            if flags & MORE_COMPONENTS == 0 {
                break;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Simple glyph decoding
// ---------------------------------------------------------------------------

/// A decoded `glyf` point, before it becomes a path command.
#[derive(Clone, Copy)]
struct GlyphPoint {
    p: Point,
    on_curve: bool,
}

/// Read one axis of a simple glyph's delta-encoded coordinates.
///
/// Each point spends 0, 1 or 2 bytes on this axis, selected by two flag
/// bits, and the values are deltas from the previous point:
///
/// * `short` set — one unsigned byte, whose sign comes from `same_or_pos`;
/// * `short` clear and `same_or_pos` set — no bytes at all, delta is zero
///   (the "same as previous coordinate" case, which is why glyphs with long
///   axis-aligned runs compress so well);
/// * both clear — one signed 16-bit delta.
///
/// `pos` is advanced past the bytes consumed so the caller can read the
/// second axis immediately after the first.
fn read_coord_deltas(
    d: &[u8],
    pos: &mut usize,
    flags: &[u8],
    short: u8,
    same_or_pos: u8,
) -> Result<Vec<i32>, SfntError> {
    let mut acc: i32 = 0;
    let mut out = Vec::with_capacity(flags.len());
    for f in flags {
        if f & short != 0 {
            let v = i32::from(*d.get(*pos).ok_or(SfntError::MalformedTable("glyf"))?);
            *pos = pos.checked_add(1).ok_or(SfntError::TooShort)?;
            acc = if f & same_or_pos != 0 {
                acc.checked_add(v)
            } else {
                acc.checked_sub(v)
            }
            .ok_or(SfntError::MalformedTable("glyf"))?;
        } else if f & same_or_pos == 0 {
            let v = i32::from(i16_at(d, *pos).ok_or(SfntError::MalformedTable("glyf"))?);
            *pos = pos.checked_add(2).ok_or(SfntError::TooShort)?;
            acc = acc.checked_add(v).ok_or(SfntError::MalformedTable("glyf"))?;
        }
        out.push(acc);
    }
    Ok(out)
}

fn parse_simple_glyph(d: &[u8], num_contours: usize, out: &mut Outline) -> Result<(), SfntError> {
    const ON_CURVE: u8 = 0x01;
    const X_SHORT: u8 = 0x02;
    const Y_SHORT: u8 = 0x04;
    const REPEAT: u8 = 0x08;
    const X_SAME_OR_POS: u8 = 0x10;
    const Y_SAME_OR_POS: u8 = 0x20;

    if num_contours == 0 {
        return Ok(());
    }

    let mut end_pts = Vec::with_capacity(num_contours);
    for i in 0..num_contours {
        let off = i.checked_mul(2).ok_or(SfntError::TooShort)?;
        end_pts.push(u16_at(d, off).ok_or(SfntError::MalformedTable("glyf"))?);
    }
    // Contours must be non-decreasing; a font that violates this could make
    // the per-contour slicing below produce an empty or inverted range.
    for w in end_pts.windows(2) {
        let (a, b) = (
            w.first().ok_or(SfntError::MalformedTable("glyf"))?,
            w.get(1).ok_or(SfntError::MalformedTable("glyf"))?,
        );
        if b < a {
            return Err(SfntError::MalformedTable("glyf"));
        }
    }
    let last = *end_pts.last().ok_or(SfntError::MalformedTable("glyf"))?;
    let num_points = usize::from(last).checked_add(1).ok_or(SfntError::TooShort)?;

    let mut pos = num_contours.checked_mul(2).ok_or(SfntError::TooShort)?;
    let instr_len = u16_at(d, pos).ok_or(SfntError::MalformedTable("glyf"))?;
    pos = pos
        .checked_add(2)
        .and_then(|p| p.checked_add(usize::from(instr_len)))
        .ok_or(SfntError::TooShort)?;

    // Flags, run-length encoded via the REPEAT bit.
    let mut flags = Vec::with_capacity(num_points);
    while flags.len() < num_points {
        let f = *d.get(pos).ok_or(SfntError::MalformedTable("glyf"))?;
        pos = pos.checked_add(1).ok_or(SfntError::TooShort)?;
        flags.push(f);
        if f & REPEAT != 0 {
            let n = *d.get(pos).ok_or(SfntError::MalformedTable("glyf"))?;
            pos = pos.checked_add(1).ok_or(SfntError::TooShort)?;
            for _ in 0..n {
                if flags.len() >= num_points {
                    break;
                }
                flags.push(f);
            }
        }
    }

    // Coordinates are stored as deltas: the whole x array first, then the
    // whole y array. The two axes use identical encoding with different flag
    // bits, so one routine reads both — keeping them separate invited the two
    // copies to drift.
    let xs = read_coord_deltas(d, &mut pos, &flags, X_SHORT, X_SAME_OR_POS)?;
    let ys = read_coord_deltas(d, &mut pos, &flags, Y_SHORT, Y_SAME_OR_POS)?;

    #[allow(clippy::cast_precision_loss)] // font units fit f32 exactly (|v| < 2^24)
    let points: Vec<GlyphPoint> = flags
        .iter()
        .zip(xs.iter())
        .zip(ys.iter())
        .map(|((f, x), y)| GlyphPoint {
            p: Point::new(*x as f32, *y as f32),
            on_curve: f & ON_CURVE != 0,
        })
        .collect();

    let mut start = 0usize;
    for end in &end_pts {
        let end_idx = usize::from(*end).checked_add(1).ok_or(SfntError::TooShort)?;
        let contour = points
            .get(start..end_idx)
            .ok_or(SfntError::MalformedTable("glyf"))?;
        emit_contour(contour, out);
        start = end_idx;
    }
    Ok(())
}

/// Turn one contour's points into path commands.
///
/// TrueType contours are closed sequences of on- and off-curve points where
/// two consecutive off-curve points imply an on-curve point at their
/// midpoint, and a contour may begin off-curve (in which case the start
/// point is itself implied). Both cases are common in real fonts — Noto and
/// DejaVu both contain contours with no on-curve point at index 0.
fn emit_contour(pts: &[GlyphPoint], out: &mut Outline) {
    let n = pts.len();
    if n == 0 {
        return;
    }
    // `walk` is the contour's points in order, starting just after whichever
    // point anchors the path.
    let (start, walk): (Point, Vec<GlyphPoint>) =
        if let Some(i) = pts.iter().position(|p| p.on_curve) {
            let anchor = pts.get(i).map_or(Point::default(), |p| p.p);
            // Rotate the slice so the walk begins after the anchor and wraps
            // exactly once back to it.
            let walk = pts
                .iter()
                .copied()
                .cycle()
                .skip(i.saturating_add(1))
                .take(n.saturating_sub(1))
                .collect();
            (anchor, walk)
        } else {
            // Every point is a control point: the contour is a closed curve
            // whose start is the implied midpoint of the last and first.
            let first = pts.first().map_or(Point::default(), |p| p.p);
            let last = pts
                .get(n.saturating_sub(1))
                .map_or(Point::default(), |p| p.p);
            (first.midpoint(last), pts.to_vec())
        };

    out.commands.push(PathCmd::MoveTo(start));
    let mut ctrl: Option<Point> = None;
    for pt in walk {
        if pt.on_curve {
            match ctrl.take() {
                Some(c) => out.commands.push(PathCmd::QuadTo(c, pt.p)),
                None => out.commands.push(PathCmd::LineTo(pt.p)),
            }
        } else {
            if let Some(c) = ctrl {
                let mid = c.midpoint(pt.p);
                out.commands.push(PathCmd::QuadTo(c, mid));
            }
            ctrl = Some(pt.p);
        }
    }
    match ctrl {
        Some(c) => out.commands.push(PathCmd::QuadTo(c, start)),
        None => out.commands.push(PathCmd::LineTo(start)),
    }
    out.commands.push(PathCmd::Close);
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
    clippy::float_cmp,
    clippy::too_many_lines
)]
mod tests {
    use super::*;

    /// Build a minimal but *real* TrueType file in memory.
    ///
    /// Tests need a font whose every coordinate is known exactly, which no
    /// shipped font gives us, and the parser must be exercised against real
    /// byte layout rather than a mock. So the tests synthesise one:
    ///
    /// * glyph 0 — `.notdef`, empty
    /// * glyph 1 — 'A' at U+0041: a 100x100 square from (100,0)
    /// * glyph 2 — 'B' at U+0042: a triangle with one off-curve point
    /// * glyph 3 — 'C' at U+0043: a composite placing glyph 1 at +500,+200
    pub(super) fn build_test_font() -> Vec<u8> {
        fn be16(v: u16) -> [u8; 2] {
            v.to_be_bytes()
        }
        fn be16i(v: i16) -> [u8; 2] {
            v.to_be_bytes()
        }
        fn be32(v: u32) -> [u8; 4] {
            v.to_be_bytes()
        }

        // ---- glyf ----------------------------------------------------------
        let mut glyf: Vec<u8> = Vec::new();

        // glyph 0: empty (zero-length loca entry, no bytes emitted)
        let g0 = 0usize;

        // glyph 1: square (100,0) (200,0) (200,100) (100,100), all on-curve
        let g1 = glyf.len();
        glyf.extend_from_slice(&be16i(1)); // numberOfContours
        glyf.extend_from_slice(&be16i(100)); // xMin
        glyf.extend_from_slice(&be16i(0)); // yMin
        glyf.extend_from_slice(&be16i(200)); // xMax
        glyf.extend_from_slice(&be16i(100)); // yMax
        glyf.extend_from_slice(&be16(3)); // endPtsOfContours[0]
        glyf.extend_from_slice(&be16(0)); // instructionLength
        glyf.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]); // flags: ON_CURVE, long coords
        glyf.extend_from_slice(&be16i(100)); // x deltas
        glyf.extend_from_slice(&be16i(100));
        glyf.extend_from_slice(&be16i(0));
        glyf.extend_from_slice(&be16i(-100));
        glyf.extend_from_slice(&be16i(0)); // y deltas
        glyf.extend_from_slice(&be16i(0));
        glyf.extend_from_slice(&be16i(100));
        glyf.extend_from_slice(&be16i(0));

        // glyph 2: (0,0) on, (50,200) OFF, (100,0) on
        let g2 = glyf.len();
        glyf.extend_from_slice(&be16i(1));
        glyf.extend_from_slice(&be16i(0));
        glyf.extend_from_slice(&be16i(0));
        glyf.extend_from_slice(&be16i(100));
        glyf.extend_from_slice(&be16i(200));
        glyf.extend_from_slice(&be16(2));
        glyf.extend_from_slice(&be16(0));
        glyf.extend_from_slice(&[0x01, 0x00, 0x01]);
        glyf.extend_from_slice(&be16i(0));
        glyf.extend_from_slice(&be16i(50));
        glyf.extend_from_slice(&be16i(50));
        glyf.extend_from_slice(&be16i(0));
        glyf.extend_from_slice(&be16i(200));
        glyf.extend_from_slice(&be16i(-200));

        // glyph 3: composite = glyph 1 translated by (500, 200)
        let g3 = glyf.len();
        glyf.extend_from_slice(&be16i(-1)); // composite
        glyf.extend_from_slice(&be16i(600));
        glyf.extend_from_slice(&be16i(200));
        glyf.extend_from_slice(&be16i(700));
        glyf.extend_from_slice(&be16i(300));
        glyf.extend_from_slice(&be16(0x0003)); // ARG_1_AND_2_ARE_WORDS | ARGS_ARE_XY_VALUES
        glyf.extend_from_slice(&be16(1)); // component glyph index
        glyf.extend_from_slice(&be16i(500));
        glyf.extend_from_slice(&be16i(200));
        let g_end = glyf.len();

        // ---- loca (long format) --------------------------------------------
        let mut loca = Vec::new();
        for off in [g0, g1, g2, g3, g_end] {
            loca.extend_from_slice(&be32(u32::try_from(off).unwrap()));
        }

        // ---- head ----------------------------------------------------------
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&be16(1000)); // unitsPerEm
        head[50..52].copy_from_slice(&be16i(1)); // indexToLocFormat = long

        // ---- hhea ----------------------------------------------------------
        let mut hhea = vec![0u8; 36];
        hhea[4..6].copy_from_slice(&be16i(800)); // ascender
        hhea[6..8].copy_from_slice(&be16i(-200)); // descender
        hhea[8..10].copy_from_slice(&be16i(100)); // lineGap
        hhea[34..36].copy_from_slice(&be16(3)); // numberOfHMetrics

        // ---- maxp ----------------------------------------------------------
        let mut maxp = vec![0u8; 6];
        maxp[4..6].copy_from_slice(&be16(4)); // numGlyphs

        // ---- hmtx: 3 full metrics + 1 trailing bearing ----------------------
        let mut hmtx = Vec::new();
        for (adv, lsb) in [(600u16, 0i16), (300, 100), (400, 0)] {
            hmtx.extend_from_slice(&be16(adv));
            hmtx.extend_from_slice(&be16i(lsb));
        }
        hmtx.extend_from_slice(&be16i(25)); // glyph 3's lsb only

        // ---- cmap: one format-4 subtable, platform 3 encoding 1 -------------
        // Segments: 0x41..0x43 -> gids 1..3 (idDelta = 1 - 0x41), then the
        // mandatory 0xFFFF terminator segment.
        let seg_count: u16 = 2;
        let mut sub4 = Vec::new();
        sub4.extend_from_slice(&be16(4)); // format
        sub4.extend_from_slice(&be16(32)); // length (filled below, checked)
        sub4.extend_from_slice(&be16(0)); // language
        sub4.extend_from_slice(&be16(seg_count * 2));
        sub4.extend_from_slice(&be16(4)); // searchRange
        sub4.extend_from_slice(&be16(1)); // entrySelector
        sub4.extend_from_slice(&be16(0)); // rangeShift
        sub4.extend_from_slice(&be16(0x0043)); // endCode[0]
        sub4.extend_from_slice(&be16(0xFFFF)); // endCode[1]
        sub4.extend_from_slice(&be16(0)); // reservedPad
        sub4.extend_from_slice(&be16(0x0041)); // startCode[0]
        sub4.extend_from_slice(&be16(0xFFFF)); // startCode[1]
        sub4.extend_from_slice(&be16i(1i16 - 0x41)); // idDelta[0]
        sub4.extend_from_slice(&be16i(1)); // idDelta[1]
        sub4.extend_from_slice(&be16(0)); // idRangeOffset[0]
        sub4.extend_from_slice(&be16(0)); // idRangeOffset[1]
        let sub4_len = u16::try_from(sub4.len()).unwrap();
        sub4[2..4].copy_from_slice(&be16(sub4_len));

        let mut cmap = Vec::new();
        cmap.extend_from_slice(&be16(0)); // version
        cmap.extend_from_slice(&be16(1)); // numTables
        cmap.extend_from_slice(&be16(3)); // platformID
        cmap.extend_from_slice(&be16(1)); // encodingID
        cmap.extend_from_slice(&be32(12)); // offset to subtable
        cmap.extend_from_slice(&sub4);

        assemble(&[
            (*b"cmap", cmap),
            (*b"glyf", glyf),
            (*b"head", head),
            (*b"hhea", hhea),
            (*b"hmtx", hmtx),
            (*b"loca", loca),
            (*b"maxp", maxp),
        ])
    }

    /// Lay out tables into a valid sfnt container.
    fn assemble(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
        let n = u16::try_from(tables.len()).unwrap();
        let mut out = Vec::new();
        out.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]); // sfnt version
        out.extend_from_slice(&n.to_be_bytes());
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // search hints, unused by us
        let dir_len = 12 + 16 * tables.len();
        let mut offset = dir_len;
        let mut body = Vec::new();
        for (tag, data) in tables {
            out.extend_from_slice(tag);
            out.extend_from_slice(&0u32.to_be_bytes()); // checksum, unchecked
            out.extend_from_slice(&u32::try_from(offset).unwrap().to_be_bytes());
            out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
            body.extend_from_slice(data);
            // Tables must start on a 4-byte boundary.
            let pad = (4 - data.len() % 4) % 4;
            body.extend(core::iter::repeat_n(0u8, pad));
            offset += data.len() + pad;
        }
        out.extend_from_slice(&body);
        out
    }

    fn face() -> Face {
        Face::parse(build_test_font()).expect("synthetic font must parse")
    }

    #[test]
    fn parses_header_and_metrics() {
        let f = face();
        assert_eq!(f.units_per_em(), 1000);
        assert_eq!(f.num_glyphs(), 4);
        let m = f.metrics();
        assert_eq!(m.ascender, 800);
        assert_eq!(m.descender, -200);
        assert_eq!(m.line_gap, 100);
        assert_eq!(m.line_height(), 1100);
    }

    #[test]
    fn rejects_non_font_data() {
        assert_eq!(Face::parse(vec![0u8; 4]).err(), Some(SfntError::BadMagic));
        assert_eq!(Face::parse(Vec::new()).err(), Some(SfntError::TooShort));
    }

    #[test]
    fn cmap_format4_maps_characters() {
        let f = face();
        assert!(f.has_cmap());
        assert_eq!(f.glyph_index('A'), Some(1));
        assert_eq!(f.glyph_index('B'), Some(2));
        assert_eq!(f.glyph_index('C'), Some(3));
        // Outside every segment.
        assert_eq!(f.glyph_index('Z'), None);
        assert_eq!(f.glyph_index('\u{1F600}'), None);
    }

    #[test]
    fn advances_and_bearings_including_the_monospaced_tail() {
        let f = face();
        assert_eq!(f.advance(0).unwrap(), 600);
        assert_eq!(f.advance(1).unwrap(), 300);
        assert_eq!(f.advance(2).unwrap(), 400);
        // Glyph 3 is past numberOfHMetrics: it inherits metric 2's advance
        // but has its own bearing.
        assert_eq!(f.advance(3).unwrap(), 400);
        assert_eq!(f.left_side_bearing(1).unwrap(), 100);
        assert_eq!(f.left_side_bearing(3).unwrap(), 25);
        assert_eq!(f.advance(4), Err(SfntError::GlyphOutOfRange));
    }

    #[test]
    fn empty_glyph_has_no_outline() {
        let f = face();
        assert!(f.outline(0).unwrap().is_empty());
    }

    #[test]
    fn simple_glyph_outline_is_exact() {
        let f = face();
        let o = f.outline(1).unwrap();
        assert_eq!(
            o.commands,
            vec![
                PathCmd::MoveTo(Point::new(100.0, 0.0)),
                PathCmd::LineTo(Point::new(200.0, 0.0)),
                PathCmd::LineTo(Point::new(200.0, 100.0)),
                PathCmd::LineTo(Point::new(100.0, 100.0)),
                PathCmd::LineTo(Point::new(100.0, 0.0)),
                PathCmd::Close,
            ]
        );
        let b = o.bbox().unwrap();
        assert_eq!(b.x_min, 100.0);
        assert_eq!(b.y_max, 100.0);
    }

    #[test]
    fn off_curve_point_becomes_a_quadratic() {
        let f = face();
        let o = f.outline(2).unwrap();
        assert_eq!(
            o.commands,
            vec![
                PathCmd::MoveTo(Point::new(0.0, 0.0)),
                PathCmd::QuadTo(Point::new(50.0, 200.0), Point::new(100.0, 0.0)),
                PathCmd::LineTo(Point::new(0.0, 0.0)),
                PathCmd::Close,
            ]
        );
    }

    #[test]
    fn composite_glyph_translates_its_component() {
        let f = face();
        let o = f.outline(3).unwrap();
        assert_eq!(
            o.commands.first(),
            Some(&PathCmd::MoveTo(Point::new(600.0, 200.0)))
        );
        let b = o.bbox().unwrap();
        assert_eq!(b.x_min, 600.0);
        assert_eq!(b.y_min, 200.0);
        assert_eq!(b.x_max, 700.0);
        assert_eq!(b.y_max, 300.0);
    }

    #[test]
    fn truncation_anywhere_is_an_error_not_a_panic() {
        // Every prefix of a valid font must either parse or fail cleanly.
        let full = build_test_font();
        for cut in 0..full.len() {
            let part = full.get(..cut).unwrap().to_vec();
            if let Ok(f) = Face::parse(part) {
                // A face that still parses must not panic on any glyph.
                for gid in 0..f.num_glyphs() {
                    let _ = f.outline(gid);
                    let _ = f.advance(gid);
                    let _ = f.left_side_bearing(gid);
                }
                for ch in ['A', 'B', 'C', 'Z', '\u{1F600}'] {
                    let _ = f.glyph_index(ch);
                }
            }
        }
    }

    #[test]
    fn byte_corruption_anywhere_is_an_error_not_a_panic() {
        // Flip a byte at every position and confirm no input reaches a panic.
        let full = build_test_font();
        for i in 0..full.len() {
            let mut bad = full.clone();
            if let Some(b) = bad.get_mut(i) {
                *b = b.wrapping_add(0x7F);
            }
            if let Ok(f) = Face::parse(bad) {
                for gid in 0..f.num_glyphs().min(64) {
                    let _ = f.outline(gid);
                    let _ = f.advance(gid);
                }
                for ch in ['A', 'B', 'C', '\u{FFFD}'] {
                    let _ = f.glyph_index(ch);
                }
            }
        }
    }

    #[test]
    fn transform_composition_matches_sequential_application() {
        let inner = Transform {
            a: 2.0,
            b: 0.0,
            c: 0.0,
            d: 3.0,
            e: 10.0,
            f: 20.0,
        };
        let outer = Transform {
            a: 0.0,
            b: 1.0,
            c: -1.0,
            d: 0.0,
            e: 5.0,
            f: -5.0,
        };
        let p = Point::new(7.0, 11.0);
        let composed = inner.then(&outer).apply(p);
        let sequential = outer.apply(inner.apply(p));
        assert!((composed.x - sequential.x).abs() < 1e-4);
        assert!((composed.y - sequential.y).abs() < 1e-4);
    }
}
