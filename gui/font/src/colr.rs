//! Colour glyphs: `COLR`, versions 0 and 1, with the palettes of `CPAL`.
//!
//! An emoji face does not store pictures. It stores, for each emoji, a recipe
//! for painting one out of ordinary glyph outlines:
//!
//! * **Version 0** is a list of layers, bottom first -- an outline and a
//!   colour each, the colour an index into the face's palette. A smiling
//!   face is a yellow disc, then two brown eyes, then a brown mouth.
//! * **Version 1** is a *paint graph*: a tree whose leaves are fills (a solid
//!   colour, or a linear, radial or sweep gradient along a "colour line" of
//!   stops), whose `PaintGlyph` nodes clip what is below them to an outline,
//!   and whose other nodes move what is below them (translate, scale,
//!   rotate, skew, any affine), paint several things in turn (a layer list),
//!   reuse another glyph's graph, or combine two paintings with one of
//!   twenty-eight compositing modes. Noto Color Emoji's vector build is this.
//!
//! [`render`] draws either kind into a [`ColourImage`] at a pixel size. The
//! arithmetic is floating point throughout -- there is no one reference
//! rendering to match bit for bit, as there is for the image codecs: every
//! engine rasterizes outlines its own way -- and follows the specification's
//! definitions, with the transform conventions of fontTools, which the fonts
//! are built with: skews by `tan(-x)` and `tan(y)`, rotation
//! counter-clockwise, a transform's own paint drawn in the space it
//! transforms, and a sweep gradient's angles stored less 180 degrees (the
//! "bias" that lets a full turn fit in the field).
//!
//! # Colour
//!
//! Everything is painted in premultiplied RGBA, and gradients interpolate
//! premultiplied -- the choice that does not darken a stop fading to
//! transparent. Palette entry `0xFFFF` is the text's own colour, which is why
//! the result says whether it was used ([`ColourImage::uses_foreground`]):
//! a glyph that never reaches for it can be drawn once and reused in any
//! colour of text.
//!
//! # Cost
//!
//! A layer is rasterized over the pixels its outline's box touches, not the
//! whole canvas, since most layers of an emoji are small parts of it -- an
//! eye, a highlight. A fill under a `PaintGlyph` (through any transforms) is
//! evaluated only where the outline covers; only a `PaintGlyph` over a more
//! complicated graph, and a composite, paint into scratch canvases.
//!
//! # Hostile fonts
//!
//! A paint graph is a graph, not a tree -- `PaintColrGlyph` can name a glyph
//! whose graph names the first -- and a layer list can be long. Rendering
//! stops going deeper than [`MAX_DEPTH`], stops visiting after
//! [`MAX_PAINTS`] nodes, and stops painting once it has touched
//! [`WORK_PER_PIXEL`] times as many pixels as the canvas has, so a cycle or a
//! fan-out costs a bounded amount and draws what it had got to. Scratch
//! canvases come out of a fixed budget ([`CANVAS_BUDGET`]); a composite that
//! would exceed it is skipped. Every offset is bounds-checked, and the canvas
//! is at most [`MAX_COLOUR_PIXELS`].
//!
//! # Variations
//!
//! A variable colour font moves its paints with its axes: every `Var` paint
//! format, `VarColorStop` and format-2 clip box ends in a `varIndexBase`, and
//! its *n*th field moves by the delta the variation store holds for index
//! `varIndexBase + n` -- through the `DeltaSetIndexMap` if the table has one,
//! as the outer and inner halves of the index if not -- in the field's own
//! units: whole font units for a coordinate, 1/16384 for an `F2DOT14`, 1/65536
//! for a `Fixed`. At the default instance nothing is read.
//!
//! Not modelled: only the first palette is used, and a `PaintColrGlyph` does
//! not apply the clip box of the glyph it names.

use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::f32::consts::{PI, TAU};

use crate::raster::{coverage_on, mad};
use crate::sfnt::{Face, Outline, PathCmd, Point};
use crate::var::Coords;
use crate::varstore::{IndexMap, VarStore};

/// How deep a paint graph is followed.
pub const MAX_DEPTH: u32 = 64;

/// How many paint nodes one glyph may visit in all.
pub const MAX_PAINTS: usize = 20_000;

/// The largest colour glyph drawn, in pixels: 1024 by 1024. [`render`]
/// declines a bigger one, and the caller draws the outline instead.
pub const MAX_COLOUR_PIXELS: usize = 1 << 20;

/// How many canvas pixels may exist at once -- the glyph's own canvas and the
/// scratch ones -- before a node that needs another is skipped. Four
/// full-size canvases, 64 MiB at 16 bytes a pixel.
pub const CANVAS_BUDGET: usize = 4 * MAX_COLOUR_PIXELS;

/// How many times over a glyph's canvas its painting may touch every pixel.
/// Noto's busiest emoji touch well under ten.
pub const WORK_PER_PIXEL: usize = 64;

/// A colour glyph drawn at a size, placed as a
/// [`GlyphMask`](crate::raster::GlyphMask) is: `left` and `top` from the pen
/// position on the baseline, y growing downward.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColourImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// X offset of the image's left edge from the pen position.
    pub left: i32,
    /// Y offset of the image's top edge from the baseline, downward-positive.
    pub top: i32,
    /// Row-major **premultiplied** `0xAARRGGBB`.
    pub pixels: Vec<u32>,
    /// Whether the glyph painted with the text's colour (palette entry
    /// `0xFFFF`), so that it looks different in different colours of text.
    pub uses_foreground: bool,
}

/// Whether `face` has a colour recipe for glyph `gid`.
#[must_use]
pub fn has_colour(face: &Face, gid: u16) -> bool {
    Tables::of(face, &Coords::default())
        .is_some_and(|t| t.base_v1(gid).is_some() || t.base_v0(gid).is_some())
}

/// Glyph `gid` of `face` painted in colour at `scale` pixels per font unit,
/// at variation instance `coords`, with `foreground` (straight `0xAARRGGBB`)
/// as the text colour.
///
/// `None` if the face has no colour recipe for the glyph, or the glyph would
/// be bigger than [`MAX_COLOUR_PIXELS`] -- the caller's cue to draw its
/// outline instead. A recipe that paints nothing is an empty image.
#[must_use]
pub fn render(
    face: &Face,
    gid: u16,
    scale: f32,
    coords: &Coords,
    foreground: u32,
) -> Option<ColourImage> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let tables = Tables::of(face, coords)?;
    let root = if let Some(paint) = tables.base_v1(gid) {
        Root::Paint(paint)
    } else {
        Root::Layers(tables.base_v0(gid)?)
    };
    let mut r = Renderer {
        t: &tables,
        face,
        coords,
        palette: tables.cpal.and_then(Palette::zero).unwrap_or_default(),
        foreground: premultiply(unpack(foreground)),
        used_foreground: false,
        visited: 0,
        work: 0,
        width: 0,
        height: 0,
        live: 0,
    };
    // Font units to pixels, y down; the canvas' own origin comes later.
    let flip = Affine::scale(scale, -scale);
    // The canvas: the glyph's clip box where it has one, the union of what
    // its outlines cover otherwise.
    let bounds = if let Some(clip) = tables.clip_box(gid) {
        clip.map(flip)
    } else {
        let mut b = Rect::EMPTY;
        match root {
            Root::Paint(paint) => r.bounds(paint, flip, &mut b, 0),
            Root::Layers(layers) => {
                for (layer, _) in tables.layers_v0(layers) {
                    if let Some(outline) = r.outline(layer) {
                        b = b.union(outline_rect(&outline).map(flip));
                    }
                }
            }
        }
        b
    };
    let (left, top) = (snap_down(bounds.min_x), snap_down(bounds.min_y));
    let (w, h) = (snap_up(bounds.max_x) - left, snap_up(bounds.max_y) - top);
    if bounds.is_empty() || w < 1.0 || h < 1.0 {
        return Some(ColourImage::default());
    }
    // A non-empty box is at least a pixel each way, so the product bounds
    // both sides; the offsets must fit an `i32` as well. (Each comparison is
    // false for a NaN.)
    #[allow(clippy::cast_precision_loss, reason = "a power of two, exact")]
    let fits = w * h <= MAX_COLOUR_PIXELS as f32 && left.abs() <= 1.0e8 && top.abs() <= 1.0e8;
    if !fits {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "whole numbers from 1 to MAX_COLOUR_PIXELS, checked above"
    )]
    let (width, height) = (w as usize, h as usize);
    r.width = width;
    r.height = height;
    r.work = WORK_PER_PIXEL.saturating_mul(width.saturating_mul(height));
    let root_m = Affine::translate(-left, -top).then(flip);
    let mut canvas = r.canvas()?;
    r.visited = 0;
    match root {
        Root::Paint(paint) => r.paint(paint, root_m, &mut canvas, 0),
        Root::Layers(layers) => {
            for (layer, colour) in tables.layers_v0(layers) {
                let fill = Fill::Solid(r.palette_colour(colour, 1.0));
                r.glyph(layer, root_m, &fill, &mut canvas);
            }
        }
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "width and height are at most MAX_COLOUR_PIXELS, left and top at most 1e8"
    )]
    Some(ColourImage {
        width: width as u32,
        height: height as u32,
        left: left as i32,
        top: top as i32,
        pixels: canvas.px.iter().map(|&c| pack(c)).collect(),
        uses_foreground: r.used_foreground,
    })
}

/// Where a glyph's recipe starts.
#[derive(Clone, Copy)]
enum Root {
    /// A version-1 paint, at this offset in `COLR`.
    Paint(usize),
    /// Version-0 layers: `(first, count)` in the layer records.
    Layers((usize, usize)),
}

// ---------------------------------------------------------------------------
// Reading the tables
// ---------------------------------------------------------------------------

