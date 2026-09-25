//! Reading one strip or tile: finding its bytes and decompressing them.
//!
//! A port of libtiff 4.7.1's `TIFFFillStrip`/`TIFFFillTile` (where the bytes
//! are, and the checks on their count) and `TIFFReadEncodedStrip`/
//! `TIFFReadEncodedTile` (the codec, the predictor, the byte swap), for a
//! file read whole into memory -- libtiff's memory-mapped path, which is how
//! its callers open a file.
//!
//! Every failure is final. The reference -- libtiff's RGBA reader, as image
//! viewers call it -- stops at the first strip that will not read and shows
//! nothing, so a strip here either decodes completely or refuses the file.

use alloc::vec::Vec;

use super::dir::{self, Directory, File, compression, photometric};
use super::fax::{self, Fax};
use super::lzw::Lzw;
use crate::jpeg::{ColorSpace, Decompress, Headed, Tables};
use crate::{ImageError, ImageResult, Limits};

/// The JPEG codec's state for one image (libtiff's `JPEGState`).
struct Jpeg {
    /// libjpeg's permanent tables: `JPEGTables`', then whatever each strip
    /// redefines, for the strips after it.
    tables: Tables,
    /// `sp->h_sampling`, `sp->v_sampling`: the `YCbCr` subsampling for
    /// `YCbCr`, else 1.
    h_sampling: u16,
    v_sampling: u16,
}

/// libtiff's JPEG progress monitor abandons a datastream at this many scans.
const JPEG_MAX_SCANS: u32 = 100;

/// State kept across the strips of one image, as libtiff keeps it on the
/// `TIFF` handle.
pub(super) struct Reader<'a> {
    file: File<'a>,
    dir: &'a Directory,
    /// libtiff's raw-data buffer size (`tif_rawdatasize`). Only the first
    /// uncompressed tile is checked against it, but what it holds depends
    /// on every strip read before.
    raw_capacity: u64,
    /// Whether the raw buffer last pointed into the file itself
    /// (`TIFF_BUFFERMMAP`) rather than at a copy.
    raw_in_file: bool,
    /// A bit-reversed copy of the current strip, for `FillOrder` 2.
    reversed: Vec<u8>,
    lzw: Option<Lzw>,
    fax: Option<Fax>,
    jpeg: Option<Jpeg>,
    /// The codec's one-time setup (`tif_setupdecode`), once it has run.
    setup: Option<bool>,
    /// What a JPEG strip's decode may allocate.
    limits: Limits,
}

impl<'a> Reader<'a> {
    pub(super) fn new(file: File<'a>, dir: &'a Directory, limits: Limits) -> Self {
        Self {
            file,
            dir,
            raw_capacity: 0,
            raw_in_file: false,
            reversed: Vec::new(),
            lzw: None,
            fax: None,
            jpeg: None,
            setup: None,
            limits,
        }
    }

    /// Decode strip (or tile) `index` into `out[..size]`, as
    /// `TIFFReadEncodedStrip` does. `first_tile` is set for the first tile of
    /// an image, which libtiff reads through a stricter path
    /// (`_TIFFReadEncodedTileAndAllocBuffer`); `alloc_size` is the buffer
    /// that path would allocate.
    ///
    /// # Errors
    ///
    /// Any failure to find or decode the strip.
    pub(super) fn decode(
        &mut self,
        index: u32,
        out: &mut [u8],
        size: usize,
        first_tile: Option<u64>,
    ) -> ImageResult<()> {
        if index >= self.dir.strips {
            return Err(ImageError::Malformed("TIFF strip out of range"));
        }
        let raw = self.fill(index)?;
        if let Some(alloc_size) = first_tile {
            let tile_size = self.dir.tile_size().unwrap_or(0);
            if self.dir.compression == compression::NONE {
                if self.raw_capacity != tile_size {
                    return Err(ImageError::Malformed("TIFF tile of the wrong size"));
                }
            } else {
                let ratio = match self.dir.compression {
                    compression::ZSTD => 33_000,
                    compression::LZMA => 7_000,
                    _ => 1_000,
                };
                if alloc_size > 100_000_000
                    && self.raw_capacity < tile_size.checked_div(ratio).unwrap_or(0)
                {
                    return Err(ImageError::Malformed("TIFF tile implausibly compressed"));
                }
            }
        }
        self.setup()?;
        let dest = out
            .get_mut(..size)
            .ok_or(ImageError::Malformed("TIFF strip larger than its buffer"))?;
        self.run_codec(raw, dest, index)?;
        self.after(dest)
    }

