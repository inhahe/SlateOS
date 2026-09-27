//! `glyf` — a TrueType glyph's points, and the outline drawn through them,
//! built as HarfBuzz builds them.
//!
//! # Points first, then the path
//!
//! HarfBuzz (`OT::glyf_impl::Glyph::get_points`, 14.3.0) does not assemble a
//! glyph as a path. It gathers the glyph's *points* -- a simple glyph's
//! contour points, or a composite's components' points, each moved by that
//! component's transform -- with four **phantom points** after them (the two
//! ends of the advance, the top and bottom of the vertical one); moves every
//! point by the glyph's `gvar` deltas at the instance asked for; shifts the
//! whole glyph so its left phantom point lands on the origin; and only then
//! walks the points into a path (`path_builder_t`), making up an on-curve
//! point halfway between two control points as it goes.
//!
//! At a varied instance the coordinates are fractions, and each step of that
//! order shows in their last bits: a halfway point made up before the shift
//! is not the one made up after it, and a component's points moved by
//! `x * a + y * c` are not those moved by a fused multiply-add. So this module
//! takes HarfBuzz's steps in HarfBuzz's order with HarfBuzz's arithmetic, and
//! `tools/outline_oracle.py` compares every coordinate of every glyph with
//! `==`. The default instance goes through the same steps without the deltas.
//!
//! # Also as HarfBuzz has it
//!
//! * A contour that starts off the curve is walked from its *last* point,
//!   which is where FreeType starts one too.
//! * A component placed by matching two points, rather than by an offset, is
//!   moved so that its point lands on the parent's -- the parent's point
//!   counted from the start of the whole glyph, as HarfBuzz counts it.
//! * A component that refers back to a glyph being expanded is dropped when
//!   HarfBuzz's cycle detector notices -- a tortoise and hare
//!   (`hb_decycler_t`), which notices a level or two late, so a cyclic glyph
//!   draws its other components that many extra times, as in HarfBuzz.
//!   Nesting past 64 levels, visiting more than 2,048 glyphs, or collecting
//!   more than 200,000 points fails the glyph.
//! * An off-curve point flagged `0x80` is a cubic's control point (the
//!   proposed cubic `glyf`).
//! * Malformed data is read as HarfBuzz reads it: a component list that runs
//!   out ends early, a component naming a glyph that does not exist (or whose
//!   `loca` entry is unusable) is an empty glyph, a coordinate accumulates
//!   past 16 bits rather than wrapping.
//!
//! # Not here
//!
//! * The top and bottom phantom points are placed as if the face had no
//!   `vmtx`, which this crate does not read. Nothing drawn, and nothing
//!   measured horizontally, depends on them.
//! * FreeType's reading of the same glyph, which the auto-hinter needs, is
//!   `Face::load_unscaled` in [`crate::sfnt`]: whole font units and
//!   FreeType's rounding, kept apart on purpose (design-decisions §1325).
//!
//! [`crate::sfnt`]: crate::sfnt

use alloc::vec::Vec;

use crate::gvar::Scalars;
use crate::sfnt::{BBox, Face, Outline, PathCmd, Point, SfntError, i16_at, u16_at};

/// A point flag: on the curve.
const FLAG_ON_CURVE: u8 = 0x01;
/// A point flag: the x delta is one byte, its sign in [`FLAG_X_SAME`].
const FLAG_X_SHORT: u8 = 0x02;
/// A point flag: the y delta is one byte, its sign in [`FLAG_Y_SAME`].
const FLAG_Y_SHORT: u8 = 0x04;
/// A point flag: the next byte counts further points with these flags.
const FLAG_REPEAT: u8 = 0x08;
/// A point flag: a short x delta is positive; a long one is absent (zero).
const FLAG_X_SAME: u8 = 0x10;
/// A point flag: a short y delta is positive; a long one is absent (zero).
const FLAG_Y_SAME: u8 = 0x20;
/// A point flag: an off-curve point is a cubic's control point.
const FLAG_CUBIC: u8 = 0x80;

/// The points every glyph carries after its own: see [`phantoms`].
pub(crate) const PHANTOM_COUNT: usize = 4;

/// `HB_MAX_NESTING_LEVEL`: how deep components may nest.
const MAX_NESTING_LEVEL: u32 = 64;
/// `HB_MAX_GRAPH_EDGE_COUNT`: how many glyphs one glyph may visit in all,
/// which is what stops a glyph using the same component twice at every level
/// from growing without bound.
const MAX_GRAPH_EDGE_COUNT: u32 = 2048;
/// `HB_GLYF_MAX_POINTS`: how many points one glyph may collect.
const MAX_POINTS: usize = 200_000;

/// Component flags.
const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
const ARGS_ARE_XY_VALUES: u16 = 0x0002;
const WE_HAVE_A_SCALE: u16 = 0x0008;
const MORE_COMPONENTS: u16 = 0x0020;
const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;
const USE_MY_METRICS: u16 = 0x0200;
const SCALED_COMPONENT_OFFSET: u16 = 0x0800;
const UNSCALED_COMPONENT_OFFSET: u16 = 0x1000;
const GID_IS_24BIT: u16 = 0x2000;

/// One of a glyph's points: HarfBuzz's `contour_point_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ContourPoint {
    pub(crate) x: f32,
    pub(crate) y: f32,
    /// The point's flags byte as the glyph stores it, of which
    /// [`FLAG_ON_CURVE`] and [`FLAG_CUBIC`] matter once it is read.
    pub(crate) flag: u8,
    /// The last point of its contour. A composite's offset points are each
    /// flagged, one point to a contour, so that `gvar`'s interpolation leaves
    /// them alone.
    pub(crate) is_end_point: bool,
}

impl ContourPoint {
    const fn at(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            flag: 0,
            is_end_point: false,
        }
    }

    /// `contour_point_t::transform`: each product rounded, then the sum --
    /// not fused, which would round once and differ in the last bit.
    fn transform(&mut self, m: &[f32; 4]) {
        let x = self.x * m[0] + self.y * m[2];
        self.y = self.x * m[1] + self.y * m[3];
        self.x = x;
    }
}

