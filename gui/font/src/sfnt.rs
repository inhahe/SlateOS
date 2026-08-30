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
//! * Container: bare TrueType (`0x00010000`, `true`), OpenType with either
//!   outline flavour (`OTTO`), and TrueType Collections (`ttcf` — the first
//!   face is used).
//! * Tables: `head`, `hhea`, `maxp`, `hmtx`, `loca`, `glyf`, `cmap`, and
//!   `CFF ` by way of [`cff`](crate::cff).
//! * `cmap` subtable formats 0 (byte), 4 (BMP segmented) and 12 (full
//!   UCS-4 groups), chosen in that order of preference: a format-12 Unicode
//!   subtable wins over format 4, which wins over format 0.
//! * Simple glyphs, including the implied on-curve midpoints between two
//!   consecutive off-curve points, and contours that begin off-curve.
//! * Composite glyphs, including nested composites, with the `WE_HAVE_A_SCALE`
//!   / `X_AND_Y_SCALE` / `TWO_BY_TWO` transforms.
//! * PostScript outlines — Type 2 charstrings in a `CFF ` table. Those are a
//!   different representation entirely (a stack machine with subroutines, not
//!   a point list), so they live in their own module; this one detects the
//!   table, hands it over, and presents the result as the same `Outline`.
//!   See [`cff`](crate::cff) for what that module covers.
//!
//! # What is not, and why that is an error rather than a silent wrong answer
//!
//! * **CFF2** (the variable-font revision of `CFF `). A face carrying one
//!   fails to open with `SfntError::CffUnsupported` rather than being
//!   misparsed as CFF, which it structurally resembles but is not.
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

use crate::device::{Corrections, Ppem};
use crate::gpos::{Adjust, Positioning, Run};
use crate::gsub::{SubGlyph, Substitutions};
use crate::gvar;
use crate::indic_shape::{self, Script};
use crate::kern::Kerning;
use crate::lang::Lang;
use crate::mark::MarkPositioning;
use crate::otl;
use crate::script::ScriptTags;
use crate::var;
use crate::varstore;

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
    /// The face stores outlines in a CFF construct that [`cff`](crate::cff)
    /// deliberately does not guess at — CFF2, Type 1 charstrings, or one of
    /// the Type 2 arithmetic operators. The string names which.
    CffUnsupported(&'static str),
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
            Self::CffUnsupported(what) => write!(f, "unsupported CFF construct: {what}"),
            Self::CompositeTooDeep => f.write_str("composite glyph nests too deeply"),
        }
    }
}

// `core::error::Error` rather than `std::error::Error`: the two are the same
// trait, but naming it through `core` is what lets a caller wrap this in its
// own error type without dragging `std` into this crate (see the crate docs
// on `no_std`).
impl core::error::Error for SfntError {}

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

pub(crate) fn u16_at(d: &[u8], off: usize) -> Option<u16> {
    let end = off.checked_add(2)?;
    let b: [u8; 2] = d.get(off..end)?.try_into().ok()?;
    Some(u16::from_be_bytes(b))
}

pub(crate) fn i16_at(d: &[u8], off: usize) -> Option<i16> {
    let end = off.checked_add(2)?;
    let b: [u8; 2] = d.get(off..end)?.try_into().ok()?;
    Some(i16::from_be_bytes(b))
}

/// A three-byte big-endian code point, which is how `cmap` format 14 spells
/// one — the only place in the format that does.
fn u24_at(d: &[u8], off: usize) -> Option<u32> {
    let end = off.checked_add(3)?;
    let b: [u8; 3] = d.get(off..end)?.try_into().ok()?;
    Some(u32::from_be_bytes([0, b[0], b[1], b[2]]))
}

/// The index of the first of `count` records that does not sort before `base`.
///
/// The records begin at `first`, are `stride` bytes apart, and each opens with
/// a three-byte code point — the shape both halves of a `cmap` format-14
/// selector record have, and both are required to be sorted. Returns `count`
/// when every record sorts before `base`, which the caller must treat as a
/// miss rather than as an index.
///
/// A truncated table reads as "not before", which walks the search toward the
/// low end and leaves the caller to fail the equality check it does anyway.
/// Some faces list fourteen thousand variation sequences, so this is a binary
/// search and not a scan.
fn first_at_or_after(data: &[u8], first: usize, stride: usize, count: usize, base: u32) -> usize {
    let mut lo = 0usize;
    let mut hi = count;
    while lo < hi {
        let mid = lo.saturating_add(hi.saturating_sub(lo) / 2);
        let at = first.saturating_add(mid.saturating_mul(stride));
        if u24_at(data, at).is_some_and(|cp| cp < base) {
            lo = mid.saturating_add(1);
        } else {
            hi = mid;
        }
    }
    lo
}

pub(crate) fn u32_at(d: &[u8], off: usize) -> Option<u32> {
    let end = off.checked_add(4)?;
    let b: [u8; 4] = d.get(off..end)?.try_into().ok()?;
    Some(u32::from_be_bytes(b))
}

