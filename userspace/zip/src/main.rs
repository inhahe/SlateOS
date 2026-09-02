//! Slate OS zip/unzip archive utility.
//!
//! Multi-personality binary: detects operating mode from `argv[0]`.
//!
//! # Modes
//!
//! - **zip**: create or update ZIP archives
//! - **unzip**: extract files from ZIP archives
//!
//! # Format
//!
//! Implements the classic ZIP format (PKWARE Application Note):
//! - Local file headers (signature 0x04034b50)
//! - Central directory entries (signature 0x02014b50)
//! - End of central directory record (signature 0x06054b50)
//! - Compression methods: Stored (0) and DEFLATE (8)
//! - CRC32 checksums with standard polynomial 0xEDB88320, from the `crc32`
//!   crate (the reflected-IEEE one PKZIP specifies, not CRC32C)
//! - Compression methods: Stored (0) and DEFLATE (8)
//!
//! # Where DEFLATE lives
//!
//! Decompression is [`deflate::inflate_limited`]. It is not implemented here,
//! and the `_limited` half is the reason it is that call and not `inflate`:
//! an entry is decoded under a ceiling equal to the size its central
//! directory declares, so an archive that lies about how far it expands is
//! refused at the byte that exceeds the claim rather than after the expansion
//! has been allocated.
//!
//! Compression is [`deflate::deflate_level`], which takes the `-0`..`-9` level
//! this tool accepts. It used to be a local encoder for exactly that reason —
//! the crate's `deflate()` fixes the level and there was nothing to call — and
//! `deflate_level` is what closed the gap.

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
//
// zip parses the ZIP container format and implements DEFLATE / CRC32:
// arithmetic on file offsets, code lengths, Huffman table indices, and
// CRC accumulators is bounded by container header limits and
// length checks immediately preceding any slice/index. Decompression
// errors surface as Err rather than panic.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use crc32::crc32;
use quoting::{quoteaf_os, quotef_os};
use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::SystemTime;

// Note: all I/O goes through std (std::fs / std::io / std::process), which
// reaches native Slate OS syscalls via the posix libc layer.  A previous
// hand-rolled syscall stub here hardcoded Linux numbers (WRITE=1=SYS_EXIT,
// OPEN=2=SYS_TASK_ID, EXIT=60=SYS_SYSCTL_GET, ...) that collide with
// unrelated native syscalls; it was dead code and has been removed.

// ============================================================================
// DOS date/time encoding (ZIP uses MS-DOS date/time fields)
// ============================================================================

/// Encode a `SystemTime` into a DOS date (high 16 bits) and DOS time (low 16 bits).
///
/// DOS date: bits 15-9 = year-1980, bits 8-5 = month (1-12), bits 4-0 = day (1-31)
/// DOS time: bits 15-11 = hours (0-23), bits 10-5 = minutes (0-59), bits 4-0 = seconds/2
fn encode_dos_datetime(t: SystemTime) -> (u16, u16) {
    // Fall back to a fixed timestamp on error: 1980-01-01 00:00:00
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    // Convert Unix epoch seconds to a rough calendar date.
    // We use a simple Gregorian calendar calculation (no leap-second awareness).
    let (year, month, day, hour, minute, second) = unix_secs_to_datetime(secs);

    let dos_year = year.saturating_sub(1980).min(127) as u16;
    let dos_date = (dos_year << 9) | ((month as u16) << 5) | (day as u16);
    let dos_time = ((hour as u16) << 11) | ((minute as u16) << 5) | ((second / 2) as u16);
    (dos_date, dos_time)
}

/// Convert Unix epoch seconds to (year, month, day, hour, minute, second).
fn unix_secs_to_datetime(secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let second = (secs % 60) as u32;
    let minutes_total = secs / 60;
    let minute = (minutes_total % 60) as u32;
    let hours_total = minutes_total / 60;
    let hour = (hours_total % 24) as u32;
    let days_total = hours_total / 24;

    // Gregorian calendar from days since 1970-01-01
    // Using algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days_total as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if m <= 2 { y + 1 } else { y };

    (yr as u32, m as u32, d as u32, hour, minute, second)
}

// ============================================================================
// Constants
// ============================================================================

/// Local file header signature.
const SIG_LOCAL: u32 = 0x0403_4B50;
/// Central directory file header signature.
const SIG_CENTRAL: u32 = 0x0201_4B50;
/// End of central directory signature.
const SIG_EOCD: u32 = 0x0605_4B50;

/// Compression method: stored (no compression).
const METHOD_STORED: u16 = 0;
/// Compression method: deflated.
const METHOD_DEFLATE: u16 = 8;

/// Version needed to extract: 2.0 (for DEFLATE).
const VERSION_NEEDED_DEFLATE: u16 = 20;
/// Version needed to extract: 1.0 (for stored).
const VERSION_NEEDED_STORED: u16 = 10;
/// Version made by: Unix/MS-DOS compatible at spec 2.0.
const VERSION_MADE_BY: u16 = 0x0314; // 3 = Unix, 20 = version 2.0

/// General purpose bit flag: data descriptor present.
#[allow(dead_code)]
const GP_DATA_DESCRIPTOR: u16 = 1 << 3;

// ============================================================================
// DEFLATE: see the `deflate` crate
// ============================================================================
//
// Neither half of RFC 1951 lives here any more. Both halves came out for the
// same underlying reason -- this file was the tree's third independent
// implementation of the format -- but they came out at different times and for
// different immediate causes, so both are recorded.
//
// The decompressor went first, per
// requests/a-b-userspace-zip-carries-a-third-deflate-and-a-second-zip-parser.md.
// It was a `BitReader`, a canonical-Huffman decoder, and `deflate_decompress` /
// `decode_huffman_block` / `decode_dynamic_headers` -- about 350 lines, now
// `deflate::inflate_limited`.
//
//
// This file used to carry its own: a `BitReader`, a canonical-Huffman decoder,
// and `deflate_decompress` / `decode_huffman_block` / `decode_dynamic_headers`
// -- about 350 lines, and the tree's third independent implementation of
// RFC 1951. It is now `deflate::inflate_limited`, per
// requests/a-b-userspace-zip-carries-a-third-deflate-and-a-second-zip-parser.md.
//
// The deletion fixed a live bug rather than merely removing duplication, which
// is the part worth recording. The decoder that was here grew its output
// `Vec` with no ceiling, and the only thing that would have noticed a stream
// expanding beyond the size the archive declared was the length comparison in
// `zip_extract_entry` -- which runs *after* decompression returns. So a
// 40 KiB entry claiming to hold 300 bytes was decompressed in full, all of it
// resident, and only then rejected: the check was real but it was on the far
// side of the allocation it was supposed to prevent. Lane A found the same
// hole in the kernel's copy while promoting it to `ziparchive`, which is how
// we knew to look here.
//
// `inflate_limited` takes the cap as a parameter and refuses at the byte that
// would exceed it, so the entry's declared `uncompressed_size` -- a number we
// have from the central directory before decoding starts -- is now an
// enforced ceiling instead of an after-the-fact assertion.
//
// The compressor followed once `deflate::deflate_level` landed, per
// requests/a-b-deflate-level-has-landed-and-your-local-compressor-was-the-better-one.md.
// The `-0`..`-9` levels this tool accepts were the only reason the local
// `deflate_compress` outlived the decompressor: the crate's `deflate()` takes no
// level, so at that point there was simply nothing to call. `deflate_level(data,
// level)` closed that, and the local LZ77 + fixed-Huffman encoder -- about 380
// lines carrying its own length/distance tables, a `BitWriter`, and a hash-chain
// match finder -- went with it.
//
// This is not a byte-for-byte swap, and it is worth knowing why before anyone
// diffs two archives. The local encoder emitted fixed-Huffman blocks and
// nothing else; the crate tries both fixed and dynamic Huffman and keeps
// whichever is smaller, so the same input at the same level generally
// compresses further and never to the identical bytes. What is preserved is
// the only property that matters at a container boundary: every level still
// emits a valid DEFLATE stream that any unzip decodes. The tests below assert
// that, and that compressible input gets smaller -- not a byte count, which is
// an encoder detail and was never ours to pin.
// ============================================================================

// ============================================================================
// ZIP archive structures
// ============================================================================

