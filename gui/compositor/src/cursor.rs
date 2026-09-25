//! The mouse pointer: what it looks like, and how it gets onto the screen.
//!
//! # Why this exists
//!
//! Until 2026-09-24 nothing drew a pointer. The compositor worked out *which*
//! pointer belonged under the mouse — an I-beam over text, arrows on a window
//! edge, a hand over a link — and then discarded the answer: on the development
//! host the arrow on screen was Windows' own, and on real hardware there would
//! have been none at all (`open-questions.md` C-Q18).
//!
//! # A plane, not part of the picture
//!
//! The pointer is drawn *over* the composited frame at the moment it is shown,
//! never into it — the arrangement a display controller's cursor plane has, and
//! the reason this module hands presenters a separate [`PointerSprite`] rather
//! than the compositor painting one. Three things follow:
//!
//! - **Moving the pointer changes nothing in the scene.** No damage, no
//!   repaint, no window re-rendered because the mouse crossed it —
//!   `compose_frame` has nothing to compose. A presenter restores the few
//!   hundred pixels the pointer covered and draws it again elsewhere.
//! - **Fullscreen keeps its shortcut.** A direct-scanout frame is never
//!   composited, so a pointer that had to be composited could not appear over
//!   it — the dilemma C-Q18 put to the operator. Every presenter that exists
//!   copies the frame to the display anyway, and the pointer is drawn onto that
//!   copy at no cost. (A future *zero-copy* scanout, which flips to a client's
//!   own buffer, is where the dilemma returns; that is what the hardware cursor
//!   plane is for.)
//! - **A hardware cursor is a presenter's business.** The sprite is exactly
//!   what `SYS_DRM_CURSOR_SET` wants — an ARGB image and a hot spot — so moving
//!   to the hardware plane changes the DRM presenter and nothing above it.
//!
//! # The artwork
//!
//! Every shape is vector outlines on a 32-unit grid ([`art`]), rasterized with
//! the font engine's exact-area rasterizer at whatever size the user and the
//! display ask for, so a 48-pixel pointer on a 2x display is as sharp as a
//! 16-pixel one. The outline is not drawn separately: it is the body's coverage
//! grown by a pixel or two ([`grow`]), which keeps it exactly concentric with
//! the fill at every size — two separately drawn shapes would drift apart by a
//! pixel somewhere.

use std::collections::HashMap;
use std::sync::Arc;

use osfont::raster::rasterize;
use osfont::sfnt::{Outline, PathCmd, Point};

use crate::{CursorShape, Rect};

/// The side of the design grid every shape is drawn on.
const GRID: f32 = 32.0;

/// The smallest and largest pointer, in pixels, that will be rasterized.
///
/// The user's setting is 16 to 48 and a display scale multiplies it; the bounds
/// are there so a nonsense scale cannot ask for a zero-pixel pointer or a
/// ten-thousand-pixel one.
const MIN_SIZE_PX: u32 = 8;
/// See [`MIN_SIZE_PX`].
const MAX_SIZE_PX: u32 = 256;

/// How the pointer should look: its size and its two colours.
///
/// Resolved by the compositor from the user's appearance settings and the
/// display under the pointer, so this module never needs to know what a
/// setting is called or where it is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CursorStyle {
    /// The pointer's nominal size in pixels: the side of the 32-unit grid.
    pub size_px: u32,
    /// The body colour, `0xAARRGGBB`.
    pub fill: u32,
    /// The outline colour, and the colour of interior marks.
    pub outline: u32,
}

/// The pointer the display should show: what, where, and in what style.
///
/// A description rather than pixels, so the compositor stays free of artwork
/// and the question "what pointer is up" can be answered and tested without
/// rasterizing anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerState {
    /// The shape. Never [`CursorShape::Hidden`] — a hidden pointer is no
    /// pointer at all.
    pub shape: CursorShape,
    /// Where the hot spot is, in frame coordinates.
    pub x: i32,
    /// See [`x`](Self::x).
    pub y: i32,
    /// How it looks.
    pub style: CursorStyle,
}

/// A rasterized pointer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The hot spot — the pixel that *is* the pointer's position — from the
    /// image's top-left corner.
    pub hot_x: u32,
    /// See [`hot_x`](Self::hot_x).
    pub hot_y: u32,
    /// Row-major **premultiplied** `0xAARRGGBB`, `width * height` of them.
    ///
    /// Premultiplied because the only thing ever done with these is to lay
    /// them over a frame, and premultiplied source-over is one multiply per
    /// channel with no division — and because the anti-aliased edge, where
    /// alpha is fractional, is exactly where a straight-alpha round trip loses
    /// precision.
    pub pixels: Vec<u32>,
}

/// A pointer placed on the frame: an image and where its top-left corner goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PointerSprite {
    /// The picture. Shared, because it is cached and a presenter may hold on
    /// to it across frames.
    pub image: Arc<CursorImage>,
    /// The image's left edge in frame coordinates. May be negative: a pointer
    /// at the very edge of the screen hangs off it.
    pub x: i32,
    /// The image's top edge in frame coordinates.
    pub y: i32,
}

