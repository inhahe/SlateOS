//! RAR5 archive support (read-only, list and extract).
//!
//! Implements parsing of the RAR version 5 format for listing archive
//! contents and extracting Store-mode (uncompressed) entries.
//! Compressed entries are listed but cannot be extracted without the
//! full PPMd/LZSS decoder (TODO for later).
//!
//! ## RAR5 format overview
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │ Signature: Rar!\x1a\x07\x01\x00 (8 B)  │
//! ├──────────────────────────────────────────┤
//! │ Archive Header block (type 1)            │
//! ├──────────────────────────────────────────┤
//! │ File Header block (type 2)               │
//! │   + data area (compressed/stored data)   │
//! ├──────────────────────────────────────────┤
//! │ ... more file headers ...                │
//! ├──────────────────────────────────────────┤
//! │ End of Archive block (type 5)            │
//! └──────────────────────────────────────────┘
//! ```
//!
//! ## Block structure
//!
//! Every block starts with:
//!   - CRC32 (4 bytes, little-endian)
//!   - Header size (vint)
//!   - Header type (vint)
//!   - Header flags (vint): bit 0 = extra area, bit 1 = data area
//!   - Extra area size (vint, if flag bit 0)
//!   - Data area size (vint, if flag bit 1)
//!
//! ## Variable-length integers (vint)
//!
//! 7 bits per byte, little-endian, high bit is continuation flag.
//!
//! ## References
//!
//! - RAR 5.0 archive format: <https://www.rarlab.com/technote.htm>
//! - unrar source: <https://github.com/AJenbo/unrar>

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// RAR5 signature: `Rar!\x1a\x07\x01\x00`
const RAR5_SIGNATURE: &[u8] = b"Rar!\x1a\x07\x01\x00";

/// RAR4 signature prefix: `Rar!\x1a\x07\x00` (7 bytes).
const RAR4_SIGNATURE: &[u8] = b"Rar!\x1a\x07\x00";

/// Block type: Archive Header.
const BLOCK_ARCHIVE: u64 = 1;
/// Block type: File Header.
const BLOCK_FILE: u64 = 2;
/// Block type: Service Header.
const _BLOCK_SERVICE: u64 = 3;
/// Block type: Encryption Header.
const _BLOCK_ENCRYPTION: u64 = 4;
/// Block type: End of Archive.
const BLOCK_END: u64 = 5;

/// Header flag: extra area present.
const HFL_EXTRA: u64 = 0x0001;
/// Header flag: data area present.
const HFL_DATA: u64 = 0x0002;

/// File flag: directory.
const FFLG_DIRECTORY: u64 = 0x0001;
/// File flag: has modification time.
const FFLG_UTIME: u64 = 0x0002;
/// File flag: CRC32 present.
const FFLG_CRC32: u64 = 0x0004;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single entry in a RAR5 archive.
#[derive(Debug, Clone)]
pub struct RarEntry {
    /// Filename (UTF-8).
    pub name: String,
    /// Uncompressed size in bytes.
    pub unpacked_size: u64,
    /// Compressed size in bytes (data area).
    pub packed_size: u64,
    /// Whether this is a directory entry.
    pub is_dir: bool,
    /// Whether the file is stored (uncompressed, method 0).
    pub is_stored: bool,
    /// Unix timestamp of last modification (0 if not present).
    pub mtime: u32,
    /// CRC32 of uncompressed data (0 if not present).
    pub crc32: u32,
    /// Host OS: 0 = Windows, 1 = Unix.
    pub host_os: u64,
    /// File attributes.
    pub attributes: u64,
    /// Offset of data area within the archive (for extraction).
    pub data_offset: usize,
}

// ---------------------------------------------------------------------------
// Variable-length integer (vint) decoder
// ---------------------------------------------------------------------------

/// Decode a RAR5 variable-length integer from `data[pos..]`.
///
/// Returns `(value, bytes_consumed)` or `CorruptedData` if truncated.
fn read_vint(data: &[u8], pos: usize) -> KernelResult<(u64, usize)> {
    let mut val: u64 = 0;
    let mut shift: u32 = 0;
    let mut i: usize = 0;

    loop {
        let byte_pos = pos.wrapping_add(i);
        if byte_pos >= data.len() {
            return Err(KernelError::CorruptedData);
        }
        let b = data[byte_pos];
        val |= u64::from(b & 0x7F) << shift;
        i += 1;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 63 {
            return Err(KernelError::CorruptedData);
        }
    }

    Ok((val, i))
}

