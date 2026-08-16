//! A width × height grid of [`Color`], with drawing primitives that clip.
//!
//! # Why this is a toolkit type and not an app's private one
//!
//! Three apps in this tree wrote the same thing independently — a `Vec<u8>`
//! of pixel bytes carried alongside a `width` and a `height` — and each got it
//! wrong in its own way, because the three fields have a relationship that
//! nothing in the type expresses. `apps/explorer`'s thumbnail generator held
//! **115 index expressions** that each restated it: four bytes per pixel,
//! row-major, `(y * width + x) * 4`. Every one of them was in bounds only
//! because of a bound proved somewhere else, and three of those proofs were
//! wrong. `apps/paint`'s version made the fields `pub`, which means the
//! invariant "`data.len() == width * height * 4`" could be broken by any code
//! that assigns to `data`, and its `offset()` then returned an index past the
//! end of a buffer it had already agreed was in bounds.
//!
//! `Canvas` states the shape once. It is the one place in the tree that knows
//! a pixel buffer is row-major.
//!
//! # Two decisions worth stating
//!
//! **It stores `Vec<Color>`, not `Vec<u8>`.** A pixel *is* a colour; the byte
//! order it is written in is a property of a file format or a framebuffer, not
//! of the image. Both of the buffers this replaces baked a byte order into
//! every accessor — explorer's was ARGB, paint's was RGBA — which meant a
//! function moved between them silently produced colour-swapped output, and
//! meant the byte order had to be re-derived at 115 sites rather than stated
//! at the two that serialise. Here it is confined to [`Canvas::from_argb`],
//! [`Canvas::to_argb`], [`Canvas::from_rgba`] and [`Canvas::to_rgba`], which
//! is where a byte order is actually a fact about something. `Color` is four
//! bytes and `Copy`, so this costs no memory.
//!
//! **Every primitive clips; none panics.** A rectangle drawn partly off the
//! edge draws the part that fits. Reading off-canvas returns `None`, writing
//! off-canvas does nothing. That is what all three of the replaced buffers
//! wanted at every site — several were hand-rolling a `break` to get it — and
//! it matters because the coordinates reaching a canvas often come from a file
//! the user merely *opened*: a thumbnail generated while browsing a directory,
//! a bitmap loaded into an editor. A wrong coordinate should be a cosmetic
//! defect, not a crash.
//!
//! # Example
//!
//! ```
//! use guitk::canvas::Canvas;
//! use guitk::color::Color;
//!
//! let mut c = Canvas::filled(4, 4, Color::rgb(0, 0, 0));
//! // Half off the right edge: the half that fits is drawn.
//! c.fill_rect(2, 0, 10, 1, Color::rgb(255, 0, 0));
//! assert_eq!(c.get(3, 0), Some(Color::rgb(255, 0, 0)));
//! assert_eq!(c.get(1, 0), Some(Color::rgb(0, 0, 0)));
//! // Off-canvas is None rather than a panic.
//! assert_eq!(c.get(4, 0), None);
//! ```

use crate::color::Color;

/// A width × height grid of pixels, row-major.
///
/// See the [module documentation](self) for why this exists and what it
/// guarantees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canvas {
    width: u32,
    height: u32,
    /// Exactly `width * height` entries, row-major. Private, and every
    /// constructor preserves the length, so this is an invariant rather than
    /// a convention that the next caller has to remember.
    px: Vec<Color>,
}