/// A single file entry in a ZIP archive.
#[derive(Debug, Clone)]
struct ZipEntry {
    /// File name (path within the archive, forward-slash separated).
    name: String,
    /// Compression method.
    method: u16,
    /// DOS modification date.
    mod_date: u16,
    /// DOS modification time.
    mod_time: u16,
    /// CRC32 of uncompressed data.
    crc32: u32,
    /// Compressed size in bytes.
    compressed_size: u32,
    /// Uncompressed size in bytes.
    uncompressed_size: u32,
    /// Offset of the local file header from the start of the archive.
    local_header_offset: u32,
    /// File comment (usually empty).
    comment: String,
    /// External file attributes (Unix permissions in high 16 bits).
    external_attrs: u32,
    /// Internal file attributes.
    internal_attrs: u16,
}

// ============================================================================
// ZIP reader
// ============================================================================

/// Parse all central directory entries from a ZIP archive byte slice.
fn zip_read_central_directory(data: &[u8]) -> Result<Vec<ZipEntry>, String> {
    let eocd_offset = find_eocd(data)?;
    let eocd = &data[eocd_offset..];

    if eocd.len() < 22 {
        return Err("zip: EOCD too short".to_string());
    }

    let cd_count = u16::from_le_bytes([eocd[8], eocd[9]]) as usize;
    let cd_size = u32::from_le_bytes([eocd[12], eocd[13], eocd[14], eocd[15]]) as usize;
    let cd_offset = u32::from_le_bytes([eocd[16], eocd[17], eocd[18], eocd[19]]) as usize;

    if cd_offset + cd_size > data.len() {
        return Err(format!(
            "zip: central directory at offset {cd_offset} + size {cd_size} exceeds file length {}",
            data.len()
        ));
    }

    let mut entries = Vec::with_capacity(cd_count);
    let mut pos = cd_offset;

    for entry_idx in 0..cd_count {
        if pos + 46 > data.len() {
            return Err(format!(
                "zip: central directory entry {entry_idx} truncated"
            ));
        }
        let sig = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        if sig != SIG_CENTRAL {
            return Err(format!(
                "zip: expected central dir signature at {pos:#x}, got {sig:#010x}"
            ));
        }

        let method = u16::from_le_bytes([data[pos + 10], data[pos + 11]]);
        let mod_time = u16::from_le_bytes([data[pos + 12], data[pos + 13]]);
        let mod_date = u16::from_le_bytes([data[pos + 14], data[pos + 15]]);
        let entry_crc = u32::from_le_bytes([
            data[pos + 16],
            data[pos + 17],
            data[pos + 18],
            data[pos + 19],
        ]);
        let comp_size = u32::from_le_bytes([
            data[pos + 20],
            data[pos + 21],
            data[pos + 22],
            data[pos + 23],
        ]);
        let uncomp_size = u32::from_le_bytes([
            data[pos + 24],
            data[pos + 25],
            data[pos + 26],
            data[pos + 27],
        ]);
        let fname_len = u16::from_le_bytes([data[pos + 28], data[pos + 29]]) as usize;
        let extra_len = u16::from_le_bytes([data[pos + 30], data[pos + 31]]) as usize;
        let comment_len = u16::from_le_bytes([data[pos + 32], data[pos + 33]]) as usize;
        let internal_attrs = u16::from_le_bytes([data[pos + 36], data[pos + 37]]);
        let external_attrs = u32::from_le_bytes([
            data[pos + 38],
            data[pos + 39],
            data[pos + 40],
            data[pos + 41],
        ]);
        let lhdr_offset = u32::from_le_bytes([
            data[pos + 42],
            data[pos + 43],
            data[pos + 44],
            data[pos + 45],
        ]);

        let name_start = pos + 46;
        let name_end = name_start + fname_len;
        let comment_start = name_end + extra_len;
        let comment_end = comment_start + comment_len;

        if comment_end > data.len() {
            return Err(format!(
                "zip: entry {entry_idx} name/comment extends beyond data"
            ));
        }

        let name = String::from_utf8_lossy(&data[name_start..name_end]).into_owned();
        let comment = String::from_utf8_lossy(&data[comment_start..comment_end]).into_owned();

        entries.push(ZipEntry {
            name,
            method,
            mod_date,
            mod_time,
            crc32: entry_crc,
            compressed_size: comp_size,
            uncompressed_size: uncomp_size,
            local_header_offset: lhdr_offset,
            comment,
            external_attrs,
            internal_attrs,
        });

        pos = comment_end;
    }

    Ok(entries)
}

/// Locate the End of Central Directory record by scanning backwards from the end.
fn find_eocd(data: &[u8]) -> Result<usize, String> {
    if data.len() < 22 {
        return Err("zip: file too small to contain EOCD".to_string());
    }
    // EOCD has a variable-length comment (up to 65535 bytes) at the end.
    let search_start = data.len().saturating_sub(22 + 65535);
    let search_end = data.len() - 22;

    // Scan backwards (EOCD is usually near the end).
    let mut i = search_end;
    loop {
        if data[i] == 0x50 && data[i + 1] == 0x4B && data[i + 2] == 0x05 && data[i + 3] == 0x06 {
            // Verify the comment length matches.
            let comment_len = u16::from_le_bytes([data[i + 20], data[i + 21]]) as usize;
            if i + 22 + comment_len == data.len() {
                return Ok(i);
            }
        }
        if i == search_start {
            break;
        }
        i -= 1;
    }
    Err("zip: EOCD record not found".to_string())
}

/// Extract the compressed data for a given entry from the archive.
///
/// Returns `(compressed_bytes, actual_crc, actual_comp_size, actual_uncomp_size)`.
/// The last three are from the local header (which may be more up-to-date for
/// entries written with data descriptors).
fn zip_read_local_data<'a>(data: &'a [u8], entry: &ZipEntry) -> Result<&'a [u8], String> {
    let offset = entry.local_header_offset as usize;
    if offset + 30 > data.len() {
        return Err(format!(
            "zip: local header for '{}' at offset {offset:#x} exceeds file",
            entry.name
        ));
    }

    let sig = u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]);
    if sig != SIG_LOCAL {
        return Err(format!(
            "zip: expected local file header for '{}' at {offset:#x}, got {sig:#010x}",
            entry.name
        ));
    }

    let fname_len = u16::from_le_bytes([data[offset + 26], data[offset + 27]]) as usize;
    let extra_len = u16::from_le_bytes([data[offset + 28], data[offset + 29]]) as usize;
    let data_start = offset + 30 + fname_len + extra_len;
    let comp_size = entry.compressed_size as usize;

    if data_start + comp_size > data.len() {
        return Err(format!(
            "zip: compressed data for '{}' at {data_start:#x}+{comp_size} exceeds file",
            entry.name
        ));
    }

    Ok(&data[data_start..data_start + comp_size])
}

/// Decompress a single ZIP entry and verify its CRC32.
fn zip_extract_entry(data: &[u8], entry: &ZipEntry) -> Result<Vec<u8>, String> {
    let compressed = zip_read_local_data(data, entry)?;

    let declared = entry.uncompressed_size as usize;

    let output = match entry.method {
        // Stored entries cannot expand: the bytes are already the output, and
        // `zip_read_local_data` has bounded that slice by the file's own size.
        METHOD_STORED => compressed.to_vec(),
        METHOD_DEFLATE => {
            // The cap is the size the central directory declares, so an entry
            // that decompresses to more than it claims is refused *at* the
            // byte that exceeds it rather than after the whole expansion is
            // resident. This is the only check here that runs before the
            // memory is committed -- the length comparison below and the CRC
            // after it are both true statements made too late to matter.
            deflate::inflate_limited(compressed, declared).map_err(|e| match e {
                // Worth its own wording. The crate's message is "decompressed
                // size exceeds the caller's limit", which describes a limit
                // the user did not set and cannot see; what actually happened
                // is that the archive contradicted itself.
                deflate::Error::OutputTooLarge => format!(
                    "zip: '{}': declares {declared} byte(s) but decompresses to more; \
                     refusing to expand it",
                    entry.name
                ),
                other => format!("zip: '{}': {other}", entry.name),
            })?
        }
        other => {
            return Err(format!(
                "zip: '{}': unsupported compression method {other}",
                entry.name
            ));
        }
    };

    // The over-long direction is caught above, mid-decode. This still catches
    // the other one -- an entry decompressing to *fewer* bytes than it
    // declares -- which no output cap can see.
    if output.len() != declared {
        return Err(format!(
            "zip: '{}': size mismatch: expected {}, got {}",
            entry.name,
            declared,
            output.len()
        ));
    }

    // Verify CRC32.
    let computed = crc32(&output);
    if computed != entry.crc32 {
        return Err(format!(
            "zip: '{}': CRC32 mismatch: expected {:#010x}, got {:#010x}",
            entry.name, entry.crc32, computed
        ));
    }

    Ok(output)
}

