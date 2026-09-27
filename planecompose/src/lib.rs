//! Compose DRM planes into a scanout image: crop, scale, place, blend.
//!
//! A DRM *plane* is one layer of what a display shows: the primary plane (the
//! desktop), an overlay (a video), a cursor. Each takes a *source rectangle* out
//! of its framebuffer and shows it at a *destination rectangle* on the display,
//! scaled to fit. The kernel's software backends -- the bootloader framebuffer
//! and virtio-gpu's 2D path -- have no hardware to do that, so until
//! design-decisions §976 they stored the rectangles, reported success, and
//! ignored them. This crate is the arithmetic that honours them, kept out of the
//! kernel so every pixel of it is tested on the host.
//!
//! # What it does
//!
//! [`compose`] writes the layers, bottom first, into a [`Target`], touching only
//! the pixels inside a *damage* rectangle -- a cursor move recomposes two cursor-
//! sized squares, not the screen. Scaling is nearest-neighbour, sampling the
//! source at the centre of each destination pixel ([`sample`]). Blending is
//! [`Blend::Opaque`] (the pixel replaces what is below) or
//! [`Blend::Premultiplied`] (Linux DRM's default "pixel blend mode":
//! `out = src + dst * (1 - alpha)`). Where no layer covers a pixel, the
//! background colour is written.
//!
//! [`check_layer`] is the other half: the validation an atomic commit runs
//! before anything is composed, so a rectangle a backend cannot show is refused
//! with an error rather than accepted and dropped.
//!
//! # Memory as the kernel holds it
//!
//! A kernel framebuffer is not one slice: it is a list of 16 KiB frames that
//! are not adjacent, and a row of pixels can run across two of them. So every
//! buffer here is [`Paged`] -- equal pages, the last possibly shorter -- and a
//! contiguous buffer is simply the one-page case. With the page size and the
//! pitch both multiples of four bytes, no pixel ever straddles two pages, which
//! [`compose`] checks before it writes anything.
//!
//! Every pixel format here is 32 bits per pixel, little-endian `B, G, R, A`
//! bytes -- DRM's XRGB8888 and ARGB8888. No allocation, no `std`, no panics:
//! every access is checked and every offset is computed with checked
//! arithmetic.

#![no_std]

/// Bytes per pixel of every format this composes.
pub const BPP: u32 = 4;

/// Largest framebuffer or target dimension accepted. It keeps every 16.16
/// coordinate inside a `u32` (`16384 << 16 == 1 << 30`) with room to add.
pub const MAX_DIMENSION: u32 = 16384;

/// One in 16.16 fixed point.
const ONE: u64 = 1 << 16;

/// A rectangle in whole pixels. `x` and `y` may be negative: a plane may hang
/// off the top or left edge of the display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    #[must_use]
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    /// True when the rectangle covers no pixel.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// The part of `self` that is also inside `other`, or `None`.
    #[must_use]
    pub fn intersect(self, other: Self) -> Option<Self> {
        // i64 holds any i32 plus any u32, so none of these can overflow; the
        // checked forms make that visible rather than assumed.
        let x0 = i64::from(self.x).max(i64::from(other.x));
        let y0 = i64::from(self.y).max(i64::from(other.y));
        let x1 = i64::from(self.x)
            .checked_add(i64::from(self.w))?
            .min(i64::from(other.x).checked_add(i64::from(other.w))?);
        let y1 = i64::from(self.y)
            .checked_add(i64::from(self.h))?
            .min(i64::from(other.y).checked_add(i64::from(other.h))?);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some(Self {
            x: i32::try_from(x0).ok()?,
            y: i32::try_from(y0).ok()?,
            w: u32::try_from(x1.checked_sub(x0)?).ok()?,
            h: u32::try_from(y1.checked_sub(y0)?).ok()?,
        })
    }

    /// The smallest rectangle covering both, or the non-empty one.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let x0 = i64::from(self.x).min(i64::from(other.x));
        let y0 = i64::from(self.y).min(i64::from(other.y));
        let x1 = i64::from(self.x)
            .saturating_add(i64::from(self.w))
            .max(i64::from(other.x).saturating_add(i64::from(other.w)));
        let y1 = i64::from(self.y)
            .saturating_add(i64::from(self.h))
            .max(i64::from(other.y).saturating_add(i64::from(other.h)));
        Self {
            x: i32::try_from(x0).unwrap_or(i32::MIN),
            y: i32::try_from(y0).unwrap_or(i32::MIN),
            w: u32::try_from(x1.saturating_sub(x0)).unwrap_or(u32::MAX),
            h: u32::try_from(y1.saturating_sub(y0)).unwrap_or(u32::MAX),
        }
    }

    /// True when row `y` lies inside.
    fn contains_row(self, y: i64) -> bool {
        let top = i64::from(self.y);
        top.checked_add(i64::from(self.h))
            .is_some_and(|bottom| y >= top && y < bottom)
    }
}