/// The glyph's outline at `coords` (HarfBuzz's normalized coordinates; empty
/// or all zero for the default instance), in font units, placed on its left
/// side bearing: HarfBuzz's `hb_font_draw_glyph`, pen call for pen call.
///
/// # Errors
///
/// Where HarfBuzz draws nothing: the glyph's data cannot be read, its
/// components nest or repeat past HarfBuzz's limits, or its variation data is
/// broken. An id past the face is [`SfntError::GlyphOutOfRange`].
pub(crate) fn outline(face: &Face, gid: u16, coords: &[i16]) -> Result<Outline, SfntError> {
    let points = points(face, gid, coords, Scalars::Drawn)?;
    let mut out = Outline::default();
    draw(contour_points(&points), &mut out);
    Ok(out)
}

/// The box around the glyph's points at `coords`, as HarfBuzz measures it
/// for `hb_font_get_glyph_extents` (`points_aggregator_t`): every point of
/// every contour, on the curve or not, and no scalar cache (see
/// [`Scalars`]). `None` for a glyph without contour points.
///
/// # Errors
///
/// As [`outline`].
pub(crate) fn bounds(face: &Face, gid: u16, coords: &[i16]) -> Result<Option<BBox>, SfntError> {
    let points = points(face, gid, coords, Scalars::Measured)?;
    let mut them = contour_points(&points).iter();
    let Some(first) = them.next() else {
        return Ok(None);
    };
    let mut b = BBox {
        x_min: first.x,
        y_min: first.y,
        x_max: first.x,
        y_max: first.y,
    };
    for p in them {
        b.x_min = b.x_min.min(p.x);
        b.y_min = b.y_min.min(p.y);
        b.x_max = b.x_max.max(p.x);
        b.y_max = b.y_max.max(p.y);
    }
    Ok(Some(b))
}

/// The glyph's advance at `coords` in a face whose `gvar` must carry it, for
/// want of `HVAR`: HarfBuzz 14.3.0's `glyf_accelerator_t::
/// get_advance_with_var_unscaled`, which `hb_font_get_glyph_h_advance` asks
/// in that case -- the distance between the left and right phantom points
/// where `gvar` moved them, by HarfBuzz's `roundf` and never below zero; or
/// half an em, HarfBuzz's answer for a glyph it cannot read.
pub(crate) fn advance(face: &Face, gid: u16, coords: &[i16]) -> u16 {
    let half_em = face.units_per_em() / 2;
    let Ok(points) = points(face, gid, coords, Scalars::Drawn) else {
        return half_em;
    };
    let first = points.len().saturating_sub(PHANTOM_COUNT);
    let (Some(left), Some(right)) = (points.get(first), points.get(first.saturating_add(1))) else {
        return half_em;
    };
    let width = crate::hbcalc::roundf(right.x - left.x).clamp(0.0, f32::from(u16::MAX));
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a whole number clamped into `u16` just above"
    )]
    {
        width as u16
    }
}

/// All of the glyph's points at `coords`, its four phantom points last, as
/// HarfBuzz's `glyf_accelerator_t::get_points` collects them.
///
/// # Errors
///
/// As [`outline`].
pub(crate) fn points(
    face: &Face,
    gid: u16,
    coords: &[i16],
    scalars: Scalars,
) -> Result<Vec<ContourPoint>, SfntError> {
    if gid >= face.num_glyphs() {
        return Err(SfntError::GlyphOutOfRange);
    }
    let mut builder = Builder {
        face,
        coords,
        varied: coords.iter().any(|&c| c != 0),
        scalars,
        edges: 0,
        decycler: Decycler::default(),
    };
    let mut all = Vec::new();
    builder.get_points(&glyph_for_gid(face, u32::from(gid)), &mut all, 0)?;
    Ok(all)
}

/// The points that are the glyph's contours: all of them but the phantoms.
fn contour_points(points: &[ContourPoint]) -> &[ContourPoint] {
    points
        .get(..points.len().saturating_sub(PHANTOM_COUNT))
        .unwrap_or_default()
}

/// A glyph as HarfBuzz's `glyph_for_gid` finds it.
struct Glyph<'a> {
    /// Its id -- or `None` for the empty glyph HarfBuzz makes of an id past
    /// the face or a `loca` entry it will not use, whose metrics it then
    /// cannot look up.
    gid: Option<u16>,
    bytes: &'a [u8],
    kind: Kind,
}

enum Kind {
    /// No contours, or data too short for a header: only phantom points.
    Empty,
    /// This many contours.
    Simple(usize),
    Composite,
}

impl Glyph<'_> {
    /// The header's `xMin`, or 0 without a header.
    fn x_min(&self) -> i32 {
        self.header_word(2)
    }

    /// The header's `yMax`, or 0 without a header.
    fn y_max(&self) -> i32 {
        self.header_word(8)
    }

    fn header_word(&self, at: usize) -> i32 {
        if self.bytes.len() < 10 {
            return 0;
        }
        i16_at(self.bytes, at).map_or(0, i32::from)
    }
}

fn glyph_for_gid(face: &Face, gid: u32) -> Glyph<'_> {
    let none = Glyph {
        gid: None,
        bytes: &[],
        kind: Kind::Empty,
    };
    let Ok(gid) = u16::try_from(gid) else {
        return none;
    };
    let Some(bytes) = face.glyf_bytes(gid) else {
        return none;
    };
    let contours = if bytes.len() < 10 {
        0
    } else {
        i16_at(bytes, 0).unwrap_or(0)
    };
    let kind = match usize::try_from(contours) {
        Ok(0) => Kind::Empty,
        Ok(n) => Kind::Simple(n),
        Err(_) => Kind::Composite,
    };
    Glyph {
        gid: Some(gid),
        bytes,
        kind,
    }
}