fn u8_at(d: &[u8], at: usize) -> Option<u8> {
    d.get(at).copied()
}

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        d.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn i16_at(d: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_be_bytes(
        d.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

/// An `Offset24`, as a `usize`.
fn u24_at(d: &[u8], at: usize) -> Option<usize> {
    let b: [u8; 3] = d.get(at..at.checked_add(3)?)?.try_into().ok()?;
    Some(usize::from(b[0]) << 16 | usize::from(b[1]) << 8 | usize::from(b[2]))
}

/// A `uint32` or `Offset32`, as a `usize`.
fn u32_at(d: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32::from_be_bytes(
        d.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
    .ok()
}

fn i32_at(d: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_be_bytes(
        d.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// An `F2DOT14`: a signed fixed-point number with 14 fractional bits.
fn f2dot14(d: &[u8], at: usize) -> Option<f32> {
    Some(f32::from(i16_at(d, at)?) / 16384.0)
}

/// A `Fixed`: 16.16.
#[allow(
    clippy::cast_precision_loss,
    reason = "a transform coefficient needs no more than f32 holds"
)]
fn fixed(d: &[u8], at: usize) -> Option<f32> {
    Some(i32_at(d, at)? as f32 / 65536.0)
}

/// An `FWORD`: a signed coordinate in font units.
fn fword(d: &[u8], at: usize) -> Option<f32> {
    Some(f32::from(i16_at(d, at)?))
}

/// A `UFWORD`: an unsigned distance in font units.
fn ufword(d: &[u8], at: usize) -> Option<f32> {
    Some(f32::from(u16_at(d, at)?))
}

/// A relative offset from `base`, as an absolute position in `COLR`; the
/// null offset is no position.
fn at_offset(base: usize, offset: usize) -> Option<usize> {
    if offset == 0 {
        None
    } else {
        base.checked_add(offset)
    }
}

/// The first of `count` records of `stride` bytes from `first`, each
/// starting with a glyph id and sorted by it, whose glyph id is `gid`.
fn find_record(d: &[u8], first: usize, stride: usize, count: usize, gid: u16) -> Option<usize> {
    let (mut lo, mut hi) = (0usize, count);
    while lo < hi {
        let mid = lo.midpoint(hi);
        let at = first.checked_add(mid.checked_mul(stride)?)?;
        match u16_at(d, at)?.cmp(&gid) {
            Ordering::Less => lo = mid.saturating_add(1),
            Ordering::Greater => hi = mid,
            Ordering::Equal => return Some(at),
        }
    }
    None
}

/// `COLR`, ready to read, and the face's `CPAL`.
struct Tables<'a> {
    /// The `COLR` table itself; every position below is in it.
    colr: &'a [u8],
    cpal: Option<&'a [u8]>,
    /// The version-0 base glyph records: `(position, count)`.
    base_v0: Option<(usize, usize)>,
    /// The version-0 layer records: `(position, count)`.
    layers_v0: Option<(usize, usize)>,
    base_list: Option<usize>,
    layer_list: Option<usize>,
    clip_list: Option<usize>,
    /// The instance drawn, normalized.
    coords: &'a [i16],
    /// The variation store and index map, for a variable font drawn away
    /// from its default instance; `None` otherwise, when nothing moves.
    var: Option<(VarStore, Option<IndexMap>)>,
}

/// A `varIndexBase` that names no variation.
const NO_VARIATION: u32 = 0xFFFF_FFFF;

impl<'a> Tables<'a> {
    fn of(face: &'a Face, coords: &'a Coords) -> Option<Self> {
        let (colr, cpal) = face.colour_tables()?;
        let version = u16_at(colr, 0)?;
        let records =
            |offset: usize, count: usize| (offset != 0 && count != 0).then_some((offset, count));
        let base_v0 = records(u32_at(colr, 4)?, usize::from(u16_at(colr, 2)?));
        let layers_v0 = records(u32_at(colr, 8)?, usize::from(u16_at(colr, 12)?));
        let list = |at: usize| u32_at(colr, at).filter(|&v| v != 0);
        let (base_list, layer_list, clip_list) = if version >= 1 {
            (list(14), list(18), list(22))
        } else {
            (None, None, None)
        };
        let var = if version >= 1 && !coords.is_default() {
            let axes = face.variation_axes().map_or(0, |v| v.axes().len());
            list(30)
                .and_then(|store| VarStore::parse(colr, store, axes))
                .map(|store| (store, list(26).and_then(|map| IndexMap::parse(colr, map))))
        } else {
            None
        };
        Some(Self {
            colr,
            cpal,
            base_v0,
            layers_v0,
            base_list,
            layer_list,
            clip_list,
            coords: coords.as_slice(),
            var,
        })
    }

    /// The `varIndexBase` at `at`, or none if the field is not there.
    fn var_base(&self, at: Option<usize>) -> u32 {
        at.and_then(|at| u32_at(self.colr, at))
            .and_then(|base| u32::try_from(base).ok())
            .unwrap_or(NO_VARIATION)
    }

    /// How far field `i` of a table whose `varIndexBase` is `base` moves at
    /// this instance, in the field's raw units. Nothing, for a font that does
    /// not vary, at the default instance, or for an index with no row.
    fn delta(&self, base: u32, i: u32) -> f32 {
        let Some((store, map)) = &self.var else {
            return 0.0;
        };
        if base == NO_VARIATION {
            return 0.0;
        }
        let Some(index) = base.checked_add(i) else {
            return 0.0;
        };
        let row = match map {
            Some(map) => map.get(self.colr, index),
            // No map: the index is the row, outer half and inner half.
            None => u16::try_from(index >> 16)
                .ok()
                .zip(u16::try_from(index & 0xFFFF).ok()),
        };
        row.and_then(|(outer, inner)| store.delta(self.colr, outer, inner, self.coords))
            .unwrap_or(0.0)
    }

    /// The version-1 paint of base glyph `gid`, from the BaseGlyphList.
    fn base_v1(&self, gid: u16) -> Option<usize> {
        let list = self.base_list?;
        let count = u32_at(self.colr, list)?;
        let record = find_record(self.colr, list.checked_add(4)?, 6, count, gid)?;
        at_offset(list, u32_at(self.colr, record.checked_add(2)?)?)
    }

    /// The version-0 layers of base glyph `gid`: `(first, count)`.
    fn base_v0(&self, gid: u16) -> Option<(usize, usize)> {
        let (records, count) = self.base_v0?;
        let record = find_record(self.colr, records, 6, count, gid)?;
        let first = u16_at(self.colr, record.checked_add(2)?)?;
        let count = u16_at(self.colr, record.checked_add(4)?)?;
        Some((usize::from(first), usize::from(count)))
    }

    /// Version-0 layer records `first..first + count`, `(glyph, palette
    /// index)` each, stopping at the end of the records.
    fn layers_v0(&self, (first, count): (usize, usize)) -> impl Iterator<Item = (u16, u16)> + '_ {
        let (records, n) = self.layers_v0.unwrap_or((0, 0));
        (first..first.saturating_add(count))
            .take_while(move |&i| i < n)
            .map_while(move |i| {
                let at = records.checked_add(i.checked_mul(4)?)?;
                Some((
                    u16_at(self.colr, at)?,
                    u16_at(self.colr, at.checked_add(2)?)?,
                ))
            })
    }

    /// The paint of layer `i` of the LayerList.
    fn layer_v1(&self, i: usize) -> Option<usize> {
        let list = self.layer_list?;
        if i >= u32_at(self.colr, list)? {
            return None;
        }
        let at = list.checked_add(4)?.checked_add(i.checked_mul(4)?)?;
        at_offset(list, u32_at(self.colr, at)?)
    }

    /// The clip box of base glyph `gid`, in font units, if the ClipList
    /// gives it one.
    fn clip_box(&self, gid: u16) -> Option<Rect> {
        let d = self.colr;
        let list = self.clip_list?;
        if u8_at(d, list)? != 1 {
            return None;
        }
        let count = u32_at(d, list.checked_add(1)?)?;
        // Sorted by first glyph and not overlapping: the last clip starting
        // at or before `gid` is the only one that can hold it.
        let clips = list.checked_add(5)?;
        let (mut lo, mut hi) = (0usize, count);
        while lo < hi {
            let mid = lo.midpoint(hi);
            if u16_at(d, clips.checked_add(mid.checked_mul(7)?)?)? <= gid {
                lo = mid.saturating_add(1);
            } else {
                hi = mid;
            }
        }
        let at = clips.checked_add(lo.checked_sub(1)?.checked_mul(7)?)?;
        if u16_at(d, at.checked_add(2)?)? < gid {
            return None;
        }
        let clip = at_offset(list, u24_at(d, at.checked_add(4)?)?)?;
        // Formats 1 and 2 share the box; 2 adds a variation index.
        let format = u8_at(d, clip)?;
        if !matches!(format, 1 | 2) {
            return None;
        }
        let base = self.var_base((format == 2).then(|| clip.checked_add(9)).flatten());
        let v = |k: usize, i: u32| Some(fword(d, clip.checked_add(k)?)? + self.delta(base, i));
        Some(Rect {
            min_x: v(1, 0)?,
            min_y: v(3, 1)?,
            max_x: v(5, 2)?,
            max_y: v(7, 3)?,
        })
    }

    /// The paint record at `at`; `None` if it is truncated, of a format this
    /// does not know, or points nowhere it must.
    fn node(&self, at: usize) -> Option<Node> {
        let d = self.colr;
        let field = |k: usize| at.checked_add(k);
        let child = |k: usize| at_offset(at, u24_at(d, field(k)?)?);
        let format = u8_at(d, at)?;
        // The variable formats are the odd ones from 3 to 31; each ends in a
        // `varIndexBase`, at an offset that depends on the format.
        let base = self.var_base(match format {
            3 => field(5),
            5 | 7 => field(16),
            9 => field(12),
            _ => None,
        });
        // Field `k`, moved by delta `i`: in font units, or in 1/16384 for an
        // `F2DOT14`.
        let word = |k: usize, i: u32| Some(fword(d, field(k)?)? + self.delta(base, i));
        let uword = |k: usize, i: u32| Some(ufword(d, field(k)?)? + self.delta(base, i));
        let frac = |k: usize, i: u32| Some(f2dot14(d, field(k)?)? + self.delta(base, i) / 16384.0);
        let point = |k: usize, i: u32| {
            Some(Point::new(
                word(k, i)?,
                word(k.checked_add(2)?, i.checked_add(1)?)?,
            ))
        };
        Some(match format {
            1 => Node::Layers {
                count: usize::from(u8_at(d, field(1)?)?),
                first: u32_at(d, field(2)?)?,
            },
            2 | 3 => Node::Solid {
                index: u16_at(d, field(1)?)?,
                alpha: frac(3, 0)?,
            },
            4 | 5 => Node::Gradient {
                line: child(1)?,
                variable: format == 5,
                gradient: Gradient::linear(point(4, 0)?, point(8, 2)?, point(12, 4)?),
            },
            6 | 7 => Node::Gradient {
                line: child(1)?,
                variable: format == 7,
                gradient: Gradient::radial(
                    point(4, 0)?,
                    uword(8, 2)?.max(0.0),
                    point(10, 3)?,
                    uword(14, 5)?.max(0.0),
                ),
            },
            8 | 9 => Node::Gradient {
                line: child(1)?,
                variable: format == 9,
                // Stored less a half-turn, so that 0 to 360 degrees fits
                // the field's -2 to 2.
                gradient: Gradient::Sweep {
                    c: point(4, 0)?,
                    start: (frac(8, 2)? + 1.0) * PI,
                    end: (frac(10, 3)? + 1.0) * PI,
                },
            },
            10 => Node::Glyph {
                paint: child(1)?,
                gid: u16_at(d, field(4)?)?,
            },
            11 => Node::ColrGlyph {
                gid: u16_at(d, field(1)?)?,
            },
            12..=31 => Node::Transform {
                paint: child(1)?,
                transform: self.transform(format, at)?,
            },
            32 => Node::Composite {
                source: child(1)?,
                mode: u8_at(d, field(4)?)?,
                backdrop: child(5)?,
            },
            _ => return None,
        })
    }

    /// The transform of paint `at`, of format 12 to 31.
    fn transform(&self, format: u8, at: usize) -> Option<Affine> {
        let d = self.colr;
        let field = |k: usize| at.checked_add(k);
        // Where the variable form keeps its `varIndexBase`: after its last
        // field (for 13, in the `VarAffine2x3` it points at, read below).
        let base = self.var_base(match format {
            21 | 25 => field(6),
            15 | 17 | 29 => field(8),
            23 | 27 => field(10),
            19 | 31 => field(12),
            _ => None,
        });
        let word = |k: usize, i: u32| Some(fword(d, field(k)?)? + self.delta(base, i));
        let frac = |k: usize, i: u32| Some(f2dot14(d, field(k)?)? + self.delta(base, i) / 16384.0);
        Some(match format {
            12 | 13 => {
                let t = at_offset(at, u24_at(d, field(4)?)?)?;
                let base = self.var_base((format == 13).then(|| t.checked_add(24)).flatten());
                let f = |k: usize, i: u32| {
                    Some(fixed(d, t.checked_add(k)?)? + self.delta(base, i) / 65536.0)
                };
                Affine {
                    xx: f(0, 0)?,
                    yx: f(4, 1)?,
                    xy: f(8, 2)?,
                    yy: f(12, 3)?,
                    dx: f(16, 4)?,
                    dy: f(20, 5)?,
                }
            }
            14 | 15 => Affine::translate(word(4, 0)?, word(6, 1)?),
            16 | 17 => Affine::scale(frac(4, 0)?, frac(6, 1)?),
            18 | 19 => Affine::around(
                Affine::scale(frac(4, 0)?, frac(6, 1)?),
                word(8, 2)?,
                word(10, 3)?,
            ),
            20 | 21 => Affine::scale(frac(4, 0)?, frac(4, 0)?),
            22 | 23 => Affine::around(
                Affine::scale(frac(4, 0)?, frac(4, 0)?),
                word(6, 1)?,
                word(8, 2)?,
            ),
            24 | 25 => Affine::rotate(frac(4, 0)?),
            26 | 27 => Affine::around(Affine::rotate(frac(4, 0)?), word(6, 1)?, word(8, 2)?),
            28 | 29 => Affine::skew(frac(4, 0)?, frac(6, 1)?),
            30 | 31 => Affine::around(
                Affine::skew(frac(4, 0)?, frac(6, 1)?),
                word(8, 2)?,
                word(10, 3)?,
            ),
            _ => return None,
        })
    }
}

/// A paint record, read.
#[derive(Clone, Copy)]
enum Node {
    /// Format 1: LayerList entries `first..first + count`, bottom first.
    Layers { first: usize, count: usize },
    /// Formats 2 and 3.
    Solid { index: u16, alpha: f32 },
    /// Formats 4 to 9: the colour line at `line` (with variation indices if
    /// `variable`) along `gradient`.
    Gradient {
        line: usize,
        variable: bool,
        gradient: Gradient,
    },
    /// Format 10: `paint`, cut to glyph `gid`'s outline.
    Glyph { paint: usize, gid: u16 },
    /// Format 11: another base glyph's graph.
    ColrGlyph { gid: u16 },
    /// Formats 12 to 31: `paint`, drawn in the space `transform` makes.
    Transform { paint: usize, transform: Affine },
    /// Format 32.
    Composite {
        source: usize,
        mode: u8,
        backdrop: usize,
    },
}

/// `CPAL`'s first palette, read an entry at a time as the glyph asks for it.
///
/// Not converted up front: Segoe UI Emoji's palette has 65,429 entries and a
/// glyph uses a few dozen, so building the whole palette for every glyph drawn
/// cost more than painting most of them.
#[derive(Clone, Copy, Default)]
struct Palette<'a> {
    cpal: &'a [u8],
    /// Where palette 0's first colour record is.
    records: usize,
    /// How many entries a palette has.
    entries: usize,
}

