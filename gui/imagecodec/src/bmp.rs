//! BMP -- the Windows bitmap -- decoded as Chrome decodes it.
//!
//! # What a BMP is
//!
//! A 14-byte file header (`BM`, then where the pixels start), a bitmap header
//! whose own size says which one it is -- OS/2 1.x's 12 bytes, Windows' 40
//! (`BITMAPINFOHEADER`) and its 52-, 56-, 108- and 124-byte successors (V2 to
//! V5), or one of OS/2 2.x's 16 to 64 -- then colour masks or a palette, then
//! rows of pixels, bottom row first unless the height is negative. A pixel is
//! a palette index of 1, 2, 4 or 8 bits; a 16- or 32-bit word cut into channels
//! by masks; or 24- or 32-bit blue-green-red. Rows are stored plainly, or
//! run-length encoded (RLE8, RLE4, and OS/2's RLE24).
//!
//! # Chrome's decoder, exactly
//!
//! BMP's documentation leaves much unsaid, and decoders disagree about all of
//! it: what an index past the palette means, what pixels a run-length stream
//! skips become, whether a 32-bit picture's fourth byte is alpha, what a file
//! that stops short shows. Chrome answers each through its BMP decoder, which
//! is image-rs 0.25.10's (inside Skia, as `rust_bmp`) with four patches of
//! Chromium's own, read in its lenient mode; this module is a port of that
//! decoder, and the tests hold it to the original, compiled from the same
//! source, pixel for pixel and refusal for refusal (`tests/data/generate_bmp.py`).
//! Its answers, where others differ:
//!
//! - **Any error means no picture.** A file cut short, anywhere -- the last
//!   row's padding included -- or a run-length stream that stops before it is
//!   done, is refused, as Chrome refuses to show it.
//! - **Pixels a run-length stream skips are black**, as Windows shows them
//!   (Firefox and Chrome's older decoder left them transparent).
//! - **An index past the palette is black**, and a palette longer than its
//!   bit depth allows is cut to fit.
//! - **A 32-bit `BITMAPINFOHEADER` picture is opaque**: its fourth byte is not
//!   alpha, since most files that fill it did not mean it as alpha. A V4 or V5
//!   header with an alpha mask makes it alpha -- even uncompressed -- and so
//!   does a bit-field picture's alpha mask. An alpha channel of all zeros is
//!   taken at its word: transparent.
//! - **Colour spaces and ICC profiles are not applied**, as for PNG and JPEG
//!   (see the crate's docs). But a V5 header naming an embedded profile that
//!   lies past the end of the file makes Chrome refuse the file, and so it
//!   does here.
//!
//! # Hostile input
//!
//! Nothing here panics for any input. Every read is bounded by the bytes
//! present, the picture is checked against [`Limits`] before its buffer
//! exists, and nothing else is sized from the file: the palette is a fixed 256
//! entries whatever the file claims.

use alloc::vec;

use crate::{Image, ImageError, ImageResult, Limits};

/// Whether `bytes` begins like a BMP: `BM`, which is all Chrome looks at.
#[must_use]
pub fn is_bmp(bytes: &[u8]) -> bool {
    bytes.starts_with(b"BM")
}

/// The file header: `BM`, the file's size, four reserved bytes, and where the
/// pixels start.
const FILE_HEADER_SIZE: usize = 14;

/// Compression methods (`biCompression`).
const BI_RGB: u32 = 0;
const BI_RLE8: u32 = 1;
const BI_RLE4: u32 = 2;
const BI_BITFIELDS: u32 = 3;
/// JPEG for Windows, OS/2 2.x's RLE24 when the header is OS/2's.
const BI_JPEG: u32 = 4;
const BI_PNG: u32 = 5;
const BI_ALPHABITFIELDS: u32 = 6;
const BI_CMYK: u32 = 11;
const BI_CMYKRLE4: u32 = 13;

/// The largest width, and height of a bottom-up picture, that is accepted.
const MAX_WIDTH_HEIGHT: i32 = 0xFFFF;

/// A V5 header's colour-space type for an embedded ICC profile: `MBED`.
const PROFILE_EMBEDDED: u32 = 0x4D42_4544;

/// A palette always has 256 entries: indices past the file's own read black.
const PALETTE_ENTRIES: usize = 256;

