//! The header and the first directory, read as libtiff 4.7.1 reads them.
//!
//! A port of `TIFFClientOpen`'s header check and `TIFFReadDirectory` --
//! with the parts of `tif_dirread.c` those lean on: how an entry's value is
//! found and converted (`TIFFReadDirEntry*`), which tags must read cleanly
//! for the directory to be accepted at all and which are dropped with a
//! warning, and the repairs libtiff makes to directories real writers get
//! wrong (a missing or implausible `StripByteCounts`, colour channels that
//! should have been extra samples, a palette image with no palette).
//!
//! Only what decoding the picture needs is kept. libtiff reads every tag it
//! knows; this reads the ones that can change the pixels or refuse the file,
//! and for the rest keeps only the one fact that matters -- a tag of a type
//! libtiff cannot size makes its strip-size estimate fail.

use alloc::vec;
use alloc::vec::Vec;

use crate::{ImageError, ImageResult};

/// Tag numbers this reader acts on.
#[allow(dead_code)] // The table is the TIFF 6.0 names; not all are read yet.
pub(super) mod tag {
    pub const IMAGE_WIDTH: u16 = 256;
    pub const IMAGE_LENGTH: u16 = 257;
    pub const BITS_PER_SAMPLE: u16 = 258;
    pub const COMPRESSION: u16 = 259;
    pub const PHOTOMETRIC: u16 = 262;
    pub const FILL_ORDER: u16 = 266;
    pub const STRIP_OFFSETS: u16 = 273;
    pub const ORIENTATION: u16 = 274;
    pub const SAMPLES_PER_PIXEL: u16 = 277;
    pub const ROWS_PER_STRIP: u16 = 278;
    pub const STRIP_BYTE_COUNTS: u16 = 279;
    pub const MIN_SAMPLE_VALUE: u16 = 280;
    pub const MAX_SAMPLE_VALUE: u16 = 281;
    pub const PLANAR_CONFIG: u16 = 284;
    pub const GROUP3_OPTIONS: u16 = 292;
    pub const GROUP4_OPTIONS: u16 = 293;
    pub const TRANSFER_FUNCTION: u16 = 301;
    pub const PREDICTOR: u16 = 317;
    pub const WHITE_POINT: u16 = 318;
    pub const COLOR_MAP: u16 = 320;
    pub const TILE_WIDTH: u16 = 322;
    pub const TILE_LENGTH: u16 = 323;
    pub const TILE_OFFSETS: u16 = 324;
    pub const TILE_BYTE_COUNTS: u16 = 325;
    pub const BAD_FAX_LINES: u16 = 326;
    pub const CLEAN_FAX_DATA: u16 = 327;
    pub const CONSECUTIVE_BAD_FAX_LINES: u16 = 328;
    pub const INK_SET: u16 = 332;
    pub const NUMBER_OF_INKS: u16 = 334;
    pub const EXTRA_SAMPLES: u16 = 338;
    pub const SAMPLE_FORMAT: u16 = 339;
    pub const SMIN_SAMPLE_VALUE: u16 = 340;
    pub const SMAX_SAMPLE_VALUE: u16 = 341;
    pub const JPEG_TABLES: u16 = 347;
    pub const JPEG_PROC: u16 = 512;
    pub const JPEG_IF_OFFSET: u16 = 513;
    pub const JPEG_IF_BYTE_COUNT: u16 = 514;
    pub const JPEG_RESTART_INTERVAL: u16 = 515;
    pub const JPEG_Q_TABLES: u16 = 519;
    pub const JPEG_DC_TABLES: u16 = 520;
    pub const JPEG_AC_TABLES: u16 = 521;
    pub const YCBCR_COEFFICIENTS: u16 = 529;
    pub const YCBCR_SUBSAMPLING: u16 = 530;
    pub const YCBCR_POSITIONING: u16 = 531;
    pub const REFERENCE_BLACK_WHITE: u16 = 532;
    pub const MATTEING: u16 = 32995;
    pub const DATA_TYPE: u16 = 32996;
    pub const IMAGE_DEPTH: u16 = 32997;
    pub const TILE_DEPTH: u16 = 32998;
    pub const LERC_PARAMETERS: u16 = 50674;
}

/// Compression schemes, by the numbers the `Compression` tag uses.
#[allow(dead_code)]
pub(super) mod compression {
    pub const NONE: u16 = 1;
    pub const CCITT_RLE: u16 = 2;
    pub const CCITT_FAX3: u16 = 3;
    pub const CCITT_FAX4: u16 = 4;
    pub const LZW: u16 = 5;
    pub const OJPEG: u16 = 6;
    pub const JPEG: u16 = 7;
    pub const ADOBE_DEFLATE: u16 = 8;
    pub const NEXT: u16 = 32766;
    pub const CCITT_RLEW: u16 = 32771;
    pub const PACKBITS: u16 = 32773;
    pub const THUNDERSCAN: u16 = 32809;
    pub const PIXARLOG: u16 = 32909;
    pub const DEFLATE: u16 = 32946;
    pub const JBIG: u16 = 34661;
    pub const SGILOG: u16 = 34676;
    pub const SGILOG24: u16 = 34677;
    pub const LERC: u16 = 34887;
    pub const LZMA: u16 = 34925;
    pub const ZSTD: u16 = 50000;
    pub const WEBP: u16 = 50001;

    /// Whether the reference libtiff -- a distribution's build, with zlib,
    /// libdeflate and libjpeg -- has a decoder for `scheme`.
    ///
    /// This is the reference's list, not this crate's: which tags a
    /// directory keeps depends on it (`_TIFFCheckFieldIsValidForCodec`), and
    /// a file is accepted or refused by what the reference would do. A
    /// scheme on this list that this crate cannot decode is refused later,
    /// by name.
    pub fn configured(scheme: u16) -> bool {
        matches!(
            scheme,
            NONE | CCITT_RLE
                | CCITT_FAX3
                | CCITT_FAX4
                | LZW
                | OJPEG
                | JPEG
                | ADOBE_DEFLATE
                | NEXT
                | CCITT_RLEW
                | PACKBITS
                | THUNDERSCAN
                | 32908
                | PIXARLOG
                | DEFLATE
                | SGILOG
                | SGILOG24
        )
    }

    /// Whether libtiff knows `scheme` at all -- has an entry for it in its
    /// codec table, configured or not. An unknown scheme is not refused
    /// until a strip is decoded; a known, unconfigured one is refused up
    /// front (`TIFFRGBAImageOK`).
    pub fn known(scheme: u16) -> bool {
        configured(scheme) || matches!(scheme, JBIG | LERC | LZMA | ZSTD | WEBP)
    }

    /// Whether the codec reads bits in its own order, so `FillOrder` does not
    /// reverse the raw bytes first (`TIFF_NOBITREV`).
    pub fn reads_own_bit_order(scheme: u16) -> bool {
        matches!(
            scheme,
            CCITT_RLE | CCITT_RLEW | CCITT_FAX3 | CCITT_FAX4 | JPEG | OJPEG
        )
    }
}

/// Photometric interpretations.
#[allow(dead_code)]
pub(super) mod photometric {
    pub const MIN_IS_WHITE: u16 = 0;
    pub const MIN_IS_BLACK: u16 = 1;
    pub const RGB: u16 = 2;
    pub const PALETTE: u16 = 3;
    pub const MASK: u16 = 4;
    pub const SEPARATED: u16 = 5;
    pub const YCBCR: u16 = 6;
    pub const CIELAB: u16 = 8;
    pub const ICCLAB: u16 = 9;
    pub const ITULAB: u16 = 10;
    pub const CFA: u16 = 32803;
    pub const LOGL: u16 = 32844;
    pub const LOGLUV: u16 = 32845;
}

/// Field types.
mod kind {
    pub const BYTE: u16 = 1;
    pub const ASCII: u16 = 2;
    pub const SHORT: u16 = 3;
    pub const LONG: u16 = 4;
    pub const RATIONAL: u16 = 5;
    pub const SBYTE: u16 = 6;
    pub const UNDEFINED: u16 = 7;
    pub const SSHORT: u16 = 8;
    pub const SLONG: u16 = 9;
    pub const SRATIONAL: u16 = 10;
    pub const FLOAT: u16 = 11;
    pub const DOUBLE: u16 = 12;
    pub const IFD: u16 = 13;
    pub const LONG8: u16 = 16;
    pub const SLONG8: u16 = 17;
    pub const IFD8: u16 = 18;