// ============================================================================
// ZIP writer
// ============================================================================

/// Builds a ZIP archive in memory.
struct ZipWriter {
    buf: Vec<u8>,
    entries: Vec<ZipEntry>,
}

impl ZipWriter {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            entries: Vec::new(),
        }
    }

    /// Add a file to the archive.
    ///
    /// `name` is the stored path (forward slashes, no leading slash).
    /// `data` is the raw (uncompressed) file contents.
    /// `level` is the compression level (0 = stored, 1-9 = deflate).
    /// `mod_date` and `mod_time` are DOS-encoded date/time.
    ///
    /// Infallible, like [`ZipWriter::add_directory`]. It used to return
    /// `Result` because the local encoder it called could report a bad level;
    /// `deflate::deflate_level` clamps instead, so there is no longer an error
    /// to return and callers should not have to pretend otherwise.
    fn add_file(&mut self, name: &str, data: &[u8], level: u8, mod_date: u16, mod_time: u16) {
        let (method, compressed) = if level == 0 {
            (METHOD_STORED, data.to_vec())
        } else {
            let comp = deflate::deflate_level(data, level);
            // Only use deflate if it actually shrinks the data.
            if comp.len() < data.len() {
                (METHOD_DEFLATE, comp)
            } else {
                (METHOD_STORED, data.to_vec())
            }
        };

        let file_crc = crc32(data);
        let local_offset = self.buf.len() as u32;
        let version_needed = if method == METHOD_DEFLATE {
            VERSION_NEEDED_DEFLATE
        } else {
            VERSION_NEEDED_STORED
        };

        // Local file header.
        let name_bytes = name.as_bytes();
        write_u32_le(&mut self.buf, SIG_LOCAL);
        write_u16_le(&mut self.buf, version_needed);
        write_u16_le(&mut self.buf, 0); // general purpose bit flag
        write_u16_le(&mut self.buf, method);
        write_u16_le(&mut self.buf, mod_time);
        write_u16_le(&mut self.buf, mod_date);
        write_u32_le(&mut self.buf, file_crc);
        write_u32_le(&mut self.buf, compressed.len() as u32);
        write_u32_le(&mut self.buf, data.len() as u32);
        write_u16_le(&mut self.buf, name_bytes.len() as u16);
        write_u16_le(&mut self.buf, 0); // extra field length
        self.buf.extend_from_slice(name_bytes);

        // File data.
        self.buf.extend_from_slice(&compressed);

        self.entries.push(ZipEntry {
            name: name.to_string(),
            method,
            mod_date,
            mod_time,
            crc32: file_crc,
            compressed_size: compressed.len() as u32,
            uncompressed_size: data.len() as u32,
            local_header_offset: local_offset,
            comment: String::new(),
            external_attrs: 0,
            internal_attrs: 0,
        });
    }

    /// Add a directory entry (stored, no data).
    fn add_directory(&mut self, name: &str, mod_date: u16, mod_time: u16) {
        // Directory names must end with '/'.
        let dir_name = if name.ends_with('/') {
            name.to_string()
        } else {
            format!("{name}/")
        };

        let local_offset = self.buf.len() as u32;
        let name_bytes = dir_name.as_bytes();

        write_u32_le(&mut self.buf, SIG_LOCAL);
        write_u16_le(&mut self.buf, VERSION_NEEDED_STORED);
        write_u16_le(&mut self.buf, 0);
        write_u16_le(&mut self.buf, METHOD_STORED);
        write_u16_le(&mut self.buf, mod_time);
        write_u16_le(&mut self.buf, mod_date);
        write_u32_le(&mut self.buf, 0); // crc
        write_u32_le(&mut self.buf, 0); // compressed size
        write_u32_le(&mut self.buf, 0); // uncompressed size
        write_u16_le(&mut self.buf, name_bytes.len() as u16);
        write_u16_le(&mut self.buf, 0);
        self.buf.extend_from_slice(name_bytes);
        // No data.

        self.entries.push(ZipEntry {
            name: dir_name,
            method: METHOD_STORED,
            mod_date,
            mod_time,
            crc32: 0,
            compressed_size: 0,
            uncompressed_size: 0,
            local_header_offset: local_offset,
            comment: String::new(),
            external_attrs: 0x0010_0000, // directory attribute for Unix
            internal_attrs: 0,
        });
    }

    /// Finish the archive and return the complete ZIP bytes.
    fn finish(mut self) -> Vec<u8> {
        let cd_offset = self.buf.len() as u32;

        for entry in &self.entries {
            let name_bytes = entry.name.as_bytes();
            let comment_bytes = entry.comment.as_bytes();

            write_u32_le(&mut self.buf, SIG_CENTRAL);
            write_u16_le(&mut self.buf, VERSION_MADE_BY);
            let version_needed = if entry.method == METHOD_DEFLATE {
                VERSION_NEEDED_DEFLATE
            } else {
                VERSION_NEEDED_STORED
            };
            write_u16_le(&mut self.buf, version_needed);
            write_u16_le(&mut self.buf, 0); // general purpose bit flag
            write_u16_le(&mut self.buf, entry.method);
            write_u16_le(&mut self.buf, entry.mod_time);
            write_u16_le(&mut self.buf, entry.mod_date);
            write_u32_le(&mut self.buf, entry.crc32);
            write_u32_le(&mut self.buf, entry.compressed_size);
            write_u32_le(&mut self.buf, entry.uncompressed_size);
            write_u16_le(&mut self.buf, name_bytes.len() as u16);
            write_u16_le(&mut self.buf, 0); // extra field length
            write_u16_le(&mut self.buf, comment_bytes.len() as u16);
            write_u16_le(&mut self.buf, 0); // disk number start
            write_u16_le(&mut self.buf, entry.internal_attrs);
            write_u32_le(&mut self.buf, entry.external_attrs);
            write_u32_le(&mut self.buf, entry.local_header_offset);
            self.buf.extend_from_slice(name_bytes);
            self.buf.extend_from_slice(comment_bytes);
        }

        let cd_size = self.buf.len() as u32 - cd_offset;
        let entry_count = self.entries.len() as u16;

        // End of central directory record.
        write_u32_le(&mut self.buf, SIG_EOCD);
        write_u16_le(&mut self.buf, 0); // disk number
        write_u16_le(&mut self.buf, 0); // disk with start of CD
        write_u16_le(&mut self.buf, entry_count);
        write_u16_le(&mut self.buf, entry_count);
        write_u32_le(&mut self.buf, cd_size);
        write_u32_le(&mut self.buf, cd_offset);
        write_u16_le(&mut self.buf, 0); // archive comment length

        self.buf
    }
}

// ============================================================================
// Binary write helpers
// ============================================================================

#[inline]
fn write_u16_le(buf: &mut Vec<u8>, val: u16) {
    buf.extend_from_slice(&val.to_le_bytes());
}

#[inline]
fn write_u32_le(buf: &mut Vec<u8>, val: u32) {
    buf.extend_from_slice(&val.to_le_bytes());
}

// ============================================================================
// File system helpers
// ============================================================================

/// Read an entire file into a `Vec<u8>`.
fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    let mut f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)
        .map_err(|e| format!("{}: read error: {e}", path.display()))?;
    Ok(data)
}

/// Write bytes to a file.
fn write_file(path: &Path, data: &[u8]) -> Result<(), String> {
    let mut f = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(data)
        .map_err(|e| format!("{}: write error: {e}", path.display()))?;
    Ok(())
}

/// Get modification time of a file, or epoch on error.
fn file_mtime(path: &Path) -> SystemTime {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Create parent directories for a path, if they don't exist.
fn create_parent_dirs(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|e| format!("{}: mkdir: {e}", parent.display()))?;
    }
    Ok(())
}

/// Glob-style pattern matching (supports `*` and `?`).
///
/// `*` matches any sequence of characters (not crossing directory boundaries).
/// For simplicity in this implementation, `*` matches any sequence including `/`.
fn glob_matches(pattern: &str, name: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), name.as_bytes())
}