/// Which bitmap header the file has, from its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// OS/2 1.x's `BITMAPCOREHEADER`, 12 bytes.
    Core,
    /// `BITMAPINFOHEADER`, 40 bytes.
    Info,
    /// `BITMAPV2INFOHEADER`, 52 bytes: masks for red, green and blue.
    V2,
    /// `BITMAPV3INFOHEADER`, 56 bytes: and for alpha.
    V3,
    /// `BITMAPV4HEADER`, 108 bytes: and a colour space.
    V4,
    /// `BITMAPV5HEADER`, 124 bytes: and an ICC profile.
    V5,
    /// OS/2 2.x's, 16 to 64 bytes: a `BITMAPINFOHEADER` cut short or extended.
    Os2,
}

/// How the pixels are stored (image-rs's `ImageType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    Palette,
    Rgb16,
    Rgb24,
    Rgb32,
    Rgba32,
    Rle8,
    Rle4,
    Rle24,
    Bitfields16,
    Bitfields32,
}

/// One channel of a 16- or 32-bit pixel: where it is, and how wide.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Bitfield {
    shift: u32,
    len: u32,
}

/// How 1- to 6-bit channels are widened to 8 (`round(v * 255 / max)`).
const WIDEN_1: [u8; 2] = [0, 255];
const WIDEN_2: [u8; 4] = [0, 85, 170, 255];
const WIDEN_3: [u8; 8] = [0, 36, 73, 109, 146, 182, 219, 255];
const WIDEN_4: [u8; 16] = [
    0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255,
];
const WIDEN_5: [u8; 32] = [
    0, 8, 16, 25, 33, 41, 49, 58, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 165, 173,
    181, 189, 197, 206, 214, 222, 230, 239, 247, 255,
];
const WIDEN_6: [u8; 64] = [
    0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 45, 49, 53, 57, 61, 65, 69, 73, 77, 81, 85, 89, 93,
    97, 101, 105, 109, 113, 117, 121, 125, 130, 134, 138, 142, 146, 150, 154, 158, 162, 166, 170,
    174, 178, 182, 186, 190, 194, 198, 202, 206, 210, 215, 219, 223, 227, 231, 235, 239, 243, 247,
    251, 255,
];

impl Bitfield {
    /// A channel from its mask, which must be one run of set bits inside a
    /// pixel of `max_len` bits. A channel wider than 8 bits keeps its top 8.
    fn from_mask(mask: u32, max_len: u32) -> ImageResult<Self> {
        if mask == 0 {
            return Ok(Self::default());
        }
        let mut shift = mask.trailing_zeros();
        let mut len = (!(mask >> shift)).trailing_zeros();
        if len != mask.count_ones() {
            return Err(ImageError::Malformed("a BMP colour mask with a gap in it"));
        }
        if len.saturating_add(shift) > max_len {
            return Err(ImageError::Malformed(
                "a BMP colour mask wider than its pixel",
            ));
        }
        if len > 8 {
            shift = shift.saturating_add(len.saturating_sub(8));
            len = 8;
        }
        Ok(Self { shift, len })
    }

    /// The channel's value in `data`, widened to 8 bits.
    fn read(self, data: u32) -> u8 {
        widen(data.checked_shr(self.shift).unwrap_or(0), self.len)
    }
}

/// The low `len` bits of `value` widened to 8, as Chrome widens a channel of
/// a 16- or 32-bit pixel: `round(v * 255 / (2^len - 1))`. A length of 0 is a
/// channel that is not there, and reads 0; one over 8 is not a length this is
/// ever given.
pub(crate) fn widen(value: u32, len: u32) -> u8 {
    let at = |table: &[u8], mask: u32| table.get((value & mask) as usize).copied().unwrap_or(0);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "each arm masks to at most 8 bits first"
    )]
    match len {
        1 => at(&WIDEN_1, 0b1),
        2 => at(&WIDEN_2, 0b11),
        3 => at(&WIDEN_3, 0b111),
        4 => at(&WIDEN_4, 0b1111),
        5 => at(&WIDEN_5, 0b1_1111),
        6 => at(&WIDEN_6, 0b11_1111),
        7 => (((value & 0x7F) << 1) | ((value & 0x7F) >> 6)) as u8,
        8 => (value & 0xFF) as u8,
        _ => 0,
    }
}

/// The four channels of a 16- or 32-bit pixel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Bitfields {
    r: Bitfield,
    g: Bitfield,
    b: Bitfield,
    a: Bitfield,
}

impl Bitfields {
    /// Uncompressed 16-bit pixels: five bits each of red, green and blue.
    const RGB555: Self = Self {
        r: Bitfield { shift: 10, len: 5 },
        g: Bitfield { shift: 5, len: 5 },
        b: Bitfield { shift: 0, len: 5 },
        a: Bitfield { shift: 0, len: 0 },
    };