pub(crate) fn tag_at(d: &[u8], off: usize) -> Option<[u8; 4]> {
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
/// Both curve orders appear because the two outline formats disagree:
/// `glyf` can only express quadratics, and CFF's Type 2 charstrings can only
/// express cubics. Converting one to the other is either lossy (cubic to
/// quadratic needs subdivision to stay under a tolerance) or pointless
/// (quadratic to cubic inflates every curve for no gain), so the path type
/// carries both and the rasterizer flattens each in its own terms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathCmd {
    /// Start a new contour at this point.
    MoveTo(Point),
    /// Straight segment to this point.
    LineTo(Point),
    /// Quadratic bezier: control point, then end point.
    QuadTo(Point, Point),
    /// Cubic bezier: two control points, then end point. CFF outlines only.
    CurveTo(Point, Point, Point),
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
                PathCmd::CurveTo(c1, c2, p) => {
                    extend(c1);
                    extend(c2);
                    extend(p);
                }
                PathCmd::Close => {}
            }
        }
        b
    }

    /// Move every point right by `dx` font units.
    ///
    /// Used for the one shift a `glyf` outline needs after parsing — see
    /// [`Face::glyf_shift`].
    fn translate_x(&mut self, dx: f32) {
        for cmd in &mut self.commands {
            match cmd {
                PathCmd::MoveTo(p) | PathCmd::LineTo(p) => p.x += dx,
                PathCmd::QuadTo(a, b) => {
                    a.x += dx;
                    b.x += dx;
                }
                PathCmd::CurveTo(a, b, c) => {
                    a.x += dx;
                    b.x += dx;
                    c.x += dx;
                }
                PathCmd::Close => {}
            }
        }
    }

    /// Append `other`, transformed by `t`. Used to assemble composites.
    fn extend_transformed(&mut self, other: &Self, t: &Transform) {
        self.commands.reserve(other.commands.len());
        for cmd in &other.commands {
            self.commands.push(match *cmd {
                PathCmd::MoveTo(p) => PathCmd::MoveTo(t.apply(p)),
                PathCmd::LineTo(p) => PathCmd::LineTo(t.apply(p)),
                PathCmd::QuadTo(c, p) => PathCmd::QuadTo(t.apply(c), t.apply(p)),
                PathCmd::CurveTo(c1, c2, p) => {
                    PathCmd::CurveTo(t.apply(c1), t.apply(c2), t.apply(p))
                }
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
pub(crate) struct Span {
    pub(crate) off: usize,
    pub(crate) len: usize,
}

/// A `cmap` subtable we know how to read.
#[derive(Clone, Copy, Debug)]
struct CmapSub {
    off: usize,
    format: u16,
    /// Platform 3, encoding 0: the table keys on `0xF0xx`, not on Unicode.
    symbol: bool,
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
    /// The widest advance in the face. Declared by `hhea`, so reading it
    /// costs nothing — the alternative is scanning every entry of `hmtx`.
    pub advance_width_max: u16,
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
    outlines: Outlines,
    hmtx: Span,
    cmap: Option<CmapSub>,
    /// Where the `cmap`'s format-14 subtable begins, if it has one — the
    /// Unicode Variation Sequences table, which says which glyph a *pair* of
    /// characters draws as. Kept beside `cmap` rather than as one of its
    /// candidates because it is not an alternative to a format-4 or -12 table
    /// but an addition to it: format 14 maps no single character at all.
    /// `None` for nearly every face. See [`Face::variation_glyph`].
    variations: Option<usize>,
    /// The `name` table, kept as a span rather than decoded at parse time: a
    /// face is opened to draw with, and only a font picker or a family lookup
    /// ever asks for the strings.
    name: Option<Span>,
    /// Where this face sits in its family. Decoded eagerly, unlike `name`,
    /// because it is six bytes at fixed offsets rather than a table walk.
    style: Style,
    /// Pair kerning, from `GPOS` or the legacy `kern` table. `None` for the
    /// many faces — monospace ones especially — that carry none.
    kerning: Option<Kerning>,
    /// Glyph substitution from `GSUB`. `None` for a face with no `GSUB`, or
    /// one whose `GSUB` carries no default-on feature reaching a lookup type
    /// this can apply.
    substitutions: Option<Substitutions>,
    /// Which glyphs this face calls combining marks, from `GPOS` mark coverage
    /// and `GDEF` glyph classes. `None` for the many faces that only ever
    /// expect precomposed characters.
    ///
    /// Kept apart from [`positioning`](Self::positioning) because the two
    /// answer different questions: this one is asked about a bare glyph — *is
    /// this a mark, whose advance must be dropped* — which is a property of the
    /// face, while the pass is asked about a whole run.
    marks: Option<MarkPositioning>,
    /// Every `GPOS` lookup the positioning pass can apply, resolved once per
    /// script the face registers. `None` for a face with no `GPOS`, or one
    /// whose `GPOS` reaches nothing of a type the pass knows.
    positioning: Option<Positioning>,
    /// Whether the file carries a `GPOS` table at all — which is a different
    /// question from whether any of the three things this crate reads out of
    /// it (kerning, `mark`, `mkmk`) is present. See
    /// [`has_positioning`](Face::has_positioning) for why the distinction is
    /// worth a field.
    has_positioning: bool,
    /// Every script tag the `GPOS` ScriptList names, sorted. Empty for a face
    /// with no `GPOS`.
    ///
    /// The *names*, not the selections: [`positioning`](Self::positioning)
    /// records only the scripts that reach a lookup this crate can apply, which
    /// is a narrower set and the wrong one for the one caller here. See
    /// [`gpos_names_script`](Face::gpos_names_script).
    gpos_scripts: Vec<[u8; 4]>,
    /// Every script tag the `GSUB` ScriptList names, sorted. Empty for a face
    /// with no `GSUB`.
    ///
    /// Held for the same reason as [`gpos_scripts`](Self::gpos_scripts) and
    /// with more at stake. Two things read *which* of these tags a run is
    /// shaped under — whether the Indic shaper follows the old Uniscribe spec
    /// or the revised one, and whether it runs at all — and both are answered
    /// wrongly by asking [`substitutions`](Self::substitutions), which records
    /// only the scripts that reached a lookup this crate can apply. `Hack` is
    /// the face that proves it: its `GSUB` registers `DFLT` and `latn`, and
    /// neither one's default language system selects a single feature this
    /// crate asks for, so `substitutions` is `None` outright and the face
    /// appears to name no script at all. HarfBuzz's
    /// `hb_ot_layout_table_select_script` reads the ScriptList and nothing
    /// else, and so does this.
    gsub_scripts: Vec<[u8; 4]>,
    /// The axes this face can vary along, from `fvar`, with `avar`'s correction
    /// folded in. `None` for the 549 of this host's 556 faces that are not
    /// variable, and also for a variable face whose `fvar` is unreadable — a
    /// malformed variation table costs the face its variability, not its
    /// ability to draw.
    ///
    /// Named apart from [`variations`](Self::variations), which is the `cmap`
    /// format-14 Unicode Variation *Sequences* subtable and an unrelated
    /// feature that the specification unhelpfully gave a near-identical name.
    variation_axes: Option<var::Variations>,
    /// The per-glyph outline deltas from `gvar`, if this face has both a
    /// readable `fvar` and a `gvar` that agrees with it about the axes.
    ///
    /// Holds only the table's header and offset array; a glyph's own tuples are
    /// read on demand, because a face varies thousands of glyphs and a run
    /// draws a few dozen.
    gvar: Option<gvar::Gvar>,
    /// The per-glyph advance-width deltas from `HVAR`, on the same terms as
    /// [`gvar`](Self::gvar): present only when `fvar` parsed and the store
    /// agrees with it about the axis count.
    ///
    /// Without this a varied glyph is drawn at its varied shape but spaced at
    /// its default width, which shows up as text growing progressively more
    /// crowded (or gappier) along a line as a weight axis moves.
    hvar: Option<varstore::Hvar>,
    /// The face-wide metric deltas from `MVAR` — ascender, descender, line
    /// gap, x-height, cap height and the underline.
    ///
    /// Distinct from `HVAR` in *what* it varies, identical in *how*: both are
    /// an `ItemVariationStore` reached through a different index.
    mvar: Option<varstore::Mvar>,
    /// `GDEF`'s `ItemVariationStore` — the deltas a `GPOS` `VariationIndex`
    /// points into, which is how a variable face varies its *positions*
    /// (mark anchors, kerns) rather than its shapes or its advances.
    ///
    /// The bare store, not a wrapper like [`Hvar`](varstore::Hvar) or
    /// [`Mvar`](varstore::Mvar), because `GDEF` has no index of its own: the
    /// outer/inner pair arrives in the `VariationIndex`'s own first four bytes,
    /// so there is nothing between the caller and `VarStore::delta`.
    gdef_store: Option<varstore::VarStore>,
}

/// Where a face sits within its family — the axes a font picker selects on.
///
/// Taken from `OS/2`, which is the table that describes the face's design
/// rather than its outlines. Fonts within one family differ only in these,
/// which is what makes them the right key for "give me the bold one".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    /// `usWeightClass`, on the CSS scale: 100 (Thin) to 900 (Black), with 400
    /// regular and 700 bold. Numeric rather than an enum because the scale is
    /// continuous — a family may ship 300, 350 and 400 — and collapsing it to
    /// named steps at parse time would throw away what the selector needs to
    /// choose between them.
    pub weight: u16,
    /// Whether the face is italic or oblique. The two are not distinguished:
    /// no UI in this system offers a choice between them, and many families
    /// label a true italic with the oblique bit anyway.
    pub italic: bool,
    /// `usWidthClass`: 1 (ultra-condensed) to 9 (ultra-expanded), 5 normal.
    ///
    /// Needed because a condensed face is normally a *separate* typographic
    /// family member rather than a separate family, so without this a request
    /// for "Arial" can be answered with Arial Narrow.
    pub width: u8,
}

impl Style {
    /// The weight of an unremarkable text face.
    pub const REGULAR: u16 = 400;
    /// The weight `Weight::Bold` asks for.
    pub const BOLD: u16 = 700;
    /// `usWidthClass` for a face that is not condensed or extended.
    pub const NORMAL_WIDTH: u8 = 5;

    /// What a face claims when it says nothing: upright, regular, normal width.
    const DEFAULT: Self = Self {
        weight: Self::REGULAR,
        italic: false,
        width: Self::NORMAL_WIDTH,
    };
}

impl Default for Style {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Where a face keeps its outlines.
///
/// The two are alternatives, not options: a font has `loca`+`glyf` or it has
/// `CFF `, never both, and the tables of one are meaningless to the other.
/// Making that an enum rather than a pair of `Option` fields is what stops
/// the rest of the parser from having to ask "and what if neither?" at every
/// step.
#[derive(Clone, Debug)]
enum Outlines {
    /// TrueType: `loca` indexes into `glyf`.
    Glyf {
        /// `indexToLocFormat`: false = 16-bit `loca` (offsets halved), true = 32-bit.
        loca_long: bool,
        loca: Span,
        glyf: Span,
    },
    /// PostScript: Type 2 charstrings in `CFF `. Boxed because it is much the
    /// larger of the two variants and every `Face` would otherwise carry its
    /// size.
    Cff(alloc::boxed::Box<crate::cff::Cff>),
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
        let mut name = None;
        let mut os2 = None;
        let mut cff = None;
        let mut gdef = None;
        let mut gpos = None;
        let mut gsub = None;
        let mut kern = None;
        let mut fvar = None;
        let mut avar = None;
        let mut gvar_span = None;
        let mut hvar_span = None;
        let mut mvar_span = None;
        let mut has_cff2 = false;

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
                b"name" => name = Some(span),
                b"OS/2" => os2 = Some(span),
                b"CFF " => cff = Some(span),
                b"GDEF" => gdef = Some(span),
                b"GPOS" => gpos = Some(span),
                b"GSUB" => gsub = Some(span),
                b"kern" => kern = Some(span),
                b"fvar" => fvar = Some(span),
                b"avar" => avar = Some(span),
                b"gvar" => gvar_span = Some(span),
                b"HVAR" => hvar_span = Some(span),
                b"MVAR" => mvar_span = Some(span),
                b"CFF2" => has_cff2 = true,
                _ => {}
            }
        }

        let head = head.ok_or(SfntError::MissingTable("head"))?;
        let hhea = hhea.ok_or(SfntError::MissingTable("hhea"))?;
        let maxp = maxp.ok_or(SfntError::MissingTable("maxp"))?;
        let hmtx = hmtx.ok_or(SfntError::MissingTable("hmtx"))?;

        let head_data = data
            .get(head.off..head.off.checked_add(head.len).ok_or(SfntError::TooShort)?)
            .ok_or(SfntError::MalformedTable("head"))?;
        let units_per_em = u16_at(head_data, 18).ok_or(SfntError::MalformedTable("head"))?;
        if units_per_em == 0 {
            // Every metric in the file is a ratio against this; zero would
            // make every scale computation a division by zero.
            return Err(SfntError::MalformedTable("head"));
        }

        // Which outline format this face uses. `glyf` wins when a file
        // somehow carries both: a `glyf` face is the one whose `loca` and
        // `head.indexToLocFormat` were validated above, so preferring it
        // keeps the two consistent.
        let outlines = if let (Some(loca), Some(glyf)) = (loca, glyf) {
            let loca_format = i16_at(head_data, 50).ok_or(SfntError::MalformedTable("head"))?;
            let loca_long = match loca_format {
                0 => false,
                1 => true,
                _ => return Err(SfntError::MalformedTable("head")),
            };
            Outlines::Glyf {
                loca_long,
                loca,
                glyf,
            }
        } else if let Some(span) = cff {
            Outlines::Cff(alloc::boxed::Box::new(crate::cff::Cff::parse(
                &data,
                span.off,
                span.len,
                units_per_em,
            )?))
        } else if has_cff2 {
            // CFF2 is the variable-font revision of CFF: no Name INDEX, blend
            // operators, an item-variation store. Running it as CFF would
            // misread it rather than fail.
            return Err(SfntError::CffUnsupported("CFF2 table"));
        } else {
            return Err(SfntError::MissingTable("glyf"));
        };

        let hhea_data = data
            .get(hhea.off..hhea.off.checked_add(hhea.len).ok_or(SfntError::TooShort)?)
            .ok_or(SfntError::MalformedTable("hhea"))?;
        let ascender = i16_at(hhea_data, 4).ok_or(SfntError::MalformedTable("hhea"))?;
        let descender = i16_at(hhea_data, 6).ok_or(SfntError::MalformedTable("hhea"))?;
        let line_gap = i16_at(hhea_data, 8).ok_or(SfntError::MalformedTable("hhea"))?;
        let advance_width_max = u16_at(hhea_data, 10).ok_or(SfntError::MalformedTable("hhea"))?;
        let num_h_metrics = u16_at(hhea_data, 34).ok_or(SfntError::MalformedTable("hhea"))?;

        let maxp_data = data
            .get(maxp.off..maxp.off.checked_add(maxp.len).ok_or(SfntError::TooShort)?)
            .ok_or(SfntError::MalformedTable("maxp"))?;
        let num_glyphs = u16_at(maxp_data, 4).ok_or(SfntError::MalformedTable("maxp"))?;

        let cmap_sub = match cmap {
            Some(span) => Self::select_cmap(&data, span),
            None => None,
        };
        let variations = cmap.and_then(|span| Self::find_variation_selectors(&data, span));

        let os2_data = os2.and_then(|s| data.get(s.off..s.off.checked_add(s.len)?));
        let style = Self::parse_style(os2_data, head_data);

        // Eager, unlike `name`: the result is a short list of offsets, and
        // deferring it would mean re-deciding "GPOS or the legacy table?" on
        // every pair of glyphs drawn.
        // `GDEF` comes along for the same reason it does below: a `kern`
        // lookup marked "ignore marks" needs something to have said which
        // glyphs are marks before the flag means anything.
        let kerning = Kerning::parse(&data, gpos, kern, gdef);
        // Same reasoning: a list of subtable offsets, found once, rather than
        // a `GSUB` walk per glyph.
        // `GDEF` comes along because a lookup's flag is expressed in terms of
        // the glyph classes it defines: "ignore marks" means nothing until
        // something has said which glyphs are marks.
        let substitutions = Substitutions::parse(&data, gsub, gdef);
        let marks = MarkPositioning::parse(&data, gpos, gdef);
        // And again: the feature walk is per face, the selection per run.
        let positioning = gpos.and_then(|span| Positioning::parse(&data, span, gdef));
        // Four bytes per script and a handful of scripts per face, so this is
        // cheaper than the offset walk that finds it and is wanted on a path
        // that runs once per shaped run.
        let mut gpos_scripts = gpos
            .and_then(|span| otl::script_tags(&data, span.off))
            .unwrap_or_default();
        gpos_scripts.sort_unstable();
        gpos_scripts.dedup();
        let mut gsub_scripts = gsub
            .and_then(|span| otl::script_tags(&data, span.off))
            .unwrap_or_default();
        gsub_scripts.sort_unstable();
        gsub_scripts.dedup();

        // `avar` without `fvar` is not a lesser variable face, it is nothing at
        // all: `avar` corrects a normalized coordinate per axis, and without
        // `fvar` there are no axes to normalize against. Reached through
        // `and_then` for that reason rather than parsed on its own.
        let variation_axes = fvar.and_then(|span| var::Variations::parse(&data, span, avar));

        // Deliberately gated on `variation_axes` rather than on `fvar`: `gvar`'s
        // tuples are indexed by axis position, so reading it needs the axis
        // *count*, and the count only exists once `fvar` has actually parsed. A
        // face whose `fvar` was rejected therefore also declines `gvar`, which
        // is the right outcome — deltas applied against axes we could not read
        // would deform the glyph rather than vary it.
        let gvar = variation_axes.as_ref().and_then(|v| {
            gvar_span.and_then(|span| gvar::Gvar::parse(&data, span, v.axes().len(), num_glyphs))
        });

        // Gated on `variation_axes` for the same reason `gvar` is, and with an
        // extra edge: an `ItemVariationStore` names its axes only by position,
        // so a store read against the wrong axis count silently pairs axis *k*
        // with a coordinate that meant a different axis. `VarStore::parse`
        // refuses a count mismatch outright rather than reading it anyway.
        let hvar = variation_axes.as_ref().and_then(|v| {
            hvar_span.and_then(|span| varstore::Hvar::parse(&data, span.off, v.axes().len()))
        });
        let mvar = variation_axes.as_ref().and_then(|v| {
            mvar_span.and_then(|span| varstore::Mvar::parse(&data, span.off, v.axes().len()))
        });
        let gdef_store = variation_axes
            .as_ref()
            .and_then(|v| gdef.and_then(|span| Self::gdef_var_store(&data, span, v.axes().len())));

        Ok(Self {
            metrics: FaceMetrics {
                ascender,
                descender,
                line_gap,
                advance_width_max,
                units_per_em,
            },
            num_glyphs,
            num_h_metrics,
            outlines,
            hmtx,
            cmap: cmap_sub,
            variations,
            name,
            style,
            kerning,
            substitutions,
            marks,
            positioning,
            has_positioning: gpos.is_some(),
            gpos_scripts,
            gsub_scripts,
            variation_axes,
            gvar,
            hvar,
            mvar,
            gdef_store,
            data,
        })
    }

    /// `GDEF`'s `ItemVariationStore`, if this `GDEF` is new enough to have one.
    ///
    /// The offset is an `Offset32` at `GDEF + 14`, and — unlike every other
    /// field in that header — it exists only from **version 1.3**. Earlier
    /// `GDEF`s simply stop before it, so reading it unconditionally would
    /// interpret whichever subtable happens to follow the header as an offset
    /// and index the font from an arbitrary place. The version check is the
    /// whole safety of this function; the bounds checks below only stop it
    /// reading off the end once it has already gone wrong.
    fn gdef_var_store(data: &[u8], gdef: Span, axis_count: usize) -> Option<varstore::VarStore> {
        let major = u16_at(data, gdef.off)?;
        let minor = u16_at(data, gdef.off.checked_add(2)?)?;
        if (major, minor) < (1, 3) {
            return None;
        }
        let offset = u32_at(data, gdef.off.checked_add(14)?)?;
        if offset == 0 {
            return None;
        }
        let at = gdef.off.checked_add(usize::try_from(offset).ok()?)?;
        varstore::VarStore::parse(data, at, axis_count)
    }

    /// Decode the face's place in its family from `OS/2`, falling back to
    /// `head`.
    ///
    /// `OS/2` is optional in the spec and genuinely absent from some older and
    /// some Apple-only faces, so `head.macStyle` is the backstop: it carries
    /// only bold and italic flags, which is exactly enough to tell a family's
    /// four legacy members apart. That is why this never fails — a face with
    /// no style information at all is a regular upright one, which is both the
    /// most likely truth and the least damaging guess.
    fn parse_style(os2: Option<&[u8]>, head: &[u8]) -> Style {
        // `macStyle` bit 0 is bold and bit 1 is italic. Read first so it can
        // fill in for whichever `OS/2` field turns out to be unusable.
        let mac_style = u16_at(head, 44).unwrap_or(0);
        let mac_bold = mac_style & 0x0001 != 0;
        let mac_italic = mac_style & 0x0002 != 0;

        let Some(os2) = os2 else {
            return Style {
                weight: if mac_bold {
                    Style::BOLD
                } else {
                    Style::REGULAR
                },
                italic: mac_italic,
                width: Style::NORMAL_WIDTH,
            };
        };

        // `fsSelection` bit 0 is italic and bit 9 is oblique. Both are folded
        // together (see `Style::italic`); `macStyle` covers a face that sets
        // neither but does set the old flag.
        let fs_selection = u16_at(os2, 62).unwrap_or(0);
        let italic = fs_selection & 0x0001 != 0 || fs_selection & 0x0200 != 0 || mac_italic;

        let weight = match u16_at(os2, 4) {
            // Pre-OpenType files used a 1..=9 scale for the same axis, and
            // enough of them survive that reading a `3` as "thinner than
            // Thin" would misfile them. 1..=9 is not a valid CSS weight, so
            // the two ranges cannot be confused.
            Some(w @ 1..=9) => w.saturating_mul(100),
            // Out of range, including the 0 that some generators emit for
            // "unspecified". `macStyle` is the only thing left to go on.
            Some(0) | None => {
                if mac_bold {
                    Style::BOLD
                } else {
                    Style::REGULAR
                }
            }
            Some(w) => w.min(1000),
        };

        // 0 and values above 9 are out of the defined range; a face that
        // cannot describe its width is treated as normal rather than as
        // extremely condensed, which would exclude it from every match.
        let width = u16_at(os2, 6)
            .and_then(|w| u8::try_from(w).ok())
            .filter(|w| (1..=9).contains(w))
            .unwrap_or(Style::NORMAL_WIDTH);

        Style {
            weight,
            italic,
            width,
        }
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
            let symbol = platform == 3 && encoding == 0;
            let score = if symbol {
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
                        symbol,
                    },
                ));
            }
        }
        best.map(|(_, sub)| sub)
    }

    /// Where the `cmap`'s format-14 subtable begins, if the face has one.
    ///
    /// Not part of [`select_cmap`](Self::select_cmap)'s contest, which picks
    /// one subtable to map single characters with. Format 14 maps *pairs* —
    /// a base and a variation selector — and a face that has one always has
    /// an ordinary subtable as well.
    fn find_variation_selectors(data: &[u8], span: Span) -> Option<usize> {
        let num_tables = u16_at(data, span.off.checked_add(2)?)?;
        (0..usize::from(num_tables)).find_map(|i| {
            let rec = span.off.checked_add(4)?.checked_add(i.checked_mul(8)?)?;
            let sub_off = u32_at(data, rec.checked_add(4)?)?;
            let sub_off = span.off.checked_add(usize::try_from(sub_off).ok()?)?;
            (u16_at(data, sub_off)? == 14).then_some(sub_off)
        })
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

    /// A string from the `name` table, or `None` if the face does not carry
    /// that name in an encoding this crate reads.
    ///
    /// See [`name_id`] for the ids worth asking for. Prefer the named
    /// accessors — [`family`](Self::family), [`subfamily`](Self::subfamily),
    /// [`postscript_name`](Self::postscript_name) — which apply the
    /// typographic-versus-legacy preference that a bare id lookup cannot.
    #[must_use]
    pub fn name(&self, name_id: u16) -> Option<String> {
        let span = self.name?;
        let table = self.data.get(span.off..span.off.checked_add(span.len)?)?;
        read_name(table, name_id)
    }

    /// The family this face belongs to — "Inter", "DejaVu Sans".
    ///
    /// Prefers the *typographic* family (name id 16) over the legacy one (id
    /// 1), because the legacy pair exists to fit families into the four-style
    /// regular/italic/bold/bold-italic model that old systems could address:
    /// a face called "Inter SemiBold" in id 1 is "Inter" in id 16, and it is
    /// id 16 that groups a large family the way a font menu should.
    #[must_use]
    pub fn family(&self) -> Option<String> {
        self.name(name_id::TYPOGRAPHIC_FAMILY)
            .or_else(|| self.name(name_id::FAMILY))
    }

    /// The style within the family — "Regular", "Bold Italic", "SemiBold".
    #[must_use]
    pub fn subfamily(&self) -> Option<String> {
        self.name(name_id::TYPOGRAPHIC_SUBFAMILY)
            .or_else(|| self.name(name_id::SUBFAMILY))
    }

    /// The PostScript name — the unique, ASCII, space-free identifier.
    ///
    /// This is the one name in the table specified to be unique across faces
    /// and restricted to printable ASCII, so it is the right key for anything
    /// that has to *identify* a face rather than show it to a person.
    #[must_use]
    pub fn postscript_name(&self) -> Option<String> {
        self.name(name_id::POSTSCRIPT)
    }

    /// Where this face sits in its family: weight, slant and width.
    ///
    /// This is what a selector matches on. It comes from the face's own
    /// `OS/2` table rather than from its [`subfamily`](Self::subfamily)
    /// string, because the string is free text in whatever language the
    /// vendor chose — "Halbfett", "Demi", "65 Medium" — while the numbers are
    /// on one defined scale.
    #[must_use]
    pub const fn style(&self) -> Style {
        self.style
    }

    /// Map a character to a glyph id, or `None` when the face has no glyph
    /// for it (the caller should fall back to glyph 0, `.notdef`).
    #[must_use]
    pub fn glyph_index(&self, ch: char) -> Option<u16> {
        let sub = self.cmap?;
        let cp = ch as u32;
        if let Some(gid) = self.lookup(sub, cp) {
            return Some(gid);
        }
        // A "symbol" table does not key on Unicode: it keys on the byte the
        // character had in the font's own 8-bit encoding, lifted into the
        // private-use area at U+F000. So Wingdings stores its `A` at U+F041
        // and looking up U+0041 finds nothing — which is why fonts like
        // Wingdings and MT Extra used to draw every string as a row of empty
        // boxes even though the glyphs were right there. Retrying in that
        // range is what every other shaper does, and it is safe: a face with
        // a real Unicode table was preferred over this one already, and a
        // character above U+00FF was never in an 8-bit encoding to begin
        // with.
        if sub.symbol && cp <= 0xFF {
            return self.lookup(sub, 0xF000_u32.checked_add(cp)?);
        }
        None
    }

    /// The glyph a base character draws as when `selector` follows it, or
    /// `None` when the face knows nothing about that pair.
    ///
    /// A variation sequence is two characters that name one glyph: U+1000
    /// U+FE00 is "Myanmar ka, second form", and `mmrtext.ttf` draws it with a
    /// glyph its ordinary `cmap` has no entry for at all. `None` is the answer
    /// for nearly every pair, and the caller's job then is to draw the two
    /// characters as it would have anyway — the selector is a default
    /// ignorable, so it comes out as nothing.
    ///
    /// Three outcomes in the table and two here, which is HarfBuzz's
    /// `get_variation_glyph`: a pair listed in the *default* half means "the
    /// base's ordinary glyph is already right", and answering with that glyph
    /// says the same thing as the table while still telling the caller the
    /// pair was recognised — which matters, because a recognised pair is one
    /// glyph and an unrecognised one is two.
    #[must_use]
    pub fn variation_glyph(&self, base: char, selector: char) -> Option<u16> {
        let off = self.variations?;
        let count = u32_at(&self.data, off.checked_add(6)?)?;
        let base = base as u32;
        // A linear scan, not a binary search: the record count is the number
        // of *selectors* the face supports, which is 1 in every face seen and
        // 259 in the worst case the standard allows.
        let record = (0..usize::try_from(count).ok()?).find_map(|i| {
            let rec = off.checked_add(10)?.checked_add(i.checked_mul(11)?)?;
            (u24_at(&self.data, rec)? == selector as u32).then_some(rec)
        })?;
        // The default half: an inclusive range list, each entry a start and a
        // count of *additional* code points after it. Found by searching for
        // the first range that starts *after* the base and stepping back one,
        // since the range that could contain it is the last one before it.
        if let Some(table) = self.uvs_half(record.checked_add(3)?)
            && let Some(ranges) = u32_at(&self.data, table)
            && let Some(ranges) = usize::try_from(ranges).ok()
            && let Some(at) = first_at_or_after(
                &self.data,
                table.checked_add(4)?,
                4,
                ranges,
                base.saturating_add(1),
            )
            .checked_sub(1)
            .and_then(|i| table.checked_add(4)?.checked_add(i.checked_mul(4)?))
            && let Some(start) = u24_at(&self.data, at)
            && let Some(&extra) = self.data.get(at.checked_add(3)?)
            && base <= start.saturating_add(u32::from(extra))
        {
            return self.glyph_index(char::from_u32(base)?);
        }
        // The non-default half: an explicit code point to glyph mapping.
        let table = self.uvs_half(record.checked_add(7)?)?;
        let mappings = usize::try_from(u32_at(&self.data, table)?).ok()?;
        let first = table.checked_add(4)?;
        let i = first_at_or_after(&self.data, first, 5, mappings, base);
        if i >= mappings {
            return None;
        }
        let at = first.checked_add(i.checked_mul(5)?)?;
        if u24_at(&self.data, at) != Some(base) {
            return None;
        }
        let gid = u16_at(&self.data, at.checked_add(3)?)?;
        (gid != 0 && gid < self.num_glyphs).then_some(gid)
    }

    /// Where one half of a format-14 selector record points, or `None` when
    /// that half is absent — which the format spells as a zero offset, and a
    /// zero offset is a real one everywhere else in `sfnt`, so it cannot be
    /// left for the caller to remember.
    fn uvs_half(&self, at: usize) -> Option<usize> {
        let off = self.variations?;
        let rel = u32_at(&self.data, at).filter(|&o| o != 0)?;
        off.checked_add(usize::try_from(rel).ok()?)
    }

    /// Whether the face's `cmap` carries any variation sequences at all.
    ///
    /// Lets the shaper skip the pass that looks for them, which is nearly
    /// every shaped run: a face with a format-14 subtable is rare, and text
    /// with a variation selector in it rarer still.
    #[must_use]
    pub fn has_variation_sequences(&self) -> bool {
        self.variations.is_some()
    }

    fn lookup(&self, sub: CmapSub, cp: u32) -> Option<u16> {
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

    /// Horizontal advance for a glyph at a variable-font instance, in font
    /// units.
    ///
    /// The `hmtx` advance corrected by `HVAR`. Identical to
    /// [`advance`](Self::advance) when the face does not vary, carries no
    /// `HVAR`, or is asked for the default instance — the last because the
    /// default instance's advance *is* what `hmtx` stores, so every delta is
    /// zero there by construction.
    ///
    /// The result is clamped at zero rather than allowed to wrap: a negative
    /// advance would drag the rest of the line backwards over the glyph, which
    /// is a far more visible failure than one glyph being a shade too narrow.
    ///
    /// # Errors
    ///
    /// As [`advance`](Self::advance). An `HVAR` that names no row for this
    /// glyph is *not* an error — it contributes zero, which is the common case
    /// for a font's unmapped tail.
    pub fn advance_at(&self, gid: u16, coords: &var::Coords) -> Result<u16, SfntError> {
        let base = self.advance(gid)?;
        let Some(hvar) = self.hvar.as_ref() else {
            return Ok(base);
        };
        if coords.is_default() {
            return Ok(base);
        }
        let delta = hvar.advance_delta(&self.data, gid, coords.as_slice());
        Ok(
            u16::try_from(i32::from(base).saturating_add(i32::from(delta)).max(0))
                .unwrap_or(u16::MAX),
        )
    }

    /// The correction `MVAR` applies to the face-wide metric `tag` at `coords`,
    /// in font units.
    ///
    /// Zero when the face has no `MVAR`, does not carry that tag, or is at its
    /// default instance. Tags are the four-byte names the `MVAR` specification
    /// defines: `hasc`, `hdsc`, `hlgp`, `xhgt`, `cpht`, `undo` and so on.
    #[must_use]
    pub fn metric_delta(&self, tag: [u8; 4], coords: &var::Coords) -> i16 {
        if coords.is_default() {
            return 0;
        }
        self.mvar
            .as_ref()
            .map_or(0, |m| m.metric_delta(&self.data, tag, coords.as_slice()))
    }

    /// The face's vertical metrics at a variable-font instance.
    ///
    /// Only the three `hhea`-derived numbers vary through `MVAR`; the units
    /// per em is a design constant and the maximum advance is a bound over the
    /// whole face, which `MVAR` has no tag for.
    #[must_use]
    pub fn metrics_at(&self, coords: &var::Coords) -> FaceMetrics {
        let mut m = self.metrics;
        m.ascender = m
            .ascender
            .saturating_add(self.metric_delta(*b"hasc", coords));
        m.descender = m
            .descender
            .saturating_add(self.metric_delta(*b"hdsc", coords));
        m.line_gap = m
            .line_gap
            .saturating_add(self.metric_delta(*b"hlgp", coords));
        m
    }

    /// How much to add to `left`'s advance when `right` follows it, in font
    /// units. Negative pulls the pair closer, which is the common case.
    ///
    /// Zero is the answer for the overwhelming majority of pairs, and for
    /// every pair in a face that carries no kerning at all — so this is an
    /// adjustment to an advance, never a replacement for one.
    ///
    /// Infallible on purpose. A malformed kerning table means text that is
    /// spaced slightly wrong, which is not worth failing a draw over when the
    /// alternative is drawing nothing.
    #[must_use]
    pub fn kern(&self, left: u16, right: u16) -> i16 {
        self.kern_across(left, right, &[])
    }

    /// The same, for a pair with `between` standing between them in the run.
    ///
    /// Real faces mark their kerning lookups "ignore marks" precisely so that
    /// `A` and `V` keep kerning with an accent between them. Only a lookup
    /// whose flag would have let it see past every glyph in `between` is
    /// consulted, so a pair separated by a *letter* is still not kerned.
    ///
    /// This is the answer at the face's design size, because a face has no
    /// other size to answer at. A pair whose `GPOS` record carries a device
    /// table is kerned slightly differently once a size is known — see
    /// [`ScaledFont::kern_across`](crate::scaled::ScaledFont::kern_across),
    /// which is the one that matches how the text will actually be drawn.
    #[must_use]
    pub fn kern_across(&self, left: u16, right: u16, between: &[u16]) -> i16 {
        self.kern_across_at(left, right, between, Corrections::NONE)
    }

    /// The same, at a known size and instance, so that device-table and
    /// `VariationIndex` corrections apply.
    pub(crate) fn kern_across_at(
        &self,
        left: u16,
        right: u16,
        between: &[u16],
        corr: Corrections<'_>,
    ) -> i16 {
        self.kerning
            .as_ref()
            .map_or(0, |k| k.pair(&self.data, left, right, between, corr))
    }

    /// What `GPOS`'s device tables and variation indices should be read with
    /// when this face is drawn at `px_per_em` at instance `coords`.
    ///
    /// Here rather than in [`device`](crate::device) because both halves are
    /// the face's to supply: the em a pixel correction is converted back
    /// through, and the `GDEF` store a `VariationIndex` points into.
    ///
    /// The store is withheld at the default instance. Every delta there
    /// evaluates to zero anyway — a region's scalar is a product of factors
    /// that are all zero at coordinate zero — so this is the same answer
    /// reached without walking the store, and it keeps the non-variable path
    /// free of variable-font work.
    pub(crate) fn corrections<'a>(
        &'a self,
        px_per_em: f32,
        coords: &'a var::Coords,
    ) -> Corrections<'a> {
        let ppem = Ppem::new(px_per_em, self.metrics.units_per_em);
        if coords.is_default() {
            return Corrections::at(ppem);
        }
        Corrections::varying(ppem, self.gdef_store.as_ref(), coords.as_slice())
    }

    /// Whether this face carries any pair kerning this can read.
    ///
    /// Exposed for diagnostics and for tests that need to tell "kerned to
    /// zero" apart from "has no kerning".
    #[must_use]
    pub fn has_kerning(&self) -> bool {
        self.kerning.is_some()
    }

    /// Whether this face's kerning lives somewhere [`position`](Self::position)
    /// cannot reach — that is, in the legacy `kern` table.
    ///
    /// A shaper that runs the positioning pass must ask before also walking
    /// pairs through [`kern_across`](Self::kern_across): a `GPOS` face's pairs
    /// have already been charged by the pass, and charging them again would
    /// double every kern.
    #[must_use]
    pub fn kerns_outside_gpos(&self) -> bool {
        self.kerning.as_ref().is_some_and(Kerning::is_legacy)
    }

    /// Apply this face's `GSUB` substitutions to `glyphs`, in place.
    ///
    /// The whole run goes in at once, because that is the unit a `GSUB` lookup
    /// applies to: each lookup runs across all of it before the next begins,
    /// and a caller that fed the run in position by position would get a
    /// different — wrong — answer. A run may come out *shorter* than it went
    /// in, where glyphs ligated.
    ///
    /// The caller decides what one run is, and so where a substitution may
    /// not reach: a tab, a style change and a bidi run boundary are all
    /// expressed by passing the pieces separately.
    ///
    /// `script` says which of the face's features apply. It matters because a
    /// tag is not unique — a face supporting both Arabic and Latin has two
    /// features called `liga`, meaning entirely different things — so a run
    /// shaped under the wrong script can be rewritten by rules written for
    /// another writing system. `None` asks for the face's default features,
    /// which is the right answer for a run of digits and punctuation and the
    /// only one available for a caller holding bare glyph ids.
    ///
    /// `lang` chooses among the rules that script offers: a face may spell
    /// Turkish, Serbian or Romanian differently from the rest of the writing
    /// system, and this is how it is asked to. `None` — and any language the
    /// chosen script does not register — takes that script's default rules,
    /// which is what a caller who does not know the language should pass; a
    /// *wrong* language is worse than none. See [`lang`](crate::lang).
    ///
    /// An Indic run is not substituted but *shaped*: the same lookups run, but
    /// in thirteen stages with a reordering between two of them, because the
    /// order Indic text is stored in is not the order it is drawn in. That path
    /// runs even in a face with no `GSUB`, since moving a left matra in front
    /// of its consonant is this crate's job rather than the font's.
    pub fn substitute(
        &self,
        script: Option<ScriptTags>,
        lang: Option<Lang>,
        glyphs: &mut Vec<SubGlyph>,
    ) {
        let subs = self.substitutions.as_ref();
        let chosen = self.gsub_chosen_script(script);
        // Khmer is asked about first and without the `shaped_as_default`
        // filter, because `khmr` is in
        // [`fallback::ALWAYS_COMPLEX`](crate::fallback) — a face that files its
        // features under `latn` does not get to call Khmer's reordering off,
        // since text with an unmoved pre-base vowel is not plainer but wrong.
        // The two arms are exclusive: no script tag is both.
        if crate::khmer::shapes(script) {
            crate::khmer::shape(&self.data, subs, script, lang, glyphs, |ch| {
                self.glyph_index(ch)
            });
            return;
        }
        // Myanmar *is* filtered, unlike Khmer: `mym2` is a complex script but
        // not an always-complex one, so a face that files its features under
        // `DFLT` or `latn` is taken at its word and the run is shaped plainly.
        // That is HarfBuzz's rule and this crate's, and the filter has to be
        // the same one the Indic arm below uses or the two would disagree
        // about the same face.
        if crate::myanmar::shapes(script) && !crate::fallback::shaped_as_default(script, chosen) {
            crate::myanmar::shape(&self.data, subs, script, lang, glyphs, |ch| {
                self.glyph_index(ch)
            });
            return;
        }
        // Then the Universal Shaping Engine, which takes every complex script
        // the three specialised shapers above and the Indic arm below do not:
        // eighty-eight of them, from Balinese to Zanabazar Square. Same filter
        // as Myanmar and Indic — a face that files its features under `DFLT` or
        // `latn` has said it wants none of this. The arms stay exclusive: the
        // USE tag list is `fallback::COMPLEX_SCRIPTS` with the other shapers'
        // tags removed, which is checked by a test in `universal`.
        if crate::universal::shapes(script) && !crate::fallback::shaped_as_default(script, chosen) {
            crate::universal::shape(&self.data, subs, script, lang, glyphs, |ch| {
                self.glyph_index(ch)
            });
            return;
        }
        match Script::shaping(script)
            .filter(|_| !crate::fallback::shaped_as_default(script, chosen))
        {
            Some(indic) => {
                indic_shape::shape(
                    &self.data,
                    subs,
                    script,
                    lang,
                    chosen,
                    indic,
                    glyphs,
                    |ch| self.glyph_index(ch),
                );
            }
            None => {
                if let Some(subs) = subs {
                    subs.apply(&self.data, script, lang, glyphs);
                }
            }
        }
    }

    /// Whether a run of `script` is shaped by the default shaper even though
    /// its script asks for a complex one, because this face files its `GSUB`
    /// features under `DFLT` or `latn`.
    ///
    /// The face's half of [`fallback::shaped_as_default`](crate::fallback::shaped_as_default),
    /// which is where the reasoning is. Three callers, and they have to agree:
    /// whether the Indic shaper runs at all, whether the run's marks may be
    /// placed by measurement, and whether their advances are zeroed are three
    /// fields of one HarfBuzz shaper struct, so answering them from different
    /// premises would produce a combination no shaper implements.
    ///
    /// Asked per run rather than cached for the same reason as
    /// [`applies_gpos`](crate::scaled::ScaledFont): the answer depends on the
    /// run, so the face has nowhere to put it, and it costs a walk of a
    /// five-tag fallback chain against a sorted script list.
    #[must_use]
    pub fn shapes_as_default(&self, script: Option<ScriptTags>) -> bool {
        crate::fallback::shaped_as_default(script, self.gsub_chosen_script(script))
    }

    /// Which of this face's `GSUB` script tags a run of `script` is shaped
    /// under, or `None` when the face names none of the ones it would accept.
    ///
    /// Three callers, and all ask because the *tag* carries meaning beyond
    /// which features it selects: [`shapes_as_default`](Self::shapes_as_default),
    /// where `DFLT` or `latn` says the designer wrote no complex shaping; the
    /// Indic shaper, where `deva` and `dev2` are two different specs; and the
    /// Thai private-use fallback, which runs exactly when the answer is *not*
    /// `thai` — including for a face with no `GSUB` at all, which is why that
    /// caller cannot use `shapes_as_default`. See
    /// [`otl::chosen_from`](crate::otl::chosen_from).
    #[must_use]
    pub(crate) fn gsub_chosen_script(&self, script: Option<ScriptTags>) -> Option<[u8; 4]> {
        otl::chosen_from(&self.gsub_scripts, script)
    }

    /// Whether this face carries any `GSUB` substitution this can apply.
    ///
    /// Exposed for the same reason as [`Face::has_kerning`]: to tell "nothing
    /// to substitute in this run" apart from "this face substitutes nothing at
    /// all", and to let a caller skip the pass entirely.
    #[must_use]
    pub fn has_substitutions(&self) -> bool {
        self.substitutions.is_some()
    }

    /// Whether `glyph` is a combining mark — drawn onto what precedes it
    /// rather than after it.
    ///
    /// `false` for every glyph in a face that says nothing about marks at
    /// all, which is the right answer there: with neither `GDEF` classes nor
    /// anchors there is no way to tell a mark from a letter, and treating it
    /// as a letter at least advances the pen instead of stacking the run on
    /// one spot.
    #[must_use]
    pub fn is_mark(&self, glyph: u16) -> bool {
        self.marks
            .as_ref()
            .is_some_and(|m| m.is_mark(&self.data, glyph))
    }

    /// Whether the face's `GDEF` sorts its glyphs into classes.
    ///
    /// Narrower than [`has_marks`](Self::has_marks), which is also true of a
    /// face that has mark anchors and no `GlyphClassDef`. This is the question
    /// that decides whether the shaper may believe the *face* about which
    /// glyphs are marks, or has to work it out from the characters —
    /// HarfBuzz's `fallback_glyph_classes`, `!hb_ot_layout_has_glyph_classes`.
    #[must_use]
    pub(crate) fn classifies_glyphs(&self) -> bool {
        self.marks.as_ref().is_some_and(MarkPositioning::classifies)
    }

    /// Position one run of glyphs with this face's `GPOS`.
    ///
    /// One adjustment per glyph, in font units: what its advance became and how
    /// far its image sits from the pen. Every lookup the run's script reaches is
    /// applied, in the order the table lists them — see [`gpos`](crate::gpos)
    /// for why that has to be one pass rather than one pass per effect.
    ///
    /// `None` when the face has no `GPOS` the pass can use, which leaves the
    /// caller with the nominal advances it already had.
    #[must_use]
    pub(crate) fn position(&self, run: &Run<'_>) -> Option<Vec<Adjust>> {
        Some(self.positioning.as_ref()?.apply(&self.data, run))
    }

    /// Whether this face has any `GPOS` lookup the positioning pass can apply.
    ///
    /// Lets a shaper skip building the pass's inputs — an advance and a
    /// mark flag per glyph — for the faces that would do nothing with them.
    #[must_use]
    pub(crate) fn has_gpos_lookups(&self) -> bool {
        self.positioning.is_some()
    }

    /// Whether the face ships a legacy `kern` table this can read, whatever
    /// `GPOS` also offers.
    ///
    /// Deliberately not the same question as
    /// [`kerns_outside_gpos`](Self::kerns_outside_gpos), which asks whether the
    /// legacy table is the one the *pair-at-a-time* interface reads and so
    /// answers `false` the moment `GPOS` carries any `kern` feature at all. A
    /// shaper has a run, and a run has a script; the two halves of its decision
    /// are this and [`gpos_kerns`](Self::gpos_kerns).
    #[must_use]
    pub(crate) fn has_legacy_kern(&self) -> bool {
        self.kerning.as_ref().is_some_and(Kerning::has_legacy)
    }

    /// Whether a run of `script` in `lang` reaches a `kern` feature in this
    /// face's `GPOS`.
    ///
    /// A face files its `GPOS` features under particular scripts, so this is a
    /// question about the run and not about the face: Leelawadee registers only
    /// `thai`, so its Latin text reaches no `GPOS` kerning at all even though
    /// the table plainly has some, and must be kerned from the legacy table
    /// instead. HarfBuzz makes the same call the same way — `apply_kern` is
    /// switched on by `!has_gpos_kern`, and `has_gpos_kern` is looked up in the
    /// shaping plan's *selected* script.
    #[must_use]
    pub(crate) fn gpos_kerns(&self, script: Option<ScriptTags>, lang: Option<Lang>) -> bool {
        self.positioning
            .as_ref()
            .is_some_and(|gpos| gpos.kerns(script, lang))
    }

    /// The legacy `kern` table's adjustment to `left`'s advance when `right`
    /// follows it, ignoring whatever `GPOS` says about the pair.
    ///
    /// What a run that [`has_legacy_kern`](Self::has_legacy_kern) and no
    /// [`gpos_kerns`](Self::gpos_kerns) needs. Distinct from
    /// [`kern_across`](Self::kern_across), which prefers `GPOS` face-wide and
    /// so would answer 0 for exactly the faces this exists for.
    #[must_use]
    pub(crate) fn legacy_kern_across(&self, left: u16, right: u16, between: &[u16]) -> i16 {
        self.kerning
            .as_ref()
            .map_or(0, |k| k.legacy_pair(&self.data, left, right, between))
    }

    /// Whether this face can tell a combining mark from a letter — because it
    /// carries `GPOS` mark anchors, `GDEF` glyph classes, or both.
    ///
    /// A shaper uses this to skip the mark pass entirely on the majority of
    /// faces that have nothing to say, rather than paying a per-glyph
    /// [`is_mark`](Self::is_mark) for an answer that is always `false`.
    #[must_use]
    pub fn has_marks(&self) -> bool {
        self.marks.is_some()
    }

    /// Whether the face carries a `GPOS` table at all.
    ///
    /// Not the same question as [`has_marks`](Self::has_marks) or
    /// [`has_kerning`](Self::has_kerning), and the difference decides whether a
    /// shaper may invent placements. A face *with* `GPOS` has been through a
    /// designer who chose what to position and what to leave alone: if it
    /// carries no `mark` feature, that is a statement, not an omission, and
    /// synthesizing accent placement there would fight the design and collide
    /// with the `GPOS` lookups this crate does not read yet. A face with no
    /// `GPOS` at all has made no such statement — nothing in it can place a
    /// combining mark, so a mark left at the pen is simply wrong, and a
    /// synthesized position is the best answer available.
    ///
    /// This is the same line HarfBuzz draws (`hb_ot_layout_has_positioning`),
    /// which matters because HarfBuzz is what this crate's shaping is checked
    /// against — see `gui/font/tools/harfbuzz_sweep.py`.
    #[must_use]
    pub fn has_positioning(&self) -> bool {
        self.has_positioning
    }

    /// The face's variation axes, or `None` if it is not a variable font.
    ///
    /// Returning the whole [`Variations`](var::Variations) rather than just the
    /// axis list is deliberate: normalizing a user coordinate needs `avar`'s
    /// correction as well as the axis bounds, and splitting the two across two
    /// accessors invites a caller to normalize with only half of them -- which
    /// produces a plausible number that is wrong by exactly the amount the
    /// designer added `avar` to fix.
    ///
    /// The face itself is *not* at any instance. A chosen position along these
    /// axes belongs to the scaled font, so that one parsed face can serve two
    /// weights at once without being re-read.
    #[must_use]
    pub fn variation_axes(&self) -> Option<&var::Variations> {
        self.variation_axes.as_ref()
    }

    /// Whether the face's `GPOS` ScriptList names `tag` itself.
    ///
    /// Deliberately not "would a run of `tag` reach any lookup": no fallback
    /// chain is followed, and a script table with no usable lookups still
    /// counts. The caller is [`fallback::demands_own_gpos_script`], which asks
    /// on behalf of a script whose shaper refuses a face's `GPOS` unless the
    /// face was written with that script in mind — and a face that names the
    /// script was, whatever it then chose to do about it.
    ///
    /// [`fallback::demands_own_gpos_script`]:
    ///     crate::fallback::demands_own_gpos_script
    #[must_use]
    pub(crate) fn gpos_names_script(&self, tag: &[u8; 4]) -> bool {
        self.gpos_scripts.binary_search(tag).is_ok()
    }

    /// The glyph's ink box in font units, or `None` if it cannot be read.
    ///
    /// Distinct from [`Outline::bbox`] on the outline this face would hand
    /// back: for a `glyf` face this is the box the font *states* in the
    /// glyph's own header, which is the tight box around the curves, whereas
    /// walking the outline yields the hull of the control points and so a
    /// slightly larger one. The stated box is what FreeType and HarfBuzz
    /// report, and it is free — four `i16`s at a fixed offset, no point
    /// decoding, no composite recursion.
    ///
    /// A glyph with no outline (space, and anything `loca` gives zero length)
    /// has an all-zero box rather than `None`: it exists and it draws nothing,
    /// which is a different answer from "cannot say".
    ///
    /// CFF faces have no stated box, so there the outline is walked and the
    /// result is the conservative one.
    #[must_use]
    pub fn glyph_bbox(&self, gid: u16) -> Option<BBox> {
        const EMPTY: BBox = BBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 0.0,
            y_max: 0.0,
        };
        if let Outlines::Cff(_) = &self.outlines {
            return Some(self.outline(gid).ok()?.bbox().unwrap_or(EMPTY));
        }
        let Some(span) = self.glyph_span(gid).ok()? else {
            return Some(EMPTY);
        };
        let end = span.off.checked_add(span.len)?;
        let g = self.data.get(span.off..end)?;
        let dx = self.glyf_shift(gid);
        Some(BBox {
            x_min: f32::from(i16_at(g, 2)?) + dx,
            y_min: f32::from(i16_at(g, 4)?),
            x_max: f32::from(i16_at(g, 6)?) + dx,
            y_max: f32::from(i16_at(g, 8)?),
        })
    }

    /// The same at a variable-font instance, which is not the same box.
    ///
    /// The box a `glyf` glyph states in its header describes the *default*
    /// outline and nothing else — `gvar` moves the points without rewriting it,
    /// and there is no per-instance box anywhere in the format. So away from the
    /// default instance the outline has to be built and measured, which is what
    /// FreeType and HarfBuzz both do: HarfBuzz reads the stated box only while
    /// `font->num_coords` is zero.
    ///
    /// That makes the answer very slightly *looser* off the default instance —
    /// the hull of the control points rather than the tight box around the
    /// curves — because that is the only box a point walk can produce, and it is
    /// the one HarfBuzz produces there too. A caller comparing the two instances
    /// is comparing two different measurements, not one measurement of two
    /// shapes; the boxes are used to stack accents, where a few units of slack
    /// off the default weight is invisible and disagreeing with HarfBuzz about
    /// where the accent goes is not.
    #[must_use]
    pub fn glyph_bbox_at(&self, gid: u16, coords: &var::Coords) -> Option<BBox> {
        if coords.is_default() || self.gvar.is_none() {
            return self.glyph_bbox(gid);
        }
        const EMPTY: BBox = BBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 0.0,
            y_max: 0.0,
        };
        Some(self.outline_at(gid, coords).ok()?.bbox().unwrap_or(EMPTY))
    }

    /// How far a `glyf` glyph's ink has to move right for it to start at the
    /// left side bearing `hmtx` states.
    ///
    /// The spec says a glyph's stored `xMin` and its `hmtx` left side bearing
    /// are the same number, and in most fonts, for most glyphs, they are.
    /// Where they disagree every real rasterizer believes `hmtx` and moves the
    /// outline: FreeType sets `pp1.x = bbox.xMin - left_bearing` and then
    /// translates the loaded points by `-pp1.x` (`TT_Process_Simple_Glyph`),
    /// and HarfBuzz reports `x_bearing = lsb` with the header's width under a
    /// comment calling it "undocumented rasterizer behavior"
    /// (`hb-ot-glyf-table.hh`).
    ///
    /// Windows ships a font that needs it: `LATINWD.TTF` stores `.notdef` at
    /// `xMin = 0` with a left side bearing of 68, so its 546-unit box belongs
    /// centred in the 682-unit cell, not flush against the pen. Reading the
    /// header alone drew it 68 units too far left and, because the mark
    /// fallback measures ink boxes, dragged every combining mark on a
    /// `.notdef` base along with it.
    ///
    /// Zero whenever the two agree, whenever the glyph has no outline, and
    /// whenever `hmtx` cannot answer — never an error, because a face with a
    /// damaged `hmtx` should still draw.
    fn glyf_shift(&self, gid: u16) -> f32 {
        let Ok(Some(span)) = self.glyph_span(gid) else {
            return 0.0;
        };
        let Some(x_min) = span
            .off
            .checked_add(span.len)
            .and_then(|end| self.data.get(span.off..end))
            .and_then(|g| i16_at(g, 2))
        else {
            return 0.0;
        };
        self.left_side_bearing(gid)
            .map_or(0.0, |lsb| f32::from(lsb) - f32::from(x_min))
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
        let Outlines::Glyf {
            loca_long,
            loca,
            glyf,
        } = &self.outlines
        else {
            return Err(SfntError::MissingTable("glyf"));
        };
        let i = usize::from(gid);
        let (start, end) = if *loca_long {
            let a = loca
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
            let a = loca
                .off
                .checked_add(i.checked_mul(2).ok_or(SfntError::TooShort)?)
                .ok_or(SfntError::TooShort)?;
            let s = u16_at(&self.data, a).ok_or(SfntError::MalformedTable("loca"))?;
            let e = u16_at(&self.data, a.checked_add(2).ok_or(SfntError::TooShort)?)
                .ok_or(SfntError::MalformedTable("loca"))?;
            // The short format stores halved offsets, which is why a
            // short-loca font can only address 128 KiB of glyph data.
            (
                usize::from(s).saturating_mul(2),
                usize::from(e).saturating_mul(2),
            )
        };
        if end <= start {
            return Ok(None);
        }
        let len = end
            .checked_sub(start)
            .ok_or(SfntError::MalformedTable("loca"))?;
        let off = glyf
            .off
            .checked_add(start)
            .ok_or(SfntError::MalformedTable("loca"))?;
        if off.checked_add(len).is_none_or(|e| e > self.data.len()) {
            return Err(SfntError::MalformedTable("loca"));
        }
        if len > glyf.len {
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
        if let Outlines::Cff(cff) = &self.outlines {
            // A CFF face's glyph count lives in the CharStrings INDEX as well
            // as in `maxp`. `maxp` is what every other part of this module
            // trusts, so the range check stays here and `cff` is asked only
            // for glyphs it agrees exist.
            if gid >= self.num_glyphs {
                return Err(SfntError::GlyphOutOfRange);
            }
            return cff.outline(&self.data, gid);
        }
        let mut out = Outline::default();
        self.outline_into(gid, &mut out, 0)?;
        // The points are stored relative to the glyph's own `xMin`; the
        // rasterizer positions them from `hmtx`. See `glyf_shift`.
        out.translate_x(self.glyf_shift(gid));
        Ok(out)
    }

    /// Extract a glyph's outline at a variable-font instance.
    ///
    /// `coords` are normalized axis coordinates, as produced by
    /// [`Variations::normalize`](crate::var::Variations::normalize). Identical
    /// to [`outline`](Self::outline) when this face does not vary, when it has
    /// no `gvar`, or when `coords` is the default instance — the last of which
    /// is not merely an optimization: the default instance is *defined* as the
    /// outline in `glyf`, and every tuple's scalar is zero there, so taking the
    /// short path and taking the long one must agree, and the short one cannot
    /// be wrong.
    ///
    /// # Errors
    ///
    /// As [`outline`](Self::outline). A `gvar` that is unreadable for this
    /// particular glyph is *not* an error: it yields the default outline, on
    /// the same reasoning as [`Gvar::parse`](crate::gvar::Gvar) returning
    /// `None` — a face that fails to vary still draws.
    pub fn outline_at(&self, gid: u16, coords: &var::Coords) -> Result<Outline, SfntError> {
        let Some(gvar) = self.gvar.as_ref() else {
            return self.outline(gid);
        };
        if coords.is_default() || !matches!(self.outlines, Outlines::Glyf { .. }) {
            return self.outline(gid);
        }
        let mut out = Outline::default();
        let phantom = self.outline_into_at(gvar, gid, coords.as_slice(), &mut out, 0)?;
        // The outline is placed so that the left side bearing point lands on
        // the origin. That point moves with the glyph, so the shift computed
        // from the *default* `xMin` has to be corrected by however far `gvar`
        // moved it — which is exactly the first phantom point's horizontal
        // delta. Getting this wrong shifts the varied glyph sideways relative
        // to its neighbours rather than deforming it, which is the sort of bug
        // that looks like bad kerning.
        out.translate_x(self.glyf_shift(gid) - phantom);
        Ok(out)
    }

    /// Build one glyph at `coords`, returning the horizontal delta of its left
    /// phantom point so the caller can correct the side-bearing shift.
    ///
    /// Recursive calls discard that return value: a component is placed by the
    /// parent's transform, and the parent's phantom points — not the child's —
    /// are what position the finished glyph.
    fn outline_into_at(
        &self,
        gvar: &gvar::Gvar,
        gid: u16,
        coords: &[i16],
        out: &mut Outline,
        depth: u8,
    ) -> Result<f32, SfntError> {
        if depth > MAX_COMPOSITE_DEPTH {
            return Err(SfntError::CompositeTooDeep);
        }
        let Some(span) = self.glyph_span(gid)? else {
            return Ok(0.0);
        };
        let end = span.off.checked_add(span.len).ok_or(SfntError::TooShort)?;
        let g = self
            .data
            .get(span.off..end)
            .ok_or(SfntError::MalformedTable("glyf"))?;
        let num_contours = i16_at(g, 0).ok_or(SfntError::MalformedTable("glyf"))?;
        let body = g.get(10..).ok_or(SfntError::MalformedTable("glyf"))?;
        if num_contours >= 0 {
            let n = usize::try_from(num_contours).map_err(|_| SfntError::MalformedTable("glyf"))?;
            let Some(mut glyph) = read_simple_glyph(body, n)? else {
                return Ok(0.0);
            };
            let phantom = match gvar.deltas(&self.data, gid, coords, &glyph.points, &glyph.ends) {
                Some(deltas) => {
                    for (pt, &(dx, dy)) in glyph.points.iter_mut().zip(deltas.iter()) {
                        pt.p.x += dx;
                        pt.p.y += dy;
                    }
                    deltas.get(glyph.points.len()).map_or(0.0, |&(dx, _)| dx)
                }
                None => 0.0,
            };
            emit_glyph(&glyph, out);
            Ok(phantom)
        } else {
            let mut components = read_components(body)?;
            // A composite's variation point array has one entry per component,
            // holding that component's placement offset. There are no contours,
            // so `ends` is empty and no interpolation happens — a component the
            // font did not name simply does not move.
            let placements: Vec<GlyphPoint> = components
                .iter()
                .map(|c| GlyphPoint {
                    p: Point::new(c.xform.e, c.xform.f),
                    on_curve: true,
                })
                .collect();
            let phantom = match gvar.deltas(&self.data, gid, coords, &placements, &[]) {
                Some(deltas) => {
                    for (c, &(dx, dy)) in components.iter_mut().zip(deltas.iter()) {
                        // Only an offset-placed component may move: a
                        // point-matched one is anchored to the parent's own
                        // geometry, which has already varied, so adding the
                        // delta here would move it a second time.
                        if c.offset_placed {
                            c.xform.e += dx;
                            c.xform.f += dy;
                        }
                    }
                    deltas.get(components.len()).map_or(0.0, |&(dx, _)| dx)
                }
                None => 0.0,
            };
            for c in components {
                let mut child = Outline::default();
                self.outline_into_at(
                    gvar,
                    c.gid,
                    coords,
                    &mut child,
                    depth.checked_add(1).ok_or(SfntError::CompositeTooDeep)?,
                )?;
                out.extend_transformed(&child, &c.xform);
            }
            Ok(phantom)
        }
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
        for c in read_components(data)? {
            // Recurse into the component and splice its (transformed) path in.
            // Building the child separately is what makes nested composites
            // work: the child's own component transforms compose naturally
            // because they were already applied when it was built.
            let mut child = Outline::default();
            self.outline_into(
                c.gid,
                &mut child,
                depth.checked_add(1).ok_or(SfntError::CompositeTooDeep)?,
            )?;
            out.extend_transformed(&child, &c.xform);
        }
        Ok(())
    }
}