impl<'a> Palette<'a> {
    fn zero(cpal: &'a [u8]) -> Option<Self> {
        let entries = usize::from(u16_at(cpal, 2)?);
        if u16_at(cpal, 4)? == 0 {
            return None;
        }
        let first = usize::from(u16_at(cpal, 12)?);
        let records = u32_at(cpal, 8)?.checked_add(first.checked_mul(4)?)?;
        Some(Self {
            cpal,
            records,
            entries,
        })
    }

    /// Entry `index`, straight RGBA from 0 to 1; `None` past the palette.
    fn get(&self, index: usize) -> Option<[f32; 4]> {
        if index >= self.entries {
            return None;
        }
        let at = self.records.checked_add(index.checked_mul(4)?)?;
        // Stored blue, green, red, alpha.
        let [b, g, r, a]: [u8; 4] = self.cpal.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some([r, g, b, a].map(|c| f32::from(c) / 255.0))
    }
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// An affine transform: `(xx x + xy y + dx, yx x + yy y + dy)`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Affine {
    xx: f32,
    yx: f32,
    xy: f32,
    yy: f32,
    dx: f32,
    dy: f32,
}

impl Affine {
    const fn translate(dx: f32, dy: f32) -> Self {
        Self {
            xx: 1.0,
            yx: 0.0,
            xy: 0.0,
            yy: 1.0,
            dx,
            dy,
        }
    }

    const fn scale(sx: f32, sy: f32) -> Self {
        Self {
            xx: sx,
            yx: 0.0,
            xy: 0.0,
            yy: sy,
            dx: 0.0,
            dy: 0.0,
        }
    }

    /// Counter-clockwise (y up) by `half_turns` half-turns.
    fn rotate(half_turns: f32) -> Self {
        let (s, c) = (half_turns * PI).sin_cos();
        Self {
            xx: c,
            yx: s,
            xy: -s,
            yy: c,
            dx: 0.0,
            dy: 0.0,
        }
    }

    /// fontTools' `skew(-x, y)`, the angles in half-turns: the y axis leans
    /// `x` counter-clockwise, the x axis `y`.
    fn skew(x_half_turns: f32, y_half_turns: f32) -> Self {
        Self {
            xx: 1.0,
            yx: (y_half_turns * PI).tan(),
            xy: (-x_half_turns * PI).tan(),
            yy: 1.0,
            dx: 0.0,
            dy: 0.0,
        }
    }

    /// `self` after `inner`: `inner` is applied to a point first.
    fn then(self, inner: Self) -> Self {
        Self {
            xx: mad(self.xx, inner.xx, self.xy * inner.yx),
            yx: mad(self.yx, inner.xx, self.yy * inner.yx),
            xy: mad(self.xx, inner.xy, self.xy * inner.yy),
            yy: mad(self.yx, inner.xy, self.yy * inner.yy),
            dx: mad(self.xx, inner.dx, mad(self.xy, inner.dy, self.dx)),
            dy: mad(self.yx, inner.dx, mad(self.yy, inner.dy, self.dy)),
        }
    }

    /// `inner` about the point `(cx, cy)` rather than the origin.
    fn around(inner: Self, cx: f32, cy: f32) -> Self {
        Self::translate(cx, cy)
            .then(inner)
            .then(Self::translate(-cx, -cy))
    }

    fn apply(self, p: Point) -> Point {
        Point::new(
            mad(self.xx, p.x, mad(self.xy, p.y, self.dx)),
            mad(self.yx, p.x, mad(self.yy, p.y, self.dy)),
        )
    }

    /// The transform undoing this one; `None` if it squashes the plane flat.
    fn invert(self) -> Option<Self> {
        let det = mad(self.xx, self.yy, -(self.xy * self.yx));
        if !det.is_normal() {
            return None;
        }
        let inv = det.recip();
        let (xx, yx, xy, yy) = (self.yy * inv, -self.yx * inv, -self.xy * inv, self.xx * inv);
        Some(Self {
            xx,
            yx,
            xy,
            yy,
            dx: -mad(xx, self.dx, xy * self.dy),
            dy: -mad(yx, self.dx, yy * self.dy),
        })
    }
}

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl Rect {
    const EMPTY: Self = Self {
        min_x: f32::INFINITY,
        min_y: f32::INFINITY,
        max_x: f32::NEG_INFINITY,
        max_y: f32::NEG_INFINITY,
    };

    /// Also true of a box with a NaN in it.
    fn is_empty(self) -> bool {
        !(self.min_x < self.max_x && self.min_y < self.max_y)
    }

    fn union(self, other: Self) -> Self {
        if other.is_empty() {
            return self;
        }
        Self {
            min_x: self.min_x.min(other.min_x),
            min_y: self.min_y.min(other.min_y),
            max_x: self.max_x.max(other.max_x),
            max_y: self.max_y.max(other.max_y),
        }
    }

    fn add(&mut self, p: Point) {
        if p.x.is_finite() && p.y.is_finite() {
            self.min_x = self.min_x.min(p.x);
            self.min_y = self.min_y.min(p.y);
            self.max_x = self.max_x.max(p.x);
            self.max_y = self.max_y.max(p.y);
        }
    }

    /// The box around this one's four corners under `m`.
    fn map(self, m: Affine) -> Self {
        if self.is_empty() {
            return Self::EMPTY;
        }
        let mut b = Self::EMPTY;
        for (x, y) in [
            (self.min_x, self.min_y),
            (self.max_x, self.min_y),
            (self.min_x, self.max_y),
            (self.max_x, self.max_y),
        ] {
            b.add(m.apply(Point::new(x, y)));
        }
        b
    }
}

/// The box around every point of an outline, control points included.
fn outline_rect(outline: &Outline) -> Rect {
    let mut b = Rect::EMPTY;
    for cmd in &outline.commands {
        match *cmd {
            PathCmd::MoveTo(p) | PathCmd::LineTo(p) => b.add(p),
            PathCmd::QuadTo(c, p) => {
                b.add(c);
                b.add(p);
            }
            PathCmd::CurveTo(c1, c2, p) => {
                b.add(c1);
                b.add(c2);
                b.add(p);
            }
            PathCmd::Close => {}
        }
    }
    b
}

/// A rectangle of canvas pixels: columns `x0..x1`, rows `y0..y1`, not empty.
#[derive(Clone, Copy, Debug)]
struct Region {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

impl Region {
    fn width(self) -> usize {
        self.x1.saturating_sub(self.x0)
    }

    fn height(self) -> usize {
        self.y1.saturating_sub(self.y0)
    }

    fn area(self) -> usize {
        self.width().saturating_mul(self.height())
    }
}

/// How far past a pixel edge a bound may stray before it takes in the next
/// pixel: a 256th of a pixel, less than one step of 8-bit coverage. What lies
/// that close to an edge is floating-point error in a transform -- a square
/// turned a quarter-turn ends a millionth of a pixel past where it began --
/// not ink, and without the allowance it costs a column of empty pixels.
const SNAP: f32 = 1.0 / 256.0;

/// The pixel edge at or below `v`, allowing [`SNAP`].
fn snap_down(v: f32) -> f32 {
    (v + SNAP).floor()
}

/// The pixel edge at or above `v`, allowing [`SNAP`].
fn snap_up(v: f32) -> f32 {
    (v - SNAP).ceil()
}

/// The centre of pixel `(x, y)`.
#[allow(
    clippy::cast_precision_loss,
    reason = "canvas coordinates are below 2^20"
)]
fn centre(x: usize, y: usize) -> Point {
    Point::new(x as f32 + 0.5, y as f32 + 0.5)
}

// ---------------------------------------------------------------------------
// Colour
// ---------------------------------------------------------------------------

/// Premultiplied RGBA, each from 0 to 1.
pub(crate) type Rgba = [f32; 4];

const CLEAR: Rgba = [0.0; 4];

/// Straight `0xAARRGGBB` as straight RGBA from 0 to 1.
pub(crate) fn unpack(argb: u32) -> Rgba {
    let [a, r, g, b] = argb.to_be_bytes();
    [r, g, b, a].map(|c| f32::from(c) / 255.0)
}

/// Straight RGBA to premultiplied.
pub(crate) fn premultiply([r, g, b, a]: Rgba) -> Rgba {
    [r * a, g * a, b * a, a]
}

fn scale_rgba(c: Rgba, k: f32) -> Rgba {
    c.map(|v| v * k)
}

/// Premultiplied `0xAARRGGBB`, each channel rounded and kept no brighter
/// than its alpha.
pub(crate) fn pack(c: Rgba) -> u32 {
    let a = c[3].clamp(0.0, 1.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255.5 first"
    )]
    let byte = |v: f32| (v.clamp(0.0, a) * 255.0 + 0.5) as u8;
    u32::from_be_bytes([byte(a), byte(c[0]), byte(c[1]), byte(c[2])])
}

/// `src` over `dst`, premultiplied.
fn over(src: Rgba, dst: Rgba) -> Rgba {
    let k = 1.0 - src[3];
    [
        mad(k, dst[0], src[0]),
        mad(k, dst[1], src[1]),
        mad(k, dst[2], src[2]),
        mad(k, dst[3], src[3]),
    ]
}

/// A premultiplied RGBA canvas the size of the glyph's.
struct Canvas {
    width: usize,
    px: Vec<Rgba>,
}

impl Canvas {
    /// Columns `x0..x1` of row `y`.
    fn span_mut(&mut self, y: usize, x0: usize, x1: usize) -> Option<&mut [Rgba]> {
        let row = y.checked_mul(self.width)?;
        self.px.get_mut(row.checked_add(x0)?..row.checked_add(x1)?)
    }

    /// Columns `x0..x1` of row `y`.
    fn span(&self, y: usize, x0: usize, x1: usize) -> Option<&[Rgba]> {
        let row = y.checked_mul(self.width)?;
        self.px.get(row.checked_add(x0)?..row.checked_add(x1)?)
    }
}

/// What a leaf of the graph paints.
enum Fill {
    Solid(Rgba),
    /// A linear gradient, its parameter a plane over the canvas: `t = a x +
    /// b y + c` at canvas point `(x, y)` -- the paint's transform and the
    /// gradient's projection folded into three numbers, so a pixel costs two
    /// multiply-adds rather than a transform and a division.
    Linear {
        line: ColourLine,
        a: f32,
        b: f32,
        c: f32,
    },
    /// A radial or sweep gradient, `inv` taking canvas pixels back to its own
    /// space.
    Gradient {
        line: ColourLine,
        gradient: Gradient,
        inv: Affine,
    },
}

impl Fill {
    /// Whether it paints nothing at all.
    fn is_clear(&self) -> bool {
        matches!(self, Self::Solid(c) if c[3] <= 0.0)
    }

    /// Blend this fill over `dst`, the canvas row `y` from column `x0`, each
    /// pixel at its coverage in `cover`.
    ///
    /// A row at a time because a fill's position in its own space moves by a
    /// constant from one pixel to the next: a linear gradient's parameter by
    /// `a`, a radial or sweep gradient's point by the inverse transform's
    /// first column. Stepping costs two additions where transforming each
    /// pixel cost four multiply-adds.
    fn blend_row(&self, x0: usize, y: usize, cover: &[f32], dst: &mut [Rgba]) {
        match self {
            Self::Solid(colour) => {
                for (d, &k) in dst.iter_mut().zip(cover) {
                    if k > 0.0 {
                        *d = over(scale_rgba(*colour, k), *d);
                    }
                }
            }
            Self::Linear { line, a, b, c } => {
                let p = centre(x0, y);
                let mut t = mad(*a, p.x, mad(*b, p.y, *c));
                for (d, &k) in dst.iter_mut().zip(cover) {
                    if k > 0.0 {
                        *d = over(scale_rgba(line.at(t), k), *d);
                    }
                    t += *a;
                }
            }
            Self::Gradient {
                line,
                gradient,
                inv,
            } => {
                let mut p = inv.apply(centre(x0, y));
                for (d, &k) in dst.iter_mut().zip(cover) {
                    if k > 0.0 {
                        let colour = gradient.t(p).map_or(CLEAR, |t| line.at(t));
                        *d = over(scale_rgba(colour, k), *d);
                    }
                    p.x += inv.xx;
                    p.y += inv.yx;
                }
            }
        }
    }

    /// The colour at canvas point `p`.
    fn at(&self, p: Point) -> Rgba {
        match self {
            Self::Solid(c) => *c,
            Self::Linear { line, a, b, c } => line.at(mad(*a, p.x, mad(*b, p.y, *c))),
            Self::Gradient {
                line,
                gradient,
                inv,
            } => gradient.t(inv.apply(p)).map_or(CLEAR, |t| line.at(t)),
        }
    }
}

/// How finely a colour line is tabulated: this many steps across its stops'
/// span -- the size of the gradient caches Skia long used. A black-to-white
/// line changes by one 8-bit level a step, so taking the nearest step instead
/// of interpolating shows nothing 8-bit colour would not; a hard edge, two
/// stops at one offset, lands within a 256th of the span.
///
/// Small because the table is built for every gradient fill, and a colour
/// emoji is hundreds of small fills: at 1024 steps the tables cost as much
/// as the pixels they sped up.
const LINE_STEPS: usize = 256;