    /// Bytes per value (`TIFFDataWidth`); 0 for a type libtiff does not know.
    pub fn width(kind: u16) -> u64 {
        match kind {
            BYTE | ASCII | SBYTE | UNDEFINED => 1,
            SHORT | SSHORT => 2,
            LONG | SLONG | FLOAT | IFD => 4,
            RATIONAL | SRATIONAL | DOUBLE | LONG8 | SLONG8 | IFD8 => 8,
            _ => 0,
        }
    }

    /// The eight integer types every integer reader accepts.
    pub fn integer(kind: u16) -> bool {
        matches!(
            kind,
            BYTE | SBYTE | SHORT | SSHORT | LONG | SLONG | LONG8 | SLONG8
        )
    }

    /// The integer types plus the four that read as numbers with fractions.
    pub fn numeric(kind: u16) -> bool {
        integer(kind) || matches!(kind, RATIONAL | SRATIONAL | FLOAT | DOUBLE)
    }
}

/// Why a directory entry's value could not be read (`TIFFReadDirEntryErr`).
///
/// Which one it was never changes what happens -- a tag libtiff insists on
/// fails the directory, any other is dropped -- but keeping them apart keeps
/// this a line-for-line reading of the C.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadErr {
    Count,
    Type,
    Io,
    Range,
    Psdif,
    Sizesan,
    Alloc,
}

/// One twelve-byte (or, in a BigTIFF, twenty-byte) directory entry.
#[derive(Clone, Copy, Debug)]
struct Entry {
    tag: u16,
    kind: u16,
    count: u64,
    /// The value field exactly as it lies in the file: four bytes, or eight
    /// in a BigTIFF. A value that fits is left-justified in it; one that does
    /// not is found at the offset it holds.
    value: [u8; 8],
    /// A later duplicate of an earlier tag, or a tag already consumed.
    ignore: bool,
}

/// A TIFF file's bytes and how to read numbers from them.
#[derive(Clone, Copy)]
pub(super) struct File<'a> {
    pub(super) data: &'a [u8],
    pub(super) big_endian: bool,
    pub(super) big_tiff: bool,
}

impl<'a> File<'a> {
    fn size(&self) -> u64 {
        self.data.len() as u64
    }

    /// `len` bytes at `at`, if all of them are in the file.
    fn bytes(&self, at: u64, len: u64) -> Option<&'a [u8]> {
        let start = usize::try_from(at).ok()?;
        let len = usize::try_from(len).ok()?;
        self.data.get(start..start.checked_add(len)?)
    }

    pub(super) fn u16(&self, raw: &[u8]) -> u16 {
        let b = [
            raw.first().copied().unwrap_or(0),
            raw.get(1).copied().unwrap_or(0),
        ];
        if self.big_endian {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        }
    }

    pub(super) fn u32(&self, raw: &[u8]) -> u32 {
        let mut b = [0u8; 4];
        for (slot, byte) in b.iter_mut().zip(raw) {
            *slot = *byte;
        }
        if self.big_endian {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        }
    }

    pub(super) fn u64(&self, raw: &[u8]) -> u64 {
        let mut b = [0u8; 8];
        for (slot, byte) in b.iter_mut().zip(raw) {
            *slot = *byte;
        }
        if self.big_endian {
            u64::from_be_bytes(b)
        } else {
            u64::from_le_bytes(b)
        }
    }

    /// Where an entry's out-of-line value is.
    fn offset(&self, entry: &Entry) -> u64 {
        if self.big_tiff {
            self.u64(&entry.value)
        } else {
            u64::from(self.u32(&entry.value))
        }
    }

    /// The bytes of an entry's first `count` values: in the entry if they
    /// fit, else at its offset (`TIFFReadDirEntryArrayWithLimit`, for a
    /// memory-mapped file). `None` for no values, or a type of no width.
    fn raw(&self, entry: &Entry, max_count: u64) -> Result<Option<(Raw<'a>, u32)>, ReadErr> {
        let width = kind::width(entry.kind);
        let target = entry.count.min(max_count);
        if target == 0 || width == 0 {
            return Ok(None);
        }
        // Whether the value, as written, fits in the entry -- judged on the
        // count in the file, not the one asked for.
        let original_clamped = entry.count.min(10).saturating_mul(width);
        const MAX_SIZE_TAG_DATA: u64 = 2_147_483_647;
        if MAX_SIZE_TAG_DATA.checked_div(width).unwrap_or(0) < target
            || MAX_SIZE_TAG_DATA / 8 < target
        {
            return Err(ReadErr::Sizesan);
        }
        let count = u32::try_from(target).map_err(|_| ReadErr::Sizesan)?;
        let size = target.saturating_mul(width);
        if size > 100 * 1024 * 1024 && size > self.size() {
            return Err(ReadErr::Alloc);
        }
        if size > self.size() {
            return Err(ReadErr::Io);
        }
        let inline = if self.big_tiff { 8 } else { 4 };
        if original_clamped <= inline && size <= inline {
            let len = usize::try_from(size).map_err(|_| ReadErr::Io)?;
            return Ok(Some((Raw::Inline(entry.value, len), count)));
        }
        let at = self.offset(entry);
        let bytes = self.bytes(at, size).ok_or(ReadErr::Io)?;
        Ok(Some((Raw::File(bytes), count)))
    }

    /// The `i`-th integer of `raw`, read as `kind`.
    fn integer_at(&self, raw: &[u8], kind: u16, i: usize) -> i128 {
        let w = usize::try_from(kind::width(kind)).unwrap_or(0);
        let at = i.saturating_mul(w);
        let bytes = raw.get(at..at.saturating_add(w)).unwrap_or(&[]);
        match kind {
            kind::BYTE | kind::UNDEFINED | kind::ASCII => {
                i128::from(bytes.first().copied().unwrap_or(0))
            }
            kind::SBYTE => i128::from(bytes.first().copied().unwrap_or(0).cast_signed()),
            kind::SHORT => i128::from(self.u16(bytes)),
            kind::SSHORT => i128::from(self.u16(bytes).cast_signed()),
            kind::LONG | kind::IFD => i128::from(self.u32(bytes)),
            kind::SLONG => i128::from(self.u32(bytes).cast_signed()),
            kind::LONG8 | kind::IFD8 => i128::from(self.u64(bytes)),
            kind::SLONG8 => i128::from(self.u64(bytes).cast_signed()),
            _ => 0,
        }
    }

    /// One integer, for a field of one value (`TIFFReadDirEntryShort` and
    /// its siblings): the count must be 1, the type an integer type (or,
    /// for a byte, `UNDEFINED`), and the value in `range`.
    fn scalar(
        &self,
        entry: &Entry,
        range: (i128, i128),
        undefined_ok: bool,
    ) -> Result<i128, ReadErr> {
        if entry.count != 1 {
            return Err(ReadErr::Count);
        }
        if !(kind::integer(entry.kind) || (undefined_ok && entry.kind == kind::UNDEFINED)) {
            return Err(ReadErr::Type);
        }
        let value = if matches!(entry.kind, kind::LONG8 | kind::SLONG8) && !self.big_tiff {
            // Eight bytes do not fit in a classic entry: they are at its
            // offset, and must all be in the file.
            let at = self.offset(entry);
            let bytes = self.bytes(at, 8).ok_or(ReadErr::Io)?;
            self.integer_at(bytes, entry.kind, 0)
        } else {
            self.integer_at(&entry.value, entry.kind, 0)
        };
        if value < range.0 || value > range.1 {
            return Err(ReadErr::Range);
        }
        Ok(value)
    }

    fn short(&self, entry: &Entry) -> Result<u16, ReadErr> {
        let v = self.scalar(entry, (0, i128::from(u16::MAX)), false)?;
        u16::try_from(v).map_err(|_| ReadErr::Range)
    }

    fn long(&self, entry: &Entry) -> Result<u32, ReadErr> {
        let v = self.scalar(entry, (0, i128::from(u32::MAX)), false)?;
        u32::try_from(v).map_err(|_| ReadErr::Range)
    }

    /// Integers, each of which must be in `range`
    /// (`TIFFReadDirEntryShortArray` and its siblings).
    fn integers(
        &self,
        entry: &Entry,
        range: (i128, i128),
        max_count: u64,
    ) -> Result<Vec<i128>, ReadErr> {
        if !kind::integer(entry.kind) {
            return Err(ReadErr::Type);
        }
        let Some((raw, count)) = self.raw(entry, max_count)? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let v = self.integer_at(raw.bytes(), entry.kind, i);
            if v < range.0 || v > range.1 {
                return Err(ReadErr::Range);
            }
            out.push(v);
        }
        Ok(out)
    }

    /// Bytes (`TIFFReadDirEntryByteArray`): ASCII and UNDEFINED as they
    /// are, any integer type whose values are 0 to 255.
    fn byte_array(&self, entry: &Entry) -> Result<Vec<u8>, ReadErr> {
        if matches!(entry.kind, kind::ASCII | kind::UNDEFINED | kind::BYTE) {
            return Ok(match self.raw(entry, u64::MAX)? {
                Some((raw, _)) => raw.bytes().to_vec(),
                None => Vec::new(),
            });
        }
        Ok(self
            .integers(entry, (0, 255), u64::MAX)?
            .into_iter()
            .map(|v| u8::try_from(v).unwrap_or(0))
            .collect())
    }

    fn shorts(&self, entry: &Entry) -> Result<Vec<u16>, ReadErr> {
        Ok(self
            .integers(entry, (0, i128::from(u16::MAX)), u64::MAX)?
            .into_iter()
            .map(|v| u16::try_from(v).unwrap_or(0))
            .collect())
    }

    /// Numbers of a fixed count (`TIFFReadDirEntryFloatArray`, for a
    /// `TIFF_SETGET_C0_FLOAT` field): the wrong count, or anything that will
    /// not read, drops the field -- `None`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)] // As the C converts.
    fn floats(&self, entry: &Entry, count: u64) -> Option<Vec<f32>> {
        if entry.count != count || !kind::numeric(entry.kind) {
            return None;
        }
        let (raw, n) = self.raw(entry, u64::MAX).ok()??;
        let bytes = raw.bytes();
        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n as usize {
            let at = i.saturating_mul(usize::try_from(kind::width(entry.kind)).unwrap_or(0));
            let here = bytes.get(at..).unwrap_or(&[]);
            let v = match entry.kind {
                // Numerator over denominator, each converted to single
                // precision first; the denominator unsigned even in a
                // signed rational; zero over zero is zero.
                kind::RATIONAL | kind::SRATIONAL => {
                    let den = self.u32(here.get(4..).unwrap_or(&[]));
                    if den == 0 {
                        0.0
                    } else if entry.kind == kind::RATIONAL {
                        self.u32(here) as f32 / den as f32
                    } else {
                        self.u32(here).cast_signed() as f32 / den as f32
                    }
                }
                kind::FLOAT => f32::from_bits(self.u32(here)),
                kind::DOUBLE => f64::from_bits(self.u64(here)) as f32,
                // An integer straight to single precision, rounded once.
                _ => self.integer_at(bytes, entry.kind, i) as f32,
            };
            out.push(v);
        }
        Some(out)
    }

    /// A per-sample field of which libtiff keeps one value: at least one
    /// value per sample, and the first `samples` all equal
    /// (`TIFFReadDirEntryPersampleShort`).
    fn per_sample_short(&self, entry: &Entry, samples: u16) -> Result<u16, ReadErr> {
        if entry.count < u64::from(samples) {
            return Err(ReadErr::Count);
        }
        let values = self.shorts(entry)?;
        let Some(&first) = values.first() else {
            // No values at all: the C returns without setting one.
            return Err(ReadErr::Count);
        };
        if values
            .iter()
            .take(usize::from(samples))
            .any(|&v| v != first)
        {
            return Err(ReadErr::Psdif);
        }
        Ok(first)
    }
}

