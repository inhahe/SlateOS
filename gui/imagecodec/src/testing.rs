//! Real picture files for other crates' tests to decode.
//!
//! # Why this is in the library and not in a test module
//!
//! Every caller of this crate needs a `.png` to point its own tests at, and
//! every one of them was writing its own. `apps/imageviewer` had a `png_bytes`
//! helper; `apps/explorer` was about to grow an identical one; this crate's
//! `png` tests have a third. Three hand-rolled PNG encoders is three chances to
//! encode a subtly different file, and the two that live in test modules cannot
//! be shared at all — a `#[cfg(test)]` item is invisible outside its own crate,
//! so "just call theirs" is not available even in principle.
//!
//! That is the same argument that put the *decoder* in one crate rather than
//! one per caller (design-decisions.md §555), and it applies with more force
//! here: a wrong decoder shows a wrong picture, but a wrong *fixture* makes a
//! correct decoder look broken, or — far worse — makes a broken one look
//! correct.
//!
//! # What it deliberately is not
//!
//! Not a general PNG encoder. It emits one shape of file: 8-bit RGBA, no
//! interlacing, filter 0 on every row, pixel data in **stored** (uncompressed)
//! deflate blocks. That is enough to be a genuine, standards-conforming PNG
//! that any decoder must read, and small enough to be obviously correct by
//! inspection. A test that needs a palette, a bit depth, an interlaced file or
//! a specific filter is testing the *decoder*, and belongs in this crate's own
//! `png` tests where the bytes are laid out by hand on purpose.
//!
//! Also not a *compressor*: stored blocks make the output slightly larger than
//! the input, which for a fixture is the right trade. Building real Huffman
//! tables here would mean this module could be wrong in a way that looks like
//! the decompressor being wrong.

use alloc::vec;
use alloc::vec::Vec;

/// A valid 8-bit RGBA PNG of `width` × `height`, with `pixel(x, y)` supplying
/// each pixel as `[r, g, b, a]` with **straight** (non-premultiplied) alpha.
///
/// Prefer a recognisable pattern over a flat colour: a test asserting that a
/// picture was decoded rather than invented can only tell the difference if the
/// picture has something in it to tell apart.
///
/// ```
/// let bytes = imagecodec::testing::png_rgba(4, 3, |x, y| {
///     [(x % 256) as u8, (y % 256) as u8, 0x40, 0xFF]
/// });
/// let image = imagecodec::decode(&bytes, imagecodec::Limits::default())
///     .expect("the fixture is a valid PNG");
/// assert_eq!((image.width, image.height), (4, 3));
/// // Row 1, pixel 2, as 0xAARRGGBB.
/// assert_eq!(image.pixels[1 * 4 + 2], 0xFF02_0140);
/// ```
#[must_use]
pub fn png_rgba(width: u32, height: u32, mut pixel: impl FnMut(u32, u32) -> [u8; 4]) -> Vec<u8> {
    // Filter byte 0 ("None") per row, then the row's RGBA samples. Filter 0 is
    // chosen because a fixture's job is to be trivially right, and every other
    // filter makes the expected bytes a function of the row above.
    let mut raw = Vec::new();
    for y in 0..height {
        raw.push(0u8);
        for x in 0..width {
            raw.extend_from_slice(&pixel(x, y));
        }
    }

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    // Bit depth 8, colour type 6 (truecolour with alpha), compression 0
    // (deflate), filter method 0 (adaptive), interlace 0 (none).
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    push_chunk(&mut png, b"IHDR", &ihdr);
    push_chunk(&mut png, b"IDAT", &zlib_stored(&raw));
    push_chunk(&mut png, b"IEND", &[]);
    png
}

/// [`png_rgba`] with the gradient most callers want: red follows *x*, green
/// follows *y*, blue is a constant `0x40`, fully opaque.
///
/// Every channel varies or is fixed for a reason a failing test can use: a
/// picture that came out transposed shows up as red and green swapped, one that
/// came out blank has no `0x40` in it anywhere, and one whose alpha was
/// premultiplied has no `0xFF` alpha.
#[must_use]
pub fn png_gradient(width: u32, height: u32) -> Vec<u8> {
    png_rgba(width, height, |x, y| {
        [
            u8::try_from(x % 256).unwrap_or(0),
            u8::try_from(y % 256).unwrap_or(0),
            0x40,
            0xFF,
        ]
    })
}