    /// `TIFFFillStrip`/`TIFFFillTile`: the strip's bytes, bit-reversed if the
    /// fill order asks and the codec does not do it itself.
    fn fill(&mut self, index: u32) -> ImageResult<Raw> {
        let dir = self.dir;
        let mut count = dir::byte_count(dir, index);
        if count == 0 || count > i64::MAX as u64 {
            return Err(ImageError::Malformed("TIFF strip of no bytes"));
        }
        // A count wildly larger than the strip could need is cut down.
        if count > 1024 * 1024 {
            let strip = if dir.tiled {
                dir.tile_size()
            } else {
                dir.strip_size()
            }
            .unwrap_or(0);
            if strip != 0 && count.saturating_sub(4096) / 10 > strip {
                count = strip.saturating_mul(10).saturating_add(4096);
            }
        }
        let offset = dir::strip_offset(dir, index);
        let size = self.file.data.len() as u64;
        if count > size || offset > size.saturating_sub(count) {
            return Err(ImageError::Truncated);
        }
        let start = usize::try_from(offset).map_err(|_| ImageError::Truncated)?;
        let len = usize::try_from(count).map_err(|_| ImageError::Truncated)?;
        let reverse = dir.fill_order != 1 && !compression::reads_own_bit_order(dir.compression);
        if !reverse {
            self.raw_capacity = count;
            self.raw_in_file = true;
            return Ok(Raw::File { start, len });
        }
        if self.raw_in_file {
            self.raw_capacity = 0;
            self.raw_in_file = false;
        }
        if count > self.raw_capacity {
            self.raw_capacity = count.div_ceil(1024).saturating_mul(1024);
        }
        let bytes = self
            .file
            .data
            .get(start..start.saturating_add(len))
            .ok_or(ImageError::Truncated)?;
        self.reversed.clear();
        self.reversed.extend(bytes.iter().map(|b| b.reverse_bits()));
        Ok(Raw::Reversed)
    }

    /// The codec's one-time setup (`tif_setupdecode`): for the codecs
    /// with a predictor, whether it can apply to these samples
    /// (`PredictorSetup`); for fax, its state (`Fax3SetupState`). A setup
    /// that fails fails every strip.
    fn setup(&mut self) -> ImageResult<()> {
        if self.setup.is_none() {
            let ok = predictor_valid(self.dir) && self.setup_fax() && self.setup_jpeg();
            self.setup = Some(ok);
        }
        if self.setup == Some(true) {
            Ok(())
        } else {
            Err(ImageError::Unsupported(
                "TIFF compression for these samples",
            ))
        }
    }

    /// `Fax3SetupState`, for the fax schemes: true for any other.
    fn setup_fax(&mut self) -> bool {
        let dir = self.dir;
        let kind = match dir.compression {
            compression::CCITT_RLE => fax::Kind::Rle {
                word_aligned: false,
            },
            compression::CCITT_RLEW => fax::Kind::Rle { word_aligned: true },
            compression::CCITT_FAX3 => fax::Kind::Group3 {
                two_d: dir.group3_options & 1 != 0,
            },
            compression::CCITT_FAX4 => fax::Kind::Group4,
            _ => return true,
        };
        let (row_bytes, row_pixels) = if dir.tiled {
            (dir.tile_row_size(), dir.tile_width)
        } else {
            (dir.scanline_size(), dir.width)
        };
        let Some(row_bytes) = row_bytes else {
            return false;
        };
        self.fax = Fax::new(
            kind,
            dir.bits_per_sample,
            dir.samples_per_pixel,
            dir.planar_config == 2,
            row_bytes,
            row_pixels,
            dir.fill_order,
        );
        self.fax.is_some()
    }

    /// `JPEGSetupDecode`, for JPEG: true for any other scheme. `JPEGTables`,
    /// if the directory has it, must be a datastream of tables alone -- it is
    /// read here, once, into the tables every strip is decoded with.
    fn setup_jpeg(&mut self) -> bool {
        let dir = self.dir;
        if dir.compression != compression::JPEG {
            return true;
        }
        let mut tables = Tables::new();
        if let Some(bytes) = &dir.jpeg_tables {
            let mut stream = Decompress::new(bytes, &mut tables);
            if stream.read_header(false) != Ok(Headed::TablesOnly) {
                return false;
            }
        }
        let (h_sampling, v_sampling) = if dir.photometric == Some(photometric::YCBCR) {
            (dir.ycbcr_subsampling[0], dir.ycbcr_subsampling[1])
        } else {
            (1, 1)
        };
        self.jpeg = Some(Jpeg {
            tables,
            h_sampling,
            v_sampling,
        });
        true
    }

