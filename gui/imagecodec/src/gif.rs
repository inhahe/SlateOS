//! GIF (87a and 89a): pictures of up to 256 colours, LZW-compressed, and the
//! animations made of them.
//!
//! # What a GIF is
//!
//! A six-byte signature, then a *logical screen*: the canvas size and, usually,
//! a global colour table. Then blocks, until a trailer byte: *images* -- each a
//! rectangle of colour indices somewhere on the screen, with an optional colour
//! table of its own, LZW-compressed (`gif/lzw.rs`) -- and *extensions*, of
//! which two matter here. The graphic control extension gives the next image a
//! transparent index, a delay, and what to do with its rectangle once it has
//! been shown (its *disposal*); the NETSCAPE application extension says how
//! many times an animation plays.
//!
//! # A frame is a canvas, not an image
//!
//! An animation is not a list of pictures but a canvas each image is drawn
//! onto, over whatever the ones before it left there. So a frame is the canvas
//! after an image is drawn, and before the next is drawn the previous image's
//! disposal is applied: leave its rectangle (1, and 0, "unspecified"), clear it
//! (2), or put back what was under it (3). [`Animation`] does that compositing
//! one frame at a time, reusing one canvas; [`decode`] is its first frame,
//! which is what a thumbnail and a still viewer show, and what the format says
//! a viewer that does not animate should show.
//!
//! # Where decoders disagree, this does what browsers do
//!
//! The format leaves several things to the decoder, and the decoders people see
//! GIFs through answer them one way, which is the answer here:
//!
//! - **The canvas starts transparent, and disposal 2 clears to transparent.**
//!   The header's background colour is ignored, as every browser ignores it.
//!   (Pillow paints it: the tests say where that makes the two differ.)
//! - **Disposal 3 on the first frame** puts back the empty canvas.
//! - **A first image larger than the logical screen enlarges the canvas** to
//!   hold it, as Firefox and Pillow do; later images are clipped to the canvas.
//! - **A delay of 0 or 1 hundredths of a second is shown for a tenth of one**
//!   ([`Frame::display_delay_ms`]): no browser shows a frame faster, and files
//!   are written for browsers.
//! - **An index past the end of its colour table** is opaque black, and an
//!   image with no colour table at all reads its indices as greys, as Pillow
//!   does.
//! - **Data that stops short** leaves the rest of that image undrawn, over
//!   whatever the canvas held there, and is still a frame: a GIF cut off while
//!   downloading shows what arrived.
//! - **An unknown block** ends the file where it stands; everything before it
//!   is kept.
//!
//! # Hostile input
//!
//! As for the other decoders: nothing here panics for any input, every read is
//! bounded by the bytes actually present, and the canvas, the frame buffer and
//! the rectangle saved for disposal 3 are all checked against [`Limits`] before
//! they exist. The dictionary is a fixed 4096 entries whatever the file claims.

use alloc::vec;
use alloc::vec::Vec;

use crate::{Image, ImageError, ImageResult, Limits};

mod lzw;

use lzw::{SubBlocks, Table};

/// Whether `bytes` starts with a GIF signature.
#[must_use]
pub fn is_gif(bytes: &[u8]) -> bool {
    bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")
}

/// A picture's size: the logical screen, enlarged to hold the first image if
/// that is bigger, which is the canvas [`decode`] returns.
///
/// # Errors
///
/// [`ImageError::Truncated`] or [`ImageError::Malformed`] for a file too short
/// or too broken to say.
pub fn dimensions(bytes: &[u8]) -> ImageResult<(u32, u32)> {
    let layout = Layout::read(bytes)?;
    Ok((to_u32(layout.width)?, to_u32(layout.height)?))
}

/// The first frame: the first image drawn onto the canvas.
///
/// # Errors
///
/// [`ImageError::TooLarge`] past `limits`; [`ImageError::Malformed`] for a file
/// with no image in it; [`ImageError::Truncated`] for one too short to have a
/// screen.
pub fn decode(bytes: &[u8], limits: Limits) -> ImageResult<Image> {
    let mut animation = Animation::new(bytes, limits)?;
    if animation.next_frame()?.is_none() {
        return Err(ImageError::Malformed("a GIF with no image in it"));
    }
    Ok(animation.into_canvas())
}