/// A colour line: how a gradient's parameter becomes a colour.
///
/// Tabulated when it is built: [`at`](Self::at) is called once a pixel, and
/// a binary search over the stops and an interpolation there were a large
/// part of a gradient's cost. The stops are kept only to build the table
/// ([`Stops`]).
struct ColourLine {
    extend: u8,
    /// The first stop's offset.
    first: f32,
    /// Steps per unit of the parameter: `LINE_STEPS / (last - first)`, or 0
    /// when every stop sits at one offset.
    scale: f32,
    /// `LINE_STEPS + 1` colours, evenly spaced from the first stop's offset to
    /// the last's; or, for a line whose stops share one offset, the colours
    /// before and after it.
    table: Vec<Rgba>,
}

impl ColourLine {
    /// The line through `stops`, sorted, premultiplied, with `extend` for the
    /// parameters outside them.
    fn new(extend: u8, stops: &Stops) -> Self {
        let (Some(&(first, first_c)), Some(&(last, last_c))) = (stops.0.first(), stops.0.last())
        else {
            return Self {
                extend,
                first: 0.0,
                scale: 0.0,
                table: Vec::new(),
            };
        };
        let span = last - first;
        let tabulable = span > f32::EPSILON && span.is_finite();
        if !tabulable {
            return Self {
                extend,
                first,
                scale: 0.0,
                table: alloc::vec![first_c, last_c],
            };
        }
        #[allow(clippy::cast_precision_loss, reason = "a small power of two, exact")]
        let steps = LINE_STEPS as f32;
        Self {
            extend,
            first,
            scale: steps / span,
            table: stops.sample(first, span / steps),
        }
    }

    /// The colour at gradient parameter `t`.
    fn at(&self, t: f32) -> Rgba {
        if !t.is_finite() {
            return CLEAR;
        }
        if self.scale <= 0.0 {
            // Every stop at one offset: before it, the first colour; from it
            // on, the last.
            let i = usize::from(t >= self.first);
            return self.table.get(i).copied().unwrap_or(CLEAR);
        }
        // How many steps along the stops' span, and wrapped for REPEAT and
        // REFLECT -- with truncation rather than `rem_euclid`, which on this
        // target is the C library's `fmodf`.
        #[allow(clippy::cast_precision_loss, reason = "a small power of two, exact")]
        let steps = LINE_STEPS as f32;
        let u = (t - self.first) * self.scale;
        let u = match self.extend {
            1 => wrap(u, steps),
            2 => {
                let w = wrap(u, 2.0 * steps);
                if w > steps { 2.0 * steps - w } else { w }
            }
            _ => u,
        };
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to the table first"
        )]
        let i = (u.clamp(0.0, steps) + 0.5) as usize;
        self.table.get(i.min(LINE_STEPS)).copied().unwrap_or(CLEAR)
    }
}

/// `u` wrapped into `0..period`, without `f32::rem_euclid` (a C-library call
/// on this target): truncation finds the whole periods.
fn wrap(u: f32, period: f32) -> f32 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "a saturated quotient only means a wrong phase for a parameter \
                  billions of periods out"
    )]
    let whole = (u / period) as i64 as f32;
    let r = u - whole * period;
    if r < 0.0 { r + period } else { r }
}

/// A colour line's stops, sorted by offset, premultiplied: what a
/// [`ColourLine`] tabulates.
struct Stops(Vec<(f32, Rgba)>);

impl Stops {
    /// The colours at `LINE_STEPS + 1` parameters, `first` and each `step`
    /// after it: the first stop's colour before it, the last's after, and
    /// between two stops the blend of the two -- in one walk over the stops
    /// rather than a search per step.
    fn sample(&self, first: f32, step: f32) -> Vec<Rgba> {
        let mut table = Vec::with_capacity(LINE_STEPS.saturating_add(1));
        let (Some(&(lo, lo_c)), Some(&(hi, hi_c))) = (self.0.first(), self.0.last()) else {
            return table;
        };
        // `seg` is the last stop at or before `t`, as `at`'s search finds it.
        let mut seg = 0usize;
        for k in 0..=LINE_STEPS {
            #[allow(clippy::cast_precision_loss, reason = "at most LINE_STEPS, exact")]
            let t = mad(step, k as f32, first);
            if t <= lo {
                table.push(lo_c);
                continue;
            }
            if t >= hi {
                table.push(hi_c);
                continue;
            }
            while self
                .0
                .get(seg.saturating_add(1))
                .is_some_and(|&(o, _)| o <= t)
            {
                seg = seg.saturating_add(1);
            }
            let (Some(&(o0, c0)), Some(&(o1, c1))) =
                (self.0.get(seg), self.0.get(seg.saturating_add(1)))
            else {
                table.push(hi_c);
                continue;
            };
            let f = if o1 > o0 { (t - o0) / (o1 - o0) } else { 1.0 };
            let mut out = CLEAR;
            for ((o, a), b) in out.iter_mut().zip(c0).zip(c1) {
                *o = mad(b - a, f, a);
            }
            table.push(out);
        }
        table
    }
}

/// A gradient's geometry, in the space of its paint.
#[derive(Clone, Copy)]
enum Gradient {
    /// Bands parallel to `p0`-`p2`, the parameter running from `p0` (0) to
    /// `p3` (1): `p1` moved onto the perpendicular of `p0`-`p2` through `p0`.
    Linear { p0: Point, p3: Point },
    /// The two-point conical gradient: circle `(c0, r0)` grows into
    /// `(c1, r1)`, and on past them as the extend mode says. Built by
    /// [`Gradient::radial`], which works out once what every pixel's equation
    /// shares: `c1 - c0`, `r1 - r0`, the leading coefficient `a` and its
    /// reciprocal (0 when `a` is).
    Radial {
        c0: Point,
        r0: f32,
        cdx: f32,
        cdy: f32,
        dr: f32,
        a: f32,
        inv_a: f32,
    },
    /// Around `c`, counter-clockwise, from angle `start` to angle `end`
    /// (radians).
    Sweep { c: Point, start: f32, end: f32 },
}

impl Gradient {
    /// The two-point conical gradient from circle `(c0, r0)` to `(c1, r1)`.
    fn radial(c0: Point, r0: f32, c1: Point, r1: f32) -> Self {
        let (cdx, cdy, dr) = (c1.x - c0.x, c1.y - c0.y, r1 - r0);
        let a = mad(cdx, cdx, mad(cdy, cdy, -(dr * dr)));
        let inv_a = if a.abs() > f32::EPSILON {
            a.recip()
        } else {
            0.0
        };
        Self::Radial {
            c0,
            r0,
            cdx,
            cdy,
            dr,
            a,
            inv_a,
        }
    }

    fn linear(p0: Point, p1: Point, p2: Point) -> Self {
        let (nx, ny) = (p0.y - p2.y, p2.x - p0.x);
        let len2 = mad(nx, nx, ny * ny);
        let p3 = if len2 > f32::EPSILON {
            let k = mad(p1.x - p0.x, nx, (p1.y - p0.y) * ny) / len2;
            Point::new(mad(nx, k, p0.x), mad(ny, k, p0.y))
        } else {
            // No direction for the bands: run straight from p0 to p1.
            p1
        };
        Self::Linear { p0, p3 }
    }

    /// The gradient parameter at `p`; `None` where the gradient paints
    /// nothing -- outside every circle of a radial one, anywhere on a linear
    /// one of no length.
    fn t(&self, p: Point) -> Option<f32> {
        match *self {
            Self::Linear { p0, p3 } => {
                let (vx, vy) = (p3.x - p0.x, p3.y - p0.y);
                let len2 = mad(vx, vx, vy * vy);
                (len2 > f32::EPSILON).then(|| mad(p.x - p0.x, vx, (p.y - p0.y) * vy) / len2)
            }
            Self::Radial {
                c0,
                r0,
                cdx,
                cdy,
                dr,
                a,
                inv_a,
            } => {
                // The largest t whose circle, c0 + t (c1 - c0) with radius
                // r0 + t (r1 - r0) >= 0, passes through p.
                let (px, py) = (p.x - c0.x, p.y - c0.y);
                let b = mad(px, cdx, mad(py, cdy, r0 * dr));
                let c = mad(px, px, mad(py, py, -(r0 * r0)));
                let reaches = |t: f32| mad(dr, t, r0) >= 0.0;
                if a.abs() <= f32::EPSILON {
                    if b.abs() <= f32::EPSILON {
                        return None;
                    }
                    let t = c / (2.0 * b);
                    return reaches(t).then_some(t);
                }
                let disc = mad(b, b, -(a * c));
                if disc < 0.0 {
                    return None;
                }
                let root = disc.sqrt();
                let (t1, t2) = ((b + root) * inv_a, (b - root) * inv_a);
                let (hi, lo) = if t1 >= t2 { (t1, t2) } else { (t2, t1) };
                if reaches(hi) {
                    Some(hi)
                } else {
                    reaches(lo).then_some(lo)
                }
            }
            Self::Sweep { c, start, end } => {
                // The angle from 0 to a full turn, as Skia measures it.
                let angle = (p.y - c.y).atan2(p.x - c.x).rem_euclid(TAU);
                let span = end - start;
                if span.abs() <= f32::EPSILON {
                    return Some(if angle < start { 0.0 } else { 1.0 });
                }
                Some((angle - start) / span)
            }
        }
    }
}

/// `s` (the source) combined with `b` (the backdrop) by compositing mode
/// `mode`, both premultiplied: the Porter-Duff operators and the blend modes
/// of W3C Compositing and Blending Level 1, numbered as `COLR` numbers them.
fn composite(mode: u8, s: Rgba, b: Rgba) -> Rgba {
    let (sa, ba) = (s[3], b[3]);
    // out = s * fs + b * fb.
    let porter_duff = |fs: f32, fb: f32| {
        let mut out = CLEAR;
        for ((o, x), y) in out.iter_mut().zip(s).zip(b) {
            *o = mad(x, fs, y * fb);
        }
        out
    };
    match mode {
        0 => CLEAR,
        1 => s,
        2 => b,
        4 => porter_duff(1.0 - ba, 1.0),
        5 => porter_duff(ba, 0.0),
        6 => porter_duff(0.0, sa),
        7 => porter_duff(1.0 - ba, 0.0),
        8 => porter_duff(0.0, 1.0 - sa),
        9 => porter_duff(ba, 1.0 - sa),
        10 => porter_duff(1.0 - ba, sa),
        11 => porter_duff(1.0 - ba, 1.0 - sa),
        12 => porter_duff(1.0, 1.0).map(|v| v.min(1.0)),
        13..=27 => blend(mode, s, b),
        // SRC_OVER, and what the specification does not define.
        _ => porter_duff(1.0, 1.0 - sa),
    }
}

/// A blend mode: the blended colour where both are present, each alone where
/// only it is.
fn blend(mode: u8, s: Rgba, b: Rgba) -> Rgba {
    let (sa, ba) = (s[3], b[3]);
    let straight = |c: Rgba, a: f32| {
        if a > 0.0 {
            [c[0] / a, c[1] / a, c[2] / a]
        } else {
            [0.0; 3]
        }
    };
    let (cs, cb) = (straight(s, sa), straight(b, ba));
    let mixed = if mode >= 24 {
        non_separable(mode, cs, cb)
    } else {
        let mut m = [0.0; 3];
        for ((m, &x), &y) in m.iter_mut().zip(&cs).zip(&cb) {
            *m = separable(mode, x, y);
        }
        m
    };
    let both = sa * ba;
    let mut out = [0.0, 0.0, 0.0, sa + ba - both];
    for (((o, &x), &y), &m) in out.iter_mut().zip(&s).zip(&b).zip(&mixed) {
        *o = mad(x, 1.0 - ba, mad(y, 1.0 - sa, both * m));
    }
    out
}

/// One channel of a separable blend mode, straight colour: `s` the source,
/// `b` the backdrop.
fn separable(mode: u8, s: f32, b: f32) -> f32 {
    let screen = |x: f32, y: f32| x + y - x * y;
    // HardLight(backdrop, source).
    let hard_light = |b: f32, s: f32| {
        if s <= 0.5 {
            b * 2.0 * s
        } else {
            screen(b, 2.0 * s - 1.0)
        }
    };
    match mode {
        13 => screen(b, s),
        // Overlay is hard light with the two swapped.
        14 => hard_light(s, b),
        15 => s.min(b),
        16 => s.max(b),
        17 => {
            if b <= 0.0 {
                0.0
            } else if s >= 1.0 {
                1.0
            } else {
                (b / (1.0 - s)).min(1.0)
            }
        }
        18 => {
            if b >= 1.0 {
                1.0
            } else if s <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - b) / s).min(1.0)
            }
        }
        19 => hard_light(b, s),
        20 => {
            if s <= 0.5 {
                mad(mad(2.0f32, -s, 1.0) * b, -(1.0 - b), b)
            } else {
                let d = if b <= 0.25 {
                    (mad(16.0f32, b, -12.0) * b + 4.0) * b
                } else {
                    b.sqrt()
                };
                mad(mad(2.0f32, s, -1.0), d - b, b)
            }
        }
        21 => (b - s).abs(),
        22 => mad(-2.0 * b, s, b + s),
        _ => b * s,
    }
}