    /// `JPEGPreDecode` then `JPEGDecode`: strip (or tile) `index`'s JPEG
    /// datastream into `out`, checked against the strip it stands for.
    fn decode_jpeg(&mut self, bytes: &[u8], out: &mut [u8], index: u32) -> ImageResult<()> {
        let dir = self.dir;
        let fail = ImageError::Malformed("TIFF JPEG strip");
        let jpeg = self.jpeg.as_mut().ok_or(fail.clone())?;
        let (h_sampling, v_sampling) = (jpeg.h_sampling, jpeg.v_sampling);
        let mut stream = Decompress::new(bytes, &mut jpeg.tables);
        if stream.read_header(true) != Ok(Headed::Image) {
            return Err(fail);
        }
        // The strip or tile this datastream stands for.
        let per_plane = dir.strips_per_image.max(1);
        // `per_plane` is at least 1.
        let plane = index.checked_div(per_plane).unwrap_or(0);
        let row = index
            .checked_rem(per_plane)
            .unwrap_or(0)
            .wrapping_mul(dir.rows_per_strip);
        let (mut segment_width, mut segment_height, bytes_per_line) = if dir.tiled {
            (dir.tile_width, dir.tile_length, dir.tile_row_size())
        } else {
            (
                dir.width,
                dir.length.wrapping_sub(row).min(dir.rows_per_strip),
                dir.scanline_size(),
            )
        };
        if dir.planar_config == 2 && plane > 0 {
            if h_sampling == 0 || v_sampling == 0 {
                return Err(fail);
            }
            segment_width = dir::howmany32(segment_width, u32::from(h_sampling));
            segment_height = dir::howmany32(segment_height, u32::from(v_sampling));
        }
        let width = u32::try_from(stream.image_width()).unwrap_or(u32::MAX);
        let height = u32::try_from(stream.image_height()).unwrap_or(u32::MAX);
        // A last strip whose datastream kept the full strip's height is
        // tolerated; anything else larger than its strip is not.
        let tall_last_strip = width == segment_width
            && height > segment_height
            && row.wrapping_add(segment_height) == dir.length
            && !dir.tiled;
        if !tall_last_strip && (width > segment_width || height > segment_height) {
            return Err(fail);
        }
        let components = if dir.planar_config == 1 {
            usize::from(dir.samples_per_pixel)
        } else {
            1
        };
        if stream.num_components() != components
            || u16::from(stream.data_precision()) != dir.bits_per_sample
        {
            return Err(fail);
        }
        // The luma's sampling must be the TIFF's, every other component's 1x1.
        let sampling_ok = if dir.planar_config == 1 {
            stream.sampling(0) == Some((usize::from(h_sampling), usize::from(v_sampling)))
                && (1..components).all(|ci| stream.sampling(ci) == Some((1, 1)))
        } else {
            stream.sampling(0) == Some((1, 1))
        };
        if !sampling_ok {
            return Err(fail);
        }
        if dir.planar_config == 1 && dir.photometric == Some(photometric::YCBCR) {
            // `JPEGCOLORMODE_RGB`, which `TIFFRGBAImageBegin` always asks for.
            stream.set_color_spaces(ColorSpace::YCbCr, ColorSpace::Rgb);
        } else {
            stream.set_color_spaces(ColorSpace::Unknown, ColorSpace::Unknown);
        }
        stream.set_max_scans(JPEG_MAX_SCANS);
        let line = usize::try_from(bytes_per_line.unwrap_or(0)).unwrap_or(usize::MAX);
        let rows = if line == 0 {
            0
        } else {
            out.len()
                .checked_div(line)
                .unwrap_or(0)
                .min(stream.image_height())
        };
        stream
            .start(&self.limits, Some(rows))
            .map_err(|_| fail.clone())?;
        if line == 0 {
            out.fill(0);
            return Err(fail);
        }
        // Each scanline goes straight into the strip buffer a line apart, as
        // libtiff reads it. A scanline is a byte a sample, which only a
        // lossless JPEG of fewer than 8 bits makes longer than the TIFF's
        // packed line: then the scanlines overlap, and libtiff's last one runs
        // past the buffer's end -- a heap overflow there, stopped at the end
        // here, so the two agree wherever libtiff's run is defined.
        for r in 0..rows {
            let Ok(samples) = stream.read_row() else {
                out.fill(0);
                return Err(fail);
            };
            let at = r.saturating_mul(line);
            let end = at.saturating_add(samples.len()).min(out.len());
            if let (Some(dest), Some(src)) =
                (out.get_mut(at..end), samples.get(..end.saturating_sub(at)))
            {
                dest.copy_from_slice(src);
            }
        }
        // Once every row is read the datastream is finished: its markers are
        // read to the end, and tables defined after the scan stay for the
        // strips after it. Whether that succeeds changes nothing else --
        // libtiff returns `output_scanline < output_height ||
        // TIFFjpeg_finish_decompress(sp)`, and a C `||` is 1 whenever either
        // side is nonzero, the failure's -1 included.
        if stream.output_scanline() >= stream.output_height() {
            // Deliberately discarded, as libtiff discards it; see above.
            let _ = stream.finish();
        }
        Ok(())
    }