/// The first frame, averaged down to fit `max_w` x `max_h`.
///
/// The frame is composited at its own size first -- it is not a picture until
/// it is on its canvas -- and GIFs are small enough that this costs nothing a
/// thumbnailer would notice; then it is averaged by the same rule a PNG
/// thumbnail is.
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

/// How many times an animation plays, as the file says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    /// No loop count at all: play once.
    Once,
    /// A loop count of zero: play forever.
    Forever,
    /// A loop count of `n`. Decoders differ on whether that counts the first
    /// play, so it is reported as written.
    Count(u16),
}

/// One frame of an [`Animation`].
#[derive(Clone, Copy, Debug)]
pub struct Frame<'a> {
    /// The canvas, with this frame's image drawn onto it.
    pub image: &'a Image,
    /// How long to show it for, in hundredths of a second, as the file says.
    pub delay_cs: u16,
}

impl Frame<'_> {
    /// How long a browser shows this frame, in milliseconds: the file's delay,
    /// except that 0 and 1 hundredths are shown for a tenth of a second.
    ///
    /// Browsers settled on that because files that say "as fast as you can"
    /// were written for decoders that could not go faster than about ten
    /// frames a second; played at their word they flicker.
    #[must_use]
    pub fn display_delay_ms(&self) -> u32 {
        if self.delay_cs <= 1 {
            100
        } else {
            u32::from(self.delay_cs).saturating_mul(10)
        }
    }
}

/// A GIF's frames, composited one at a time onto one canvas.
///
/// ```ignore
/// let mut animation = Animation::new(&bytes, Limits::default())?;
/// while let Some(frame) = animation.next_frame()? {
///     show(frame.image, frame.display_delay_ms());
/// }
/// animation.rewind(); // and again, as `animation.repeat()` says
/// ```
pub struct Animation<'a> {
    bytes: &'a [u8],
    limits: Limits,
    width: usize,
    height: usize,
    global: Option<Palette>,
    /// Where the first block after the screen and its colour table starts.
    start: usize,
    /// Where the next block starts.
    at: usize,
    frames: usize,
    repeat: Repeat,
    canvas: Image,
    /// What the frame last shown asked to have done with its rectangle before
    /// the next is drawn.
    pending: Option<Pending>,
    /// The frame being decoded, as colour indices in the order the file sends
    /// them; kept between frames so an animation allocates it once.
    indices: Vec<u8>,
    table: Table,
}

impl<'a> Animation<'a> {
    /// Read the file's structure -- its screen, how many images it has, how
    /// often it loops -- without decoding any image, and make the canvas.
    ///
    /// # Errors
    ///
    /// [`ImageError::TooLarge`] if the canvas is past `limits`;
    /// [`ImageError::Truncated`] or [`ImageError::Malformed`] for a file with
    /// no screen to draw on.
    pub fn new(bytes: &'a [u8], limits: Limits) -> ImageResult<Self> {
        let layout = Layout::read(bytes)?;
        let pixels = layout.width.saturating_mul(layout.height);
        if pixels as u64 > limits.max_pixels {
            return Err(ImageError::TooLarge {
                pixels: pixels as u64,
                limit: limits.max_pixels,
            });
        }
        let canvas_bytes = pixels.saturating_mul(4);
        if canvas_bytes > limits.max_decompressed_bytes {
            return Err(ImageError::TooLarge {
                pixels: canvas_bytes as u64,
                limit: limits.max_decompressed_bytes as u64,
            });
        }
        Ok(Self {
            bytes,
            limits,
            width: layout.width,
            height: layout.height,
            global: layout.global,
            start: layout.start,
            at: layout.start,
            frames: layout.frames,
            repeat: layout.repeat,
            canvas: Image {
                width: to_u32(layout.width)?,
                height: to_u32(layout.height)?,
                pixels: vec![0; pixels],
            },
            pending: None,
            indices: Vec::new(),
            table: Table::new(),
        })
    }