fn glob_match_inner(pat: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si) = (0usize, 0usize);
    // `star_pi` records the pattern position just after the most recent `*`;
    // `star_si` records how far into `s` that `*` has been stretched so far.
    let mut star_pi: Option<usize> = None;
    let mut star_si = 0usize;

    // Advance through the string. Each branch makes progress; the star-backtrack
    // case only ever increments `star_si` up to `s.len()`, so this terminates.
    while si < s.len() {
        if pi < pat.len() && (pat[pi] == b'?' || pat[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < pat.len() && pat[pi] == b'*' {
            star_pi = Some(pi);
            star_si = si;
            pi += 1;
        } else if let Some(sp) = star_pi {
            // Backtrack: let the last `*` swallow one more character of `s`.
            pi = sp + 1;
            star_si += 1;
            si = star_si;
        } else {
            return false;
        }
    }

    // String consumed: any trailing pattern must be all `*` to match.
    while pi < pat.len() && pat[pi] == b'*' {
        pi += 1;
    }
    pi == pat.len()
}

/// Format file size as a human-readable string.
fn human_size(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KiB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", n as f64 / (1024.0 * 1024.0))
    }
}

/// Decode a DOS date/time back to a display string.
fn dos_datetime_str(mod_date: u16, mod_time: u16) -> String {
    let year = 1980 + ((mod_date >> 9) & 0x7F);
    let month = (mod_date >> 5) & 0x0F;
    let day = mod_date & 0x1F;
    let hour = (mod_time >> 11) & 0x1F;
    let minute = (mod_time >> 5) & 0x3F;
    let second = (mod_time & 0x1F) * 2;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}

// ============================================================================
// Recursive directory listing
// ============================================================================

/// Collect all files (recursively) under `dir`, recording them relative to `base`.
fn collect_files(
    dir: &Path,
    base: &Path,
    junk_paths: bool,
    excludes: &[String],
    files_out: &mut Vec<(PathBuf, String)>,
    errors: &mut Vec<String>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            errors.push(format!("{}: {e}", dir.display()));
            return;
        }
    };

    let mut children: Vec<PathBuf> = entries
        .filter_map(|e| match e {
            Ok(de) => Some(de.path()),
            Err(err) => {
                errors.push(format!("{}: {err}", dir.display()));
                None
            }
        })
        .collect();
    children.sort();

    for child in &children {
        let arc_name = archive_name(child, base, junk_paths);
        if excludes.iter().any(|p| glob_matches(p, &arc_name)) {
            continue;
        }
        if child.is_dir() {
            collect_files(child, base, junk_paths, excludes, files_out, errors);
        } else {
            files_out.push((child.clone(), arc_name));
        }
    }
}

/// Compute the archive name for a file.
///
/// If `junk_paths` is true, only the filename is stored (no directory component).
/// Otherwise the path relative to `base` is stored, using forward slashes.
fn archive_name(path: &Path, base: &Path, junk_paths: bool) -> String {
    if junk_paths {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        path.strip_prefix(base)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

// ============================================================================
// CLI option types
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolMode {
    Zip,
    Unzip,
}

/// Options for zip mode.
struct ZipOptions {
    /// Archive file path.
    archive: String,
    /// Source files/dirs to add.
    sources: Vec<String>,
    /// Compression level (0-9).
    level: u8,
    /// Recurse into directories.
    recursive: bool,
    /// Junk paths (store only filename).
    junk_paths: bool,
    /// Verbose output.
    verbose: bool,
    /// Quiet (suppress normal output).
    quiet: bool,
    /// Patterns to exclude.
    excludes: Vec<String>,
    /// Update mode: only add newer files.
    update: bool,
}

/// Options for unzip mode.
struct UnzipOptions {
    /// Archive file path.
    archive: String,
    /// Specific files to extract (empty = all).
    files: Vec<String>,
    /// Output directory.
    dest_dir: String,
    /// List contents instead of extracting.
    list: bool,
    /// Test integrity only.
    test: bool,
    /// Overwrite without prompting.
    overwrite: bool,
    /// Never overwrite.
    no_overwrite: bool,
    /// Verbose listing.
    verbose: bool,
    /// Quiet (suppress normal output).
    quiet: bool,
}

// ============================================================================
// Argument parsing
// ============================================================================

fn detect_mode(argv0: &str) -> ToolMode {
    let base = Path::new(argv0)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(argv0);
    if base == "unzip" {
        ToolMode::Unzip
    } else {
        ToolMode::Zip
    }
}

fn parse_zip_args(args: &[String]) -> Result<ZipOptions, String> {
    if args.is_empty() {
        return Err("zip: no arguments (try -h for help)".to_string());
    }

    let mut level: u8 = 6;
    let mut recursive = false;
    let mut junk_paths = false;
    let mut verbose = false;
    let mut quiet = false;
    let mut excludes: Vec<String> = Vec::new();
    let mut update = false;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--" {
            i += 1;
            while i < args.len() {
                positional.push(args[i].clone());
                i += 1;
            }
            break;
        }
        if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            for ch in arg[1..].chars() {
                match ch {
                    'r' => recursive = true,
                    'j' => junk_paths = true,
                    'v' => verbose = true,
                    'q' => quiet = true,
                    'u' => update = true,
                    'x' => {
                        i += 1;
                        if i >= args.len() {
                            return Err("zip: -x requires a pattern argument".to_string());
                        }
                        excludes.push(args[i].clone());
                    }
                    '0'..='9' => level = ch as u8 - b'0',
                    'h' => {
                        print_zip_usage();
                        process::exit(0);
                    }
                    c => return Err(format!("zip: unknown option: -{c}")),
                }
            }
        } else if arg == "--help" {
            print_zip_usage();
            process::exit(0);
        } else {
            positional.push(arg.to_string());
        }
        i += 1;
    }

    if positional.is_empty() {
        return Err("zip: no archive name specified".to_string());
    }

    let archive = positional[0].clone();
    let sources = positional[1..].to_vec();

    Ok(ZipOptions {
        archive,
        sources,
        level,
        recursive,
        junk_paths,
        verbose,
        quiet,
        excludes,
        update,
    })
}

fn parse_unzip_args(args: &[String]) -> Result<UnzipOptions, String> {
    if args.is_empty() {
        return Err("unzip: no arguments (try -h for help)".to_string());
    }

    let mut dest_dir = String::from(".");
    let mut list = false;
    let mut test = false;
    let mut overwrite = false;
    let mut no_overwrite = false;
    let mut verbose = false;
    let mut quiet = false;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--" {
            i += 1;
            while i < args.len() {
                positional.push(args[i].clone());
                i += 1;
            }
            break;
        }
        if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            for ch in arg[1..].chars() {
                match ch {
                    'd' => {
                        i += 1;
                        if i >= args.len() {
                            return Err("unzip: -d requires a directory argument".to_string());
                        }
                        dest_dir.clone_from(&args[i]);
                    }
                    'l' => list = true,
                    't' => test = true,
                    'o' => overwrite = true,
                    'n' => no_overwrite = true,
                    'v' => verbose = true,
                    'q' => quiet = true,
                    'h' => {
                        print_unzip_usage();
                        process::exit(0);
                    }
                    c => return Err(format!("unzip: unknown option: -{c}")),
                }
            }
        } else if arg == "--help" {
            print_unzip_usage();
            process::exit(0);
        } else {
            positional.push(arg.to_string());
        }
        i += 1;
    }

    if positional.is_empty() {
        return Err("unzip: no archive name specified".to_string());
    }

    let archive = positional[0].clone();
    let files = positional[1..].to_vec();

    Ok(UnzipOptions {
        archive,
        files,
        dest_dir,
        list,
        test,
        overwrite,
        no_overwrite,
        verbose,
        quiet,
    })
}

// ============================================================================
// zip mode implementation
// ============================================================================