impl Canvas {
    /// The number of pixels in a `width × height` image, or `None` if that
    /// many pixels could not be allocated on this machine.
    ///
    /// Checking `width * height` alone is **not** enough, and getting that
    /// wrong is a live abort rather than a theoretical one: on a 64-bit
    /// machine `u32::MAX × u32::MAX` fits in a `usize` comfortably, so the
    /// multiply succeeds and the panic happens one line later inside `Vec`,
    /// which rejects any request for more than `isize::MAX` *bytes*. A
    /// four-byte pixel means the real ceiling is a quarter of that. An image
    /// header can claim any dimensions it likes, so this is reachable by
    /// opening a file.
    ///
    /// What this cannot prevent is an allocation that is representable but
    /// larger than the machine has — a 100 000 × 100 000 canvas is a legal 40
    /// GB request. That is an out-of-memory condition rather than an overflow,
    /// and it needs a fallible allocator to handle, not a bounds check.
    fn pixel_count(width: u32, height: u32) -> Option<usize> {
        let n = (width as usize).checked_mul(height as usize)?;
        let bytes = n.checked_mul(size_of::<Color>())?;
        (bytes <= isize::MAX as usize).then_some(n)
    }

    /// A fully transparent canvas.
    ///
    /// Dimensions whose buffer could not be addressed collapse to `0 × 0`
    /// rather than aborting: the width and height reaching here often come
    /// from an image header, which is to say from a file the user did not
    /// write. An empty canvas draws nothing and reads `None`, which every
    /// primitive here already handles.
    #[must_use]
    pub fn transparent(width: u32, height: u32) -> Self {
        Self::filled(width, height, Color::TRANSPARENT)
    }

    /// A canvas filled with a single colour. See [`Canvas::transparent`] for
    /// what happens to dimensions that are too large.
    #[must_use]
    pub fn filled(width: u32, height: u32, color: Color) -> Self {
        match Self::pixel_count(width, height) {
            Some(n) => Self {
                width,
                height,
                px: vec![color; n],
            },
            None => Self::empty(),
        }
    }