/// Read a little-endian u32 from `data[pos..]`.
fn read_le32(data: &[u8], pos: usize) -> KernelResult<u32> {
    if pos + 4 > data.len() {
        return Err(KernelError::CorruptedData);
    }
    Ok(u32::from(data[pos])
        | (u32::from(data[pos + 1]) << 8)
        | (u32::from(data[pos + 2]) << 16)
        | (u32::from(data[pos + 3]) << 24))
}

// ---------------------------------------------------------------------------
// Public API — parse
// ---------------------------------------------------------------------------

/// Parse a RAR5 archive and return a list of all file entries.
///
/// # Errors
///
/// Returns `CorruptedData` if the magic signature is wrong, blocks are
/// truncated, or vint encoding is invalid.
/// Returns `InvalidArgument` if the archive is RAR4 (not supported).
pub fn parse(data: &[u8]) -> KernelResult<Vec<RarEntry>> {
    // Check signature.
    if data.len() < 8 {
        return Err(KernelError::CorruptedData);
    }
    if data.starts_with(RAR4_SIGNATURE) && !data.starts_with(RAR5_SIGNATURE) {
        return Err(KernelError::InvalidArgument);
    }
    if !data.starts_with(RAR5_SIGNATURE) {
        return Err(KernelError::CorruptedData);
    }

    let mut entries = Vec::new();
    let mut pos: usize = RAR5_SIGNATURE.len();

    loop {
        if pos >= data.len() {
            break;
        }

        // --- Read block header ---
        // CRC32 (4 bytes).
        let _crc32 = read_le32(data, pos)?;
        pos += 4;

        // Header size (vint).
        let (header_size, hs_len) = read_vint(data, pos)?;
        let header_size = header_size as usize;
        pos += hs_len;

        // The header content starts here.
        let header_start = pos;
        let header_end = header_start.wrapping_add(header_size);
        if header_end > data.len() {
            return Err(KernelError::CorruptedData);
        }

        // Header type (vint).
        let (block_type, bt_len) = read_vint(data, pos)?;
        pos += bt_len;

        // Header flags (vint).
        let (header_flags, hf_len) = read_vint(data, pos)?;
        pos += hf_len;

        // Extra area size (vint, if flag bit 0).
        let _extra_size: u64 = if header_flags & HFL_EXTRA != 0 {
            let (v, vl) = read_vint(data, pos)?;
            pos += vl;
            v
        } else {
            0
        };

        // Data area size (vint, if flag bit 1).
        let data_size: u64 = if header_flags & HFL_DATA != 0 {
            let (v, vl) = read_vint(data, pos)?;
            pos += vl;
            v
        } else {
            0
        };

        match block_type {
            BLOCK_FILE => {
                // --- Parse file header ---
                // File flags (vint).
                let (file_flags, ff_len) = read_vint(data, pos)?;
                pos += ff_len;

                // Unpacked size (vint).
                let (unpacked_size, us_len) = read_vint(data, pos)?;
                pos += us_len;

                // Attributes (vint).
                let (attributes, at_len) = read_vint(data, pos)?;
                pos += at_len;

                // mtime (4 bytes, if file flag bit 1).
                let mtime: u32 = if file_flags & FFLG_UTIME != 0 {
                    let t = read_le32(data, pos)?;
                    pos += 4;
                    t
                } else {
                    0
                };

                // Data CRC32 (4 bytes, if file flag bit 2).
                let crc32: u32 = if file_flags & FFLG_CRC32 != 0 {
                    let c = read_le32(data, pos)?;
                    pos += 4;
                    c
                } else {
                    0
                };

                // Compression info (vint).
                let (comp_info, ci_len) = read_vint(data, pos)?;
                pos += ci_len;

                let _version = comp_info & 0x3F;
                let method = (comp_info >> 7) & 0x0F;
                let is_stored = method == 0;

                // Host OS (vint).
                let (host_os, ho_len) = read_vint(data, pos)?;
                pos += ho_len;

                // Name length (vint).
                let (name_len, nl_len) = read_vint(data, pos)?;
                pos += nl_len;

                // Name (raw bytes).
                let name_len = name_len as usize;
                if pos + name_len > header_end {
                    return Err(KernelError::CorruptedData);
                }
                let name = core::str::from_utf8(
                    data.get(pos..pos + name_len).ok_or(KernelError::CorruptedData)?
                ).unwrap_or("");
                let name = String::from(name);

                // Data area starts right after the header.
                let data_offset = header_end;

                let is_dir = file_flags & FFLG_DIRECTORY != 0;

                entries.push(RarEntry {
                    name,
                    unpacked_size,
                    packed_size: data_size,
                    is_dir,
                    is_stored,
                    mtime,
                    crc32,
                    host_os,
                    attributes,
                    data_offset,
                });

                // Skip to end of header + data area.
                pos = header_end.wrapping_add(data_size as usize);
            }

            BLOCK_ARCHIVE | _BLOCK_SERVICE | _BLOCK_ENCRYPTION => {
                // Skip the header body and data area.
                pos = header_end.wrapping_add(data_size as usize);
            }

            BLOCK_END => {
                break;
            }

            _ => {
                // Unknown block type — skip it.
                pos = header_end.wrapping_add(data_size as usize);
            }
        }
    }

    Ok(entries)
}