    fn from_masks(masks: [u32; 4], max_len: u32) -> ImageResult<Self> {
        let [r, g, b, a] = masks;
        Ok(Self {
            r: Bitfield::from_mask(r, max_len)?,
            g: Bitfield::from_mask(g, max_len)?,
            b: Bitfield::from_mask(b, max_len)?,
            a: Bitfield::from_mask(a, max_len)?,
        })
    }

    /// The pixel `data` as `0xAARRGGBB`; opaque unless `alpha` and the alpha
    /// mask has bits.
    fn argb(self, data: u32, alpha: bool) -> u32 {
        let a = if alpha && self.a.len != 0 {
            self.a.read(data)
        } else {
            0xFF
        };
        argb(a, self.r.read(data), self.g.read(data), self.b.read(data))
    }
}

pub(crate) const fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    u32::from_be_bytes([a, r, g, b])
}

pub(crate) fn le16(bytes: &[u8], at: usize) -> u16 {
    let byte = |i: usize| bytes.get(at.saturating_add(i)).copied().unwrap_or(0);
    u16::from_le_bytes([byte(0), byte(1)])
}

pub(crate) fn le32(bytes: &[u8], at: usize) -> u32 {
    let byte = |i: usize| bytes.get(at.saturating_add(i)).copied().unwrap_or(0);
    u32::from_le_bytes([byte(0), byte(1), byte(2), byte(3)])
}

/// A position in the file, reading forward; running out is [`ImageError::Truncated`].
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    const fn at(data: &'a [u8], at: usize) -> Self {
        Self { data, at }
    }

    fn take(&mut self, n: usize) -> ImageResult<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or(ImageError::Truncated)?;
        let bytes = self.data.get(self.at..end).ok_or(ImageError::Truncated)?;
        self.at = end;
        Ok(bytes)
    }

    fn byte(&mut self) -> ImageResult<u8> {
        let byte = *self.data.get(self.at).ok_or(ImageError::Truncated)?;
        self.at = self.at.saturating_add(1);
        Ok(byte)
    }
}

/// What the headers say (image-rs's `read_headers`).
#[derive(Debug)]
struct Header {
    width: u32,
    height: u32,
    top_down: bool,
    bit_count: u16,
    layout: Layout,
    colors_used: u32,
    /// Where the pixels start, from the file header.
    data_offset: usize,
    /// Where the palette starts: after the headers.
    palette_offset: usize,
    /// Three bytes a palette entry for OS/2 1.x, four for the rest.
    bytes_per_color: usize,
    bitfields: Bitfields,
    /// Whether the picture has alpha (image-rs's `add_alpha_channel`).
    alpha: bool,
    /// An embedded ICC profile's place in the file: offset and size.
    icc: Option<(u64, u32)>,
}

/// The fields of a `BITMAPINFOHEADER` after its size (image-rs's
/// `ParsedInfoHeader::parse`), from 36 bytes.
struct InfoFields {
    width: i32,
    height: i32,
    top_down: bool,
    bit_count: u16,
    compression: u32,
    colors_used: u32,
}

fn info_fields(buffer: &[u8]) -> ImageResult<InfoFields> {
    #[allow(clippy::cast_possible_wrap, reason = "the fields are signed")]
    let width = le32(buffer, 0) as i32;
    #[allow(clippy::cast_possible_wrap, reason = "the fields are signed")]
    let mut height = le32(buffer, 4) as i32;
    if width < 0 {
        return Err(ImageError::Malformed("a BMP of negative width"));
    }
    if width > MAX_WIDTH_HEIGHT || height > MAX_WIDTH_HEIGHT {
        return Err(ImageError::Malformed("a BMP wider or taller than 65535"));
    }
    if height == i32::MIN {
        return Err(ImageError::Malformed("a BMP of impossible height"));
    }
    let top_down = height < 0;
    if top_down {
        height = height.saturating_neg();
    }
    Ok(InfoFields {
        width,
        height,
        top_down,
        bit_count: le16(buffer, 10),
        compression: le32(buffer, 12),
        colors_used: le32(buffer, 28),
    })
}