    /// The `0 × 0` canvas.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            px: Vec::new(),
        }
    }

    /// Wrap an existing pixel vector, rejecting one whose length does not
    /// match the dimensions.
    ///
    /// Returning `None` rather than padding or truncating is deliberate: a
    /// length mismatch means the caller's idea of the image disagrees with the
    /// data, and silently resolving that disagreement produces an image that
    /// is wrong in a way nothing downstream can detect.
    #[must_use]
    pub fn from_pixels(width: u32, height: u32, px: Vec<Color>) -> Option<Self> {
        (px.len() == Self::pixel_count(width, height)?).then_some(Self { width, height, px })
    }

    /// Wrap a buffer of `A, R, G, B` bytes, four per pixel, row-major.
    #[must_use]
    pub fn from_argb(width: u32, height: u32, bytes: &[u8]) -> Option<Self> {
        Self::from_bytes(width, height, bytes, |[a, r, g, b]| Color::rgba(r, g, b, a))
    }

    /// Wrap a buffer of `R, G, B, A` bytes, four per pixel, row-major.
    #[must_use]
    pub fn from_rgba(width: u32, height: u32, bytes: &[u8]) -> Option<Self> {
        Self::from_bytes(width, height, bytes, |[r, g, b, a]| Color::rgba(r, g, b, a))
    }

    fn from_bytes(
        width: u32,
        height: u32,
        bytes: &[u8],
        decode: fn([u8; 4]) -> Color,
    ) -> Option<Self> {
        let n = Self::pixel_count(width, height)?;
        if bytes.len() != n.checked_mul(4)? {
            return None;
        }
        let px = bytes
            .chunks_exact(4)
            .map(|c| match *c {
                [a, b, g, d] => decode([a, b, g, d]),
                // `chunks_exact(4)` yields only 4-element slices, so this arm
                // is unreachable; it is written rather than `unreachable!()`
                // so a future change to the chunk size cannot introduce a
                // panic in code that decodes a file off disk.
                _ => Color::TRANSPARENT,
            })
            .collect();
        Some(Self { width, height, px })
    }

    /// Serialise to `A, R, G, B` bytes, four per pixel, row-major.
    #[must_use]
    pub fn to_argb(&self) -> Vec<u8> {
        self.to_bytes(|c| [c.a, c.r, c.g, c.b])
    }

    /// Serialise to `R, G, B, A` bytes, four per pixel, row-major.
    #[must_use]
    pub fn to_rgba(&self) -> Vec<u8> {
        self.to_bytes(|c| [c.r, c.g, c.b, c.a])
    }

    fn to_bytes(&self, encode: fn(Color) -> [u8; 4]) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len().saturating_mul(4));
        for c in &self.px {
            out.extend_from_slice(&encode(*c));
        }
        out
    }

    /// Width in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Whether the canvas has no pixels.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.px.is_empty()
    }

    /// The pixels, row-major.
    #[must_use]
    pub fn pixels(&self) -> &[Color] {
        &self.px
    }

    /// Index of the pixel at `(x, y)`, or `None` if it is off-canvas.
    fn index(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        (y as usize)
            .checked_mul(self.width as usize)?
            .checked_add(x as usize)
    }

    /// Read a pixel. `None` if `(x, y)` is off-canvas.
    #[must_use]
    pub fn get(&self, x: u32, y: u32) -> Option<Color> {
        self.px.get(self.index(x, y)?).copied()
    }

    /// Write a pixel. Off-canvas coordinates are ignored.
    pub fn set(&mut self, x: u32, y: u32, color: Color) {
        let Some(i) = self.index(x, y) else { return };
        if let Some(p) = self.px.get_mut(i) {
            *p = color;
        }
    }

    /// Alpha-blend `color` over the pixel at `(x, y)`. Off-canvas is ignored.
    pub fn blend(&mut self, x: u32, y: u32, color: Color) {
        if let Some(under) = self.get(x, y) {
            self.set(x, y, color.over(under));
        }
    }

    /// Fill the whole canvas with one colour.
    pub fn fill(&mut self, color: Color) {
        self.px.fill(color);
    }

    /// Fill the rectangle at `(x, y)` of size `w × h`, clipped to the canvas.
    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        let x_end = x.saturating_add(w).min(self.width);
        let y_end = y.saturating_add(h).min(self.height);
        for py in y..y_end {
            for px in x..x_end {
                self.set(px, py, color);
            }
        }
    }

    /// A `w × h` copy of the region whose top-left is `(x, y)`.
    ///
    /// Parts of the region that lie off this canvas come out transparent, so
    /// the result is always exactly `w × h` — a caller that asked for a
    /// rectangle gets one, rather than a smaller image it then has to
    /// discover the size of.
    #[must_use]
    pub fn copy_region(&self, x: u32, y: u32, w: u32, h: u32) -> Self {
        let mut out = Self::transparent(w, h);
        for dy in 0..h {
            for dx in 0..w {
                if let Some(c) = self.get(x.saturating_add(dx), y.saturating_add(dy)) {
                    out.set(dx, dy, c);
                }
            }
        }
        out
    }

    /// Alpha-blend `src` onto this canvas with its top-left at `(x, y)`.
    ///
    /// The offset is signed so a selection dragged off the top or left edge
    /// composites the part that remains visible.
    pub fn blend_from(&mut self, src: &Self, x: i32, y: i32) {
        self.composite(src, x, y, Self::blend);
    }

    /// Copy `src` onto this canvas at `(x, y)`, replacing rather than blending.
    pub fn draw_from(&mut self, src: &Self, x: i32, y: i32) {
        self.composite(src, x, y, Self::set);
    }

    fn composite(&mut self, src: &Self, x: i32, y: i32, put: fn(&mut Self, u32, u32, Color)) {
        for sy in 0..src.height {
            for sx in 0..src.width {
                let (Ok(sx_i), Ok(sy_i)) = (i32::try_from(sx), i32::try_from(sy)) else {
                    // Beyond 2^31 pixels across; there is no destination
                    // coordinate for it either, so there is nothing to draw.
                    continue;
                };
                let (dx, dy) = (x.saturating_add(sx_i), y.saturating_add(sy_i));
                let (Ok(dx), Ok(dy)) = (u32::try_from(dx), u32::try_from(dy)) else {
                    // Negative: off the top or left edge. `set`/`blend` clip
                    // the other two edges themselves.
                    continue;
                };
                if let Some(c) = src.get(sx, sy) {
                    put(self, dx, dy, c);
                }
            }
        }
    }

    /// Mirror left-to-right, in place.
    pub fn flip_horizontal(&mut self) {
        for row in self.px.chunks_exact_mut(self.width.max(1) as usize) {
            row.reverse();
        }
    }

    /// Mirror top-to-bottom, in place.
    pub fn flip_vertical(&mut self) {
        let stride = self.width.max(1) as usize;
        let rows = self.height as usize;
        for y in 0..rows / 2 {
            let Some(other) = rows.checked_sub(y).and_then(|r| r.checked_sub(1)) else {
                continue;
            };
            let (Some(a), Some(b)) = (y.checked_mul(stride), other.checked_mul(stride)) else {
                continue;
            };
            for i in 0..stride {
                let (Some(ai), Some(bi)) = (a.checked_add(i), b.checked_add(i)) else {
                    continue;
                };
                if ai < self.px.len() && bi < self.px.len() {
                    self.px.swap(ai, bi);
                }
            }
        }
    }

    /// A copy rotated 90° clockwise. Width and height swap.
    #[must_use]
    pub fn rotate_90_cw(&self) -> Self {
        let mut out = Self::transparent(self.height, self.width);
        for y in 0..self.height {
            for x in 0..self.width {
                if let Some(c) = self.get(x, y) {
                    // The pixel in row `y` moves to column `height - 1 - y`.
                    out.set(self.height.saturating_sub(1).saturating_sub(y), x, c);
                }
            }
        }
        out
    }

    /// A copy rotated 90° counter-clockwise. Width and height swap.
    #[must_use]
    pub fn rotate_90_ccw(&self) -> Self {
        let mut out = Self::transparent(self.height, self.width);
        for y in 0..self.height {
            for x in 0..self.width {
                if let Some(c) = self.get(x, y) {
                    out.set(y, self.width.saturating_sub(1).saturating_sub(x), c);
                }
            }
        }
        out
    }

    /// A copy rotated 180°.
    #[must_use]
    pub fn rotate_180(&self) -> Self {
        let mut out = self.clone();
        out.px.reverse();
        out
    }

    /// A `new_width × new_height` copy, sampled nearest-neighbour.
    ///
    /// Nearest-neighbour is the right default for an image *editor*: it is the
    /// only resampling that introduces no colours the user did not paint, so a
    /// palette stays a palette and a hard edge stays hard. Smooth downscaling
    /// of a photograph wants a box filter instead, which is a different
    /// operation with a different name rather than a mode of this one.
    #[must_use]
    pub fn resize_nearest(&self, new_width: u32, new_height: u32) -> Self {
        let mut out = Self::transparent(new_width, new_height);
        if new_width == 0 || new_height == 0 || self.is_empty() {
            return out;
        }
        for ny in 0..new_height {
            for nx in 0..new_width {
                // u64 throughout: `nx * width` is up to 2^64 for adversarial
                // dimensions, and f64 would lose the low bits of a large one.
                let sx = u64::from(nx)
                    .saturating_mul(u64::from(self.width))
                    .checked_div(u64::from(new_width))
                    .unwrap_or(0);
                let sy = u64::from(ny)
                    .saturating_mul(u64::from(self.height))
                    .checked_div(u64::from(new_height))
                    .unwrap_or(0);
                let sx = u32::try_from(sx).unwrap_or(u32::MAX);
                let sy = u32::try_from(sy).unwrap_or(u32::MAX);
                if let Some(c) = self.get(
                    sx.min(self.width.saturating_sub(1)),
                    sy.min(self.height.saturating_sub(1)),
                ) {
                    out.set(nx, ny, c);
                }
            }
        }
        out
    }

    /// A `new_width × new_height` copy with the existing pixels in place,
    /// cropped on the right and bottom and padded with `fill`.
    ///
    /// This is *canvas resize*, not image resize: it changes how much room
    /// there is around the picture without touching the picture.
    #[must_use]
    pub fn resized_canvas(&self, new_width: u32, new_height: u32, fill: Color) -> Self {
        let mut out = Self::filled(new_width, new_height, fill);
        for y in 0..self.height.min(new_height) {
            for x in 0..self.width.min(new_width) {
                if let Some(c) = self.get(x, y) {
                    out.set(x, y, c);
                }
            }
        }
        out
    }

    /// A copy downscaled by averaging each destination pixel's source region.
    ///
    /// Unlike [`Canvas::resize_nearest`] this is only a *reduction*: asking for
    /// a larger size returns a clone, because averaging cannot invent detail
    /// and a box filter used to enlarge just produces blocky nearest-neighbour
    /// output at more cost.
    #[must_use]
    pub fn box_downscale(&self, new_width: u32, new_height: u32) -> Self {
        if new_width == 0 || new_height == 0 || self.is_empty() {
            return Self::transparent(new_width, new_height);
        }
        if new_width >= self.width && new_height >= self.height {
            return self.clone();
        }
        let mut out = Self::transparent(new_width, new_height);
        for dy in 0..new_height {
            for dx in 0..new_width {
                let (sx0, sx1) = span(dx, self.width, new_width);
                let (sy0, sy1) = span(dy, self.height, new_height);
                let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
                for sy in sy0..sy1 {
                    for sx in sx0..sx1 {
                        if let Some(c) = self.get(sx, sy) {
                            r = r.saturating_add(u64::from(c.r));
                            g = g.saturating_add(u64::from(c.g));
                            b = b.saturating_add(u64::from(c.b));
                            a = a.saturating_add(u64::from(c.a));
                            n = n.saturating_add(1);
                        }
                    }
                }
                // One guard for all four channels; four `checked_div`s would
                // add four redundant unwraps under the same proof.
                #[allow(clippy::manual_checked_ops)]
                if n > 0 {
                    out.set(
                        dx,
                        dy,
                        Color::rgba((r / n) as u8, (g / n) as u8, (b / n) as u8, (a / n) as u8),
                    );
                }
            }
        }
        out
    }
}