    fn run_codec(&mut self, raw: Raw, out: &mut [u8], index: u32) -> ImageResult<()> {
        let offset = dir::strip_offset(self.dir, index);
        let bytes: &[u8] = match raw {
            Raw::File { start, len } => self
                .file
                .data
                .get(start..start.saturating_add(len))
                .ok_or(ImageError::Truncated)?,
            Raw::Reversed => &self.reversed,
        };
        match self.dir.compression {
            compression::NONE => {
                // `DumpModeDecode`: the strip must hold at least what is asked.
                let src = bytes.get(..out.len()).ok_or(ImageError::Truncated)?;
                out.copy_from_slice(src);
                Ok(())
            }
            compression::PACKBITS => packbits(bytes, out),
            compression::LZW => self.lzw.get_or_insert_with(Lzw::new).decode(bytes, out),
            compression::CCITT_RLE
            | compression::CCITT_RLEW
            | compression::CCITT_FAX3
            | compression::CCITT_FAX4 => {
                let fax = self
                    .fax
                    .as_mut()
                    .ok_or(ImageError::Unsupported("TIFF fax"))?;
                let decoded = fax.decode(bytes, out, offset);
                // libtiff's fax decoders fail with -1, and it tests a
                // strip's decode with `<= 0` but a tile's for truth: a
                // fax tile that fails is shown as far as it decoded.
                if self.dir.tiled { Ok(()) } else { decoded }
            }
            compression::JPEG => {
                let owned;
                let bytes: &[u8] = match raw {
                    Raw::File { start, len } => self
                        .file
                        .data
                        .get(start..start.saturating_add(len))
                        .ok_or(ImageError::Truncated)?,
                    Raw::Reversed => {
                        owned = core::mem::take(&mut self.reversed);
                        &owned
                    }
                };
                self.decode_jpeg(bytes, out, index)
            }
            compression::DEFLATE | compression::ADOBE_DEFLATE => {
                let full = if self.dir.tiled {
                    self.dir.tile_size()
                } else {
                    self.dir.strip_size()
                };
                let slack = usize::try_from(full.unwrap_or(0)).unwrap_or(usize::MAX);
                inflate(bytes, out, slack)
            }
            scheme if !compression::known(scheme) => {
                Err(ImageError::Unsupported("TIFF compression scheme"))
            }
            _ => Err(ImageError::Unsupported(
                "TIFF compression scheme not yet decoded here",
            )),
        }
    }

    /// What follows the codec: the horizontal predictor
    /// (`PredictorDecodeTile`) and the byte swap of big-endian 16-bit
    /// samples into this machine's order (`tif_postdecode`), which the
    /// predictor does itself when it runs.
    fn after(&self, out: &mut [u8]) -> ImageResult<()> {
        let dir = self.dir;
        // `JPEGSetupDecode` takes the byte swap away (`_TIFFNoPostDecode`).
        if dir.compression == compression::JPEG {
            return Ok(());
        }
        let swab16 = self.file.big_endian && dir.bits_per_sample == 16;
        let uses_predictor = matches!(
            dir.compression,
            compression::LZW | compression::DEFLATE | compression::ADOBE_DEFLATE
        );
        if uses_predictor && dir.predictor == 2 {
            let row = usize::try_from(
                if dir.tiled {
                    dir.tile_row_size()
                } else {
                    dir.scanline_size()
                }
                .unwrap_or(0),
            )
            .map_err(|_| ImageError::Malformed("TIFF row size"))?;
            if row == 0 || !out.len().is_multiple_of(row) {
                return Err(ImageError::Malformed("TIFF strip not whole rows"));
            }
            let stride = if dir.planar_config == 1 {
                usize::from(dir.samples_per_pixel)
            } else {
                1
            };
            for chunk in out.chunks_mut(row) {
                match dir.bits_per_sample {
                    8 => accumulate8(chunk, stride)?,
                    16 => {
                        if swab16 {
                            swap16(chunk);
                        }
                        accumulate16(chunk, stride)?;
                    }
                    // 32 and 64 bits pass setup but not the RGBA reader,
                    // which never asks for their strips.
                    _ => return Err(ImageError::Unsupported("TIFF predictor for these samples")),
                }
            }
            return Ok(());
        }
        if swab16 {
            swap16(out);
        }
        Ok(())
    }
}