/// Where an entry's value bytes are.
enum Raw<'a> {
    Inline([u8; 8], usize),
    File(&'a [u8]),
}

impl Raw<'_> {
    fn bytes(&self) -> &[u8] {
        match self {
            Raw::Inline(b, len) => b.get(..*len).unwrap_or(&[]),
            Raw::File(b) => b,
        }
    }
}

/// Whether `bytes` begins like a TIFF: "II" or "MM" and 42 (classic) or 43
/// (BigTIFF), or Microsoft Document Imaging's "EP", which libtiff reads as
/// a little-endian TIFF.
#[must_use]
pub(super) fn is_tiff(bytes: &[u8]) -> bool {
    matches!(
        bytes.get(..4),
        Some([b'I', b'I', 42 | 43, 0] | [b'M', b'M', 0, 42 | 43] | [b'E', b'P', 42 | 43, 0])
    )
}

/// The header: byte order, classic or BigTIFF, and the first directory's
/// offset.
pub(super) fn header(data: &[u8]) -> ImageResult<(File<'_>, u64)> {
    let head = data.get(..8).ok_or(ImageError::Truncated)?;
    let big_endian = match head.get(..2) {
        Some(b"II" | b"EP") => false,
        Some(b"MM") => true,
        _ => return Err(ImageError::UnknownFormat),
    };
    let mut file = File {
        data,
        big_endian,
        big_tiff: false,
    };
    match file.u16(head.get(2..).unwrap_or(&[])) {
        42 => Ok((file, u64::from(file.u32(head.get(4..).unwrap_or(&[]))))),
        43 => {
            let big = data.get(..16).ok_or(ImageError::Truncated)?;
            if file.u16(big.get(4..).unwrap_or(&[])) != 8
                || file.u16(big.get(6..).unwrap_or(&[])) != 0
            {
                return Err(ImageError::Malformed("BigTIFF header"));
            }
            file.big_tiff = true;
            Ok((file, file.u64(big.get(8..).unwrap_or(&[]))))
        }
        _ => Err(ImageError::Malformed("TIFF version")),
    }
}

/// `ExtraSamples` values.
pub(super) mod extra {
    pub const UNSPECIFIED: u16 = 0;
    pub const ASSOCIATED_ALPHA: u16 = 1;
    pub const UNASSOCIATED_ALPHA: u16 = 2;
}

/// A directory as `TIFFReadDirectory` leaves it: the fields decoding reads.
#[derive(Clone, Debug)]
pub(super) struct Directory {
    pub(super) width: u32,
    pub(super) length: u32,
    pub(super) depth: u32,
    pub(super) bits_per_sample: u16,
    pub(super) samples_per_pixel: u16,
    pub(super) compression: u16,
    /// `None` when the file has no `PhotometricInterpretation`.
    pub(super) photometric: Option<u16>,
    pub(super) fill_order: u16,
    pub(super) orientation: u16,
    pub(super) planar_config: u16,
    pub(super) rows_per_strip: u32,
    pub(super) tiled: bool,
    pub(super) tile_width: u32,
    pub(super) tile_length: u32,
    pub(super) tile_depth: u32,
    pub(super) sample_format: u16,
    /// One `ExtraSamples` value per extra sample.
    pub(super) sample_info: Vec<u16>,
    /// Red, green and blue, `1 << bits_per_sample` entries each.
    pub(super) color_map: Option<[Vec<u16>; 3]>,
    pub(super) ycbcr_subsampling: [u16; 2],
    /// Whether `YCbCrSubsampling` was read from the file, which the JPEG
    /// codec notes (`ycbcrsampling_fetched`): if not, it takes the
    /// subsampling from the first strip's JPEG header instead.
    pub(super) ycbcr_subsampling_set: bool,
    /// `JPEGTables`: a JPEG datastream of tables alone, which every strip's
    /// abbreviated datastream is decoded with.
    pub(super) jpeg_tables: Option<Vec<u8>>,
    /// `YCbCrCoefficients`, when present and of three values.
    pub(super) ycbcr_coefficients: Option<[f32; 3]>,
    /// `ReferenceBlackWhite`, when present and of six values.
    pub(super) reference_black_white: Option<[f32; 6]>,
    /// `WhitePoint`, when present and of two values.
    pub(super) white_point: Option<[f32; 2]>,
    pub(super) ink_set: u16,
    pub(super) predictor: u16,
    /// `Group3Options`, of a Group 3 fax image; bit 0 says rows may be
    /// coded against the row before.
    pub(super) group3_options: u32,
    /// Whether a codec hands back this `YCbCr` image's chroma already
    /// upsampled (`TIFF_UPSAMPLED`, which the JPEG codec sets when asked for
    /// RGB), so sizes count every pixel's three samples.
    pub(super) upsampled: bool,
    pub(super) strips: u32,
    pub(super) strips_per_image: u32,
    pub(super) strip_offsets: Vec<u64>,
    pub(super) strip_byte_counts: Vec<u64>,
    /// The old-style JPEG codec's tags, and what it keeps of
    /// `YCbCrSubsampling` (`OJPEGVSetField`).
    pub(super) ojpeg: OjpegTags,
}