/// A source rectangle in 16.16 fixed point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl FixedRect {
    /// A rectangle of whole pixels.
    ///
    /// Refused rather than narrowed when a value is out of range: a request
    /// quietly capped to fit is a request shown other than as asked, which is
    /// what design-decisions §976 rules out. Every value at most
    /// [`MAX_DIMENSION`] (2^14) keeps each field, and `x + w`, inside `u32` in
    /// 16.16.
    ///
    /// # Errors
    ///
    /// [`Error::BadDimension`] if any value exceeds [`MAX_DIMENSION`].
    pub fn pixels(x: u32, y: u32, w: u32, h: u32) -> Result<Self, Error> {
        if x > MAX_DIMENSION || y > MAX_DIMENSION || w > MAX_DIMENSION || h > MAX_DIMENSION {
            return Err(Error::BadDimension);
        }
        // Cannot overflow: each value is at most 2^14, so each shift is at
        // most 2^30.
        let f = |v: u32| v << 16;
        Ok(Self {
            x: f(x),
            y: f(y),
            w: f(w),
            h: f(h),
        })
    }

    /// The whole of a `w` x `h` framebuffer.
    ///
    /// # Errors
    ///
    /// [`Error::BadDimension`] if either dimension exceeds [`MAX_DIMENSION`].
    pub fn whole(w: u32, h: u32) -> Result<Self, Error> {
        Self::pixels(0, 0, w, h)
    }
}

/// How a layer's pixels combine with what is beneath them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// The layer's pixel replaces the one below (XRGB8888, or a plane whose
    /// alpha is ignored).
    Opaque,
    /// `out = src + dst * (255 - src.alpha) / 255`, per channel, alpha too:
    /// Linux DRM's default for a plane with an alpha channel.
    Premultiplied,
}

/// Why a layer or buffer was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A source or destination rectangle covers no pixel.
    EmptyRect,
    /// The source rectangle leaves its framebuffer.
    SourceOutOfBounds,
    /// The scale from source to destination is beyond the backend's limits.
    ScaleOutOfRange,
    /// A buffer is shorter than its width, height and pitch say, or its pitch
    /// is narrower than a row of pixels.
    BufferTooSmall,
    /// A dimension beyond [`MAX_DIMENSION`], or zero.
    BadDimension,
    /// A page size or pitch that is not a multiple of four bytes, or a page
    /// other than the last that is not exactly the page size -- either would
    /// let a pixel straddle two pages.
    Unaligned,
}

/// The scaling a backend supports, as whole factors: `max_upscale = 8` allows
/// a destination up to eight times the source, `max_downscale = 8` down to an
/// eighth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_upscale: u32,
    pub max_downscale: u32,
}

impl Limits {
    /// What the software backends accept: nearest-neighbour has no quality
    /// floor, so the limit exists only to bound the cost of a pathological
    /// request.
    pub const SOFTWARE: Self = Self {
        max_upscale: 16,
        max_downscale: 16,
    };
    /// Unscaled only: a backend or plane that can place but not stretch.
    pub const UNSCALED: Self = Self {
        max_upscale: 1,
        max_downscale: 1,
    };
}

