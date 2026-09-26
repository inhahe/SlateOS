//! ICO and CUR -- Windows icons and cursors -- decoded as Chrome decodes them.
//!
//! # What an icon file is
//!
//! A six-byte directory header -- reserved, then 1 for an icon or 2 for a
//! cursor, then how many images -- and a sixteen-byte entry for each: its
//! width and height (0 meaning 256), a colour count, a bit depth (a cursor
//! keeps its hot spot there instead), its size, and where it is. Each image is
//! a PNG, or a BMP without its file header whose height counts twice: the
//! colour bitmap, then a one-bit "AND" mask whose set bits are the pixels to
//! leave transparent.
//!
//! # Chrome's decoder
//!
//! Chrome reads icons with its older C++ decoders -- `ICOImageDecoder`, and
//! `BMPImageReader` in its icon mode -- not with the image-rs one it now
//! reads plain BMPs with (see [`crate::bmp`]), and this module ports those:
//!
//! - **One image is shown: the best** -- the largest, then the one of most
//!   bits per pixel -- and only it is decoded. If it will not decode, the icon
//!   does not show, whatever the others hold.
//! - **Its size is its directory entry's**, and a PNG or BMP of any other size
//!   is refused. Every image must lie past the directory.
//! - **A BMP's AND mask makes pixels transparent** unless the BMP carries real
//!   alpha: 32 bits a pixel, with an alpha mask, and some alpha that is not
//!   zero. A 32-bit icon whose alpha is all zero was written for a reader that
//!   ignores alpha, so it is shown opaque, masked.
//! - **An OS/2 BMP header is refused**, as are OS/2's compressions; so is any
//!   file that stops short, the mask included.
//! - **Run-length data is read strictly**, as Chrome's older reader reads it:
//!   an absolute run or a delta past the end of its row is an error, and
//!   pixels a run skips stay transparent.
//! - **Two entries equally good** (the same size and depth) are taken in
//!   directory order. Chrome sorts with an unstable sort, which for such a
//!   pair may pick either; no real icon has two.
//! - **A V5 header's embedded colour profile** must be in the file, as in
//!   Chrome; unlike Chrome, what is in it is not checked, since profiles are
//!   not applied (see the crate's docs).
//!
//! # Hostile input
//!
//! As for the other decoders: nothing here panics, every read is bounded by
//! the bytes present, and the picture -- at most 256 by 256, since it must
//! match its directory entry -- is checked against [`Limits`] before its
//! buffer exists.

use alloc::vec;
use alloc::vec::Vec;

use crate::bmp::{argb, le16, le32, widen};
use crate::{Image, ImageError, ImageResult, Limits};

/// Whether `bytes` begins like an icon (`00 00 01 00`) or a cursor
/// (`00 00 02 00`).
#[must_use]
pub fn is_ico(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0, 0, 1, 0]) || bytes.starts_with(&[0, 0, 2, 0])
}

const DIRECTORY_SIZE: usize = 6;
const ENTRY_SIZE: usize = 16;
const ICON: u16 = 1;
const CURSOR: u16 = 2;

/// A directory entry, as far as choosing and checking an image needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry {
    width: u32,
    height: u32,
    bit_count: u16,
    offset: usize,
}

/// Read the directory (`ProcessDirectory`, `ProcessDirectoryEntries`),
/// best entry first.
fn directory(data: &[u8]) -> ImageResult<Vec<Entry>> {
    let head = data.get(..DIRECTORY_SIZE).ok_or(ImageError::Truncated)?;
    let file_type = le16(head, 2);
    let count = usize::from(le16(head, 4));
    if (file_type != ICON && file_type != CURSOR) || count == 0 {
        return Err(ImageError::Malformed("an icon directory of no icons"));
    }
    // At most 65535 entries of 16 bytes: this cannot overflow.
    let end = DIRECTORY_SIZE.saturating_add(count.saturating_mul(ENTRY_SIZE));
    let table = data.get(DIRECTORY_SIZE..end).ok_or(ImageError::Truncated)?;
    let mut entries: Vec<Entry> = table
        .chunks_exact(ENTRY_SIZE)
        .map(|raw| {
            let side = |byte: Option<&u8>| match byte.copied().unwrap_or(0) {
                0 => 256,
                n => u32::from(n),
            };
            let mut bit_count = if file_type == CURSOR { 0 } else { le16(raw, 6) };
            if bit_count == 0 {
                // Only a colour count: the fewest bits that index it.
                let colours = side(raw.get(2));
                let bits = u32::BITS.saturating_sub(colours.saturating_sub(1).leading_zeros());
                bit_count = u16::try_from(bits).unwrap_or(0);
            }
            Entry {
                width: side(raw.first()),
                height: side(raw.get(1)),
                bit_count,
                offset: le32(raw, 12) as usize,
            }
        })
        .collect();
    if entries.iter().any(|entry| entry.offset < end) {
        return Err(ImageError::Malformed("an icon image inside its directory"));
    }
    // Largest first, then deepest; equals in directory order.
    entries.sort_by(|a, b| {
        let area = |e: &Entry| e.width.saturating_mul(e.height);
        area(b).cmp(&area(a)).then(b.bit_count.cmp(&a.bit_count))
    });
    Ok(entries)
}

