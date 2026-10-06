//! A picture a program holds in memory and a window draws by naming it:
//! what a rich input shows in its text ([`crate::richinput`]), and what the
//! program's clipboard carries beside text ([`crate::clipboard`]).
//!
//! # Drawing one
//!
//! A render tree names a picture by number -- [`RenderCommand::Image`]'s
//! `image_id` -- and the pixels go to the window once, under that number,
//! for every frame after to name. A [`Picture`] carries its number with it,
//! given when it is made, so two pictures never share one, and a copy of a
//! picture -- the clipboard's, a pasted one, a deletion undone -- is the same
//! picture under the same number, sent once. [`Uploads`] keeps track of
//! which pictures a window has: each frame the program says which it shows,
//! and is told which to hand over and which to give back
//! ([`Uploads::changes`]), the giving back first, as `design-decisions.md`
//! §557 has it for every picture a window is given.
//!
//! The numbers start far up the id space ([`FIRST_ID`]), so a program
//! numbering its own pictures from nothing never meets them.
//!
//! # The pixels
//!
//! As `imagecodec` decodes them -- `0xAARRGGBB`, straight alpha, row-major,
//! no padding -- which is also what a window takes
//! ([`WireBytes::from_le_argb`]) and what `imagecodec::encode_png` writes a
//! file from: a picture is decoded once and never converted again. The
//! pixels are shared and never change after the picture is made, so a
//! picture is cheap to copy and its number always names the same pixels.
//!
//! [`RenderCommand::Image`]: crate::render::RenderCommand::Image

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use imagecodec::{EncodeError, Image, ImageError, Limits};

use crate::canvas::{Canvas, WireBytes};

/// The first number a picture is given: far up the id space -- "PICT" in its
/// top bytes -- so that the numbers a program gives its own pictures,
/// counted from nothing, never reach it.
pub const FIRST_ID: u64 = 0x5049_4354_0000_0000;

/// The number the next picture made is given.
static NEXT_ID: AtomicU64 = AtomicU64::new(FIRST_ID);

/// A picture: its pixels, shared, and the number a window knows it by.
///
/// Two pictures are equal when they are the same picture -- one made, and
/// copied -- not when their pixels happen to match: the number is the
/// identity, and as the pixels never change it always names the same ones.
#[derive(Clone, Debug)]
pub struct Picture {
    id: u64,
    image: Arc<Image>,
}

impl PartialEq for Picture {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Picture {}

/// A side of a picture as a distance on the screen.
#[allow(
    clippy::cast_precision_loss,
    reason = "a picture's side is at most a few thousand pixels -- decoding \
              refuses more than the compositor's 7680x4320 -- and every whole \
              number up to 16 777 216 is exact in an f32"
)]
const fn side(n: u32) -> f32 {
    n as f32
}