/// The hue, saturation, colour and luminosity modes, on straight colour.
fn non_separable(mode: u8, s: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    let lum = |c: [f32; 3]| mad(0.3f32, c[0], mad(0.59f32, c[1], 0.11 * c[2]));
    let max = |c: [f32; 3]| c[0].max(c[1]).max(c[2]);
    let min = |c: [f32; 3]| c[0].min(c[1]).min(c[2]);
    let clip = |c: [f32; 3]| {
        let (l, n, x) = (lum(c), min(c), max(c));
        let mut c = c;
        if n < 0.0 && l - n > f32::EPSILON {
            c = c.map(|v| l + (v - l) * l / (l - n));
        }
        if x > 1.0 && x - l > f32::EPSILON {
            c = c.map(|v| l + (v - l) * (1.0 - l) / (x - l));
        }
        c
    };
    let set_lum = |c: [f32; 3], l: f32| {
        let d = l - lum(c);
        clip(c.map(|v| v + d))
    };
    let sat = |c: [f32; 3]| max(c) - min(c);
    let set_sat = |c: [f32; 3], s: f32| {
        let (hi, lo) = (max(c), min(c));
        if hi - lo <= f32::EPSILON {
            return [0.0; 3];
        }
        c.map(|v| (v - lo) * s / (hi - lo))
    };
    match mode {
        24 => set_lum(set_sat(s, sat(b)), lum(b)),
        25 => set_lum(set_sat(b, sat(s)), lum(b)),
        26 => set_lum(s, lum(b)),
        _ => set_lum(b, lum(s)),
    }
}

// ---------------------------------------------------------------------------
// Painting
// ---------------------------------------------------------------------------

struct Renderer<'a> {
    t: &'a Tables<'a>,
    face: &'a Face,
    coords: &'a Coords,
    /// Palette 0.
    palette: Palette<'a>,
    /// The text colour, premultiplied.
    foreground: Rgba,
    used_foreground: bool,
    /// Paint nodes visited so far, against [`MAX_PAINTS`].
    visited: usize,
    /// Pixels still to be touched before painting stops.
    work: usize,
    width: usize,
    height: usize,
    /// Canvas pixels in existence, against [`CANVAS_BUDGET`].
    live: usize,
}

impl Renderer<'_> {
    fn outline(&self, gid: u16) -> Option<Outline> {
        self.face.outline_at(gid, self.coords).ok()
    }

    /// Enter a node `depth` deep: `false` once the graph is too deep or has
    /// been visited enough.
    fn enter(&mut self, depth: u32) -> bool {
        if depth > MAX_DEPTH || self.visited >= MAX_PAINTS {
            return false;
        }
        self.visited = self.visited.saturating_add(1);
        true
    }

    /// Spend `pixels` of the work budget; `false`, and nothing spent, if it
    /// has not got them.
    fn charge(&mut self, pixels: usize) -> bool {
        match self.work.checked_sub(pixels) {
            Some(left) => {
                self.work = left;
                true
            }
            None => false,
        }
    }

    /// A clear canvas, if the budget has room for one.
    fn canvas(&mut self) -> Option<Canvas> {
        let n = self.width.checked_mul(self.height)?;
        let live = self.live.checked_add(n).filter(|&l| l <= CANVAS_BUDGET)?;
        if !self.charge(n) {
            return None;
        }
        self.live = live;
        Some(Canvas {
            width: self.width,
            px: vec![CLEAR; n],
        })
    }

    /// Give a canvas back to the budget.
    fn release(&mut self, canvas: Canvas) {
        self.live = self.live.saturating_sub(canvas.px.len());
    }

    /// Palette entry `index` at `alpha`, premultiplied; `0xFFFF` is the
    /// text colour, and an entry the palette does not have is transparent.
    fn palette_colour(&mut self, index: u16, alpha: f32) -> Rgba {
        let alpha = alpha.clamp(0.0, 1.0);
        if index == 0xFFFF {
            self.used_foreground = true;
            return scale_rgba(self.foreground, alpha);
        }
        let [r, g, b, a] = self.palette.get(usize::from(index)).unwrap_or([0.0; 4]);
        premultiply([r, g, b, a * alpha])
    }

    /// The colour line at `at`, its stops sorted.
    fn colour_line(&mut self, at: usize, variable: bool) -> Option<ColourLine> {
        let d = self.t.colr;
        let extend = u8_at(d, at)?;
        let count = usize::from(u16_at(d, at.checked_add(1)?)?);
        // A VarColorStop carries a four-byte variation index as well.
        let size = if variable { 10 } else { 6 };
        let mut stops = Vec::with_capacity(count);
        for i in 0..count {
            let s = at.checked_add(3)?.checked_add(i.checked_mul(size)?)?;
            // A VarColorStop's offset and alpha move, in that order.
            let base = self
                .t
                .var_base(variable.then(|| s.checked_add(6)).flatten());
            let offset = f2dot14(d, s)? + self.t.delta(base, 0) / 16384.0;
            let index = u16_at(d, s.checked_add(2)?)?;
            let alpha = f2dot14(d, s.checked_add(4)?)? + self.t.delta(base, 1) / 16384.0;
            stops.push((offset, self.palette_colour(index, alpha)));
        }
        // Stable, so that two stops at one offset keep their order: a hard
        // edge from the first colour to the second.
        stops.sort_by(|a, b| a.0.total_cmp(&b.0));
        Some(ColourLine::new(extend, &Stops(stops)))
    }

    /// A solid or gradient node as a fill landing on the canvas under `m`.
    fn fill_of(&mut self, node: Node, m: Affine) -> Fill {
        match node {
            Node::Solid { index, alpha } => Fill::Solid(self.palette_colour(index, alpha)),
            Node::Gradient {
                line,
                variable,
                gradient,
            } => match (m.invert(), self.colour_line(line, variable)) {
                (Some(inv), Some(line)) => match gradient {
                    Gradient::Linear { p0, p3 } => {
                        // t = ((p - p0) . v) / |v|^2 at p = inv(x, y), which is
                        // affine in (x, y): its three coefficients.
                        let (vx, vy) = (p3.x - p0.x, p3.y - p0.y);
                        let len2 = mad(vx, vx, vy * vy);
                        // False for a NaN as well as for no length.
                        let has_length = len2 > f32::EPSILON;
                        if !has_length {
                            return Fill::Solid(CLEAR);
                        }
                        let (vx, vy) = (vx / len2, vy / len2);
                        Fill::Linear {
                            line,
                            a: mad(vx, inv.xx, vy * inv.yx),
                            b: mad(vx, inv.xy, vy * inv.yy),
                            c: mad(vx, inv.dx - p0.x, vy * (inv.dy - p0.y)),
                        }
                    }
                    _ => Fill::Gradient {
                        line,
                        gradient,
                        inv,
                    },
                },
                _ => Fill::Solid(CLEAR),
            },
            _ => Fill::Solid(CLEAR),
        }
    }

    /// If the paint at `at` is a solid or gradient fill under any number of
    /// transforms, that fill under `m`. `None` for anything else, which
    /// needs a canvas of its own.
    fn as_fill(&mut self, at: usize, m: Affine, depth: u32) -> Option<Fill> {
        if !self.enter(depth) {
            return Some(Fill::Solid(CLEAR));
        }
        match self.t.node(at)? {
            node @ (Node::Solid { .. } | Node::Gradient { .. }) => Some(self.fill_of(node, m)),
            Node::Transform { paint, transform } => {
                self.as_fill(paint, m.then(transform), depth.saturating_add(1))
            }
            _ => None,
        }
    }

    /// The pixels of the canvas `r` touches; `None` if it misses it.
    fn region(&self, r: Rect) -> Option<Region> {
        if r.is_empty() {
            return None;
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "clamped to the canvas, whose sides are below 2^20"
        )]
        let clamp = |v: f32, hi: usize| v.clamp(0.0, hi as f32) as usize;
        let region = Region {
            x0: clamp(snap_down(r.min_x), self.width),
            y0: clamp(snap_down(r.min_y), self.height),
            x1: clamp(snap_up(r.max_x), self.width),
            y1: clamp(snap_up(r.max_y), self.height),
        };
        (region.x0 < region.x1 && region.y0 < region.y1).then_some(region)
    }

    /// How much of each pixel glyph `gid`'s outline covers under `m`, over
    /// the region its box touches.
    fn coverage(&mut self, gid: u16, m: Affine) -> Option<(Region, Vec<f32>)> {
        let outline = self.outline(gid)?;
        let region = self.region(outline_rect(&outline).map(m))?;
        if !self.charge(region.area()) {
            return None;
        }
        // The region's corner at the origin: its first pixel's centre is
        // `(x0 + 0.5, y0 + 0.5)`.
        let corner = centre(region.x0, region.y0);
        let shift = Affine::translate(0.5 - corner.x, 0.5 - corner.y).then(m);
        let cover = coverage_on(
            &outline,
            &|p| shift.apply(p),
            region.width(),
            region.height(),
        )
        .ok()?;
        Some((region, cover))
    }

    /// Fill glyph `gid`, under `m`, with `fill`.
    fn glyph(&mut self, gid: u16, m: Affine, fill: &Fill, canvas: &mut Canvas) {
        if fill.is_clear() {
            return;
        }
        let Some((region, cover)) = self.coverage(gid, m) else {
            return;
        };
        let rows = cover.chunks_exact(region.width());
        for (y, cover) in (region.y0..region.y1).zip(rows) {
            if let Some(span) = canvas.span_mut(y, region.x0, region.x1) {
                fill.blend_row(region.x0, y, cover, span);
            }
        }
    }

    /// Paint the paint at `at` onto `canvas`, under `m` (its space to canvas
    /// pixels).
    fn paint(&mut self, at: usize, m: Affine, canvas: &mut Canvas, depth: u32) {
        if !self.enter(depth) {
            return;
        }
        let Some(node) = self.t.node(at) else { return };
        let deeper = depth.saturating_add(1);
        match node {
            Node::Layers { first, count } => {
                for i in 0..count {
                    if let Some(layer) = first.checked_add(i).and_then(|i| self.t.layer_v1(i)) {
                        self.paint(layer, m, canvas, deeper);
                    }
                }
            }
            Node::Solid { .. } | Node::Gradient { .. } => {
                let fill = self.fill_of(node, m);
                if fill.is_clear() || !self.charge(canvas.px.len()) {
                    return;
                }
                for (y, row) in canvas.px.chunks_exact_mut(self.width).enumerate() {
                    for (x, dst) in row.iter_mut().enumerate() {
                        *dst = over(fill.at(centre(x, y)), *dst);
                    }
                }
            }
            Node::Glyph { paint, gid } => {
                // The usual case, a fill: painted straight through the
                // outline.
                if let Some(fill) = self.as_fill(paint, m, deeper) {
                    self.glyph(gid, m, &fill, canvas);
                    return;
                }
                // Anything else is painted whole, then cut to the outline.
                let Some((region, cover)) = self.coverage(gid, m) else {
                    return;
                };
                let Some(mut layer) = self.canvas() else {
                    return;
                };
                self.paint(paint, m, &mut layer, deeper);
                let rows = cover.chunks_exact(region.width());
                for (y, cover) in (region.y0..region.y1).zip(rows) {
                    let (Some(dst), Some(src)) = (
                        canvas.span_mut(y, region.x0, region.x1),
                        layer.span(y, region.x0, region.x1),
                    ) else {
                        continue;
                    };
                    for ((dst, &src), &k) in dst.iter_mut().zip(src).zip(cover) {
                        if k > 0.0 {
                            *dst = over(scale_rgba(src, k), *dst);
                        }
                    }
                }
                self.release(layer);
            }
            Node::ColrGlyph { gid } => {
                if let Some(paint) = self.t.base_v1(gid) {
                    self.paint(paint, m, canvas, deeper);
                }
            }
            Node::Transform { paint, transform } => {
                self.paint(paint, m.then(transform), canvas, deeper);
            }
            Node::Composite {
                source,
                mode,
                backdrop,
            } => {
                let Some(mut src) = self.canvas() else { return };
                let Some(mut dst) = self.canvas() else {
                    self.release(src);
                    return;
                };
                self.paint(source, m, &mut src, deeper);
                self.paint(backdrop, m, &mut dst, deeper);
                if self.charge(canvas.px.len()) {
                    for ((out, &s), &b) in canvas.px.iter_mut().zip(&src.px).zip(&dst.px) {
                        *out = over(composite(mode, s, b), *out);
                    }
                }
                self.release(src);
                self.release(dst);
            }
        }
    }

    /// Grow `b` by what the paint at `at` can cover, under `m`: the outlines
    /// of its `PaintGlyph`s.
    fn bounds(&mut self, at: usize, m: Affine, b: &mut Rect, depth: u32) {
        if !self.enter(depth) {
            return;
        }
        let Some(node) = self.t.node(at) else { return };
        let deeper = depth.saturating_add(1);
        match node {
            Node::Layers { first, count } => {
                for i in 0..count {
                    if let Some(layer) = first.checked_add(i).and_then(|i| self.t.layer_v1(i)) {
                        self.bounds(layer, m, b, deeper);
                    }
                }
            }
            Node::Glyph { gid, .. } => {
                if let Some(outline) = self.outline(gid) {
                    *b = b.union(outline_rect(&outline).map(m));
                }
            }
            Node::ColrGlyph { gid } => {
                if let Some(paint) = self.t.base_v1(gid) {
                    self.bounds(paint, m, b, deeper);
                }
            }
            Node::Transform { paint, transform } => {
                self.bounds(paint, m.then(transform), b, deeper);
            }
            Node::Composite {
                source, backdrop, ..
            } => {
                self.bounds(source, m, b, deeper);
                self.bounds(backdrop, m, b, deeper);
            }
            // A fill with no outline above it is bounded only by the canvas.
            Node::Solid { .. } | Node::Gradient { .. } => {}
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::unnecessary_box_returns,
    reason = "test fixture builder; a paint's children are boxed, so its helpers return boxes"
)]
pub(crate) mod tests {
    use super::*;
    use crate::sfnt::tests::build_test_font_with;
    use alloc::boxed::Box;