/// The icon's size: its best image's, as its directory gives it.
///
/// # Errors
///
/// [`ImageError::Truncated`] or [`ImageError::Malformed`] for a directory
/// too short or broken to say.
pub fn dimensions(bytes: &[u8]) -> ImageResult<(u32, u32)> {
    let best = directory(bytes)?
        .first()
        .copied()
        .ok_or(ImageError::Malformed("an icon directory of no icons"))?;
    Ok((best.width, best.height))
}

/// Decode an icon or cursor: its best image.
///
/// # Errors
///
/// [`ImageError::TooLarge`] past `limits`; [`ImageError::Truncated`] for a
/// file that stops short; [`ImageError::Malformed`] or
/// [`ImageError::Unsupported`] for one Chrome would not show either.
pub fn decode(bytes: &[u8], limits: Limits) -> ImageResult<Image> {
    let best = directory(bytes)?
        .first()
        .copied()
        .ok_or(ImageError::Malformed("an icon directory of no icons"))?;
    let pixels = u64::from(best.width).saturating_mul(u64::from(best.height));
    if pixels > limits.max_pixels {
        return Err(ImageError::TooLarge {
            pixels,
            limit: limits.max_pixels,
        });
    }
    let image = bytes.get(best.offset..).unwrap_or(&[]);
    if image.len() < 4 {
        return Err(ImageError::Truncated);
    }
    if image.starts_with(b"\x89PNG") {
        if crate::png::dimensions(image)? != (best.width, best.height) {
            return Err(ImageError::Malformed(
                "a PNG in an icon a different size from its entry",
            ));
        }
        return crate::png::decode(image, limits);
    }
    decode_bmp(bytes, best, limits)
}

/// Decode an icon averaged down to fit `max_w` x `max_h`.
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

// ---------------------------------------------------------------------------
// BMPImageReader, in its icon mode
// ---------------------------------------------------------------------------

/// Compression methods, as the older reader names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Compression {
    Rgb,
    Rle8,
    Rle4,
    Bitfields,
    AlphaBitfields,
}

/// The info header, as `ReadInfoHeader` leaves it.
struct Header {
    size: u32,
    width: u32,
    height: u32,
    top_down: bool,
    bit_count: u16,
    compression: Compression,
    colors_used: u32,
    masks: [u32; 4],
    rgb_masks_in_header: bool,
    alpha_mask_in_header: bool,
    /// An embedded ICC profile's place in the file, and its size.
    profile: Option<(usize, usize)>,
}

