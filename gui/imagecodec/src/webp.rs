//! WebP (RFC 9649): the RIFF container, and the pictures in it.
//!
//! A WebP file is a RIFF file whose chunks hold one of three things: a lossy
//! picture (`VP8 `, a VP8 video key frame), a lossless one (`VP8L`), or -- the
//! extended format, announced by a `VP8X` chunk -- either of those with a
//! separate alpha plane (`ALPH`), or an animation (`ANIM`, then an `ANMF`
//! chunk per frame), plus metadata this crate has no use for.
//!
//! # What this reads
//!
//! Everything, as libwebp reads it -- libwebp being the decoder in the
//! browsers, in Pillow, and behind nearly every other program that shows a
//! WebP:
//!
//! * the container, accepted or refused by libwebp's own three readers
//!   (`webp/riff.rs`), so a file this shows is a file they show;
//! * lossless pictures (`webp/lossless.rs`) and lossy ones
//!   (`webp/lossy.rs`), the latter with an alpha plane (`webp/alpha.rs`),
//!   decoded to exactly libwebp's pixels, damaged files included;
//! * animations, composited frame by frame as libwebp's animation decoder
//!   composites them ([`Animation`]). [`decode`] gives an animation's first
//!   frame, which is what a thumbnail or a still viewer shows.
//!
//! Whether a picture is shown with its alpha is decided as Pillow and Firefox
//! decide it, by what `WebPGetFeatures` says of the file: a lossless picture
//! whose header says it has no alpha, or an animation whose `VP8X` chunk does
//! not say it has any, is shown opaque whatever its pixels' alpha. Encoders
//! set those flags whenever there is alpha to show, so only a damaged or
//! hand-made file can tell the difference.
//!
//! # Hostile input
//!
//! Every chunk length is checked against the bytes present before anything is
//! read through it; a size in a header is checked against [`Limits`] before any
//! pixel buffer exists; and the decoders bound everything they allocate the
//! same way (see their modules).

use alloc::vec;
use alloc::vec::Vec;
use core::ops::Range;

use crate::{Image, ImageError, ImageResult, Limits};

mod alpha;
mod lossless;
mod lossy;
mod riff;

use riff::{Blend, Dispose};

/// Whether `bytes` is a RIFF file of type WEBP.
#[must_use]
pub fn is_webp(bytes: &[u8]) -> bool {
    bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WEBP")
}

/// A WebP's canvas size, from its headers alone, as libwebp's
/// `WebPGetFeatures` reads it: for a file cut short after its `VP8X` chunk,
/// that chunk's canvas.
///
/// # Errors
///
/// [`ImageError::Truncated`] or [`ImageError::Malformed`] for a file too short
/// or too broken to say.
pub fn dimensions(bytes: &[u8]) -> ImageResult<(u32, u32)> {
    let features = riff::features(bytes)?;
    Ok((features.width, features.height))
}

/// Decode a WebP: a still picture, or an animation's first frame on its
/// canvas.
///
/// # Errors
///
/// [`ImageError::TooLarge`] past `limits`; [`ImageError::Truncated`] or
/// [`ImageError::Malformed`] for a file libwebp would not show, including one
/// whose container libwebp refuses; otherwise what the bitstream's decoder
/// reports.
pub fn decode(bytes: &[u8], limits: Limits) -> ImageResult<Image> {
    let file = File::read(bytes)?;
    file.check(limits, 1)?;
    let frame = file
        .demux
        .frames
        .first()
        .ok_or(ImageError::Malformed("a WebP with no picture in it"))?;
    let picture = decode_frame(bytes, frame, limits)?;
    let rect = Rect::of(frame);
    let mut image = if rect == Rect::whole(file.width, file.height) {
        picture
    } else {
        // The first frame of an animation, which need not cover the canvas:
        // the rest of it is transparent, as libwebp leaves it.
        let mut canvas = Image {
            width: file.width,
            height: file.height,
            pixels: vec![0; file.pixels],
        };
        draw(&mut canvas.pixels, file.width as usize, &picture, rect);
        canvas
    };
    if !file.has_alpha {
        make_opaque(&mut image.pixels);
    }
    Ok(image)
}

