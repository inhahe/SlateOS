//! A colour glyph's box, measured as HarfBuzz measures it.
//!
//! HarfBuzz asks `COLR` for a glyph's extents before any outline table
//! (`hb_ot_get_glyph_extents`), and `COLR` answers in one of two ways
//! (`COLR::get_extents`):
//!
//! * a glyph the `ClipList` covers reports its **clip box** -- each corner's
//!   variation delta rounded before it is added -- and nothing else is
//!   looked at;
//! * any other glyph with a colour recipe has its **paint measured**
//!   (`hb_paint_extents`): the recipe is walked with a stack of transforms,
//!   a stack of clips and a stack of groups, and every fill adds the clip it
//!   is drawn under to the group it is drawn into. A clip glyph's box is the
//!   box around its drawn path -- every point HarfBuzz's draw callbacks are
//!   handed, control points included -- carried through the transform it is
//!   drawn under; a composite combines its two groups as its mode does (a
//!   `SRC_IN` keeps only where both are, a `CLEAR` keeps nothing, and so on).
//!   A version-1 recipe is measured only if a first walk (`hb_paint_bounded`)
//!   finds that everything it fills is under some clip -- a fill that is not
//!   covers the whole plane, and HarfBuzz reports such a glyph as an empty
//!   box -- and a version-0 recipe is its layers, each a glyph filled.
//!
//! [`glyph_extents`] is that, for [`Face::glyph_extents_at`], which the
//! fallback mark placement reads. The box is not the renderer's: the
//! renderer asks what a glyph covers at a size, in pixels, and bounds its
//! canvas by it; this asks what HarfBuzz would say, to the unit, and so
//! repeats HarfBuzz's arithmetic step for step -- `float` throughout, the
//! products and sums in HarfBuzz's order, every corner rounded half away
//! from zero at the end (`hb_extents_t::to_glyph_extents`) -- including
//! where it is odd: a clip glyph that draws nothing still clips to a 1-by-1
//! box, because transforming the void box turns it into one.
//!
//! The walk stops where HarfBuzz's does -- 64 levels deep, 2,048 edges in
//! all, and at a `PaintColrGlyph` or layer that its cycle detectors
//! (`hb_decycler_t`, [`Decycler`]) catch -- and treats a paint record that
//! is null, points outside the table or is cut short as HarfBuzz's
//! sanitizer leaves such a record: a null paint, which paints nothing but
//! still counts as an edge.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use super::{Tables, at_offset, i16_at, u8_at, u16_at, u24_at, u32_at};
use crate::glyf::Decycler;
use crate::sfnt::{Face, PathCmd};
use crate::var::Coords;

/// The box HarfBuzz reports for glyph `gid` of `face` at `coords` from
/// `COLR`: its clip box if the `ClipList` gives it one, else its measured
/// paint -- `[0; 4]` when that is unbounded or paints nothing. Left edge,
/// top edge, width rightwards, height downwards, in font units.
///
/// `None` when `COLR` has neither for the glyph (or the face has no `COLR`):
/// HarfBuzz then measures its outline.
#[must_use]
pub(crate) fn glyph_extents(face: &Face, gid: u16, coords: &Coords) -> Option<[i32; 4]> {
    let tables = Tables::of(face, coords)?;
    if let Some([x_min, y_min, x_max, y_max]) = tables.clip_corners(gid) {
        // `COLR::get_extents` hands the clip box to `scale_glyph_extents`,
        // whose edges pass through `int16_t`.
        return Some(crate::hbcalc::scale_glyph_extents_at_upem([
            x_min,
            y_max,
            x_max.wrapping_sub(x_min),
            y_min.wrapping_sub(y_max),
        ]));
    }
    let mut sink = ExtentsSink::new();
    if let Some(paint) = tables.base_paint(gid) {
        // `COLR::paint_glyph` with no clip box: a first walk says whether
        // the graph is bounded at all, and only then does the second
        // measure it.
        let mut first = Walk::new(&tables, face, coords, BoundedSink::new());
        first.root(gid, paint.at(), true);
        let bounded = first.sink.bounded;
        let mut walk = Walk::new(&tables, face, coords, sink);
        walk.root(gid, paint.at(), bounded);
        sink = walk.sink;
    } else {
        // Version 0: each layer is `fill_glyph`, which HarfBuzz's default
        // decomposes into a clip, a fill and the clip's end.
        let layers = tables.base_v0(gid)?;
        for (layer, _) in tables.layers_v0(layers) {
            sink.push_clip_glyph(face, layer, coords);
            sink.paint();
            sink.pop_clip();
        }
    }
    let extents = sink.extents();
    Some(if extents.is_void() {
        [0; 4]
    } else {
        extents.to_glyph_extents()
    })
}

// ---------------------------------------------------------------------------
// HarfBuzz's geometry, in `float`
// ---------------------------------------------------------------------------

/// `hb_min`: the first when the two tie, the second when either is not a
/// number.
fn hb_min(a: f32, b: f32) -> f32 {
    if a <= b { a } else { b }
}