/// Read-only memory in pages of `page_size` bytes; the last page may be
/// shorter. A contiguous buffer is one page.
#[derive(Clone, Copy, Debug)]
pub struct Paged<'a> {
    pub pages: &'a [&'a [u8]],
    pub page_size: usize,
}

/// Writable memory in pages, as [`Paged`].
#[derive(Debug)]
pub struct PagedMut<'a, 'p> {
    pub pages: &'a mut [&'p mut [u8]],
    pub page_size: usize,
}

/// Total length of pages shaped as [`Paged`] requires.
fn paged_len<I: Iterator<Item = usize>>(lens: I, page_size: usize) -> Result<usize, Error> {
    if page_size == 0 || !page_size.is_multiple_of(4) {
        return Err(Error::Unaligned);
    }
    let mut total = 0usize;
    let mut short_seen = false;
    for len in lens {
        // A short page may only be the last one.
        if short_seen || len > page_size {
            return Err(Error::Unaligned);
        }
        short_seen = len < page_size;
        total = total.checked_add(len).ok_or(Error::BufferTooSmall)?;
    }
    Ok(total)
}

/// (page index, offset within it) of byte `off`.
fn locate(off: usize, page_size: usize) -> Option<(usize, usize)> {
    Some((off.checked_div(page_size)?, off.checked_rem(page_size)?))
}

impl Paged<'_> {
    fn pixel(&self, off: usize) -> Option<&[u8]> {
        let (page, at) = locate(off, self.page_size)?;
        self.pages.get(page)?.get(at..at.checked_add(4)?)
    }
}

impl PagedMut<'_, '_> {
    fn pixel_mut(&mut self, off: usize) -> Option<&mut [u8]> {
        let (page, at) = locate(off, self.page_size)?;
        self.pages.get_mut(page)?.get_mut(at..at.checked_add(4)?)
    }
}

/// A framebuffer's pixels, read-only.
#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    pub data: Paged<'a>,
    pub width: u32,
    pub height: u32,
    /// Bytes from one row to the next.
    pub pitch: u32,
}

/// The scanout image being composed.
#[derive(Debug)]
pub struct Target<'a, 'p> {
    pub data: PagedMut<'a, 'p>,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
}

/// One plane to compose.
#[derive(Clone, Copy, Debug)]
pub struct Layer<'a> {
    pub source: Source<'a>,
    pub src: FixedRect,
    pub dst: Rect,
    pub blend: Blend,
}

/// Byte offset of pixel column `col` in row `row` at `pitch` bytes per row.
fn offset(row: u32, pitch: u32, col: u32) -> Option<usize> {
    let at = u64::from(row)
        .checked_mul(u64::from(pitch))?
        .checked_add(u64::from(col).checked_mul(u64::from(BPP))?)?;
    usize::try_from(at).ok()
}

/// `count` pixels, in bytes.
fn pixels_len(count: u32) -> Option<usize> {
    usize::try_from(u64::from(count).checked_mul(u64::from(BPP))?).ok()
}

fn check_buffer(len: usize, width: u32, height: u32, pitch: u32) -> Result<(), Error> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(Error::BadDimension);
    }
    if !pitch.is_multiple_of(BPP) {
        return Err(Error::Unaligned);
    }
    let row = pixels_len(width).ok_or(Error::BufferTooSmall)?;
    if usize::try_from(pitch).map_or(true, |p| p < row) {
        return Err(Error::BufferTooSmall);
    }
    // The last row need only hold its pixels, not a whole pitch.
    let last_row = height.checked_sub(1).ok_or(Error::BadDimension)?;
    let need = offset(last_row, pitch, 0)
        .and_then(|start| start.checked_add(row))
        .ok_or(Error::BufferTooSmall)?;
    if len < need {
        return Err(Error::BufferTooSmall);
    }
    Ok(())
}