/// Where a filled strip's bytes are.
#[derive(Clone, Copy)]
enum Raw {
    File { start: usize, len: usize },
    Reversed,
}

/// Whether `PredictorSetup` accepts the predictor for these samples.
fn predictor_valid(dir: &Directory) -> bool {
    let uses_predictor = matches!(
        dir.compression,
        compression::LZW | compression::DEFLATE | compression::ADOBE_DEFLATE
    );
    if !uses_predictor {
        return true;
    }
    let ok = match dir.predictor {
        1 => return true,
        2 => matches!(dir.bits_per_sample, 8 | 16 | 32 | 64),
        3 => dir.sample_format == 3 && matches!(dir.bits_per_sample, 16 | 24 | 32 | 64),
        _ => false,
    };
    let row = if dir.tiled {
        dir.tile_row_size()
    } else {
        dir.scanline_size()
    };
    ok && row.is_some()
}

/// `horAcc8`: each sample is the sum of itself and the one `stride` before.
fn accumulate8(row: &mut [u8], stride: usize) -> ImageResult<()> {
    if stride == 0 || !row.len().is_multiple_of(stride) {
        return Err(ImageError::Malformed("TIFF predictor row"));
    }
    for i in stride..row.len() {
        let prev = row.get(i.wrapping_sub(stride)).copied().unwrap_or(0);
        if let Some(b) = row.get_mut(i) {
            *b = b.wrapping_add(prev);
        }
    }
    Ok(())
}

/// `horAcc16`, on little-endian words.
fn accumulate16(row: &mut [u8], stride: usize) -> ImageResult<()> {
    let pair = stride.checked_mul(2).unwrap_or(0);
    if pair == 0 || !row.len().is_multiple_of(pair) {
        return Err(ImageError::Malformed("TIFF predictor row"));
    }
    for i in (pair..row.len()).step_by(2) {
        let prev = word(row, i.wrapping_sub(pair));
        let sum = word(row, i).wrapping_add(prev).to_le_bytes();
        if let Some(slot) = row.get_mut(i..i.wrapping_add(2)) {
            slot.copy_from_slice(&sum);
        }
    }
    Ok(())
}

/// The little-endian word at byte `at`.
fn word(row: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([
        row.get(at).copied().unwrap_or(0),
        row.get(at.wrapping_add(1)).copied().unwrap_or(0),
    ])
}

/// Swap each pair of bytes (`TIFFSwabArrayOfShort`).
fn swap16(data: &mut [u8]) {
    for pair in data.chunks_exact_mut(2) {
        pair.swap(0, 1);
    }
}

/// `PackBitsDecode`.
fn packbits(raw: &[u8], out: &mut [u8]) -> ImageResult<()> {
    let mut input = raw.iter();
    let mut op = 0usize;
    let occ = out.len();
    while op < occ {
        let Some(&header) = input.next() else { break };
        let n = header.cast_signed();
        let left = occ.saturating_sub(op);
        if n < 0 {
            if n == -128 {
                continue;
            }
            // Replicate the next byte 1 - n times, or as many as fit.
            let count = usize::from(n.unsigned_abs()).saturating_add(1).min(left);
            let Some(&b) = input.next() else { break };
            out.get_mut(op..op.saturating_add(count))
                .ok_or(ImageError::Truncated)?
                .fill(b);
            op = op.saturating_add(count);
        } else {
            // Copy the next n + 1 bytes, or as many as fit.
            let count = usize::from(n.unsigned_abs()).saturating_add(1).min(left);
            let rest = input.as_slice();
            let Some(src) = rest.get(..count) else { break };
            out.get_mut(op..op.saturating_add(count))
                .ok_or(ImageError::Truncated)?
                .copy_from_slice(src);
            op = op.saturating_add(count);
            input = rest.get(count..).unwrap_or(&[]).iter();
        }
    }
    if op < occ {
        return Err(ImageError::Corrupt(
            "PackBits data ends before the strip does",
        ));
    }
    Ok(())
}