impl Picture {
    /// `image` as a picture, under a number of its own -- or `None` for one
    /// with no pixels, or with a different number of pixels than its size
    /// says, which no window could be given.
    #[must_use]
    pub fn new(image: Image) -> Option<Self> {
        let count = usize::try_from(image.width)
            .ok()?
            .checked_mul(usize::try_from(image.height).ok()?)?;
        if count == 0 || image.pixels.len() != count {
            return None;
        }
        Some(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            image: Arc::new(image),
        })
    }

    /// The picture a file holds -- PNG, JPEG, GIF, WebP, BMP, ICO or TIFF,
    /// found from its bytes and never from its name -- no larger than the
    /// compositor can store (`imagecodec::Limits`' default).
    ///
    /// # Errors
    ///
    /// Why the bytes are not a picture this system reads: an unknown format,
    /// a file cut short or malformed, or one larger than a window can take.
    pub fn decode(bytes: &[u8]) -> Result<Self, ImageError> {
        let image = imagecodec::decode(bytes, Limits::default())?;
        // A decoded image always has pixels, as many as its size says.
        Self::new(image).ok_or(ImageError::Malformed("a picture with no pixels"))
    }

    /// A picture of what is drawn on `canvas` -- a screenshot, a drawing --
    /// or `None` for an empty canvas.
    #[must_use]
    pub fn from_canvas(canvas: &Canvas) -> Option<Self> {
        let pixels = canvas
            .pixels()
            .iter()
            .map(|c| u32::from_be_bytes([c.a, c.r, c.g, c.b]))
            .collect();
        Self::new(Image {
            width: canvas.width(),
            height: canvas.height(),
            pixels,
        })
    }

    /// The number a window knows it by: what a render tree's
    /// `RenderCommand::Image` names it with.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Its width in pixels; never zero.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.image.width
    }

    /// Its height in pixels; never zero.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.image.height
    }

    /// Its pixels: `0xAARRGGBB`, row-major, `width * height` of them.
    #[must_use]
    pub fn pixels(&self) -> &[u32] {
        &self.image.pixels
    }

    /// Its pixels in the order a window takes them
    /// (`BufferFormat::Argb8888`), four bytes each and `width * 4` to a row:
    /// what an upload hands over.
    #[must_use]
    pub fn wire_bytes(&self) -> WireBytes {
        WireBytes::from_le_argb(&self.image.pixels)
    }

    /// It as a PNG file: what a picture leaving the program is written as.
    ///
    /// # Errors
    ///
    /// A picture too large for PNG to describe.
    pub fn to_png(&self) -> Result<Vec<u8>, EncodeError> {
        imagecodec::encode_png(self.image.width, self.image.height, &self.image.pixels)
    }

    /// The size it is shown at in a box `width` across: its own, or scaled
    /// down to the box's width, its shape kept. A box of no width shows it
    /// at its own size.
    #[must_use]
    pub fn fitted(&self, width: f32) -> (f32, f32) {
        let (w, h) = (side(self.image.width), side(self.image.height));
        if width > 0.0 && w > width {
            (width, h * (width / w))
        } else {
            (w, h)
        }
    }
}

/// What a window is to be given or to give back, so it holds the pictures a
/// program shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Hand this picture over, under its number.
    Upload(Picture),
    /// Give back the picture under this number: nothing shows it now.
    Drop(u64),
}

/// Which pictures a window has been given, so that each picture a program
/// shows is handed over once, and given back once nothing shows it.
///
/// One per window. A program asks it every frame, before drawing, with the
/// pictures it is about to show ([`changes`](Self::changes)), and passes
/// what it is told on to the window -- in the window library, as
/// `ImageChange::Upload` and `ImageChange::Drop`.
#[derive(Clone, Debug, Default)]
pub struct Uploads {
    /// The numbers of the pictures the window holds.
    sent: BTreeSet<u64>,
}

impl Uploads {
    /// A window given nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What to give the window so it holds exactly the pictures in `shown`
    /// -- each picture it holds that is not shown now given back, and then
    /// each shown one it does not hold handed over, once however often it
    /// is shown. The giving back comes first, so a window whose room fits
    /// one large picture is never asked to hold two (`design-decisions.md`
    /// §557).
    pub fn changes<'a>(&mut self, shown: impl IntoIterator<Item = &'a Picture>) -> Vec<Change> {
        let mut wanted: Vec<&Picture> = Vec::new();
        let mut ids = BTreeSet::new();
        for picture in shown {
            if ids.insert(picture.id()) {
                wanted.push(picture);
            }
        }
        let mut out: Vec<Change> = self
            .sent
            .difference(&ids)
            .map(|&id| Change::Drop(id))
            .collect();
        out.extend(
            wanted
                .into_iter()
                .filter(|p| !self.sent.contains(&p.id()))
                .map(|p| Change::Upload(p.clone())),
        );
        self.sent = ids;
        out
    }

    /// Whether the window holds the picture numbered `id`.
    #[must_use]
    pub fn holds(&self, id: u64) -> bool {
        self.sent.contains(&id)
    }

    /// The window lost what it held -- it was closed and opened again, or
    /// its connection was -- so every picture shown is handed over anew.
    pub fn forget(&mut self) {
        self.sent.clear();
    }
}

#[cfg(test)]
#[path = "picture_tests.rs"]
mod tests;