impl PointerSprite {
    /// The frame rectangle the sprite covers, before clipping to any display.
    #[must_use]
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.image.width, self.image.height)
    }

    /// Lay the pointer over a row-major ARGB frame of `width` x `height`.
    ///
    /// Returns the part of the frame it touched, if any, so a caller keeping
    /// its own copy of the frame knows what to restore when the pointer moves.
    pub fn blend_over(&self, frame: &mut [u32], width: u32, height: u32) -> Option<Rect> {
        let visible = self.rect().intersect(&Rect::new(0, 0, width, height))?;
        let stride = usize::try_from(width).ok()?;
        self.for_each_visible(visible, |sprite_px, x, y| {
            let at = y.checked_mul(stride)?.checked_add(x)?;
            let dst = frame.get_mut(at)?;
            *dst = over(sprite_px, *dst);
            Some(())
        });
        Some(visible)
    }

    /// Lay the pointer over an `XRGB8888` scanout buffer: rows of `pitch`
    /// bytes, `view_width` x `view_height` pixels, showing the frame rectangle
    /// whose top-left corner is `(origin_x, origin_y)`.
    ///
    /// Returns the part of the *view* it touched, in view coordinates.
    pub fn blend_over_xrgb(
        &self,
        dst: &mut [u8],
        pitch: usize,
        view_width: u32,
        view_height: u32,
        origin_x: i32,
        origin_y: i32,
    ) -> Option<Rect> {
        let local = PointerSprite {
            image: Arc::clone(&self.image),
            x: self.x.saturating_sub(origin_x),
            y: self.y.saturating_sub(origin_y),
        };
        let visible = local
            .rect()
            .intersect(&Rect::new(0, 0, view_width, view_height))?;
        local.for_each_visible(visible, |sprite_px, x, y| {
            let at = y.checked_mul(pitch)?.checked_add(x.checked_mul(4)?)?;
            let bytes = dst.get_mut(at..at.checked_add(4)?)?;
            let old = u32::from_le_bytes(<[u8; 4]>::try_from(&*bytes).ok()?);
            bytes.copy_from_slice(&(over(sprite_px, old) | 0xFF00_0000).to_le_bytes());
            Some(())
        });
        Some(visible)
    }

    /// Visit every sprite pixel inside `visible` (a rectangle in the same
    /// coordinates as the sprite's position), with the destination column and
    /// row it lands on.
    fn for_each_visible(
        &self,
        visible: Rect,
        mut put: impl FnMut(u32, usize, usize) -> Option<()>,
    ) {
        let image = &self.image;
        for y in visible.y..visible.bottom() {
            let (Ok(dy), Ok(sy)) = (usize::try_from(y), u32::try_from(y.saturating_sub(self.y)))
            else {
                continue;
            };
            for x in visible.x..visible.right() {
                let (Ok(dx), Ok(sx)) =
                    (usize::try_from(x), u32::try_from(x.saturating_sub(self.x)))
                else {
                    continue;
                };
                let Some(px) = image.pixel(sx, sy) else {
                    continue;
                };
                if px >> 24 == 0 {
                    continue;
                }
                // A write that falls outside the destination is dropped, as a
                // clip would: the sprite never panics the display server.
                let _ = put(px, dx, dy);
            }
        }
    }
}

impl CursorImage {
    /// The premultiplied pixel at `(x, y)`, or `None` outside the image.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<u32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = usize::try_from(y)
            .ok()?
            .checked_mul(usize::try_from(self.width).ok()?)?
            .checked_add(usize::try_from(x).ok()?)?;
        self.pixels.get(at).copied()
    }

    /// The same image with every pixel's colour passed through `filter`,
    /// which works on straight (not premultiplied) `0xAARRGGBB`.
    ///
    /// For the accessibility colour filters and night light, which the server
    /// applies to the frame and must apply to the pointer too — a pointer that
    /// stayed cold white on a warmed screen would be the one thing on it the
    /// setting missed.
    #[must_use]
    pub fn filtered(&self, filter: impl Fn(u32) -> u32) -> CursorImage {
        CursorImage {
            pixels: self
                .pixels
                .iter()
                .map(|&px| premultiply(filter(unpremultiply(px))))
                .collect(),
            ..self.clone()
        }
    }
}