/// Decode a WebP averaged down to fit `max_w` x `max_h`, by the same rule as
/// a PNG or a GIF thumbnail.
///
/// # Errors
///
/// As [`decode`].
pub fn decode_scaled(bytes: &[u8], limits: Limits, max_w: u32, max_h: u32) -> ImageResult<Image> {
    let image = decode(bytes, limits)?;
    if max_w == 0 || max_h == 0 {
        return Ok(image);
    }
    crate::scale::shrink_to_fit(image, max_w, max_h)
}

/// A file read as libwebp's animation decoder reads it before decoding
/// anything (`WebPAnimDecoderNew`): its features, then the demuxer's walk,
/// then room for the canvases.
struct File<'a> {
    demux: riff::Demuxer<'a>,
    width: u32,
    height: u32,
    pixels: usize,
    /// Whether it is shown with its alpha (see the module's docs).
    has_alpha: bool,
}

impl<'a> File<'a> {
    fn read(bytes: &'a [u8]) -> ImageResult<Self> {
        let features = riff::features(bytes)?;
        let demux = riff::Demuxer::read(bytes)?;
        let (width, height) = (demux.canvas_width, demux.canvas_height);
        // The demuxer keeps the product below 2^32, which a usize holds.
        let pixels = (width as usize).saturating_mul(height as usize);
        Ok(Self {
            demux,
            width,
            height,
            pixels,
            has_alpha: features.has_alpha,
        })
    }

    /// Check that `canvases` canvases of the file's size fit in `limits`.
    fn check(&self, limits: Limits, canvases: usize) -> ImageResult<()> {
        if self.pixels as u64 > limits.max_pixels {
            return Err(ImageError::TooLarge {
                pixels: self.pixels as u64,
                limit: limits.max_pixels,
            });
        }
        let canvas_bytes = self.pixels.saturating_mul(4).saturating_mul(canvases);
        if canvas_bytes > limits.max_decompressed_bytes {
            return Err(ImageError::TooLarge {
                pixels: canvas_bytes as u64,
                limit: limits.max_decompressed_bytes as u64,
            });
        }
        Ok(())
    }
}

/// Decode one frame's picture at its own size, as `WebPDecode` decodes the
/// bytes the demuxer hands it: its headers read again, then the bitstream.
fn decode_frame(bytes: &[u8], frame: &riff::Frame, limits: Limits) -> ImageResult<Image> {
    let fragment = frame.fragment(bytes);
    let headers = riff::fragment_headers(fragment)?;
    // The bitstream's decoder is given everything from its start to the end
    // of the fragment, which includes the byte padding an odd-length chunk:
    // a stream that overruns its chunk by one reads it, as in libwebp.
    let stream = fragment.get(headers.offset..).unwrap_or(&[]);
    let image = if headers.is_lossless {
        let (width, height, pixels) = lossless::decode(stream, limits)?;
        Image {
            width,
            height,
            pixels,
        }
    } else {
        let decoded = lossy::decode(stream, headers.compressed_size, limits)?;
        let width = u32::try_from(decoded.width)
            .map_err(|_| ImageError::Malformed("a VP8 frame of an impossible size"))?;
        let height = u32::try_from(decoded.height)
            .map_err(|_| ImageError::Malformed("a VP8 frame of an impossible size"))?;
        let plane = match headers.alpha {
            Some((at, size)) => {
                let chunk = fragment
                    .get(at..at.saturating_add(size))
                    .ok_or(ImageError::Truncated)?;
                Some(alpha::decode(chunk, width, height, limits)?)
            }
            None => None,
        };
        Image {
            width,
            height,
            pixels: decoded.to_argb(plane.as_deref()),
        }
    };
    // The demuxer took the frame's size from this same header, so the two
    // cannot differ; checked all the same, since the canvas is drawn by it.
    if (image.width, image.height) != (frame.width, frame.height) {
        return Err(ImageError::Malformed(
            "a frame whose picture is not the size its header says",
        ));
    }
    Ok(image)
}

/// A frame's rectangle on its canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Rect {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

impl Rect {
    const fn of(frame: &riff::Frame) -> Self {
        Self {
            x: frame.x_offset as usize,
            y: frame.y_offset as usize,
            width: frame.width as usize,
            height: frame.height as usize,
        }
    }