    /// The canvas's width and height.
    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.canvas.width, self.canvas.height)
    }

    /// How many images the file holds: more than one is an animation.
    #[must_use]
    pub const fn frame_count(&self) -> usize {
        self.frames
    }

    /// How many times the file asks to be played.
    #[must_use]
    pub const fn repeat(&self) -> Repeat {
        self.repeat
    }

    /// The next frame, or `None` after the last.
    ///
    /// # Errors
    ///
    /// [`ImageError::TooLarge`] for an image past the limits. A frame whose
    /// data is broken or cut short is not an error: what it had is drawn.
    pub fn next_frame(&mut self) -> ImageResult<Option<Frame<'_>>> {
        let mut control = Control::default();
        loop {
            match next_block(self.bytes, &mut self.at) {
                Block::Control(found) => control = found,
                Block::Image(image) => {
                    self.dispose();
                    self.draw(&image, control)?;
                    return Ok(Some(Frame {
                        image: &self.canvas,
                        delay_cs: control.delay_cs,
                    }));
                }
                Block::Repeat(_) | Block::Other => {}
                Block::End => return Ok(None),
            }
        }
    }

    /// Back to before the first frame, with an empty canvas.
    pub fn rewind(&mut self) {
        self.at = self.start;
        self.pending = None;
        self.canvas.pixels.fill(0);
    }

    /// The canvas as it stands, given up.
    #[must_use]
    pub fn into_canvas(self) -> Image {
        self.canvas
    }

    /// Carry out the disposal the frame last shown asked for.
    fn dispose(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let rect = pending.rect;
        match pending.action {
            Action::Clear => {
                for row in rect.top..rect.bottom {
                    if let Some(line) = span(&mut self.canvas.pixels, self.width, row, rect) {
                        line.fill(0);
                    }
                }
            }
            Action::Restore(saved) => {
                let width = rect.right.saturating_sub(rect.left).max(1);
                for (row, source) in (rect.top..rect.bottom).zip(saved.chunks(width)) {
                    if let Some(line) = span(&mut self.canvas.pixels, self.width, row, rect) {
                        for (slot, &pixel) in line.iter_mut().zip(source) {
                            *slot = pixel;
                        }
                    }
                }
            }
        }
    }

    /// Decode `image` and draw it onto the canvas as `control` says, and
    /// remember what to do with its rectangle before the next frame.
    fn draw(&mut self, image: &ImageBlock, control: Control) -> ImageResult<()> {
        let mut data = SubBlocks::new(self.bytes, image.data);
        let rect = Rect::clipped(image, self.width, self.height);
        let area = image.width.saturating_mul(image.height);
        let saved_bytes = if control.disposal == 3 {
            rect.area().saturating_mul(4)
        } else {
            0
        };
        let bytes = self
            .width
            .saturating_mul(self.height)
            .saturating_mul(4)
            .saturating_add(area)
            .saturating_add(saved_bytes);
        if area as u64 > self.limits.max_pixels || bytes > self.limits.max_decompressed_bytes {
            return Err(ImageError::TooLarge {
                pixels: area as u64,
                limit: self.limits.max_pixels,
            });
        }

        // Disposal 3 puts back what was under the image, so that is saved
        // before the image is drawn over it.
        let saved = (control.disposal == 3).then(|| self.save(rect));

        if rect.area() > 0 {
            self.indices.clear();
            self.indices.resize(area, 0);
            let written = self
                .table
                .decode(image.min_code_size, &mut data, &mut self.indices);
            let local = image.local.and_then(|(table, entries)| {
                self.bytes
                    .get(table..)
                    .and_then(|rest| Palette::read(rest, entries))
            });
            let palette = local.as_ref().or(self.global.as_ref()).unwrap_or(&GREYS);
            paint(
                &mut self.canvas.pixels,
                self.width,
                self.indices.get(..written).unwrap_or(&[]),
                image,
                rect,
                palette,
                control.transparent,
            );
        }
        self.at = data.finish();

        self.pending = match control.disposal {
            2 => Some(Pending {
                rect,
                action: Action::Clear,
            }),
            3 => saved.map(|pixels| Pending {
                rect,
                action: Action::Restore(pixels),
            }),
            _ => None,
        };
        Ok(())
    }

    /// The canvas under `rect`, row by row.
    fn save(&self, rect: Rect) -> Vec<u32> {
        let mut saved = Vec::with_capacity(rect.area());
        for row in rect.top..rect.bottom {
            let start = row.saturating_mul(self.width).saturating_add(rect.left);
            let end = row.saturating_mul(self.width).saturating_add(rect.right);
            if let Some(line) = self.canvas.pixels.get(start..end) {
                saved.extend_from_slice(line);
            }
        }
        saved
    }
}