/// Premultiplied `src` over opaque-or-not `dst`, both `0xAARRGGBB`.
fn over(src: u32, dst: u32) -> u32 {
    let a = src >> 24;
    if a == 0xFF {
        return src;
    }
    let inv = 255u32.saturating_sub(a);
    let channel = |shift: u32| -> u32 {
        let s = (src >> shift) & 0xFF;
        let d = (dst >> shift) & 0xFF;
        // s + d * inv / 255, rounded; at most 255 because s <= a.
        s.saturating_add(d.saturating_mul(inv).saturating_add(127) / 255)
            .min(255)
    };
    (channel(24) << 24) | (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

/// Straight `0xAARRGGBB` to premultiplied.
fn premultiply(px: u32) -> u32 {
    let a = px >> 24;
    let channel = |shift: u32| ((px >> shift) & 0xFF).saturating_mul(a).saturating_add(127) / 255;
    (a << 24) | (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

/// Premultiplied `0xAARRGGBB` to straight.
fn unpremultiply(px: u32) -> u32 {
    let a = px >> 24;
    if a == 0 {
        return 0;
    }
    let channel = |shift: u32| {
        ((px >> shift) & 0xFF)
            .saturating_mul(255)
            .saturating_add(a / 2)
            .checked_div(a)
            .unwrap_or(0)
            .min(255)
    };
    (a << 24) | (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

/// Rasterized pointers, kept so a frame does not rasterize one.
///
/// Keyed on shape and style, which is everything the picture depends on. The
/// style changes only when the user changes a setting or the pointer crosses
/// onto a display of a different scale, so in practice this holds a handful of
/// images; it is bounded all the same, because "in practice" is a claim about
/// how the compositor is driven, not a property of this type.
#[derive(Debug, Default)]
pub struct CursorCache {
    images: HashMap<(CursorShape, CursorStyle), Arc<CursorImage>>,
}

impl CursorCache {
    /// Most images held before the cache starts again from empty.
    ///
    /// Thirteen shapes times a few styles is far below this; reaching it means
    /// the style is changing continuously, and then starting over is the right
    /// response rather than a pessimisation.
    const CAPACITY: usize = 64;

    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The sprite for `state`, rasterizing its image if this is the first time
    /// that shape has been asked for in that style. `None` for a shape that
    /// draws nothing.
    pub fn sprite(&mut self, state: &PointerState) -> Option<PointerSprite> {
        let key = (state.shape, state.style);
        let image = match self.images.get(&key) {
            Some(image) => Arc::clone(image),
            None => {
                let image = Arc::new(render(state.shape, &state.style)?);
                if self.images.len() >= Self::CAPACITY {
                    self.images.clear();
                }
                self.images.insert(key, Arc::clone(&image));
                image
            }
        };
        let hot_x = i32::try_from(image.hot_x).unwrap_or(0);
        let hot_y = i32::try_from(image.hot_y).unwrap_or(0);
        Some(PointerSprite {
            x: state.x.saturating_sub(hot_x),
            y: state.y.saturating_sub(hot_y),
            image,
        })
    }
}

// ---------------------------------------------------------------------------
// Rasterizing
// ---------------------------------------------------------------------------

/// Rasterize `shape` in `style`. `None` for [`CursorShape::Hidden`].
#[must_use]
pub fn render(shape: CursorShape, style: &CursorStyle) -> Option<CursorImage> {
    let art = art(shape)?;
    let size = style.size_px.clamp(MIN_SIZE_PX, MAX_SIZE_PX);
    #[allow(
        clippy::cast_precision_loss,
        reason = "size is clamped to 8..=256, exact in f32"
    )]
    let scale = size as f32 / GRID;

    // The outline's width: a pixel up to 24 px, growing with the pointer so a
    // large one does not look drawn in hairline. Measured in pixels rather
    // than grid units on purpose — an outline is there to separate the pointer
    // from the screen behind it, and what separates is pixels.
    let outline_px = (size / 24).max(1);
    let margin = outline_px.saturating_add(1);
    let side = size.saturating_add(margin.saturating_mul(2));
    let len = usize::try_from(side)
        .ok()?
        .checked_mul(usize::try_from(side).ok()?)?;

    let body = coverage(&art.body, scale, margin, side)?;
    let marks = coverage(&art.marks, scale, margin, side)?;
    let ring = grow(&body, side, outline_px);

    let mut pixels = Vec::with_capacity(len);
    for i in 0..len {
        let mut px = [0.0f32; 4]; // premultiplied a, r, g, b
        paint(&mut px, style.outline, ring.get(i).copied().unwrap_or(0.0));
        paint(&mut px, style.fill, body.get(i).copied().unwrap_or(0.0));
        paint(&mut px, style.outline, marks.get(i).copied().unwrap_or(0.0));
        pixels.push(pack(px));
    }

    let hot = |v: f32| {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a hot spot on the 32-unit grid, scaled by at most 8, is a small positive number"
        )]
        let px = (v * scale).floor().max(0.0) as u32;
        px.saturating_add(margin).min(side.saturating_sub(1))
    };
    Some(CursorImage {
        width: side,
        height: side,
        hot_x: hot(art.hot.0),
        hot_y: hot(art.hot.1),
        pixels,
    })
}

/// Coverage, 0 to 1, of `contours` scaled by `scale` onto a `side` x `side`
/// canvas whose grid origin is `margin` pixels in from the top-left.
fn coverage(contours: &[Contour], scale: f32, margin: u32, side: u32) -> Option<Vec<f32>> {
    let len = usize::try_from(side)
        .ok()?
        .checked_mul(usize::try_from(side).ok()?)?;
    let mut out = vec![0.0f32; len];
    if contours.is_empty() {
        return Some(out);
    }
    // The rasterizer works in font space — y up — and flips; the grid is y
    // down, so it is handed the mirror image and flips it back.
    let mut commands = Vec::new();
    for contour in contours {
        let mut points = contour.iter();
        let Some(&(x, y)) = points.next() else {
            continue;
        };
        commands.push(PathCmd::MoveTo(Point::new(x, -y)));
        for &(x, y) in points {
            commands.push(PathCmd::LineTo(Point::new(x, -y)));
        }
        commands.push(PathCmd::Close);
    }
    let mask = rasterize(&Outline { commands }, scale).ok()?;
    let margin = i64::from(margin);
    for row in 0..mask.height {
        for col in 0..mask.width {
            let x = i64::from(mask.left)
                .saturating_add(margin)
                .saturating_add(i64::from(col));
            let y = i64::from(mask.top)
                .saturating_add(margin)
                .saturating_add(i64::from(row));
            let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
                continue;
            };
            if x >= side || y >= side {
                continue;
            }
            let at = usize::try_from(y)
                .ok()
                .and_then(|y| y.checked_mul(usize::try_from(side).ok()?))
                .and_then(|r| r.checked_add(usize::try_from(x).ok()?));
            if let Some(cell) = at.and_then(|at| out.get_mut(at)) {
                *cell = f32::from(mask.at(col, row)) / 255.0;
            }
        }
    }
    Some(out)
}

/// `coverage` grown outward by `radius` pixels, with a soft edge: the outline.
///
/// Each pixel takes the strongest coverage within the radius, faded over the
/// last pixel of distance so the grown edge is anti-aliased like the one it
/// grew from. Deriving the outline from the body — rather than drawing a
/// second, larger shape — is what keeps the two concentric at every size.
fn grow(coverage: &[f32], side: u32, radius: u32) -> Vec<f32> {
    let side_i = i64::from(side);
    let r = i64::from(radius).saturating_add(1);
    #[allow(
        clippy::cast_precision_loss,
        reason = "the radius is a few pixels, exact in f32"
    )]
    let reach = radius as f32 + 0.5;
    let at = |x: i64, y: i64| -> f32 {
        if x < 0 || y < 0 || x >= side_i || y >= side_i {
            return 0.0;
        }
        usize::try_from(y.saturating_mul(side_i).saturating_add(x))
            .ok()
            .and_then(|i| coverage.get(i))
            .copied()
            .unwrap_or(0.0)
    };
    let mut out = Vec::with_capacity(coverage.len());
    for y in 0..side_i {
        for x in 0..side_i {
            let mut best = 0.0f32;
            for dy in r.saturating_neg()..=r {
                for dx in r.saturating_neg()..=r {
                    #[allow(
                        clippy::cast_precision_loss,
                        reason = "offsets of a few pixels, exact in f32"
                    )]
                    let distance =
                        (dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy)) as f32).sqrt();
                    let weight = (reach - distance + 0.5).clamp(0.0, 1.0);
                    if weight > 0.0 {
                        best = best.max(at(x.saturating_add(dx), y.saturating_add(dy)) * weight);
                    }
                }
            }
            out.push(best);
        }
    }
    out
}

