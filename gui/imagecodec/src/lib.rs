//! Turning a picture file into the pixels the compositor draws.
//!
//! # Why this crate exists
//!
//! The desktop could draw a picture and could be *given* one — in-process via
//! `Compositor::register_image`, and from another process via the display
//! protocol's `UploadImage` (design-decisions.md → 556). What it could not do
//! was obtain one: nothing in the tree turned a `.png` on disk into pixels. So
//! five call sites already emitted an "draw image *n*" command naming an id that
//! no pixels were ever stored under — and drawing an unknown id renders
//! *nothing, silently, by design*. A wallpaper the user picked simply did not
//! appear, and nothing reported why (`known-issues.md` →
//! `TD-C-NOTHING-DECODES-A-PICTURE-SO-EVERY-IMAGE-ID-NAMES-NOTHING`).
//!
//! One crate rather than a reader per caller, for the reason
//! design-decisions.md → 555 spells out at length: two implementations of one
//! user-facing concept are not redundancy, they are a disagreement waiting to
//! be found by a user, and neither test suite can see it because each is tested
//! only against itself.
//!
//! That rule applies to this crate as much as to its callers, and until
//! 2026-08-26 it was being broken here. PNG pixel data is a zlib stream, so this
//! crate carried its own DEFLATE decoder — the *second* one in the tree, beside
//! the kernel's. Both were tested only against themselves, which is precisely
//! the arrangement the paragraph above says cannot detect a disagreement. The
//! shared one is now the `deflate` crate (`no_std` + `alloc`, depending only on
//! `crc32`), 872 lines here were deleted, and the two are one.
//!
//! # The output is what the compositor stores
//!
//! [`Image::pixels`] is densely packed `0xAARRGGBB` — the same layout
//! `BufferFormat::Argb8888` names on the wire and the same one
//! `SharedBuffer`/`ImageAsset` normalise to. That is not a coincidence to be
//! tidied away later: a decoder that produced its own arrangement would put a
//! conversion pass between every picture and every screen, and the conversion
//! is where the channel order gets swapped.
//!
//! Alpha is **straight, not premultiplied**, because the compositor's
//! `blend_pixel` multiplies by the alpha itself. Handing it premultiplied
//! pixels would multiply twice and darken every translucent edge.
//!
//! # The input is untrusted, and that shapes everything
//!
//! A wallpaper, an icon, a thumbnail of a downloaded photograph — all of them
//! are files somebody else wrote. So:
//!
//! - **Nothing here panics, for any input.** Every index is `get`, every
//!   arithmetic operation on a value the file chose is checked or saturating,
//!   and there is a test that flips every bit of a valid file one at a time.
//!   A corrupt icon in a directory listing must not take down the file manager,
//!   let alone the shell that hosts it.
//! - **Nothing is allocated on the strength of a number in a header.** A PNG
//!   header declaring 65535×65535 costs the attacker eight bytes and would cost
//!   us 17 GB. [`Limits`] is checked against the header *before* the pixel
//!   buffer exists, and the decompressor is given the exact output size the
//!   header implies, so a "zip bomb" in the pixel stream stops at the first
//!   byte past it.
//!
//! # What is here and what is not
//!
//! PNG (RFC 2083) in full: every colour type, every bit depth, palettes,
//! transparency, and Adam7 interlacing. Colour management (`gAMA`, `cHRM`,
//! `iCCP`, `sRGB`) is parsed past and not applied — the desktop has no colour
//! pipeline to apply it to yet, and applying half of it would be worse than
//! applying none. APNG animation is likewise skipped: an animated PNG decodes
//! to its first frame, which is what its own specification says a decoder that
//! does not animate must show.
//!
//! JPEG is not here yet. It is the other format a wallpaper is likely to be in
//! and is the next thing this crate should grow.
//!
//! # Picture files for *other* crates' tests
//!
//! [`testing`] emits real, small PNGs. It is public rather than `#[cfg(test)]`
//! on purpose: every caller of this crate needs a `.png` to point its own tests
//! at, and a `#[cfg(test)]` helper is invisible outside the crate that declares
//! it — so "share theirs" is not an option even in principle, and three callers
//! meant three hand-rolled encoders that could each be wrong differently.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;

pub mod png;
pub mod testing;

/// A decoded picture: densely packed `0xAARRGGBB`, row-major, no padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels. Never zero in a successfully decoded image.
    pub width: u32,
    /// Height in pixels. Never zero in a successfully decoded image.
    pub height: u32,
    /// `width * height` pixels, row-major, `0xAARRGGBB`, straight alpha.
    pub pixels: Vec<u32>,
}