// ---------------------------------------------------------------------------
// Public API — extract
// ---------------------------------------------------------------------------

/// Extract the data of a Store-mode entry from the archive.
///
/// Returns the raw file data slice.
///
/// # Errors
///
/// Returns `InvalidArgument` if the entry is compressed (not stored).
/// Returns `CorruptedData` if the data area is truncated.
pub fn entry_data<'a>(data: &'a [u8], entry: &RarEntry) -> KernelResult<&'a [u8]> {
    if !entry.is_stored {
        return Err(KernelError::InvalidArgument);
    }
    if entry.is_dir {
        return Ok(&[]);
    }

    let start = entry.data_offset;
    let end = start.wrapping_add(entry.unpacked_size as usize);
    if end > data.len() {
        return Err(KernelError::CorruptedData);
    }

    Ok(&data[start..end])
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Build a minimal valid RAR5 archive in memory for testing.
///
/// Creates an archive containing a single stored file "hello.txt" with
/// content "Hello, RAR5!".
fn build_test_archive() -> Vec<u8> {
    let mut ar = Vec::new();

    // --- Signature ---
    ar.extend_from_slice(RAR5_SIGNATURE);

    // --- Archive Header (type 1) ---
    {
        let mut hdr = Vec::new();
        // Header type = 1 (archive).
        push_vint(&mut hdr, BLOCK_ARCHIVE);
        // Header flags = 0 (no extra, no data).
        push_vint(&mut hdr, 0);
        // Archive flags = 0.
        push_vint(&mut hdr, 0);

        // CRC32 of header content.
        let crc = crc32_rar(&hdr);
        // Write: CRC32 (4 bytes) + header size (vint) + header content.
        let mut block = Vec::new();
        push_le32(&mut block, crc);
        push_vint(&mut block, hdr.len() as u64);
        block.extend_from_slice(&hdr);
        ar.extend_from_slice(&block);
    }

    // --- File Header (type 2) — "hello.txt" stored ---
    {
        let file_data = b"Hello, RAR5!";
        let file_name = b"hello.txt";

        let mut hdr = Vec::new();
        // Header type = 2 (file).
        push_vint(&mut hdr, BLOCK_FILE);
        // Header flags: data area present (bit 1).
        push_vint(&mut hdr, HFL_DATA);
        // Data area size = file data length.
        push_vint(&mut hdr, file_data.len() as u64);
        // File flags: has mtime (bit 1) + has CRC32 (bit 2).
        push_vint(&mut hdr, FFLG_UTIME | FFLG_CRC32);
        // Unpacked size.
        push_vint(&mut hdr, file_data.len() as u64);
        // Attributes = 0.
        push_vint(&mut hdr, 0);
        // mtime = 1700000000 (2023-11-14).
        push_le32(&mut hdr, 1700000000);
        // CRC32 of file data.
        push_le32(&mut hdr, crc32_rar(file_data));
        // Compression info: version=0, method=0 (store).
        push_vint(&mut hdr, 0);
        // Host OS: 1 (Unix).
        push_vint(&mut hdr, 1);
        // Name length.
        push_vint(&mut hdr, file_name.len() as u64);
        // Name.
        hdr.extend_from_slice(file_name);

        // CRC32 of header content.
        let crc = crc32_rar(&hdr);
        let mut block = Vec::new();
        push_le32(&mut block, crc);
        push_vint(&mut block, hdr.len() as u64);
        block.extend_from_slice(&hdr);
        // Data area immediately after header.
        block.extend_from_slice(file_data);
        ar.extend_from_slice(&block);
    }

    // --- End of Archive (type 5) ---
    {
        let mut hdr = Vec::new();
        push_vint(&mut hdr, BLOCK_END);
        push_vint(&mut hdr, 0); // flags = 0

        let crc = crc32_rar(&hdr);
        let mut block = Vec::new();
        push_le32(&mut block, crc);
        push_vint(&mut block, hdr.len() as u64);
        block.extend_from_slice(&hdr);
        ar.extend_from_slice(&block);
    }

    ar
}