/// `hb_max`, likewise.
fn hb_max(a: f32, b: f32) -> f32 {
    if a >= b { a } else { b }
}

/// `hb_extents_t<float>`: a box, *void* -- nothing in it yet -- while its
/// left edge is right of its right, and *empty* -- no area -- while either
/// pair of edges has not crossed.
#[derive(Clone, Copy, Debug, PartialEq)]
struct HbExtents {
    x_min: f32,
    y_min: f32,
    x_max: f32,
    y_max: f32,
}

impl HbExtents {
    /// The default box, which is void.
    const VOID: Self = Self {
        x_min: 0.0,
        y_min: 0.0,
        x_max: -1.0,
        y_max: -1.0,
    };

    fn is_empty(&self) -> bool {
        self.x_min >= self.x_max || self.y_min >= self.y_max
    }

    fn is_void(&self) -> bool {
        self.x_min > self.x_max
    }

    fn union(&mut self, o: &Self) {
        if o.is_empty() {
            return;
        }
        if self.is_empty() {
            *self = *o;
            return;
        }
        self.x_min = hb_min(self.x_min, o.x_min);
        self.y_min = hb_min(self.y_min, o.y_min);
        self.x_max = hb_max(self.x_max, o.x_max);
        self.y_max = hb_max(self.y_max, o.y_max);
    }

    fn intersect(&mut self, o: &Self) {
        if o.is_empty() || self.is_empty() {
            *self = Self::VOID;
            return;
        }
        self.x_min = hb_max(self.x_min, o.x_min);
        self.y_min = hb_max(self.y_min, o.y_min);
        self.x_max = hb_min(self.x_max, o.x_max);
        self.y_max = hb_min(self.y_max, o.y_max);
    }

    fn add_point(&mut self, x: f32, y: f32) {
        if self.is_void() {
            *self = Self {
                x_min: x,
                y_min: y,
                x_max: x,
                y_max: y,
            };
        } else {
            self.x_min = hb_min(self.x_min, x);
            self.y_min = hb_min(self.y_min, y);
            self.x_max = hb_max(self.x_max, x);
            self.y_max = hb_max(self.y_max, y);
        }
    }

    /// `to_glyph_extents`: each edge rounded half away from zero, in
    /// `double`, then the left edge, the top edge, the width and the
    /// (negative) height, each clamped into an `hb_position_t` -- all
    /// zeros if any edge is not a finite number.
    fn to_glyph_extents(self) -> [i32; 4] {
        let [x0, y0, x1, y1] =
            [self.x_min, self.y_min, self.x_max, self.y_max].map(|v| f64::from(v).round());
        if ![x0, y0, x1, y1].iter().all(|v| v.is_finite()) {
            return [0; 4];
        }
        [x0, y1, x1 - x0, y0 - y1].map(|v| {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a whole number clamped into i32's range, as clamp_to_hb_position casts it"
            )]
            let whole = v.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
            whole
        })
    }
}

/// `hb_transform_t<float>`: `x' = x0 + xx x + xy y`, `y' = y0 + yx x + yy y`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct HbTransform {
    xx: f32,
    yx: f32,
    xy: f32,
    yy: f32,
    x0: f32,
    y0: f32,
}

/// `HB_PI`: the `float` HarfBuzz turns half-turns into radians with.
const HB_PI: f32 = core::f32::consts::PI;

impl HbTransform {
    /// Also the font's transform, and its inverse, at one unit per font
    /// unit -- the scale glyph extents are measured at.
    const IDENTITY: Self = Self {
        xx: 1.0,
        yx: 0.0,
        xy: 0.0,
        yy: 1.0,
        x0: 0.0,
        y0: 0.0,
    };

    /// HarfBuzz's null `Affine2x3`: every field zero.
    const ZERO: Self = Self {
        xx: 0.0,
        yx: 0.0,
        xy: 0.0,
        yy: 0.0,
        x0: 0.0,
        y0: 0.0,
    };

    /// `multiply(o)`: `self` after `o`, as cairo multiplies.
    fn multiply(&mut self, o: &Self) {
        let (a, b) = (*self, o);
        *self = Self {
            xx: a.xx * b.xx + a.xy * b.yx,
            yx: a.yx * b.xx + a.yy * b.yx,
            xy: a.xx * b.xy + a.xy * b.yy,
            yy: a.yx * b.xy + a.yy * b.yy,
            x0: a.xx * b.x0 + a.xy * b.y0 + a.x0,
            y0: a.yx * b.x0 + a.yy * b.y0 + a.y0,
        };
    }