/// The part of canvas row `row` that `rect` covers, in a canvas `width` wide.
fn span(pixels: &mut [u32], width: usize, row: usize, rect: Rect) -> Option<&mut [u32]> {
    let base = row.checked_mul(width)?;
    pixels.get_mut(base.checked_add(rect.left)?..base.checked_add(rect.right)?)
}

/// Draw an image's `decoded` indices -- in the file's order, which for an
/// interlaced image is not top to bottom -- onto a canvas `width` wide within
/// `rect`, skipping the transparent index.
fn paint(
    canvas: &mut [u32],
    width: usize,
    decoded: &[u8],
    image: &ImageBlock,
    rect: Rect,
    palette: &Palette,
    transparent: Option<u8>,
) {
    let rows = if image.interlace {
        interlaced_rows(image.height)
    } else {
        Vec::new()
    };
    // The image's column under the rectangle's first canvas column.
    let skip = rect.left.saturating_sub(image.left);
    for (sent, indices) in decoded.chunks(image.width.max(1)).enumerate() {
        let row = if image.interlace {
            rows.get(sent).copied().unwrap_or(usize::MAX)
        } else {
            sent
        };
        let y = image.top.saturating_add(row);
        if y < rect.top || y >= rect.bottom {
            continue;
        }
        let Some(line) = span(canvas, width, y, rect) else {
            continue;
        };
        for (slot, &index) in line.iter_mut().zip(indices.iter().skip(skip)) {
            if Some(index) != transparent {
                *slot = palette.colour(index);
            }
        }
    }
}

/// The rows of an interlaced image in the order the file sends them: every
/// eighth from 0, every eighth from 4, every fourth from 2, every second from 1.
fn interlaced_rows(height: usize) -> Vec<usize> {
    let mut rows = Vec::with_capacity(height);
    for (first, step) in [(0usize, 8usize), (4, 8), (2, 4), (1, 2)] {
        rows.extend((first..height).step_by(step));
    }
    rows
}

/// The part of the canvas an image covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
}

impl Rect {
    /// `image`'s rectangle clipped to a `width` x `height` canvas; empty (with
    /// `right == left`, `bottom == top`) if it misses it altogether.
    fn clipped(image: &ImageBlock, width: usize, height: usize) -> Self {
        let left = image.left.min(width);
        let top = image.top.min(height);
        Self {
            left,
            top,
            right: image.left.saturating_add(image.width).clamp(left, width),
            bottom: image.top.saturating_add(image.height).clamp(top, height),
        }
    }

    const fn area(self) -> usize {
        self.right
            .saturating_sub(self.left)
            .saturating_mul(self.bottom.saturating_sub(self.top))
    }
}

/// What to do with a frame's rectangle before the next is drawn.
struct Pending {
    rect: Rect,
    action: Action,
}

enum Action {
    /// Disposal 2: back to transparent.
    Clear,
    /// Disposal 3: back to what was there, saved row by row.
    Restore(Vec<u32>),
}

/// A colour table, as `0xAARRGGBB`. Entries past the table's own length are
/// opaque black, so every index a byte can hold has a colour.
struct Palette {
    colours: [u32; 256],
}

impl Palette {
    /// The table of `entries` colours at the start of `bytes`, three bytes
    /// each.
    fn read(bytes: &[u8], entries: usize) -> Option<Self> {
        let table = bytes.get(..entries.checked_mul(3)?)?;
        let mut colours = [OPAQUE_BLACK; 256];
        for (slot, &[r, g, b]) in colours.iter_mut().zip(table.as_chunks::<3>().0) {
            *slot = 0xFF00_0000 | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
        }
        Some(Self { colours })
    }

    fn colour(&self, index: u8) -> u32 {
        self.colours
            .get(usize::from(index))
            .copied()
            .unwrap_or(OPAQUE_BLACK)
    }
}