/// The storage a compression method and bit depth make
/// (`image_type_from_compression`).
fn layout(compression: u32, bit_count: u16, kind: Kind) -> ImageResult<Layout> {
    match compression {
        BI_RGB => match bit_count {
            1 | 2 | 4 | 8 => Ok(Layout::Palette),
            16 => Ok(Layout::Rgb16),
            24 => Ok(Layout::Rgb24),
            32 => Ok(Layout::Rgb32),
            _ => Err(ImageError::Malformed("a BMP of an unknown bit depth")),
        },
        BI_RLE8 if bit_count == 8 => Ok(Layout::Rle8),
        BI_RLE4 if bit_count == 4 => Ok(Layout::Rle4),
        BI_RLE8 | BI_RLE4 => Err(ImageError::Malformed(
            "a run-length BMP of the wrong bit depth",
        )),
        BI_BITFIELDS | BI_ALPHABITFIELDS => match bit_count {
            16 => Ok(Layout::Bitfields16),
            32 => Ok(Layout::Bitfields32),
            _ => Err(ImageError::Malformed(
                "a bit-field BMP of the wrong bit depth",
            )),
        },
        BI_JPEG if kind == Kind::Os2 && bit_count == 24 => Ok(Layout::Rle24),
        BI_JPEG if kind == Kind::Os2 => Err(ImageError::Malformed(
            "an OS/2 RLE24 BMP of the wrong bit depth",
        )),
        BI_JPEG => Err(ImageError::Unsupported("a BMP holding a JPEG")),
        BI_PNG => Err(ImageError::Unsupported("a BMP holding a PNG")),
        BI_CMYK..=BI_CMYKRLE4 => Err(ImageError::Unsupported("a CMYK BMP")),
        _ => Err(ImageError::Malformed("a BMP of an unknown compression")),
    }
}

/// Read the file header, the bitmap header and any colour masks.
#[allow(
    clippy::too_many_lines,
    reason = "one transcription of image-rs's read_headers, kept whole to be read against it"
)]
fn read_header(data: &[u8]) -> ImageResult<Header> {
    let mut r = Reader::at(data, 0);
    let file = r.take(FILE_HEADER_SIZE)?;
    if !file.starts_with(b"BM") {
        return Err(ImageError::Malformed("a BMP without its signature"));
    }
    let data_offset = le32(file, 10) as usize;
    let size = le32(r.take(4)?, 0);
    let kind = match size {
        12 => Kind::Core,
        40 => Kind::Info,
        52 => Kind::V2,
        56 => Kind::V3,
        108 => Kind::V4,
        124 => Kind::V5,
        0..12 => {
            return Err(ImageError::Malformed(
                "a BMP header smaller than any there is",
            ));
        }
        16..=64 if size.is_multiple_of(4) || size == 42 || size == 46 => Kind::Os2,
        _ => return Err(ImageError::Unsupported("a BMP header of an unknown size")),
    };

    let (fields, layout) = if kind == Kind::Core {
        let core = r.take(8)?;
        let bit_count = le16(core, 6);
        let layout = match bit_count {
            1 | 4 | 8 => Layout::Palette,
            24 => Layout::Rgb24,
            _ => return Err(ImageError::Malformed("an OS/2 BMP of an unknown bit depth")),
        };
        let fields = InfoFields {
            width: i32::from(le16(core, 0)),
            height: i32::from(le16(core, 2)),
            top_down: false,
            bit_count,
            compression: BI_RGB,
            colors_used: 0,
        };
        (fields, layout)
    } else {
        let mut buffer = [0u8; 36];
        let remaining = size.saturating_sub(4) as usize;
        let read = remaining.min(buffer.len());
        if let Some(head) = buffer.get_mut(..read) {
            head.copy_from_slice(r.take(read)?);
        }
        // An OS/2 header's fields past a BITMAPINFOHEADER's, which nothing
        // reads. A Windows header's are left for what follows: its masks.
        if kind == Kind::Os2 {
            r.take(remaining.saturating_sub(buffer.len()))?;
        }
        let fields = info_fields(&buffer)?;
        let layout = layout(fields.compression, fields.bit_count, kind)?;
        (fields, layout)
    };
    if fields.width <= 0 || fields.height <= 0 {
        return Err(ImageError::Malformed("a BMP of no size"));
    }

    let mut header = Header {
        width: fields.width.unsigned_abs(),
        height: fields.height.unsigned_abs(),
        top_down: fields.top_down,
        bit_count: fields.bit_count,
        layout,
        colors_used: fields.colors_used,
        data_offset,
        palette_offset: 0,
        bytes_per_color: if kind == Kind::Core { 3 } else { 4 },
        bitfields: Bitfields::RGB555,
        alpha: false,
        icc: None,
    };

    // Colour masks: in the header from V2 on (which is where the reader now
    // is, 40 bytes in), after it for a BITMAPINFOHEADER -- the same place.
    let mut masks_after_header = 0;
    // A fourth mask, for alpha, is in the header from V3 on, or follows a
    // Windows header that says so -- not an OS/2 one, whatever its
    // compression code, since image-rs reads OS/2 bit fields as three masks.
    let alpha_bitfields = fields.compression == BI_ALPHABITFIELDS && kind != Kind::Os2;
    if matches!(layout, Layout::Bitfields16 | Layout::Bitfields32) {
        let four = matches!(kind, Kind::V3 | Kind::V4 | Kind::V5) || alpha_bitfields;
        let masks = r.take(if four { 16 } else { 12 })?;
        let alpha_mask = if four { le32(masks, 12) } else { 0 };
        let max_len = if layout == Layout::Bitfields16 {
            16
        } else {
            32
        };
        header.bitfields = Bitfields::from_masks(
            [le32(masks, 0), le32(masks, 4), le32(masks, 8), alpha_mask],
            max_len,
        )?;
        header.alpha = alpha_mask != 0;
        if matches!(kind, Kind::Info | Kind::V4 | Kind::V5) {
            masks_after_header = if alpha_bitfields { 16 } else { 12 };
        }
    } else if layout == Layout::Rgb32 && size >= 108 {
        // A V4 or V5 header's alpha mask makes an uncompressed 32-bit
        // picture's fourth byte alpha, whatever mask it is (Chromium's patch
        // 0002, lenient).
        let masks = r.take(16)?;
        if le32(masks, 12) != 0 {
            header.alpha = true;
            header.layout = Layout::Rgba32;
        }
    }

    if size >= 124 {
        let v5 = Reader::at(data, FILE_HEADER_SIZE.saturating_add(4)).take(120)?;
        let profile_offset = le32(v5, 108);
        let profile_size = le32(v5, 112);
        if le32(v5, 52) == PROFILE_EMBEDDED && profile_size != 0 && profile_offset != 0 {
            let offset = (FILE_HEADER_SIZE as u64).saturating_add(u64::from(profile_offset));
            header.icc = Some((offset, profile_size));
        }
    }

    header.palette_offset = FILE_HEADER_SIZE
        .saturating_add(size as usize)
        .saturating_add(masks_after_header);
    Ok(header)
}