/// A glyph's four phantom points, before any deltas: the left and right ends
/// of its advance -- placed so that the left one sits `lsb` to the left of
/// `xMin` -- and the top and bottom of its vertical advance.
///
/// The vertical pair is placed as HarfBuzz places it for a face without
/// `vmtx` (at `yMax`, and one em below), which is what every face is to this
/// crate; see the module docs.
fn phantoms(face: &Face, glyph: &Glyph<'_>) -> [ContourPoint; PHANTOM_COUNT] {
    let lsb = glyph
        .gid
        .and_then(|g| face.left_side_bearing(g).ok())
        .map_or(0, i32::from);
    let h_adv = glyph
        .gid
        .and_then(|g| face.advance(g).ok())
        .map_or(0, i32::from);
    let h_delta = glyph.x_min().saturating_sub(lsb);
    let v_orig = glyph.y_max();
    let v_adv = i32::from(face.units_per_em());
    [
        ContourPoint::at(whole(h_delta), 0.0),
        ContourPoint::at(whole(h_adv.saturating_add(h_delta)), 0.0),
        ContourPoint::at(0.0, whole(v_orig)),
        ContourPoint::at(0.0, whole(v_orig.saturating_sub(v_adv))),
    ]
}

/// An integer as C converts one to `float`: to the nearest, which is exact
/// below 2^24 and so for everything but a malformed glyph's coordinates.
fn whole(v: i32) -> f32 {
    #[allow(
        clippy::cast_precision_loss,
        reason = "rounds to nearest past 2^24, as C's int-to-float conversion does"
    )]
    {
        v as f32
    }
}

struct Builder<'f> {
    face: &'f Face,
    coords: &'f [i16],
    /// Some coordinate is not zero (`hb_any (coords)`): only then does
    /// `gvar` apply.
    varied: bool,
    scalars: Scalars,
    /// Glyphs visited so far, for [`MAX_GRAPH_EDGE_COUNT`].
    edges: u32,
    decycler: Decycler,
}

impl Builder<'_> {
    /// `Glyph::get_points`: append `glyph`'s points, its phantom points
    /// last, to `all`.
    fn get_points(
        &mut self,
        glyph: &Glyph<'_>,
        all: &mut Vec<ContourPoint>,
        depth: u32,
    ) -> Result<(), SfntError> {
        if depth > MAX_NESTING_LEVEL || self.edges > MAX_GRAPH_EDGE_COUNT {
            return Err(SfntError::CompositeTooDeep);
        }
        self.edges = self.edges.saturating_add(1);

        let shift = match glyph.kind {
            Kind::Simple(contours) => {
                let old = all.len();
                read_contour_points(glyph.bytes, contours, all)?;
                all.extend_from_slice(&phantoms(self.face, glyph));
                let own = all.get_mut(old..).unwrap_or_default();
                self.vary(glyph, own)?;
                left_phantom(all)
            }
            Kind::Composite => {
                let records = components(glyph.bytes);
                // A composite's own points are one per component -- its
                // offset, which `gvar` may move -- and its phantom points.
                let mut own: Vec<ContourPoint> = records.iter().map(Record::offset).collect();
                own.extend_from_slice(&phantoms(self.face, glyph));
                self.vary(glyph, &mut own)?;
                let node = self.decycler.enter();
                let placed = self.place(&records, &mut own, all, depth, node);
                self.decycler.leave();
                placed?;
                let first_phantom = own.len().saturating_sub(PHANTOM_COUNT);
                all.extend_from_slice(own.get(first_phantom..).unwrap_or_default());
                left_phantom(all)
            }
            Kind::Empty => {
                let mut own = phantoms(self.face, glyph);
                self.vary(glyph, &mut own)?;
                all.extend_from_slice(&own);
                left_phantom(all)
            }
        };

        // "Undocumented rasterizer behavior": the finished glyph moves so
        // that its left phantom point, where `gvar` left it, is the origin.
        #[allow(
            clippy::float_cmp,
            reason = "HarfBuzz's own test: a zero shift is skipped, which \
                      keeps a -0.0 coordinate's sign"
        )]
        if depth == 0 && shift != 0.0 {
            for p in all.iter_mut() {
                p.x -= shift;
            }
        }
        Ok(())
    }

    /// Move `points` -- `glyph`'s own, phantoms last -- by its `gvar`
    /// deltas, at a varied instance.
    fn vary(&self, glyph: &Glyph<'_>, points: &mut [ContourPoint]) -> Result<(), SfntError> {
        match glyph.gid {
            Some(gid) if self.varied => {
                self.face.gvar_apply(gid, self.coords, points, self.scalars)
            }
            _ => Ok(()),
        }
    }

    /// The component loop of `Glyph::get_points`: append each component's
    /// points to `all`, placed. `own` is the composite's own points (the
    /// varied offsets, then its phantoms, which a `USE_MY_METRICS`
    /// component replaces with its own).
    fn place(
        &mut self,
        records: &[Record],
        own: &mut [ContourPoint],
        all: &mut Vec<ContourPoint>,
        depth: u32,
        node: usize,
    ) -> Result<(), SfntError> {
        let first_phantom = own.len().saturating_sub(PHANTOM_COUNT);
        for (index, record) in records.iter().enumerate() {
            if !self.decycler.visit(node, record.gid) {
                continue;
            }
            let old = all.len();
            let child = glyph_for_gid(self.face, record.gid);
            self.get_points(&child, all, depth.saturating_add(1))?;

            let comp = all.get_mut(old..).unwrap_or_default();
            if record.flags & USE_MY_METRICS != 0 {
                let theirs = comp.get(comp.len().saturating_sub(PHANTOM_COUNT)..);
                let ours = own.get_mut(first_phantom..);
                if let (Some(theirs), Some(ours)) = (theirs, ours)
                    && theirs.len() == ours.len()
                {
                    ours.copy_from_slice(theirs);
                }
            }
            if let Some(&offset) = own.get(index) {
                record.transform_points(comp, offset);
            }

            if record.flags & ARGS_ARE_XY_VALUES == 0 {
                // Matched points: the parent's point counted over the whole
                // glyph so far, the component's over its own points and
                // phantoms, both after the transform.
                let (p1, p2) = record.anchor;
                let comp_len = all.len().saturating_sub(old);
                if p1 < all.len() && p2 < comp_len {
                    let (Some(&target), Some(&moving)) =
                        (all.get(p1), all.get(old.saturating_add(p2)))
                    else {
                        continue;
                    };
                    let (dx, dy) = (target.x - moving.x, target.y - moving.y);
                    translate(all.get_mut(old..).unwrap_or_default(), dx, dy);
                }
            }

            all.truncate(all.len().saturating_sub(PHANTOM_COUNT));
            if all.len() > MAX_POINTS {
                return Err(SfntError::MalformedTable("glyf"));
            }
        }
        Ok(())
    }
}