    // The fixture's glyphs, in font units (1000 to the em):
    //   1: a square, x 100..200, y 0..100
    //   2: a triangle, (0,0) to (100,0) under a quadratic apex at (50,200)
    //   3: glyph 1 moved to x 600..700, y 200..300

    /// A paint graph, to be laid out as `COLR` bytes.
    enum P {
        Layers(u8, u32),
        Solid(u16, f32),
        /// Extend, stops `(offset, palette index, alpha)`, then p0, p1, p2.
        Linear(u8, Vec<(f32, u16, f32)>, [i16; 6]),
        /// Extend, stops, then c0, r0, c1, r1.
        Radial(u8, Vec<(f32, u16, f32)>, [i16; 2], u16, [i16; 2], u16),
        /// Extend, stops, centre, then the start and end angles *as stored*
        /// (half-turns less one).
        Sweep(u8, Vec<(f32, u16, f32)>, [i16; 2], f32, f32),
        Glyph(u16, Box<P>),
        ColrGlyph(u16),
        /// xx, yx, xy, yy, dx, dy.
        Transform([f32; 6], Box<P>),
        Translate(i16, i16, Box<P>),
        Scale(f32, f32, Box<P>),
        Rotate(f32, Box<P>),
        RotateAround(f32, i16, i16, Box<P>),
        Skew(f32, f32, Box<P>),
        Composite(u8, Box<P>, Box<P>),
        /// `PaintVarSolid`: palette index, alpha, `varIndexBase`.
        VarSolid(u16, f32, u32),
        /// `PaintVarTranslate`: dx, dy, `varIndexBase`.
        VarTranslate(i16, i16, u32, Box<P>),
    }

    fn f2(v: f32) -> [u8; 2] {
        ((v * 16384.0).round() as i16).to_be_bytes()
    }

    fn patch24(out: &mut [u8], at: usize, value: usize) {
        out[at..at + 3].copy_from_slice(&(value as u32).to_be_bytes()[1..]);
    }

    fn colour_line(out: &mut Vec<u8>, extend: u8, stops: &[(f32, u16, f32)]) {
        out.push(extend);
        out.extend_from_slice(&(stops.len() as u16).to_be_bytes());
        for &(offset, index, alpha) in stops {
            out.extend_from_slice(&f2(offset));
            out.extend_from_slice(&index.to_be_bytes());
            out.extend_from_slice(&f2(alpha));
        }
    }

    fn words(out: &mut Vec<u8>, vs: &[i16]) {
        for v in vs {
            out.extend_from_slice(&v.to_be_bytes());
        }
    }

    /// Lay `p` out at the end of `out`, its children after it; where it went.
    fn emit(p: &P, out: &mut Vec<u8>) -> usize {
        let at = out.len();
        let gradient = |out: &mut Vec<u8>, extend: u8, stops: &[(f32, u16, f32)]| {
            let line = out.len();
            colour_line(out, extend, stops);
            patch24(out, at + 1, line - at);
        };
        let child = |out: &mut Vec<u8>, k: usize, p: &P| {
            let c = emit(p, out);
            patch24(out, at + k, c - at);
        };
        match p {
            P::Layers(n, first) => {
                out.extend_from_slice(&[1, *n]);
                out.extend_from_slice(&first.to_be_bytes());
            }
            P::Solid(i, a) => {
                out.push(2);
                out.extend_from_slice(&i.to_be_bytes());
                out.extend_from_slice(&f2(*a));
            }
            P::Linear(extend, stops, pts) => {
                out.extend_from_slice(&[4, 0, 0, 0]);
                words(out, pts);
                gradient(out, *extend, stops);
            }
            P::Radial(extend, stops, c0, r0, c1, r1) => {
                out.extend_from_slice(&[6, 0, 0, 0]);
                words(out, c0);
                out.extend_from_slice(&r0.to_be_bytes());
                words(out, c1);
                out.extend_from_slice(&r1.to_be_bytes());
                gradient(out, *extend, stops);
            }
            P::Sweep(extend, stops, c, start, end) => {
                out.extend_from_slice(&[8, 0, 0, 0]);
                words(out, c);
                out.extend_from_slice(&f2(*start));
                out.extend_from_slice(&f2(*end));
                gradient(out, *extend, stops);
            }
            P::Glyph(gid, paint) => {
                out.extend_from_slice(&[10, 0, 0, 0]);
                out.extend_from_slice(&gid.to_be_bytes());
                child(out, 1, paint);
            }
            P::ColrGlyph(gid) => {
                out.push(11);
                out.extend_from_slice(&gid.to_be_bytes());
            }
            P::Transform(m, paint) => {
                out.extend_from_slice(&[12, 0, 0, 0, 0, 0, 0]);
                child(out, 1, paint);
                let t = out.len();
                for v in m {
                    out.extend_from_slice(&((v * 65536.0).round() as i32).to_be_bytes());
                }
                patch24(out, at + 4, t - at);
            }
            P::Translate(dx, dy, paint) => {
                out.extend_from_slice(&[14, 0, 0, 0]);
                words(out, &[*dx, *dy]);
                child(out, 1, paint);
            }
            P::Scale(sx, sy, paint) => {
                out.extend_from_slice(&[16, 0, 0, 0]);
                out.extend_from_slice(&f2(*sx));
                out.extend_from_slice(&f2(*sy));
                child(out, 1, paint);
            }
            P::Rotate(angle, paint) => {
                out.extend_from_slice(&[24, 0, 0, 0]);
                out.extend_from_slice(&f2(*angle));
                child(out, 1, paint);
            }
            P::RotateAround(angle, cx, cy, paint) => {
                out.extend_from_slice(&[26, 0, 0, 0]);
                out.extend_from_slice(&f2(*angle));
                words(out, &[*cx, *cy]);
                child(out, 1, paint);
            }
            P::Skew(x, y, paint) => {
                out.extend_from_slice(&[28, 0, 0, 0]);
                out.extend_from_slice(&f2(*x));
                out.extend_from_slice(&f2(*y));
                child(out, 1, paint);
            }
            P::Composite(mode, source, backdrop) => {
                out.extend_from_slice(&[32, 0, 0, 0, *mode, 0, 0, 0]);
                child(out, 1, source);
                child(out, 5, backdrop);
            }
            P::VarSolid(i, a, base) => {
                out.push(3);
                out.extend_from_slice(&i.to_be_bytes());
                out.extend_from_slice(&f2(*a));
                out.extend_from_slice(&base.to_be_bytes());
            }
            P::VarTranslate(dx, dy, base, paint) => {
                out.extend_from_slice(&[15, 0, 0, 0]);
                words(out, &[*dx, *dy]);
                out.extend_from_slice(&base.to_be_bytes());
                child(out, 1, paint);
            }
        }
        at
    }

    fn put32(out: &mut [u8], at: usize, v: usize) {
        out[at..at + 4].copy_from_slice(&(v as u32).to_be_bytes());
    }

    /// A version-1 `COLR`: `bases` (sorted by glyph), `layers`, and `clips`
    /// (first glyph, last glyph, box).
    fn colr_v1(bases: &[(u16, P)], layers: &[P], clips: &[(u16, u16, [i16; 4])]) -> Vec<u8> {
        colr_v1_varied(bases, layers, clips, None, None)
    }

    /// [`colr_v1`] with an `ItemVariationStore` and a `DeltaSetIndexMap`.
    fn colr_v1_varied(
        bases: &[(u16, P)],
        layers: &[P],
        clips: &[(u16, u16, [i16; 4])],
        store: Option<Vec<u8>>,
        map: Option<Vec<u8>>,
    ) -> Vec<u8> {
        let mut out = vec![0u8; 34];
        out[0..2].copy_from_slice(&1u16.to_be_bytes());
        let base_list = out.len();
        out.extend_from_slice(&(bases.len() as u32).to_be_bytes());
        for (gid, _) in bases {
            out.extend_from_slice(&gid.to_be_bytes());
            out.extend_from_slice(&[0; 4]);
        }
        let layer_list = out.len();
        out.extend_from_slice(&(layers.len() as u32).to_be_bytes());
        out.extend(core::iter::repeat_n(0u8, 4 * layers.len()));
        let clip_list = out.len();
        out.push(1);
        out.extend_from_slice(&(clips.len() as u32).to_be_bytes());
        for (start, end, _) in clips {
            out.extend_from_slice(&start.to_be_bytes());
            out.extend_from_slice(&end.to_be_bytes());
            out.extend_from_slice(&[0; 3]);
        }
        for (i, (_, p)) in bases.iter().enumerate() {
            let at = emit(p, &mut out);
            put32(&mut out, base_list + 4 + 6 * i + 2, at - base_list);
        }
        for (i, p) in layers.iter().enumerate() {
            let at = emit(p, &mut out);
            put32(&mut out, layer_list + 4 + 4 * i, at - layer_list);
        }
        for (i, (_, _, b)) in clips.iter().enumerate() {
            let at = out.len();
            out.push(1);
            words(&mut out, b);
            patch24(&mut out, clip_list + 5 + 7 * i + 4, at - clip_list);
        }
        put32(&mut out, 14, base_list);
        put32(&mut out, 18, layer_list);
        put32(&mut out, 22, if clips.is_empty() { 0 } else { clip_list });
        for (field, table) in [(26, map), (30, store)] {
            if let Some(table) = table {
                let at = out.len();
                out.extend_from_slice(&table);
                put32(&mut out, field, at);
            }
        }
        out
    }

    /// A version-0 `COLR`: base glyphs `(glyph, first layer, layer count)`
    /// and layers `(glyph, palette index)`.
    pub(crate) fn colr_v0(bases: &[(u16, u16, u16)], layers: &[(u16, u16)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&(bases.len() as u16).to_be_bytes());
        out.extend_from_slice(&14u32.to_be_bytes());
        out.extend_from_slice(&((14 + 6 * bases.len()) as u32).to_be_bytes());
        out.extend_from_slice(&(layers.len() as u16).to_be_bytes());
        for &(gid, first, count) in bases {
            for v in [gid, first, count] {
                out.extend_from_slice(&v.to_be_bytes());
            }
        }
        for &(gid, index) in layers {
            out.extend_from_slice(&gid.to_be_bytes());
            out.extend_from_slice(&index.to_be_bytes());
        }
        out
    }