/// Validate one plane's rectangles, as an atomic commit must before it shows
/// anything: a request is either honoured or refused, never accepted and
/// ignored (design-decisions §976).
///
/// # Errors
///
/// [`Error::EmptyRect`], [`Error::SourceOutOfBounds`],
/// [`Error::ScaleOutOfRange`], or [`Error::BadDimension`] for a framebuffer
/// or destination with a zero or oversized dimension.
pub fn check_layer(
    fb_width: u32,
    fb_height: u32,
    src: FixedRect,
    dst: Rect,
    limits: Limits,
) -> Result<(), Error> {
    if fb_width == 0 || fb_height == 0 || fb_width > MAX_DIMENSION || fb_height > MAX_DIMENSION {
        return Err(Error::BadDimension);
    }
    if src.w == 0 || src.h == 0 || dst.is_empty() {
        return Err(Error::EmptyRect);
    }
    if dst.w > MAX_DIMENSION || dst.h > MAX_DIMENSION {
        return Err(Error::BadDimension);
    }
    // Every product below is at most 2^30 * 2^16, well inside u64.
    for (start, len, fb) in [(src.x, src.w, fb_width), (src.y, src.h, fb_height)] {
        let end = u64::from(start).checked_add(u64::from(len));
        let limit = u64::from(fb).checked_mul(ONE);
        if end.zip(limit).is_none_or(|(e, l)| e > l) {
            return Err(Error::SourceOutOfBounds);
        }
    }
    for (s, d) in [(src.w, dst.w), (src.h, dst.h)] {
        let d_fixed = u64::from(d).checked_mul(ONE);
        let up = u64::from(s).checked_mul(u64::from(limits.max_upscale));
        let down = d_fixed.and_then(|df| df.checked_mul(u64::from(limits.max_downscale)));
        match (d_fixed, up, down) {
            (Some(df), Some(up), Some(down)) if df <= up && u64::from(s) <= down => {}
            _ => return Err(Error::ScaleOutOfRange),
        }
    }
    Ok(())
}

/// The source coordinate, in whole pixels, that destination offset `i` (0-based
/// within a destination span of `dst_len`) samples: the source pixel under the
/// centre of the destination pixel. `src_start` and `src_len` are 16.16.
///
/// The definition, spelled out once so the fast and the general path cannot
/// disagree: `floor((src_start + (2i + 1) * src_len / (2 * dst_len)) / 65536)`.
#[must_use]
pub fn sample(src_start: u32, src_len: u32, dst_len: u32, i: u32) -> u32 {
    let centre = u64::from(i)
        .checked_mul(2)
        .and_then(|t| t.checked_add(1))
        .and_then(|t| t.checked_mul(u64::from(src_len)));
    let fixed = centre
        .zip(u64::from(dst_len).checked_mul(2))
        .and_then(|(num, den)| num.checked_div(den))
        .and_then(|off| off.checked_add(u64::from(src_start)))
        .unwrap_or(u64::from(src_start));
    u32::try_from(fixed >> 16).unwrap_or(u32::MAX)
}

/// The destination pixels a layer draws from `region` of its source: the
/// smallest rectangle holding every destination pixel whose [`sample`] falls
/// inside `region`, or `None` if none does. `region` is in whole source
/// pixels, `src` and `dst` are the layer's rectangles.
///
/// This is what a partial flush needs: a client that redrew part of its
/// buffer recomposes only the part of the display that shows it, including
/// under a scaled or moved layer.
#[must_use]
pub fn source_to_dest(region: Rect, src: FixedRect, dst: Rect) -> Option<Rect> {
    let (x0, x1) = span_to_dest(region.x, region.w, src.x, src.w, dst.w)?;
    let (y0, y1) = span_to_dest(region.y, region.h, src.y, src.h, dst.h)?;
    let w = x1.checked_sub(x0)?;
    let h = y1.checked_sub(y0)?;
    let x = i32::try_from(i64::from(dst.x).checked_add(i64::from(x0))?).ok()?;
    let y = i32::try_from(i64::from(dst.y).checked_add(i64::from(y0))?).ok()?;
    Some(Rect::new(x, y, w, h))
}