/// The left phantom point's x -- the fourth point from the end.
fn left_phantom(all: &[ContourPoint]) -> f32 {
    all.len()
        .checked_sub(PHANTOM_COUNT)
        .and_then(|i| all.get(i))
        .map_or(0.0, |p| p.x)
}

/// `CompositeGlyphRecord::translate`: an axis moved only if its offset is not
/// zero, which keeps a `-0.0` where HarfBuzz keeps one.
#[allow(
    clippy::float_cmp,
    reason = "HarfBuzz's own tests, exactly: zero is skipped"
)]
fn translate(points: &mut [ContourPoint], dx: f32, dy: f32) {
    if dx != 0.0 && dy != 0.0 {
        for p in points {
            p.x += dx;
            p.y += dy;
        }
    } else if dx != 0.0 {
        for p in points {
            p.x += dx;
        }
    } else if dy != 0.0 {
        for p in points {
            p.y += dy;
        }
    }
}

/// One component record of a composite glyph.
struct Record {
    flags: u16,
    gid: u32,
    /// The offset, as `float`; zero for a component placed by points.
    tx: f32,
    ty: f32,
    /// The parent's point and the component's, for one placed by points.
    anchor: (usize, usize),
    /// `x' = x * m0 + y * m2`, `y' = x * m1 + y * m3`.
    matrix: [f32; 4],
}

impl Record {
    /// The component's offset as one of the composite's points
    /// (`CompositeGlyphRecord::get_points`).
    fn offset(&self) -> ContourPoint {
        ContourPoint {
            x: self.tx,
            y: self.ty,
            flag: 0,
            is_end_point: true,
        }
    }

    /// `CompositeGlyphRecord::transform_points`: the matrix then the offset,
    /// or with `SCALED_COMPONENT_OFFSET` alone the other way round.
    fn transform_points(&self, points: &mut [ContourPoint], offset: ContourPoint) {
        let scaled_offsets = self.flags & (SCALED_COMPONENT_OFFSET | UNSCALED_COMPONENT_OFFSET)
            == SCALED_COMPONENT_OFFSET;
        if scaled_offsets {
            translate(points, offset.x, offset.y);
            self.transform(points);
        } else {
            self.transform(points);
            translate(points, offset.x, offset.y);
        }
    }

    #[allow(
        clippy::float_cmp,
        reason = "HarfBuzz's own test for an identity matrix, exactly"
    )]
    fn transform(&self, points: &mut [ContourPoint]) {
        let m = &self.matrix;
        if m[0] != 1.0 || m[1] != 0.0 || m[2] != 0.0 || m[3] != 1.0 {
            for p in points {
                p.transform(m);
            }
        }
    }
}

/// A composite's component records, as HarfBuzz's `composite_iter_t` walks
/// them: a record that does not fit in the glyph ends the list, quietly.
fn components(bytes: &[u8]) -> Vec<Record> {
    let mut out = Vec::new();
    let mut at = 10usize;
    while let Some(flags) = u16_at(bytes, at) {
        let size = record_size(flags);
        if at.checked_add(4).is_none_or(|e| e > bytes.len())
            || at.checked_add(size).is_none_or(|e| e > bytes.len())
        {
            break;
        }
        let Some(record) = read_record(bytes, at, flags) else {
            break;
        };
        out.push(record);
        if flags & MORE_COMPONENTS == 0 {
            break;
        }
        at = at.saturating_add(size);
    }
    out
}

/// `CompositeGlyphRecord::get_size`.
fn record_size(flags: u16) -> usize {
    let mut size = 4usize;
    if flags & GID_IS_24BIT != 0 {
        size = size.saturating_add(1);
    }
    size = size.saturating_add(if flags & ARG_1_AND_2_ARE_WORDS != 0 {
        4
    } else {
        2
    });
    size.saturating_add(if flags & WE_HAVE_A_SCALE != 0 {
        2
    } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
        4
    } else if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
        8
    } else {
        0
    })
}

/// One record at `at`, which [`components`] has checked fits.
fn read_record(bytes: &[u8], at: usize, flags: u16) -> Option<Record> {
    let byte = |i: usize| bytes.get(i).copied();
    let mut p = at.checked_add(2)?;
    let gid = if flags & GID_IS_24BIT != 0 {
        let g = (u32::from(byte(p)?) << 16)
            | (u32::from(byte(p.checked_add(1)?)?) << 8)
            | u32::from(byte(p.checked_add(2)?)?);
        p = p.checked_add(3)?;
        g
    } else {
        let g = u32::from(u16_at(bytes, p)?);
        p = p.checked_add(2)?;
        g
    };
    let (tx, ty, anchor) = if flags & ARG_1_AND_2_ARE_WORDS != 0 {
        let a = u16_at(bytes, p)?;
        let b = u16_at(bytes, p.checked_add(2)?)?;
        p = p.checked_add(4)?;
        #[allow(clippy::cast_possible_wrap, reason = "an offset word is signed")]
        (
            i32::from(a as i16),
            i32::from(b as i16),
            (usize::from(a), usize::from(b)),
        )
    } else {
        let a = byte(p)?;
        let b = byte(p.checked_add(1)?)?;
        p = p.checked_add(2)?;
        #[allow(clippy::cast_possible_wrap, reason = "an offset byte is signed")]
        (
            i32::from(a as i8),
            i32::from(b as i8),
            (usize::from(a), usize::from(b)),
        )
    };
    // A component placed by points has no offset of its own.
    let (tx, ty) = if flags & ARGS_ARE_XY_VALUES == 0 {
        (0, 0)
    } else {
        (tx, ty)
    };
    let f2dot14 = |i: usize| -> Option<f32> { Some(f32::from(i16_at(bytes, i)?) / 16384.0) };
    let mut matrix = [1.0, 0.0, 0.0, 1.0];
    if flags & WE_HAVE_A_SCALE != 0 {
        let s = f2dot14(p)?;
        matrix = [s, 0.0, 0.0, s];
    } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
        matrix = [f2dot14(p)?, 0.0, 0.0, f2dot14(p.checked_add(2)?)?];
    } else if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
        matrix = [
            f2dot14(p)?,
            f2dot14(p.checked_add(2)?)?,
            f2dot14(p.checked_add(4)?)?,
            f2dot14(p.checked_add(6)?)?,
        ];
    }
    Some(Record {
        flags,
        gid,
        tx: whole(tx),
        ty: whole(ty),
        anchor,
        matrix,
    })
}