/// `ReadInfoHeaderSize`, `ProcessInfoHeader`, `ReadInfoHeader` and
/// `IsInfoHeaderValid`, for a BMP at `at` inside an icon.
#[allow(
    clippy::too_many_lines,
    reason = "a transcription of four short C++ functions that read one header, kept together to be read against them"
)]
fn read_header(data: &[u8], at: usize) -> ImageResult<Header> {
    let size = le32(data.get(at..).unwrap_or(&[]), 0);
    if data.len().saturating_sub(at) < 4 {
        return Err(ImageError::Truncated);
    }
    let windows_size = matches!(size, 40 | 52 | 56 | 108 | 124);
    let os2_size =
        (16..=64).contains(&size) && (size.is_multiple_of(4) || size == 42 || size == 46);
    if size != 12 && !windows_size && !os2_size {
        return Err(ImageError::Unsupported(
            "an icon image header of an unknown size",
        ));
    }
    let header = data
        .get(at..at.saturating_add(size as usize))
        .filter(|h| h.len() == size as usize)
        .ok_or(ImageError::Truncated)?;
    // Every OS/2 header is refused in an icon ("only Windows V3+ has ICOs"),
    // and so is a Windows header whose compression turns out to be OS/2's.
    if size == 12 || !windows_size {
        return Err(ImageError::Malformed("an OS/2 bitmap in an icon"));
    }
    #[allow(clippy::cast_possible_wrap, reason = "the fields are signed")]
    let width = le32(header, 4) as i32;
    // The height counts the AND mask too; C's division, towards zero.
    #[allow(clippy::cast_possible_wrap, reason = "the fields are signed")]
    let height = (le32(header, 8) as i32) / 2;
    let top_down = height < 0;
    let height = height.saturating_abs();
    let bit_count = le16(header, 14);
    let raw = if size >= 20 { le32(header, 16) } else { 0 };
    let compression = match raw {
        // OS/2's Huffman and RLE24, which an icon may not hold.
        3 if bit_count == 1 => return Err(ImageError::Malformed("an OS/2 bitmap in an icon")),
        4 if bit_count == 24 => return Err(ImageError::Malformed("an OS/2 bitmap in an icon")),
        0 => Compression::Rgb,
        1 => Compression::Rle8,
        2 => Compression::Rle4,
        3 => Compression::Bitfields,
        6 => Compression::AlphaBitfields,
        _ => {
            return Err(ImageError::Unsupported(
                "an icon image of an unknown compression",
            ));
        }
    };
    let colors_used = if size >= 36 { le32(header, 32) } else { 0 };
    let rgb_masks_in_header = size >= 52;
    let alpha_mask_in_header = size >= 56;
    let mut masks = [0u32; 4];
    if rgb_masks_in_header {
        masks = [le32(header, 40), le32(header, 44), le32(header, 48), 0];
    }
    if alpha_mask_in_header {
        if let Some(mask) = masks.get_mut(3) {
            *mask = le32(header, 52);
        }
    }
    let mut profile = None;
    if size >= 120 && le32(header, 56) == 0x4D42_4544 {
        let offset = at.saturating_add(le32(header, 112) as usize);
        profile = Some((offset, le32(header, 116) as usize));
    }

    // IsInfoHeaderValid.
    if width <= 0 || height == 0 {
        return Err(ImageError::Malformed("an icon image of no size"));
    }
    let valid_depth = matches!(bit_count, 1 | 2 | 4 | 8 | 16 | 24 | 32);
    let valid = valid_depth
        && match compression {
            Compression::Rgb => true,
            Compression::Rle8 => bit_count <= 8,
            Compression::Rle4 => bit_count <= 4,
            Compression::Bitfields | Compression::AlphaBitfields => {
                bit_count == 16 || bit_count == 32
            }
        };
    if !valid {
        return Err(ImageError::Malformed(
            "an icon image of a bit depth its compression cannot have",
        ));
    }
    if width >= 1 << 16 || height >= 1 << 16 {
        return Err(ImageError::Malformed(
            "an icon image wider or taller than 65535",
        ));
    }
    // A palette longer than the depth allows, or of no stated length, is as
    // long as the depth allows; a run-length compression fixes the depth.
    let mut colors_used = colors_used;
    if bit_count < 16 {
        let most = 1u32 << bit_count;
        if colors_used == 0 || colors_used > most {
            colors_used = most;
        }
    }
    let bit_count = match compression {
        Compression::Rle8 => 8,
        Compression::Rle4 => 4,
        _ => bit_count,
    };
    Ok(Header {
        size,
        width: width.unsigned_abs(),
        height: height.unsigned_abs(),
        top_down,
        bit_count,
        compression,
        colors_used,
        masks,
        rgb_masks_in_header,
        alpha_mask_in_header,
        profile,
    })
}

/// A 16- to 32-bit pixel's channels: where each is, how wide (for widening),
/// and its mask.
#[derive(Clone, Copy, Debug, Default)]
struct Channel {
    mask: u32,
    shift: u32,
    /// Bits under 8, widened through a table; 0 for 8 or more.
    narrow: u32,
}