    fn transform_point(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.x0 + self.xx * x + self.xy * y,
            self.y0 + self.yx * x + self.yy * y,
        )
    }

    /// `transform_extents`: the box around `e`'s four corners, moved --
    /// which makes even the void box a real one, one unit square.
    fn transform_extents(&self, e: &HbExtents) -> HbExtents {
        let mut out = HbExtents::VOID;
        for (x, y) in [
            (e.x_min, e.y_min),
            (e.x_min, e.y_max),
            (e.x_max, e.y_min),
            (e.x_max, e.y_max),
        ] {
            let (x, y) = self.transform_point(x, y);
            out.add_point(x, y);
        }
        out
    }

    fn translation(x: f32, y: f32) -> Self {
        Self {
            x0: x,
            y0: y,
            ..Self::IDENTITY
        }
    }

    fn scaling(sx: f32, sy: f32) -> Self {
        Self {
            xx: sx,
            yy: sy,
            ..Self::IDENTITY
        }
    }

    /// `scaling_around_center`: a centre at 0 moves nothing, even times a
    /// factor that is not a number.
    #[allow(
        clippy::float_cmp,
        reason = "HarfBuzz's own test of a centre against 0"
    )]
    fn scaling_around_center(sx: f32, sy: f32, cx: f32, cy: f32) -> Self {
        Self {
            x0: if cx == 0.0 { 0.0 } else { (1.0 - sx) * cx },
            y0: if cy == 0.0 { 0.0 } else { (1.0 - sy) * cy },
            ..Self::scaling(sx, sy)
        }
    }

    fn rotation(radians: f32) -> Self {
        let (s, c) = radians.sin_cos();
        Self {
            xx: c,
            yx: s,
            xy: -s,
            yy: c,
            x0: 0.0,
            y0: 0.0,
        }
    }

    fn rotation_around_center(radians: f32, cx: f32, cy: f32) -> Self {
        let (s, c) = radians.sin_cos();
        Self {
            xx: c,
            yx: s,
            xy: -s,
            yy: c,
            x0: (1.0 - c) * cx + s * cy,
            y0: -s * cx + (1.0 - c) * cy,
        }
    }

    /// `tanf` of a skew angle, and no skew for none.
    #[allow(
        clippy::float_cmp,
        reason = "HarfBuzz's own test of an angle against 0"
    )]
    fn skew_tan(radians: f32) -> f32 {
        if radians == 0.0 { 0.0 } else { radians.tan() }
    }

    fn skewing(skew_x: f32, skew_y: f32) -> Self {
        Self {
            yx: Self::skew_tan(skew_y),
            xy: Self::skew_tan(skew_x),
            ..Self::IDENTITY
        }
    }

    #[allow(
        clippy::float_cmp,
        reason = "HarfBuzz's own test of a centre against 0"
    )]
    fn skewing_around_center(skew_x: f32, skew_y: f32, cx: f32, cy: f32) -> Self {
        let (tx, ty) = (Self::skew_tan(skew_x), Self::skew_tan(skew_y));
        Self {
            xx: 1.0,
            yx: ty,
            xy: tx,
            yy: 1.0,
            x0: if cy == 0.0 { 0.0 } else { -tx * cy },
            y0: if cx == 0.0 { 0.0 } else { -ty * cx },
        }
    }
}

/// `hb_bounds_t<float>`'s status: a box, nothing at all, or everything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Unbounded,
    Bounded,
    Empty,
}

/// `hb_bounds_t<float>`. The box is read only while the status is
/// [`Status::Bounded`]; another status may leave a stale one behind, as
/// HarfBuzz's does.
#[derive(Clone, Copy, Debug, PartialEq)]
struct HbBounds {
    status: Status,
    extents: HbExtents,
}

impl HbBounds {
    const fn of(status: Status) -> Self {
        Self {
            status,
            extents: HbExtents::VOID,
        }
    }

    fn from_extents(extents: HbExtents) -> Self {
        Self {
            status: if extents.is_empty() {
                Status::Empty
            } else {
                Status::Bounded
            },
            extents,
        }
    }

    fn union(&mut self, o: &Self) {
        match (o.status, self.status) {
            (Status::Unbounded, _) => self.status = Status::Unbounded,
            (Status::Bounded, Status::Empty) => *self = *o,
            (Status::Bounded, Status::Bounded) => self.extents.union(&o.extents),
            _ => {}
        }
    }

    fn intersect(&mut self, o: &Self) {
        match (o.status, self.status) {
            (Status::Empty, _) => self.status = Status::Empty,
            (Status::Bounded, Status::Unbounded) => *self = *o,
            (Status::Bounded, Status::Bounded) => {
                self.extents.intersect(&o.extents);
                if self.extents.is_empty() {
                    self.status = Status::Empty;
                }
            }
            _ => {}
        }
    }
}

/// Compositing modes, numbered as `COLR` and `hb_paint_composite_mode_t`
/// number them -- the ones the measurements tell apart.
mod mode {
    pub(super) const CLEAR: u8 = 0;
    pub(super) const SRC: u8 = 1;
    pub(super) const DEST: u8 = 2;
    pub(super) const SRC_OVER: u8 = 3;
    pub(super) const SRC_IN: u8 = 5;
    pub(super) const DEST_IN: u8 = 6;
    pub(super) const SRC_OUT: u8 = 7;
    pub(super) const DEST_OUT: u8 = 8;
    /// `HSL_LUMINOSITY`, the last there is: `PaintComposite` takes any
    /// later number for `CLEAR`.
    pub(super) const LAST: u8 = 27;
}