/// A simple glyph's contour points, appended to `all`: HarfBuzz's
/// `SimpleGlyph::get_contour_points`, `read_flags` and `read_points`.
///
/// Each contour's last point is flagged; a coordinate is the running sum of
/// its deltas in 32 bits, as HarfBuzz sums it.
fn read_contour_points(
    bytes: &[u8],
    contours: usize,
    all: &mut Vec<ContourPoint>,
) -> Result<(), SfntError> {
    const BAD: SfntError = SfntError::MalformedTable("glyf");
    // The contours' last points, then the instructions' length.
    let end_pt = |i: usize| -> Option<usize> {
        let at = i.checked_mul(2)?.checked_add(10)?;
        u16_at(bytes, at).map(usize::from)
    };
    let instructions = end_pt(contours).ok_or(BAD)?;
    let last = end_pt(contours.checked_sub(1).ok_or(BAD)?).ok_or(BAD)?;
    let count = last.checked_add(1).ok_or(BAD)?;
    if count < contours {
        return Err(BAD);
    }
    let base = all.len();
    all.resize(base.checked_add(count).ok_or(BAD)?, ContourPoint::default());
    let points = all.get_mut(base..).ok_or(BAD)?;
    for i in 0..contours {
        if let Some(p) = end_pt(i).and_then(|e| points.get_mut(e)) {
            p.is_end_point = true;
        }
    }
    let mut p = contours
        .checked_add(1)
        .and_then(|n| n.checked_mul(2))
        .and_then(|n| n.checked_add(10))
        .and_then(|n| n.checked_add(instructions))
        .ok_or(BAD)?;
    if p >= bytes.len() {
        return Err(BAD);
    }
    read_flags(bytes, &mut p, points).ok_or(BAD)?;
    read_coords(bytes, &mut p, points, FLAG_X_SHORT, FLAG_X_SAME, true).ok_or(BAD)?;
    read_coords(bytes, &mut p, points, FLAG_Y_SHORT, FLAG_Y_SAME, false).ok_or(BAD)?;
    Ok(())
}

/// The run-length-coded flags, one per point.
fn read_flags(bytes: &[u8], p: &mut usize, points: &mut [ContourPoint]) -> Option<()> {
    let mut i = 0usize;
    while i < points.len() {
        let flag = *bytes.get(*p)?;
        *p = p.checked_add(1)?;
        points.get_mut(i)?.flag = flag;
        i = i.checked_add(1)?;
        if flag & FLAG_REPEAT != 0 {
            let repeat = usize::from(*bytes.get(*p)?);
            *p = p.checked_add(1)?;
            let stop = i.saturating_add(repeat).min(points.len());
            for point in points.get_mut(i..stop)? {
                point.flag = flag;
            }
            i = stop;
        }
    }
    Some(())
}

/// One axis's coordinates, each the previous one plus a delta.
fn read_coords(
    bytes: &[u8],
    p: &mut usize,
    points: &mut [ContourPoint],
    short: u8,
    same: u8,
    x: bool,
) -> Option<()> {
    let mut v = 0i32;
    for point in points {
        let flag = point.flag;
        if flag & short != 0 {
            let d = i32::from(*bytes.get(*p)?);
            *p = p.checked_add(1)?;
            v = if flag & same != 0 {
                v.wrapping_add(d)
            } else {
                v.wrapping_sub(d)
            };
        } else if flag & same == 0 {
            v = v.wrapping_add(i32::from(i16_at(bytes, *p)?));
            *p = p.checked_add(2)?;
        }
        if x {
            point.x = whole(v);
        } else {
            point.y = whole(v);
        }
    }
    Some(())
}

/// HarfBuzz's cycle detector (`hb_decycler_t`), one node per composite being
/// expanded: each node remembers the component it is visiting, and a
/// component is dropped when it is the one a trailing node -- the tortoise,
/// which moves one node for every two the chain grows -- is visiting.
///
/// `COLR`'s extents walk (`crate::colr::extents`) keeps two more, as
/// HarfBuzz's paint context does: one over the colour glyphs a
/// `PaintColrGlyph` names, one over the layers a `PaintColrLayers` visits.
#[derive(Default)]
pub(crate) struct Decycler {
    /// Toggled as nodes come and go; the tortoise moves when it is set.
    awake: bool,
    /// The trailing node's depth.
    tortoise: Option<usize>,
    /// Each node's current component.
    values: Vec<u32>,
}

impl Decycler {
    /// A new node, one deeper: `hb_decycler_node_t`'s constructor.
    pub(crate) fn enter(&mut self) -> usize {
        self.awake = !self.awake;
        let node = self.values.len();
        self.values.push(0);
        match self.tortoise {
            None => self.tortoise = Some(node),
            Some(t) if self.awake => self.tortoise = Some(t.saturating_add(1)),
            Some(_) => {}
        }
        node
    }

    /// The deepest node goes: the destructor.
    pub(crate) fn leave(&mut self) {
        self.values.pop();
        if self.awake {
            self.tortoise = self.tortoise.and_then(|t| t.checked_sub(1));
        }
        self.awake = !self.awake;
    }

    /// Whether `node` may visit `gid`: not if the tortoise, another node, is
    /// visiting it.
    pub(crate) fn visit(&mut self, node: usize, gid: u32) -> bool {
        if let Some(v) = self.values.get_mut(node) {
            *v = gid;
        }
        match self.tortoise {
            Some(t) if t != node => self.values.get(t) != Some(&gid),
            _ => true,
        }
    }
}