const OPAQUE_BLACK: u32 = 0xFF00_0000;

/// What an image with no colour table at all reads its indices as.
static GREYS: Palette = Palette { colours: greys() };

/// Index `i` as the grey `i`.
// Evaluated at compile time, where an index out of bounds would fail the
// build rather than panic at run time -- and `grey` stays under the length.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "grey < 256, the array's length, checked at compile time"
)]
const fn greys() -> [u32; 256] {
    let mut colours = [0u32; 256];
    let mut grey = 0u32;
    while grey < 256 {
        colours[grey as usize] = 0xFF00_0000 | (grey << 16) | (grey << 8) | grey;
        grey += 1;
    }
    colours
}

/// A graphic control extension: what the next image is drawn with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Control {
    disposal: u8,
    transparent: Option<u8>,
    delay_cs: u16,
}

/// An image descriptor, and where its data starts.
struct ImageBlock {
    left: usize,
    top: usize,
    width: usize,
    height: usize,
    interlace: bool,
    /// Where the image's own colour table is, and how many entries it has:
    /// read only when the image is drawn, not by every walk past it.
    local: Option<(usize, usize)>,
    min_code_size: u8,
    /// The first data sub-block's length byte.
    data: usize,
}

/// What the next block of the file is.
enum Block {
    Control(Control),
    Repeat(Repeat),
    Image(ImageBlock),
    /// An extension that changes nothing here, or an image whose data this
    /// cannot decode (a minimum code size past 8), stepped past.
    Other,
    /// The trailer, the end of the data, or a block this does not know.
    End,
}

/// Read the block at `*at` and move `*at` past everything but an image's data,
/// which is the caller's to read or skip.
fn next_block(bytes: &[u8], at: &mut usize) -> Block {
    let Some(&introducer) = bytes.get(*at) else {
        return Block::End;
    };
    match introducer {
        0x21 => {
            let label = bytes.get(at.saturating_add(1)).copied();
            let body = at.saturating_add(2);
            let block = match label {
                Some(0xF9) => read_control(bytes, body),
                Some(0xFF) => read_application(bytes, body),
                Some(_) => Block::Other,
                None => return end(bytes, at),
            };
            *at = SubBlocks::new(bytes, body).finish();
            block
        }
        0x2C => read_image(bytes, at),
        _ => end(bytes, at),
    }
}

/// The end of the blocks: the cursor goes to the end of the file, so every
/// later call ends too.
fn end(bytes: &[u8], at: &mut usize) -> Block {
    *at = bytes.len();
    Block::End
}

/// A graphic control extension's one sub-block: packed flags, a delay and a
/// transparent index. Read leniently: a sub-block shorter than four bytes
/// gives what it has.
fn read_control(bytes: &[u8], body: usize) -> Block {
    let length = usize::from(bytes.get(body).copied().unwrap_or(0));
    let field = |i: usize| {
        if i < length {
            bytes.get(body.saturating_add(1).saturating_add(i)).copied()
        } else {
            None
        }
    };
    let packed = field(0).unwrap_or(0);
    let delay = u16::from_le_bytes([field(1).unwrap_or(0), field(2).unwrap_or(0)]);
    Block::Control(Control {
        disposal: (packed >> 2) & 0x07,
        transparent: if packed & 1 == 1 { field(3) } else { None },
        delay_cs: delay,
    })
}

/// An application extension: only NETSCAPE2.0 (or its twin ANIMEXTS1.0) and
/// only its loop count matter.
fn read_application(bytes: &[u8], body: usize) -> Block {
    let Some(identifier) = bytes.get(body..body.saturating_add(12)) else {
        return Block::Other;
    };
    if identifier.first() != Some(&11) {
        return Block::Other;
    }
    let name = identifier.get(1..).unwrap_or(&[]);
    if name != b"NETSCAPE2.0" && name != b"ANIMEXTS1.0" {
        return Block::Other;
    }
    // The next sub-block: [3, 1, count low, count high].
    let sub = body.saturating_add(12);
    match bytes.get(sub..sub.saturating_add(4)) {
        Some(&[3, 1, low, high]) => match u16::from_le_bytes([low, high]) {
            0 => Block::Repeat(Repeat::Forever),
            count => Block::Repeat(Repeat::Count(count)),
        },
        _ => Block::Other,
    }
}