impl Channel {
    fn read(self, pixel: u32) -> u8 {
        let value = (pixel & self.mask).checked_shr(self.shift).unwrap_or(0);
        if self.narrow == 0 {
            #[allow(clippy::cast_possible_truncation, reason = "a uint8_t in C")]
            {
                value as u8
            }
        } else {
            widen(value, self.narrow)
        }
    }
}

/// `ProcessBitmasks`: the four channels, from the masks. Masks are cut to the
/// bit depth; ones that overlap, or have a gap, are refused.
fn channels(header: &Header, mut masks: [u32; 4]) -> ImageResult<[Channel; 4]> {
    let mut out = [Channel::default(); 4];
    for i in 0..4 {
        let Some(mask) = masks.get_mut(i) else {
            continue;
        };
        if header.bit_count < 32 {
            *mask &= (1u32 << header.bit_count).wrapping_sub(1);
        }
        let mask = *mask;
        if mask == 0 {
            continue;
        }
        if masks.iter().take(i).any(|&earlier| earlier & mask != 0) {
            return Err(ImageError::Malformed("icon colour masks that overlap"));
        }
        let shift = mask.trailing_zeros();
        let bits = (mask >> shift).trailing_ones();
        if (mask >> shift).checked_shr(bits).unwrap_or(0) != 0 {
            return Err(ImageError::Malformed(
                "an icon colour mask with a gap in it",
            ));
        }
        let channel = if bits >= 8 {
            Channel {
                mask,
                shift: shift.saturating_add(bits.saturating_sub(8)),
                narrow: 0,
            }
        } else {
            Channel {
                mask,
                shift,
                narrow: bits,
            }
        };
        if let Some(slot) = out.get_mut(i) {
            *slot = channel;
        }
    }
    Ok(out)
}

/// A position in the file; running out is [`ImageError::Truncated`].
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> ImageResult<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or(ImageError::Truncated)?;
        let bytes = self.data.get(self.at..end).ok_or(ImageError::Truncated)?;
        self.at = end;
        Ok(bytes)
    }
}

/// The picture being drawn, and where drawing is, as the older reader walks
/// it: from the bottom row up, unless top-down.
struct Canvas {
    pixels: Vec<u32>,
    width: usize,
    height: usize,
    top_down: bool,
    x: usize,
    /// The row being drawn, counted in the order rows are stored.
    row: usize,
    seen_zero_alpha: bool,
    seen_non_zero_alpha: bool,
}

impl Canvas {
    /// Whether `rows` more rows would be past the last (`PastEndOfImage`).
    fn past_end(&self, rows: usize) -> bool {
        self.row.saturating_add(rows) >= self.height
    }

    fn set(&mut self, colour: u32) {
        let y = if self.top_down {
            self.row
        } else {
            self.height.saturating_sub(1).saturating_sub(self.row)
        };
        let at = y.saturating_mul(self.width).saturating_add(self.x);
        if self.x < self.width {
            if let Some(pixel) = self.pixels.get_mut(at) {
                *pixel = colour;
            }
        }
        self.x = self.x.saturating_add(1);
    }

    fn next_row(&mut self) {
        self.x = 0;
        self.row = self.row.saturating_add(1);
    }
}