/// The tags of old-style JPEG compression, as `OJPEGVSetField` keeps them.
#[derive(Debug, Clone)]
pub(super) struct OjpegTags {
    /// `JPEGInterchangeFormat` and its length.
    pub(super) jif: u64,
    pub(super) jif_len: u64,
    /// `JPEGQTables`, `JPEGDCTables`, `JPEGACTables`: offsets, at most three
    /// of each (more and the tag is dropped).
    pub(super) q_tables: Vec<u64>,
    pub(super) dc_tables: Vec<u64>,
    pub(super) ac_tables: Vec<u64>,
    /// `JPEGRestartInterval`.
    pub(super) restart_interval: u16,
    /// `YCbCrSubsampling` as the codec keeps it: in a byte each.
    pub(super) hor: u8,
    pub(super) ver: u8,
}

impl Directory {
    /// What `TIFFDefaultDirectory` starts from.
    fn new() -> Self {
        Self {
            width: 0,
            length: 0,
            depth: 1,
            bits_per_sample: 1,
            samples_per_pixel: 1,
            compression: compression::NONE,
            photometric: None,
            fill_order: 1,
            orientation: 1,
            planar_config: 1,
            rows_per_strip: u32::MAX,
            tiled: false,
            tile_width: 0,
            tile_length: 0,
            tile_depth: 1,
            sample_format: 1,
            sample_info: Vec::new(),
            color_map: None,
            ycbcr_subsampling: [2, 2],
            ycbcr_subsampling_set: false,
            jpeg_tables: None,
            ycbcr_coefficients: None,
            reference_black_white: None,
            white_point: None,
            ink_set: 1,
            predictor: 1,
            group3_options: 0,
            upsampled: false,
            strips: 0,
            strips_per_image: 0,
            strip_offsets: Vec::new(),
            strip_byte_counts: Vec::new(),
            // `TIFFInitOJPEG` sets 2x2 itself, as the tag's own default.
            ojpeg: OjpegTags {
                jif: 0,
                jif_len: 0,
                q_tables: Vec::new(),
                dc_tables: Vec::new(),
                ac_tables: Vec::new(),
                restart_interval: 0,
                hor: 2,
                ver: 2,
            },
        }
    }

    pub(super) fn extra_samples(&self) -> u16 {
        u16::try_from(self.sample_info.len()).unwrap_or(u16::MAX)
    }

    /// The photometric interpretation libtiff's size arithmetic sees: the
    /// field, or 0 -- `MINISWHITE` -- when there is none, which is what the
    /// zeroed directory holds.
    pub(super) fn photometric_or_zero(&self) -> u16 {
        self.photometric.unwrap_or(photometric::MIN_IS_WHITE)
    }

    /// Whether this is packed `YCbCr` whose chroma is stored subsampled, as
    /// the size arithmetic treats it (`isUpSampled` is false: nothing here
    /// asks libjpeg to upsample).
    fn subsampled_ycbcr(&self) -> bool {
        self.planar_config == 1
            && self.photometric_or_zero() == photometric::YCBCR
            && !self.upsampled
    }

    /// Bytes in one row of the image (`TIFFScanlineSize64`); `None` for an
    /// error.
    pub(super) fn scanline_size(&self) -> Option<u64> {
        let size = if self.planar_config == 1 {
            if self.subsampled_ycbcr() && self.samples_per_pixel == 3 {
                let [h, v] = self.valid_subsampling()?;
                let block_samples = u64::from(h).checked_mul(u64::from(v))?.checked_add(2)?;
                let blocks = u64::from(howmany32(self.width, u32::from(h)));
                let row = blocks
                    .checked_mul(block_samples)?
                    .checked_mul(u64::from(self.bits_per_sample))?
                    .div_ceil(8);
                row.checked_div(u64::from(v))?
            } else {
                let samples =
                    u64::from(self.width).checked_mul(u64::from(self.samples_per_pixel))?;
                samples
                    .checked_mul(u64::from(self.bits_per_sample))?
                    .div_ceil(8)
            }
        } else {
            u64::from(self.width)
                .checked_mul(u64::from(self.bits_per_sample))?
                .div_ceil(8)
        };
        (size != 0).then_some(size)
    }

    /// The `YCbCrSubsampling` pair, if it is one libtiff's size arithmetic
    /// accepts: each 1, 2 or 4.
    fn valid_subsampling(&self) -> Option<[u16; 2]> {
        let [h, v] = self.ycbcr_subsampling;
        (matches!(h, 1 | 2 | 4) && matches!(v, 1 | 2 | 4)).then_some([h, v])
    }

    /// Bytes in `rows` rows of a strip (`TIFFVStripSize64`); `None` for an
    /// error or zero.
    pub(super) fn strip_size_rows(&self, rows: u32) -> Option<u64> {
        let rows = if rows == u32::MAX { self.length } else { rows };
        let size = if self.subsampled_ycbcr() {
            if self.samples_per_pixel != 3 {
                return None;
            }
            let [h, v] = self.valid_subsampling()?;
            let block_samples = u64::from(h).checked_mul(u64::from(v))?.checked_add(2)?;
            let blocks_h = u64::from(howmany32(self.width, u32::from(h)));
            let blocks_v = u64::from(howmany32(rows, u32::from(v)));
            let row = blocks_h
                .checked_mul(block_samples)?
                .checked_mul(u64::from(self.bits_per_sample))?
                .div_ceil(8);
            row.checked_mul(blocks_v)?
        } else {
            u64::from(rows).checked_mul(self.scanline_size()?)?
        };
        (size != 0).then_some(size)
    }

    /// Bytes in a full strip (`TIFFStripSize64`).
    pub(super) fn strip_size(&self) -> Option<u64> {
        self.strip_size_rows(self.rows_per_strip.min(self.length))
    }

    /// Bytes in one row of a tile (`TIFFTileRowSize64`).
    pub(super) fn tile_row_size(&self) -> Option<u64> {
        if self.tile_length == 0 || self.tile_width == 0 {
            return None;
        }
        let mut bits = u64::from(self.bits_per_sample).checked_mul(u64::from(self.tile_width))?;
        if self.planar_config == 1 {
            bits = bits.checked_mul(u64::from(self.samples_per_pixel))?;
        }
        let size = bits.div_ceil(8);
        (size != 0).then_some(size)
    }

    /// Bytes in `rows` rows of a tile (`TIFFVTileSize64`).
    pub(super) fn tile_size_rows(&self, rows: u32) -> Option<u64> {
        if self.tile_length == 0 || self.tile_width == 0 || self.tile_depth == 0 {
            return None;
        }
        let size = if self.subsampled_ycbcr() && self.samples_per_pixel == 3 {
            let [h, v] = self.valid_subsampling()?;
            let block_samples = u64::from(h).checked_mul(u64::from(v))?.checked_add(2)?;
            let blocks_h = u64::from(howmany32(self.tile_width, u32::from(h)));
            let blocks_v = u64::from(howmany32(rows, u32::from(v)));
            let row = blocks_h
                .checked_mul(block_samples)?
                .checked_mul(u64::from(self.bits_per_sample))?
                .div_ceil(8);
            row.checked_mul(blocks_v)?
        } else {
            u64::from(rows).checked_mul(self.tile_row_size()?)?
        };
        (size != 0).then_some(size)
    }

    /// Bytes in a tile (`TIFFTileSize64`).
    pub(super) fn tile_size(&self) -> Option<u64> {
        self.tile_size_rows(self.tile_length)
    }

    /// How many strips an image of this shape has (`TIFFNumberOfStrips`).
    fn number_of_strips(&self) -> u32 {
        let n = if self.rows_per_strip == u32::MAX {
            1
        } else {
            howmany32(self.length, self.rows_per_strip)
        };
        if self.planar_config == 2 {
            n.checked_mul(u32::from(self.samples_per_pixel))
                .unwrap_or(0)
        } else {
            n
        }
    }

    /// How many tiles (`TIFFNumberOfTiles`).
    fn number_of_tiles(&self) -> u32 {
        let dx = if self.tile_width == u32::MAX {
            self.width
        } else {
            self.tile_width
        };
        let dy = if self.tile_length == u32::MAX {
            self.length
        } else {
            self.tile_length
        };
        let dz = if self.tile_depth == u32::MAX {
            self.depth
        } else {
            self.tile_depth
        };
        let n = if dx == 0 || dy == 0 || dz == 0 {
            0
        } else {
            let across = u64::from(howmany32(self.width, dx));
            let down = u64::from(howmany32(self.length, dy));
            let deep = u64::from(howmany32(self.depth, dz));
            across
                .checked_mul(down)
                .and_then(|n| n.checked_mul(deep))
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0)
        };
        if self.planar_config == 2 {
            n.checked_mul(u32::from(self.samples_per_pixel))
                .unwrap_or(0)
        } else {
            n
        }
    }
}