    const fn whole(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width: width as usize,
            height: height as usize,
        }
    }

    /// Whether it is the whole canvas (`IsFullFrame`, which compares sizes
    /// only: a rectangle the canvas's size inside it has no offset).
    const fn is_full(self, canvas: Self) -> bool {
        self.width == canvas.width && self.height == canvas.height
    }

    /// Its part of row `y` of a canvas `stride` pixels wide.
    fn row(self, stride: usize, y: usize) -> Option<Range<usize>> {
        span(stride, y, self.x, self.width)
    }
}

/// Where `width` pixels from column `left` of row `y` are in a canvas
/// `stride` pixels wide.
fn span(stride: usize, y: usize, left: usize, width: usize) -> Option<Range<usize>> {
    let start = y.checked_mul(stride)?.checked_add(left)?;
    Some(start..start.checked_add(width)?)
}

/// Copy `picture` into `canvas` (`stride` pixels wide) at `rect`, replacing
/// what was there, alpha and all, as libwebp's decode into the canvas does.
fn draw(canvas: &mut [u32], stride: usize, picture: &Image, rect: Rect) {
    if rect.width == 0 {
        return;
    }
    for (row, line) in picture.pixels.chunks(rect.width).enumerate() {
        let target = rect
            .row(stride, rect.y.saturating_add(row))
            .and_then(|range| canvas.get_mut(range));
        if let Some(target) = target {
            target.copy_from_slice(line);
        }
    }
}

/// Force every pixel's alpha to opaque: how a picture whose file says it has
/// no alpha is shown.
fn make_opaque(pixels: &mut [u32]) {
    for pixel in pixels {
        *pixel |= 0xFF00_0000;
    }
}

/// `BlendPixelNonPremult` (libwebp's `anim_decode.c`): `src` over `dst`,
/// neither premultiplied, in libwebp's integer arithmetic -- which is not
/// exactly the ideal formula, and is what the frames are shown with.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "alphas are 8-bit, so the blended alpha is 1..=255 and each channel's sum times the scale stays below 2^32 (libwebp asserts both)"
)]
fn blend_pixel(src: u32, dst: u32) -> u32 {
    let src_a = src >> 24;
    if src_a == 0 {
        return dst;
    }
    let dst_a = dst >> 24;
    // An approximation of dst_a * (255 - src_a) / 255.
    let dst_factor_a = (dst_a * (256 - src_a)) >> 8;
    let blend_a = src_a + dst_factor_a;
    let scale = (1 << 24) / blend_a;
    let channel = |shift: u32| {
        let s = (src >> shift) & 0xFF;
        let d = (dst >> shift) & 0xFF;
        ((s * src_a + d * dst_factor_a) * scale) >> 24
    };
    (blend_a << 24) | (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

/// `BlendPixelRowNonPremult`: blend every pixel of `src` that is not opaque
/// over the pixel under it in `dst`.
fn blend_row(src: &mut [u32], dst: &[u32]) {
    for (s, &d) in src.iter_mut().zip(dst) {
        if *s >> 24 != 0xFF {
            *s = blend_pixel(*s, d);
        }
    }
}

/// `FindBlendRangeAtRow`: the parts of row `y` of `src` that are outside
/// `dst`, as up to two runs of (left, width); a width of 0 is no run.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "rectangles are inside a canvas of at most 2^24 pixels a side, and each subtraction is guarded by the comparison before it"
)]
const fn blend_ranges(src: Rect, dst: Rect, y: usize) -> [(usize, usize); 2] {
    let src_max_x = src.x + src.width;
    let dst_max_x = dst.x + dst.width;
    let dst_max_y = dst.y + dst.height;
    if y < dst.y || y >= dst_max_y || src.x >= dst_max_x || src_max_x <= dst.x {
        return [(src.x, src.width), (0, 0)];
    }
    let mut ranges = [(0, 0); 2];
    if src.x < dst.x {
        ranges[0] = (src.x, dst.x - src.x);
    }
    if src_max_x > dst_max_x {
        ranges[1] = (dst_max_x, src_max_x - dst_max_x);
    }
    ranges
}