fn run_zip(opts: &ZipOptions) -> Result<(), String> {
    if opts.sources.is_empty() {
        return Err("zip: no source files specified".to_string());
    }

    // If archive already exists and we're in update mode, load it.
    let mut existing: Vec<ZipEntry> = Vec::new();
    let mut existing_data: Vec<u8> = Vec::new();
    let archive_path = Path::new(&opts.archive);

    if opts.update && archive_path.exists() {
        existing_data = read_file(archive_path)?;
        existing = zip_read_central_directory(&existing_data)
            .map_err(|e| format!("zip: reading existing archive: {e}"))?;
    }

    let mut writer = ZipWriter::new();
    let mut total_files = 0u64;
    let mut total_bytes = 0u64;
    let mut errors: Vec<String> = Vec::new();

    for source in &opts.sources {
        let path = Path::new(source);

        if !path.exists() {
            errors.push(format!("zip: {source}: No such file or directory"));
            continue;
        }

        if path.is_dir() {
            if opts.recursive {
                let mut files: Vec<(PathBuf, String)> = Vec::new();
                collect_files(
                    path,
                    path,
                    opts.junk_paths,
                    &opts.excludes,
                    &mut files,
                    &mut errors,
                );
                // Also add the directory entry itself.
                let dir_arc_name =
                    archive_name(path, path.parent().unwrap_or(path), opts.junk_paths);
                if !dir_arc_name.is_empty() {
                    let (dd, dt) = encode_dos_datetime(file_mtime(path));
                    writer.add_directory(&dir_arc_name, dd, dt);
                }
                for (fpath, arc_name) in &files {
                    if let Err(e) = add_one_file(
                        &mut writer,
                        fpath,
                        arc_name,
                        opts.level,
                        opts.update,
                        &existing,
                        &existing_data,
                        opts.verbose,
                        opts.quiet,
                        &mut total_files,
                        &mut total_bytes,
                    ) {
                        errors.push(e);
                    }
                }
            } else if !opts.quiet {
                eprintln!(
                    "zip: {}: is a directory -- ignored (use -r for recursive)",
                    quotef_os(source)
                );
            }
            continue;
        }

        let arc_name = if opts.junk_paths {
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            source.replace('\\', "/")
        };

        if opts.excludes.iter().any(|p| glob_matches(p, &arc_name)) {
            continue;
        }

        if let Err(e) = add_one_file(
            &mut writer,
            path,
            &arc_name,
            opts.level,
            opts.update,
            &existing,
            &existing_data,
            opts.verbose,
            opts.quiet,
            &mut total_files,
            &mut total_bytes,
        ) {
            errors.push(e);
        }
    }

    let archive_bytes = writer.finish();
    write_file(archive_path, &archive_bytes)?;

    if !opts.quiet {
        eprintln!(
            "zip: {total_files} file(s), {} → {} (archive: {})",
            human_size(total_bytes),
            opts.archive,
            human_size(archive_bytes.len() as u64),
        );
    }

    if !errors.is_empty() {
        for e in &errors {
            eprintln!("{e}");
        }
        return Err(format!("zip: {} error(s) occurred", errors.len()));
    }

    Ok(())
}

#[allow(clippy::similar_names)]
fn add_one_file(
    writer: &mut ZipWriter,
    path: &Path,
    arc_name: &str,
    level: u8,
    update: bool,
    existing: &[ZipEntry],
    existing_data: &[u8],
    verbose: bool,
    quiet: bool,
    total_files: &mut u64,
    total_bytes: &mut u64,
) -> Result<(), String> {
    // In update mode, check if a newer version already exists in the archive.
    if update && let Some(existing_entry) = existing.iter().find(|e| e.name == arc_name) {
        let file_mtime = encode_dos_datetime(file_mtime(path));
        let entry_mtime = (existing_entry.mod_date, existing_entry.mod_time);
        if entry_mtime >= file_mtime {
            // Existing entry is as new or newer; copy it.
            let comp_data = zip_read_local_data(existing_data, existing_entry)
                .map_err(|e| format!("zip: update mode: {e}"))?;
            writer.buf.extend_from_slice(comp_data); // simplified copy
            writer.entries.push(existing_entry.clone());
            return Ok(());
        }
    }

    let data = read_file(path)?;
    let (mod_date, mod_time) = encode_dos_datetime(file_mtime(path));

    writer.add_file(arc_name, &data, level, mod_date, mod_time);

    if verbose && !quiet {
        let last = writer.entries.last();
        let comp_size = last.map_or(0, |e| e.compressed_size as u64);
        let ratio = if data.is_empty() {
            0.0
        } else {
            100.0 * (1.0 - comp_size as f64 / data.len() as f64)
        };
        eprintln!("  adding: {arc_name} (deflated {ratio:.0}%)");
    }

    *total_files += 1;
    *total_bytes += data.len() as u64;
    Ok(())
}

// ============================================================================
// unzip mode implementation
// ============================================================================

fn run_unzip(opts: &UnzipOptions) -> Result<(), String> {
    let archive_path = Path::new(&opts.archive);
    let archive_data =
        read_file(archive_path).map_err(|e| format!("unzip: cannot open {}: {e}", opts.archive))?;

    let entries = zip_read_central_directory(&archive_data)
        .map_err(|e| format!("unzip: {}: {e}", opts.archive))?;

    if opts.list || opts.verbose {
        list_archive(&entries, opts);
        return Ok(());
    }

    if opts.test {
        return test_archive(&archive_data, &entries, opts);
    }

    extract_archive(&archive_data, &entries, opts)
}

fn list_archive(entries: &[ZipEntry], opts: &UnzipOptions) {
    // Pre-rendered dashed separator rows (avoids passing empty literals to
    // width/fill format specifiers).
    const SEP_VERBOSE: &str =
        "---------- ----- ---------- ----------  ----------------  --------------------";
    const SEP_PLAIN: &str = "----------  ----------------  --------------------";

    if !opts.quiet {
        if opts.verbose {
            println!(
                "{:>10} {:>5} {:>10} {:>10}  Date/Time  Name",
                "Length", "Method", "Compressed", "Ratio"
            );
            println!("{SEP_VERBOSE}");
        } else {
            println!("{:>10}  Date/Time          Name", "Length");
            println!("{SEP_PLAIN}");
        }
    }

    let mut total_uncomp = 0u64;
    let mut total_comp = 0u64;
    let mut count = 0usize;

    for entry in entries {
        if !opts.files.is_empty() && !opts.files.iter().any(|f| glob_matches(f, &entry.name)) {
            continue;
        }
        let dt = dos_datetime_str(entry.mod_date, entry.mod_time);
        if opts.verbose {
            let method_str = match entry.method {
                METHOD_STORED => "Stored",
                METHOD_DEFLATE => "Defl:N",
                m => Box::leak(format!("{m}").into_boxed_str()),
            };
            let ratio = if entry.uncompressed_size == 0 {
                0.0
            } else {
                100.0 * (1.0 - entry.compressed_size as f64 / entry.uncompressed_size as f64)
            };
            println!(
                "{:>10} {:>6} {:>10} {:>9.0}%  {}  {}",
                entry.uncompressed_size, method_str, entry.compressed_size, ratio, dt, entry.name
            );
        } else {
            println!("{:>10}  {}  {}", entry.uncompressed_size, dt, entry.name);
        }
        total_uncomp += u64::from(entry.uncompressed_size);
        total_comp += u64::from(entry.compressed_size);
        count += 1;
    }

    if !opts.quiet {
        if opts.verbose {
            println!("{SEP_VERBOSE}");
            let ratio = if total_uncomp == 0 {
                0.0
            } else {
                100.0 * (1.0 - total_comp as f64 / total_uncomp as f64)
            };
            println!(
                "{total_uncomp:>10}          {total_comp:>10} {ratio:>9.0}%                    {count} files"
            );
        } else {
            println!("{SEP_PLAIN}");
            println!("{total_uncomp:>10}                    {count} files");
        }
    }
}

fn test_archive(
    archive_data: &[u8],
    entries: &[ZipEntry],
    opts: &UnzipOptions,
) -> Result<(), String> {
    let mut errors = 0usize;

    for entry in entries {
        if !opts.files.is_empty() && !opts.files.iter().any(|f| glob_matches(f, &entry.name)) {
            continue;
        }
        if entry.name.ends_with('/') {
            continue; // skip directory entries
        }
        match zip_extract_entry(archive_data, entry) {
            Ok(_) => {
                if !opts.quiet {
                    println!("    testing: {}   OK", entry.name);
                }
            }
            Err(e) => {
                eprintln!("    testing: {}   FAILED: {e}", entry.name);
                errors += 1;
            }
        }
    }

    if errors > 0 {
        Err(format!("unzip: {errors} error(s) during test"))
    } else {
        if !opts.quiet {
            println!("No errors detected in archive.");
        }
        Ok(())
    }
}