/// `TIFFhowmany_32`: `x / y` rounded up -- and 0 when `x` is within `y` of
/// the top of the range, where the C refuses to round.
pub(super) fn howmany32(x: u32, y: u32) -> u32 {
    if y == 0 {
        return 0;
    }
    let y1 = y.wrapping_sub(1);
    if x < u32::MAX.wrapping_sub(y1) {
        // No overflow: x + (y - 1) < u32::MAX.
        x.wrapping_add(y1).checked_div(y).unwrap_or(0)
    } else {
        0
    }
}

/// Read the first directory, as `TIFFReadDirectory` does.
///
/// # Errors
///
/// [`ImageError::Malformed`] or [`ImageError::Truncated`] for a directory
/// libtiff would not open.
#[allow(clippy::too_many_lines)] // One function, as the C is: its order is the point.
pub(super) fn read(file: &File<'_>, offset: u64) -> ImageResult<Directory> {
    let bad = |what: &'static str| ImageError::Malformed(what);
    let mut entries = fetch(file, offset)?;
    // Duplicates of a tag are ignored: the first wins.
    for i in 0..entries.len() {
        let Some(tag) = entries.get(i).map(|e| e.tag) else {
            break;
        };
        for later in entries.iter_mut().skip(i.saturating_add(1)) {
            if later.tag == tag {
                later.ignore = true;
            }
        }
    }
    let mut dir = Directory::new();
    let find = |entries: &[Entry], tag: u16| entries.iter().position(|e| e.tag == tag && !e.ignore);

    // SamplesPerPixel first, since Compression may be written once per sample.
    let mut spp_set = false;
    if let Some(i) = find(&entries, tag::SAMPLES_PER_PIXEL) {
        let entry = entries.get(i).copied().ok_or(bad("TIFF directory"))?;
        let v = file
            .short(&entry)
            .map_err(|_| bad("TIFF SamplesPerPixel"))?;
        if v == 0 {
            return Err(bad("TIFF SamplesPerPixel"));
        }
        dir.samples_per_pixel = v;
        spp_set = true;
        if let Some(e) = entries.get_mut(i) {
            e.ignore = true;
        }
    }
    if let Some(i) = find(&entries, tag::COMPRESSION) {
        let entry = entries.get(i).copied().ok_or(bad("TIFF directory"))?;
        let v = match file.short(&entry) {
            Err(ReadErr::Count) => file.per_sample_short(&entry, dir.samples_per_pixel),
            other => other,
        }
        .map_err(|_| bad("TIFF Compression"))?;
        dir.compression = v;
        if let Some(e) = entries.get_mut(i) {
            e.ignore = true;
        }
    }

    // First pass: the fields that size everything else.
    let mut dimensions_set = false;
    let mut tile_dimensions_set = false;
    let mut have_offsets = false;
    let mut have_byte_counts = false;
    let mut rows_per_strip_set = false;
    for entry in &mut entries {
        if entry.ignore {
            continue;
        }
        match entry.tag {
            tag::STRIP_OFFSETS | tag::TILE_OFFSETS => have_offsets = true,
            tag::STRIP_BYTE_COUNTS | tag::TILE_BYTE_COUNTS => have_byte_counts = true,
            tag::IMAGE_WIDTH | tag::IMAGE_LENGTH => {
                let v = file.long(entry).map_err(|_| bad("TIFF image size"))?;
                if entry.tag == tag::IMAGE_WIDTH {
                    dir.width = v;
                } else {
                    dir.length = v;
                }
                dimensions_set = true;
                entry.ignore = true;
            }
            tag::IMAGE_DEPTH => {
                dir.depth = file.long(entry).map_err(|_| bad("TIFF ImageDepth"))?;
                entry.ignore = true;
            }
            tag::TILE_WIDTH | tag::TILE_LENGTH => {
                let v = file.long(entry).map_err(|_| bad("TIFF tile size"))?;
                if entry.tag == tag::TILE_WIDTH {
                    dir.tile_width = v;
                } else {
                    dir.tile_length = v;
                }
                dir.tiled = true;
                tile_dimensions_set = true;
                entry.ignore = true;
            }
            tag::TILE_DEPTH => {
                let v = file.long(entry).map_err(|_| bad("TIFF TileDepth"))?;
                if v == 0 {
                    return Err(bad("TIFF TileDepth"));
                }
                dir.tile_depth = v;
                entry.ignore = true;
            }
            tag::PLANAR_CONFIG => {
                let v = file
                    .short(entry)
                    .map_err(|_| bad("TIFF PlanarConfiguration"))?;
                if v != 1 && v != 2 {
                    return Err(bad("TIFF PlanarConfiguration"));
                }
                dir.planar_config = v;
                entry.ignore = true;
            }
            tag::ROWS_PER_STRIP => {
                let v = file.long(entry).map_err(|_| bad("TIFF RowsPerStrip"))?;
                if v == 0 {
                    return Err(bad("TIFF RowsPerStrip"));
                }
                dir.rows_per_strip = v;
                // Until a tile tag has been read, rows per strip is also the
                // tile height, and the tile the width of the image so far: a
                // TileWidth later with no TileLength makes tiles this tall.
                if !tile_dimensions_set {
                    dir.tile_length = v;
                    dir.tile_width = dir.width;
                }
                rows_per_strip_set = true;
                entry.ignore = true;
            }
            tag::EXTRA_SAMPLES => {
                if entry.count > 0xFFFF {
                    return Err(bad("TIFF ExtraSamples"));
                }
                let mut values = file.shorts(entry).map_err(|_| bad("TIFF ExtraSamples"))?;
                if values.len() > usize::from(dir.samples_per_pixel) {
                    return Err(bad("TIFF ExtraSamples"));
                }
                for v in &mut values {
                    if *v > extra::UNASSOCIATED_ALPHA {
                        // Corel Draw writes 999 for unassociated alpha.
                        if *v == 999 {
                            *v = extra::UNASSOCIATED_ALPHA;
                        } else {
                            return Err(bad("TIFF ExtraSamples"));
                        }
                    }
                }
                dir.sample_info = values;
                entry.ignore = true;
            }
            t if !field_valid_for_codec(t, dir.compression) => entry.ignore = true,
            _ => {}
        }
    }
    // Old-style JPEG hack: separate planes, but one strip offset and one
    // byte count, is better read as planes together.
    if dir.compression == compression::OJPEG && dir.planar_config == 2 {
        let one = |t: u16| {
            entries
                .iter()
                .find(|e| e.tag == t)
                .is_some_and(|e| e.count == 1)
        };
        if one(tag::STRIP_OFFSETS) && one(tag::STRIP_BYTE_COUNTS) {
            dir.planar_config = 1;
        }
    }
    if !dimensions_set {
        return Err(bad("TIFF directory without ImageLength"));
    }

    // Second pass: everything else.
    let mut bits_read = false;
    let mut offsets_entry: Option<Entry> = None;
    let mut counts_entry: Option<Entry> = None;
    for entry in &entries {
        if entry.ignore {
            continue;
        }
        match entry.tag {
            tag::MIN_SAMPLE_VALUE
            | tag::MAX_SAMPLE_VALUE
            | tag::BITS_PER_SAMPLE
            | tag::DATA_TYPE
            | tag::SAMPLE_FORMAT => {
                let v = match file.short(entry) {
                    Err(ReadErr::Count) => file.per_sample_short(entry, dir.samples_per_pixel),
                    other => other,
                }
                .map_err(|_| bad("TIFF per-sample field"))?;
                match entry.tag {
                    tag::BITS_PER_SAMPLE => {
                        dir.bits_per_sample = v;
                        bits_read = true;
                    }
                    tag::DATA_TYPE => {
                        dir.sample_format = match v {
                            0 => 4,
                            1 => 2,
                            2 => 1,
                            3 => 3,
                            _ => return Err(bad("TIFF DataType")),
                        };
                    }
                    tag::SAMPLE_FORMAT => {
                        if !(1..=6).contains(&v) {
                            return Err(bad("TIFF SampleFormat"));
                        }
                        dir.sample_format = v;
                    }
                    _ => {}
                }
            }
            tag::SMIN_SAMPLE_VALUE | tag::SMAX_SAMPLE_VALUE => {
                if entry.count != u64::from(dir.samples_per_pixel) || !kind::numeric(entry.kind) {
                    return Err(bad("TIFF per-sample field"));
                }
                file.raw(entry, u64::MAX)
                    .map_err(|_| bad("TIFF per-sample field"))?;
            }
            tag::STRIP_OFFSETS | tag::TILE_OFFSETS => offsets_entry = Some(*entry),
            tag::STRIP_BYTE_COUNTS | tag::TILE_BYTE_COUNTS => counts_entry = Some(*entry),
            tag::COLOR_MAP => {
                // Only once BitsPerSample has said how long it must be, and
                // never for more than 24 bits: a malformed one is dropped.
                if !bits_read || dir.bits_per_sample > 24 {
                    continue;
                }
                let per = 1u64 << dir.bits_per_sample;
                if entry.count != per.saturating_mul(3) {
                    continue;
                }
                if let Ok(values) = file.shorts(entry) {
                    let n = usize::try_from(per).unwrap_or(0);
                    let red = values.get(..n).map(<[u16]>::to_vec);
                    let green = values.get(n..n.saturating_mul(2)).map(<[u16]>::to_vec);
                    let blue = values.get(n.saturating_mul(2)..).map(<[u16]>::to_vec);
                    if let (Some(r), Some(g), Some(b)) = (red, green, blue) {
                        dir.color_map = Some([r, g, b]);
                    }
                }
            }
            _ => second_pass_field(file, entry, &mut dir),
        }
    }

    // Old-style JPEG hacks: a missing or RGB photometric is `YCbCr`, missing
    // bits are 8, and missing samples are 3 for `YCbCr` and 1 for grey.
    if dir.compression == compression::OJPEG {
        match dir.photometric {
            None | Some(photometric::RGB) => dir.photometric = Some(photometric::YCBCR),
            Some(_) => {}
        }
        if !bits_read {
            dir.bits_per_sample = 8;
        }
        if !spp_set {
            match dir.photometric {
                Some(photometric::YCBCR) => dir.samples_per_pixel = 3,
                Some(photometric::MIN_IS_WHITE | photometric::MIN_IS_BLACK) => {
                    dir.samples_per_pixel = 1;
                }
                _ => {}
            }
        }
    }

    // Strips or tiles.
    if tile_dimensions_set {
        dir.strips = dir.number_of_tiles();
        dir.tiled = true;
    } else {
        dir.strips = dir.number_of_strips();
        dir.tile_width = dir.width;
        dir.tile_length = dir.rows_per_strip;
        dir.tile_depth = dir.depth;
        dir.tiled = false;
    }
    if dir.strips == 0 {
        return Err(bad("TIFF of no strips"));
    }
    dir.strips_per_image = dir.strips;
    if dir.planar_config == 2 {
        dir.strips_per_image = dir
            .strips_per_image
            .checked_div(u32::from(dir.samples_per_pixel))
            .unwrap_or(0);
    }
    // Old-style JPEG hack: one strip needs no offset, its data being in
    // the JPEGInterchangeFormat stream; it reads as offset 0.
    let ojpeg_one_strip = dir.compression == compression::OJPEG && !dir.tiled && dir.strips == 1;
    if !(have_offsets || ojpeg_one_strip) {
        return Err(bad("TIFF without StripOffsets"));
    }
    if let Some(entry) = offsets_entry {
        dir.strip_offsets = strip_array(file, &entry, dir.strips)?;
    }
    if let Some(entry) = counts_entry {
        dir.strip_byte_counts = strip_array(file, &entry, dir.strips)?;
    }

    // Channels beyond the photometric interpretation's colours are extra
    // samples, whatever ExtraSamples said.
    let colours = max_color_channels(dir.photometric_or_zero());
    let extras = dir.extra_samples();
    if colours != 0 && dir.samples_per_pixel.saturating_sub(extras) > colours {
        let n = dir.samples_per_pixel.saturating_sub(colours);
        dir.sample_info.resize(usize::from(n), extra::UNSPECIFIED);
    }

    // A palette image with no palette.
    if dir.photometric == Some(photometric::PALETTE) && dir.color_map.is_none() {
        if dir.bits_per_sample >= 8 && dir.samples_per_pixel == 3 {
            dir.photometric = Some(photometric::RGB);
        } else if dir.bits_per_sample >= 8 {
            dir.photometric = Some(photometric::MIN_IS_BLACK);
        } else {
            return Err(bad("TIFF palette image without a ColorMap"));
        }
    }

    if dir.compression != compression::OJPEG {
        if !have_byte_counts {
            let contig = dir.planar_config == 1;
            if (contig && dir.strips > 1)
                || (!contig && dir.strips != u32::from(dir.samples_per_pixel))
            {
                return Err(bad("TIFF without StripByteCounts"));
            }
            estimate_byte_counts(file, &entries, offset, &mut dir, rows_per_strip_set)?;
        } else {
            // A lone strip whose count is implausible, or uncompressed strips
            // whose first two counts differ (some writers fill the array with
            // the offsets): both are recomputed.
            let lone_and_bad = dir.strips == 1 && !dir.tiled && byte_count_looks_bad(file, &dir);
            let uneven = dir.planar_config == 1
                && dir.strips > 2
                && dir.compression == compression::NONE
                && byte_count(&dir, 0) != byte_count(&dir, 1)
                && byte_count(&dir, 0) != 0
                && byte_count(&dir, 1) != 0;
            if lone_and_bad || uneven {
                estimate_byte_counts(file, &entries, offset, &mut dir, rows_per_strip_set)?;
            }
        }
    }
    let _ = spp_set;

    // The codec's chance to fix tags up (`tif_fixuptags`).
    if dir.compression == compression::JPEG {
        jpeg_fixup_subsampling(file, &mut dir);
    }
    // Old-style JPEG reports the subsampling its data has, which it reads
    // the first time it is asked (`OJPEGVGetField`) -- by the scanline size
    // below, for `YCbCr` held together, and by the RGBA reader otherwise, to
    // the same effect.
    if dir.compression == compression::OJPEG {
        super::ojpeg::subsampling_as_read(file.data, &mut dir);
    }

    if dir.scanline_size().is_none() {
        return Err(bad("TIFF of zero-width rows"));
    }
    let size = if dir.tiled {
        dir.tile_size()
    } else {
        dir.strip_size()
    };
    if size.is_none() {
        return Err(bad("TIFF of empty strips"));
    }
    Ok(dir)
}