// ---------------------------------------------------------------------------
// The two measurements
// ---------------------------------------------------------------------------

/// The paint callbacks HarfBuzz's two measurements implement -- the ones a
/// walk of a paint graph makes that either of them hears. (Colours,
/// gradients and images are all just [`paint`](Sink::paint) to both.)
trait Sink {
    fn push_transform(&mut self, t: &HbTransform);
    fn pop_transform(&mut self);
    fn push_clip_glyph(&mut self, face: &Face, gid: u16, coords: &Coords);
    fn push_clip_rectangle(&mut self, e: HbExtents);
    fn pop_clip(&mut self);
    fn push_group(&mut self);
    fn pop_group(&mut self, mode: u8);
    fn paint(&mut self);
}

/// `hb_paint_bounded_context_t`: whether everything painted is under some
/// clip.
struct BoundedSink {
    bounded: bool,
    clips: u32,
    groups: Vec<bool>,
}

impl BoundedSink {
    fn new() -> Self {
        Self {
            bounded: true,
            clips: 0,
            groups: Vec::new(),
        }
    }
}

impl Sink for BoundedSink {
    fn push_transform(&mut self, _: &HbTransform) {}

    fn pop_transform(&mut self) {}

    fn push_clip_glyph(&mut self, _: &Face, _: u16, _: &Coords) {
        self.clips = self.clips.saturating_add(1);
    }

    fn push_clip_rectangle(&mut self, _: HbExtents) {
        self.clips = self.clips.saturating_add(1);
    }

    fn pop_clip(&mut self) {
        self.clips = self.clips.saturating_sub(1);
    }

    fn push_group(&mut self) {
        self.groups.push(self.bounded);
        self.bounded = true;
    }

    fn pop_group(&mut self, m: u8) {
        let source = self.bounded;
        // An unbalanced pop reads HarfBuzz's empty-vector default, false.
        let backdrop = self.groups.pop().unwrap_or(false);
        self.bounded = match m {
            mode::CLEAR => true,
            mode::SRC | mode::SRC_OUT => source,
            mode::DEST | mode::DEST_OUT => backdrop,
            mode::SRC_IN | mode::DEST_IN => backdrop && source,
            _ => backdrop || source,
        };
    }

    fn paint(&mut self) {
        if self.clips == 0 {
            self.bounded = false;
        }
    }
}

/// `hb_paint_extents_context_t`: a transform, clip and group stack, each
/// paint adding the clip it is under to the group it is in.
struct ExtentsSink {
    transforms: Vec<HbTransform>,
    clips: Vec<HbBounds>,
    groups: Vec<HbBounds>,
    /// Each glyph clipped to so far, and the box it draws.
    drawn: BTreeMap<u16, HbExtents>,
}

/// The box around glyph `gid` at `coords` as HarfBuzz draws it into
/// `hb_draw_extents`: every point a segment is drawn to, control points too,
/// and the point each contour starts from -- which the draw session hands on
/// only when a segment follows it, so a contour that draws no segment adds
/// nothing. Void, for a glyph that draws nothing.
fn drawn_extents(face: &Face, gid: u16, coords: &Coords) -> HbExtents {
    let mut e = HbExtents::VOID;
    let Ok(outline) = face.outline_at(gid, coords) else {
        return e;
    };
    // `hb_draw_state_t`: whether a path is open, and where the pen is.
    let mut open = false;
    let (mut x, mut y) = (0.0f32, 0.0f32);
    for cmd in &outline.commands {
        let (to, controls): (_, &[_]) = match cmd {
            PathCmd::MoveTo(p) => {
                open = false;
                (x, y) = (p.x, p.y);
                continue;
            }
            PathCmd::Close => {
                open = false;
                (x, y) = (0.0, 0.0);
                continue;
            }
            PathCmd::LineTo(p) => (p, &[]),
            PathCmd::QuadTo(c, p) => (p, core::slice::from_ref(c)),
            PathCmd::CurveTo(a, b, p) => (p, &[*a, *b]),
        };
        if !open {
            e.add_point(x, y);
            open = true;
        }
        for c in controls {
            e.add_point(c.x, c.y);
        }
        e.add_point(to.x, to.y);
        (x, y) = (to.x, to.y);
    }
    e
}

impl ExtentsSink {
    fn new() -> Self {
        Self {
            transforms: vec![HbTransform::IDENTITY],
            clips: vec![HbBounds::of(Status::Unbounded)],
            groups: vec![HbBounds::of(Status::Empty)],
            drawn: BTreeMap::new(),
        }
    }

    /// `get_extents`: the outermost group's box, whatever its status.
    fn extents(&self) -> HbExtents {
        self.groups.last().map_or(HbExtents::VOID, |g| g.extents)
    }

    fn transform(&self) -> HbTransform {
        self.transforms
            .last()
            .copied()
            .unwrap_or(HbTransform::IDENTITY)
    }

