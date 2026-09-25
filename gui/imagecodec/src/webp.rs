//! WebP (RFC 9649): the RIFF container, and the pictures in it.
//!
//! A WebP file is a RIFF file whose chunks hold one of three things: a lossy
//! picture (`VP8 `, a VP8 video key frame), a lossless one (`VP8L`), or -- the
//! extended format, announced by a `VP8X` chunk -- either of those with a
//! separate alpha plane (`ALPH`) or an animation (`ANIM` and `ANMF`), plus
//! metadata this crate has no use for.
//!
//! # What this reads
//!
//! Lossless pictures in full (`webp/lossless.rs`), in the simple format or the
//! extended one. Lossy pictures, and animations, are refused by name for now
//! ([`ImageError::Unsupported`]) rather than half-read.
//!
//! # Hostile input
//!
//! Every chunk length is checked against the bytes present before anything is
//! read through it; a size in a header is checked against [`Limits`] before any
//! pixel buffer exists; and the lossless decoder bounds everything it
//! allocates the same way (see its module).

use crate::{Image, ImageError, ImageResult, Limits};

mod lossless;

/// Whether `bytes` is a RIFF file of type WEBP.
#[must_use]
pub fn is_webp(bytes: &[u8]) -> bool {
    bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WEBP")
}

/// A chunk: its four-character code and its payload.
struct Chunk<'a> {
    fourcc: [u8; 4],
    payload: &'a [u8],
}

/// The chunks after the file header, in order, each padded to an even length
/// as RIFF requires. A chunk whose length runs past the file ends the walk.
fn chunks(bytes: &[u8]) -> impl Iterator<Item = Chunk<'_>> + '_ {
    let mut at = 12usize;
    core::iter::from_fn(move || {
        let header = bytes.get(at..at.checked_add(8)?)?;
        let fourcc: [u8; 4] = header.get(..4)?.try_into().ok()?;
        let size = u32::from_le_bytes(header.get(4..8)?.try_into().ok()?) as usize;
        let start = at.checked_add(8)?;
        let payload = bytes.get(start..start.checked_add(size)?)?;
        at = start.checked_add(size)?.checked_add(size & 1)?;
        Some(Chunk { fourcc, payload })
    })
}

/// What the file holds, found by walking its chunks once.
enum Content<'a> {
    Lossless(&'a [u8]),
    Lossy,
    Animated,
}

/// The canvas size and what is on it.
struct Layout<'a> {
    width: u32,
    height: u32,
    content: Content<'a>,
}

fn layout(bytes: &[u8]) -> ImageResult<Layout<'_>> {
    if !is_webp(bytes) {
        return Err(ImageError::Malformed("not a RIFF WEBP file"));
    }
    let mut chunks = chunks(bytes);
    let first = chunks.next().ok_or(ImageError::Truncated)?;
    match &first.fourcc {
        b"VP8L" => {
            let (width, height) = lossless::dimensions(first.payload)?;
            Ok(Layout {
                width,
                height,
                content: Content::Lossless(first.payload),
            })
        }
        b"VP8 " => {
            let (width, height) = lossy_dimensions(first.payload)?;
            Ok(Layout {
                width,
                height,
                content: Content::Lossy,
            })
        }
        b"VP8X" => {
            let header = first.payload.get(..10).ok_or(ImageError::Truncated)?;
            let flags = header.first().copied().unwrap_or(0);
            let u24 = |i: usize| {
                u32::from_le_bytes([
                    header.get(i).copied().unwrap_or(0),
                    header.get(i.saturating_add(1)).copied().unwrap_or(0),
                    header.get(i.saturating_add(2)).copied().unwrap_or(0),
                    0,
                ])
            };
            // A 24-bit field plus one, which a u32 always holds.
            let width = u24(4).saturating_add(1);
            let height = u24(7).saturating_add(1);
            if flags & 0x02 != 0 {
                return Ok(Layout {
                    width,
                    height,
                    content: Content::Animated,
                });
            }
            for chunk in chunks {
                match &chunk.fourcc {
                    b"VP8L" => {
                        let own = lossless::dimensions(chunk.payload)?;
                        if own != (width, height) {
                            return Err(ImageError::Malformed(
                                "a VP8L picture a different size from its canvas",
                            ));
                        }
                        return Ok(Layout {
                            width,
                            height,
                            content: Content::Lossless(chunk.payload),
                        });
                    }
                    b"VP8 " => {
                        return Ok(Layout {
                            width,
                            height,
                            content: Content::Lossy,
                        });
                    }
                    _ => {}
                }
            }
            Err(ImageError::Malformed(
                "an extended WebP with no picture in it",
            ))
        }
        _ => Err(ImageError::Malformed(
            "a WebP whose first chunk is not a picture",
        )),
    }
}

/// A lossy key frame's size, from its frame header (RFC 6386 §9.1): three
/// bytes of frame tag, the start code `9D 01 2A`, then two 16-bit fields whose
/// low 14 bits are the width and the height.
fn lossy_dimensions(payload: &[u8]) -> ImageResult<(u32, u32)> {
    let header = payload.get(..10).ok_or(ImageError::Truncated)?;
    if header.get(3..6) != Some(&[0x9D, 0x01, 0x2A]) {
        return Err(ImageError::Malformed("a VP8 frame without its start code"));
    }
    let field = |i: usize| {
        u32::from(u16::from_le_bytes([
            header.get(i).copied().unwrap_or(0),
            header.get(i.saturating_add(1)).copied().unwrap_or(0),
        ])) & 0x3FFF
    };
    Ok((field(6), field(8)))
}

/// A WebP's canvas size.
///
/// # Errors
///
/// [`ImageError::Truncated`] or [`ImageError::Malformed`] for a file too short
/// or too broken to say.
pub fn dimensions(bytes: &[u8]) -> ImageResult<(u32, u32)> {
    let layout = layout(bytes)?;
    Ok((layout.width, layout.height))
}

/// Decode a WebP.
///
/// # Errors
///
/// [`ImageError::Unsupported`] for a lossy or animated one, for now;
/// [`ImageError::TooLarge`] past `limits`; otherwise what the bitstream's
/// decoder reports.
pub fn decode(bytes: &[u8], limits: Limits) -> ImageResult<Image> {
    let layout = layout(bytes)?;
    let pixels_claimed = u64::from(layout.width).saturating_mul(u64::from(layout.height));
    if pixels_claimed > limits.max_pixels {
        return Err(ImageError::TooLarge {
            pixels: pixels_claimed,
            limit: limits.max_pixels,
        });
    }
    match layout.content {
        Content::Lossless(payload) => {
            let (width, height, pixels) = lossless::decode(payload, limits)?;
            Ok(Image {
                width,
                height,
                pixels,
            })
        }
        Content::Lossy => Err(ImageError::Unsupported("lossy WebP")),
        Content::Animated => Err(ImageError::Unsupported("animated WebP")),
    }
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