/// A strip's byte count, 0 past the end of the array.
pub(super) fn byte_count(dir: &Directory, strip: u32) -> u64 {
    dir.strip_byte_counts
        .get(strip as usize)
        .copied()
        .unwrap_or(0)
}

/// A strip's offset, 0 past the end of the array.
pub(super) fn strip_offset(dir: &Directory, strip: u32) -> u64 {
    dir.strip_offsets.get(strip as usize).copied().unwrap_or(0)
}

/// `JPEGFixupTagsSubsampling`: for `YCbCr` JPEG with no `YCbCrSubsampling`
/// tag, the subsampling the first strip's JPEG frame actually uses.
///
/// Some writers leave the tag out and subsample 2x1 or 1x1 all the same, and
/// the strip sizes depend on it, so libtiff reads the first strip's markers up
/// to its frame header and takes the luma's sampling factors from there -- if
/// the chroma is unsubsampled and the factors are ones a TIFF can say. It reads
/// the strip in 2048-byte pieces through the file, so a strip whose count runs
/// past the end of the file fails when a piece would, and a failure of any kind
/// leaves the tags as they were.
fn jpeg_fixup_subsampling(file: &File<'_>, dir: &mut Directory) {
    if dir.photometric != Some(photometric::YCBCR)
        || dir.planar_config != 1
        || dir.samples_per_pixel != 3
        || dir.ycbcr_subsampling_set
    {
        return;
    }
    let offset = strip_offset(dir, 0);
    if offset == 0 {
        return;
    }
    let mut reader = FixupReader {
        data: file.data,
        buffer: &[],
        file_offset: offset,
        file_left: byte_count(dir, 0),
    };
    let spp = dir.samples_per_pixel;
    if let Some([h, v]) = reader.frame_sampling(spp) {
        dir.ycbcr_subsampling = [h, v];
    }
}