/// Read the palette (`read_palette`): as many entries as the header says,
/// no more than the bit depth allows, padded with black to 256.
fn read_palette(data: &[u8], header: &Header) -> ImageResult<[[u8; 3]; PALETTE_ENTRIES]> {
    let most = 1usize << header.bit_count.min(8);
    let entries = match header.colors_used {
        0 => most,
        used => (used as usize).min(most),
    };
    let bytes = Reader::at(data, header.palette_offset)
        .take(entries.saturating_mul(header.bytes_per_color))?;
    let mut palette = [[0u8; 3]; PALETTE_ENTRIES];
    for (entry, bgr) in palette
        .iter_mut()
        .zip(bytes.chunks_exact(header.bytes_per_color))
    {
        if let [b, g, r, ..] = *bgr {
            *entry = [r, g, b];
        }
    }
    Ok(palette)
}

/// A BMP's width and height, from its headers alone.
///
/// # Errors
///
/// [`ImageError::Truncated`] or [`ImageError::Malformed`] for headers too
/// short or broken to say, [`ImageError::Unsupported`] for a kind of BMP
/// Chrome does not show either (one holding a JPEG or a PNG, say).
pub fn dimensions(bytes: &[u8]) -> ImageResult<(u32, u32)> {
    let header = read_header(bytes)?;
    Ok((header.width, header.height))
}