/// Along one axis: the destination offsets `[i0, i1)` whose samples land in
/// source pixels `[start, start + len)`. `sample` is monotonic in `i`, so the
/// set is one run; its ends are found by binary search over `[0, dst_len)`,
/// which keeps this exact for any scale instead of approximating the inverse.
fn span_to_dest(
    start: i32,
    len: u32,
    src_start: u32,
    src_len: u32,
    dst_len: u32,
) -> Option<(u32, u32)> {
    if len == 0 || dst_len == 0 {
        return None;
    }
    let lo = i64::from(start);
    let hi = lo.checked_add(i64::from(len))?;
    let at = |i: u32| i64::from(sample(src_start, src_len, dst_len, i));
    // The first i whose sample is >= lo, and the first whose sample is >= hi.
    let first_at_least = |bound: i64| -> u32 {
        let (mut a, mut b) = (0u32, dst_len);
        while a < b {
            let mid = a.saturating_add(b.saturating_sub(a).checked_div(2).unwrap_or(0));
            if at(mid) < bound {
                a = mid.saturating_add(1);
            } else {
                b = mid;
            }
        }
        a
    };
    let i0 = first_at_least(lo);
    let i1 = first_at_least(hi);
    if i0 >= i1 {
        return None;
    }
    Some((i0, i1))
}

/// One blended channel: `s + d * (255 - a) / 255`, rounded, saturating. The
/// product is at most `255 * 255 + 127`, far inside a u32.
fn over(s: u8, d: u8, a: u8) -> u8 {
    let inv = u32::from(a ^ 0xFF); // 255 - a, for a u8
    let rest = u32::from(d)
        .checked_mul(inv)
        .and_then(|p| p.checked_add(127))
        .map_or(0, |p| p / 255);
    u8::try_from(u32::from(s).saturating_add(rest)).unwrap_or(u8::MAX)
}