impl Image {
    /// The pixels as bytes, ready to hand to `register_image` or
    /// `Connection::upload_image` with `BufferFormat::Argb8888`.
    ///
    /// A copy, and unapologetically so: the alternative is
    /// `bytemuck`-style transmutation of a `Vec<u32>` into a `Vec<u8>`, which
    /// is `unsafe`, endian-dependent, and saves one pass over an image that has
    /// already been through several.
    #[must_use]
    pub fn to_argb_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len().saturating_mul(4));
        for px in &self.pixels {
            out.extend_from_slice(&px.to_le_bytes());
        }
        out
    }

    /// Bytes from the start of one row to the start of the next, for the same
    /// callers. Always `width * 4`: this crate never pads.
    #[must_use]
    pub const fn stride(&self) -> u32 {
        self.width.saturating_mul(4)
    }
}

/// What a decoder will accept before it refuses to start.
///
/// Every field is a bound on something an attacker chooses with a handful of
/// bytes. They are checked against the file's *header*, before any buffer the
/// header describes is allocated — a limit applied afterwards is not a limit,
/// it is a post-mortem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Most pixels an image may contain.
    ///
    /// Defaults to the compositor's own `MAX_BUFFER_PIXELS` (7680×4320), on the
    /// grounds that a picture the compositor could not store is a picture there
    /// is no point decoding. A caller that only wants a 64-pixel icon should
    /// lower it and get its refusal from the header rather than from a
    /// 132-megabyte buffer.
    pub max_pixels: u64,
    /// Most bytes of decompressed pixel data to accept.
    ///
    /// Distinct from `max_pixels` because the two bound different attacks: a
    /// huge declared size, versus a small declared size whose compressed stream
    /// expands without end. In practice the PNG decoder computes the exact
    /// figure its header implies and passes the smaller of the two, so this is
    /// a ceiling on the computation rather than the number usually used.
    pub max_decompressed_bytes: usize,
}

impl Limits {
    /// The compositor's framebuffer cap: 7680×4320.
    pub const DEFAULT_MAX_PIXELS: u64 = 7680 * 4320;
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_pixels: Self::DEFAULT_MAX_PIXELS,
            // Sixteen bytes per pixel: the widest PNG is 16-bit RGBA, which is
            // eight bytes per pixel of pixel data plus a filter byte per row,
            // and the doubling leaves room for that without a second constant
            // that could disagree with this one.
            max_decompressed_bytes: (Self::DEFAULT_MAX_PIXELS as usize).saturating_mul(16),
        }
    }
}

/// Why a picture could not be decoded.
///
/// Deliberately one type across formats, so that a caller which does not care
/// *which* format failed — a thumbnail generator handed a directory of mixed
/// files — has one thing to match on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageError {
    /// The bytes do not begin with any signature this crate recognises.
    UnknownFormat,
    /// The file ended in the middle of a structure it had announced.
    Truncated,
    /// A header field is not one the format permits — a colour type of 7, a
    /// bit depth of 3, a filter method that does not exist.
    ///
    /// Carries a short description naming the field, because "invalid PNG" in a
    /// log is a bug report nobody can act on.
    Malformed(&'static str),
    /// The file is a valid picture of a kind this crate cannot yet decode.
    Unsupported(&'static str),
    /// The picture is larger than [`Limits`] allows.
    TooLarge {
        /// Pixels the file declares.
        pixels: u64,
        /// Pixels the caller allowed.
        limit: u64,
    },
    /// A checksum in the file does not match the bytes it covers.
    Corrupt(&'static str),
    /// The compressed pixel data could not be read.
    Compressed(deflate::Error),
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFormat => f.write_str("not a picture format this system reads"),
            Self::Truncated => f.write_str("file ends mid-structure"),
            Self::Malformed(what) => write!(f, "malformed {what}"),
            Self::Unsupported(what) => write!(f, "unsupported {what}"),
            Self::TooLarge { pixels, limit } => {
                write!(
                    f,
                    "image of {pixels} pixels exceeds the {limit}-pixel limit"
                )
            }
            Self::Corrupt(what) => write!(f, "corrupt {what}"),
            Self::Compressed(e) => write!(f, "compressed data: {}", compression_failure(*e)),
        }
    }
}