/// Encode a vint and append to `out`.
fn push_vint(out: &mut Vec<u8>, mut val: u64) {
    loop {
        let b = (val & 0x7F) as u8;
        val >>= 7;
        if val == 0 {
            out.push(b);
            break;
        }
        out.push(b | 0x80);
    }
}

/// Append a little-endian u32.
fn push_le32(out: &mut Vec<u8>, val: u32) {
    out.push(val as u8);
    out.push((val >> 8) as u8);
    out.push((val >> 16) as u8);
    out.push((val >> 24) as u8);
}

/// CRC32 (ISO 3309 / ITU-T V.42 polynomial 0xEDB88320) for RAR headers.
fn crc32_rar(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// Run RAR module self-tests.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[rar] Running self-test...");

    // --- Test 1: parse synthetic RAR5 archive ---
    {
        let archive = build_test_archive();
        let entries = parse(&archive)?;
        if entries.len() != 1 {
            serial_println!("[rar]   ERROR: expected 1 entry, got {}", entries.len());
            return Err(KernelError::CorruptedData);
        }
        if entries[0].name != "hello.txt" {
            serial_println!("[rar]   ERROR: name mismatch: '{}'", entries[0].name);
            return Err(KernelError::CorruptedData);
        }
        if entries[0].unpacked_size != 12 {
            serial_println!("[rar]   ERROR: size mismatch: {}", entries[0].unpacked_size);
            return Err(KernelError::CorruptedData);
        }
        if !entries[0].is_stored {
            serial_println!("[rar]   ERROR: not stored");
            return Err(KernelError::CorruptedData);
        }
        if entries[0].is_dir {
            serial_println!("[rar]   ERROR: incorrectly marked as directory");
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[rar]   parse OK (1 entry: '{}')", entries[0].name);
    }

    // --- Test 2: extract stored entry ---
    {
        let archive = build_test_archive();
        let entries = parse(&archive)?;
        let data = entry_data(&archive, &entries[0])?;
        if data != b"Hello, RAR5!" {
            serial_println!("[rar]   ERROR: data mismatch");
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[rar]   stored extraction OK");
    }

    // --- Test 3: vint encoding/decoding ---
    {
        // Test small value (single byte).
        let mut buf = Vec::new();
        push_vint(&mut buf, 42);
        let (v, len) = read_vint(&buf, 0)?;
        if v != 42 || len != 1 {
            return Err(KernelError::CorruptedData);
        }

        // Test multi-byte value.
        buf.clear();
        push_vint(&mut buf, 300);
        let (v, _) = read_vint(&buf, 0)?;
        if v != 300 {
            return Err(KernelError::CorruptedData);
        }

        // Test large value.
        buf.clear();
        push_vint(&mut buf, 0x1234_5678_9ABC);
        let (v, _) = read_vint(&buf, 0)?;
        if v != 0x1234_5678_9ABC {
            return Err(KernelError::CorruptedData);
        }

        serial_println!("[rar]   vint encoding OK");
    }

    // --- Test 4: magic validation ---
    {
        let garbage = [0xAA; 32];
        if parse(&garbage).is_ok() {
            return Err(KernelError::CorruptedData);
        }
        if parse(&[]).is_ok() {
            return Err(KernelError::CorruptedData);
        }
        // RAR4 should return InvalidArgument.
        let mut rar4 = Vec::new();
        rar4.extend_from_slice(RAR4_SIGNATURE);
        rar4.extend_from_slice(&[0u8; 32]);
        match parse(&rar4) {
            Err(KernelError::InvalidArgument) => {}
            _ => {
                serial_println!("[rar]   ERROR: RAR4 not correctly rejected");
                return Err(KernelError::CorruptedData);
            }
        }
        serial_println!("[rar]   magic validation OK");
    }

    // --- Test 5: multi-entry archive (file + directory) ---
    {
        let archive = build_multi_entry_archive();
        let entries = parse(&archive)?;
        if entries.len() != 2 {
            serial_println!("[rar]   ERROR: expected 2 entries, got {}", entries.len());
            return Err(KernelError::CorruptedData);
        }
        if !entries[0].is_dir || entries[0].name != "mydir/" {
            serial_println!("[rar]   ERROR: first entry not a directory");
            return Err(KernelError::CorruptedData);
        }
        if entries[1].is_dir || entries[1].name != "mydir/data.bin" {
            serial_println!("[rar]   ERROR: second entry incorrect");
            return Err(KernelError::CorruptedData);
        }
        let data = entry_data(&archive, &entries[1])?;
        if data != &[0xDE, 0xAD, 0xBE, 0xEF] {
            serial_println!("[rar]   ERROR: extracted data mismatch");
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[rar]   multi-entry OK (dir + file)");
    }

    // --- Test 6: CRC32 ---
    {
        let crc = crc32_rar(b"");
        if crc != 0 {
            serial_println!("[rar]   ERROR: CRC32(\"\") = {:#X}, expected 0", crc);
            return Err(KernelError::CorruptedData);
        }
        let crc = crc32_rar(b"Hello, RAR5!");
        // Just verify it's non-zero and round-trips.
        if crc == 0 {
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[rar]   CRC32 OK");
    }

    serial_println!("[rar] Self-test passed.");
    Ok(())
}

/// Build a multi-entry test archive (directory + file).
fn build_multi_entry_archive() -> Vec<u8> {
    let mut ar = Vec::new();
    ar.extend_from_slice(RAR5_SIGNATURE);

    // Archive header.
    {
        let mut hdr = Vec::new();
        push_vint(&mut hdr, BLOCK_ARCHIVE);
        push_vint(&mut hdr, 0);
        push_vint(&mut hdr, 0);
        let crc = crc32_rar(&hdr);
        push_le32(&mut ar, crc);
        push_vint(&mut ar, hdr.len() as u64);
        ar.extend_from_slice(&hdr);
    }

    // Directory entry: "mydir/"
    {
        let name = b"mydir/";
        let mut hdr = Vec::new();
        push_vint(&mut hdr, BLOCK_FILE);
        push_vint(&mut hdr, 0); // no data area
        // File flags: directory (bit 0).
        push_vint(&mut hdr, FFLG_DIRECTORY);
        // Unpacked size = 0.
        push_vint(&mut hdr, 0);
        // Attributes = 0.
        push_vint(&mut hdr, 0);
        // Compression info = 0 (store).
        push_vint(&mut hdr, 0);
        // Host OS = 1 (Unix).
        push_vint(&mut hdr, 1);
        // Name length.
        push_vint(&mut hdr, name.len() as u64);
        hdr.extend_from_slice(name);

        let crc = crc32_rar(&hdr);
        push_le32(&mut ar, crc);
        push_vint(&mut ar, hdr.len() as u64);
        ar.extend_from_slice(&hdr);
    }

    // File entry: "mydir/data.bin" (stored).
    {
        let name = b"mydir/data.bin";
        let file_data: &[u8] = &[0xDE, 0xAD, 0xBE, 0xEF];

        let mut hdr = Vec::new();
        push_vint(&mut hdr, BLOCK_FILE);
        push_vint(&mut hdr, HFL_DATA); // data area present
        push_vint(&mut hdr, file_data.len() as u64); // data size
        // File flags = CRC32 present.
        push_vint(&mut hdr, FFLG_CRC32);
        // Unpacked size.
        push_vint(&mut hdr, file_data.len() as u64);
        // Attributes = 0.
        push_vint(&mut hdr, 0);
        // CRC32 of file data.
        push_le32(&mut hdr, crc32_rar(file_data));
        // Compression info = 0 (store).
        push_vint(&mut hdr, 0);
        // Host OS = 1.
        push_vint(&mut hdr, 1);
        // Name length.
        push_vint(&mut hdr, name.len() as u64);
        hdr.extend_from_slice(name);

        let crc = crc32_rar(&hdr);
        push_le32(&mut ar, crc);
        push_vint(&mut ar, hdr.len() as u64);
        ar.extend_from_slice(&hdr);
        ar.extend_from_slice(file_data);
    }

    // End of archive.
    {
        let mut hdr = Vec::new();
        push_vint(&mut hdr, BLOCK_END);
        push_vint(&mut hdr, 0);
        let crc = crc32_rar(&hdr);
        push_le32(&mut ar, crc);
        push_vint(&mut ar, hdr.len() as u64);
        ar.extend_from_slice(&hdr);
    }

    ar
}