    /// A `CPAL` of one palette, `0xAARRGGBB` each.
    pub(crate) fn cpal(colours: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        let n = colours.len() as u16;
        for v in [0u16, n, 1, n] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(&14u32.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        for c in colours {
            let [a, r, g, b] = c.to_be_bytes();
            out.extend_from_slice(&[b, g, r, a]);
        }
        out
    }

    /// The fixture face with glyph 1 (`A`) painted as a red square and
    /// glyph 2 (`B`) as its triangle in the text colour, both version-0
    /// layers; glyph 3 (`C`) has no colour recipe and is drawn as an outline.
    pub(crate) fn colour_fixture() -> Vec<u8> {
        build_test_font_with(vec![
            (
                *b"COLR",
                colr_v0(&[(1, 0, 1), (2, 1, 1)], &[(1, 0), (2, 0xFFFF)]),
            ),
            (*b"CPAL", cpal(&[0xFFFF_0000])),
        ])
    }

    const RED: u16 = 0;
    const GREEN: u16 = 1;
    const BLUE: u16 = 2;
    const FOREGROUND: u16 = 0xFFFF;

    fn face_with(colr: Vec<u8>) -> Face {
        let cpal = cpal(&[0xFFFF_0000, 0xFF00_FF00, 0xFF00_00FF]);
        Face::parse(build_test_font_with(vec![
            (*b"COLR", colr),
            (*b"CPAL", cpal),
        ]))
        .unwrap()
    }

    /// Glyph `gid` at 100 px to the em -- a tenth of a pixel per font unit
    /// -- in black text.
    fn draw(face: &Face, gid: u16) -> ColourImage {
        render(face, gid, 0.1, &Coords::default(), 0xFF00_0000).expect("a colour glyph")
    }

    fn at(img: &ColourImage, x: u32, y: u32) -> u32 {
        img.pixels[(y * img.width + x) as usize]
    }

    /// `[alpha, red, green, blue]` of a pixel.
    fn argb(img: &ColourImage, x: u32, y: u32) -> [u8; 4] {
        at(img, x, y).to_be_bytes()
    }

    /// Whether every channel of `a` is within one of `b`'s: an edge that
    /// lands a ten-millionth of a pixel off a pixel boundary, as 100 font
    /// units times 0.1 in `f32` does, turns 127.5 into 127.49.
    fn near(a: u32, b: u32) -> bool {
        a.to_be_bytes()
            .iter()
            .zip(b.to_be_bytes())
            .all(|(&x, y)| x.abs_diff(y) <= 1)
    }

    fn place(img: &ColourImage) -> (i32, i32, u32, u32) {
        (img.left, img.top, img.width, img.height)
    }

    fn solid(index: u16) -> Box<P> {
        Box::new(P::Solid(index, 1.0))
    }

    fn square(fill: Box<P>) -> Box<P> {
        Box::new(P::Glyph(1, fill))
    }

    /// A face whose glyph 1 is `paint`.
    fn one(paint: P) -> Face {
        face_with(colr_v1(&[(1, paint)], &[], &[]))
    }

    #[test]
    fn a_version_0_glyph_is_its_layers_in_palette_colours() {
        let face = face_with(colr_v0(
            &[(1, 0, 1), (2, 1, 2)],
            &[(1, RED), (2, FOREGROUND), (1, GREEN)],
        ));
        assert!(has_colour(&face, 1) && has_colour(&face, 2));
        assert!(!has_colour(&face, 3));
        assert!(render(&face, 3, 0.1, &Coords::default(), 0).is_none());
        // The square, 10 pixels a side, its top-left corner 10 pixels right
        // of the pen and 10 above the baseline; solid red throughout.
        let img = draw(&face, 1);
        assert_eq!(place(&img), (10, -10, 10, 10));
        assert!(img.pixels.iter().all(|&p| p == 0xFFFF_0000));
        assert!(!img.uses_foreground);
        // The triangle in the text colour, then the square beside it in
        // green: one canvas around both.
        let img = render(&face, 2, 0.1, &Coords::default(), 0x8000_00FF).unwrap();
        assert!(img.uses_foreground);
        assert_eq!(place(&img), (0, -20, 20, 20));
        // Inside the triangle, near its base: half-transparent blue,
        // premultiplied.
        assert_eq!(argb(&img, 5, 18), [0x80, 0, 0, 0x80]);
        assert_eq!(argb(&img, 15, 18), [0xFF, 0, 0xFF, 0]);
        // Above the square, beside the apex: nothing.
        assert_eq!(at(&img, 15, 2), 0);
    }

    #[test]
    fn a_solid_paint_is_cut_to_its_glyph() {
        let img = draw(&one(P::Glyph(1, Box::new(P::Solid(GREEN, 0.5)))), 1);
        assert_eq!(place(&img), (10, -10, 10, 10));
        assert!(
            img.pixels.iter().all(|&p| near(p, 0x8000_8000)),
            "{:x?}",
            img.pixels
        );
    }

    #[test]
    fn the_text_colour_is_palette_entry_ffff_and_is_reported() {
        let face = one(P::Glyph(1, solid(FOREGROUND)));
        let img = render(&face, 1, 0.1, &Coords::default(), 0xFF12_3456).unwrap();
        assert!(img.uses_foreground);
        assert!(img.pixels.iter().all(|&p| p == 0xFF12_3456));
        // In a gradient's stops too.
        let face = one(P::Glyph(
            1,
            Box::new(P::Linear(
                0,
                vec![(0.0, RED, 1.0), (1.0, FOREGROUND, 1.0)],
                [100, 0, 200, 0, 100, 100],
            )),
        ));
        assert!(draw(&face, 1).uses_foreground);
        // And not otherwise.
        assert!(!draw(&one(P::Glyph(1, solid(RED))), 1).uses_foreground);
    }

    #[test]
    fn a_linear_gradient_runs_from_p0_to_p1_in_bands_along_p0_p2() {
        let stops = vec![(0.0, RED, 1.0), (1.0, BLUE, 1.0)];
        // Left to right across the square, in vertical bands.
        let img = draw(
            &one(P::Glyph(
                1,
                Box::new(P::Linear(0, stops.clone(), [100, 0, 200, 0, 100, 100])),
            )),
            1,
        );
        for y in 0..10 {
            for x in 0..10 {
                let [a, r, g, b] = argb(&img, x, y);
                // Pixel centres sit at t = 0.05, 0.15, ... 0.95.
                let t = (x as f32 + 0.5) / 10.0;
                assert_eq!((a, g), (255, 0));
                assert!(
                    (f32::from(r) - (1.0 - t) * 255.0).abs() <= 1.0,
                    "{x},{y}: {r}"
                );
                assert!((f32::from(b) - t * 255.0).abs() <= 1.0, "{x},{y}: {b}");
            }
        }
        // With p2 up the diagonal, the bands lie along the diagonal: the
        // parameter runs from p0 towards (150, -50), p1 swung onto the
        // perpendicular -- so the square's centre is on the band through p0,
        // and its bottom-right corner a whole band away.
        let img = draw(
            &one(P::Glyph(
                1,
                Box::new(P::Linear(0, stops, [100, 0, 200, 0, 200, 100])),
            )),
            1,
        );
        let [_, r, _, b] = argb(&img, 4, 5);
        assert!(r >= 240 && b <= 15, "{r} {b}");
        let [_, r, _, b] = argb(&img, 9, 9);
        assert!(r <= 30 && b >= 225, "{r} {b}");
        // The anti-diagonal is one band.
        assert_eq!(argb(&img, 0, 9), argb(&img, 9, 0));
    }

    #[test]
    fn a_radial_gradient_grows_from_the_first_circle_to_the_second() {
        let img = draw(
            &one(P::Glyph(
                1,
                Box::new(P::Radial(
                    0,
                    vec![(0.0, RED, 1.0), (1.0, BLUE, 1.0)],
                    [150, 50],
                    0,
                    [150, 50],
                    50,
                )),
            )),
            1,
        );
        // The four centre pixels' centres are 7 units from the centre.
        let t = 5f32.hypot(5.0) / 50.0;
        let [_, r, _, b] = argb(&img, 4, 4);
        assert!((f32::from(r) - (1.0 - t) * 255.0).abs() <= 1.0, "{r} {t}");
        assert!((f32::from(b) - t * 255.0).abs() <= 1.0, "{b} {t}");
        assert_eq!(argb(&img, 4, 4), argb(&img, 5, 5));
        // A corner pixel is 64 units out, past the second circle: padded.
        assert_eq!(argb(&img, 0, 0), [255, 0, 0, 255]);
        // Three pixels right of the centre.
        let [_, r, _, b] = argb(&img, 7, 4);
        let t = 25f32.hypot(5.0) / 50.0;
        assert!((f32::from(r) - (1.0 - t) * 255.0).abs() <= 1.0, "{r} {t}");
        assert!((f32::from(b) - t * 255.0).abs() <= 1.0, "{b} {t}");
    }

    #[test]
    fn a_sweep_gradient_turns_counter_clockwise_from_its_stored_start() {
        // Stored -1.0 and 1.0: 0 and 360 degrees, once the bias is added.
        let img = draw(
            &one(P::Glyph(
                1,
                Box::new(P::Sweep(
                    0,
                    vec![(0.0, RED, 1.0), (1.0, BLUE, 1.0)],
                    [150, 50],
                    -1.0,
                    1.0,
                )),
            )),
            1,
        );
        let expect = |deg: f32, [_, r, _, b]: [u8; 4]| {
            let t = deg / 360.0;
            assert!(
                (f32::from(r) - (1.0 - t) * 255.0).abs() <= 1.5,
                "{deg}: {r}"
            );
            assert!((f32::from(b) - t * 255.0).abs() <= 1.5, "{deg}: {b}");
        };
        // Just above the +x axis, a few degrees round: nearly red. Just below
        // it, a few degrees short of the full turn: nearly blue.
        expect(5f32.atan2(45.0).to_degrees(), argb(&img, 9, 4));
        expect(360.0 - 5f32.atan2(45.0).to_degrees(), argb(&img, 9, 5));
        // Near the top, a quarter of the way round; at the left, half-way.
        expect(45f32.atan2(5.0).to_degrees(), argb(&img, 5, 0));
        expect(180.0 + 5f32.atan2(45.0).to_degrees(), argb(&img, 0, 5));
    }

    #[test]
    fn transforms_move_their_paint_as_fonttools_does() {
        let red = || square(solid(RED));
        let placed = |p: P| place(&draw(&one(p), 1));
        // 300 units right: the square at x 400..500.
        assert_eq!(placed(P::Translate(300, 0, red())), (40, -10, 10, 10));
        // A quarter-turn counter-clockwise about the origin takes (x, y) to
        // (-y, x): the square at x -100..0, y 100..200.
        assert_eq!(placed(P::Rotate(0.5, red())), (-10, -20, 10, 10));
        // A half-turn about its own centre leaves it where it was.
        let img = draw(&one(P::RotateAround(1.0, 150, 50, red())), 1);
        assert_eq!(place(&img), (10, -10, 10, 10));
        assert!(img.pixels.iter().all(|&p| p == 0xFFFF_0000));
        // Scaled: x 50..100, y 0..150.
        assert_eq!(placed(P::Scale(0.5, 1.5, red())), (5, -15, 5, 15));
        // An affine: half size, then 20 units up.
        assert_eq!(
            placed(P::Transform([0.5, 0.0, 0.0, 0.5, 0.0, 20.0], red())),
            (5, -7, 5, 5)
        );
        // Nested transforms apply innermost first: halved, then moved --
        // x 350..400, not x 200..250.
        assert_eq!(
            placed(P::Translate(300, 0, Box::new(P::Scale(0.5, 0.5, red())))),
            (35, -5, 5, 5)
        );
        // A skew of 45 degrees on x leans the y axis counter-clockwise:
        // x' = x - y, so the top of the square moves left, to x 0..100.
        let img = draw(&one(P::Skew(0.25, 0.0, red())), 1);
        assert_eq!(place(&img), (0, -10, 20, 10));
        assert_eq!(argb(&img, 15, 0)[0], 0);
        assert_eq!(argb(&img, 15, 9)[0], 255);
        assert_eq!(argb(&img, 2, 0)[0], 255);
    }

    #[test]
    fn a_composite_combines_its_source_and_backdrop() {
        // A red field, and green in the square only, on a canvas twice as
        // wide as the square: pixel 2 is inside the square, pixel 15 not.
        let with_mode = |mode: u8| {
            let paint = P::Composite(mode, solid(RED), square(solid(GREEN)));
            let img = draw(
                &face_with(colr_v1(&[(1, paint)], &[], &[(1, 1, [100, 0, 300, 100])])),
                1,
            );
            assert_eq!(place(&img), (10, -10, 20, 10));
            (at(&img, 2, 5), at(&img, 15, 5))
        };
        assert_eq!(with_mode(0), (0, 0));
        assert_eq!(with_mode(1), (0xFFFF_0000, 0xFFFF_0000));
        assert_eq!(with_mode(2), (0xFF00_FF00, 0));
        assert_eq!(with_mode(3), (0xFFFF_0000, 0xFFFF_0000));
        assert_eq!(with_mode(4), (0xFF00_FF00, 0xFFFF_0000));
        assert_eq!(with_mode(5), (0xFFFF_0000, 0));
        assert_eq!(with_mode(6), (0xFF00_FF00, 0));
        assert_eq!(with_mode(7), (0, 0xFFFF_0000));
        assert_eq!(with_mode(8), (0, 0));
        assert_eq!(with_mode(9), (0xFFFF_0000, 0));
        assert_eq!(with_mode(10), (0xFF00_FF00, 0xFFFF_0000));
        assert_eq!(with_mode(11), (0, 0xFFFF_0000));
        assert_eq!(with_mode(12), (0xFFFF_FF00, 0xFFFF_0000));
        // Multiply: red times green is black.
        assert_eq!(with_mode(23), (0xFF00_0000, 0xFFFF_0000));
    }

    #[test]
    fn blend_modes_follow_the_w3c_formulas() {
        let s = [0.8, 0.4, 0.2, 1.0];
        let b = [0.25, 0.5, 0.75, 1.0];
        let close = |got: Rgba, want: [f32; 3]| {
            for (g, w) in got.iter().zip(want) {
                assert!((g - w).abs() < 1e-5, "{got:?} {want:?}");
            }
            assert!((got[3] - 1.0).abs() < 1e-6);
        };
        close(composite(23, s, b), [0.2, 0.2, 0.15]);
        close(composite(13, s, b), [0.85, 0.7, 0.8]);
        close(composite(15, s, b), [0.25, 0.4, 0.2]);
        close(composite(16, s, b), [0.8, 0.5, 0.75]);
        close(composite(21, s, b), [0.55, 0.1, 0.55]);
        close(composite(22, s, b), [0.65, 0.5, 0.65]);
        // Overlay: the backdrop decides multiply or screen.
        close(composite(14, s, b), [0.4, 0.4, 0.6]);
        // Hard light: the source does.
        close(composite(19, s, b), [0.7, 0.4, 0.3]);
        // Colour dodge and burn.
        close(composite(17, s, b), [1.0, 0.833_333_3, 0.9375]);
        close(composite(18, s, b), [0.062_5, 0.0, 0.0]);
        // Luminosity keeps the backdrop's hue and takes the source's
        // lightness; colour the reverse.
        let lum = |c: Rgba| 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2];
        assert!((lum(composite(27, s, b)) - lum(s)).abs() < 1e-5);
        assert!((lum(composite(26, s, b)) - lum(b)).abs() < 1e-5);
        // Where only one is present, it shows alone.
        assert_eq!(composite(23, s, CLEAR), s);
        assert_eq!(composite(23, CLEAR, b), b);
        // An unknown mode is source over.
        let half = [0.0, 0.0, 0.5, 0.5];
        assert_eq!(composite(200, half, b), over(half, b));
    }