/// Walk contour points into `out` as HarfBuzz's `glyf_accelerator_t::
/// get_points` feeds them to its `path_builder_t`: a contour that starts on
/// the curve from its first point, one that starts off it from its last.
fn draw(points: &[ContourPoint], out: &mut Outline) {
    let mut pen = PathBuilder::new(out);
    let count = points.len();
    let mut i = 0usize;
    while i < count {
        let Some(first) = points.get(i) else {
            break;
        };
        if first.flag & FLAG_ON_CURVE != 0 {
            while let Some(p) = points.get(i) {
                pen.consume_point(p);
                if p.is_end_point {
                    pen.contour_end();
                    break;
                }
                i = i.saturating_add(1);
            }
        } else {
            let start = i;
            while points.get(i).is_some_and(|p| !p.is_end_point) {
                i = i.saturating_add(1);
            }
            if let Some(end) = points.get(i) {
                pen.consume_point(end);
            }
            for p in points.get(start..i).unwrap_or_default() {
                pen.consume_point(p);
            }
            pen.contour_end();
        }
        i = i.saturating_add(1);
    }
    // The session's end closes whatever is open, as its destructor does.
    pen.session.close_path();
}

/// HarfBuzz's `path_builder_t`, which turns `glyf` points into quadratic
/// (and cubic) segments: two control points in a row imply an on-curve point
/// halfway between them, `(a + b) * 0.5`.
struct PathBuilder<'o> {
    session: Session<'o>,
    first_oncurve: Option<Point>,
    first_offcurve: Option<Point>,
    first_offcurve2: Option<Point>,
    last_offcurve: Option<Point>,
    last_offcurve2: Option<Point>,
}

/// `optional_point_t::mid`.
fn mid(a: Point, b: Point) -> Point {
    Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
}

impl<'o> PathBuilder<'o> {
    fn new(out: &'o mut Outline) -> Self {
        Self {
            session: Session::new(out),
            first_oncurve: None,
            first_offcurve: None,
            first_offcurve2: None,
            last_offcurve: None,
            last_offcurve2: None,
        }
    }

    fn consume_point(&mut self, point: &ContourPoint) {
        let on_curve = point.flag & FLAG_ON_CURVE != 0;
        let cubic = !on_curve && point.flag & FLAG_CUBIC != 0;
        let p = Point::new(point.x, point.y);
        let Some(_) = self.first_oncurve else {
            if on_curve {
                self.first_oncurve = Some(p);
                self.session.move_to(p);
            } else if cubic && self.first_offcurve2.is_none() {
                self.first_offcurve2 = self.first_offcurve;
                self.first_offcurve = Some(p);
            } else if let Some(first) = self.first_offcurve {
                let m = mid(first, p);
                self.first_oncurve = Some(m);
                self.last_offcurve = Some(p);
                self.session.move_to(m);
            } else {
                self.first_offcurve = Some(p);
            }
            return;
        };
        match self.last_offcurve {
            Some(last) if on_curve => {
                if let Some(last2) = self.last_offcurve2.take() {
                    self.session.cubic_to(last2, last, p);
                } else {
                    self.session.quadratic_to(last, p);
                }
                self.last_offcurve = None;
            }
            Some(last) => {
                if cubic && self.last_offcurve2.is_none() {
                    self.last_offcurve2 = Some(last);
                    self.last_offcurve = Some(p);
                } else {
                    let m = mid(last, p);
                    if cubic {
                        if let Some(last2) = self.last_offcurve2.take() {
                            self.session.cubic_to(last2, last, m);
                        }
                    } else {
                        self.session.quadratic_to(last, m);
                    }
                    self.last_offcurve = Some(p);
                }
            }
            None if on_curve => self.session.line_to(p),
            None => self.last_offcurve = Some(p),
        }
    }

    fn contour_end(&mut self) {
        if let (Some(first), Some(last)) = (self.first_offcurve, self.last_offcurve) {
            let m = mid(last, self.first_offcurve2.unwrap_or(first));
            if let Some(last2) = self.last_offcurve2.take() {
                self.session.cubic_to(last2, last, m);
            } else {
                self.session.quadratic_to(last, m);
            }
            self.last_offcurve = None;
        }
        match (self.first_offcurve, self.last_offcurve, self.first_oncurve) {
            (Some(first), _, Some(on)) => {
                if let Some(first2) = self.first_offcurve2 {
                    self.session.cubic_to(first2, first, on);
                } else {
                    self.session.quadratic_to(first, on);
                }
            }
            (None, Some(last), Some(on)) => {
                if let Some(last2) = self.last_offcurve2 {
                    self.session.cubic_to(last2, last, on);
                } else {
                    self.session.quadratic_to(last, on);
                }
            }
            // Back to the first on-curve point with a line -- even when the
            // pen is already there, as it is when a contour's last point
            // repeats its first: HarfBuzz draws that line, of no length.
            (None, None, Some(on)) => self.session.line_to(on),
            // A contour of one control point: a curve of no length at it.
            (Some(first), _, None) => {
                self.session.move_to(first);
                self.session.quadratic_to(first, first);
            }
            (None, _, None) => {}
        }
        // Ready for the next contour -- all but `first_offcurve2`, which
        // HarfBuzz does not clear, so a cubic contour's can reach the next.
        self.first_oncurve = None;
        self.first_offcurve = None;
        self.last_offcurve = None;
        self.last_offcurve2 = None;
        self.session.close_path();
    }
}

/// HarfBuzz's `hb_draw_session_t`, as far as a glyph needs it: a contour's
/// first point is not emitted until a segment follows it, and closing one
/// that has not come back to its start draws the line that does.
struct Session<'o> {
    out: &'o mut Outline,
    open: bool,
    start: Point,
    current: Point,
}

impl<'o> Session<'o> {
    fn new(out: &'o mut Outline) -> Self {
        Self {
            out,
            open: false,
            start: Point::default(),
            current: Point::default(),
        }
    }

    fn move_to(&mut self, p: Point) {
        if self.open {
            self.close_path();
        }
        self.current = p;
    }

    fn start_path(&mut self) {
        if !self.open {
            self.out.commands.push(PathCmd::MoveTo(self.current));
            self.open = true;
            self.start = self.current;
        }
    }