/// Decode the BMP of `entry` (`DecodeBMP`, in icon mode).
fn decode_bmp(data: &[u8], entry: Entry, limits: Limits) -> ImageResult<Image> {
    let header = read_header(data, entry.offset)?;
    if let Some((offset, size)) = header.profile {
        let present = offset <= data.len() && data.len().saturating_sub(offset) >= size;
        if !present {
            return Err(ImageError::Truncated);
        }
        // Chrome makes a colour profile of it, and there is none to make of
        // nothing. (What a longer one holds is not checked; see the docs.)
        if size == 0 {
            return Err(ImageError::Malformed(
                "an icon image's empty colour profile",
            ));
        }
    }
    if (header.width, header.height) != (entry.width, entry.height) {
        return Err(ImageError::Malformed(
            "a BMP in an icon a different size from its entry",
        ));
    }
    let count = (header.width as usize).saturating_mul(header.height as usize);
    if count.saturating_mul(4) > limits.max_decompressed_bytes {
        return Err(ImageError::TooLarge {
            pixels: count.saturating_mul(4) as u64,
            limit: limits.max_decompressed_bytes as u64,
        });
    }
    let mut r = Reader {
        data,
        at: entry.offset.saturating_add(header.size as usize),
    };

    let mut chans = [Channel::default(); 4];
    let mut palette = Vec::new();
    if header.bit_count >= 16 {
        let mut masks = header.masks;
        if !matches!(
            header.compression,
            Compression::Bitfields | Compression::AlphaBitfields
        ) {
            masks = if header.bit_count == 16 {
                [0x7C00, 0x03E0, 0x001F, masks[3]]
            } else {
                [0x00FF_0000, 0xFF00, 0xFF, masks[3]]
            };
        } else if !header.rgb_masks_in_header {
            let alpha = header.compression == Compression::AlphaBitfields;
            let stored = r.take(if alpha { 16 } else { 12 })?;
            masks = [
                le32(stored, 0),
                le32(stored, 4),
                le32(stored, 8),
                if alpha { le32(stored, 12) } else { masks[3] },
            ];
        }
        if !header.alpha_mask_in_header && header.compression != Compression::AlphaBitfields {
            // In an icon, an uncompressed 32-bit image's fourth byte is alpha.
            let alpha = header.compression != Compression::Bitfields && header.bit_count == 32;
            masks[3] = if alpha { 0xFF00_0000 } else { 0 };
        }
        chans = channels(&header, masks)?;
    } else {
        let table = r.take((header.colors_used as usize).saturating_mul(4))?;
        palette = table
            .chunks_exact(4)
            .map(|bgr| match *bgr {
                [b, g, r, _] => argb(0xFF, r, g, b),
                _ => argb(0xFF, 0, 0, 0),
            })
            .collect();
    }

    let mut canvas = Canvas {
        pixels: vec![0; count],
        width: header.width as usize,
        height: header.height as usize,
        top_down: header.top_down,
        x: 0,
        row: 0,
        seen_zero_alpha: false,
        seen_non_zero_alpha: false,
    };
    match header.compression {
        Compression::Rle8 | Compression::Rle4 => {
            decode_rle(&mut r, &header, &palette, &mut canvas)?;
        }
        _ => {
            while !canvas.past_end(0) {
                run(
                    &mut r,
                    header.bit_count,
                    &palette,
                    chans,
                    &mut canvas,
                    false,
                    None,
                )?;
                canvas.next_row();
            }
        }
    }

    // The AND mask, unless the pixels had alpha of their own.
    if header.bit_count < 16 || chans[3].mask == 0 || !canvas.seen_non_zero_alpha {
        canvas.x = 0;
        canvas.row = 0;
        while !canvas.past_end(0) {
            run(&mut r, 1, &palette, chans, &mut canvas, true, None)?;
            canvas.next_row();
        }
    }
    Ok(Image {
        width: header.width,
        height: header.height,
        pixels: canvas.pixels,
    })
}

