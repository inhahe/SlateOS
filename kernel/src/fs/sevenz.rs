//! 7-Zip archive extraction (read-only).
//!
//! Implements parsing and decompression of `.7z` archives using the
//! existing LZMA, LZMA2, DEFLATE, and BZip2 decoders.
//!
//! ## Format overview
//!
//! A 7z file consists of:
//! 1. Signature header (32 bytes): magic, version, start-header CRC
//! 2. Start header: points to the main header (offset + size + CRC)
//! 3. Main header (often compressed): stream info, file metadata
//! 4. Pack streams: compressed data for all folders
//!
//! The main header can itself be LZMA2-compressed ("encoded header"),
//! requiring a two-pass parse: decompress header, then parse it.
//!
//! 7z uses "solid" compression: multiple files in a folder are
//! concatenated before compression.  After decompressing, individual
//! files are extracted by splitting the output at cumulative size
//! boundaries.
//!
//! ## Supported codecs
//!
//! - Copy (0x00): stored, no compression
//! - LZMA (0x030101): LZMA with 5-byte properties header
//! - LZMA2 (0x21): LZMA2 with dict-size byte
//! - DEFLATE (0x040108): RFC 1951 inflate
//! - BZip2 (0x040202): bzip2 stream decompression
//!
//! Unsupported: AES-256 encryption, BCJ/BCJ2 filters, PPMd, Delta.
//!
//! ## References
//!
//! - 7-Zip LZMA SDK: `7zFormat.txt`
//! - p7zip source code
//! - <https://py7zr.readthedocs.io/en/latest/archive_format.html>

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// 7z constants
// ---------------------------------------------------------------------------

/// 7z file signature (6 bytes).
const SEVENZ_MAGIC: [u8; 6] = [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C];

/// 7z property IDs used in the header.
const K_END: u8 = 0x00;
const K_HEADER: u8 = 0x01;
const K_ARCHIVE_PROPERTIES: u8 = 0x02;
const K_MAIN_STREAMS_INFO: u8 = 0x04;
const K_FILES_INFO: u8 = 0x05;
const K_PACK_INFO: u8 = 0x06;
const K_UNPACK_INFO: u8 = 0x07;
const K_SUB_STREAMS_INFO: u8 = 0x08;
const K_SIZE: u8 = 0x09;
const K_CRC: u8 = 0x0A;
const K_FOLDER: u8 = 0x0B;
const K_CODERS_UNPACK_SIZE: u8 = 0x0C;
const K_NUM_UNPACK_STREAM: u8 = 0x0D;
const K_EMPTY_STREAM: u8 = 0x0E;
const K_EMPTY_FILE: u8 = 0x0F;
const K_ANTI: u8 = 0x10;
const K_NAME: u8 = 0x11;
const K_CREATION_TIME: u8 = 0x12;
const K_LAST_ACCESS_TIME: u8 = 0x13;
const K_LAST_WRITE_TIME: u8 = 0x14;
const K_WIN_ATTRIBUTES: u8 = 0x15;
const K_ENCODED_HEADER: u8 = 0x17;

/// 7z codec IDs.
const CODEC_COPY: u64 = 0x00;
const CODEC_LZMA: u64 = 0x03_01_01;
const CODEC_LZMA2: u64 = 0x21;
const CODEC_DEFLATE: u64 = 0x04_01_08;
const CODEC_BZIP2: u64 = 0x04_02_02;

/// Safety limit on decompressed output (256 MiB, same as XZ).
const MAX_OUTPUT: usize = 256 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// A single coder (codec) in a folder.
struct Coder {
    /// Codec ID (e.g., CODEC_LZMA2).
    id: u64,
    /// Codec properties (e.g., LZMA properties bytes).
    props: Vec<u8>,
}

/// A folder: one compression unit containing one or more files.
struct Folder {
    /// Codec chain (for MVP: support single coder only).
    coders: Vec<Coder>,
    /// Uncompressed sizes for each output stream.
    unpack_sizes: Vec<u64>,
}

impl Folder {
    /// Total uncompressed size of this folder's output.
    fn total_unpack_size(&self) -> u64 {
        self.unpack_sizes.iter().sum()
    }
}