/// Paint `color` at `coverage` over the premultiplied `px` (a, r, g, b).
fn paint(px: &mut [f32; 4], color: u32, coverage: f32) {
    if coverage <= 0.0 {
        return;
    }
    let alpha = f32::from(((color >> 24) & 0xFF) as u8) / 255.0 * coverage.min(1.0);
    let keep = 1.0 - alpha;
    let channel = |shift: u32| f32::from(((color >> shift) & 0xFF) as u8) / 255.0;
    px[0] = alpha + px[0] * keep;
    px[1] = channel(16) * alpha + px[1] * keep;
    px[2] = channel(8) * alpha + px[2] * keep;
    px[3] = channel(0) * alpha + px[3] * keep;
}

/// Premultiplied float (a, r, g, b) to a premultiplied `0xAARRGGBB`.
fn pack(px: [f32; 4]) -> u32 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255 first"
    )]
    let byte = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u32;
    let a = byte(px[0]);
    // A premultiplied channel can never exceed its alpha; rounding could
    // otherwise make it one more, which `over` would then carry into a wrap.
    let c = |v: f32| byte(v).min(a);
    (a << 24) | (c(px[1]) << 16) | (c(px[2]) << 8) | c(px[3])
}

// ---------------------------------------------------------------------------
// The artwork
// ---------------------------------------------------------------------------

/// One closed polygon on the 32-unit grid, y down.
type Contour = Vec<(f32, f32)>;

/// A pointer shape: filled contours, marks drawn over them in the outline
/// colour, and the hot spot, all on the 32-unit grid.
struct Art {
    body: Vec<Contour>,
    marks: Vec<Contour>,
    hot: (f32, f32),
}

/// The artwork for `shape`, or `None` for a shape that draws nothing.
///
/// Every contour is normalised to one winding direction ([`wound`]) before it
/// is used, except a hole, which is wound the other way on purpose: the
/// rasterizer fills by non-zero winding, so overlapping pieces of one shape
/// union and a counter-wound one cuts a hole. A piece accidentally wound the
/// other way would cut a hole where it was meant to add — which is why the
/// direction is enforced rather than remembered.
fn art(shape: CursorShape) -> Option<Art> {
    let centre = (16.0, 16.0);
    Some(match shape {
        CursorShape::Hidden => return None,
        CursorShape::Arrow => Art {
            body: vec![arrow()],
            marks: Vec::new(),
            hot: (2.0, 2.0),
        },
        CursorShape::Text => Art {
            body: vec![
                rect(15.0, 6.0, 17.0, 26.0),
                rect(11.0, 4.0, 21.0, 6.5),
                rect(11.0, 25.5, 21.0, 28.0),
            ],
            marks: Vec::new(),
            hot: centre,
        },
        CursorShape::Hand => hand(),
        CursorShape::ResizeNS => Art {
            body: vec![double_arrow()],
            marks: Vec::new(),
            hot: centre,
        },
        CursorShape::ResizeEW => Art {
            body: vec![rotate(&double_arrow(), centre, 90.0)],
            marks: Vec::new(),
            hot: centre,
        },
        // North-west to south-east: the vertical arrow turned a quarter of the
        // way toward the east, so its top points north-west.
        CursorShape::ResizeNWSE => Art {
            body: vec![rotate(&double_arrow(), centre, -45.0)],
            marks: Vec::new(),
            hot: centre,
        },
        CursorShape::ResizeNESW => Art {
            body: vec![rotate(&double_arrow(), centre, 45.0)],
            marks: Vec::new(),
            hot: centre,
        },
        CursorShape::Move => Art {
            body: vec![four_way_arrow()],
            marks: Vec::new(),
            hot: centre,
        },
        CursorShape::Wait => hourglass(),
        CursorShape::Help => help(),
        CursorShape::Crosshair => Art {
            body: vec![
                capsule((16.0, 2.5), (16.0, 12.0), 1.25),
                capsule((16.0, 20.0), (16.0, 29.5), 1.25),
                capsule((2.5, 16.0), (12.0, 16.0), 1.25),
                capsule((20.0, 16.0), (29.5, 16.0), 1.25),
            ],
            marks: Vec::new(),
            hot: centre,
        },
        CursorShape::NotAllowed => Art {
            body: vec![
                circle(centre, 12.5),
                hole(centre, 9.0),
                // The bar runs top-left to bottom-right, inside the ring.
                capsule((9.5, 9.5), (22.5, 22.5), 1.9),
            ],
            marks: Vec::new(),
            hot: centre,
        },
    })
}