/// Decode a BMP to `0xAARRGGBB` pixels.
///
/// # Errors
///
/// [`ImageError::TooLarge`] past `limits`; [`ImageError::Truncated`] for a
/// file that stops short anywhere; [`ImageError::Malformed`] or
/// [`ImageError::Unsupported`] for one Chrome would not show either.
pub fn decode(bytes: &[u8], limits: Limits) -> ImageResult<Image> {
    let header = read_header(bytes)?;
    let pixels = u64::from(header.width).saturating_mul(u64::from(header.height));
    if pixels > limits.max_pixels {
        return Err(ImageError::TooLarge {
            pixels,
            limit: limits.max_pixels,
        });
    }
    let count = usize::try_from(pixels).map_err(|_| ImageError::TooLarge {
        pixels,
        limit: limits.max_pixels,
    })?;
    let buffer_bytes = count.saturating_mul(4);
    if buffer_bytes > limits.max_decompressed_bytes {
        return Err(ImageError::TooLarge {
            pixels: buffer_bytes as u64,
            limit: limits.max_decompressed_bytes as u64,
        });
    }

    let palette = if matches!(header.layout, Layout::Palette | Layout::Rle4 | Layout::Rle8) {
        read_palette(bytes, &header)?
    } else {
        [[0; 3]; PALETTE_ENTRIES]
    };
    if let Some((offset, size)) = header.icc {
        let end = offset
            .checked_add(u64::from(size))
            .ok_or(ImageError::Malformed("a BMP colour profile past any file"))?;
        if end > bytes.len() as u64 {
            return Err(ImageError::Truncated);
        }
    }

    // Opaque black: what a run-length stream's skipped pixels are.
    let mut out = vec![argb(0xFF, 0, 0, 0); count];
    let canvas = Canvas {
        pixels: &mut out,
        width: header.width as usize,
        height: header.height as usize,
        top_down: header.top_down,
    };
    match header.layout {
        Layout::Palette => decode_indexed(bytes, &header, &palette, canvas)?,
        Layout::Rle8 | Layout::Rle4 | Layout::Rle24 => {
            decode_rle(bytes, &header, &palette, canvas)?;
        }
        _ => decode_direct(bytes, &header, canvas)?,
    }
    Ok(Image {
        width: header.width,
        height: header.height,
        pixels: out,
    })
}

/// Decode a BMP averaged down to fit `max_w` x `max_h`, by the same rule as
/// any other thumbnail.
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

/// The output, and the order the file's rows go into it.
struct Canvas<'a> {
    pixels: &'a mut [u32],
    width: usize,
    height: usize,
    top_down: bool,
}

impl Canvas<'_> {
    /// The output row the file's `row`th goes to: the same row top-down, from
    /// the bottom otherwise.
    fn row(&mut self, row: usize) -> &mut [u32] {
        let index = if self.top_down {
            Some(row)
        } else {
            self.height
                .checked_sub(1)
                .and_then(|last| last.checked_sub(row))
        };
        let start = index.and_then(|i| i.checked_mul(self.width));
        let range = start.and_then(|s| Some(s..s.checked_add(self.width)?));
        range
            .and_then(|range| self.pixels.get_mut(range))
            .unwrap_or(&mut [])
    }
}

/// The bytes of each stored row, padding included: rows are whole multiples
/// of four bytes.
fn padded_row(bits_per_pixel: u16, width: usize) -> usize {
    let bits = usize::from(bits_per_pixel).saturating_mul(width);
    bits.div_ceil(32).saturating_mul(4)
}

/// Palette pixels, 1, 2, 4 or 8 bits each (`read_palettized_pixel_data`).
fn decode_indexed(
    data: &[u8],
    header: &Header,
    palette: &[[u8; 3]; PALETTE_ENTRIES],
    mut canvas: Canvas<'_>,
) -> ImageResult<()> {
    let bits = header.bit_count;
    let row_len = padded_row(bits, canvas.width);
    let per_byte = 8usize
        .checked_div(usize::from(bits.clamp(1, 8)))
        .unwrap_or(1);
    let mask = u8::MAX
        .checked_shr(u32::from(8u16.saturating_sub(bits.clamp(1, 8))))
        .unwrap_or(u8::MAX);
    let mut r = Reader::at(data, header.data_offset);
    for row in 0..canvas.height {
        let stored = r.take(row_len)?;
        let out = canvas.row(row);
        let indices = stored.iter().flat_map(|&byte| {
            (0..per_byte).map(move |i| {
                // Most significant bits first.
                let shift =
                    8usize.saturating_sub(usize::from(bits).saturating_mul(i.saturating_add(1)));
                (byte >> shift) & mask
            })
        });
        for (pixel, index) in out.iter_mut().zip(indices) {
            let [r, g, b] = palette.get(usize::from(index)).copied().unwrap_or([0; 3]);
            *pixel = argb(0xFF, r, g, b);
        }
    }
    Ok(())
}