/// A file entry extracted from the archive.
pub struct SevenZEntry {
    /// File name (UTF-8, forward-slash separated).
    pub name: String,
    /// File data (empty for directories).
    pub data: Vec<u8>,
    /// Whether this entry is a directory.
    pub is_dir: bool,
}

// ---------------------------------------------------------------------------
// 7z VLI reader (different from XZ VLI)
// ---------------------------------------------------------------------------

/// Read a 7z variable-length integer.
///
/// 7z VLI encoding: the first byte's leading zero bit position determines
/// how many additional bytes follow.  If first byte is 0xFF, read 8 more
/// bytes as little-endian u64.
///
/// Examples:
/// - `0xxx xxxx` → 1 byte, value = byte & 0x7F
/// - `10xx xxxx yy` → 2 bytes, value = ((byte & 0x3F) << 8) | next
/// - `110x xxxx yy yy` → 3 bytes
/// - `1111 1111 yy×8` → 9 bytes, value = LE u64 from next 8
struct SevenZReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> SevenZReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_u8(&mut self) -> KernelResult<u8> {
        let b = *self.data.get(self.pos).ok_or(KernelError::CorruptedData)?;
        self.pos += 1;
        Ok(b)
    }

    fn read_u32_le(&mut self) -> KernelResult<u32> {
        if self.pos + 4 > self.data.len() {
            return Err(KernelError::CorruptedData);
        }
        let v = u32::from_le_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }

    fn read_u64_le(&mut self) -> KernelResult<u64> {
        if self.pos + 8 > self.data.len() {
            return Err(KernelError::CorruptedData);
        }
        let v = u64::from_le_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
            self.data[self.pos + 4],
            self.data[self.pos + 5],
            self.data[self.pos + 6],
            self.data[self.pos + 7],
        ]);
        self.pos += 8;
        Ok(v)
    }

    fn read_bytes(&mut self, n: usize) -> KernelResult<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(KernelError::CorruptedData)?;
        let slice = self.data.get(self.pos..end)
            .ok_or(KernelError::CorruptedData)?;
        self.pos = end;
        Ok(slice)
    }

    /// Read a 7z variable-length integer.
    fn read_vli(&mut self) -> KernelResult<u64> {
        let first = self.read_u8()?;
        let mut mask = 0x80u8;
        let mut val = 0u64;

        for i in 0u32..8 {
            if (first & mask) == 0 {
                // This bit is the terminating zero.
                val |= u64::from(first & (mask.wrapping_sub(1))) << (i.wrapping_mul(8));
                return Ok(val);
            }
            let next = self.read_u8()?;
            val |= u64::from(next) << (i.wrapping_mul(8));
            mask >>= 1;
        }

        // All 8 high bits set: first byte = 0xFF, read 8 more bytes.
        // But we already read them in the loop above. The value is already assembled.
        Ok(val)
    }

    /// Skip `n` bytes.
    fn skip(&mut self, n: usize) -> KernelResult<()> {
        let end = self.pos.checked_add(n).ok_or(KernelError::CorruptedData)?;
        if end > self.data.len() {
            return Err(KernelError::CorruptedData);
        }
        self.pos = end;
        Ok(())
    }

    /// Read a boolean vector: `num_items` bits, optionally all-true.
    fn read_bool_vector(&mut self, num_items: usize) -> KernelResult<Vec<bool>> {
        let all_defined = self.read_u8()?;
        if all_defined != 0 {
            return Ok(vec![true; num_items]);
        }
        let mut result = Vec::with_capacity(num_items);
        let mut byte = 0u8;
        let mut bit_pos = 0u8;
        for _ in 0..num_items {
            if bit_pos == 0 {
                byte = self.read_u8()?;
                bit_pos = 8;
            }
            bit_pos -= 1;
            result.push((byte >> bit_pos) & 1 != 0);
        }
        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// CRC-32 (7z uses ISO 3309 / ITU-T V.42 polynomial, same as gzip/ZIP)
// ---------------------------------------------------------------------------

fn crc32_7z(data: &[u8]) -> u32 {
    super::compress::crc32_iso_pub(data)
}

// ---------------------------------------------------------------------------
// Header parsing
// ---------------------------------------------------------------------------

/// Parsed archive header containing streams info and file entries.
struct ArchiveHeader {
    /// Pack stream offsets (relative to pack start).
    pack_sizes: Vec<u64>,
    /// Folders (codec info + unpack sizes).
    folders: Vec<Folder>,
    /// Sub-stream sizes within each folder (for solid archives).
    substream_sizes: Vec<Vec<u64>>,
    /// File entries (names, attributes).
    files: Vec<FileInfo>,
}

/// Raw file info from the header.
struct FileInfo {
    name: String,
    is_dir: bool,
    is_empty_stream: bool,
    size: u64,
}

fn parse_header(reader: &mut SevenZReader<'_>) -> KernelResult<ArchiveHeader> {
    let mut header = ArchiveHeader {
        pack_sizes: Vec::new(),
        folders: Vec::new(),
        substream_sizes: Vec::new(),
        files: Vec::new(),
    };

    loop {
        let prop_id = reader.read_u8()?;
        match prop_id {
            K_END => break,
            K_MAIN_STREAMS_INFO => {
                parse_streams_info(reader, &mut header)?;
            }
            K_FILES_INFO => {
                parse_files_info(reader, &mut header)?;
            }
            K_ARCHIVE_PROPERTIES => {
                // Skip archive properties.
                skip_property_data(reader)?;
            }
            _ => {
                // Unknown property: skip its data.
                skip_property_data(reader)?;
            }
        }
    }

    Ok(header)
}

fn skip_property_data(reader: &mut SevenZReader<'_>) -> KernelResult<()> {
    loop {
        let id = reader.read_u8()?;
        if id == K_END {
            return Ok(());
        }
        let size = reader.read_vli()? as usize;
        reader.skip(size)?;
    }
}

fn parse_streams_info(
    reader: &mut SevenZReader<'_>,
    header: &mut ArchiveHeader,
) -> KernelResult<()> {
    loop {
        let prop_id = reader.read_u8()?;
        match prop_id {
            K_END => break,
            K_PACK_INFO => parse_pack_info(reader, header)?,
            K_UNPACK_INFO => parse_unpack_info(reader, header)?,
            K_SUB_STREAMS_INFO => parse_substreams_info(reader, header)?,
            _ => {
                let size = reader.read_vli()? as usize;
                reader.skip(size)?;
            }
        }
    }
    Ok(())
}

fn parse_pack_info(
    reader: &mut SevenZReader<'_>,
    header: &mut ArchiveHeader,
) -> KernelResult<()> {
    let _pack_pos = reader.read_vli()?; // Offset of pack streams from signature end.
    let num_pack_streams = reader.read_vli()? as usize;

    loop {
        let prop_id = reader.read_u8()?;
        match prop_id {
            K_END => break,
            K_SIZE => {
                for _ in 0..num_pack_streams {
                    header.pack_sizes.push(reader.read_vli()?);
                }
            }
            K_CRC => {
                // Skip CRC data for pack streams.
                let _defined = reader.read_bool_vector(num_pack_streams)?;
                for i in 0..num_pack_streams {
                    if _defined.get(i).copied().unwrap_or(false) {
                        reader.skip(4)?; // CRC32
                    }
                }
            }
            _ => {
                let size = reader.read_vli()? as usize;
                reader.skip(size)?;
            }
        }
    }
    Ok(())
}

fn parse_unpack_info(
    reader: &mut SevenZReader<'_>,
    header: &mut ArchiveHeader,
) -> KernelResult<()> {
    loop {
        let prop_id = reader.read_u8()?;
        match prop_id {
            K_END => break,
            K_FOLDER => {
                let num_folders = reader.read_vli()? as usize;
                let external = reader.read_u8()?;
                if external != 0 {
                    return Err(KernelError::NotSupported); // External folders
                }
                for _ in 0..num_folders {
                    header.folders.push(parse_folder(reader)?);
                }
            }
            K_CODERS_UNPACK_SIZE => {
                // Read unpack sizes for each coder output in each folder.
                for folder in &mut header.folders {
                    let num_outputs = folder.coders.len().max(1);
                    folder.unpack_sizes.clear();
                    for _ in 0..num_outputs {
                        folder.unpack_sizes.push(reader.read_vli()?);
                    }
                }
            }
            K_CRC => {
                // Folder unpack CRCs.
                let num = header.folders.len();
                let defined = reader.read_bool_vector(num)?;
                for i in 0..num {
                    if defined.get(i).copied().unwrap_or(false) {
                        reader.skip(4)?; // CRC32
                    }
                }
            }
            _ => {
                let size = reader.read_vli()? as usize;
                reader.skip(size)?;
            }
        }
    }
    Ok(())
}

fn parse_folder(reader: &mut SevenZReader<'_>) -> KernelResult<Folder> {
    let num_coders = reader.read_vli()? as usize;
    if num_coders == 0 || num_coders > 4 {
        return Err(KernelError::CorruptedData);
    }

    let mut coders = Vec::with_capacity(num_coders);
    let mut total_in = 0usize;
    let mut total_out = 0usize;

    for _ in 0..num_coders {
        let flags = reader.read_u8()?;
        let id_size = (flags & 0x0F) as usize;
        let has_props = (flags & 0x20) != 0;
        let complex = (flags & 0x10) != 0;

        // Read codec ID.
        let id_bytes = reader.read_bytes(id_size)?;
        let mut id = 0u64;
        for &b in id_bytes {
            id = (id << 8) | u64::from(b);
        }

        if complex {
            let _num_in = reader.read_vli()?;
            let _num_out = reader.read_vli()?;
            total_in += _num_in as usize;
            total_out += _num_out as usize;
        } else {
            total_in += 1;
            total_out += 1;
        }

        let props = if has_props {
            let props_size = reader.read_vli()? as usize;
            reader.read_bytes(props_size)?.to_vec()
        } else {
            Vec::new()
        };

        coders.push(Coder { id, props });
    }

    // Bind pairs (for multi-coder chains).
    let num_bind_pairs = total_out.saturating_sub(1);
    for _ in 0..num_bind_pairs {
        let _in_idx = reader.read_vli()?;
        let _out_idx = reader.read_vli()?;
    }

    // Packed stream indices.
    let num_packed = total_in.saturating_sub(num_bind_pairs);
    if num_packed > 1 {
        for _ in 0..num_packed {
            let _idx = reader.read_vli()?;
        }
    }

    Ok(Folder {
        coders,
        unpack_sizes: Vec::new(),
    })
}

fn parse_substreams_info(
    reader: &mut SevenZReader<'_>,
    header: &mut ArchiveHeader,
) -> KernelResult<()> {
    let num_folders = header.folders.len();
    let mut num_unpack_streams_in_folder = vec![1u64; num_folders];

    loop {
        let prop_id = reader.read_u8()?;
        match prop_id {
            K_END => break,
            K_NUM_UNPACK_STREAM => {
                for i in 0..num_folders {
                    num_unpack_streams_in_folder[i] = reader.read_vli()?;
                }
            }
            K_SIZE => {
                // Read sub-stream sizes for each folder.
                header.substream_sizes.clear();
                for fi in 0..num_folders {
                    let num_ss = num_unpack_streams_in_folder[fi] as usize;
                    let mut sizes = Vec::with_capacity(num_ss);
                    let mut total = 0u64;
                    // Read N-1 sizes; the last is implied.
                    for _ in 0..num_ss.saturating_sub(1) {
                        let s = reader.read_vli()?;
                        sizes.push(s);
                        total = total.wrapping_add(s);
                    }
                    let folder_total = header.folders.get(fi)
                        .map(|f| f.total_unpack_size())
                        .unwrap_or(0);
                    sizes.push(folder_total.saturating_sub(total));
                    header.substream_sizes.push(sizes);
                }
            }
            K_CRC => {
                // Sub-stream CRCs.
                let total_ss: usize = num_unpack_streams_in_folder.iter()
                    .map(|&n| n as usize)
                    .sum();
                let defined = reader.read_bool_vector(total_ss)?;
                for i in 0..total_ss {
                    if defined.get(i).copied().unwrap_or(false) {
                        reader.skip(4)?;
                    }
                }
            }
            _ => {
                let size = reader.read_vli()? as usize;
                reader.skip(size)?;
            }
        }
    }

    // If no sub-stream sizes were given, each folder has one stream = full unpack size.
    if header.substream_sizes.is_empty() {
        for (fi, &n) in num_unpack_streams_in_folder.iter().enumerate() {
            if n == 1 {
                let total = header.folders.get(fi)
                    .map(|f| f.total_unpack_size())
                    .unwrap_or(0);
                header.substream_sizes.push(vec![total]);
            }
        }
    }

    Ok(())
}

fn parse_files_info(
    reader: &mut SevenZReader<'_>,
    header: &mut ArchiveHeader,
) -> KernelResult<()> {
    let num_files = reader.read_vli()? as usize;
    header.files.clear();
    header.files.resize_with(num_files, || FileInfo {
        name: String::new(),
        is_dir: false,
        is_empty_stream: false,
        size: 0,
    });

    let mut empty_streams = vec![false; num_files];

    loop {
        let prop_id = reader.read_u8()?;
        if prop_id == K_END {
            break;
        }

        let prop_size = reader.read_vli()? as usize;
        let start_pos = reader.pos;

        match prop_id {
            K_NAME => {
                let external = reader.read_u8()?;
                if external != 0 {
                    // Skip external reference.
                    reader.pos = start_pos + prop_size;
                    continue;
                }
                // Names are UTF-16LE, null-terminated.
                for fi in 0..num_files {
                    let mut name_u16 = Vec::new();
                    loop {
                        if reader.pos + 2 > reader.data.len() {
                            break;
                        }
                        let lo = reader.data[reader.pos];
                        let hi = reader.data[reader.pos + 1];
                        reader.pos += 2;
                        let ch = u16::from_le_bytes([lo, hi]);
                        if ch == 0 {
                            break;
                        }
                        name_u16.push(ch);
                    }
                    // Convert UTF-16 to UTF-8.
                    let name = String::from_utf16_lossy(&name_u16);
                    // Normalize path separators.
                    let name = name.replace('\\', "/");
                    if let Some(f) = header.files.get_mut(fi) {
                        f.name = name;
                    }
                }
            }
            K_EMPTY_STREAM => {
                // Bit vector: which files are empty streams (dirs or empty files).
                let mut byte = 0u8;
                let mut bit_pos = 0u8;
                for i in 0..num_files {
                    if bit_pos == 0 {
                        byte = reader.read_u8()?;
                        bit_pos = 8;
                    }
                    bit_pos -= 1;
                    let is_empty = (byte >> bit_pos) & 1 != 0;
                    empty_streams[i] = is_empty;
                    if let Some(f) = header.files.get_mut(i) {
                        f.is_empty_stream = is_empty;
                    }
                }
            }
            K_EMPTY_FILE => {
                // Skip empty file bit vector (within empty streams).
                reader.pos = start_pos + prop_size;
            }
            K_ANTI => {
                reader.pos = start_pos + prop_size;
            }
            K_WIN_ATTRIBUTES => {
                let _defined = reader.read_bool_vector(num_files)?;
                let external = reader.read_u8()?;
                if external != 0 {
                    reader.pos = start_pos + prop_size;
                    continue;
                }
                for i in 0..num_files {
                    if _defined.get(i).copied().unwrap_or(false) {
                        let attrs = reader.read_u32_le()?;
                        if let Some(f) = header.files.get_mut(i) {
                            // Windows directory attribute = 0x10.
                            f.is_dir = (attrs & 0x10) != 0;
                        }
                    }
                }
            }
            _ => {
                // Skip unknown property.
                reader.pos = start_pos + prop_size;
            }
        }

        // Ensure we consumed exactly prop_size bytes.
        if reader.pos < start_pos + prop_size {
            reader.pos = start_pos + prop_size;
        }
    }

    // Mark empty-stream files without WIN_ATTRIBUTES as dirs if name ends with '/'.
    for f in &mut header.files {
        if f.is_empty_stream && !f.is_dir && f.name.ends_with('/') {
            f.is_dir = true;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Codec dispatch
// ---------------------------------------------------------------------------

/// Decompress a folder's pack stream using its codec.
///
/// Only supports single-coder folders.  Multi-coder chains (e.g.,
/// BCJ + LZMA2) return `NotSupported`.
fn decompress_folder(
    folder: &Folder,
    packed_data: &[u8],
    unpack_size: u64,
) -> KernelResult<Vec<u8>> {
    if folder.coders.is_empty() {
        return Err(KernelError::CorruptedData);
    }

    // For MVP: only support single-coder folders.
    if folder.coders.len() > 1 {
        return Err(KernelError::NotSupported);
    }

    let coder = &folder.coders[0];
    let out_size = unpack_size as usize;

    if out_size > MAX_OUTPUT {
        return Err(KernelError::OutOfMemory);
    }

    match coder.id {
        CODEC_COPY => {
            // Stored data: just copy.
            if packed_data.len() < out_size {
                return Err(KernelError::CorruptedData);
            }
            Ok(packed_data[..out_size].to_vec())
        }
        CODEC_LZMA => {
            // LZMA: properties are 5 bytes (lc/lp/pb byte + 4-byte dict size LE).
            if coder.props.len() < 5 {
                return Err(KernelError::CorruptedData);
            }
            let props_byte = coder.props[0];
            let lc = props_byte % 9;
            let remainder = props_byte / 9;
            let lp = remainder % 5;
            let pb = remainder / 5;
            let dict_size = u32::from_le_bytes([
                coder.props[1], coder.props[2],
                coder.props[3], coder.props[4],
            ]).max(4096);

            let mut lzma = super::xz::LzmaState::new(lc, lp, pb);
            let mut output = Vec::new();
            super::xz::lzma_decode(
                &mut lzma, packed_data, out_size, &mut output, dict_size,
            )?;
            Ok(output)
        }
        CODEC_LZMA2 => {
            // LZMA2: properties is 1 byte (dict size byte).
            if coder.props.is_empty() {
                return Err(KernelError::CorruptedData);
            }
            let dict_size = super::xz::lzma2_dict_size(coder.props[0]).max(4096);
            let output = super::xz::lzma2_decode(packed_data, dict_size)?;
            Ok(output)
        }
        CODEC_DEFLATE => {
            // Raw DEFLATE stream.
            let output = super::compress::inflate(packed_data)?;
            Ok(output)
        }
        CODEC_BZIP2 => {
            // BZip2 stream.
            let output = super::bzip2::bunzip2(packed_data)?;
            Ok(output)
        }
        _ => {
            Err(KernelError::NotSupported)
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Extract all files from a 7z archive.
///
/// Returns a list of file entries (name + data).  Directories are included
/// with empty data and `is_dir = true`.
pub fn un7z(data: &[u8]) -> KernelResult<Vec<SevenZEntry>> {
    // --- Parse signature header (12 bytes minimum) ---
    if data.len() < 32 {
        return Err(KernelError::CorruptedData);
    }

    // Check magic.
    if data.get(..6) != Some(&SEVENZ_MAGIC[..]) {
        return Err(KernelError::CorruptedData);
    }

    // Version (bytes 6-7): we accept any version.
    // Start header CRC (bytes 8-11): CRC of bytes 12-31.
    let start_header_crc_stored = u32::from_le_bytes([
        data[8], data[9], data[10], data[11],
    ]);
    let start_header_crc_computed = crc32_7z(&data[12..32]);
    if start_header_crc_stored != start_header_crc_computed {
        return Err(KernelError::CorruptedData);
    }

    // Start header (bytes 12-31):
    // - next_header_offset (u64 LE): offset from end of signature header (byte 32)
    // - next_header_size (u64 LE)
    // - next_header_crc (u32 LE)
    let next_header_offset = u64::from_le_bytes([
        data[12], data[13], data[14], data[15],
        data[16], data[17], data[18], data[19],
    ]);
    let next_header_size = u64::from_le_bytes([
        data[20], data[21], data[22], data[23],
        data[24], data[25], data[26], data[27],
    ]);
    let _next_header_crc = u32::from_le_bytes([
        data[28], data[29], data[30], data[31],
    ]);

    // The main header starts at (32 + next_header_offset).
    let header_start = 32u64.wrapping_add(next_header_offset) as usize;
    let header_end = header_start.wrapping_add(next_header_size as usize);
    let header_data = data.get(header_start..header_end)
        .ok_or(KernelError::CorruptedData)?;

    // The pack data starts right after the signature header (byte 32).
    let pack_start = 32usize;

    // Parse the header (may need to decompress an encoded header first).
    let mut reader = SevenZReader::new(header_data);
    let first_id = reader.read_u8()?;

    let header = if first_id == K_HEADER {
        // Plain header.
        parse_header(&mut reader)?
    } else if first_id == K_ENCODED_HEADER {
        // The header itself is compressed.  Parse its streams info,
        // decompress it, then parse the decompressed header.
        let mut enc_header = ArchiveHeader {
            pack_sizes: Vec::new(),
            folders: Vec::new(),
            substream_sizes: Vec::new(),
            files: Vec::new(),
        };
        parse_streams_info(&mut reader, &mut enc_header)?;

        // Decompress the encoded header.
        if enc_header.folders.is_empty() {
            return Err(KernelError::CorruptedData);
        }

        // The encoded header's pack data starts at pack_start (byte 32).
        let pack_size = enc_header.pack_sizes.first().copied().unwrap_or(0) as usize;
        let pack_data = data.get(pack_start..pack_start + pack_size)
            .ok_or(KernelError::CorruptedData)?;
        let unpack_size = enc_header.folders[0].total_unpack_size();
        let decompressed = decompress_folder(&enc_header.folders[0], pack_data, unpack_size)?;

        // Now parse the decompressed header.
        let mut inner_reader = SevenZReader::new(&decompressed);
        let inner_id = inner_reader.read_u8()?;
        if inner_id != K_HEADER {
            return Err(KernelError::CorruptedData);
        }
        parse_header(&mut inner_reader)?
    } else {
        return Err(KernelError::CorruptedData);
    };

    // --- Extract files ---
    let mut entries = Vec::new();

    // Build the mapping: file index → (folder index, sub-stream index).
    // Non-empty-stream files are assigned to folders in order.
    let mut file_to_stream: Vec<Option<(usize, usize)>> = vec![None; header.files.len()];
    let mut folder_idx = 0usize;
    let mut substream_idx = 0usize;

    for (fi, finfo) in header.files.iter().enumerate() {
        if finfo.is_empty_stream {
            continue;
        }
        if folder_idx >= header.folders.len() {
            break; // More files than folders can serve
        }
        let folder_ss = header.substream_sizes.get(folder_idx)
            .map(|v| v.len())
            .unwrap_or(1);
        file_to_stream[fi] = Some((folder_idx, substream_idx));
        substream_idx += 1;
        if substream_idx >= folder_ss {
            folder_idx += 1;
            substream_idx = 0;
        }
    }

    // Decompress each folder and split into sub-streams.
    let mut folder_outputs: Vec<Option<Vec<u8>>> = vec![None; header.folders.len()];

    for (fi, finfo) in header.files.iter().enumerate() {
        if finfo.is_dir || finfo.is_empty_stream {
            entries.push(SevenZEntry {
                name: finfo.name.clone(),
                data: Vec::new(),
                is_dir: finfo.is_dir || finfo.name.ends_with('/'),
            });
            continue;
        }

        let (fold_idx, ss_idx) = match file_to_stream[fi] {
            Some(v) => v,
            None => {
                // File has no stream mapping; treat as empty.
                entries.push(SevenZEntry {
                    name: finfo.name.clone(),
                    data: Vec::new(),
                    is_dir: false,
                });
                continue;
            }
        };

        // Ensure this folder is decompressed.
        if folder_outputs.get(fold_idx).and_then(|o| o.as_ref()).is_none() {
            // Calculate pack data offset for this folder.
            let mut pack_offset = pack_start;
            for i in 0..fold_idx {
                pack_offset += header.pack_sizes.get(i).copied().unwrap_or(0) as usize;
            }
            let pack_size = header.pack_sizes.get(fold_idx).copied().unwrap_or(0) as usize;
            let pack_data = data.get(pack_offset..pack_offset + pack_size)
                .ok_or(KernelError::CorruptedData)?;
            let unpack_size = header.folders.get(fold_idx)
                .map(|f| f.total_unpack_size())
                .unwrap_or(0);
            let decompressed = decompress_folder(
                header.folders.get(fold_idx).ok_or(KernelError::CorruptedData)?,
                pack_data,
                unpack_size,
            )?;
            if let Some(slot) = folder_outputs.get_mut(fold_idx) {
                *slot = Some(decompressed);
            }
        }

        // Extract this sub-stream from the folder output.
        let folder_data = folder_outputs.get(fold_idx)
            .and_then(|o| o.as_ref())
            .ok_or(KernelError::CorruptedData)?;

        let ss_sizes = header.substream_sizes.get(fold_idx);
        let mut offset = 0usize;
        if let Some(sizes) = ss_sizes {
            for i in 0..ss_idx {
                offset += sizes.get(i).copied().unwrap_or(0) as usize;
            }
        }
        let ss_size = ss_sizes
            .and_then(|s| s.get(ss_idx))
            .copied()
            .unwrap_or(folder_data.len().saturating_sub(offset) as u64) as usize;

        let file_data = folder_data.get(offset..offset + ss_size)
            .unwrap_or(&[])
            .to_vec();

        entries.push(SevenZEntry {
            name: finfo.name.clone(),
            data: file_data,
            is_dir: false,
        });
    }

    Ok(entries)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for 7z parsing primitives.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[7z] Running self-test...");

    // Test 1: VLI reader.
    test_vli()?;

    // Test 2: Magic detection.
    test_magic()?;

    crate::serial_println!("[7z] Self-test passed.");
    Ok(())
}

fn test_vli() -> KernelResult<()> {
    // Single byte: 0x42 → 0x42.
    let data = [0x42u8];
    let mut reader = SevenZReader::new(&data);
    let val = reader.read_vli()?;
    if val != 0x42 {
        crate::serial_println!("[7z]   FAIL: VLI(0x42) = {}", val);
        return Err(KernelError::InternalError);
    }

    // Two bytes: 0x80 0x03 → 0x03 (first byte = 1000_0000, 0 data bits,
    // next byte = value).  Actually: leading 1 bit, so we read 1 more byte.
    // val = (first & 0x3F=0x00) << 8 | 0x03 = 3.  Wait, that's wrong.
    // Let me re-check the 7z VLI:
    // first=0x80: bit 7 set → read 1 more byte.
    // val = next_byte | (first & (0x80-1=0x7F but minus mask...))
    // Actually the logic in read_vli:
    // i=0: mask=0x80, (0x80 & 0x80) != 0, so read next byte (0x03).
    //   val |= 0x03 << 0 = 3. mask >>= 1 → 0x40.
    // i=1: mask=0x40, (0x80 & 0x40) == 0, so terminate.
    //   val |= (0x80 & (0x40-1=0x3F)) << 8 = (0x00) << 8 = 0.
    //   result = 3.
    let data2 = [0x80u8, 0x03];
    let mut reader2 = SevenZReader::new(&data2);
    let val2 = reader2.read_vli()?;
    if val2 != 3 {
        crate::serial_println!("[7z]   FAIL: VLI(0x80,0x03) = {}", val2);
        return Err(KernelError::InternalError);
    }

    // Zero: 0x00 → 0.
    let data3 = [0x00u8];
    let mut reader3 = SevenZReader::new(&data3);
    let val3 = reader3.read_vli()?;
    if val3 != 0 {
        crate::serial_println!("[7z]   FAIL: VLI(0x00) = {}", val3);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[7z]   VLI parsing OK");
    Ok(())
}

fn test_magic() -> KernelResult<()> {
    // Valid magic.
    let valid = [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C];
    if valid != SEVENZ_MAGIC {
        crate::serial_println!("[7z]   FAIL: magic mismatch");
        return Err(KernelError::InternalError);
    }

    // un7z should reject too-short data.
    if un7z(&[0x37, 0x7A]).is_ok() {
        crate::serial_println!("[7z]   FAIL: should reject short data");
        return Err(KernelError::InternalError);
    }

    // un7z should reject bad magic.
    let bad = [0u8; 32];
    if un7z(&bad).is_ok() {
        crate::serial_println!("[7z]   FAIL: should reject bad magic");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[7z]   magic validation OK");
    Ok(())
}