    fn push_clip(&mut self, e: &HbExtents) {
        let mut bounds = HbBounds::from_extents(self.transform().transform_extents(e));
        if let Some(clip) = self.clips.last() {
            bounds.intersect(clip);
        }
        self.clips.push(bounds);
    }
}

impl Sink for ExtentsSink {
    fn push_transform(&mut self, t: &HbTransform) {
        let mut next = self.transform();
        next.multiply(t);
        self.transforms.push(next);
    }

    fn pop_transform(&mut self) {
        self.transforms.pop();
    }

    /// The box glyph `gid` draws ([`drawn_extents`]), as a clip -- drawn
    /// once per walk, however many times the graph clips to it: the box
    /// depends on nothing but the glyph and the instance, and an emoji's
    /// graph clips to the same few glyphs again and again.
    fn push_clip_glyph(&mut self, face: &Face, gid: u16, coords: &Coords) {
        let e = *self
            .drawn
            .entry(gid)
            .or_insert_with(|| drawn_extents(face, gid, coords));
        self.push_clip(&e);
    }

    fn push_clip_rectangle(&mut self, e: HbExtents) {
        self.push_clip(&e);
    }

    fn pop_clip(&mut self) {
        self.clips.pop();
    }

    fn push_group(&mut self) {
        self.groups.push(HbBounds::of(Status::Empty));
    }

    fn pop_group(&mut self, m: u8) {
        let Some(source) = self.groups.pop() else {
            return;
        };
        let Some(backdrop) = self.groups.last_mut() else {
            return;
        };
        match m {
            mode::CLEAR => backdrop.status = Status::Empty,
            mode::SRC | mode::SRC_OUT => *backdrop = source,
            mode::DEST | mode::DEST_OUT => {}
            mode::SRC_IN | mode::DEST_IN => backdrop.intersect(&source),
            _ => backdrop.union(&source),
        }
    }

    fn paint(&mut self) {
        let clip = self
            .clips
            .last()
            .copied()
            .unwrap_or(HbBounds::of(Status::Unbounded));
        if let Some(group) = self.groups.last_mut() {
            group.union(&clip);
        }
    }
}

// ---------------------------------------------------------------------------
// The walk
// ---------------------------------------------------------------------------

/// `HB_MAX_NESTING_LEVEL`: how deep the walk goes.
const MAX_NESTING: i32 = 64;

/// `HB_MAX_GRAPH_EDGE_COUNT`: how many edges the walk follows in all.
const MAX_EDGES: i32 = 2048;

/// One walk of a version-1 paint graph for one of the measurements: the
/// graph limits and cycle detectors of HarfBuzz's `hb_paint_context_t`,
/// which each measurement has afresh.
struct Walk<'t, 'f, S: Sink> {
    tables: &'t Tables<'f>,
    face: &'f Face,
    coords: &'f Coords,
    sink: S,
    depth_left: i32,
    edges_left: i32,
    glyphs: Decycler,
    layers: Decycler,
}

impl<'t, 'f, S: Sink> Walk<'t, 'f, S> {
    fn new(tables: &'t Tables<'f>, face: &'f Face, coords: &'f Coords, sink: S) -> Self {
        Self {
            tables,
            face,
            coords,
            sink,
            depth_left: MAX_NESTING,
            edges_left: MAX_EDGES,
            glyphs: Decycler::default(),
            layers: Decycler::default(),
        }
    }

    /// `COLR::paint_glyph` for a version-1 glyph with no clip box: the glyph
    /// marked visited, and the font's transform around its graph -- which is
    /// walked only if `walk_graph`.
    fn root(&mut self, gid: u16, paint: Option<usize>, walk_graph: bool) {
        let node = self.glyphs.enter();
        self.glyphs.visit(node, u32::from(gid));
        self.sink.push_transform(&HbTransform::IDENTITY);
        if walk_graph {
            self.recurse(paint);
        }
        self.sink.pop_transform();
        self.glyphs.leave();
    }

    /// `hb_paint_context_t::recurse`: one edge, and one level deeper, to the
    /// paint at `at` -- or to the null paint, for `None`.
    fn recurse(&mut self, at: Option<usize>) {
        if self.depth_left <= 0 || self.edges_left <= 0 {
            return;
        }
        self.depth_left = self.depth_left.saturating_sub(1);
        self.edges_left = self.edges_left.saturating_sub(1);
        if let Some(at) = at {
            self.dispatch(at);
        }
        self.depth_left = self.depth_left.saturating_add(1);
    }