/// 16-, 24- and 32-bit pixels, plain or by bit fields.
fn decode_direct(data: &[u8], header: &Header, mut canvas: Canvas<'_>) -> ImageResult<()> {
    let bytes_per_pixel = match header.layout {
        Layout::Rgb16 | Layout::Bitfields16 => 2,
        Layout::Rgb24 => 3,
        _ => 4,
    };
    let unpadded = canvas.width.saturating_mul(bytes_per_pixel);
    let row_len = unpadded.div_ceil(4).saturating_mul(4);
    let mut r = Reader::at(data, header.data_offset);
    for row in 0..canvas.height {
        let stored = r.take(row_len)?;
        let out = canvas.row(row);
        let stored = stored.chunks_exact(bytes_per_pixel);
        for (pixel, bytes) in out.iter_mut().zip(stored) {
            *pixel = match (header.layout, bytes) {
                (Layout::Rgb24 | Layout::Rgb32, &[b, g, r, ..]) => argb(0xFF, r, g, b),
                (Layout::Rgba32, &[b, g, r, a]) => argb(a, r, g, b),
                (Layout::Rgb16 | Layout::Bitfields16, &[lo, hi]) => header
                    .bitfields
                    .argb(u32::from(u16::from_le_bytes([lo, hi])), header.alpha),
                (_, &[b0, b1, b2, b3]) => header
                    .bitfields
                    .argb(u32::from_le_bytes([b0, b1, b2, b3]), header.alpha),
                _ => *pixel,
            };
        }
    }
    Ok(())
}

/// A run-length encoded picture (`read_rle_data`, lenient).
///
/// Rows change only at an end-of-line code or a delta that moves down; pixels
/// past the end of a row are dropped until one comes. Decoding ends at the
/// end-of-bitmap code, at a delta past the last row, or once every row has
/// been ended -- whatever follows is not read -- and running out of data
/// before any of those is an error. Pixels nothing wrote stay black.
#[allow(
    clippy::too_many_lines,
    reason = "one transcription of one function, kept whole to be read against it"
)]
fn decode_rle(
    data: &[u8],
    header: &Header,
    palette: &[[u8; 3]; PALETTE_ENTRIES],
    mut canvas: Canvas<'_>,
) -> ImageResult<()> {
    let colour = |index: u8| {
        let [r, g, b] = palette.get(usize::from(index)).copied().unwrap_or([0; 3]);
        argb(0xFF, r, g, b)
    };
    let width = canvas.width;
    let mut r = Reader::at(data, header.data_offset);
    let mut row = 0usize;
    while row < canvas.height {
        // `at` is where the next pixel goes in this row; `x` counts every
        // pixel the stream has placed on it, those dropped past its end
        // included, which a delta down then skips on the next row.
        let mut at = 0usize;
        let mut x = 0usize;
        loop {
            let control = r.byte()?;
            if control == 0 {
                match r.byte()? {
                    // End of line.
                    0 => {
                        row = row.saturating_add(1);
                        break;
                    }
                    // End of bitmap.
                    1 => return Ok(()),
                    // Delta: move right and down, over pixels left black.
                    2 => {
                        let dx = usize::from(r.byte()?);
                        let dy = usize::from(r.byte()?);
                        if dy > 0 {
                            row = row.saturating_add(dy);
                            if row >= canvas.height {
                                return Ok(());
                            }
                            at = x.min(width);
                        }
                        at = at.saturating_add(dx).min(width);
                        x = x.saturating_add(dx);
                    }
                    // Absolute: `count` pixels given one by one, padded to an
                    // even number of bytes.
                    count => {
                        let count = usize::from(count);
                        let out = canvas.row(row);
                        match header.layout {
                            Layout::Rle8 => {
                                let stored = r.take(count.saturating_add(count & 1))?;
                                for &index in stored.iter().take(count) {
                                    if let Some(pixel) = out.get_mut(at) {
                                        *pixel = colour(index);
                                        at = at.saturating_add(1);
                                    }
                                }
                            }
                            Layout::Rle4 => {
                                let bytes = count.div_ceil(2);
                                let stored = r.take(bytes.saturating_add(bytes & 1))?;
                                let nibbles = stored.iter().flat_map(|&b| [b >> 4, b & 0xF]);
                                for index in nibbles.take(count) {
                                    if let Some(pixel) = out.get_mut(at) {
                                        *pixel = colour(index);
                                        at = at.saturating_add(1);
                                    }
                                }
                            }
                            _ => {
                                for _ in 0..count {
                                    let bgr = r.take(3)?;
                                    if let (Some(pixel), &[b, g, r]) = (out.get_mut(at), bgr) {
                                        *pixel = argb(0xFF, r, g, b);
                                        at = at.saturating_add(1);
                                    }
                                }
                                if count % 2 == 1 {
                                    r.byte()?;
                                }
                            }
                        }
                        x = x.saturating_add(count);
                    }
                }
            } else {
                // Encoded: `control` pixels of one colour (two alternating,
                // for RLE4).
                let count = usize::from(control);
                let (first, second) = match header.layout {
                    Layout::Rle8 => {
                        let c = colour(r.byte()?);
                        (c, c)
                    }
                    Layout::Rle4 => {
                        let pair = r.byte()?;
                        (colour(pair >> 4), colour(pair & 0xF))
                    }
                    _ => {
                        let bgr = r.take(3)?;
                        let c = argb(0xFF, le_byte(bgr, 2), le_byte(bgr, 1), le_byte(bgr, 0));
                        (c, c)
                    }
                };
                let out = canvas.row(row);
                for i in 0..count {
                    let Some(pixel) = out.get_mut(at) else {
                        break;
                    };
                    *pixel = if i % 2 == 0 { first } else { second };
                    at = at.saturating_add(1);
                }
                x = x.saturating_add(count);
            }
        }
    }
    Ok(())
}