/// One entry of a composite glyph: which glyph, and where it goes.
///
/// Split out of the drawing loop because `gvar` varies a composite by moving
/// its components: each component contributes exactly one point to the glyph's
/// variation point array, and that point is this transform's translation. The
/// default instance and a varied one therefore differ only in `xform.e` and
/// `xform.f`, and both paths can share the reading.
#[derive(Clone, Copy)]
pub(crate) struct Component {
    pub(crate) gid: u16,
    pub(crate) xform: Transform,
    /// Whether the placement came from an (x, y) offset rather than a pair of
    /// point indices. `gvar` may only move the offset kind — a point-matched
    /// component is positioned by the *parent's* geometry, which has already
    /// moved, so adding a delta on top would move it twice.
    pub(crate) offset_placed: bool,
}

/// Read a composite glyph's component list.
fn read_components(data: &[u8]) -> Result<Vec<Component>, SfntError> {
    const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
    const ARGS_ARE_XY_VALUES: u16 = 0x0002;
    const WE_HAVE_A_SCALE: u16 = 0x0008;
    const MORE_COMPONENTS: u16 = 0x0020;
    const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
    const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;

    // A composite with more components than a face has glyphs is malformed;
    // the bound stops a corrupt `MORE_COMPONENTS` chain from looping until the
    // data runs out, which on a large `glyf` is a long time.
    const MAX_COMPONENTS: usize = 4096;

    let mut out = Vec::new();
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
        let offset_placed = flags & ARGS_ARE_XY_VALUES != 0;
        let (tx, ty) = if offset_placed {
            (arg1, arg2)
        } else {
            (0.0, 0.0)
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

        out.push(Component {
            gid: component,
            xform,
            offset_placed,
        });

        if flags & MORE_COMPONENTS == 0 || out.len() >= MAX_COMPONENTS {
            break;
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Simple glyph decoding
// ---------------------------------------------------------------------------

/// A decoded `glyf` point, before it becomes a path command.
#[derive(Clone, Copy)]
pub(crate) struct GlyphPoint {
    pub(crate) p: Point,
    pub(crate) on_curve: bool,
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
            acc = acc
                .checked_add(v)
                .ok_or(SfntError::MalformedTable("glyf"))?;
        }
        out.push(acc);
    }
    Ok(out)
}

/// One simple glyph as the file stores it: points, and where each contour ends.
///
/// This is the shape `gvar` needs and path commands are not. A variation delta
/// is addressed by *point index*, and the points a tuple does not mention are
/// filled in by interpolating between the ones it does — both of which require
/// the point array to still exist, and to still be divided into contours, since
/// the interpolation wraps within a contour and never across one.
///
/// So the outline is built in two steps rather than one: read the points, then
/// (after any deltas are applied) turn them into path commands. The default
/// instance goes through both back to back, which is why nothing outside this
/// module notices.
pub(crate) struct SimpleGlyph {
    pub(crate) points: Vec<GlyphPoint>,
    /// Index one past the last point of each contour, so contour *i* is
    /// `points[ends[i-1]..ends[i]]`. Kept as the exclusive end rather than the
    /// file's inclusive last index, because every use here is a slice bound.
    pub(crate) ends: Vec<usize>,
}

/// Turn a point array back into path commands, one contour at a time.
fn emit_glyph(glyph: &SimpleGlyph, out: &mut Outline) {
    let mut start = 0usize;
    for &end in &glyph.ends {
        let Some(contour) = glyph.points.get(start..end) else {
            // Only reachable if `ends` and `points` disagree, which
            // `read_simple_glyph` has already ruled out. Stopping is right
            // regardless: the contours read so far are correct.
            return;
        };
        emit_contour(contour, out);
        start = end;
    }
}

fn parse_simple_glyph(d: &[u8], num_contours: usize, out: &mut Outline) -> Result<(), SfntError> {
    let Some(glyph) = read_simple_glyph(d, num_contours)? else {
        return Ok(());
    };
    emit_glyph(&glyph, out);
    Ok(())
}

/// Read a simple glyph's points without collapsing them into a path.
///
/// `Ok(None)` for a glyph with no contours, which is a real and common thing —
/// the space glyph is one.
pub(crate) fn read_simple_glyph(
    d: &[u8],
    num_contours: usize,
) -> Result<Option<SimpleGlyph>, SfntError> {
    const ON_CURVE: u8 = 0x01;
    const X_SHORT: u8 = 0x02;
    const Y_SHORT: u8 = 0x04;
    const REPEAT: u8 = 0x08;
    const X_SAME_OR_POS: u8 = 0x10;
    const Y_SAME_OR_POS: u8 = 0x20;

    if num_contours == 0 {
        return Ok(None);
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
    let num_points = usize::from(last)
        .checked_add(1)
        .ok_or(SfntError::TooShort)?;

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

    // Validate every contour bound now, so that `emit_glyph` and the `gvar`
    // interpolation can both slice by them without re-checking — and so that a
    // malformed file is an error here rather than a silently-short outline
    // three call frames away.
    let mut ends = Vec::with_capacity(num_contours);
    let mut start = 0usize;
    for end in &end_pts {
        let end_idx = usize::from(*end)
            .checked_add(1)
            .ok_or(SfntError::TooShort)?;
        if end_idx > points.len() || end_idx < start {
            return Err(SfntError::MalformedTable("glyf"));
        }
        ends.push(end_idx);
        start = end_idx;
    }
    Ok(Some(SimpleGlyph { points, ends }))
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
// `name` table
// ---------------------------------------------------------------------------

/// The `name` table entries worth asking for by number.
///
/// The full list runs to 25 ids, most of which are legal text (the licence,
/// the vendor URL) rather than anything a renderer or a font menu uses. These
/// are the ones with a caller.
pub mod name_id {
    /// Family, in the legacy four-style grouping — see [`Face::family`].
    ///
    /// [`Face::family`]: super::Face::family
    pub const FAMILY: u16 = 1;
    /// Style within the legacy family: only ever Regular, Italic, Bold or
    /// Bold Italic.
    pub const SUBFAMILY: u16 = 2;
    /// Human-readable full name, family and style together.
    pub const FULL_NAME: u16 = 4;
    /// Unique ASCII identifier for the face.
    pub const POSTSCRIPT: u16 = 6;
    /// Family in the unrestricted, typographic grouping.
    pub const TYPOGRAPHIC_FAMILY: u16 = 16;
    /// Style within the typographic family — "SemiBold", "Condensed Light".
    pub const TYPOGRAPHIC_SUBFAMILY: u16 = 17;
}

/// One `name` record's string, decoded, choosing the best-encoded copy.
///
/// A face carries the same name several times over, once per platform and
/// language it was built for, and the records are in no useful order. This
/// scores every record for `name_id` and decodes the winner:
///
/// * A **Windows** (platform 3) or **Unicode** (platform 0) record is UTF-16BE
///   and is preferred, because it can express every name any font uses.
/// * **English** is preferred within that (Windows language 0x0409), so a face
///   built for several markets reports the name a font menu here should show
///   rather than whichever language happened to be recorded first.
/// * A **Macintosh Roman** (platform 1, encoding 0) record is the fallback,
///   decoded through a real MacRoman table rather than being assumed ASCII —
///   the assumption silently corrupts every accented name.
///
/// Anything else — Mac non-Roman, the deprecated ISO platform, an unpaired
/// UTF-16 surrogate — is skipped rather than guessed at, so a name is either
/// right or absent.
fn read_name(table: &[u8], name_id: u16) -> Option<String> {
    let count = u16_at(table, 2)?;
    let storage = usize::from(u16_at(table, 4)?);

    let mut best: Option<(u8, usize, usize, u16, u16)> = None;
    for i in 0..usize::from(count) {
        let rec = 6usize.checked_add(i.checked_mul(12)?)?;
        let platform = u16_at(table, rec)?;
        let encoding = u16_at(table, rec.checked_add(2)?)?;
        let language = u16_at(table, rec.checked_add(4)?)?;
        if u16_at(table, rec.checked_add(6)?)? != name_id {
            continue;
        }
        let len = usize::from(u16_at(table, rec.checked_add(8)?)?);
        let off = storage.checked_add(usize::from(u16_at(table, rec.checked_add(10)?)?))?;
        // A record whose string runs past the table is not a string.
        if off.checked_add(len).is_none_or(|e| e > table.len()) {
            continue;
        }
        let Some(score) = name_score(platform, encoding, language) else {
            continue;
        };
        // Higher score wins; ties go to the first record, which is the
        // font's own preferred order.
        if best.is_none_or(|(s, ..)| score > s) {
            best = Some((score, off, len, platform, encoding));
        }
    }

    let (_, off, len, platform, _) = best?;
    let bytes = table.get(off..off.checked_add(len)?)?;
    if platform == 1 {
        Some(decode_mac_roman(bytes))
    } else {
        decode_utf16_be(bytes)
    }
}

/// How much this crate wants a given `name` record, or `None` to skip it.
fn name_score(platform: u16, encoding: u16, language: u16) -> Option<u8> {
    match platform {
        // Windows. Encodings 1 (BMP) and 10 (full UCS-4) are both UTF-16BE on
        // the wire; the rest are legacy codepages we do not decode.
        3 if encoding == 1 || encoding == 10 => Some(if language == 0x0409 { 4 } else { 3 }),
        // Unicode platform: always UTF-16BE, no language field to speak of.
        0 => Some(2),
        // Macintosh Roman.
        1 if encoding == 0 => Some(if language == 0 { 1 } else { 0 }),
        _ => None,
    }
}

/// UTF-16BE to a `String`, or `None` if the bytes are not valid UTF-16.
///
/// Returning `None` rather than substituting replacement characters is
/// deliberate: a mis-decoded family name silently fails to match the family it
/// belongs to, which is harder to diagnose than a missing one.
fn decode_utf16_be(bytes: &[u8]) -> Option<String> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let mut units = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        units.push(u16::from_be_bytes([*pair.first()?, *pair.get(1)?]));
    }
    char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .ok()
}

/// The 128 characters MacRoman assigns above ASCII, in code order from 0x80.
///
/// Needed because the Macintosh records are the only copy of a name in plenty
/// of older faces, and treating their high half as Latin-1 (or as ASCII) turns
/// every accented family name into mojibake.
const MAC_ROMAN_HIGH: [char; 128] = [
    'Ä', 'Å', 'Ç', 'É', 'Ñ', 'Ö', 'Ü', 'á', 'à', 'â', 'ä', 'ã', 'å', 'ç', 'é', 'è', 'ê', 'ë', 'í',
    'ì', 'î', 'ï', 'ñ', 'ó', 'ò', 'ô', 'ö', 'õ', 'ú', 'ù', 'û', 'ü', '†', '°', '¢', '£', '§', '•',
    '¶', 'ß', '®', '©', '™', '´', '¨', '≠', 'Æ', 'Ø', '∞', '±', '≤', '≥', '¥', 'µ', '∂', '∑', '∏',
    'π', '∫', 'ª', 'º', 'Ω', 'æ', 'ø', '¿', '¡', '¬', '√', 'ƒ', '≈', '∆', '«', '»', '…', '\u{a0}',
    'À', 'Ã', 'Õ', 'Œ', 'œ', '–', '—', '“', '”', '‘', '’', '÷', '◊', 'ÿ', 'Ÿ', '⁄', '€', '‹', '›',
    'ﬁ', 'ﬂ', '‡', '·', '‚', '„', '‰', 'Â', 'Ê', 'Á', 'Ë', 'È', 'Í', 'Î', 'Ï', 'Ì', 'Ó', 'Ô',
    '\u{f8ff}', 'Ò', 'Ú', 'Û', 'Ù', 'ı', 'ˆ', '˜', '¯', '˘', '˙', '˚', '¸', '˝', '˛', 'ˇ',
];

/// MacRoman bytes to a `String`. Cannot fail — every byte has a character.
fn decode_mac_roman(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| {
            if b.is_ascii() {
                char::from(*b)
            } else {
                // The index is `b - 0x80` for a non-ASCII byte, so it is in
                // 0..128 and the table has 128 entries.
                MAC_ROMAN_HIGH
                    .get(usize::from(*b).saturating_sub(0x80))
                    .copied()
                    .unwrap_or('\u{fffd}')
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::too_many_lines
)]
pub(crate) mod tests {
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
    pub(crate) fn build_test_font() -> Vec<u8> {
        assemble(&build_test_tables(TRUE_LSB_3))
    }

    /// Glyph 3's left side bearing, equal to its stored `xMin` as a
    /// well-formed font's is. [`build_test_font_with_trailing_lsb`] is how a
    /// test disagrees with it.
    const TRUE_LSB_3: i16 = 600;

    /// The fixture with glyph 3's `hmtx` bearing set to `lsb`, which the
    /// glyph's stored `xMin` of 600 then contradicts.
    fn build_test_font_with_trailing_lsb(lsb: i16) -> Vec<u8> {
        assemble(&build_test_tables(lsb))
    }

    /// The fixture's tables, before they are laid out.
    ///
    /// Separate from [`build_test_font`] so a test can add one more table to
    /// the list — see [`build_test_font_with_gpos_scripts`] — rather than
    /// re-deriving four glyphs and a `cmap` to change one thing.
    fn build_test_tables(lsb_3: i16) -> Vec<([u8; 4], Vec<u8>)> {
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
        hhea[10..12].copy_from_slice(&be16(600)); // advanceWidthMax
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
        hmtx.extend_from_slice(&be16i(lsb_3)); // glyph 3's lsb only

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

        alloc::vec![
            (*b"cmap", cmap),
            (*b"glyf", glyf),
            (*b"head", head),
            (*b"hhea", hhea),
            (*b"hmtx", hmtx),
            (*b"loca", loca),
            (*b"maxp", maxp),
        ]
    }

    /// The fixture plus a `GPOS` that registers exactly `scripts` and does
    /// nothing.
    ///
    /// `scripts` is written into the ScriptList in the order given, since a
    /// real font's ScriptList is in whatever order its compiler emitted and
    /// nothing may depend on it being sorted. See
    /// [`build_test_font_with_layout`] for why the table is empty.
    pub(crate) fn build_test_font_with_gpos_scripts(scripts: &[[u8; 4]]) -> Vec<u8> {
        build_test_font_with_layout(scripts, &[])
    }

    /// The fixture plus a `GDEF` whose `GlyphClassDef` gives glyph `i` the
    /// class `classes[i]`, counting from glyph 0.
    ///
    /// See [`build_test_font_with_layout`] for what the classes mean and for
    /// the combination of the two tables.
    pub(crate) fn build_test_font_with_gdef_classes(classes: &[u16]) -> Vec<u8> {
        build_test_font_with_layout(&[], classes)
    }

    /// The fixture with a `GPOS` registering `scripts` and a `GDEF`
    /// classifying `classes`, either of which is omitted when empty.
    ///
    /// The `GPOS` has an empty FeatureList and an empty LookupList, so the
    /// table positions nothing at all — which is the point. The question it
    /// exists to ask is whether a *run* accepts the face's `GPOS`, and that is
    /// decided by the ScriptList alone: a face that names a script has been
    /// written with it in mind whatever it then does about it, and a face that
    /// does not name one has not. A table with real lookups in it would let a
    /// test pass for the wrong reason, by positioning something.
    ///
    /// In `classes`, `1` is base, `2` ligature, `3` mark, and `0` — which is
    /// also the class of any glyph past the end of the slice — is
    /// "unclassified". The distinction that matters to the shaper is not any
    /// one glyph's class but whether the table exists at all: a face that
    /// classifies has stated which of its glyphs are marks, so a glyph it
    /// leaves out is one it declined to call a mark, and the general category
    /// of the character must not second-guess it. No `AttachList`,
    /// `LigCaretList` or `MarkAttachClassDef`, all of which are legitimately
    /// absent from real fonts and none of which this asks about.
    ///
    /// Both together is not a contrivance: it is the ordinary shape of a real
    /// font, and the only way to reach the case where a face has said which
    /// glyphs are marks *and* nothing is going to fall back.
    pub(crate) fn build_test_font_with_layout(scripts: &[[u8; 4]], classes: &[u16]) -> Vec<u8> {
        let mut tables = build_test_tables(TRUE_LSB_3);
        if !scripts.is_empty() {
            tables.push((*b"GPOS", empty_layout_table(scripts)));
        }
        if !classes.is_empty() {
            tables.push((*b"GDEF", glyph_classes(classes)));
        }
        tables.sort_unstable_by_key(|&(tag, _)| tag);
        assemble(&tables)
    }

    /// A `GDEF` 1.0 whose only content is a `GlyphClassDef`.
    fn glyph_classes(classes: &[u16]) -> Vec<u8> {
        let n = u16::try_from(classes.len()).expect("a test may not classify 65536 glyphs");
        let mut gdef: Vec<u8> = Vec::new();
        gdef.extend_from_slice(&1u16.to_be_bytes()); // majorVersion
        gdef.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
        gdef.extend_from_slice(&12u16.to_be_bytes()); // glyphClassDefOffset
        gdef.extend_from_slice(&0u16.to_be_bytes()); // attachListOffset
        gdef.extend_from_slice(&0u16.to_be_bytes()); // ligCaretListOffset
        gdef.extend_from_slice(&0u16.to_be_bytes()); // markAttachClassDefOffset
        // ClassDefFormat 1, starting at glyph 0 so the array is the classes.
        gdef.extend_from_slice(&1u16.to_be_bytes());
        gdef.extend_from_slice(&0u16.to_be_bytes()); // startGlyphID
        gdef.extend_from_slice(&n.to_be_bytes()); // glyphCount
        for class in classes {
            gdef.extend_from_slice(&class.to_be_bytes());
        }
        gdef
    }

    /// The fixture plus a `GSUB` that registers exactly `scripts` and
    /// substitutes nothing.
    ///
    /// The shape of `Hack`, which is the face that made the difference between
    /// "the ScriptList names this tag" and "a run under this tag reaches a
    /// lookup" observable: its `GSUB` registers `DFLT` and `latn` and neither
    /// one's default language system selects a feature this crate asks for, so
    /// [`Substitutions`] is `None` outright and the only record that the face
    /// named those scripts is [`Face::gsub_scripts`].
    pub(crate) fn build_test_font_with_gsub_scripts(scripts: &[[u8; 4]]) -> Vec<u8> {
        build_test_font_with_gsub_and_classes(scripts, &[])
    }

    /// The fixture with a `GSUB` registering `scripts` and a `GDEF`
    /// classifying `classes`, the latter omitted when empty.
    ///
    /// The `GSUB` half is what [`build_test_font_with_gsub_scripts`] is for and
    /// the `GDEF` half what [`build_test_font_with_gdef_classes`] is for; the
    /// pair exists because which script a face files its features under is
    /// asked of `GSUB` alone — that is where a face says whether it wants a
    /// complex shaper at all — while whether a glyph is a mark is asked of
    /// `GDEF` alone. A test about a run that reaches the *default* shaper in a
    /// face that classifies its glyphs needs both, and neither single-table
    /// builder can express it.
    pub(crate) fn build_test_font_with_gsub_and_classes(
        scripts: &[[u8; 4]],
        classes: &[u16],
    ) -> Vec<u8> {
        let mut tables = build_test_tables(TRUE_LSB_3);
        tables.push((*b"GSUB", empty_layout_table(scripts)));
        if !classes.is_empty() {
            tables.push((*b"GDEF", glyph_classes(classes)));
        }
        tables.sort_unstable_by_key(|&(tag, _)| tag);
        assemble(&tables)
    }

    /// One selector record of a `cmap` format-14 subtable: the selector, the
    /// default half as `(start, additional)` ranges, and the non-default half
    /// as `(code point, glyph)` pairs. An empty half is omitted from the file,
    /// which the format spells as a zero offset.
    ///
    /// Both halves must be sorted by code point — the format requires it and
    /// [`first_at_or_after`] binary-searches on the assumption. A test that
    /// wants an unsorted one has to build the bytes itself.
    pub(crate) type Uvs<'a> = (u32, &'a [(u32, u8)], &'a [(u32, u16)]);

    /// The fixture plus a `cmap` format-14 subtable holding `records`.
    ///
    /// The fixture's ordinary format-4 subtable stays, and stays *first*: a
    /// face with variation sequences always has an ordinary table too, since
    /// format 14 maps pairs and nothing else. The second `cmap` record is
    /// filed under platform 0 encoding 5, which is what the encoding is for,
    /// though nothing reads it — [`Face::find_variation_selectors`] looks for
    /// the format and not for the platform, exactly as HarfBuzz does.
    pub(crate) fn build_test_font_with_uvs(records: &[Uvs<'_>]) -> Vec<u8> {
        let mut tables = build_test_tables(TRUE_LSB_3);
        let sub14 = uvs_subtable(records);
        for &mut (tag, ref mut data) in &mut tables {
            if tag != *b"cmap" {
                continue;
            }
            // The fixture's `cmap` is a four-byte header, one eight-byte
            // record, then the subtable; a second record pushes it along by
            // eight.
            let sub4 = data.split_off(12);
            let mut cmap: Vec<u8> = Vec::new();
            cmap.extend_from_slice(&0u16.to_be_bytes()); // version
            cmap.extend_from_slice(&2u16.to_be_bytes()); // numTables
            cmap.extend_from_slice(&3u16.to_be_bytes()); // platformID (Windows)
            cmap.extend_from_slice(&1u16.to_be_bytes()); // encodingID (BMP)
            cmap.extend_from_slice(&20u32.to_be_bytes());
            cmap.extend_from_slice(&0u16.to_be_bytes()); // platformID (Unicode)
            cmap.extend_from_slice(&5u16.to_be_bytes()); // encodingID (UVS)
            let at = u32::try_from(20 + sub4.len()).expect("a test cmap fits in 4 GiB");
            cmap.extend_from_slice(&at.to_be_bytes());
            cmap.extend_from_slice(&sub4);
            cmap.extend_from_slice(&sub14);
            *data = cmap;
        }
        tables.sort_unstable_by_key(|&(tag, _)| tag);
        assemble(&tables)
    }

    /// The format-14 subtable itself: a ten-byte header, one eleven-byte
    /// record per selector, then the halves those records point at, in the
    /// order the records were written.
    fn uvs_subtable(records: &[Uvs<'_>]) -> Vec<u8> {
        fn u24(v: u32) -> [u8; 3] {
            let [_, a, b, c] = v.to_be_bytes();
            [a, b, c]
        }
        let head = 10 + 11 * records.len();
        let mut recs: Vec<u8> = Vec::new();
        let mut halves: Vec<u8> = Vec::new();
        for &(selector, default, non_default) in records {
            recs.extend_from_slice(&u24(selector));
            let mut half = |empty: bool, body: &[u8]| {
                let at = if empty {
                    0
                } else {
                    u32::try_from(head + halves.len()).expect("a test subtable fits in 4 GiB")
                };
                recs.extend_from_slice(&at.to_be_bytes());
                if !empty {
                    halves.extend_from_slice(body);
                }
            };
            let mut body: Vec<u8> = Vec::new();
            let n = u32::try_from(default.len()).expect("a test may not list 4G ranges");
            body.extend_from_slice(&n.to_be_bytes());
            for &(start, extra) in default {
                body.extend_from_slice(&u24(start));
                body.push(extra);
            }
            half(default.is_empty(), &body);
            let mut body: Vec<u8> = Vec::new();
            let n = u32::try_from(non_default.len()).expect("a test may not list 4G mappings");
            body.extend_from_slice(&n.to_be_bytes());
            for &(cp, gid) in non_default {
                body.extend_from_slice(&u24(cp));
                body.extend_from_slice(&gid.to_be_bytes());
            }
            half(non_default.is_empty(), &body);
        }
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&14u16.to_be_bytes()); // format
        let len = u32::try_from(head + halves.len()).expect("a test subtable fits in 4 GiB");
        out.extend_from_slice(&len.to_be_bytes());
        let n = u32::try_from(records.len()).expect("a test may not list 4G selectors");
        out.extend_from_slice(&n.to_be_bytes());
        out.extend_from_slice(&recs);
        out.extend_from_slice(&halves);
        out
    }

    /// A `GPOS` or `GSUB` whose ScriptList names `scripts` and which does
    /// nothing. The two tables share a header, so one builder serves both.
    fn empty_layout_table(scripts: &[[u8; 4]]) -> Vec<u8> {
        let n = u16::try_from(scripts.len()).expect("a test may not register 65536 scripts");
        // The ScriptList begins right after the five-field header, each
        // ScriptRecord is six bytes, and each of the (identical, empty) script
        // tables it points at is four.
        let script_list = 10usize;
        let records = 2 + 6 * usize::from(n);
        let mut gpos: Vec<u8> = Vec::new();
        gpos.extend_from_slice(&1u16.to_be_bytes()); // majorVersion
        gpos.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
        gpos.extend_from_slice(&u16::try_from(script_list).unwrap().to_be_bytes());
        // FeatureList and LookupList sit after the ScriptList, which is the
        // record array plus one four-byte table per record.
        let feature_list = script_list + records + 4 * usize::from(n);
        gpos.extend_from_slice(&u16::try_from(feature_list).unwrap().to_be_bytes());
        gpos.extend_from_slice(&u16::try_from(feature_list + 2).unwrap().to_be_bytes());
        gpos.extend_from_slice(&n.to_be_bytes()); // scriptCount
        for (i, tag) in scripts.iter().enumerate() {
            gpos.extend_from_slice(tag);
            // Offsets in a ScriptRecord are from the start of the ScriptList.
            let at = records + 4 * i;
            gpos.extend_from_slice(&u16::try_from(at).unwrap().to_be_bytes());
        }
        for _ in scripts {
            gpos.extend_from_slice(&0u16.to_be_bytes()); // defaultLangSysOffset
            gpos.extend_from_slice(&0u16.to_be_bytes()); // langSysCount
        }
        gpos.extend_from_slice(&0u16.to_be_bytes()); // featureCount
        gpos.extend_from_slice(&0u16.to_be_bytes()); // lookupCount
        gpos
    }

    /// The fixture plus an `fvar` with one `wght` axis and a `gvar` that moves
    /// all four glyphs.
    ///
    /// This exists because **no font installed on this host can test the
    /// interesting half of `gvar`**. A sweep of every variable face here
    /// (`tools/variable_survey.py`, and the phantom probe written alongside
    /// `installed_variable_fonts_vary_their_outlines`) found that all seven
    /// carry `HVAR`, express their advance and side-bearing variation *there*,
    /// and therefore leave every phantom point's delta at zero — in 400 glyphs
    /// per face, not one moved. Real files likewise rarely exercise IUP in a way
    /// that a bounding box would notice, and none of them varies a composite's
    /// component offset. Those three paths are the ones a reader gets wrong, so
    /// they are built here by hand where the expected answer can be computed on
    /// paper rather than trusted from a second implementation.
    ///
    /// The axis is `wght` 100/400/700, so user weight 700 is normalized +1.0
    /// (every tuple at full strength) and 550 is exactly +0.5 (every tuple
    /// halved) — two instances whose arithmetic is exact in `f32` and so can be
    /// asserted without a tolerance.
    pub(crate) fn build_variable_test_font() -> Vec<u8> {
        let mut tables = build_test_tables(TRUE_LSB_3);
        // 'H' is added to the `cmap` on top of the base fixture's 'A'-'C',
        // mapped onto the square, because `ScaledFont` measures its cap height
        // from that character's outline. Without it the cap height falls back
        // to a fraction of the ascender and no longer varies, so the assertion
        // that outline-derived metrics follow the instance would pass whatever
        // the code did. The base fixture is left alone rather than given the
        // same mapping: adding it there would silently change the cap height
        // every other test in this crate sees.
        for (tag, data) in &mut tables {
            if tag == b"cmap" {
                *data = format4_cmap(&[
                    (0x0041, 0x0043, 1 - 0x41),
                    (0x0048, 0x0048, 1 - 0x48),
                    (0xFFFF, 0xFFFF, 1),
                ]);
            }
        }
        tables.push((*b"fvar", variable_fvar()));
        tables.push((*b"gvar", variable_gvar()));
        tables.sort_by_key(|(tag, _)| *tag);
        assemble(&tables)
    }

    /// A `cmap` holding one format-4 subtable with the given
    /// `(start, end, idDelta)` segments, which must be sorted and must end with
    /// the mandatory `0xFFFF` one.
    fn format4_cmap(segments: &[(u16, u16, i16)]) -> Vec<u8> {
        let n = u16::try_from(segments.len()).expect("a test may not need 65536 segments");
        let mut sub = Vec::new();
        sub.extend_from_slice(&4u16.to_be_bytes()); // format
        sub.extend_from_slice(&0u16.to_be_bytes()); // length, filled in below
        sub.extend_from_slice(&0u16.to_be_bytes()); // language
        sub.extend_from_slice(&(n * 2).to_be_bytes());
        // The three search hints are derived from the segment count and are
        // ignored by every reader that binary-searches the arrays itself, this
        // one included. They are written correctly anyway so the fixture stays
        // a file a stricter reader would also accept.
        let entry_selector = u16::try_from(n.ilog2()).unwrap();
        let search_range = 2 * (1u16 << entry_selector);
        sub.extend_from_slice(&search_range.to_be_bytes());
        sub.extend_from_slice(&entry_selector.to_be_bytes());
        sub.extend_from_slice(&(n * 2 - search_range).to_be_bytes());
        for &(_, end, _) in segments {
            sub.extend_from_slice(&end.to_be_bytes());
        }
        sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
        for &(start, _, _) in segments {
            sub.extend_from_slice(&start.to_be_bytes());
        }
        for &(_, _, delta) in segments {
            sub.extend_from_slice(&delta.to_be_bytes());
        }
        for _ in segments {
            sub.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset
        }
        let len = u16::try_from(sub.len()).unwrap();
        sub[2..4].copy_from_slice(&len.to_be_bytes());

        let mut cmap = Vec::new();
        cmap.extend_from_slice(&0u16.to_be_bytes()); // version
        cmap.extend_from_slice(&1u16.to_be_bytes()); // numTables
        cmap.extend_from_slice(&3u16.to_be_bytes()); // platformID
        cmap.extend_from_slice(&1u16.to_be_bytes()); // encodingID
        cmap.extend_from_slice(&12u32.to_be_bytes()); // offset to the subtable
        cmap.extend_from_slice(&sub);
        cmap
    }

    /// One `wght` axis, 100 to 700 with the default at 400, and one named
    /// instance at the top of it.
    fn variable_fvar() -> Vec<u8> {
        fn fixed(v: f32) -> [u8; 4] {
            #[allow(clippy::cast_possible_truncation)]
            let raw = (v * 65536.0) as i32;
            raw.to_be_bytes()
        }
        let mut fvar = Vec::new();
        fvar.extend_from_slice(&1u16.to_be_bytes()); // majorVersion
        fvar.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
        fvar.extend_from_slice(&16u16.to_be_bytes()); // axesArrayOffset
        fvar.extend_from_slice(&2u16.to_be_bytes()); // reserved
        fvar.extend_from_slice(&1u16.to_be_bytes()); // axisCount
        fvar.extend_from_slice(&20u16.to_be_bytes()); // axisSize
        fvar.extend_from_slice(&1u16.to_be_bytes()); // instanceCount
        fvar.extend_from_slice(&8u16.to_be_bytes()); // instanceSize
        fvar.extend_from_slice(b"wght");
        fvar.extend_from_slice(&fixed(100.0));
        fvar.extend_from_slice(&fixed(400.0));
        fvar.extend_from_slice(&fixed(700.0));
        fvar.extend_from_slice(&0u16.to_be_bytes()); // flags
        fvar.extend_from_slice(&256u16.to_be_bytes()); // axisNameID
        fvar.extend_from_slice(&257u16.to_be_bytes()); // subfamilyNameID
        fvar.extend_from_slice(&0u16.to_be_bytes()); // flags
        fvar.extend_from_slice(&fixed(700.0));
        fvar
    }

    /// Variation data for glyphs 1, 2 and 3, each chosen to exercise one thing
    /// the host's own fonts cannot.
    ///
    /// * **Glyph 1** names every point, including the phantoms, and gives the
    ///   left phantom a delta of +5. That is the only way to check that the
    ///   varied side-bearing correction is subtracted rather than added; with a
    ///   zero phantom the two signs are indistinguishable.
    /// * **Glyph 2** carries two tuples, one peaking at each end of the axis,
    ///   so at every instance one of them scores zero and must still be stepped
    ///   over byte for byte. The bold one names points 0 and 2 of a three-point
    ///   contour and leaves point 1 — the off-curve control — to IUP, which
    ///   must place it at the interpolated x and refuse to move it in y (its
    ///   two neighbours share a y, so there is no ratio). The light one names
    ///   point 1 alone, which is the case that translates a contour bodily.
    /// * **Glyph 3** is the composite, whose single variation point is its
    ///   component's placement offset.
    fn variable_gvar() -> Vec<u8> {
        // Glyph 1: eight points (four contour, four phantom), all named, so the
        // tuple carries a full delta array and no point list.
        let mut g1 = Vec::new();
        g1.extend_from_slice(&1u16.to_be_bytes()); // tupleVariationCount
        g1.extend_from_slice(&10u16.to_be_bytes()); // dataOffset
        g1.extend_from_slice(&18u16.to_be_bytes()); // variationDataSize
        g1.extend_from_slice(&0x8000u16.to_be_bytes()); // EMBEDDED_PEAK_TUPLE
        g1.extend_from_slice(&16384u16.to_be_bytes()); // peak: wght at +1
        // A byte run of eight, twice: x then y. Point 4 is the left phantom.
        g1.extend_from_slice(&[0x07, 10, 20, 30, 40, 5, 0, 0, 0]);
        g1.extend_from_slice(&[0x07, 1, 2, 3, 4, 0, 0, 0, 0]);
        assert_eq!(g1.len(), 28, "glyph 1's variation data is 10 + 18 bytes");

        // Glyph 2: seven points and *two* tuples, one peaking at each end of
        // the axis, so that at either instance exactly one of them scores zero.
        // The zero-scoring one is not merely inert — its bytes still have to be
        // stepped over, because the next tuple's data begins where this one's
        // ends and not where its own header says. The two tuples are given
        // different point lists and wildly different deltas precisely so that
        // reading the second at the first's offset produces a wrong glyph
        // rather than a read error, which a test can tell apart from a
        // reader that declined to vary at all.
        let mut g2 = Vec::new();
        g2.extend_from_slice(&2u16.to_be_bytes()); // tupleVariationCount
        g2.extend_from_slice(&16u16.to_be_bytes()); // dataOffset: past both headers
        // EMBEDDED_PEAK_TUPLE | PRIVATE_POINT_NUMBERS, twice.
        g2.extend_from_slice(&7u16.to_be_bytes()); // variationDataSize
        g2.extend_from_slice(&0xA000u16.to_be_bytes());
        g2.extend_from_slice(&0xC000u16.to_be_bytes()); // peak: wght at -1
        g2.extend_from_slice(&10u16.to_be_bytes()); // variationDataSize
        g2.extend_from_slice(&0xA000u16.to_be_bytes());
        g2.extend_from_slice(&0x4000u16.to_be_bytes()); // peak: wght at +1
        // Tuple 1 (light): names point 1 alone and moves it by (100,100), which
        // — being the contour's only named point — translates the whole contour.
        g2.extend_from_slice(&[0x01, 0x00, 0x01]);
        g2.extend_from_slice(&[0x00, 100]);
        g2.extend_from_slice(&[0x00, 100]);
        // Tuple 2 (bold): names points 0 and 2, stored as deltas from the
        // previous point number: 0, then +2.
        g2.extend_from_slice(&[0x02, 0x01, 0x00, 0x02]);
        g2.extend_from_slice(&[0x01, 0, 100]); // x deltas for those two
        g2.extend_from_slice(&[0x01, 0, 0]); // y deltas
        assert_eq!(
            g2.len(),
            33,
            "glyph 2's variation data is 16 + 7 + 10 bytes"
        );

        // Glyph 3: one component plus four phantoms.
        let mut g3 = Vec::new();
        g3.extend_from_slice(&1u16.to_be_bytes());
        g3.extend_from_slice(&10u16.to_be_bytes());
        g3.extend_from_slice(&12u16.to_be_bytes()); // variationDataSize
        g3.extend_from_slice(&0x8000u16.to_be_bytes());
        g3.extend_from_slice(&16384u16.to_be_bytes());
        g3.extend_from_slice(&[0x04, 25, 0, 0, 0, 0]);
        #[allow(clippy::cast_sign_loss)]
        g3.extend_from_slice(&[0x04, -35i8 as u8, 0, 0, 0, 0]);
        assert_eq!(g3.len(), 22, "glyph 3's variation data is 10 + 12 bytes");

        let mut body = Vec::new();
        let mut offsets = alloc::vec![0u32]; // glyph 0 does not vary
        for g in [&g1, &g2, &g3] {
            offsets.push(u32::try_from(body.len()).unwrap());
            body.extend_from_slice(g);
        }
        offsets.push(u32::try_from(body.len()).unwrap());

        // The offset array is `glyphCount + 1` long and sits immediately after
        // the twenty-byte header, so the data array starts at 20 + 4 * 5.
        let data_array = 20 + 4 * u32::try_from(offsets.len()).unwrap();
        let mut gvar = Vec::new();
        gvar.extend_from_slice(&1u16.to_be_bytes()); // majorVersion
        gvar.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
        gvar.extend_from_slice(&1u16.to_be_bytes()); // axisCount
        gvar.extend_from_slice(&0u16.to_be_bytes()); // sharedTupleCount
        gvar.extend_from_slice(&0u32.to_be_bytes()); // sharedTuplesOffset
        gvar.extend_from_slice(&4u16.to_be_bytes()); // glyphCount
        gvar.extend_from_slice(&1u16.to_be_bytes()); // flags: long offsets
        gvar.extend_from_slice(&data_array.to_be_bytes());
        for off in offsets {
            gvar.extend_from_slice(&off.to_be_bytes());
        }
        gvar.extend_from_slice(&body);
        gvar
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

    /// VARIATION SELECTOR-1, the one a real face is most likely to carry.
    const VS1: char = '\u{FE00}';
    /// VARIATION SELECTOR-2, used here as the selector a face does *not* list.
    const VS2: char = '\u{FE01}';

    fn uvs_face(records: &[Uvs<'_>]) -> Face {
        Face::parse(build_test_font_with_uvs(records)).expect("a font with a UVS table must parse")
    }

    /// The ordinary fixture has no format-14 subtable, so every pair misses —
    /// and the shaper's skip-the-pass flag says so, which is what keeps the
    /// lookup off the hot path for the overwhelming majority of faces.
    #[test]
    fn a_face_with_no_format_14_subtable_recognises_no_pair() {
        let f = face();
        assert!(!f.has_variation_sequences());
        assert_eq!(f.variation_glyph('A', VS1), None);
    }

    /// The non-default half is an explicit mapping: this pair draws as *that*
    /// glyph, whatever the base's ordinary `cmap` entry says. 'A' maps to
    /// glyph 1 normally; under the selector it is glyph 2.
    #[test]
    fn the_non_default_half_names_a_glyph_of_its_own() {
        let f = uvs_face(&[(VS1 as u32, &[], &[('A' as u32, 2)])]);
        assert!(f.has_variation_sequences());
        assert_eq!(
            f.glyph_index('A'),
            Some(1),
            "the ordinary cmap is untouched"
        );
        assert_eq!(f.variation_glyph('A', VS1), Some(2));
    }

    /// The default half means "the base's ordinary glyph is already right",
    /// and answering with that glyph says the same thing as the table while
    /// still reporting that the pair was *recognised* — which is the whole
    /// distinction that matters upstream, since a recognised pair is one glyph
    /// and an unrecognised one is two.
    #[test]
    fn the_default_half_answers_with_the_bases_ordinary_glyph() {
        // One range: U+0041 plus two more code points, so U+0041..=U+0043.
        let f = uvs_face(&[(VS1 as u32, &[('A' as u32, 2)], &[])]);
        for (ch, gid) in [('A', 1), ('B', 2), ('C', 3)] {
            assert_eq!(
                f.variation_glyph(ch, VS1),
                Some(gid),
                "{ch} is in the range"
            );
        }
    }

    /// The inclusive range's two edges, from both sides. `additional` counts
    /// the code points *after* the start, so a zero means a range of one — an
    /// off-by-one here would either swallow the next character or drop the
    /// last of the range.
    #[test]
    fn a_default_range_ends_where_its_additional_count_says() {
        let one = uvs_face(&[('\u{FE00}' as u32, &[('B' as u32, 0)], &[])]);
        assert_eq!(one.variation_glyph('A', VS1), None, "before the range");
        assert_eq!(one.variation_glyph('B', VS1), Some(2), "the range");
        assert_eq!(one.variation_glyph('C', VS1), None, "one past it");
    }

    /// Two ranges, and a base that falls in the gap between them. This is what
    /// the "first range starting after the base, then step back one" search is
    /// for: the range that could contain the base is the last one before it,
    /// and it may still not contain it.
    #[test]
    fn a_base_between_two_default_ranges_is_not_recognised() {
        let f = uvs_face(&[(VS1 as u32, &[('A' as u32, 0), ('C' as u32, 0)], &[])]);
        assert_eq!(f.variation_glyph('A', VS1), Some(1));
        assert_eq!(f.variation_glyph('B', VS1), None, "the gap");
        assert_eq!(f.variation_glyph('C', VS1), Some(3));
    }

    /// A base below every range there is. The search returns index 0, and
    /// stepping back from 0 has no answer at all — the arm that a subtraction
    /// on `usize` would have panicked on.
    #[test]
    fn a_base_below_every_default_range_is_not_recognised() {
        let f = uvs_face(&[(VS1 as u32, &[('B' as u32, 0)], &[])]);
        assert_eq!(f.variation_glyph('A', VS1), None);
    }

    /// A selector the face does not list is not a variation sequence, however
    /// well it knows the base. The record scan is linear over the selectors,
    /// so the second record has to be reachable too.
    #[test]
    fn only_the_selectors_the_face_lists_are_recognised() {
        let f = uvs_face(&[
            (VS1 as u32, &[], &[('A' as u32, 2)]),
            (VS2 as u32, &[], &[('A' as u32, 3)]),
        ]);
        assert_eq!(f.variation_glyph('A', VS1), Some(2));
        assert_eq!(f.variation_glyph('A', VS2), Some(3), "the second record");
        assert_eq!(f.variation_glyph('A', '\u{FE02}'), None, "no third record");
    }

    /// A base the listed selector says nothing about. The binary search lands
    /// on a real record and the equality check has to reject it; without that
    /// check a near miss would answer with its neighbour's glyph.
    #[test]
    fn a_base_the_selector_does_not_map_is_not_recognised() {
        let f = uvs_face(&[(VS1 as u32, &[], &[('A' as u32, 2), ('C' as u32, 3)])]);
        assert_eq!(f.variation_glyph('B', VS1), None, "between two mappings");
        assert_eq!(f.variation_glyph('\u{0100}', VS1), None, "past the last");
    }

    /// Both halves are optional, and the format spells "absent" as a zero
    /// offset — which is a perfectly ordinary offset everywhere else in
    /// `sfnt`, so reading one as a real one would send the search into the
    /// subtable header.
    #[test]
    fn an_absent_half_is_a_zero_offset_and_not_an_offset_of_zero() {
        let default_only = uvs_face(&[(VS1 as u32, &[('A' as u32, 0)], &[])]);
        assert_eq!(default_only.variation_glyph('A', VS1), Some(1));
        assert_eq!(default_only.variation_glyph('B', VS1), None);

        let non_default_only = uvs_face(&[(VS1 as u32, &[], &[('B' as u32, 2)])]);
        assert_eq!(non_default_only.variation_glyph('A', VS1), None);
        assert_eq!(non_default_only.variation_glyph('B', VS1), Some(2));

        let neither = uvs_face(&[(VS1 as u32, &[], &[])]);
        assert!(
            neither.has_variation_sequences(),
            "the table is still there"
        );
        assert_eq!(neither.variation_glyph('A', VS1), None);
    }

    /// A pair is looked up in the default half first, so a base listed in both
    /// halves gets its ordinary glyph. Malformed — the format forbids it — but
    /// a face is not obliged to be well-formed and the order must be defined.
    #[test]
    fn the_default_half_wins_when_a_base_is_in_both() {
        let f = uvs_face(&[(VS1 as u32, &[('A' as u32, 0)], &[('A' as u32, 3)])]);
        assert_eq!(f.variation_glyph('A', VS1), Some(1));
    }

    /// A mapping to a glyph the face does not have is refused rather than
    /// passed on: the fixture has four glyphs, and a `glyf` lookup on 99 would
    /// run off the end of `loca`. Glyph 0 is refused too — `.notdef` is what a
    /// miss already means, and a face saying "this pair draws as nothing" is
    /// indistinguishable from one saying nothing at all.
    #[test]
    fn a_mapping_to_a_glyph_the_face_lacks_is_refused() {
        let f = uvs_face(&[(VS1 as u32, &[], &[('A' as u32, 99), ('B' as u32, 0)])]);
        assert_eq!(f.variation_glyph('A', VS1), None, "past num_glyphs");
        assert_eq!(f.variation_glyph('B', VS1), None, ".notdef");
    }

    /// The binary search both halves share. `count` is returned when every
    /// record sorts before the base, and the caller must read that as a miss
    /// rather than as an index — the one arm that is not reachable through
    /// [`Face::variation_glyph`]'s default half, which steps back instead.
    #[test]
    fn the_record_search_finds_the_first_not_before_the_base() {
        // Four records, stride 4, code points 1, 3, 5, 7.
        let mut data: Vec<u8> = Vec::new();
        for cp in [1u32, 3, 5, 7] {
            data.extend_from_slice(&cp.to_be_bytes()[1..]);
            data.push(0);
        }
        for (base, want) in [(0, 0), (1, 0), (2, 1), (3, 1), (6, 3), (7, 3), (8, 4)] {
            assert_eq!(
                first_at_or_after(&data, 0, 4, 4, base),
                want,
                "the first record not before {base}"
            );
        }
    }

    /// A truncated table reads as "not before", walking the search to the low
    /// end, where the caller's own equality check fails it. The alternative —
    /// reading a short record as "before" — would walk it to `count` and be
    /// reported as a clean miss, which is the same answer for the wrong
    /// reason and would hide the truncation from a caller that stepped back.
    #[test]
    fn a_truncated_record_array_searches_toward_the_low_end() {
        assert_eq!(first_at_or_after(&[], 0, 4, 8, 5), 0);
        assert_eq!(
            first_at_or_after(&[0, 0, 9], 0, 4, 8, 5),
            0,
            "one short record"
        );
    }

    #[test]
    fn a_face_names_the_gpos_scripts_its_script_list_registers() {
        // Deliberately unsorted in the file: `gpos_names_script` binary-searches
        // and so depends on the sort happening at parse time.
        let bytes = build_test_font_with_gpos_scripts(&[*b"latn", *b"DFLT", *b"arab"]);
        let f = Face::parse(bytes).expect("a font with a GPOS must parse");
        assert!(f.has_positioning(), "the file carries a GPOS");
        for tag in [b"latn", b"DFLT", b"arab"] {
            assert!(f.gpos_names_script(tag), "{} is registered", tag_text(tag));
        }
        for tag in [b"hebr", b"thai", b"dflt"] {
            assert!(!f.gpos_names_script(tag), "{} is not", tag_text(tag));
        }
    }

    /// The whole point of holding the `GSUB` ScriptList separately: this face's
    /// `GSUB` selects no lookup this crate can apply, so its `Substitutions` is
    /// `None` and asking *that* which script was chosen answers "none" — which
    /// would leave a Devanagari run on the Indic shaper in a face that has said
    /// as plainly as a font can that it does no Indic shaping. `Hack` is the
    /// shipping face this describes; before the ScriptList was read directly,
    /// its `हिन्दी` was the last string in the HarfBuzz sweep placed wrongly.
    #[test]
    fn a_latin_only_face_calls_off_the_indic_shaper_even_with_no_usable_lookups() {
        let bytes = build_test_font_with_gsub_scripts(&[*b"latn", *b"DFLT"]);
        let f = Face::parse(bytes).expect("a font with a GSUB must parse");
        assert!(
            !f.has_substitutions(),
            "the fixture's GSUB reaches no lookup this crate applies"
        );
        let deva = Some(ScriptTags::exactly(*b"dev2"));
        assert_eq!(f.gsub_chosen_script(deva), Some(*b"DFLT"));
        assert!(f.shapes_as_default(deva));
        // And a run of Latin, which was on the default shaper to begin with,
        // is not reported as having lost anything.
        assert!(!f.shapes_as_default(Some(ScriptTags::exactly(*b"latn"))));
    }

    /// The other half: a face that names the run's own script keeps its
    /// shaper, and names the *tag it registered* — which is what tells the
    /// Indic shaper whether to follow the old spec or the revised one.
    ///
    /// The run's tags come from a real character rather than from
    /// [`ScriptTags::exactly`], which sets the fallback equal to the preferred
    /// tag and so could never reach the pre-revision `deva` half of this loop.
    #[test]
    fn a_face_naming_an_indic_script_keeps_the_indic_shaper() {
        // U+0939 DEVANAGARI LETTER HA: preferred `dev2`, fallback `deva`.
        let deva = ScriptTags::of('\u{0939}');
        assert!(deva.is_some(), "Devanagari must have script tags");
        for (registered, chosen) in [(*b"dev2", *b"dev2"), (*b"deva", *b"deva")] {
            let bytes = build_test_font_with_gsub_scripts(&[registered]);
            let f = Face::parse(bytes).expect("a font with a GSUB must parse");
            assert_eq!(
                f.gsub_chosen_script(deva),
                Some(chosen),
                "{} must be chosen",
                tag_text(&registered)
            );
            assert!(!f.shapes_as_default(deva), "{}", tag_text(&registered));
        }
    }

    /// A face with no `GSUB` at all has not said "no complex shaping"; it has
    /// said nothing, and its Devanagari keeps the Indic shaper — which is the
    /// face `NO_ZERO_WIDTH_MARKS` was measured against.
    #[test]
    fn a_face_with_no_gsub_keeps_the_indic_shaper() {
        let f = face();
        let deva = Some(ScriptTags::exactly(*b"dev2"));
        assert_eq!(f.gsub_chosen_script(deva), None);
        assert!(!f.shapes_as_default(deva));
    }

    #[test]
    fn a_face_with_no_gpos_names_no_script() {
        let f = face();
        assert!(!f.has_positioning());
        for tag in [b"latn", b"DFLT", b"hebr"] {
            assert!(
                !f.gpos_names_script(tag),
                "{} cannot be named",
                tag_text(tag)
            );
        }
    }

    /// A tag as text, for assertion messages only.
    fn tag_text(tag: &[u8; 4]) -> alloc::string::String {
        tag.iter().map(|&b| char::from(b)).collect()
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
        assert_eq!(f.left_side_bearing(3).unwrap(), TRUE_LSB_3);
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

    // --- gvar: the fixture at a non-default instance ---------------------
    //
    // Every expected outline below is computed on paper from the tuple bytes
    // in `variable_gvar`, not taken from a run. The axis is arranged so that
    // both instances used here have a scalar that is exact in `f32` (1.0 and
    // 0.5), so these compare without a tolerance.

    fn variable_face() -> Face {
        Face::parse(build_variable_test_font()).expect("the variable fixture must parse")
    }

    /// The fixture at user weight `w`.
    fn at_weight(f: &Face, w: f32) -> var::Coords {
        f.variation_axes()
            .expect("the variable fixture has an fvar")
            .normalize(&[w])
    }

    #[test]
    fn the_variable_fixture_offers_one_weight_axis() {
        let f = variable_face();
        let v = f.variation_axes().unwrap();
        assert_eq!(v.axes().len(), 1);
        assert_eq!(v.axes()[0].tag, *b"wght");
        assert_eq!(v.axes()[0].default, 400.0);
        // Normalization has to land on these two exactly for the assertions
        // below to be exact rather than approximate.
        assert_eq!(at_weight(&f, 700.0).as_slice(), &[16384]);
        assert_eq!(at_weight(&f, 550.0).as_slice(), &[8192]);
    }

    #[test]
    fn the_default_instance_draws_exactly_what_glyf_holds() {
        // The default instance is *defined* as the stored outline, so this must
        // hold command for command — a tolerance here would hide a delta that
        // was applied when every tuple's scalar is zero.
        let f = variable_face();
        for gid in 0..4 {
            let plain = f.outline(gid).unwrap();
            let varied = f
                .outline_at(gid, &f.variation_axes().unwrap().default_coords())
                .unwrap();
            assert_eq!(varied.commands, plain.commands, "glyph {gid}");
        }
    }

    #[test]
    fn a_phantom_delta_corrects_the_varied_side_bearing() {
        // Glyph 1's tuple moves its four corners by (10,1) (20,2) (30,3) (40,4)
        // and its *left phantom point* by (5,0). The phantom is the left side
        // bearing point, so the whole outline slides back by 5 to keep that
        // point on the origin: a corner at 100+10 draws at 105, not 110.
        let f = variable_face();
        let o = f.outline_at(1, &at_weight(&f, 700.0)).unwrap();
        assert_eq!(
            o.commands,
            vec![
                PathCmd::MoveTo(Point::new(105.0, 1.0)),
                PathCmd::LineTo(Point::new(215.0, 2.0)),
                PathCmd::LineTo(Point::new(225.0, 103.0)),
                PathCmd::LineTo(Point::new(135.0, 104.0)),
                PathCmd::LineTo(Point::new(105.0, 1.0)),
                PathCmd::Close,
            ]
        );
    }

    #[test]
    fn a_half_strength_tuple_moves_points_half_as_far() {
        // Weight 550 is exactly half way from the default to the peak, so every
        // delta — the phantom's included — is halved. This is the assertion
        // that a scalar is applied at all rather than a tuple being all or
        // nothing.
        let f = variable_face();
        let o = f.outline_at(1, &at_weight(&f, 550.0)).unwrap();
        assert_eq!(
            o.commands,
            vec![
                PathCmd::MoveTo(Point::new(102.5, 0.5)),
                PathCmd::LineTo(Point::new(207.5, 1.0)),
                PathCmd::LineTo(Point::new(212.5, 101.5)),
                PathCmd::LineTo(Point::new(117.5, 102.0)),
                PathCmd::LineTo(Point::new(102.5, 0.5)),
                PathCmd::Close,
            ]
        );
    }

    #[test]
    fn a_point_the_tuple_did_not_name_is_interpolated_not_left_behind() {
        // Glyph 2 is (0,0) — (50,200) off-curve — (100,0). The tuple names only
        // the two ends, moving the second by (100,0). Point 1 sits half way
        // between them in x, so IUP gives it half the movement: 50, putting the
        // control point at 100. Leaving it at 50 — the bug this test exists
        // for — would tear the curve away from its own endpoints.
        //
        // In y the two named points share a coordinate and a delta, so there is
        // no ratio to interpolate along and the answer is that shared delta,
        // which is zero.
        let f = variable_face();
        let o = f.outline_at(2, &at_weight(&f, 700.0)).unwrap();
        assert_eq!(
            o.commands,
            vec![
                PathCmd::MoveTo(Point::new(0.0, 0.0)),
                PathCmd::QuadTo(Point::new(100.0, 200.0), Point::new(200.0, 0.0)),
                PathCmd::LineTo(Point::new(0.0, 0.0)),
                PathCmd::Close,
            ]
        );
    }

    #[test]
    fn a_tuple_peaking_on_the_far_side_of_the_default_applies_only_there() {
        // Glyph 2's other tuple peaks at wght -1 and names point 1 alone. At
        // weight 100 it is at full strength and the bold tuple is at zero, so
        // the contour translates bodily by (100,100) — the single-named-point
        // case, where IUP has no second point to interpolate against and every
        // unnamed point takes the named one's delta whole.
        //
        // This is also the assertion that a zero-scoring tuple still occupies
        // its bytes. At *either* instance one tuple is skipped, and a reader
        // that skipped its data as well as its arithmetic would read the
        // surviving tuple's deltas from the wrong offset and draw neither of
        // these two shapes.
        let f = variable_face();
        let o = f.outline_at(2, &at_weight(&f, 100.0)).unwrap();
        assert_eq!(
            o.commands,
            vec![
                PathCmd::MoveTo(Point::new(100.0, 100.0)),
                PathCmd::QuadTo(Point::new(150.0, 300.0), Point::new(200.0, 100.0)),
                PathCmd::LineTo(Point::new(100.0, 100.0)),
                PathCmd::Close,
            ]
        );
    }

    #[test]
    fn a_composites_component_offset_varies() {
        // Glyph 3 places glyph 1 at (500,200); its one variation point is that
        // offset, moved by (25,-35) to (525,165). The component *also* varies
        // in its own right, so the corners are glyph 1's varied points — the
        // unshifted ones, since the parent's bearing correction applies once to
        // the finished glyph and not again to each part.
        let f = variable_face();
        let o = f.outline_at(3, &at_weight(&f, 700.0)).unwrap();
        assert_eq!(
            o.commands,
            vec![
                PathCmd::MoveTo(Point::new(635.0, 166.0)),
                PathCmd::LineTo(Point::new(745.0, 167.0)),
                PathCmd::LineTo(Point::new(755.0, 268.0)),
                PathCmd::LineTo(Point::new(665.0, 269.0)),
                PathCmd::LineTo(Point::new(635.0, 166.0)),
                PathCmd::Close,
            ]
        );
    }

    #[test]
    fn an_ink_box_follows_the_instance_rather_than_the_stored_header_box() {
        // The regression this pins: `glyph_bbox` reads the four words in the
        // glyph header, and `gvar` never rewrites them, so off the default
        // instance that box describes a shape the face no longer draws. Mark
        // positioning stacks accents on this box, so a stale one puts the
        // accent where the *default* weight's ink used to be — visible as an
        // accent drifting off its base as the weight is dragged.
        let f = variable_face();
        let bold = at_weight(&f, 700.0);
        let box_at = f.glyph_bbox_at(1, &bold).expect("glyph 1 has an outline");

        // Glyph 1 at weight 700 draws its corners at (105,1) (215,2) (225,103)
        // (135,104) — see `a_phantom_delta_corrects_the_varied_side_bearing`.
        // The box is their hull, which is the box a point walk can produce and
        // the one HarfBuzz produces off the default instance too.
        assert_eq!(box_at.x_min, 105.0);
        assert_eq!(box_at.y_min, 1.0);
        assert_eq!(box_at.x_max, 225.0);
        assert_eq!(box_at.y_max, 104.0);

        // And it is genuinely a different answer, not the stored box arrived at
        // by a longer route: without this assertion the test above would pass
        // just as well on a fixture whose deltas happened to cancel.
        let stored = f.glyph_bbox(1).expect("glyph 1 states a header box");
        assert_ne!(box_at, stored);
    }

    #[test]
    fn the_default_instance_still_reads_the_box_the_glyph_states() {
        // The default instance is the one case where the header box is exactly
        // right, and it is the tight box around the *curves* where a point walk
        // gives the looser control-point hull. Measuring it anyway would make
        // every static face disagree with itself about its own ink for no gain,
        // so the fast path is a correctness requirement, not an optimisation.
        let f = variable_face();
        for gid in 1..4 {
            assert_eq!(
                f.glyph_bbox_at(gid, &var::Coords::default()),
                f.glyph_bbox(gid),
                "glyph {gid} must read its stated box at the default instance"
            );
            assert_eq!(
                f.glyph_bbox_at(gid, &at_weight(&f, 400.0)),
                f.glyph_bbox(gid),
                "glyph {gid}: weight 400 *is* the default, by another spelling"
            );
        }
    }

    #[test]
    fn a_face_without_gvar_measures_the_same_box_at_every_instance() {
        // The plain fixture cannot vary, so asking it for an instance must give
        // the stated box rather than falling into the point walk and quietly
        // loosening every box in a static face.
        let f = face();
        for gid in 0..4 {
            assert_eq!(
                f.glyph_bbox_at(gid, &var::Coords::default()),
                f.glyph_bbox(gid)
            );
        }
    }

    #[test]
    fn a_face_without_gvar_draws_the_same_outline_at_every_instance() {
        // The plain fixture has neither table. Asking it for an instance must
        // not fail and must not differ — a caller should be able to route every
        // glyph through `outline_at` without first asking whether it will help.
        let f = face();
        let coords = var::Coords::default();
        for gid in 0..4 {
            assert_eq!(
                f.outline_at(gid, &coords).unwrap().commands,
                f.outline(gid).unwrap().commands
            );
        }
    }

    #[test]
    fn an_outline_lands_on_the_bearing_hmtx_states_not_its_stored_x_min() {
        // Glyph 3's header says its ink starts at 600; `hmtx` says 25. Every
        // real rasterizer believes `hmtx` and moves the outline the 575 units
        // between them — see `Face::glyf_shift`.
        let bytes = build_test_font_with_trailing_lsb(25);
        let f = Face::parse(bytes).unwrap();
        let o = f.outline(3).unwrap();
        assert_eq!(
            o.commands.first(),
            Some(&PathCmd::MoveTo(Point::new(25.0, 200.0)))
        );
        // The reported box moves with the ink, and keeps its width.
        let b = f.glyph_bbox(3).unwrap();
        assert_eq!(b.x_min, 25.0);
        assert_eq!(b.x_max, 125.0);
        // Vertically nothing moved.
        assert_eq!(b.y_min, 200.0);
        assert_eq!(b.y_max, 300.0);
        // A glyph whose two numbers agree is untouched.
        assert_eq!(f.glyph_bbox(1).unwrap().x_min, 100.0);
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

    // -- `name` table --------------------------------------------------

    /// One `name` record, ready to be assembled by [`name_table`].
    struct NameRec {
        platform: u16,
        encoding: u16,
        language: u16,
        name_id: u16,
        /// The string exactly as it sits in the file — this deliberately takes
        /// raw bytes, because half the point of these tests is what happens
        /// when the bytes are *not* a well-formed string.
        bytes: Vec<u8>,
    }

    /// Assemble a `name` table (format 0) from records.
    fn name_table(recs: &[NameRec]) -> Vec<u8> {
        let count = u16::try_from(recs.len()).unwrap();
        let storage_off = 6 + 12 * usize::from(count);
        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_be_bytes()); // format
        out.extend_from_slice(&count.to_be_bytes());
        out.extend_from_slice(&u16::try_from(storage_off).unwrap().to_be_bytes());

        let mut storage = Vec::new();
        for r in recs {
            out.extend_from_slice(&r.platform.to_be_bytes());
            out.extend_from_slice(&r.encoding.to_be_bytes());
            out.extend_from_slice(&r.language.to_be_bytes());
            out.extend_from_slice(&r.name_id.to_be_bytes());
            out.extend_from_slice(&u16::try_from(r.bytes.len()).unwrap().to_be_bytes());
            out.extend_from_slice(&u16::try_from(storage.len()).unwrap().to_be_bytes());
            storage.extend_from_slice(&r.bytes);
        }
        out.extend_from_slice(&storage);
        out
    }

    fn utf16be(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_be_bytes).collect()
    }

    fn rec(platform: u16, encoding: u16, language: u16, name_id: u16, bytes: Vec<u8>) -> NameRec {
        NameRec {
            platform,
            encoding,
            language,
            name_id,
            bytes,
        }
    }

    #[test]
    fn a_name_is_read_from_its_best_encoded_copy() {
        // A shipped face carries the same name once per market it was built
        // for, in no particular order. Taking the first match would make the
        // family name depend on the font vendor's record ordering.
        let table = name_table(&[
            rec(1, 0, 0, name_id::FAMILY, b"MacFamily".to_vec()),
            rec(3, 1, 0x0407, name_id::FAMILY, utf16be("DeutschFamilie")),
            rec(3, 1, 0x0409, name_id::FAMILY, utf16be("Inter")),
            rec(0, 3, 0, name_id::FAMILY, utf16be("UnicodeFamily")),
        ]);
        assert_eq!(read_name(&table, name_id::FAMILY).as_deref(), Some("Inter"));
    }

    #[test]
    fn a_mac_only_name_decodes_through_mac_roman() {
        // The bug this prevents: treating MacRoman's high half as ASCII or as
        // Latin-1. 0x8E is 'é' in MacRoman but 'Ž' in Latin-1, and a face whose
        // only name record is a Mac one is common in older files.
        let table = name_table(&[rec(1, 0, 0, name_id::FAMILY, b"Caf\x8e".to_vec())]);
        assert_eq!(read_name(&table, name_id::FAMILY).as_deref(), Some("Café"));
    }

    #[test]
    fn a_name_in_an_encoding_we_cannot_read_is_absent_not_wrong() {
        // Windows encoding 2 is Shift-JIS, not UTF-16. Decoding it as UTF-16
        // would produce a plausible-looking wrong name that silently fails to
        // match the family it belongs to.
        let table = name_table(&[
            rec(3, 2, 0x0411, name_id::FAMILY, b"\x82\xa0\x82\xa2".to_vec()),
            rec(1, 32, 0, name_id::FAMILY, b"whatever".to_vec()),
        ]);
        assert_eq!(read_name(&table, name_id::FAMILY), None);
    }

    #[test]
    fn a_record_pointing_outside_the_table_is_skipped() {
        // Offsets in a `name` record are attacker-controlled in the sense that
        // matters here: they come from a file the user chose.
        let mut table = name_table(&[
            rec(3, 1, 0x0409, name_id::FAMILY, utf16be("Good")),
            rec(3, 1, 0x0409, name_id::POSTSCRIPT, utf16be("Good-Regular")),
        ]);
        // Rewrite the first record's length to run off the end.
        let len_at = 6 + 8;
        table[len_at..len_at + 2].copy_from_slice(&0xFFFFu16.to_be_bytes());
        assert_eq!(read_name(&table, name_id::FAMILY), None);
        assert_eq!(
            read_name(&table, name_id::POSTSCRIPT).as_deref(),
            Some("Good-Regular"),
            "one bad record must not cost the whole table"
        );
    }

    #[test]
    fn an_odd_length_utf16_string_is_rejected() {
        let table = name_table(&[rec(3, 1, 0x0409, name_id::FAMILY, b"\x00A\x00".to_vec())]);
        assert_eq!(read_name(&table, name_id::FAMILY), None);
    }

    #[test]
    fn an_unpaired_surrogate_is_rejected() {
        // A lone high surrogate is not a character. Substituting U+FFFD would
        // let a corrupt name compare unequal to itself across encodings.
        let table = name_table(&[rec(
            3,
            1,
            0x0409,
            name_id::FAMILY,
            alloc::vec![0xD8, 0x00, 0x00, 0x41],
        )]);
        assert_eq!(read_name(&table, name_id::FAMILY), None);
    }

    #[test]
    fn a_face_with_no_name_table_reports_no_names() {
        // `build_test_font` has no `name` table, which is exactly the case a
        // font picker must survive: names are optional, drawing is not.
        let face = Face::parse(build_test_font()).unwrap();
        assert_eq!(face.family(), None);
        assert_eq!(face.subfamily(), None);
        assert_eq!(face.postscript_name(), None);
        assert!(face.outline(face.glyph_index('A').unwrap()).is_ok());
    }

    #[test]
    fn the_typographic_family_wins_over_the_legacy_one() {
        // The whole reason both exist: id 1 must fit the four-style model, so
        // a large family is split across several id-1 names. A font menu wants
        // the id-16 grouping.
        let table = name_table(&[
            rec(3, 1, 0x0409, name_id::FAMILY, utf16be("Inter SemiBold")),
            rec(3, 1, 0x0409, name_id::SUBFAMILY, utf16be("Regular")),
            rec(3, 1, 0x0409, name_id::TYPOGRAPHIC_FAMILY, utf16be("Inter")),
            rec(
                3,
                1,
                0x0409,
                name_id::TYPOGRAPHIC_SUBFAMILY,
                utf16be("SemiBold"),
            ),
        ]);
        assert_eq!(
            read_name(&table, name_id::TYPOGRAPHIC_FAMILY).as_deref(),
            Some("Inter")
        );
        assert_eq!(
            read_name(&table, name_id::FAMILY).as_deref(),
            Some("Inter SemiBold")
        );
    }

    // ---- OS/2 style ------------------------------------------------------

    /// A `head` table long enough to hold `macStyle`, with those flags set.
    fn head_with(mac_style: u16) -> Vec<u8> {
        let mut head = vec![0_u8; 54];
        head.splice(44..46, mac_style.to_be_bytes());
        head
    }

    /// An `OS/2` table long enough to hold `fsSelection`.
    fn os2_with(weight: u16, width: u16, fs_selection: u16) -> Vec<u8> {
        let mut os2 = vec![0_u8; 78];
        os2.splice(4..6, weight.to_be_bytes());
        os2.splice(6..8, width.to_be_bytes());
        os2.splice(62..64, fs_selection.to_be_bytes());
        os2
    }

    #[test]
    fn os2_reports_the_face_style() {
        let head = head_with(0);
        let os2 = os2_with(700, 5, 0x0020);
        let style = Face::parse_style(Some(&os2), &head);
        assert_eq!(style.weight, Style::BOLD);
        assert!(!style.italic);
        assert_eq!(style.width, Style::NORMAL_WIDTH);
    }

    #[test]
    fn italic_is_read_from_either_flag_that_means_it() {
        let head = head_with(0);
        // fsSelection bit 0 is italic, bit 9 is oblique; both mean slanted,
        // and a face setting only the old macStyle bit must not read upright.
        for (os2_bits, mac, why) in [
            (0x0001, 0, "fsSelection italic"),
            (0x0200, 0, "fsSelection oblique"),
            (0x0000, 0x0002, "macStyle italic with OS/2 silent"),
        ] {
            let os2 = os2_with(400, 5, os2_bits);
            let style = Face::parse_style(Some(&os2), &head_with(mac));
            assert!(style.italic, "{why} was not read as italic");
        }
        assert!(!Face::parse_style(Some(&os2_with(400, 5, 0)), &head).italic);
    }

    #[test]
    fn a_face_with_no_os2_falls_back_to_macstyle() {
        // OS/2 is optional in the spec and absent from some older and
        // Apple-only faces; macStyle is the only style information they have.
        let bold = Face::parse_style(None, &head_with(0x0001));
        assert_eq!(bold.weight, Style::BOLD);
        assert!(!bold.italic);

        let italic = Face::parse_style(None, &head_with(0x0002));
        assert_eq!(italic.weight, Style::REGULAR);
        assert!(italic.italic);

        let plain = Face::parse_style(None, &head_with(0));
        assert_eq!(plain, Style::default());
    }

    #[test]
    fn the_old_one_to_nine_weight_scale_is_recognised() {
        // Pre-OpenType files used 1..=9 for the same axis. Read literally, a
        // `7` would be lighter than the lightest CSS weight and the face
        // would never be chosen as a family's bold.
        for (raw, expected) in [(1_u16, 100_u16), (4, 400), (7, 700), (9, 900)] {
            let style = Face::parse_style(Some(&os2_with(raw, 5, 0)), &head_with(0));
            assert_eq!(style.weight, expected, "usWeightClass {raw}");
        }
    }

    #[test]
    fn an_unusable_weight_falls_back_rather_than_being_believed() {
        // Some generators write 0 for "unspecified". Believing it would file
        // the face as lighter than Thin.
        let regular = Face::parse_style(Some(&os2_with(0, 5, 0)), &head_with(0));
        assert_eq!(regular.weight, Style::REGULAR);
        let bold = Face::parse_style(Some(&os2_with(0, 5, 0)), &head_with(0x0001));
        assert_eq!(bold.weight, Style::BOLD, "macStyle said bold");
        // Absurdly large values are clamped to the top of the scale rather
        // than wrapping or being discarded.
        assert_eq!(
            Face::parse_style(Some(&os2_with(65535, 5, 0)), &head_with(0)).weight,
            1000
        );
    }

    #[test]
    fn an_out_of_range_width_reads_as_normal() {
        // A face that cannot describe its width must not be filed as
        // ultra-condensed, or it is excluded from every ordinary match.
        for raw in [0_u16, 10, 999] {
            let style = Face::parse_style(Some(&os2_with(400, raw, 0)), &head_with(0));
            assert_eq!(style.width, Style::NORMAL_WIDTH, "usWidthClass {raw}");
        }
        assert_eq!(
            Face::parse_style(Some(&os2_with(400, 3, 0)), &head_with(0)).width,
            3,
            "a condensed face keeps its width"
        );
    }

    #[test]
    fn a_truncated_os2_does_not_panic_or_lie() {
        // Version 0 of OS/2 is 78 bytes, but files exist that are shorter
        // than their own version claims. Every field read must degrade to the
        // fallback rather than reading past the end.
        for len in [0_usize, 2, 5, 7, 61, 63] {
            let os2 = os2_with(700, 3, 0x0001);
            let truncated = os2.get(..len).expect("shorter than the table");
            let style = Face::parse_style(Some(truncated), &head_with(0));
            // Whatever it could not read must have taken a defined default.
            assert!(style.weight >= 100 && style.weight <= 1000);
            assert!((1..=9).contains(&style.width));
        }
    }

    #[test]
    fn the_synthetic_face_reports_a_default_style() {
        // It carries no OS/2 and a zeroed macStyle, which is the case every
        // other test in this file runs against.
        let face = Face::parse(build_test_font()).expect("parse");
        assert_eq!(face.style(), Style::default());
    }
}