    fn dispatch(&mut self, at: usize) {
        let Some(format) = self.tables.paint_format(at) else {
            return;
        };
        let d = self.tables.colr;
        let field = |k: usize| at.checked_add(k);
        let child = |k: usize| at_offset(at, u24_at(d, field(k)?)?);
        match format {
            1 => {
                let (Some(count), Some(first)) = (
                    field(1).and_then(|k| u8_at(d, k)),
                    field(2).and_then(|k| u32_at(d, k)),
                ) else {
                    return;
                };
                self.layers(first, count);
            }
            2..=9 => self.sink.paint(),
            10 => {
                if let Some(gid) = field(4).and_then(|k| u16_at(d, k)) {
                    self.glyph(child(1), gid);
                }
            }
            11 => {
                if let Some(gid) = field(1).and_then(|k| u16_at(d, k)) {
                    self.colr_glyph(gid);
                }
            }
            12..=31 => {
                if let Some(t) = self.tables.hb_transform(at, format) {
                    self.sink.push_transform(&t);
                    self.recurse(child(1));
                    self.sink.pop_transform();
                }
            }
            32 => {
                let Some(m) = field(4).and_then(|k| u8_at(d, k)) else {
                    return;
                };
                let m = if m <= mode::LAST { m } else { mode::CLEAR };
                self.sink.push_group();
                self.recurse(child(5));
                self.sink.push_group();
                self.recurse(child(1));
                self.sink.pop_group(m);
                self.sink.pop_group(mode::SRC_OVER);
            }
            _ => {}
        }
    }

    /// `PaintColrLayers`: LayerList entries `first..first + count`, counted
    /// in 32 bits as HarfBuzz counts them.
    fn layers(&mut self, first: usize, count: u8) {
        let Ok(first) = u32::try_from(first) else {
            return;
        };
        let end = first.wrapping_add(u32::from(count));
        let node = self.layers.enter();
        for i in first..end {
            if !self.layers.visit(node, i) {
                break;
            }
            let paint = usize::try_from(i)
                .ok()
                .and_then(|i| self.tables.layer_v1(i));
            self.recurse(paint);
        }
        self.layers.leave();
    }

    /// `PaintGlyph`: `paint` clipped to glyph `gid` -- a solid fill as one
    /// `fill_glyph`, which costs an edge but no level, while it is allowed
    /// either. The font's transform and its inverse are the identity here.
    fn glyph(&mut self, paint: Option<usize>, gid: u16) {
        let solid = paint
            .and_then(|p| self.tables.paint_format(p))
            .is_some_and(|f| matches!(f, 2 | 3));
        if solid && self.depth_left > 0 && self.edges_left > 0 {
            self.edges_left = self.edges_left.saturating_sub(1);
            self.sink.push_transform(&HbTransform::IDENTITY);
            self.sink.push_clip_glyph(self.face, gid, self.coords);
            self.sink.paint();
            self.sink.pop_clip();
            self.sink.pop_transform();
            return;
        }
        self.sink.push_transform(&HbTransform::IDENTITY);
        self.sink.push_clip_glyph(self.face, gid, self.coords);
        self.sink.push_transform(&HbTransform::IDENTITY);
        self.recurse(paint);
        self.sink.pop_transform();
        self.sink.pop_clip();
        self.sink.pop_transform();
    }

    /// `PaintColrGlyph`: another base glyph's graph, under its clip box if
    /// it has one -- unless the glyph is already being painted.
    fn colr_glyph(&mut self, gid: u16) {
        let node = self.glyphs.enter();
        if self.glyphs.visit(node, u32::from(gid)) {
            let clip = self.tables.clip_corners(gid);
            if let Some([x_min, y_min, x_max, y_max]) = clip {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "`int` to `float`, as HarfBuzz passes a clip box's corners"
                )]
                self.sink.push_clip_rectangle(HbExtents {
                    x_min: x_min as f32,
                    y_min: y_min as f32,
                    x_max: x_max as f32,
                    y_max: y_max as f32,
                });
            }
            if let Some(paint) = self.tables.base_paint(gid) {
                self.recurse(paint.at());
            }
            if clip.is_some() {
                self.sink.pop_clip();
            }
        }
        self.glyphs.leave();
    }
}