fn extract_archive(
    archive_data: &[u8],
    entries: &[ZipEntry],
    opts: &UnzipOptions,
) -> Result<(), String> {
    let dest = Path::new(&opts.dest_dir);
    let mut errors: Vec<String> = Vec::new();
    let mut extracted = 0usize;

    for entry in entries {
        if !opts.files.is_empty() && !opts.files.iter().any(|f| glob_matches(f, &entry.name)) {
            continue;
        }

        // Sanitize path: reject absolute paths and `..` components.
        if entry.name.starts_with('/') || entry.name.contains("../") || entry.name == ".." {
            eprintln!("unzip: skipping unsafe path: {}", entry.name);
            continue;
        }

        let out_path = dest.join(&entry.name);

        if entry.name.ends_with('/') {
            // Directory entry.
            if let Err(e) = fs::create_dir_all(&out_path) {
                errors.push(format!("{}: mkdir: {e}", out_path.display()));
            }
            continue;
        }

        // Overwrite logic.
        if out_path.exists() {
            if opts.no_overwrite {
                if !opts.quiet {
                    println!("unzip: not overwriting {}", out_path.display());
                }
                continue;
            }
            if !opts.overwrite {
                // Default: overwrite.
                // (Interactive prompting is not implemented; we default to overwrite.)
            }
        }

        if let Err(e) = create_parent_dirs(&out_path) {
            errors.push(e);
            continue;
        }

        match zip_extract_entry(archive_data, entry) {
            Ok(data) => {
                if let Err(e) = write_file(&out_path, &data) {
                    errors.push(e);
                } else {
                    if !opts.quiet {
                        println!("  inflating: {}", out_path.display());
                    }
                    extracted += 1;
                }
            }
            Err(e) => {
                errors.push(format!("unzip: {e}"));
            }
        }
    }

    if !opts.quiet {
        println!(
            "unzip: extracted {extracted} file(s) to {}",
            quoteaf_os(dest)
        );
    }

    if errors.is_empty() {
        Ok(())
    } else {
        for e in &errors {
            eprintln!("{e}");
        }
        Err(format!("unzip: {} error(s)", errors.len()))
    }
}

// ============================================================================
// Usage / help
// ============================================================================

fn print_zip_usage() {
    eprintln!(
        "\
Usage: zip [OPTIONS] archive.zip file1 file2 ...

Create or update a ZIP archive.

Options:
  -r          Recurse into directories
  -j          Junk paths (store only filenames)
  -0 to -9    Compression level (0=stored, default: 6)
  -v          Verbose output
  -q          Quiet
  -x PATTERN  Exclude files matching PATTERN (may repeat)
  -u          Update: only add files newer than archive entries
  -h          Show this help"
    );
}