/// `JPEGFixupTagsSubsamplingData` and its readers.
struct FixupReader<'a> {
    data: &'a [u8],
    /// What is left of the last 2048-byte piece.
    buffer: &'a [u8],
    file_offset: u64,
    file_left: u64,
}

impl FixupReader<'_> {
    /// `JPEGFixupTagsSubsamplingReadByte`.
    fn byte(&mut self) -> Option<u8> {
        if self.buffer.is_empty() {
            if self.file_left == 0 {
                return None;
            }
            let piece = self.file_left.min(2048);
            let start = usize::try_from(self.file_offset).ok()?;
            let len = usize::try_from(piece).ok()?;
            // A short read fails.
            self.buffer = self.data.get(start..start.checked_add(len)?)?;
            self.file_offset = self.file_offset.saturating_add(piece);
            self.file_left = self.file_left.saturating_sub(piece);
        }
        let (&first, rest) = self.buffer.split_first()?;
        self.buffer = rest;
        Some(first)
    }

    /// `JPEGFixupTagsSubsamplingReadWord`.
    fn word(&mut self) -> Option<u16> {
        let high = self.byte()?;
        let low = self.byte()?;
        Some(u16::from_be_bytes([high, low]))
    }

    /// `JPEGFixupTagsSubsamplingSkip`.
    fn skip(&mut self, n: u16) {
        let n = usize::from(n);
        if n <= self.buffer.len() {
            self.buffer = self.buffer.get(n..).unwrap_or(&[]);
            return;
        }
        // `n` is more than what the buffer holds.
        let beyond = n.saturating_sub(self.buffer.len()) as u64;
        self.buffer = &[];
        if beyond <= self.file_left {
            self.file_offset = self.file_offset.saturating_add(beyond);
            self.file_left = self.file_left.saturating_sub(beyond);
        } else {
            self.file_left = 0;
        }
    }

    /// `JPEGFixupTagsSubsamplingSec`: the new subsampling, or `None` to leave
    /// it as it is.
    fn frame_sampling(&mut self, spp: u16) -> Option<[u16; 2]> {
        loop {
            while self.byte()? != 0xFF {}
            let mut marker = self.byte()?;
            while marker == 0xFF {
                marker = self.byte()?;
            }
            match marker {
                0xD8 => {}
                0xFE | 0xE0..=0xEF | 0xDB | 0xDA | 0xC4 | 0xDD => {
                    let n = self.word()?;
                    if n < 2 {
                        return None;
                    }
                    if n > 2 {
                        self.skip(n.saturating_sub(2));
                    }
                }
                0xC0 | 0xC1 | 0xC2 | 0xC9 | 0xCA => {
                    let n = self.word()?;
                    if u32::from(n) != u32::from(spp).saturating_mul(3).saturating_add(8) {
                        return None;
                    }
                    self.skip(7);
                    let p = self.byte()?;
                    let (h, v) = (u16::from(p >> 4), u16::from(p & 15));
                    self.skip(1);
                    for _ in 1..spp {
                        self.skip(1);
                        if self.byte()? != 0x11 {
                            return None;
                        }
                        self.skip(1);
                    }
                    if !matches!(h, 1 | 2 | 4) || !matches!(v, 1 | 2 | 4) {
                        return None;
                    }
                    return Some([h, v]);
                }
                _ => return None,
            }
        }
    }
}

/// Read the directory's entries (`TIFFFetchDirectory`, memory-mapped).
fn fetch(file: &File<'_>, offset: u64) -> ImageResult<Vec<Entry>> {
    if offset == 0 {
        return Err(ImageError::Malformed("TIFF without a directory"));
    }
    let (count, entry_size, first) = if file.big_tiff {
        let raw = file.bytes(offset, 8).ok_or(ImageError::Truncated)?;
        let n = file.u64(raw);
        if n > 4096 {
            return Err(ImageError::Malformed("TIFF directory count"));
        }
        (n, 20u64, offset.saturating_add(8))
    } else {
        let raw = file.bytes(offset, 2).ok_or(ImageError::Truncated)?;
        let n = u64::from(file.u16(raw));
        if n > 4096 {
            return Err(ImageError::Malformed("TIFF directory count"));
        }
        (n, 12u64, offset.saturating_add(2))
    };
    if count == 0 {
        return Err(ImageError::Malformed("TIFF directory of no entries"));
    }
    let table_size = count.saturating_mul(entry_size);
    if table_size > file.size() {
        return Err(ImageError::Malformed("TIFF directory count"));
    }
    let table = file.bytes(first, table_size).ok_or(ImageError::Truncated)?;
    let mut entries = Vec::with_capacity(usize::try_from(count).unwrap_or(0));
    for raw in table.chunks_exact(usize::try_from(entry_size).unwrap_or(12)) {
        let tag = file.u16(raw);
        let kind = file.u16(raw.get(2..).unwrap_or(&[]));
        let mut value = [0u8; 8];
        let count = if file.big_tiff {
            if let Some(v) = raw.get(12..20) {
                value.copy_from_slice(v);
            }
            file.u64(raw.get(4..).unwrap_or(&[]))
        } else {
            if let Some(v) = raw.get(8..12) {
                value.get_mut(..4).unwrap_or(&mut []).copy_from_slice(v);
            }
            u64::from(file.u32(raw.get(4..).unwrap_or(&[])))
        };
        entries.push(Entry {
            tag,
            kind,
            count,
            value,
            ignore: false,
        });
    }
    Ok(entries)
}

/// Whether a codec's own tag is kept for this compression
/// (`_TIFFCheckFieldIsValidForCodec`); every other tag always is.
fn field_valid_for_codec(t: u16, scheme: u16) -> bool {
    let codec_tag = matches!(
        t,
        tag::PREDICTOR
            | tag::JPEG_TABLES
            | tag::JPEG_IF_OFFSET
            | tag::JPEG_IF_BYTE_COUNT
            | tag::JPEG_Q_TABLES
            | tag::JPEG_DC_TABLES
            | tag::JPEG_AC_TABLES
            | tag::JPEG_PROC
            | tag::JPEG_RESTART_INTERVAL
            | tag::BAD_FAX_LINES
            | tag::CLEAN_FAX_DATA
            | tag::CONSECUTIVE_BAD_FAX_LINES
            | tag::GROUP3_OPTIONS
            | tag::GROUP4_OPTIONS
            | tag::LERC_PARAMETERS
    );
    if !codec_tag {
        return true;
    }
    if !compression::configured(scheme) {
        return false;
    }
    match scheme {
        compression::LZW
        | compression::ADOBE_DEFLATE
        | compression::DEFLATE
        | compression::PIXARLOG
        | compression::LZMA
        | compression::ZSTD => t == tag::PREDICTOR,
        compression::JPEG => t == tag::JPEG_TABLES,
        compression::OJPEG => matches!(
            t,
            tag::JPEG_IF_OFFSET
                | tag::JPEG_IF_BYTE_COUNT
                | tag::JPEG_Q_TABLES
                | tag::JPEG_DC_TABLES
                | tag::JPEG_AC_TABLES
                | tag::JPEG_PROC
                | tag::JPEG_RESTART_INTERVAL
        ),
        compression::CCITT_RLE
        | compression::CCITT_RLEW
        | compression::CCITT_FAX3
        | compression::CCITT_FAX4 => match t {
            tag::BAD_FAX_LINES | tag::CLEAN_FAX_DATA | tag::CONSECUTIVE_BAD_FAX_LINES => true,
            tag::GROUP3_OPTIONS => scheme == compression::CCITT_FAX3,
            tag::GROUP4_OPTIONS => scheme == compression::CCITT_FAX4,
            _ => false,
        },
        compression::LERC => t == tag::LERC_PARAMETERS,
        _ => false,
    }
}