impl Tables<'_> {
    /// The format of the paint record at `at`, if the record's fixed part
    /// lies within the table. HarfBuzz's sanitizer nulls the offset to a
    /// record cut short, so to HarfBuzz such a record is the null paint;
    /// `None` is that here.
    fn paint_format(&self, at: usize) -> Option<u8> {
        let format = u8_at(self.colr, at)?;
        let size: usize = match format {
            11 => 3,
            2 => 5,
            1 | 10 | 20 | 24 => 6,
            12 | 13 => 7,
            14 | 16 | 28 | 32 => 8,
            3 => 9,
            21 | 22 | 25 | 26 => 10,
            8 | 15 | 17 | 18 | 29 | 30 => 12,
            23 | 27 => 14,
            4 | 6 | 9 | 19 | 31 => 16,
            5 | 7 => 20,
            // Formats this does not know paint nothing, whatever their size.
            _ => 1,
        };
        (at.checked_add(size)? <= self.colr.len()).then_some(format)
    }

    /// The transform of paint `at`, of format 12 to 31, as HarfBuzz builds it
    /// for `push_transform`: each field's delta added to its raw value before
    /// it is scaled (`F2DOT14::to_float (delta)`), angles in half-turns times
    /// `HB_PI`, skews by `-x` and `y`. (The renderer's [`super::Affine`]s
    /// scale first and add after, which can differ in the last bit.)
    fn hb_transform(&self, at: usize, format: u8) -> Option<HbTransform> {
        let d = self.colr;
        let field = |k: usize| at.checked_add(k);
        // A variable format -- the odd ones -- keeps its `varIndexBase`
        // after its fixed fields, at `k`.
        let base = |k: usize| self.var_base(if format % 2 == 1 { field(k) } else { None });
        let int = |k: usize| Some(f32::from(i16_at(d, field(k)?)?));
        let f2dot14 = |k: usize, base: u32, i: u32| {
            Some((f32::from(i16_at(d, field(k)?)?) + self.delta(base, i)) / 16384.0)
        };
        Some(match format {
            12 | 13 => self.hb_affine(at, format == 13),
            14 | 15 => {
                let b = base(8);
                HbTransform::translation(int(4)? + self.delta(b, 0), int(6)? + self.delta(b, 1))
            }
            16 | 17 => {
                let b = base(8);
                HbTransform::scaling(f2dot14(4, b, 0)?, f2dot14(6, b, 1)?)
            }
            18 | 19 => {
                let b = base(12);
                HbTransform::scaling_around_center(
                    f2dot14(4, b, 0)?,
                    f2dot14(6, b, 1)?,
                    int(8)? + self.delta(b, 2),
                    int(10)? + self.delta(b, 3),
                )
            }
            20 | 21 => {
                let s = f2dot14(4, base(6), 0)?;
                HbTransform::scaling(s, s)
            }
            22 | 23 => {
                let b = base(10);
                let s = f2dot14(4, b, 0)?;
                HbTransform::scaling_around_center(
                    s,
                    s,
                    int(6)? + self.delta(b, 1),
                    int(8)? + self.delta(b, 2),
                )
            }
            24 | 25 => HbTransform::rotation(f2dot14(4, base(6), 0)? * HB_PI),
            26 | 27 => {
                let b = base(10);
                HbTransform::rotation_around_center(
                    f2dot14(4, b, 0)? * HB_PI,
                    int(6)? + self.delta(b, 1),
                    int(8)? + self.delta(b, 2),
                )
            }
            28 | 29 => {
                let b = base(8);
                HbTransform::skewing(-f2dot14(4, b, 0)? * HB_PI, f2dot14(6, b, 1)? * HB_PI)
            }
            30 | 31 => {
                let b = base(12);
                HbTransform::skewing_around_center(
                    -f2dot14(4, b, 0)? * HB_PI,
                    f2dot14(6, b, 1)? * HB_PI,
                    int(8)? + self.delta(b, 2),
                    int(10)? + self.delta(b, 3),
                )
            }
            _ => return None,
        })
    }

    /// The `Affine2x3` of `PaintTransform` `at` (a `VarAffine2x3` if
    /// `variable`), each 16.16 field's delta added before it is scaled. An
    /// affine that is not there -- a null offset, or one cut short, which
    /// HarfBuzz's sanitizer nulls -- is HarfBuzz's null object: every field
    /// zero, the variation index included.
    fn hb_affine(&self, at: usize, variable: bool) -> HbTransform {
        let d = self.colr;
        let size = if variable { 28 } else { 24 };
        let Some(affine) = at
            .checked_add(4)
            .and_then(|k| u24_at(d, k))
            .and_then(|offset| at_offset(at, offset))
            .filter(|&t| t.checked_add(size).is_some_and(|end| end <= d.len()))
        else {
            let null_base = if variable { 0 } else { super::NO_VARIATION };
            let delta = |i: u32| self.delta(null_base, i) / 65536.0;
            return HbTransform {
                xx: delta(0),
                yx: delta(1),
                xy: delta(2),
                yy: delta(3),
                x0: delta(4),
                y0: delta(5),
            };
        };
        let base = self.var_base(if variable {
            affine.checked_add(24)
        } else {
            None
        });
        let fixed = |k: usize, i: u32| {
            #[allow(
                clippy::cast_precision_loss,
                reason = "`int32_t` to `float`, rounding past 2^24 as C's conversion does"
            )]
            let raw = super::i32_at(d, affine.checked_add(k)?)? as f32;
            Some((raw + self.delta(base, i)) / 65536.0)
        };
        (|| {
            Some(HbTransform {
                xx: fixed(0, 0)?,
                yx: fixed(4, 1)?,
                xy: fixed(8, 2)?,
                yy: fixed(12, 3)?,
                x0: fixed(16, 4)?,
                y0: fixed(20, 5)?,
            })
        })()
        .unwrap_or(HbTransform::ZERO)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests fail loudly on malformed fixtures"
)]
mod tests {
    use super::*;
    use crate::colr_fixture::{COLR_EXTENTS, COLR_FACE, COLR_WGHT};