    #[test]
    fn a_colour_line_pads_repeats_and_reflects() {
        let line = |extend: u8| {
            ColourLine::new(
                extend,
                &Stops(vec![
                    (0.0, [1.0, 0.0, 0.0, 1.0]),
                    (1.0, [0.0, 0.0, 1.0, 1.0]),
                ]),
            )
        };
        let red = |c: Rgba| (c[0] * 100.0).round();
        assert_eq!(red(line(0).at(1.25)), 0.0);
        assert_eq!(red(line(0).at(-3.0)), 100.0);
        assert_eq!(red(line(1).at(1.25)), 75.0);
        assert_eq!(red(line(1).at(-0.25)), 25.0);
        assert_eq!(red(line(2).at(1.25)), 25.0);
        assert_eq!(red(line(2).at(-0.25)), 75.0);
        assert_eq!(red(line(0).at(0.5)), 50.0);
        // Two stops at one offset are a hard edge.
        let edge = ColourLine::new(
            0,
            &Stops(vec![
                (0.0, [1.0, 0.0, 0.0, 1.0]),
                (0.5, [1.0, 0.0, 0.0, 1.0]),
                (0.5, [0.0, 0.0, 1.0, 1.0]),
                (1.0, [0.0, 0.0, 1.0, 1.0]),
            ]),
        );
        assert_eq!(edge.at(0.49)[0], 1.0);
        assert_eq!(edge.at(0.51)[2], 1.0);
        // Interpolated premultiplied: a fade to transparent does not darken.
        let fade = ColourLine::new(0, &Stops(vec![(0.0, [1.0, 1.0, 1.0, 1.0]), (1.0, CLEAR)]));
        let c = fade.at(0.5);
        assert_eq!(c[0], c[3]);
    }

    #[test]
    fn a_clip_box_sizes_the_canvas() {
        let face = face_with(colr_v1(
            &[(1, P::Glyph(1, solid(RED))), (3, P::Glyph(3, solid(RED)))],
            &[],
            &[(1, 2, [0, -100, 1000, 900])],
        ));
        assert_eq!(place(&draw(&face, 1)), (0, -90, 100, 100));
        // Glyph 3 is not in the clip's range: sized to its outline.
        assert_eq!(place(&draw(&face, 3)), (60, -30, 10, 10));
        // A clip too big to draw is declined, and the caller draws the
        // outline instead.
        let face = face_with(colr_v1(
            &[(1, P::Glyph(1, solid(RED)))],
            &[],
            &[(1, 1, [-32768, -32768, 32767, 32767])],
        ));
        assert!(render(&face, 1, 0.1, &Coords::default(), 0).is_none());
        assert!(render(&face, 1, 0.01, &Coords::default(), 0).is_some());
    }

    #[test]
    fn layers_paint_bottom_first_and_a_colr_glyph_reuses_another_graph() {
        let face = face_with(colr_v1(
            &[
                (1, P::Layers(2, 0)),
                (2, P::ColrGlyph(1)),
                (3, P::Translate(-500, -200, Box::new(P::ColrGlyph(1)))),
            ],
            &[
                P::Glyph(1, solid(RED)),
                P::Glyph(1, Box::new(P::Solid(BLUE, 0.5))),
            ],
            &[],
        ));
        let img = draw(&face, 1);
        // Half blue over red.
        assert_eq!(argb(&img, 5, 5), [255, 128, 0, 128]);
        assert_eq!(draw(&face, 2), img);
        let moved = draw(&face, 3);
        assert_eq!(place(&moved), (-40, 10, 10, 10));
        assert!(
            moved
                .pixels
                .iter()
                .zip(&img.pixels)
                .all(|(&a, &b)| near(a, b))
        );
    }

    #[test]
    fn a_cycle_or_a_fan_out_ends() {
        // Glyph 1 paints red and then itself, forever.
        let face = face_with(colr_v1(
            &[(1, P::Layers(2, 0))],
            &[P::Glyph(1, solid(RED)), P::ColrGlyph(1)],
            &[],
        ));
        let img = draw(&face, 1);
        assert!(img.pixels.iter().all(|&p| p == 0xFFFF_0000));
        // Each of 255 layers is the same 255 layers: 255 to the 64th paints,
        // unless something stops it.
        let layers: Vec<P> = (0..255).map(|_| P::Layers(255, 0)).collect();
        let face = face_with(colr_v1(
            &[(1, P::Layers(255, 0))],
            &layers,
            &[(1, 1, [0, 0, 1000, 1000])],
        ));
        let img = draw(&face, 1);
        assert!(img.pixels.iter().all(|&p| p == 0));
        // Composites nested inside composites: each wants two canvases, and
        // the budget runs out long before the depth limit.
        let mut paint = P::Glyph(1, solid(RED));
        for _ in 0..40 {
            paint = P::Composite(3, Box::new(paint), solid(BLUE));
        }
        let face = face_with(colr_v1(&[(1, paint)], &[], &[(1, 1, [0, 0, 1000, 1000])]));
        let img = render(&face, 1, 1.0, &Coords::default(), 0).unwrap();
        assert_eq!((img.width, img.height), (1000, 1000));
    }

    /// A face on the fixture's one variation axis (`wght`, 100 to 700, 400
    /// the default) whose `COLR` is `colr`.
    fn variable_face_with(colr: Vec<u8>) -> Face {
        let cpal = cpal(&[0xFFFF_0000, 0xFF00_FF00, 0xFF00_00FF]);
        Face::parse(crate::sfnt::tests::build_variable_test_font_with(vec![
            (*b"COLR", colr),
            (*b"CPAL", cpal),
        ]))
        .unwrap()
    }

    fn at_weight(face: &Face, wght: f32) -> Coords {
        face.variation_axes()
            .unwrap()
            .normalize_tags(&[(*b"wght", wght)])
    }

    #[test]
    fn a_variable_paint_moves_with_the_axes() {
        // Rows: 100 and 0 (the translate), -127 (the alpha), 50.
        let store = crate::varstore::one_axis_store(&[100, 0, -127, 50]);
        let paint = P::VarTranslate(
            0,
            0,
            0,
            Box::new(P::Glyph(1, Box::new(P::VarSolid(RED, 1.0, 2)))),
        );
        let face = variable_face_with(colr_v1_varied(
            &[(1, paint)],
            &[],
            &[],
            Some(store.clone()),
            None,
        ));
        let draw_at = |wght: f32| render(&face, 1, 0.1, &at_weight(&face, wght), 0).unwrap();
        // The default instance: where the recipe says, opaque.
        let img = draw_at(400.0);
        assert_eq!(place(&img), (10, -10, 10, 10));
        assert!(img.pixels.iter().all(|&p| p == 0xFFFF_0000));
        // The heaviest: 100 units right, and alpha down by 127/16384.
        let img = draw_at(700.0);
        assert_eq!(place(&img), (20, -10, 10, 10));
        let a = (((1.0 - 127.0 / 16384.0) * 255.0) + 0.5) as u32;
        assert!(
            img.pixels.iter().all(|&p| p == (a << 24 | a << 16)),
            "{:x}",
            img.pixels[0]
        );
        // Half-way along the axis's upper half: half the delta.
        assert_eq!(place(&draw_at(550.0)), (15, -10, 10, 10));
        // A DeltaSetIndexMap sends index 0 to row 3 instead.
        let map = vec![0, 0x07, 0, 2, 3, 1];
        let paint = P::VarTranslate(0, 0, 0, square(solid(RED)));
        let face = variable_face_with(colr_v1_varied(
            &[(1, paint)],
            &[],
            &[],
            Some(store),
            Some(map),
        ));
        assert_eq!(
            place(&render(&face, 1, 0.1, &at_weight(&face, 700.0), 0).unwrap()),
            (15, -10, 10, 10)
        );
        // A base of 0xFFFFFFFF varies nothing.
        let paint = P::VarTranslate(0, 0, NO_VARIATION, square(solid(RED)));
        let face = variable_face_with(colr_v1_varied(
            &[(1, paint)],
            &[],
            &[],
            Some(crate::varstore::one_axis_store(&[100])),
            None,
        ));
        assert_eq!(
            place(&render(&face, 1, 0.1, &at_weight(&face, 700.0), 0).unwrap()),
            (10, -10, 10, 10)
        );
    }

    /// Every paint format, for the mutation test to break.
    fn everything() -> Vec<u8> {
        let stops = vec![(0.0, RED, 1.0), (0.5, FOREGROUND, 0.5), (1.0, BLUE, 1.0)];
        colr_v1(
            &[
                (1, P::Layers(3, 0)),
                (
                    2,
                    P::Composite(
                        14,
                        Box::new(P::Skew(0.1, -0.1, square(solid(GREEN)))),
                        Box::new(P::Glyph(
                            2,
                            Box::new(P::Sweep(1, stops.clone(), [50, 50], -0.5, 0.5)),
                        )),
                    ),
                ),
                (
                    3,
                    P::Transform([1.0, 0.2, -0.2, 1.0, 5.0, 5.0], Box::new(P::ColrGlyph(2))),
                ),
            ],
            &[
                P::Glyph(
                    1,
                    Box::new(P::Linear(2, stops.clone(), [100, 0, 200, 0, 100, 100])),
                ),
                P::RotateAround(
                    0.25,
                    150,
                    50,
                    Box::new(P::Glyph(
                        2,
                        Box::new(P::Radial(1, stops, [50, 50], 10, [60, 60], 90)),
                    )),
                ),
                P::Scale(0.5, 0.5, Box::new(P::ColrGlyph(3))),
            ],
            &[(2, 3, [-100, -100, 800, 400])],
        )
    }

    #[test]
    fn a_mutated_or_truncated_table_draws_something_or_nothing_but_never_panics() {
        let good = everything();
        let face = face_with(good.clone());
        for gid in 1..=3 {
            let img = render(&face, gid, 0.05, &Coords::default(), 0xFF00_0000).unwrap();
            assert!(img.pixels.iter().any(|&p| p != 0), "glyph {gid}");
        }
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..1500 {
            let mut bytes = good.clone();
            for _ in 0..=(next() % 4) {
                let i = (next() % bytes.len() as u64) as usize;
                bytes[i] = next() as u8;
            }
            let face = face_with(bytes);
            for gid in 0..=4 {
                let _ = render(&face, gid, 0.02, &Coords::default(), 0xFF00_0000);
                let _ = has_colour(&face, gid);
            }
        }
        for len in 0..good.len() {
            let face = face_with(good[..len].to_vec());
            for gid in 1..=3 {
                let _ = render(&face, gid, 0.02, &Coords::default(), 0xFF00_0000);
            }
        }
        // A version-0 table too.
        let good = colr_v0(
            &[(1, 0, 2), (2, 1, 2)],
            &[(1, RED), (2, FOREGROUND), (1, 7)],
        );
        for _ in 0..500 {
            let mut bytes = good.clone();
            let i = (next() % bytes.len() as u64) as usize;
            bytes[i] = next() as u8;
            let face = face_with(bytes);
            for gid in 0..=3 {
                let _ = render(&face, gid, 0.02, &Coords::default(), 0xFF00_0000);
            }
        }
    }
}