/// A tag of the second pass that libtiff reads with `TIFFFetchNormalTag`
/// and drops if it will not read: kept only if it reads.
fn second_pass_field(file: &File<'_>, entry: &Entry, dir: &mut Directory) {
    match entry.tag {
        tag::PHOTOMETRIC => {
            if let Ok(v) = file.short(entry) {
                dir.photometric = Some(v);
            }
        }
        tag::FILL_ORDER => {
            if let Ok(v @ (1 | 2)) = file.short(entry) {
                dir.fill_order = v;
            }
        }
        tag::ORIENTATION => {
            if let Ok(v @ 1..=8) = file.short(entry) {
                dir.orientation = v;
            }
        }
        tag::INK_SET => {
            if let Ok(v) = file.short(entry) {
                dir.ink_set = v;
            }
        }
        tag::MATTEING => {
            if let Ok(v) = file.short(entry) {
                dir.sample_info = if v != 0 {
                    vec![extra::ASSOCIATED_ALPHA]
                } else {
                    Vec::new()
                };
            }
        }
        tag::PREDICTOR => {
            if let Ok(v) = file.short(entry) {
                dir.predictor = v;
            }
        }
        tag::YCBCR_COEFFICIENTS => {
            if let Some([a, b, c]) = file.floats(entry, 3).as_deref() {
                dir.ycbcr_coefficients = Some([*a, *b, *c]);
            }
        }
        tag::REFERENCE_BLACK_WHITE => {
            if let Some(v) = file.floats(entry, 6) {
                if let Ok(six) = <[f32; 6]>::try_from(v.as_slice()) {
                    dir.reference_black_white = Some(six);
                }
            }
        }
        tag::WHITE_POINT => {
            if let Some([x, y]) = file.floats(entry, 2).as_deref() {
                dir.white_point = Some([*x, *y]);
            }
        }
        tag::GROUP3_OPTIONS => {
            if let Ok(v) = file.long(entry) {
                dir.group3_options = v;
            }
        }
        tag::YCBCR_SUBSAMPLING => {
            // Exactly two values, or it is dropped.
            if entry.count == 2 {
                if let Ok(v) = file.shorts(entry) {
                    if let [h, w] = v.as_slice() {
                        dir.ycbcr_subsampling = [*h, *w];
                        dir.ycbcr_subsampling_set = true;
                        if dir.compression == compression::OJPEG {
                            // `OJPEGVSetField` keeps each in a byte.
                            let [h, w] = [*h, *w].map(|v| v.to_le_bytes()[0]);
                            dir.ojpeg.hor = h;
                            dir.ojpeg.ver = w;
                            dir.ycbcr_subsampling = [u16::from(h), u16::from(w)];
                        }
                    }
                }
            }
        }
        tag::JPEG_IF_OFFSET | tag::JPEG_IF_BYTE_COUNT => {
            // `TIFF_SETGET_UINT64`.
            if let Ok(v) = file.scalar(entry, (0, i128::from(u64::MAX)), false) {
                let v = u64::try_from(v).unwrap_or(0);
                if entry.tag == tag::JPEG_IF_OFFSET {
                    dir.ojpeg.jif = v;
                } else {
                    dir.ojpeg.jif_len = v;
                }
            }
        }
        tag::JPEG_Q_TABLES | tag::JPEG_DC_TABLES | tag::JPEG_AC_TABLES => {
            // `TIFF_SETGET_C32_UINT64`; `OJPEGVSetField` refuses more than
            // three, and keeps nothing of none.
            if let Ok(values) = file.integers(entry, (0, i128::from(u64::MAX)), u64::MAX) {
                if (1..=3).contains(&values.len()) {
                    let values: Vec<u64> = values
                        .into_iter()
                        .map(|v| u64::try_from(v).unwrap_or(0))
                        .collect();
                    match entry.tag {
                        tag::JPEG_Q_TABLES => dir.ojpeg.q_tables = values,
                        tag::JPEG_DC_TABLES => dir.ojpeg.dc_tables = values,
                        _ => dir.ojpeg.ac_tables = values,
                    }
                }
            }
        }
        tag::JPEG_RESTART_INTERVAL => {
            if let Ok(v) = file.short(entry) {
                dir.ojpeg.restart_interval = v;
            }
        }
        tag::JPEG_TABLES => {
            // `TIFF_SETGET_C32_UINT8`, and `JPEGVSetField` refuses a count of
            // zero.
            if let Ok(bytes) = file.byte_array(entry) {
                if !bytes.is_empty() {
                    dir.jpeg_tables = Some(bytes);
                }
            }
        }
        _ => {}
    }
}

/// Read `StripOffsets` or `StripByteCounts` (`TIFFFetchStripThing`): at most
/// one value per strip, and zeros for any missing.
fn strip_array(file: &File<'_>, entry: &Entry, strips: u32) -> ImageResult<Vec<u64>> {
    let bad = ImageError::Malformed("TIFF strip array");
    let values = file
        .integers(entry, (0, i128::from(u64::MAX)), u64::from(strips))
        .map_err(|_| bad.clone())?;
    let mut out: Vec<u64> = values
        .into_iter()
        .map(|v| u64::try_from(v).unwrap_or(0))
        .collect();
    if entry.count < u64::from(strips) {
        if strips > 1_000_000 {
            return Err(bad);
        }
        out.resize(strips as usize, 0);
    }
    Ok(out)
}

/// The most colour channels a photometric interpretation has
/// (`_TIFFGetMaxColorChannels`); 0 for one libtiff leaves alone.
fn max_color_channels(p: u16) -> u16 {
    match p {
        photometric::PALETTE | photometric::MIN_IS_WHITE | photometric::MIN_IS_BLACK => 1,
        photometric::YCBCR
        | photometric::RGB
        | photometric::CIELAB
        | photometric::LOGLUV
        | photometric::ITULAB
        | photometric::ICCLAB => 3,
        photometric::SEPARATED | photometric::MASK => 4,
        _ => 0,
    }
}

/// Whether a single strip's byte count is implausible (`ByteCountLooksBad`).
fn byte_count_looks_bad(file: &File<'_>, dir: &Directory) -> bool {
    let count = byte_count(dir, 0);
    let offset = strip_offset(dir, 0);
    if offset == 0 {
        return false;
    }
    if count == 0 {
        return true;
    }
    if dir.compression != compression::NONE {
        return false;
    }
    let size = file.size();
    if offset <= size && count > size.saturating_sub(offset) {
        return true;
    }
    let scanline = dir.scanline_size().unwrap_or(0);
    match scanline.checked_mul(u64::from(dir.length)) {
        None => true,
        Some(need) => count < need,
    }
}

/// Make up byte counts from the file's size or the image's
/// (`EstimateStripByteCounts`).
fn estimate_byte_counts(
    file: &File<'_>,
    entries: &[Entry],
    dir_offset: u64,
    dir: &mut Directory,
    rows_per_strip_set: bool,
) -> ImageResult<()> {
    let bad = || ImageError::Malformed("TIFF strip sizes");
    let strips = dir.strips as usize;
    if (strips as u64).saturating_mul(8) > 100 * 1024 * 1024
        && (strips as u64).saturating_mul(8) > file.size()
    {
        return Err(bad());
    }
    let mut counts = vec![0u64; strips];
    if dir.compression != compression::NONE {
        let (header, entry_size, tail) = if file.big_tiff {
            (16u64, 20u64, 16u64)
        } else {
            (8, 12, 6)
        };
        let mut space = header
            .saturating_add(tail)
            .saturating_add((entries.len() as u64).saturating_mul(entry_size));
        let inline = if file.big_tiff { 8 } else { 4 };
        for entry in entries {
            let width = kind::width(entry.kind);
            if width == 0 {
                return Err(bad());
            }
            let size = entry.count.checked_mul(width).ok_or_else(bad)?;
            if size > inline {
                space = space.checked_add(size).ok_or_else(bad)?;
            }
        }
        let size = file.size();
        let mut each = size.saturating_sub(space);
        if size < space {
            each = size;
        }
        if dir.planar_config == 2 {
            each = each
                .checked_div(u64::from(dir.samples_per_pixel))
                .unwrap_or(0);
        }
        counts.fill(each);
        // The last strip ends at the end of the file.
        let last = strips.saturating_sub(1);
        let offset = strip_offset(dir, u32::try_from(last).unwrap_or(0));
        let count = counts.get(last).copied().unwrap_or(0);
        if offset.checked_add(count).is_none() {
            return Err(bad());
        }
        if offset.saturating_add(count) > size {
            let fixed = if offset >= size {
                0
            } else {
                size.saturating_sub(offset)
            };
            if let Some(c) = counts.get_mut(last) {
                *c = fixed;
            }
        }
    } else if dir.tiled {
        counts.fill(dir.tile_size().unwrap_or(0));
    } else {
        let row = dir.scanline_size().unwrap_or(0);
        let rows = dir.length.checked_div(dir.strips_per_image).unwrap_or(0);
        counts.fill(row.checked_mul(u64::from(rows)).ok_or_else(bad)?);
    }
    let _ = dir_offset;
    dir.strip_byte_counts = counts;
    if !rows_per_strip_set {
        dir.rows_per_strip = dir.length;
    }
    Ok(())
}