/// How many times an animation plays, as its `ANIM` chunk says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    /// A loop count of zero: play forever.
    Forever,
    /// Play this many times in all, the first included: the format counts
    /// plays, not repeats, which GIF leaves unsaid (see
    /// [`crate::gif::Repeat`]). A still picture plays once.
    Times(u16),
}

/// One frame of an [`Animation`].
#[derive(Clone, Copy, Debug)]
pub struct Frame<'a> {
    /// The canvas, with this frame composited onto it.
    pub image: &'a Image,
    /// How long to show it, in milliseconds, as the file says.
    pub duration_ms: u32,
}

impl Frame<'_> {
    /// How long a browser shows this frame, in milliseconds: the file's
    /// duration, except that 10 ms or less is shown for 100 ms -- the rule
    /// browsers apply to every animated format, GIF's
    /// ([`crate::gif::Frame::display_delay_ms`]) included, for files that ask
    /// for "as fast as you can" and flicker when taken at their word.
    #[must_use]
    pub const fn display_duration_ms(&self) -> u32 {
        if self.duration_ms <= 10 {
            100
        } else {
            self.duration_ms
        }
    }
}

/// What the animation decoder remembers of the frame before (`prev_iter`).
#[derive(Clone, Copy, Debug)]
struct Previous {
    rect: Rect,
    dispose: Dispose,
    key: bool,
}

/// A WebP's frames, composited one at a time onto one canvas, exactly as
/// libwebp's animation decoder (`WebPAnimDecoder`) composites them. A still
/// picture is an animation of one frame.
///
/// ```ignore
/// let mut animation = Animation::new(&bytes, Limits::default())?;
/// while let Some(frame) = animation.next_frame()? {
///     show(frame.image, frame.display_duration_ms());
/// }
/// animation.rewind(); // and again, as `animation.repeat()` says
/// ```
///
/// A frame is decoded whole into its own buffer before the canvas is touched,
/// so a frame that fails to decode leaves the canvas as the frame before left
/// it, and asking again fails again.
pub struct Animation<'a> {
    bytes: &'a [u8],
    limits: Limits,
    frames: Vec<riff::Frame>,
    repeat: Repeat,
    /// Whether the file is shown with its alpha (see the module's docs).
    has_alpha: bool,
    /// The canvas as the last frame left it (`curr_frame`), alpha and all:
    /// the next frame is blended with this, so it is never made opaque.
    canvas: Image,
    /// The canvas after the last frame's disposal (`prev_frame_disposed`).
    disposed: Vec<u32>,
    /// For a file shown without its alpha, the canvas as shown: opaque.
    shown: Option<Image>,
    /// The index of the next frame.
    next: usize,
    previous: Previous,
}

impl<'a> Animation<'a> {
    /// Read the file's structure -- its canvas, its frames, how often it
    /// loops -- without decoding any picture, and make the canvases.
    ///
    /// # Errors
    ///
    /// [`ImageError::TooLarge`] if the canvases are past `limits`;
    /// [`ImageError::Truncated`] or [`ImageError::Malformed`] for a file
    /// libwebp would not show.
    pub fn new(bytes: &'a [u8], limits: Limits) -> ImageResult<Self> {
        let file = File::read(bytes)?;
        // The canvas, the disposed canvas, and for a file shown opaque the
        // canvas as shown.
        file.check(limits, if file.has_alpha { 2 } else { 3 })?;
        let repeat = match u16::try_from(file.demux.loop_count) {
            Ok(0) => Repeat::Forever,
            Ok(times) => Repeat::Times(times),
            // A 16-bit field; cannot happen.
            Err(_) => Repeat::Forever,
        };
        let blank = Image {
            width: file.width,
            height: file.height,
            pixels: vec![0; file.pixels],
        };
        let shown = (!file.has_alpha).then(|| Image {
            width: file.width,
            height: file.height,
            pixels: vec![0xFF00_0000; file.pixels],
        });
        Ok(Self {
            bytes,
            limits,
            frames: file.demux.frames,
            repeat,
            has_alpha: file.has_alpha,
            disposed: vec![0; file.pixels],
            canvas: blank,
            shown,
            next: 0,
            previous: Previous {
                rect: Rect::default(),
                dispose: Dispose::None,
                key: false,
            },
        })
    }