fn print_unzip_usage() {
    eprintln!(
        "\
Usage: unzip [OPTIONS] archive.zip [file ...]

Extract files from a ZIP archive.

Options:
  -d DIR      Extract to DIR (default: current directory)
  -l          List contents
  -v          Verbose listing
  -t          Test integrity
  -o          Overwrite files without prompting
  -n          Never overwrite existing files
  -q          Quiet
  -h          Show this help"
    );
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().map_or("zip", String::as_str);
    let mode = detect_mode(argv0);

    let cli_args = if args.len() > 1 {
        &args[1..]
    } else {
        &[] as &[String]
    };

    let result = match mode {
        ToolMode::Zip => match parse_zip_args(cli_args) {
            Ok(opts) => run_zip(&opts),
            Err(e) => Err(e),
        },
        ToolMode::Unzip => match parse_unzip_args(cli_args) {
            Ok(opts) => run_unzip(&opts),
            Err(e) => Err(e),
        },
    };

    if let Err(e) = result {
        eprintln!("{e}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    // ---- CRC32 ----
    //
    // The polynomial's own test vectors (empty, "123456789" = 0xCBF43926,
    // incremental chaining) moved out with the implementation: they belong to
    // the `crc32` crate, which asserts them in its doctests, and repeating
    // them here would test that crate twice rather than testing this one.
    //
    // What is left is the question this crate cannot answer for us: that zip
    // reaches for the *right* CRC-32. Two functions in this tree answer to
    // that name -- the reflected-IEEE one PKZIP specifies, and the CRC32C
    // (Castagnoli) the kernel uses for ext4 -- and picking the wrong one
    // still compiles, still produces a stable checksum, and still round-trips
    // against ourselves. It fails only against every other unzip in the
    // world, which is not something our own round-trip tests below can see.
    // So this pins the value an outside implementation would compute.

    #[test]
    fn test_zip_uses_reflected_ieee_crc32_not_crc32c() {
        // 0xCBF43926 is reflected-IEEE over "123456789"; CRC32C over the same
        // input is 0xE3069283. Only the first is a valid ZIP checksum.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_ne!(crc32(b"123456789"), 0xE306_9283);
    }

    // ---- DOS date/time ----

    #[test]
    fn test_dos_datetime_epoch() {
        let (date, time) = encode_dos_datetime(SystemTime::UNIX_EPOCH);
        // 1970-01-01 00:00:00 → year=1970, but DOS epoch starts at 1980.
        // Year offset: 1970-1980 = -10 → clamped to 0 (1980-01-01).
        let year = 1980 + ((date >> 9) & 0x7F);
        assert_eq!(year, 1980);
        let _ = time; // just check it doesn't panic
    }

    #[test]
    fn test_dos_datetime_str_known() {
        // Encode known date 2023-06-15 12:30:00.
        let year_offset: u16 = 2023 - 1980;
        let mod_date: u16 = (year_offset << 9) | (6 << 5) | 15;
        let mod_time: u16 = (12 << 11) | (30 << 5);
        let s = dos_datetime_str(mod_date, mod_time);
        assert_eq!(s, "2023-06-15 12:30:00");
    }

    // ---- Unix datetime conversion ----

    #[test]
    fn test_unix_datetime_epoch() {
        let (y, mo, d, h, mi, s) = unix_secs_to_datetime(0);
        assert_eq!(y, 1970);
        assert_eq!(mo, 1);
        assert_eq!(d, 1);
        assert_eq!(h, 0);
        assert_eq!(mi, 0);
        assert_eq!(s, 0);
    }

    #[test]
    fn test_unix_datetime_known() {
        // 2023-01-01 00:00:00 UTC = 1672531200
        let (y, mo, d, h, mi, s) = unix_secs_to_datetime(1_672_531_200);
        assert_eq!(y, 2023);
        assert_eq!(mo, 1);
        assert_eq!(d, 1);
        assert_eq!(h, 0);
        assert_eq!(mi, 0);
        assert_eq!(s, 0);
    }

    // ---- Glob matching ----

    #[test]
    fn test_glob_exact_match() {
        assert!(glob_matches("foo.txt", "foo.txt"));
        assert!(!glob_matches("foo.txt", "bar.txt"));
    }

    #[test]
    fn test_glob_star() {
        assert!(glob_matches("*.txt", "hello.txt"));
        assert!(glob_matches("*.txt", ".txt"));
        assert!(!glob_matches("*.txt", "hello.rs"));
    }

    #[test]
    fn test_glob_question() {
        assert!(glob_matches("f?o", "foo"));
        assert!(glob_matches("f?o", "fxo"));
        assert!(!glob_matches("f?o", "fo"));
    }

    #[test]
    fn test_glob_star_prefix_suffix() {
        assert!(glob_matches("*.log", "access.log"));
        assert!(glob_matches("log*", "logfile"));
        assert!(glob_matches("*", "anything"));
        assert!(glob_matches("*", ""));
    }

    #[test]
    fn test_glob_no_match_terminates() {
        // Regression: a `*` followed by a suffix that never matches used to
        // spin forever (si ran past the end of the string unbounded).
        assert!(!glob_matches("*.txt", "hello.rs"));
        assert!(!glob_matches("*abc", "xyz"));
        assert!(!glob_matches("a*z", "abc"));
        assert!(!glob_matches("foo*", "fo"));
    }

    #[test]
    fn test_glob_multiple_stars() {
        assert!(glob_matches("*a*b*", "xaybz"));
        assert!(glob_matches("a*b*c", "abc"));
        assert!(glob_matches("**", "anything"));
        assert!(!glob_matches("a*b*c", "abx"));
    }

    // ---- Level handling ----
    //
    // These used to be six tests of a local DEFLATE encoder. That encoder is
    // gone (see the `DEFLATE: see the `deflate` crate` banner above), and
    // re-testing `deflate::deflate_level` from here would only duplicate that
    // crate's own suite while pinning this file to its output. What is still
    // this file's to get wrong is the *method choice* -- the `-0`..`-9` flag
    // is parsed here, the STORED/DEFLATE decision is made here, and neither is
    // visible to the crate. So that is what these assert, through the archive
    // rather than through the encoder.

    /// Round-trip through the container at every level `zip` accepts, since
    /// the level reaches the encoder only via `add_file` and a level that
    /// silently produced an undecodable member would look like a good archive
    /// until someone tried to open it.
    #[test]
    fn test_every_level_round_trips_through_the_archive() {
        let input: Vec<u8> = (0u8..=9).cycle().take(2000).collect();
        for level in 0u8..=9 {
            let mut writer = ZipWriter::new();
            writer.add_file("data.bin", &input, level, 0, 0);
            let archive = writer.finish();

            let entries = zip_read_central_directory(&archive).unwrap();
            let out = zip_extract_entry(&archive, &entries[0]).unwrap();
            assert_eq!(out, input, "level={level}");
        }
    }

    /// `-0` means "do not compress", and it is the one level whose method is
    /// fixed rather than chosen by whether the output shrank.
    #[test]
    fn test_level_zero_stores_even_when_the_input_would_compress() {
        let input = vec![b'a'; 4096];
        let mut writer = ZipWriter::new();
        writer.add_file("rep.bin", &input, 0, 0, 0);
        let archive = writer.finish();

        let entries = zip_read_central_directory(&archive).unwrap();
        assert_eq!(entries[0].method, METHOD_STORED);
        assert_eq!(entries[0].compressed_size, 4096);
    }

    /// The complement: at any non-zero level, compressible input must actually
    /// take the DEFLATE path and land smaller. A regression that made
    /// `deflate_level` return something larger than its input would otherwise
    /// be invisible -- the fallback below would quietly store every member and
    /// every round-trip test would still pass.
    #[test]
    fn test_compressible_input_deflates_and_shrinks() {
        let input: Vec<u8> = (0u8..=9).cycle().take(2000).collect();
        for level in 1u8..=9 {
            let mut writer = ZipWriter::new();
            writer.add_file("rep.bin", &input, level, 0, 0);
            let archive = writer.finish();

            let entries = zip_read_central_directory(&archive).unwrap();
            assert_eq!(entries[0].method, METHOD_DEFLATE, "level={level}");
            assert!(
                (entries[0].compressed_size as usize) < input.len(),
                "level={level}: compressed ({}) should be < input ({})",
                entries[0].compressed_size,
                input.len()
            );
        }
    }

    /// The `-N` flag must actually *reach* the encoder, and this is the only
    /// test here that can tell.
    ///
    /// Every other test above passes unchanged if `add_file` ignores its
    /// `level` argument and hardcodes one — the archive still round-trips, the
    /// method is still DEFLATE, the member is still smaller. Asserting that
    /// the knob has an observable *effect* is what closes that, and it is the
    /// shape lane A recommended in
    /// `requests/a-b-deflate-level-has-landed-and-your-local-compressor-was-the-better-one.md`
    /// after the same weakness hid a dead LZ77 stage in the `deflate` crate
    /// for weeks: a compression test asserting only "output got smaller"
    /// cannot distinguish a real encoder from a `memcpy`.
    ///
    /// The assertion is `<=` and not `<` because level 9 is permitted to find
    /// nothing more than level 1 did on a given input; what it may never do is
    /// come out *larger*. The corpus is chosen so the inequality is strict in
    /// practice, which is what the second assertion pins.
    #[test]
    fn test_the_level_flag_changes_the_output() {
        // Long repeats at varying distances: deeper hash-chain searching has
        // something to find here, so effort translates into ratio.
        let mut input = Vec::new();
        for i in 0..400 {
            input.extend_from_slice(b"the quick brown fox jumps over the lazy dog");
            input.extend_from_slice(&[(i % 251) as u8]);
        }

        let size_at = |level: u8| -> u32 {
            let mut writer = ZipWriter::new();
            writer.add_file("corpus.bin", &input, level, 0, 0);
            let archive = writer.finish();
            let entries = zip_read_central_directory(&archive).unwrap();
            entries[0].compressed_size
        };

        let fast = size_at(1);
        let small = size_at(9);
        assert!(
            small <= fast,
            "level 9 ({small}) must not be larger than level 1 ({fast})"
        );
        assert!(
            small < fast,
            "on this corpus level 9 ({small}) should beat level 1 ({fast}); \
             equal sizes mean the level never reached the encoder"
        );
    }

    /// Incompressible input must fall back to STORED rather than ship a member
    /// larger than the bytes it holds.
    #[test]
    fn test_incompressible_input_falls_back_to_stored() {
        // Four bytes: too short for DEFLATE's block header to pay for itself,
        // so the encoder cannot help but produce more bytes than it consumed.
        let input = b"\x00\x01\x02\x03";
        let mut writer = ZipWriter::new();
        writer.add_file("tiny.bin", input, 9, 0, 0);
        let archive = writer.finish();

        let entries = zip_read_central_directory(&archive).unwrap();
        assert_eq!(entries[0].method, METHOD_STORED);
        let out = zip_extract_entry(&archive, &entries[0]).unwrap();
        assert_eq!(out.as_slice(), input);
    }

    /// An empty member is the edge case that has broken every ZIP writer at
    /// least once: zero-length input, zero CRC, and a compressor that must not
    /// emit a member whose declared sizes disagree with its bytes.
    #[test]
    fn test_empty_member_round_trips_at_every_level() {
        for level in 0u8..=9 {
            let mut writer = ZipWriter::new();
            writer.add_file("empty.txt", b"", level, 0, 0);
            let archive = writer.finish();

            let entries = zip_read_central_directory(&archive).unwrap();
            assert_eq!(entries[0].uncompressed_size, 0, "level={level}");
            let out = zip_extract_entry(&archive, &entries[0]).unwrap();
            assert!(out.is_empty(), "level={level}");
        }
    }

    // ---- ZIP archive round-trips ----

    #[test]
    fn test_zip_single_file_stored() {
        let mut writer = ZipWriter::new();
        writer
            .add_file("hello.txt", b"Hello, ZIP!", 0, 0, 0);
        let archive = writer.finish();

        let entries = zip_read_central_directory(&archive).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "hello.txt");
        assert_eq!(entries[0].method, METHOD_STORED);

        let data = zip_extract_entry(&archive, &entries[0]).unwrap();
        assert_eq!(data, b"Hello, ZIP!");
    }

    #[test]
    fn test_zip_single_file_deflate() {
        // Compressible input so deflate actually shrinks it.
        let input: Vec<u8> = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_vec();
        let mut writer = ZipWriter::new();
        writer.add_file("rep.bin", &input, 6, 0, 0);
        let archive = writer.finish();

        let entries = zip_read_central_directory(&archive).unwrap();
        assert_eq!(entries.len(), 1);

        let data = zip_extract_entry(&archive, &entries[0]).unwrap();
        assert_eq!(data, input);
    }

    #[test]
    fn test_zip_entry_that_expands_past_its_declared_size_is_refused_mid_decode() {
        // A decompression bomb is exactly this: a central directory declaring
        // a small size over a stream that expands to a large one. The parser
        // copies that field verbatim into `ZipEntry`, so lowering it on the
        // parsed struct is the same input the crafted bytes would produce,
        // and far easier to read than patching two headers by hand.
        let input = vec![b'a'; 50_000];
        let mut writer = ZipWriter::new();
        writer.add_file("bomb.bin", &input, 6, 0, 0);
        let archive = writer.finish();

        let mut entries = zip_read_central_directory(&archive).unwrap();
        assert_eq!(entries[0].uncompressed_size, 50_000);
        entries[0].uncompressed_size = 10;

        let err = zip_extract_entry(&archive, &entries[0]).unwrap_err();

        // The wording is the assertion, not decoration. Before the output cap
        // existed this same archive was decompressed in full -- all 50 KB
        // resident -- and *then* rejected by the length comparison, reporting
        // "size mismatch: expected 10, got 50000". That message is what the
        // bug looks like: it proves the check ran on the far side of the
        // allocation it was meant to prevent. Refusing mid-decode is the only
        // way to get the message below, so this distinguishes the fix from
        // the bug rather than merely observing that both reject the file.
        assert!(
            err.contains("declares 10 byte(s) but decompresses to more"),
            "expected a refusal from the output cap, got: {err}"
        );
        assert!(
            !err.contains("size mismatch"),
            "the after-the-fact length check fired, so the cap did not: {err}"
        );
    }

    #[test]
    fn test_zip_entry_shorter_than_declared_is_still_caught() {
        // The direction an output cap structurally cannot see: the stream
        // stops early. Nothing exceeds the ceiling, so `inflate_limited`
        // returns happily and the length comparison after it is what catches
        // this. Both checks are load-bearing; they catch opposite faults.
        let input = vec![b'b'; 4_000];
        let mut writer = ZipWriter::new();
        writer.add_file("short.bin", &input, 6, 0, 0);
        let archive = writer.finish();

        let mut entries = zip_read_central_directory(&archive).unwrap();
        entries[0].uncompressed_size = 9_999;

        let err = zip_extract_entry(&archive, &entries[0]).unwrap_err();
        assert!(
            err.contains("size mismatch: expected 9999, got 4000"),
            "expected the length check to catch the short entry, got: {err}"
        );
    }

    #[test]
    fn test_zip_multiple_files() {
        let mut writer = ZipWriter::new();
        writer.add_file("a.txt", b"file A", 6, 0, 0);
        writer
            .add_file("b.txt", b"file B contents here", 6, 0, 0);
        writer.add_file("c.txt", b"", 0, 0, 0);
        let archive = writer.finish();

        let entries = zip_read_central_directory(&archive).unwrap();
        assert_eq!(entries.len(), 3);

        let a = zip_extract_entry(&archive, &entries[0]).unwrap();
        let b = zip_extract_entry(&archive, &entries[1]).unwrap();
        let c = zip_extract_entry(&archive, &entries[2]).unwrap();

        assert_eq!(a, b"file A");
        assert_eq!(b, b"file B contents here");
        assert_eq!(c, b"");
    }

    #[test]
    fn test_zip_directory_entry() {
        let mut writer = ZipWriter::new();
        writer.add_directory("subdir", 0, 0);
        writer
            .add_file("subdir/file.txt", b"inside dir", 0, 0, 0);
        let archive = writer.finish();

        let entries = zip_read_central_directory(&archive).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].name.ends_with('/'));
        assert_eq!(entries[1].name, "subdir/file.txt");
    }

    #[test]
    fn test_zip_empty_archive() {
        let writer = ZipWriter::new();
        let archive = writer.finish();
        let entries = zip_read_central_directory(&archive).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_zip_crc_mismatch_detected() {
        let mut writer = ZipWriter::new();
        writer.add_file("test.txt", b"test data", 0, 0, 0);
        let mut archive = writer.finish();

        // Corrupt a byte in the file data region.
        let entries = zip_read_central_directory(&archive).unwrap();
        let offset = entries[0].local_header_offset as usize;
        // Data starts after 30-byte header + filename length.
        let fname_len = entries[0].name.len();
        let data_offset = offset + 30 + fname_len;
        if data_offset < archive.len() {
            archive[data_offset] ^= 0xFF;
        }

        let result = zip_extract_entry(&archive, &entries[0]);
        assert!(result.is_err());
    }

    #[test]
    fn test_zip_find_eocd_basic() {
        let writer = ZipWriter::new();
        let archive = writer.finish();
        let offset = find_eocd(&archive).unwrap();
        let sig = u32::from_le_bytes([
            archive[offset],
            archive[offset + 1],
            archive[offset + 2],
            archive[offset + 3],
        ]);
        assert_eq!(sig, SIG_EOCD);
    }

    #[test]
    fn test_zip_large_file() {
        // 64 KiB of repeating data — exercises stored-block path and hash chains.
        let input: Vec<u8> = (0u8..=255).cycle().take(65536).collect();
        let mut writer = ZipWriter::new();
        writer.add_file("large.bin", &input, 6, 0, 0);
        let archive = writer.finish();

        let entries = zip_read_central_directory(&archive).unwrap();
        let data = zip_extract_entry(&archive, &entries[0]).unwrap();
        assert_eq!(data, input);
    }

    #[test]
    fn test_zip_name_with_path() {
        let mut writer = ZipWriter::new();
        writer.add_file("a/b/c.txt", b"nested", 0, 0, 0);
        let archive = writer.finish();

        let entries = zip_read_central_directory(&archive).unwrap();
        assert_eq!(entries[0].name, "a/b/c.txt");

        let data = zip_extract_entry(&archive, &entries[0]).unwrap();
        assert_eq!(data, b"nested");
    }

    // ---- archive_name helper ----

    #[test]
    fn test_archive_name_no_junk() {
        let path = Path::new("some/dir/file.txt");
        let base = Path::new("some/dir");
        let name = archive_name(path, base, false);
        assert_eq!(name, "file.txt");
    }

    #[test]
    fn test_archive_name_junk_paths() {
        let path = Path::new("some/dir/file.txt");
        let base = Path::new("some");
        let name = archive_name(path, base, true);
        assert_eq!(name, "file.txt");
    }

    // The three LZ77 token tests that stood here went with the local encoder:
    // they reached into `lz77_compress`'s `Token` stream, which is now internal
    // to the `deflate` crate and tested there against a match finder that
    // actually finds matches. Nothing at this layer can see a token.

    // ---- human_size ----

    #[test]
    fn test_human_size_bytes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
    }

    #[test]
    fn test_human_size_kib() {
        assert_eq!(human_size(1024), "1.0 KiB");
        assert_eq!(human_size(2048), "2.0 KiB");
    }

    #[test]
    fn test_human_size_mib() {
        assert_eq!(human_size(1024 * 1024), "1.0 MiB");
    }

    // ---- detect_mode ----

    #[test]
    fn test_detect_mode_zip() {
        assert_eq!(detect_mode("zip"), ToolMode::Zip);
        assert_eq!(detect_mode("/usr/bin/zip"), ToolMode::Zip);
    }

    #[test]
    fn test_detect_mode_unzip() {
        assert_eq!(detect_mode("unzip"), ToolMode::Unzip);
        assert_eq!(detect_mode("/bin/unzip"), ToolMode::Unzip);
    }

    // ---- parse_zip_args ----

    #[test]
    fn test_parse_zip_args_basic() {
        let args: Vec<String> = vec!["out.zip".into(), "file.txt".into()];
        let opts = parse_zip_args(&args).unwrap();
        assert_eq!(opts.archive, "out.zip");
        assert_eq!(opts.sources, vec!["file.txt".to_string()]);
        assert_eq!(opts.level, 6);
        assert!(!opts.recursive);
    }

    #[test]
    fn test_parse_zip_args_flags() {
        let args: Vec<String> = vec![
            "-r".into(),
            "-j".into(),
            "-9".into(),
            "-v".into(),
            "out.zip".into(),
            "dir/".into(),
        ];
        let opts = parse_zip_args(&args).unwrap();
        assert!(opts.recursive);
        assert!(opts.junk_paths);
        assert_eq!(opts.level, 9);
        assert!(opts.verbose);
    }

    #[test]
    fn test_parse_zip_args_exclude() {
        let args: Vec<String> = vec!["-x".into(), "*.log".into(), "out.zip".into(), "src/".into()];
        let opts = parse_zip_args(&args).unwrap();
        assert_eq!(opts.excludes, vec!["*.log".to_string()]);
    }

    // ---- parse_unzip_args ----

    #[test]
    fn test_parse_unzip_args_basic() {
        let args: Vec<String> = vec!["archive.zip".into()];
        let opts = parse_unzip_args(&args).unwrap();
        assert_eq!(opts.archive, "archive.zip");
        assert_eq!(opts.dest_dir, ".");
        assert!(!opts.list);
    }

    #[test]
    fn test_parse_unzip_args_flags() {
        let args: Vec<String> = vec![
            "-l".into(),
            "-d".into(),
            "/tmp/out".into(),
            "archive.zip".into(),
        ];
        let opts = parse_unzip_args(&args).unwrap();
        assert!(opts.list);
        assert_eq!(opts.dest_dir, "/tmp/out");
    }

    #[test]
    fn test_parse_unzip_args_specific_files() {
        let args: Vec<String> = vec!["archive.zip".into(), "a.txt".into(), "b.txt".into()];
        let opts = parse_unzip_args(&args).unwrap();
        assert_eq!(opts.files, vec!["a.txt".to_string(), "b.txt".to_string()]);
    }
}