/// The ordinary arrow, tip at (2, 2).
fn arrow() -> Contour {
    wound(vec![
        (2.0, 2.0),
        (2.0, 24.5),
        (7.5, 19.3),
        (11.4, 28.0),
        (14.8, 26.5),
        (10.9, 18.0),
        (18.5, 18.0),
    ])
}

/// A two-headed vertical arrow across the whole grid.
fn double_arrow() -> Contour {
    wound(vec![
        (16.0, 2.0),
        (22.5, 9.5),
        (18.0, 9.5),
        (18.0, 22.5),
        (22.5, 22.5),
        (16.0, 30.0),
        (9.5, 22.5),
        (14.0, 22.5),
        (14.0, 9.5),
        (9.5, 9.5),
    ])
}

/// Four arrowheads on a cross.
fn four_way_arrow() -> Contour {
    wound(vec![
        (16.0, 2.0),
        (21.0, 7.5),
        (17.75, 7.5),
        (17.75, 14.25),
        (24.5, 14.25),
        (24.5, 11.0),
        (30.0, 16.0),
        (24.5, 21.0),
        (24.5, 17.75),
        (17.75, 17.75),
        (17.75, 24.5),
        (21.0, 24.5),
        (16.0, 30.0),
        (11.0, 24.5),
        (14.25, 24.5),
        (14.25, 17.75),
        (7.5, 17.75),
        (7.5, 21.0),
        (2.0, 16.0),
        (7.5, 11.0),
        (7.5, 14.25),
        (14.25, 14.25),
        (14.25, 7.5),
        (11.0, 7.5),
    ])
}

/// A pointing hand, the tip of the index finger at the hot spot.
fn hand() -> Art {
    Art {
        body: vec![
            // Index finger, pointing up.
            capsule((13.25, 4.0), (13.25, 17.0), 2.25),
            // Three curled fingers, each a little lower than the last.
            capsule((17.6, 12.5), (17.6, 18.0), 2.1),
            capsule((21.8, 13.5), (21.8, 19.0), 2.1),
            capsule((25.7, 15.0), (25.7, 20.0), 1.9),
            // The palm, and the thumb reaching across it.
            rounded_rect(11.0, 16.0, 27.6, 29.5, 4.0),
            capsule((7.0, 18.5), (12.5, 24.5), 2.1),
        ],
        // The creases between the curled fingers.
        marks: vec![
            rect(15.3, 14.5, 15.9, 19.5),
            rect(19.5, 15.5, 20.1, 20.0),
            rect(23.6, 16.8, 24.2, 20.5),
        ],
        hot: (13.25, 2.0),
    }
}

/// An hourglass, sand running from the top bulb into the bottom.
fn hourglass() -> Art {
    Art {
        body: vec![
            rect(8.0, 2.5, 24.0, 6.0),
            rect(8.0, 26.0, 24.0, 29.5),
            wound(vec![
                (10.0, 6.0),
                (22.0, 6.0),
                (22.0, 9.0),
                (17.6, 16.0),
                (22.0, 23.0),
                (22.0, 26.0),
                (10.0, 26.0),
                (10.0, 23.0),
                (14.4, 16.0),
                (10.0, 9.0),
            ]),
        ],
        marks: vec![
            // What is left in the top bulb, and the pile in the bottom one.
            wound(vec![(12.5, 9.5), (19.5, 9.5), (16.0, 14.0)]),
            wound(vec![(12.0, 25.0), (16.0, 20.5), (20.0, 25.0)]),
        ],
        hot: (16.0, 16.0),
    }
}

/// The arrow, a little smaller, with a question mark beside it.
fn help() -> Art {
    let small_arrow = scale_about(&arrow(), (2.0, 2.0), 0.78);
    let mut body = vec![small_arrow];
    // The question mark's hook, as a run of round-ended strokes along an arc,
    // then its stem and dot.
    let (cx, cy, r) = (23.5, 16.5, 3.6);
    let mut hook: Vec<(f32, f32)> = (0..=9)
        .map(|i| {
            #[allow(
                clippy::cast_precision_loss,
                reason = "a step index below ten, exact in f32"
            )]
            let t = (200.0 + 25.0 * i as f32).to_radians();
            (cx + r * t.cos(), cy + r * t.sin())
        })
        .collect();
    hook.push((23.5, 22.5));
    for pair in hook.windows(2) {
        if let [a, b] = pair {
            body.push(capsule(*a, *b, 1.35));
        }
    }
    body.push(circle((23.5, 27.0), 1.6));
    Art {
        body,
        marks: Vec::new(),
        hot: (2.0, 2.0),
    }
}