/// A zlib stream decoded as libtiff decodes a whole strip: through
/// libdeflate (`libdeflate_zlib_decompress`), which accepts a stream that
/// holds more than the strip needs and refuses one that holds less.
///
/// Two differences remain, both confined to damaged streams, because the
/// shared `deflate` crate decodes a block at a time and does not say where
/// its input ended: damage later in the block that finishes the strip is
/// seen here and not by libdeflate, and the checksum of a stream that ends
/// exactly at the strip's end is read from the strip's last four bytes, not
/// from just after the stream. `slack` is how much more than the strip a
/// block may hold before this gives up.
fn inflate(raw: &[u8], out: &mut [u8], slack: usize) -> ImageResult<()> {
    let bad = ImageError::Corrupt("TIFF deflate data");
    let [cmf, flg, ..] = raw else { return Err(bad) };
    if raw.len() < 6 {
        return Err(bad);
    }
    let header = u16::from_be_bytes([*cmf, *flg]);
    if header % 31 != 0 || (header >> 8) & 0xF != 8 || header >> 12 > 7 || (header >> 5) & 1 != 0 {
        return Err(bad);
    }
    let trailer = raw.len().saturating_sub(4);
    let body = raw.get(2..trailer).ok_or(ImageError::Truncated)?;
    let mut stream = deflate::inflate_stream(body, out.len().saturating_add(slack));
    let mut filled = 0usize;
    while filled < out.len() {
        let rest = out.get_mut(filled..).ok_or(ImageError::Truncated)?;
        match stream.read(rest)? {
            0 => {
                return Err(ImageError::Corrupt(
                    "TIFF deflate data ends before the strip does",
                ));
            }
            n => filled = filled.saturating_add(n),
        }
    }
    let mut probe = [0u8; 1];
    match stream.read(&mut probe) {
        // The stream ends with the strip: its checksum must hold.
        Ok(0) => {
            let tail = raw.get(trailer..).ok_or(ImageError::Truncated)?;
            let stored = u32::from_be_bytes([
                tail.first().copied().unwrap_or(0),
                tail.get(1).copied().unwrap_or(0),
                tail.get(2).copied().unwrap_or(0),
                tail.get(3).copied().unwrap_or(0),
            ]);
            if deflate::adler32(out) != stored {
                return Err(ImageError::Corrupt("TIFF deflate checksum"));
            }
            Ok(())
        }
        // More than the strip needs: libdeflate stops, content.
        Ok(_) | Err(deflate::Error::OutputTooLarge) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    #[test]
    fn packbits_decodes_runs_and_literals() {
        // 2 literals, a run of 3, a no-op, one literal.
        let raw = [1, b'a', b'b', 0xFE, b'c', 0x80, 0, b'd'];
        let mut out = [0u8; 6];
        packbits(&raw, &mut out).unwrap();
        assert_eq!(&out, b"abcccd");
    }

    #[test]
    fn packbits_that_stops_short_is_an_error_and_one_that_runs_over_is_cut() {
        let mut out = [0u8; 4];
        assert!(packbits(&[1, b'a', b'b'], &mut out).is_err());
        let mut out = [0u8; 2];
        packbits(&[0xFD, b'x'], &mut out).unwrap();
        assert_eq!(&out, b"xx");
    }

    #[test]
    fn the_predictor_accumulates_each_channel() {
        let mut row = [10u8, 20, 1, 2, 255, 1];
        accumulate8(&mut row, 2).unwrap();
        assert_eq!(row, [10, 20, 11, 22, 10, 23]);
        let mut words = [1u8, 0, 2, 0, 0xFF, 0xFF];
        accumulate16(&mut words, 1).unwrap();
        assert_eq!(words, [1, 0, 3, 0, 2, 0]);
    }

    #[test]
    fn a_zlib_stream_with_more_than_the_strip_needs_is_accepted() {
        let data: Vec<u8> = (0..200u8).collect();
        let z = deflate::zlib_deflate(&data);
        let mut out = [0u8; 100];
        inflate(&z, &mut out, 100).unwrap();
        assert_eq!(&out[..], &data[..100]);
        let mut all = [0u8; 200];
        inflate(&z, &mut all, 0).unwrap();
        let mut more = [0u8; 201];
        assert!(inflate(&z, &mut more, 0).is_err());
    }
}