/// The half-open source span `[lo, hi)` that destination index `d` covers when
/// `src` pixels are reduced to `dst`.
///
/// Both ends floor, so consecutive spans meet exactly: `span(d).1 ==
/// span(d+1).0`. Taking the ceiling for the upper end — which is the obvious
/// way to write "round outwards so nothing is missed" — makes adjacent spans
/// *overlap* wherever the ratio is not an integer, and a source pixel counted
/// by two destination pixels is weighted twice. Reducing 10 pixels to 3 that
/// way covers them `[1,1,1,2,1,1,2,1,1,1]`: the output is subtly wrong in a
/// way that looks like nothing more than softness. `span_covers_every_source
/// _pixel_exactly_once` pins it.
fn span(d: u32, src: u32, dst: u32) -> (u32, u32) {
    if dst == 0 || src == 0 {
        return (0, 0);
    }
    let scale = |i: u64| {
        i.saturating_mul(u64::from(src))
            .checked_div(u64::from(dst))
            .unwrap_or(0)
    };
    let lo = scale(u64::from(d));
    // At least one source pixel, so a destination pixel is never averaged
    // from nothing and left transparent. This matters when only one dimension
    // is being reduced: `2 × 100` to `8 × 3` asks for a span of 2 over 8.
    let hi = scale(u64::from(d).saturating_add(1))
        .max(lo.saturating_add(1))
        .min(u64::from(src));
    (
        u32::try_from(lo).unwrap_or(u32::MAX),
        u32::try_from(hi).unwrap_or(u32::MAX),
    )
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::*;

    const RED: Color = Color::rgb(255, 0, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);

    #[test]
    fn a_canvas_holds_exactly_the_pixels_its_dimensions_claim() {
        let c = Canvas::filled(3, 5, RED);
        assert_eq!(c.width(), 3);
        assert_eq!(c.height(), 5);
        assert_eq!(c.pixels().len(), 15);
        assert!(c.pixels().iter().all(|p| *p == RED));
    }

    #[test]
    fn dimensions_that_cannot_be_addressed_collapse_to_empty() {
        // The whole point of the type: an image header can claim any size at
        // all, and the answer must be an empty canvas rather than an abort.
        let c = Canvas::transparent(u32::MAX, u32::MAX);
        assert!(c.is_empty());
        assert_eq!(c.get(0, 0), None);
    }

    #[test]
    fn reading_and_writing_off_canvas_is_not_an_error() {
        let mut c = Canvas::filled(2, 2, RED);
        assert_eq!(c.get(2, 0), None);
        assert_eq!(c.get(0, 2), None);
        c.set(99, 99, BLUE);
        assert!(c.pixels().iter().all(|p| *p == RED));
    }

    #[test]
    fn a_pixel_lands_where_row_major_says_it_does() {
        let mut c = Canvas::transparent(3, 2);
        c.set(2, 1, RED);
        assert_eq!(c.get(2, 1), Some(RED));
        assert_eq!(c.pixels()[5], RED);
        assert_eq!(c.pixels()[4], Color::TRANSPARENT);
    }

    #[test]
    fn a_rect_off_the_edge_draws_the_part_that_fits() {
        let mut c = Canvas::filled(4, 4, BLUE);
        c.fill_rect(3, 3, 100, 100, RED);
        assert_eq!(c.get(3, 3), Some(RED));
        assert_eq!(c.get(2, 3), Some(BLUE));
        // Exactly one pixel changed, so nothing wrapped to the next row.
        assert_eq!(c.pixels().iter().filter(|p| **p == RED).count(), 1);
    }

    #[test]
    fn a_rect_whose_extent_overflows_still_clips() {
        let mut c = Canvas::filled(4, 4, BLUE);
        c.fill_rect(1, 1, u32::MAX, u32::MAX, RED);
        assert_eq!(c.pixels().iter().filter(|p| **p == RED).count(), 9);
    }

    // ------------------------------------------------------------------
    // Byte order
    // ------------------------------------------------------------------

    #[test]
    fn argb_and_rgba_are_not_the_same_bytes() {
        // The bug this separation prevents: explorer's buffer was ARGB and
        // paint's was RGBA, so a function moved between them swapped channels
        // silently. Naming both makes a mix-up a compile-time choice.
        let c = Canvas::filled(1, 1, Color::rgba(1, 2, 3, 4));
        assert_eq!(c.to_argb(), [4, 1, 2, 3]);
        assert_eq!(c.to_rgba(), [1, 2, 3, 4]);
    }

    #[test]
    fn bytes_round_trip_through_both_orders() {
        let c = Canvas::filled(2, 3, Color::rgba(9, 8, 7, 6));
        assert_eq!(Canvas::from_argb(2, 3, &c.to_argb()), Some(c.clone()));
        assert_eq!(Canvas::from_rgba(2, 3, &c.to_rgba()), Some(c));
    }

    #[test]
    fn a_buffer_that_does_not_match_its_dimensions_is_rejected() {
        // Padding or truncating would produce an image that is wrong in a way
        // nothing downstream could detect.
        assert_eq!(Canvas::from_rgba(2, 2, &[0; 15]), None);
        assert_eq!(Canvas::from_rgba(2, 2, &[0; 17]), None);
        assert!(Canvas::from_rgba(2, 2, &[0; 16]).is_some());
        assert_eq!(Canvas::from_pixels(2, 2, vec![RED; 3]), None);
        assert!(Canvas::from_pixels(2, 2, vec![RED; 4]).is_some());
    }

    // ------------------------------------------------------------------
    // Compositing
    // ------------------------------------------------------------------

    #[test]
    fn a_source_dragged_off_the_top_left_composites_what_remains() {
        let mut dst = Canvas::filled(4, 4, BLUE);
        let src = Canvas::filled(3, 3, RED);
        dst.draw_from(&src, -2, -2);
        assert_eq!(dst.get(0, 0), Some(RED));
        assert_eq!(dst.get(1, 0), Some(BLUE));
        assert_eq!(dst.pixels().iter().filter(|p| **p == RED).count(), 1);
    }

    #[test]
    fn blending_respects_alpha_and_drawing_does_not() {
        let half_red = Color::rgba(255, 0, 0, 128);
        let mut a = Canvas::filled(1, 1, BLUE);
        a.blend_from(&Canvas::filled(1, 1, half_red), 0, 0);
        let blended = a.get(0, 0).unwrap();
        assert!(blended != BLUE && blended != half_red, "{blended:?}");

        let mut b = Canvas::filled(1, 1, BLUE);
        b.draw_from(&Canvas::filled(1, 1, half_red), 0, 0);
        assert_eq!(b.get(0, 0), Some(half_red));
    }

    #[test]
    fn a_copied_region_is_the_size_asked_for_even_past_the_edge() {
        let c = Canvas::filled(2, 2, RED);
        let r = c.copy_region(1, 1, 3, 3);
        assert_eq!((r.width(), r.height()), (3, 3));
        assert_eq!(r.get(0, 0), Some(RED));
        assert_eq!(r.get(1, 0), Some(Color::TRANSPARENT));
    }

    // ------------------------------------------------------------------
    // Transforms
    // ------------------------------------------------------------------

    /// A canvas whose every pixel encodes its own coordinates, so a transform
    /// that moves a pixel to the wrong place cannot go unnoticed.
    fn coords(w: u32, h: u32) -> Canvas {
        let mut c = Canvas::transparent(w, h);
        for y in 0..h {
            for x in 0..w {
                c.set(x, y, Color::rgb(x as u8, y as u8, 0));
            }
        }
        c
    }

    #[test]
    fn flips_are_their_own_inverse() {
        for (w, h) in [(1, 1), (2, 3), (3, 2), (5, 5), (4, 1)] {
            let orig = coords(w, h);
            let mut c = orig.clone();
            c.flip_horizontal();
            c.flip_horizontal();
            assert_eq!(c, orig, "h {w}x{h}");
            c.flip_vertical();
            c.flip_vertical();
            assert_eq!(c, orig, "v {w}x{h}");
        }
    }

    #[test]
    fn a_horizontal_flip_moves_a_pixel_across_its_own_row() {
        let mut c = coords(3, 2);
        c.flip_horizontal();
        assert_eq!(c.get(0, 1), Some(Color::rgb(2, 1, 0)));
        assert_eq!(c.get(2, 1), Some(Color::rgb(0, 1, 0)));
    }

    #[test]
    fn a_vertical_flip_moves_a_pixel_down_its_own_column() {
        let mut c = coords(3, 2);
        c.flip_vertical();
        assert_eq!(c.get(1, 0), Some(Color::rgb(1, 1, 0)));
        assert_eq!(c.get(1, 1), Some(Color::rgb(1, 0, 0)));
    }

    #[test]
    fn four_quarter_turns_return_the_original() {
        for (w, h) in [(1, 1), (2, 3), (3, 2), (4, 4)] {
            let orig = coords(w, h);
            let round = orig
                .rotate_90_cw()
                .rotate_90_cw()
                .rotate_90_cw()
                .rotate_90_cw();
            assert_eq!(round, orig, "cw {w}x{h}");
            assert_eq!(orig.rotate_90_cw().rotate_90_ccw(), orig, "ccw {w}x{h}");
            assert_eq!(orig.rotate_90_cw().rotate_90_cw(), orig.rotate_180());
        }
    }

    #[test]
    fn a_quarter_turn_swaps_the_dimensions() {
        let c = coords(2, 5);
        assert_eq!(
            (c.rotate_90_cw().width(), c.rotate_90_cw().height()),
            (5, 2)
        );
        // Top-left goes to top-right under a clockwise turn.
        assert_eq!(c.rotate_90_cw().get(4, 0), Some(Color::rgb(0, 0, 0)));
    }

    // ------------------------------------------------------------------
    // Resampling
    // ------------------------------------------------------------------

    #[test]
    fn nearest_resize_to_the_same_size_changes_nothing() {
        let c = coords(4, 3);
        assert_eq!(c.resize_nearest(4, 3), c);
    }

    #[test]
    fn nearest_resize_introduces_no_new_colours() {
        // The property that makes nearest-neighbour right for an editor.
        let c = coords(4, 4);
        let big = c.resize_nearest(9, 7);
        assert_eq!((big.width(), big.height()), (9, 7));
        for p in big.pixels() {
            assert!(c.pixels().contains(p), "invented colour {p:?}");
        }
    }

    #[test]
    fn nearest_resize_to_zero_is_empty_rather_than_a_panic() {
        assert!(coords(4, 4).resize_nearest(0, 5).is_empty());
        assert!(Canvas::empty().resize_nearest(4, 4).pixels().len() == 16);
    }

    #[test]
    fn a_canvas_resize_keeps_the_picture_where_it_was() {
        let c = coords(3, 3);
        let bigger = c.resized_canvas(5, 4, BLUE);
        assert_eq!(bigger.get(2, 2), c.get(2, 2));
        assert_eq!(bigger.get(4, 3), Some(BLUE));
        let smaller = c.resized_canvas(2, 2, BLUE);
        assert_eq!((smaller.width(), smaller.height()), (2, 2));
        assert_eq!(smaller.get(1, 1), c.get(1, 1));
    }

    #[test]
    fn box_downscale_averages_each_region() {
        let mut c = Canvas::transparent(2, 2);
        c.set(0, 0, Color::rgba(0, 0, 0, 255));
        c.set(1, 0, Color::rgba(100, 100, 100, 255));
        c.set(0, 1, Color::rgba(100, 100, 100, 255));
        c.set(1, 1, Color::rgba(200, 200, 200, 255));
        let small = c.box_downscale(1, 1);
        assert_eq!(small.get(0, 0), Some(Color::rgba(100, 100, 100, 255)));
    }

    #[test]
    fn box_downscale_does_not_enlarge() {
        let c = coords(2, 2);
        assert_eq!(c.box_downscale(8, 8), c);
        assert!(c.box_downscale(0, 4).is_empty());
    }

    #[test]
    fn span_covers_every_source_pixel_exactly_once() {
        // If the spans left a gap, a downscale would drop detail; if they
        // overlapped, it would weight some pixels twice. Both look like
        // "slightly soft" output rather than a failure, so check the spans
        // directly. A ceiling on the upper end fails this at 10->3.
        for (src, dst) in [(10u32, 3u32), (7, 2), (100, 7), (5, 5), (1, 1), (256, 64)] {
            let mut covered = vec![0u32; src as usize];
            for d in 0..dst {
                let (lo, hi) = span(d, src, dst);
                assert!(lo < hi, "empty span at {d} for {src}->{dst}");
                for s in lo..hi {
                    covered[s as usize] += 1;
                }
            }
            assert!(
                covered.iter().all(|n| *n == 1),
                "{src}->{dst} coverage {covered:?}"
            );
        }
    }

    #[test]
    fn a_span_is_never_empty_even_when_enlarging_one_axis() {
        // `box_downscale` reduces only if *both* axes shrink, but a caller can
        // reduce one and enlarge the other, and then a floored span would be
        // empty and leave the pixel transparent.
        for d in 0..8 {
            let (lo, hi) = span(d, 2, 8);
            assert!(lo < hi, "empty span at {d} for 2->8");
            assert!(hi <= 2, "span past the source at {d}");
        }
    }
}