/// An axis-aligned rectangle from `(x0, y0)` to `(x1, y1)`.
fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Contour {
    wound(vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
}

/// A rectangle with every corner rounded by `r`.
fn rounded_rect(x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> Contour {
    let mut points = Vec::new();
    for (corner, start) in [
        ((x1 - r, y0 + r), 270.0f32),
        ((x1 - r, y1 - r), 0.0),
        ((x0 + r, y1 - r), 90.0),
        ((x0 + r, y0 + r), 180.0),
    ] {
        points.extend(arc(corner, r, start, start + 90.0, 6));
    }
    wound(points)
}

/// A circle, wound like every other filled piece.
fn circle(centre: (f32, f32), r: f32) -> Contour {
    wound(arc(centre, r, 0.0, 360.0, 32))
}

/// A circle wound the *other* way, which the non-zero fill turns into a hole.
fn hole(centre: (f32, f32), r: f32) -> Contour {
    let mut points = circle(centre, r);
    points.reverse();
    points
}

/// A stroke from `a` to `b` of half-width `r`, with round ends.
fn capsule(a: (f32, f32), b: (f32, f32), r: f32) -> Contour {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    // The direction the stroke runs, as an angle, so the two end caps can be
    // swept as half circles facing away from each other.
    let heading = dy.atan2(dx).to_degrees();
    let mut points = arc(b, r, heading - 90.0, heading + 90.0, 12);
    points.extend(arc(a, r, heading + 90.0, heading + 270.0, 12));
    wound(points)
}

/// `segments + 1` points along a circular arc from `from` to `to` degrees.
fn arc(centre: (f32, f32), r: f32, from: f32, to: f32, segments: u16) -> Vec<(f32, f32)> {
    let steps = segments.max(1);
    (0..=steps)
        .map(|i| {
            let t = (from + (to - from) * f32::from(i) / f32::from(steps)).to_radians();
            (centre.0 + r * t.cos(), centre.1 + r * t.sin())
        })
        .collect()
}

/// `contour` turned by `degrees` (clockwise on screen) about `about`.
fn rotate(contour: &[(f32, f32)], about: (f32, f32), degrees: f32) -> Contour {
    let (sin, cos) = degrees.to_radians().sin_cos();
    wound(
        contour
            .iter()
            .map(|&(x, y)| {
                let (dx, dy) = (x - about.0, y - about.1);
                (about.0 + dx * cos - dy * sin, about.1 + dx * sin + dy * cos)
            })
            .collect(),
    )
}

/// `contour` scaled by `factor` about `about`.
fn scale_about(contour: &[(f32, f32)], about: (f32, f32), factor: f32) -> Contour {
    wound(
        contour
            .iter()
            .map(|&(x, y)| {
                (
                    about.0 + (x - about.0) * factor,
                    about.1 + (y - about.1) * factor,
                )
            })
            .collect(),
    )
}

/// `points`, reversed if need be so the polygon winds clockwise on screen —
/// positive area in y-down coordinates.
fn wound(mut points: Contour) -> Contour {
    if signed_area(&points) < 0.0 {
        points.reverse();
    }
    points
}

/// Twice the signed area of a closed polygon, by the shoelace formula;
/// positive for clockwise on screen (y down).
fn signed_area(points: &[(f32, f32)]) -> f32 {
    let mut sum = 0.0f32;
    for (i, &(x0, y0)) in points.iter().enumerate() {
        let (x1, y1) = points
            .get(i.saturating_add(1))
            .or_else(|| points.first())
            .copied()
            .unwrap_or((x0, y0));
        sum += x0 * y1 - x1 * y0;
    }
    sum
}

// The five defensive lints the workspace turns on are for production code; a
// test that indexes a fixture it just built is asserting. CLAUDE.md's lint
// policy says as much.
#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
mod tests {
    use super::*;

    const EVERY_SHAPE: [CursorShape; 13] = [
        CursorShape::Arrow,
        CursorShape::Text,
        CursorShape::Hand,
        CursorShape::ResizeNS,
        CursorShape::ResizeEW,
        CursorShape::ResizeNESW,
        CursorShape::ResizeNWSE,
        CursorShape::Move,
        CursorShape::Wait,
        CursorShape::Help,
        CursorShape::Crosshair,
        CursorShape::NotAllowed,
        CursorShape::Hidden,
    ];

    fn style(size_px: u32) -> CursorStyle {
        CursorStyle {
            size_px,
            fill: 0xFFFF_FFFF,
            outline: 0xFF00_0000,
        }
    }

    fn alpha(image: &CursorImage, x: u32, y: u32) -> u32 {
        image.pixel(x, y).unwrap_or(0) >> 24
    }

    /// Whether the pixel is (mostly) the fill colour, which for the test style
    /// is white.
    fn is_fill(image: &CursorImage, x: u32, y: u32) -> bool {
        let px = unpremultiply(image.pixel(x, y).unwrap_or(0));
        px >> 24 > 200 && (px & 0xFF) > 200
    }

    /// Whether the pixel is (mostly) the outline colour, black.
    fn is_outline(image: &CursorImage, x: u32, y: u32) -> bool {
        let px = unpremultiply(image.pixel(x, y).unwrap_or(0));
        px >> 24 > 200 && (px & 0xFF) < 60
    }

    /// Every shape that draws anything draws something, at every size the
    /// setting offers and at a 2x display's double of each.
    #[test]
    fn every_visible_shape_rasterizes_at_every_size() {
        for size in [16, 24, 32, 48, 96] {
            for shape in EVERY_SHAPE {
                let image = render(shape, &style(size));
                if shape == CursorShape::Hidden {
                    assert!(image.is_none(), "a hidden pointer drew something");
                    continue;
                }
                let image = image.unwrap_or_else(|| panic!("{shape:?} at {size}px drew nothing"));
                let inked = image.pixels.iter().filter(|&&p| p >> 24 > 0).count();
                assert!(
                    inked * 20 > (size * size) as usize,
                    "{shape:?} at {size}px inked only {inked} pixels"
                );
                assert_eq!(image.pixels.len(), (image.width * image.height) as usize);
                assert!(image.hot_x < image.width && image.hot_y < image.height);
            }
        }
    }

    /// The pointer is its size: the setting names the grid's side in pixels,
    /// and the image is that plus a margin for the outline — not twice it, not
    /// half.
    #[test]
    fn an_image_is_the_size_asked_for_plus_its_outline() {
        for size in [16, 24, 32, 48] {
            let image = render(CursorShape::Arrow, &style(size)).expect("arrow");
            assert!(
                image.width >= size && image.width <= size + 8,
                "a {size}px arrow is {}px wide",
                image.width
            );
        }
    }

    /// The arrow's hot spot is its tip, and the tip is on the image: the pixel
    /// the pointer *is* has ink, and nothing above or left of it does.
    #[test]
    fn the_arrow_points_with_its_tip() {
        for size in [16, 24, 48] {
            let image = render(CursorShape::Arrow, &style(size)).expect("arrow");
            let (hx, hy) = (image.hot_x, image.hot_y);
            assert!(
                alpha(&image, hx, hy) > 0,
                "{size}px: the hot spot has no ink"
            );
            for y in 0..hy.saturating_sub(2) {
                for x in 0..image.width {
                    assert_eq!(
                        alpha(&image, x, y),
                        0,
                        "{size}px: ink above the tip at ({x}, {y})"
                    );
                }
            }
        }
    }

    /// White inside, black around it: the two colours are where the scheme
    /// says, so the pointer is visible on a white page and on a black one.
    #[test]
    fn the_body_is_the_fill_and_the_edge_is_the_outline() {
        let image = render(CursorShape::Arrow, &style(32)).expect("arrow");
        // Well inside the arrow's shaft: grid (5, 12).
        let inside = (5.0 * 32.0 / GRID) as u32 + image.hot_x - 2;
        let row = (12.0 * 32.0 / GRID) as u32 + image.hot_y - 2;
        assert!(
            is_fill(&image, inside, row),
            "the body of the arrow is not white"
        );
        // Walking left from inside, the last inked pixel before the transparent
        // background is outline, not fill.
        let mut x = inside;
        while x > 0 && alpha(&image, x - 1, row) > 0 {
            x -= 1;
        }
        assert!(
            is_outline(&image, x, row),
            "the arrow's left edge is not black"
        );
    }

    /// Inverted swaps the two, and the accent scheme is its own colour.
    #[test]
    fn the_scheme_decides_the_colours() {
        let inverted = CursorStyle {
            size_px: 32,
            fill: 0xFF00_0000,
            outline: 0xFFFF_FFFF,
        };
        let image = render(CursorShape::Arrow, &inverted).expect("arrow");
        let x = image.hot_x + 3;
        let y = image.hot_y + 12;
        let px = unpremultiply(image.pixel(x, y).expect("in the image"));
        assert!(
            px >> 24 > 200 && (px & 0xFF) < 60,
            "an inverted arrow is not black inside: {px:#010x}"
        );
    }

    /// The not-allowed sign has a hole: the pixels between the ring and the
    /// bar are clear. A hole wound the wrong way would fill the whole disc.
    #[test]
    fn the_not_allowed_ring_has_a_hole_in_it() {
        let image = render(CursorShape::NotAllowed, &style(48)).expect("sign");
        // Grid (19.5, 11.5): in the hole, 5.7 units from the centre — clear of
        // the ring's inner edge at 9 and of the outline grown inward from it —
        // and 5.7 units from the bar's centre line, clear of its half-width.
        let s = 48.0 / GRID;
        let margin = image.hot_x - (16.0 * s) as u32;
        let (x, y) = ((19.5 * s) as u32 + margin, (11.5 * s) as u32 + margin);
        assert_eq!(
            alpha(&image, x, y),
            0,
            "the ring is filled in at ({x}, {y})"
        );
        // And the ring itself is inked, straight above the centre.
        let ring = ((16.0 * s) as u32 + margin, (4.8 * s) as u32 + margin);
        assert!(
            alpha(&image, ring.0, ring.1) > 200,
            "the ring itself is missing"
        );
    }

    /// Every contour of every shape winds the same way, except the one hole.
    /// Under a non-zero fill a piece wound backwards cuts where it should add,
    /// and nothing in the rasterizer would say so.
    #[test]
    fn every_filled_piece_winds_the_same_way() {
        for shape in EVERY_SHAPE {
            let Some(art) = art(shape) else { continue };
            for (i, contour) in art.body.iter().chain(&art.marks).enumerate() {
                let area = signed_area(contour);
                let is_hole = shape == CursorShape::NotAllowed && i == 1;
                assert_eq!(
                    area < 0.0,
                    is_hole,
                    "{shape:?} piece {i} winds {} (area {area})",
                    if area < 0.0 { "backwards" } else { "forwards" }
                );
            }
        }
    }

    /// The artwork stays on its grid, so nothing is cut off by the canvas.
    #[test]
    fn every_shape_fits_its_grid() {
        for shape in EVERY_SHAPE {
            let Some(art) = art(shape) else { continue };
            for &(x, y) in art.body.iter().chain(&art.marks).flatten() {
                assert!(
                    (0.0..=GRID).contains(&x) && (0.0..=GRID).contains(&y),
                    "{shape:?} reaches ({x}, {y}), off the grid"
                );
            }
            assert!((0.0..GRID).contains(&art.hot.0) && (0.0..GRID).contains(&art.hot.1));
        }
    }

    /// A sprite over a frame: the pointer's pixels land where its position
    /// says, the hot spot on the pointer's position, and nothing outside the
    /// image is touched.
    #[test]
    fn a_sprite_lands_with_its_hot_spot_on_the_pointer() {
        let mut cache = CursorCache::new();
        let state = PointerState {
            shape: CursorShape::Arrow,
            x: 40,
            y: 30,
            style: style(24),
        };
        let sprite = cache.sprite(&state).expect("arrow");
        let (w, h) = (100u32, 80u32);
        let mut frame = vec![0xFF33_3333u32; (w * h) as usize];
        let touched = sprite.blend_over(&mut frame, w, h).expect("on screen");
        assert_eq!(touched, sprite.rect());
        // The hot spot is inked in the sprite, so the pointer's own pixel
        // changed.
        assert_ne!(frame[(30 * w + 40) as usize], 0xFF33_3333);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                if !touched.contains(x, y) {
                    assert_eq!(frame[(y as u32 * w + x as u32) as usize], 0xFF33_3333);
                }
            }
        }
    }

    /// At the screen's edge the sprite is clipped, not wrapped and not a panic.
    #[test]
    fn a_sprite_hanging_off_the_frame_is_clipped() {
        let mut cache = CursorCache::new();
        for (x, y) in [(-5, -5), (99, 79), (-100, 40), (1000, 1000)] {
            let state = PointerState {
                shape: CursorShape::Move,
                x,
                y,
                style: style(32),
            };
            let sprite = cache.sprite(&state).expect("move");
            let mut frame = vec![0u32; 100 * 80];
            if let Some(r) = sprite.blend_over(&mut frame, 100, 80) {
                assert!(r.x >= 0 && r.y >= 0 && r.right() <= 100 && r.bottom() <= 80);
            }
        }
    }

    /// The scanout form draws the same pixels the frame form does, at the
    /// driver's pitch and from the head's own origin.
    #[test]
    fn the_scanout_form_draws_what_the_frame_form_draws() {
        let mut cache = CursorCache::new();
        let state = PointerState {
            shape: CursorShape::Hand,
            x: 70,
            y: 20,
            style: style(24),
        };
        let sprite = cache.sprite(&state).expect("hand");
        let (w, h) = (120u32, 60u32);
        let mut frame = vec![0xFF10_2030u32; (w * h) as usize];
        sprite.blend_over(&mut frame, w, h);

        // A head showing the frame's right half, with padded rows.
        let (origin_x, head_w) = (60i32, 60u32);
        let pitch = 64 * 4;
        let mut scanout = vec![0u8; pitch * h as usize];
        for y in 0..h as usize {
            for x in 0..head_w as usize {
                let at = y * pitch + x * 4;
                scanout[at..at + 4].copy_from_slice(&0xFF10_2030u32.to_le_bytes());
            }
        }
        sprite.blend_over_xrgb(&mut scanout, pitch, head_w, h, origin_x, 0);
        for y in 0..h as usize {
            for x in 0..head_w as usize {
                let at = y * pitch + x * 4;
                let got = u32::from_le_bytes(scanout[at..at + 4].try_into().unwrap());
                let want = frame[y * w as usize + x + origin_x as usize] | 0xFF00_0000;
                assert_eq!(got, want, "head pixel ({x}, {y})");
            }
        }
    }

    /// The same shape in the same style is rasterized once.
    #[test]
    fn the_cache_hands_back_the_same_image() {
        let mut cache = CursorCache::new();
        let state = PointerState {
            shape: CursorShape::Text,
            x: 0,
            y: 0,
            style: style(24),
        };
        let a = cache.sprite(&state).expect("beam");
        let b = cache
            .sprite(&PointerState { x: 50, ..state })
            .expect("beam");
        assert!(
            Arc::ptr_eq(&a.image, &b.image),
            "the image was rasterized twice"
        );
        let bigger = cache
            .sprite(&PointerState {
                style: style(48),
                ..state
            })
            .expect("beam");
        assert!(
            !Arc::ptr_eq(&a.image, &bigger.image),
            "a new size reused the old image"
        );
    }

    #[test]
    fn premultiplication_round_trips_an_opaque_colour_exactly() {
        for px in [0xFF12_3456u32, 0xFFFF_FFFF, 0xFF00_0000, 0xFFAB_CDEF] {
            assert_eq!(unpremultiply(premultiply(px)), px);
        }
        assert_eq!(premultiply(0x0012_3456), 0, "a clear pixel has no colour");
    }

    #[test]
    fn over_is_source_over() {
        // Opaque source replaces.
        assert_eq!(over(0xFF11_2233, 0xFFAA_BBCC), 0xFF11_2233);
        // Clear source leaves the destination alone.
        assert_eq!(over(0x0000_0000, 0xFFAA_BBCC), 0xFFAA_BBCC);
        // Half-covered white over black is mid grey.
        let half_white = premultiply(0x80FF_FFFF);
        let g = over(half_white, 0xFF00_0000);
        assert_eq!(g >> 24, 0xFF);
        assert!((0x7E..=0x81).contains(&(g & 0xFF)), "{g:#010x}");
    }
}