/// An image descriptor at `*at`, its colour table and its minimum code size;
/// `*at` is left on the first data sub-block.
fn read_image(bytes: &[u8], at: &mut usize) -> Block {
    let Some(descriptor) = bytes.get(at.saturating_add(1)..at.saturating_add(10)) else {
        return end(bytes, at);
    };
    let word = |i: usize| {
        usize::from(u16::from_le_bytes([
            descriptor.get(i).copied().unwrap_or(0),
            descriptor.get(i.saturating_add(1)).copied().unwrap_or(0),
        ]))
    };
    let packed = descriptor.get(8).copied().unwrap_or(0);
    let mut position = at.saturating_add(10);
    let local = if packed & 0x80 != 0 {
        let entries = 2usize << (packed & 0x07);
        let table = position;
        position = position.saturating_add(entries.saturating_mul(3));
        if position > bytes.len() {
            return end(bytes, at);
        }
        Some((table, entries))
    } else {
        None
    };
    let Some(&min_code_size) = bytes.get(position) else {
        return end(bytes, at);
    };
    let data = position.saturating_add(1);
    *at = data;
    if !(2..=8).contains(&min_code_size) {
        // Past 8, codes name indices a byte cannot hold; below 2 is outside
        // the format (giflib's encoder never writes it), and the decoders that
        // accept 1 disagree about when its codes widen, so there is no picture
        // to agree with. Stepped past as the extension it might as well be.
        *at = SubBlocks::new(bytes, data).finish();
        return Block::Other;
    }
    Block::Image(ImageBlock {
        left: word(0),
        top: word(2),
        width: word(4),
        height: word(6),
        interlace: packed & 0x40 != 0,
        local,
        min_code_size,
        data,
    })
}

/// A file's structure, read without decoding an image.
struct Layout {
    /// The canvas: the logical screen, enlarged to hold the first image.
    width: usize,
    height: usize,
    global: Option<Palette>,
    start: usize,
    frames: usize,
    repeat: Repeat,
}

impl Layout {
    fn read(bytes: &[u8]) -> ImageResult<Self> {
        if !is_gif(bytes) {
            return Err(ImageError::Malformed("not a GIF signature"));
        }
        let screen = bytes.get(6..13).ok_or(ImageError::Truncated)?;
        let word = |i: usize| {
            usize::from(u16::from_le_bytes([
                screen.get(i).copied().unwrap_or(0),
                screen.get(i.saturating_add(1)).copied().unwrap_or(0),
            ]))
        };
        let (mut width, mut height) = (word(0), word(2));
        let packed = screen.get(4).copied().unwrap_or(0);
        let mut start = 13usize;
        let global = if packed & 0x80 != 0 {
            let entries = 2usize << (packed & 0x07);
            let palette = bytes
                .get(start..)
                .and_then(|rest| Palette::read(rest, entries))
                .ok_or(ImageError::Truncated)?;
            start = start.saturating_add(entries.saturating_mul(3));
            Some(palette)
        } else {
            None
        };

        let mut frames = 0usize;
        let mut repeat = Repeat::Once;
        let mut at = start;
        loop {
            match next_block(bytes, &mut at) {
                Block::Image(image) => {
                    if frames == 0 {
                        width = width.max(image.left.saturating_add(image.width));
                        height = height.max(image.top.saturating_add(image.height));
                    }
                    frames = frames.saturating_add(1);
                    at = SubBlocks::new(bytes, image.data).finish();
                }
                Block::Repeat(found) => repeat = found,
                Block::Control(_) | Block::Other => {}
                Block::End => break,
            }
        }
        if width == 0 || height == 0 {
            return Err(ImageError::Malformed("a GIF with nothing to draw on"));
        }
        Ok(Self {
            width,
            height,
            global,
            start,
            frames,
            repeat,
        })
    }
}

fn to_u32(n: usize) -> ImageResult<u32> {
    u32::try_from(n).map_err(|_| ImageError::Malformed("an impossible size"))
}