    fn line_to(&mut self, p: Point) {
        self.start_path();
        self.out.commands.push(PathCmd::LineTo(p));
        self.current = p;
    }

    fn quadratic_to(&mut self, c: Point, p: Point) {
        self.start_path();
        self.out.commands.push(PathCmd::QuadTo(c, p));
        self.current = p;
    }

    fn cubic_to(&mut self, c1: Point, c2: Point, p: Point) {
        self.start_path();
        self.out.commands.push(PathCmd::CurveTo(c1, c2, p));
        self.current = p;
    }

    #[allow(
        clippy::float_cmp,
        reason = "HarfBuzz's own test: whether the pen is back where the \
                  contour began, exactly"
    )]
    fn close_path(&mut self) {
        if self.open {
            if self.start.x != self.current.x || self.start.y != self.current.y {
                self.out.commands.push(PathCmd::LineTo(self.start));
            }
            self.out.commands.push(PathCmd::Close);
        }
        self.open = false;
        self.start = Point::default();
        self.current = Point::default();
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "a failed test should fail at the line that did it; coordinates \
              are compared exactly because the last bit is the point"
)]
mod tests {
    use super::*;
    use alloc::string::String;

    /// Every glyph of the fixture's variable TrueType face, drawn and
    /// measured at `weight`, against HarfBuzz's drawing of it.
    fn check_drawn(weight: f32, rows: &[crate::hint::fixture::Drawn]) {
        let face = Face::parse(crate::hint::fixture::VAR.to_vec()).unwrap();
        let coords = face
            .variation_axes()
            .unwrap()
            .normalize_tags(&[(*b"wght", weight)]);
        let mut wrong = Vec::new();
        for &(gid, name, extents, ops, points) in rows {
            let got = face.glyph_extents_at(gid, &coords).unwrap();
            if got != extents {
                wrong.push(alloc::format!("{name}: box {got:?}, HarfBuzz {extents:?}"));
            }
            let outline = face.outline_at(gid, &coords).unwrap();
            let (got_ops, got_points) = crate::cff::tests::spelled(&outline.commands);
            if got_ops != ops || got_points != points {
                wrong.push(alloc::format!(
                    "{name}: path {got_ops} {got_points:?}, HarfBuzz {ops} {points:?}"
                ));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// Every glyph's advance in `face` at `weight`, against HarfBuzz's.
    fn check_advances(face: &[u8], weight: f32, want: &[u16]) {
        let face = Face::parse(face.to_vec()).unwrap();
        let coords = face
            .variation_axes()
            .unwrap()
            .normalize_tags(&[(*b"wght", weight)]);
        let got: Vec<u16> = (0..u16::try_from(want.len()).unwrap())
            .map(|gid| face.advance_at(gid, &coords).unwrap())
            .collect();
        assert_eq!(got, want, "weight {weight}");
    }

    #[test]
    fn a_face_without_hvar_advances_between_its_phantom_points() {
        use crate::hint::fixture::{VAR_NOHVAR, VAR_NOHVAR_ADVANCES_401, VAR_NOHVAR_ADVANCES_610};
        check_advances(&VAR_NOHVAR, 610.0, &VAR_NOHVAR_ADVANCES_610);
        check_advances(&VAR_NOHVAR, 401.0, &VAR_NOHVAR_ADVANCES_401);
    }

    #[test]
    fn a_face_with_hvar_advances_as_harfbuzz_reads_it() {
        use crate::hint::fixture::{VAR, VAR_ADVANCES_401, VAR_ADVANCES_610};
        check_advances(&VAR, 610.0, &VAR_ADVANCES_610);
        check_advances(&VAR, 401.0, &VAR_ADVANCES_401);
    }

    #[test]
    fn a_variable_face_is_drawn_and_measured_as_harfbuzz_draws_it() {
        // Weight 610: every delta a fraction, IUP and a varied component
        // offset among them.
        check_drawn(610.0, &crate::hint::fixture::VAR_DRAWN);
    }

    #[test]
    fn a_small_scalar_is_drawn_as_harfbuzz_caches_it() {
        // Weight 401: a scalar of 55/16384, under 2^-6, which HarfBuzz's
        // cache hands back rounded to 2^-30ths.
        check_drawn(401.0, &crate::hint::fixture::VAR_DRAWN_401);
    }

    fn on(x: f32, y: f32) -> ContourPoint {
        ContourPoint {
            x,
            y,
            flag: FLAG_ON_CURVE,
            is_end_point: false,
        }
    }

    fn off(x: f32, y: f32) -> ContourPoint {
        ContourPoint {
            x,
            y,
            flag: 0,
            is_end_point: false,
        }
    }

    fn cubic(x: f32, y: f32) -> ContourPoint {
        ContourPoint {
            flag: FLAG_CUBIC,
            ..off(x, y)
        }
    }

    fn end(mut p: ContourPoint) -> ContourPoint {
        p.is_end_point = true;
        p
    }

    fn drawn(points: &[ContourPoint]) -> (String, Vec<f32>) {
        let mut out = Outline::default();
        draw(points, &mut out);
        crate::cff::tests::spelled(&out.commands)
    }

    #[test]
    fn a_contour_that_starts_off_the_curve_starts_at_its_last_point() {
        // FreeType's and HarfBuzz's start: the last point, when it is on
        // the curve -- not the first on-curve point.
        let (ops, pts) = drawn(&[
            off(0.0, 10.0),
            on(10.0, 10.0),
            on(10.0, 0.0),
            end(on(0.0, 0.0)),
        ]);
        // The closing line back to (0, 0) is not spelled.
        assert_eq!(ops, "MQLZ");
        assert_eq!(pts[..2], [0.0, 0.0]);
    }

    #[test]
    fn a_contour_of_control_points_starts_halfway_between_its_last_and_first() {
        let (ops, pts) = drawn(&[
            off(0.0, 0.0),
            off(10.0, 0.0),
            off(10.0, 10.0),
            end(off(0.0, 10.0)),
        ]);
        assert_eq!(ops, "MQQQQZ");
        assert_eq!(pts[..2], [0.0, 5.0]);
    }

    #[test]
    fn a_halfway_point_is_the_sum_halved() {
        // (a + b) * 0.5 with the sum rounded to `f32` first: 3 + 4 halves to
        // 3.5; and 16777216 + 3 rounds to 16777220 before halving, where the
        // exact midpoint 8388609.5 would round to 8388610 -- the same here,
        // since halving is exact; the order matters once a shift or a
        // transform comes between, which is why the points are shifted
        // before the path is made.
        let (_, pts) = drawn(&[
            on(0.0, 0.0),
            off(3.0, 1.0),
            off(4.0, 1.0),
            end(on(9.0, 0.0)),
        ]);
        assert_eq!(pts[4..6], [3.5, 1.0]);
    }

    #[test]
    fn two_cubic_controls_make_a_cubic() {
        let (ops, _) = drawn(&[
            on(0.0, 0.0),
            cubic(1.0, 1.0),
            cubic(2.0, 1.0),
            end(on(3.0, 0.0)),
        ]);
        assert_eq!(ops, "MCZ");
    }

    #[test]
    fn a_cubic_contours_first_control_reaches_the_next_contour() {
        // HarfBuzz does not clear `first_offcurve2` between contours. The
        // first contour, starting and ending off the curve on cubic
        // controls, leaves (2, 2) there; the second, all quadratic controls,
        // then closes through it: halfway between its last control and
        // (2, 2), and a cubic through (2, 2) back to its start -- where
        // (20, 10) to (15, 10) and a quadratic would be the glyph's own.
        let (ops, pts) = drawn(&[
            cubic(0.0, 0.0),
            on(5.0, 5.0),
            cubic(1.0, 1.0),
            end(cubic(2.0, 2.0)),
            off(10.0, 0.0),
            off(20.0, 0.0),
            off(20.0, 10.0),
            end(off(10.0, 10.0)),
        ]);
        assert_eq!(ops, "MQCZMQQQCZ");
        let n = pts.len();
        assert_eq!(pts[n - 10..n - 6], [20.0, 10.0, 11.0, 6.0]);
        assert_eq!(pts[n - 6..], [2.0, 2.0, 10.0, 10.0, 10.0, 5.0]);
    }

    #[test]
    fn a_contour_ends_with_a_line_back_to_its_first_point() {
        // Even when its last point repeats its first, so that the line has
        // no length: HarfBuzz draws it (`path_builder_t::contour_end`), and
        // the session then has nothing left to close.
        let p = Point::new;
        let mut out = Outline::default();
        draw(&[on(0.0, 0.0), on(5.0, 0.0), end(on(0.0, 0.0))], &mut out);
        assert_eq!(
            out.commands,
            [
                PathCmd::MoveTo(p(0.0, 0.0)),
                PathCmd::LineTo(p(5.0, 0.0)),
                PathCmd::LineTo(p(0.0, 0.0)),
                PathCmd::LineTo(p(0.0, 0.0)),
                PathCmd::Close,
            ]
        );
        let mut out = Outline::default();
        draw(&[on(0.0, 0.0), end(on(5.0, 0.0))], &mut out);
        assert_eq!(
            out.commands,
            [
                PathCmd::MoveTo(p(0.0, 0.0)),
                PathCmd::LineTo(p(5.0, 0.0)),
                PathCmd::LineTo(p(0.0, 0.0)),
                PathCmd::Close,
            ]
        );
    }

    #[test]
    fn a_lone_point_draws_a_line_to_itself() {
        let p = Point::new(1.0, 1.0);
        let mut out = Outline::default();
        draw(&[end(on(1.0, 1.0))], &mut out);
        assert_eq!(
            out.commands,
            [PathCmd::MoveTo(p), PathCmd::LineTo(p), PathCmd::Close]
        );
    }

    #[test]
    fn a_lone_control_point_draws_a_curve_to_itself() {
        let p = Point::new(1.0, 1.0);
        let mut out = Outline::default();
        draw(&[end(off(1.0, 1.0))], &mut out);
        assert_eq!(
            out.commands,
            [PathCmd::MoveTo(p), PathCmd::QuadTo(p, p), PathCmd::Close]
        );
    }

    #[test]
    fn a_transform_rounds_each_product() {
        // Each product rounded to `f32` and then the sum, as C computes
        // `x * a + y * c` without contraction.
        let (a, c) = (0.1f32, 0.2f32);
        let mut p = ContourPoint::at(3.0, 7.0);
        p.transform(&[a, 0.0, c, 1.0]);
        let products = 3.0f32 * a + 7.0f32 * c;
        assert_eq!(p.x, products);
        assert_eq!(p.y, 7.0);
    }

    #[test]
    fn a_zero_offset_leaves_a_negative_zero() {
        let mut pts = [ContourPoint::at(-0.0, -0.0)];
        translate(&mut pts, 0.0, 0.0);
        assert!(pts[0].x.is_sign_negative() && pts[0].y.is_sign_negative());
        translate(&mut pts, 1.0, 0.0);
        assert_eq!(pts[0].x, 1.0);
        assert!(pts[0].y.is_sign_negative());
    }

    #[test]
    fn the_decycler_catches_a_cycle_a_level_late() {
        // Glyph A's component is A itself: the first node, which is its own
        // tortoise, visits it; the second, whose tortoise is still the
        // first, sees A there and drops it.
        let mut d = Decycler::default();
        let n0 = d.enter();
        assert!(d.visit(n0, 7));
        let n1 = d.enter();
        assert!(!d.visit(n1, 7));
        assert!(d.visit(n1, 8));
        d.leave();
        d.leave();
        assert!(d.values.is_empty());
        assert_eq!(d.tortoise, None);
        assert!(!d.awake);
    }

    #[test]
    fn the_tortoise_moves_one_node_for_every_two() {
        let mut d = Decycler::default();
        let nodes: Vec<usize> = (0..5).map(|_| d.enter()).collect();
        assert_eq!(nodes, [0, 1, 2, 3, 4]);
        // The first node puts it at 0; the third and fifth move it on one.
        assert_eq!(d.tortoise, Some(2));
        for _ in 0..5 {
            d.leave();
        }
        assert_eq!(d.tortoise, None);
    }
}