/// Compose `layers`, bottom first, into `target`, touching only the pixels
/// inside `damage` (clipped to the target). A pixel no layer covers is set to
/// `background` (`0xAARRGGBB`).
///
/// Every buffer is checked against its declared geometry before any pixel is
/// written, so an error leaves the target untouched.
///
/// # Errors
///
/// [`Error::BufferTooSmall`], [`Error::BadDimension`] or [`Error::Unaligned`]
/// for the target or any layer's source; the [`check_layer`] errors for a
/// layer's rectangles (with [`Limits::SOFTWARE`]).
pub fn compose(
    target: &mut Target<'_, '_>,
    damage: Rect,
    layers: &[Layer<'_>],
    background: u32,
) -> Result<(), Error> {
    let target_len = paged_len(
        target.data.pages.iter().map(|p| p.len()),
        target.data.page_size,
    )?;
    check_buffer(target_len, target.width, target.height, target.pitch)?;
    for layer in layers {
        let s = &layer.source;
        let len = paged_len(s.data.pages.iter().map(|p| p.len()), s.data.page_size)?;
        check_buffer(len, s.width, s.height, s.pitch)?;
        check_layer(s.width, s.height, layer.src, layer.dst, Limits::SOFTWARE)?;
    }
    let screen = Rect::new(0, 0, target.width, target.height);
    let Some(clip) = damage.intersect(screen) else {
        return Ok(());
    };
    let bg = background.to_le_bytes();
    // `clip` lies inside the screen, so its origin is non-negative.
    let (cx, cy) = (clip.x.unsigned_abs(), clip.y.unsigned_abs());
    for row in cy..cy.saturating_add(clip.h) {
        for col in cx..cx.saturating_add(clip.w) {
            let Some(px) = offset(row, target.pitch, col).and_then(|o| target.data.pixel_mut(o))
            else {
                return Err(Error::BufferTooSmall);
            };
            px.copy_from_slice(&bg);
        }
        for layer in layers {
            draw_row(target, clip, row, layer);
        }
    }
    Ok(())
}

/// Copy `len` bytes from `src` at `from` to `dst` at `to`, splitting the run at
/// both sides' page boundaries. A run that leaves either buffer stops there.
fn copy_run(
    src: &Paged<'_>,
    mut from: usize,
    dst: &mut PagedMut<'_, '_>,
    mut to: usize,
    mut len: usize,
) {
    while len > 0 {
        let (Some((sp, so)), Some((dp, dof))) =
            (locate(from, src.page_size), locate(to, dst.page_size))
        else {
            return;
        };
        let (Some(s_page), Some(d_page)) = (src.pages.get(sp), dst.pages.get_mut(dp)) else {
            return;
        };
        let chunk = len
            .min(s_page.len().saturating_sub(so))
            .min(d_page.len().saturating_sub(dof));
        if chunk == 0 {
            return;
        }
        let (Some(s), Some(d)) = (
            so.checked_add(chunk).and_then(|e| s_page.get(so..e)),
            dof.checked_add(chunk).and_then(|e| d_page.get_mut(dof..e)),
        ) else {
            return;
        };
        d.copy_from_slice(s);
        from = from.saturating_add(chunk);
        to = to.saturating_add(chunk);
        len = len.saturating_sub(chunk);
    }
}

/// Draw one layer's contribution to one target row, inside `clip`. A pixel
/// whose offset cannot be computed is skipped; after `compose`'s checks that
/// cannot happen.
fn draw_row(target: &mut Target<'_, '_>, clip: Rect, row: u32, layer: &Layer<'_>) {
    let d = layer.dst;
    if !d.contains_row(i64::from(row)) {
        return;
    }
    let Ok(row_i) = i32::try_from(row) else {
        return;
    };
    let Some(part) = Rect::new(d.x, row_i, d.w, 1).intersect(Rect::new(clip.x, row_i, clip.w, 1))
    else {
        return;
    };
    let diff = |a: i32, b: i32| u32::try_from(i64::from(a).checked_sub(i64::from(b))?).ok();
    // Offsets into the layer, and the target column; non-negative because
    // `part` lies inside both the layer and the clip (which is on screen).
    let (Some(dy), Some(first), Some(col0)) =
        (diff(row_i, d.y), diff(part.x, d.x), diff(part.x, 0))
    else {
        return;
    };
    let s = &layer.source;
    let sy = sample(layer.src.y, layer.src.h, d.h, dy);

    // The fast path: opaque and unscaled across, from a whole-pixel source
    // origin, is a plain copy of a contiguous run.
    if layer.blend == Blend::Opaque
        && u64::from(layer.src.w) == u64::from(d.w).saturating_mul(ONE)
        && layer.src.x & 0xFFFF == 0
    {
        let sx0 = (layer.src.x >> 16).saturating_add(first);
        if let (Some(from), Some(to), Some(len)) = (
            offset(sy, s.pitch, sx0),
            offset(row, target.pitch, col0),
            pixels_len(part.w),
        ) {
            copy_run(&s.data, from, &mut target.data, to, len);
        }
        return;
    }

    for k in 0..part.w {
        let sx = sample(layer.src.x, layer.src.w, d.w, first.saturating_add(k));
        let (Some(from), Some(to)) = (
            offset(sy, s.pitch, sx),
            offset(row, target.pitch, col0.saturating_add(k)),
        ) else {
            continue;
        };
        let Some(src) = s.data.pixel(from) else {
            continue;
        };
        let mut px = [0u8; 4];
        px.copy_from_slice(src);
        let Some(dst) = target.data.pixel_mut(to) else {
            continue;
        };
        match layer.blend {
            Blend::Opaque => dst.copy_from_slice(&px),
            Blend::Premultiplied => {
                let a = px[3];
                for (d, s) in dst.iter_mut().zip(px.iter()) {
                    *d = over(*s, *d, a);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