/// One line of prose for a [`deflate::Error`], because that type has no
/// `Display` of its own.
///
/// This wording used to live on `imagecodec`'s own `InflateError`. It is kept
/// here rather than dropped in favour of `{e:?}` because this string reaches a
/// person: it is what the desktop shows when a wallpaper will not open, and
/// `DistanceTooFar` is an identifier, not an explanation.
///
/// The catch-all is not optional — `deflate::Error` is `#[non_exhaustive]`, so
/// a `match` on it from outside that crate must have one. Its cost is that the
/// compiler cannot tell us when lane A adds a variant, so a new one degrades
/// quietly to its `Debug` form rather than failing the build. That is the right
/// way round for a *message*, but it does mean this table can go stale silently.
///
/// The proper fix is a `Display` impl on `deflate::Error` itself, so that every
/// caller is not left inventing its own wording and disagreeing — the same
/// two-copies-drift argument that got this crate's decoder deleted in the first
/// place. Filed as `requests/c-a-deflate-error-has-no-display.md`; when it
/// lands, delete this function and write `{e}`.
fn compression_failure(e: deflate::Error) -> &'static str {
    match e {
        deflate::Error::UnexpectedEnd => "compressed stream ended mid-symbol",
        deflate::Error::ReservedBlockType => "reserved DEFLATE block type 3",
        deflate::Error::StoredLengthMismatch => "stored block length does not match its complement",
        deflate::Error::InvalidHuffmanTable => "invalid Huffman code lengths",
        deflate::Error::InvalidSymbol => "undecodable Huffman symbol",
        deflate::Error::DistanceTooFar => "back-reference points before the start of the output",
        deflate::Error::OutputTooLarge => "decompressed size exceeds the caller's limit",
        deflate::Error::BadWrapperHeader => "not a zlib stream this decoder supports",
        deflate::Error::PresetDictionary => "zlib preset dictionary, which is not supported",
        deflate::Error::ChecksumMismatch { .. } => "zlib Adler-32 checksum mismatch",
        _ => "unreadable compressed stream",
    }
}

impl From<deflate::Error> for ImageError {
    fn from(e: deflate::Error) -> Self {
        Self::Compressed(e)
    }
}

/// The result of any decode in this crate.
pub type ImageResult<T> = Result<T, ImageError>;

/// Decode a picture, choosing the decoder by what the bytes actually are.
///
/// The signature is read from the file, never from its name: a `.jpg` that is
/// really a PNG is common enough that a name-driven decoder would fail on files
/// that open perfectly everywhere else, and a `.png` that is really a shell
/// script is how a name-driven decoder becomes a security bug.
///
/// # Errors
///
/// [`ImageError::UnknownFormat`] if no decoder claims the bytes; otherwise
/// whatever the chosen decoder reports. Never panics, for any input.
pub fn decode(bytes: &[u8], limits: Limits) -> ImageResult<Image> {
    if png::is_png(bytes) {
        return png::decode(bytes, limits);
    }
    Err(ImageError::UnknownFormat)
}

/// Read a picture's dimensions without decoding its pixels.
///
/// The file manager's detail columns and the preview engine want the size of
/// several hundred files and the pixels of none of them. Reading the header
/// alone is microseconds where a full decode is milliseconds, and — more to the
/// point — it allocates nothing that a hostile file gets to size.
///
/// # Errors
///
/// As [`decode`], minus everything that can only go wrong in the pixel data.
pub fn dimensions(bytes: &[u8]) -> ImageResult<(u32, u32)> {
    if png::is_png(bytes) {
        return png::dimensions(bytes);
    }
    Err(ImageError::UnknownFormat)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use alloc::vec;

    #[test]
    fn argb_bytes_are_little_endian_so_the_compositor_reads_them_back_unchanged() {
        // `normalize` in the compositor does `u32::from_le_bytes`, so this is
        // the one ordering that survives the round trip.
        let img = Image {
            width: 1,
            height: 1,
            pixels: vec![0x8899_AABB],
        };
        assert_eq!(img.to_argb_bytes(), vec![0xBB, 0xAA, 0x99, 0x88]);
        assert_eq!(img.stride(), 4);
    }

    #[test]
    fn bytes_that_are_not_a_picture_are_refused_rather_than_guessed_at() {
        assert_eq!(
            decode(b"#!/bin/sh\n", Limits::default()),
            Err(ImageError::UnknownFormat)
        );
        assert_eq!(
            decode(&[], Limits::default()),
            Err(ImageError::UnknownFormat)
        );
        assert_eq!(dimensions(b"GIF89a"), Err(ImageError::UnknownFormat));
    }

    #[test]
    fn the_default_limit_is_the_compositors_own_cap() {
        // A picture the compositor could not store is a picture there is no
        // point decoding, so the two numbers must not drift apart.
        assert_eq!(Limits::default().max_pixels, 7680 * 4320);
    }

    #[test]
    fn an_error_says_which_field_was_wrong() {
        // "invalid PNG" in a log is a bug report nobody can act on.
        let e = ImageError::Malformed("IHDR colour type");
        let mut s = alloc::string::String::new();
        core::fmt::write(&mut s, format_args!("{e}")).unwrap();
        assert_eq!(s, "malformed IHDR colour type");
    }
}