fn le_byte(bytes: &[u8], at: usize) -> u8 {
    bytes.get(at).copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        reason = "tests"
    )]

    use super::*;

    #[test]
    fn a_mask_is_one_run_of_bits_inside_its_pixel() {
        assert_eq!(
            Bitfield::from_mask(0x7C00, 16).unwrap(),
            Bitfield { shift: 10, len: 5 }
        );
        // Wider than 8 bits: the top 8 are kept.
        assert_eq!(
            Bitfield::from_mask(0x3FF0_0000, 32).unwrap(),
            Bitfield { shift: 22, len: 8 }
        );
        assert_eq!(Bitfield::from_mask(0, 16).unwrap(), Bitfield::default());
        assert!(Bitfield::from_mask(0x0F0F, 16).is_err(), "a gap");
        assert!(Bitfield::from_mask(0x1_F000, 16).is_err(), "past 16 bits");
        assert!(Bitfield::from_mask(0xFF00_0000, 32).is_ok());
    }

    #[test]
    fn narrow_channels_widen_as_chrome_widens_them() {
        // round(v * 255 / (2^n - 1)) for every width but 7, which replicates
        // its top bit -- the same numbers.
        for len in 1..=8u32 {
            let max = (1u32 << len) - 1;
            for v in 0..=max {
                let field = Bitfield { shift: 0, len };
                let want = ((f64::from(v) * 255.0 / f64::from(max)) + 0.5).floor() as u8;
                assert_eq!(field.read(v), want, "{v} of {len} bits");
            }
        }
    }

    #[test]
    fn a_pixel_is_read_through_its_masks() {
        let fields = Bitfields::from_masks([0xF800, 0x07E0, 0x001F, 0], 16).unwrap();
        assert_eq!(fields.argb(0xFFFF, false), 0xFFFF_FFFF);
        assert_eq!(fields.argb(0xF800, false), 0xFFFF_0000);
        assert_eq!(
            fields.argb(0x07E0, true),
            0xFF00_FF00,
            "no alpha mask: opaque"
        );
        let argb1555 = Bitfields::from_masks([0x7C00, 0x03E0, 0x001F, 0x8000], 16).unwrap();
        assert_eq!(argb1555.argb(0x7FFF, true), 0x00FF_FFFF);
        assert_eq!(argb1555.argb(0x7FFF, false), 0xFFFF_FFFF);
    }

    #[test]
    fn an_os2_header_reads_three_masks_whatever_its_compression_says() {
        // A 44-byte OS/2 2.x header with ALPHABITFIELDS: three masks, from
        // where the header ends, and no alpha.
        let mut file = b"BM".to_vec();
        file.extend_from_slice(&[0; 8]);
        file.extend_from_slice(&70u32.to_le_bytes());
        file.extend_from_slice(&44u32.to_le_bytes());
        file.extend_from_slice(&1i32.to_le_bytes());
        file.extend_from_slice(&1i32.to_le_bytes());
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&32u16.to_le_bytes());
        file.extend_from_slice(&BI_ALPHABITFIELDS.to_le_bytes());
        file.resize(14 + 44, 0);
        for mask in [0x00FF_0000u32, 0xFF00, 0xFF] {
            file.extend_from_slice(&mask.to_le_bytes());
        }
        // The pixel, whose bytes would be a broken fourth mask.
        file.extend_from_slice(&0x1234_5678u32.to_le_bytes());
        let header = read_header(&file).unwrap();
        assert!(!header.alpha);
        let image = decode(&file, Limits::default()).unwrap();
        assert_eq!(image.pixels, [0xFF34_5678]);
    }

    #[test]
    fn rows_are_padded_to_four_bytes() {
        assert_eq!(padded_row(1, 1), 4);
        assert_eq!(padded_row(1, 33), 8);
        assert_eq!(padded_row(4, 9), 8);
        assert_eq!(padded_row(8, 5), 8);
        assert_eq!(padded_row(24, 1), 4);
        assert_eq!(padded_row(24, 4), 12);
    }
}