    #[test]
    fn every_colour_glyph_measures_as_harfbuzz_measures_it() {
        let face = Face::parse(COLR_FACE.to_vec()).unwrap();
        let default = Coords::default();
        let varied = face
            .variation_axes()
            .unwrap()
            .normalize_tags(&[(*b"wght", COLR_WGHT)]);
        assert!(!varied.is_default());
        let mut wrong = Vec::new();
        for &(gid, name, at_default, at_wght) in &COLR_EXTENTS {
            for (coords, expected) in [(&default, at_default), (&varied, at_wght)] {
                let got = face.glyph_extents_at(gid, coords);
                if got != Some(expected) {
                    wrong.push((name, coords.is_default(), got, expected));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "boxes unlike HarfBuzz's (name, at the default, ours, HarfBuzz's): {wrong:#?}"
        );
    }

    #[test]
    fn a_glyph_colr_has_nothing_for_keeps_its_outline_box() {
        let face = Face::parse(COLR_FACE.to_vec()).unwrap();
        let default = Coords::default();
        // Glyph 2, `square`, is an outline glyph only -- its box drawn from
        // 100 but placed on its `hmtx` side bearing of 0, as HarfBuzz
        // places it.
        assert_eq!(glyph_extents(&face, 2, &default), None);
        assert_eq!(
            face.glyph_extents_at(2, &default),
            Some([0, 500, 400, -400])
        );
    }

    #[test]
    fn transforming_the_void_box_makes_a_unit_square() {
        let t = HbTransform::IDENTITY;
        let e = t.transform_extents(&HbExtents::VOID);
        assert_eq!(
            e,
            HbExtents {
                x_min: -1.0,
                y_min: -1.0,
                x_max: 0.0,
                y_max: 0.0
            }
        );
        assert!(!e.is_empty());
    }

    #[test]
    fn edges_round_half_away_from_zero() {
        let e = HbExtents {
            x_min: -2.5,
            y_min: -0.5,
            x_max: 2.5,
            y_max: 0.5,
        };
        // -3 and 3, -1 and 1: a width of 6 and a height of -2.
        assert_eq!(e.to_glyph_extents(), [-3, 1, 6, -2]);
        let nan = HbExtents {
            x_min: f32::NAN,
            ..e
        };
        assert_eq!(nan.to_glyph_extents(), [0; 4]);
    }

    #[test]
    fn composite_modes_combine_bounds_as_harfbuzz_does() {
        let boxed = |x0: f32, x1: f32| {
            HbBounds::from_extents(HbExtents {
                x_min: x0,
                y_min: 0.0,
                x_max: x1,
                y_max: 10.0,
            })
        };
        let run = |m: u8| {
            let mut s = ExtentsSink::new();
            s.push_group();
            s.clips.push(boxed(0.0, 10.0));
            s.paint();
            s.clips.pop();
            s.push_group();
            s.clips.push(boxed(5.0, 20.0));
            s.paint();
            s.clips.pop();
            s.pop_group(m);
            let group = *s.groups.last().unwrap();
            (group.status, group.extents.x_min, group.extents.x_max)
        };
        assert_eq!(run(mode::SRC_OVER), (Status::Bounded, 0.0, 20.0));
        assert_eq!(run(mode::SRC_IN), (Status::Bounded, 5.0, 10.0));
        assert_eq!(run(mode::SRC), (Status::Bounded, 5.0, 20.0));
        assert_eq!(run(mode::DEST_OUT), (Status::Bounded, 0.0, 10.0));
        assert_eq!(run(mode::CLEAR).0, Status::Empty);
    }

    #[test]
    fn a_fill_under_no_clip_is_unbounded_unless_an_intersection_bounds_it() {
        let mut s = BoundedSink::new();
        s.paint();
        assert!(!s.bounded);
        // SRC_IN of an unbounded fill over a clipped one is unbounded, and
        // HarfBuzz's SRC_OVER of that into a bounded backdrop is bounded.
        let mut s = BoundedSink::new();
        s.push_group();
        s.push_clip_rectangle(HbExtents::VOID);
        s.paint();
        s.pop_clip();
        s.push_group();
        s.paint();
        s.pop_group(mode::SRC_IN);
        assert!(!s.bounded);
        s.pop_group(mode::SRC_OVER);
        assert!(s.bounded);
    }

    #[test]
    fn a_mutated_table_measures_something_or_nothing_but_never_panics() {
        let range = crate::sfnt::tests::table_range(&COLR_FACE, *b"COLR").unwrap();
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for round in 0..400 {
            let mut bytes = COLR_FACE.to_vec();
            for _ in 0..=(next() % 4) {
                let i = range.start + (next() % range.len() as u64) as usize;
                bytes[i] = next() as u8;
            }
            let Ok(face) = Face::parse(bytes) else {
                continue;
            };
            let varied = face
                .variation_axes()
                .map(|axes| axes.normalize_tags(&[(*b"wght", COLR_WGHT)]))
                .unwrap_or_default();
            for gid in 0..face.num_glyphs() {
                let _ = face.glyph_extents_at(gid, &Coords::default());
                if round % 4 == 0 {
                    let _ = face.glyph_extents_at(gid, &varied);
                }
            }
        }
    }
}
