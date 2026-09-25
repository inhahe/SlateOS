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
//! Still pictures in full, in the simple format or the extended one: lossless
//! (`webp/lossless.rs`) and lossy (`webp/lossy.rs`), the latter with its alpha
//! plane (`webp/alpha.rs`). Animations are refused by name for now
//! ([`ImageError::Unsupported`]) rather than half-read.
//!
//! # Hostile input
//!
//! Every chunk length is checked against the bytes present before anything is
//! read through it; a size in a header is checked against [`Limits`] before any
//! pixel buffer exists; and the decoders bound everything they allocate the
//! same way (see their modules).

use crate::{Image, ImageError, ImageResult, Limits};

mod alpha;
mod lossless;
mod lossy;

/// Whether `bytes` is a RIFF file of type WEBP.
#[must_use]
pub fn is_webp(bytes: &[u8]) -> bool {
    bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WEBP")
}

/// A chunk: its four-character code and its payload.
struct Chunk<'a> {
    fourcc: [u8; 4],
    /// The payload, as long as the chunk's header says.
    payload: &'a [u8],
    /// The payload with the byte that pads an odd-length one, when the file
    /// has it. This is what libwebp's demuxer -- the route Pillow and the
    /// browsers' animation paths take -- hands a picture's decoder, so a
    /// bitstream that overruns its chunk by a byte reads the padding where
    /// a stricter reading would call the file truncated. Pictures are decoded
    /// from this, so that such a file decodes, or fails, as it does there.
    padded: &'a [u8],
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
        let end = start.checked_add(size)?.checked_add(size & 1)?;
        let padded = bytes.get(start..end).unwrap_or(payload);
        at = end;
        Some(Chunk {
            fourcc,
            payload,
            padded,
        })
    })
}

/// What the file holds, found by walking its chunks once.
enum Content<'a> {
    Lossless(&'a [u8]),
    /// A VP8 key frame (padded), the length its chunk declares, and the
    /// `ALPH` chunk before it if there is one.
    Lossy {
        frame: &'a [u8],
        declared: usize,
        alpha: Option<&'a [u8]>,
    },
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
                content: Content::Lossless(first.padded),
            })
        }
        b"VP8 " => {
            let (width, height) = lossy::dimensions(first.payload)?;
            Ok(Layout {
                width,
                height,
                content: Content::Lossy {
                    frame: first.padded,
                    declared: first.payload.len(),
                    alpha: None,
                },
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
            // The alpha plane is the last `ALPH` before the frame, as libwebp
            // takes it; a lossless frame has its own alpha and ignores one.
            // And a lossy frame's is used only if the header's alpha flag is
            // set: libwebp's demuxer, which Pillow and the browsers read
            // WebP through, drops it otherwise ("Clear any alpha when the
            // alpha flag is missing") -- unread, so a broken one breaks
            // nothing.
            let alpha_flag = flags & 0x10 != 0;
            let mut alpha = None;
            for chunk in chunks {
                match &chunk.fourcc {
                    b"ALPH" if alpha_flag => alpha = Some(chunk.payload),
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
                            content: Content::Lossless(chunk.padded),
                        });
                    }
                    b"VP8 " => {
                        let own = lossy::dimensions(chunk.payload)?;
                        if own != (width, height) {
                            return Err(ImageError::Malformed(
                                "a VP8 picture a different size from its canvas",
                            ));
                        }
                        return Ok(Layout {
                            width,
                            height,
                            content: Content::Lossy {
                                frame: chunk.padded,
                                declared: chunk.payload.len(),
                                alpha,
                            },
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
/// [`ImageError::Unsupported`] for an animated one, for now;
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
            let (width, height, mut pixels) = lossless::decode(payload, limits)?;
            // A stream whose header says it has no alpha is shown opaque,
            // whatever its pixels' alpha: the format calls the bit a hint that
            // must not change the decode, and it does not -- but Pillow and
            // Firefox (through libwebp's `WebPGetFeatures`) and Chrome's
            // simple-format path all show such a picture without its alpha.
            // An encoder sets the bit whenever a pixel is not opaque, so only
            // a damaged or hand-made file can tell the difference.
            if !lossless::alpha_hint(payload) {
                for pixel in &mut pixels {
                    *pixel |= 0xFF00_0000;
                }
            }
            Ok(Image {
                width,
                height,
                pixels,
            })
        }
        Content::Lossy {
            frame,
            declared,
            alpha,
        } => {
            let decoded = lossy::decode(frame, declared, limits)?;
            let plane = alpha
                .map(|chunk| alpha::decode(chunk, layout.width, layout.height, limits))
                .transpose()?;
            Ok(Image {
                width: layout.width,
                height: layout.height,
                pixels: decoded.to_argb(plane.as_deref()),
            })
        }
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