/// Length, type, payload, CRC — the shape of every PNG chunk (RFC 2083 §5.3).
fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(
        &u32::try_from(payload.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    let mut crc_over = Vec::with_capacity(4usize.saturating_add(payload.len()));
    crc_over.extend_from_slice(kind);
    crc_over.extend_from_slice(payload);
    out.extend_from_slice(&crc32(&crc_over).to_be_bytes());
}

/// A zlib stream (RFC 1950) carrying `data` verbatim in stored deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    // CMF 0x78 (deflate, 32 KiB window), FLG 0x01 — chosen so the two-byte
    // header read as a big-endian u16 is a multiple of 31, which is the check
    // every zlib reader performs first.
    let mut out = vec![0x78, 0x01];
    // A stored block's length field is 16 bits, so anything over 64 KiB of raw
    // bytes takes several blocks. Empty input still needs one, or the stream
    // ends without a final-block marker and a decoder is right to call it
    // truncated.
    let mut chunks = data.chunks(0xFFFF).peekable();
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(chunk) = chunks.next() {
        out.push(u8::from(chunks.peek().is_none()));
        let len = u16::try_from(chunk.len()).unwrap_or(u16::MAX);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// The PNG chunk checksum (RFC 2083 §15.3).
///
/// Computed bit by bit rather than from a generated table: a table would be a
/// second thing to get right, in a module whose whole value is being obviously
/// correct.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// The zlib stream checksum (RFC 1950 §9).
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        // Modular by definition; `%` after each add keeps both below 65521, so
        // neither can approach the point where a `u32` would wrap.
        a = a.wrapping_add(u32::from(byte)) % 65521;
        b = b.wrapping_add(a) % 65521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use crate::{Limits, decode};

    /// The point of the whole module: what it writes, the decoder reads, and
    /// the pixels that come back are the pixels that went in. A fixture that
    /// merely *parses* would let a channel-order mistake through in both
    /// directions at once.
    #[test]
    fn what_the_fixture_writes_is_what_the_decoder_reads() {
        let bytes = png_gradient(5, 3);
        let image = decode(&bytes, Limits::default()).expect("a valid PNG");
        assert_eq!((image.width, image.height), (5, 3));
        for y in 0..3u32 {
            for x in 0..5u32 {
                let got = image.pixels[(y * 5 + x) as usize];
                let want = 0xFF00_0040 | (x << 16) | (y << 8);
                assert_eq!(got, want, "pixel ({x}, {y})");
            }
        }
    }

    /// Alpha survives as written rather than being folded into the colour
    /// channels. A fixture that premultiplied would make a decoder that also
    /// premultiplied look right.
    #[test]
    fn a_translucent_pixel_keeps_its_colour_and_its_alpha_separately() {
        let bytes = png_rgba(1, 1, |_, _| [0xFF, 0x00, 0x00, 0x80]);
        let image = decode(&bytes, Limits::default()).expect("a valid PNG");
        assert_eq!(image.pixels[0], 0x80FF_0000);
    }

    /// Over 64 KiB of raw bytes the pixel data needs more than one stored
    /// block, and the final-block flag has to land on the last one and nowhere
    /// else. A fixture used only for tiny pictures would never exercise this,
    /// and would fail the first time somebody wrote a realistic one.
    #[test]
    fn a_picture_too_big_for_one_stored_block_still_decodes() {
        // 200 x 100 x 4 bytes + 100 filter bytes = 80_100 raw bytes, which is
        // two blocks.
        let bytes = png_gradient(200, 100);
        let image = decode(&bytes, Limits::default()).expect("a valid PNG");
        assert_eq!((image.width, image.height), (200, 100));
        assert_eq!(image.pixels.len(), 200 * 100);
        assert_eq!(image.pixels[199 * 200 % (200 * 100)] & 0x0000_00FF, 0x40);
    }

    /// A zero-pixel picture is not a valid PNG — IHDR forbids a zero dimension
    /// — so this asserts the *encoder* still produces a well-formed stream
    /// rather than panicking on the empty case, and that the decoder is what
    /// refuses it. The empty-input branch of `zlib_stored` exists for this.
    #[test]
    fn a_zero_sized_request_produces_bytes_and_a_refusal_rather_than_a_panic() {
        let bytes = png_rgba(0, 0, |_, _| [0, 0, 0, 0]);
        assert!(bytes.len() > 8, "a signature and some chunks");
        assert!(decode(&bytes, Limits::default()).is_err());
    }

    /// The CRC is over the chunk *type* and the payload, not the length, and
    /// not the payload alone. Getting that wrong produces a file every decoder
    /// rejects, which would be caught — but getting it wrong in the direction
    /// of "no CRC checked" would not be, so this checks a real value.
    #[test]
    fn the_chunk_checksum_is_over_the_type_and_the_payload() {
        // "IEND" with an empty payload has one universally-published CRC.
        let mut out = Vec::new();
        push_chunk(&mut out, b"IEND", &[]);
        assert_eq!(
            out,
            vec![0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]
        );
    }
}