    /// The canvas's width and height.
    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.canvas.width, self.canvas.height)
    }

    /// How many frames the file holds: more than one is an animation.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// How many times the file asks to be played.
    #[must_use]
    pub const fn repeat(&self) -> Repeat {
        self.repeat
    }

    /// Whether the frames are shown with their alpha. When not, every frame's
    /// image is opaque (see the module's docs).
    #[must_use]
    pub const fn has_alpha(&self) -> bool {
        self.has_alpha
    }

    /// The next frame, or `None` after the last.
    ///
    /// # Errors
    ///
    /// [`ImageError::TooLarge`] for a picture past the limits; otherwise what
    /// its decoder reports for a picture libwebp would not decode either.
    pub fn next_frame(&mut self) -> ImageResult<Option<Frame<'_>>> {
        let Some(frame) = self.frames.get(self.next).copied() else {
            return Ok(None);
        };
        let picture = decode_frame(self.bytes, &frame, self.limits)?;
        let canvas = Rect::whole(self.canvas.width, self.canvas.height);
        let stride = canvas.width;
        let rect = Rect::of(&frame);
        let first = self.next == 0;

        // IsKeyFrame: a frame that owes nothing to the canvas before it.
        let key = first
            || ((!frame.has_alpha || frame.blend == Blend::NoBlend) && rect.is_full(canvas))
            || (self.previous.dispose == Dispose::Background
                && (self.previous.rect.is_full(canvas) || self.previous.key));
        if key {
            self.canvas.pixels.fill(0);
        } else {
            self.canvas.pixels.copy_from_slice(&self.disposed);
        }
        draw(&mut self.canvas.pixels, stride, &picture, rect);

        // The frame's pixels that are not opaque were decoded over nothing;
        // blend them with what was under them. Where the frame before was
        // disposed to transparent, what was under them is nothing, and libwebp
        // leaves them unblended -- which is not quite the same as blending
        // them with transparency, so it is done the same way here.
        if !first && frame.blend == Blend::Blend && !key {
            for y in rect.y..rect.y.saturating_add(rect.height) {
                let runs = match self.previous.dispose {
                    Dispose::None => [(rect.x, rect.width), (0, 0)],
                    Dispose::Background => blend_ranges(rect, self.previous.rect, y),
                };
                for (left, width) in runs {
                    let Some(range) = span(stride, y, left, width).filter(|_| width > 0) else {
                        continue;
                    };
                    if let (Some(src), Some(dst)) = (
                        self.canvas.pixels.get_mut(range.clone()),
                        self.disposed.get(range),
                    ) {
                        blend_row(src, dst);
                    }
                }
            }
        }

        // Dispose of it for the next frame.
        self.previous = Previous {
            rect,
            dispose: frame.dispose,
            key,
        };
        self.disposed.copy_from_slice(&self.canvas.pixels);
        if frame.dispose == Dispose::Background {
            for y in rect.y..rect.y.saturating_add(rect.height) {
                if let Some(line) = rect.row(stride, y).and_then(|r| self.disposed.get_mut(r)) {
                    line.fill(0);
                }
            }
        }
        self.next = self.next.saturating_add(1);

        let image = match &mut self.shown {
            Some(shown) => {
                shown.pixels.copy_from_slice(&self.canvas.pixels);
                make_opaque(&mut shown.pixels);
                &*shown
            }
            None => &self.canvas,
        };
        Ok(Some(Frame {
            image,
            duration_ms: frame.duration,
        }))
    }

    /// Back to before the first frame, with an empty canvas.
    pub fn rewind(&mut self) {
        self.next = 0;
        self.previous = Previous {
            rect: Rect::default(),
            dispose: Dispose::None,
            key: false,
        };
        self.canvas.pixels.fill(0);
        self.disposed.fill(0);
        if let Some(shown) = &mut self.shown {
            shown.pixels.fill(0xFF00_0000);
        }
    }

    /// The canvas as last shown, given up.
    #[must_use]
    pub fn into_canvas(self) -> Image {
        self.shown.unwrap_or(self.canvas)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "tests"
    )]

    use super::*;

    /// libwebp's blend, transcribed with its C types, to hold the port to.
    fn reference_blend(src: u32, dst: u32) -> u32 {
        let src_a = (src >> 24) as u8;
        if src_a == 0 {
            return dst;
        }
        let dst_a = (dst >> 24) as u8;
        let dst_factor_a = ((u32::from(dst_a) * (256 - u32::from(src_a))) >> 8) as u8;
        let blend_a = src_a + dst_factor_a;
        let scale = (1u32 << 24) / u32::from(blend_a);
        let channel = |shift: u32| -> u8 {
            let s = ((src >> shift) & 0xFF) as u8;
            let d = ((dst >> shift) & 0xFF) as u8;
            let unscaled = u32::from(s) * u32::from(src_a) + u32::from(d) * u32::from(dst_factor_a);
            ((unscaled * scale) >> 24) as u8
        };
        (u32::from(blend_a) << 24)
            | (u32::from(channel(16)) << 16)
            | (u32::from(channel(8)) << 8)
            | u32::from(channel(0))
    }

    #[test]
    fn blending_is_libwebps_arithmetic_for_every_pair_of_alphas() {
        let colours = [0x00_00_00, 0xFF_FF_FF, 0x12_80_FE, 0xFE_01_7F];
        for src_a in 0..=255u32 {
            for dst_a in 0..=255u32 {
                for (i, &s) in colours.iter().enumerate() {
                    let d = colours[(i + 1) % colours.len()];
                    let src = (src_a << 24) | s;
                    let dst = (dst_a << 24) | d;
                    assert_eq!(blend_pixel(src, dst), reference_blend(src, dst));
                }
            }
        }
        // Not the ideal formula: half-transparent white over transparent
        // loses a level.
        assert_eq!(blend_pixel(0x03FF_FFFF, 0), 0x03FE_FEFE);
        // Nothing over something is the something; opaque is left alone.
        assert_eq!(blend_pixel(0x0012_3456, 0x8065_4321), 0x8065_4321);
        let mut row = [0xFF12_3456, 0x0000_0000];
        blend_row(&mut row, &[0x8000_0000, 0x4011_2233]);
        assert_eq!(row, [0xFF12_3456, 0x4011_2233]);
    }

    #[test]
    fn the_blend_ranges_are_the_parts_outside_the_rectangle_before() {
        let rect = |x, y, width, height| Rect {
            x,
            y,
            width,
            height,
        };
        let src = rect(2, 2, 6, 3);
        // Rows the rectangle before misses, and one beside it: all of it.
        assert_eq!(blend_ranges(src, rect(0, 5, 10, 2), 3), [(2, 6), (0, 0)]);
        assert_eq!(blend_ranges(src, rect(8, 0, 2, 10), 3), [(2, 6), (0, 0)]);
        // Inside it: nothing.
        assert_eq!(blend_ranges(src, rect(0, 0, 10, 10), 3), [(0, 0), (0, 0)]);
        // Straddling it: left and right.
        assert_eq!(blend_ranges(src, rect(4, 0, 2, 10), 3), [(2, 2), (6, 2)]);
        assert_eq!(blend_ranges(src, rect(1, 0, 4, 10), 3), [(0, 0), (5, 3)]);
    }

    /// Every animated fixture in `tests/data`.
    macro_rules! animations {
        ($($name:literal),* $(,)?) => {
            [$(($name, include_bytes!(concat!("../tests/data/", $name, ".webp")).as_slice())),*]
        };
    }

    const ANIMATIONS: [(&str, &[u8]); 9] = animations![
        "webp_anim_clear",
        "webp_anim_container",
        "webp_anim_key_chain",
        "webp_anim_lossless",
        "webp_anim_lossy",
        "webp_anim_mixed",
        "webp_anim_opaque",
        "webp_anim_rules",
        "webp_anim_unflagged",
    ];

    /// What the animated fixtures do between them.
    #[derive(Debug, Default)]
    struct Census {
        first_short: bool,
        key_opaque_whole: bool,
        key_unblended_whole: bool,
        key_after_cleared_whole: bool,
        key_after_cleared_key: bool,
        blended_over_kept: bool,
        blended_beside_cleared: [bool; 3],
        unblended_not_key: bool,
        disposed: bool,
        lossless: bool,
        lossy_with_alpha: bool,
        lossy_opaque: bool,
        alpha_unflagged: bool,
    }

    #[test]
    fn the_animation_fixtures_between_them_use_every_rule() {
        let mut census = Census::default();
        for (name, file) in ANIMATIONS {
            let features = riff::features(file).unwrap();
            let demux = riff::Demuxer::read(file).unwrap();
            let canvas = Rect::whole(demux.canvas_width, demux.canvas_height);
            let mut previous: Option<Previous> = None;
            for frame in &demux.frames {
                let rect = Rect::of(frame);
                let whole = rect.is_full(canvas);
                // Each of IsKeyFrame's reasons, in its order.
                let key = match previous {
                    None => {
                        census.first_short |= !whole;
                        true
                    }
                    Some(_) if whole && !frame.has_alpha => {
                        census.key_opaque_whole = true;
                        true
                    }
                    Some(_) if whole && frame.blend == Blend::NoBlend => {
                        census.key_unblended_whole = true;
                        true
                    }
                    Some(before) if before.dispose == Dispose::Background => {
                        census.key_after_cleared_whole |= before.rect.is_full(canvas);
                        census.key_after_cleared_key |= !before.rect.is_full(canvas) && before.key;
                        before.rect.is_full(canvas) || before.key
                    }
                    Some(_) => false,
                };
                if let (false, Some(before)) = (key, previous) {
                    match (frame.blend, before.dispose) {
                        (Blend::NoBlend, _) => census.unblended_not_key = true,
                        (Blend::Blend, Dispose::None) => census.blended_over_kept = true,
                        (Blend::Blend, Dispose::Background) => {
                            for y in rect.y..rect.y + rect.height {
                                let [left, right] = blend_ranges(rect, before.rect, y);
                                let seen = &mut census.blended_beside_cleared;
                                seen[0] |= left == (rect.x, rect.width);
                                seen[1] |= left.1 > 0 && left.1 < rect.width;
                                seen[2] |= right.1 > 0;
                            }
                        }
                    }
                }
                census.disposed |= frame.dispose == Dispose::Background;
                let headers = riff::fragment_headers(frame.fragment(file)).unwrap();
                match (headers.is_lossless, headers.alpha) {
                    (true, _) => census.lossless = true,
                    (false, Some(_)) => census.lossy_with_alpha = true,
                    (false, None) => census.lossy_opaque = true,
                }
                census.alpha_unflagged |= !features.has_alpha && frame.has_alpha;
                previous = Some(Previous {
                    rect,
                    dispose: frame.dispose,
                    key,
                });
            }
            assert!(demux.frames.len() > 1, "{name}");
        }
        let Census {
            first_short,
            key_opaque_whole,
            key_unblended_whole,
            key_after_cleared_whole,
            key_after_cleared_key,
            blended_over_kept,
            blended_beside_cleared,
            unblended_not_key,
            disposed,
            lossless,
            lossy_with_alpha,
            lossy_opaque,
            alpha_unflagged,
        } = census;
        assert!(
            first_short
                && key_opaque_whole
                && key_unblended_whole
                && key_after_cleared_whole
                && key_after_cleared_key
                && blended_over_kept
                && blended_beside_cleared.iter().all(|&b| b)
                && unblended_not_key
                && disposed
                && lossless
                && lossy_with_alpha
                && lossy_opaque
                && alpha_unflagged,
            "{census:?}"
        );
    }

    #[test]
    fn a_short_duration_is_shown_as_browsers_show_it() {
        let image = Image {
            width: 1,
            height: 1,
            pixels: vec![0],
        };
        let frame = |duration_ms| Frame {
            image: &image,
            duration_ms,
        };
        assert_eq!(frame(0).display_duration_ms(), 100);
        assert_eq!(frame(10).display_duration_ms(), 100);
        assert_eq!(frame(11).display_duration_ms(), 11);
        assert_eq!(frame(5000).display_duration_ms(), 5000);
    }
}