/// `ProcessNonRLEData`: `bits`-bit pixels from `canvas.x` -- `count` of
/// them if given (an absolute run, padded to 16 bits), otherwise the whole
/// row (padded to 32). `and_mask` draws the AND mask instead of pixels.
#[allow(
    clippy::too_many_arguments,
    reason = "the older reader's state, passed rather than bundled into a struct used only here"
)]
fn run(
    r: &mut Reader<'_>,
    bits: u16,
    palette: &[u32],
    chans: [Channel; 4],
    canvas: &mut Canvas,
    and_mask: bool,
    count: Option<usize>,
) -> ImageResult<()> {
    let pixels = count.unwrap_or(canvas.width);
    let end_x = canvas.x.saturating_add(pixels);
    if end_x > canvas.width {
        return Err(ImageError::Malformed("an icon run past the end of its row"));
    }
    let unpadded = if bits < 16 {
        let per_byte = 8usize.checked_div(usize::from(bits)).unwrap_or(1);
        pixels.div_ceil(per_byte.max(1))
    } else {
        pixels.saturating_mul(usize::from(bits / 8))
    };
    let padded = if count.is_some() {
        unpadded.div_ceil(2).saturating_mul(2)
    } else {
        unpadded.div_ceil(4).saturating_mul(4)
    };
    let stored = r.take(padded)?;
    let stored = stored.get(..unpadded).unwrap_or(&[]);
    if bits < 16 {
        let mask = u8::MAX
            .checked_shr(8u32.saturating_sub(u32::from(bits)))
            .unwrap_or(u8::MAX);
        let shift = 8u32.saturating_sub(u32::from(bits));
        'bytes: for &byte in stored {
            let mut byte = byte;
            for _ in 0..8u16.checked_div(bits).unwrap_or(1) {
                if canvas.x >= end_x {
                    break 'bytes;
                }
                let index = byte.checked_shr(shift).unwrap_or(0) & mask;
                if and_mask {
                    if index != 0 {
                        canvas.set(0);
                    } else {
                        canvas.x = canvas.x.saturating_add(1);
                    }
                } else {
                    let colour = palette
                        .get(usize::from(index))
                        .copied()
                        .unwrap_or(argb(0xFF, 0, 0, 0));
                    canvas.set(colour);
                }
                byte = byte.checked_shl(u32::from(bits)).unwrap_or(0);
            }
        }
    } else {
        let bytes_per_pixel = usize::from(bits / 8);
        for bytes in stored.chunks_exact(bytes_per_pixel.max(1)) {
            if canvas.x >= end_x {
                break;
            }
            let pixel = match *bytes {
                [a, b] => u32::from(u16::from_le_bytes([a, b])),
                [a, b, c] => u32::from_le_bytes([a, b, c, 0]),
                [a, b, c, d] => u32::from_le_bytes([a, b, c, d]),
                _ => 0,
            };
            let mut alpha = if chans[3].mask == 0 {
                0xFF
            } else {
                chans[3].read(pixel)
            };
            // An alpha channel of zeros is not alpha: shown opaque until a
            // pixel with some turns up -- then everything so far was clear.
            if !canvas.seen_non_zero_alpha && alpha == 0 {
                canvas.seen_zero_alpha = true;
                alpha = 0xFF;
            } else {
                canvas.seen_non_zero_alpha = true;
                if canvas.seen_zero_alpha {
                    canvas.pixels.fill(0);
                    canvas.seen_zero_alpha = false;
                }
            }
            let colour = argb(
                alpha,
                chans[0].read(pixel),
                chans[1].read(pixel),
                chans[2].read(pixel),
            );
            canvas.set(colour);
        }
    }
    Ok(())
}

/// `ProcessRLEData`, strictly: rows change at an end of line or a delta, a
/// delta or an absolute run past its row is an error, and an encoded run is
/// cut at the row's end. Skipped pixels stay transparent.
fn decode_rle(
    r: &mut Reader<'_>,
    header: &Header,
    palette: &[u32],
    canvas: &mut Canvas,
) -> ImageResult<()> {
    let black = argb(0xFF, 0, 0, 0);
    loop {
        let pair = r.take(2)?;
        let (count, code) = (
            pair.first().copied().unwrap_or(0),
            pair.get(1).copied().unwrap_or(0),
        );
        let past_end = canvas.past_end(0);
        if (count != 0 || code != 1) && past_end {
            return Err(ImageError::Malformed(
                "icon run-length data past its last row",
            ));
        }
        if count == 0 {
            match code {
                0 => canvas.next_row(),
                1 => return Ok(()),
                2 => {
                    let delta = r.take(2)?;
                    let dx = usize::from(delta.first().copied().unwrap_or(0));
                    let dy = usize::from(delta.get(1).copied().unwrap_or(0));
                    if canvas.x.saturating_add(dx) > canvas.width || canvas.past_end(dy) {
                        return Err(ImageError::Malformed(
                            "an icon run-length delta past its picture",
                        ));
                    }
                    canvas.x = canvas.x.saturating_add(dx);
                    canvas.row = canvas.row.saturating_add(dy);
                }
                n => {
                    let chans = [Channel::default(); 4];
                    let count = Some(usize::from(n));
                    run(r, header.bit_count, palette, chans, canvas, false, count)?;
                }
            }
        } else {
            let end_x = canvas
                .x
                .saturating_add(usize::from(count))
                .min(canvas.width);
            let indices = if header.compression == Compression::Rle4 {
                [code >> 4, code & 0xF]
            } else {
                [code, code]
            };
            let mut which = 0;
            while canvas.x < end_x {
                let index = usize::from(indices.get(which).copied().unwrap_or(0));
                canvas.set(palette.get(index).copied().unwrap_or(black));
                which ^= 1;
            }
        }
    }
}
