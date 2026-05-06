//! Unix `ar` archive format (read and write).
//!
//! Implements the common (GNU/SysV) variant of the `ar` archive format,
//! used for:
//! - Static libraries (`.a` files)
//! - Debian packages (`.deb` — outer container is `ar`)
//! - General-purpose archival
//!
//! ## Format overview
//!
//! ```text
//! "!<arch>\n"                      ← 8-byte global magic
//! ┌──────────────────────────────┐
//! │ 60-byte ASCII header         │ ← member name, timestamp, uid, gid,
//! │                              │   mode, size, magic "`\n"
//! ├──────────────────────────────┤
//! │ File data (size bytes)       │
//! │ (padded to 2-byte boundary)  │
//! └──────────────────────────────┘
//! ... more members ...
//! ```
//!
//! ## Header layout (60 bytes)
//!
//! ```text
//! Offset  Len  Field
//! 0       16   ar_name   (member name, space-padded, "/" terminated)
//! 16      12   ar_date   (decimal seconds since epoch, space-padded)
//! 28       6   ar_uid    (decimal, space-padded)
//! 34       6   ar_gid    (decimal, space-padded)
//! 40       8   ar_mode   (octal, space-padded)
//! 48      10   ar_size   (decimal bytes, space-padded)
//! 58       2   ar_fmag   ("`\n" — magic trailer)
//! ```
//!
//! ## Variants
//!
//! - GNU variant: long names stored in a special "//" member, referenced
//!   as "/offset" in ar_name.
//! - BSD variant: long names stored as "#1/len" with name prepended to data.
//!   (Not yet supported — GNU variant handles most real-world archives.)
//!
//! ## References
//!
//! - `man 5 ar`
//! - GNU binutils ar format
//! - Debian Policy Manual §22.2

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Global magic at the start of every ar archive.
const AR_MAGIC: &[u8; 8] = b"!<arch>\n";

/// Per-member header trailer (last 2 bytes of each 60-byte header).
const AR_FMAG: &[u8; 2] = b"`\n";

/// Header size in bytes.
const HEADER_SIZE: usize = 60;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// A single member extracted from an ar archive.
pub struct ArEntry {
    /// Member name (no trailing '/' or spaces).
    pub name: String,
    /// Member data.
    pub data: Vec<u8>,
    /// Modification time (Unix timestamp, 0 if not available).
    pub mtime: u64,
    /// User ID.
    pub uid: u32,
    /// Group ID.
    pub gid: u32,
    /// File mode (octal permissions).
    pub mode: u32,
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Parse a space-padded decimal ASCII field into u64.
fn parse_decimal(field: &[u8]) -> u64 {
    let s = core::str::from_utf8(field).unwrap_or("");
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return 0;
    }
    // Parse manually to avoid pulling in from_str_radix.
    let mut val = 0u64;
    for &b in trimmed.as_bytes() {
        if b < b'0' || b > b'9' {
            break;
        }
        val = val.wrapping_mul(10).wrapping_add(u64::from(b - b'0'));
    }
    val
}

/// Parse a space-padded octal ASCII field into u32.
fn parse_octal(field: &[u8]) -> u32 {
    let s = core::str::from_utf8(field).unwrap_or("");
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return 0;
    }
    let mut val = 0u32;
    for &b in trimmed.as_bytes() {
        if b < b'0' || b > b'7' {
            break;
        }
        val = val.wrapping_mul(8).wrapping_add(u32::from(b - b'0'));
    }
    val
}

/// Format a u64 as right-justified space-padded decimal in a fixed-width field.
fn format_decimal(val: u64, width: usize) -> Vec<u8> {
    let mut buf = vec![b' '; width];
    if val == 0 {
        if let Some(last) = buf.last_mut() {
            *last = b'0';
        }
        return buf;
    }
    let mut v = val;
    let mut pos = width;
    while v > 0 && pos > 0 {
        pos -= 1;
        buf[pos] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    buf
}

/// Format a u32 as right-justified space-padded octal in a fixed-width field.
fn format_octal(val: u32, width: usize) -> Vec<u8> {
    let mut buf = vec![b' '; width];
    if val == 0 {
        if let Some(last) = buf.last_mut() {
            *last = b'0';
        }
        return buf;
    }
    let mut v = val;
    let mut pos = width;
    while v > 0 && pos > 0 {
        pos -= 1;
        buf[pos] = b'0' + (v & 7) as u8;
        v >>= 3;
    }
    buf
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// Extract all members from an ar archive.
///
/// Returns a list of members.  Special members like "/" (symbol table)
/// and "//" (GNU long name table) are used internally and not included
/// in the output.
pub fn unar(data: &[u8]) -> KernelResult<Vec<ArEntry>> {
    // Verify global magic.
    if data.len() < 8 || data.get(..8) != Some(AR_MAGIC.as_slice()) {
        return Err(KernelError::CorruptedData);
    }

    let mut entries = Vec::new();
    let mut pos = 8usize; // Skip global magic.

    // GNU long name table (stored in the "//" member).
    let mut long_names: Vec<u8> = Vec::new();

    loop {
        if pos >= data.len() {
            break;
        }

        // Need at least a full header.
        if pos.wrapping_add(HEADER_SIZE) > data.len() {
            break;
        }

        let header = &data[pos..pos.wrapping_add(HEADER_SIZE)];

        // Verify per-member magic.
        if header.get(58..60) != Some(AR_FMAG.as_slice()) {
            return Err(KernelError::CorruptedData);
        }

        // Parse header fields.
        let name_field = &header[0..16];
        let date_field = &header[16..28];
        let uid_field = &header[28..34];
        let gid_field = &header[34..40];
        let mode_field = &header[40..48];
        let size_field = &header[48..58];

        let mtime = parse_decimal(date_field);
        let uid = parse_decimal(uid_field) as u32;
        let gid = parse_decimal(gid_field) as u32;
        let mode = parse_octal(mode_field);
        let size = parse_decimal(size_field) as usize;

        // Data starts after the header.
        let data_start = pos.wrapping_add(HEADER_SIZE);
        let data_end = data_start.wrapping_add(size);
        if data_end > data.len() {
            return Err(KernelError::CorruptedData);
        }

        let member_data = data.get(data_start..data_end)
            .ok_or(KernelError::CorruptedData)?;

        // Parse the member name.
        let raw_name = core::str::from_utf8(name_field).unwrap_or("");
        let raw_name = raw_name.trim_end();

        // Check for special members.
        if raw_name == "/" {
            // Symbol table — skip.
            pos = align2(data_end);
            continue;
        }

        if raw_name == "//" {
            // GNU long name table: stores long filenames separated by "/\n".
            long_names = member_data.to_vec();
            pos = align2(data_end);
            continue;
        }

        // Resolve the member name.
        let name = if raw_name.starts_with('/') && raw_name.len() > 1 {
            // GNU extended name: "/offset" references into the long name table.
            let offset_str = &raw_name[1..];
            let offset = parse_decimal(offset_str.as_bytes()) as usize;
            resolve_long_name(&long_names, offset)
        } else {
            // Regular name: strip trailing '/'.
            let n = raw_name.strip_suffix('/').unwrap_or(raw_name);
            String::from(n)
        };

        entries.push(ArEntry {
            name,
            data: member_data.to_vec(),
            mtime,
            uid,
            gid,
            mode,
        });

        // Next member starts after data, aligned to 2-byte boundary.
        pos = align2(data_end);
    }

    Ok(entries)
}

/// Resolve a GNU extended name from the "//" long name table.
///
/// Names in the table are terminated by "/\n".
fn resolve_long_name(table: &[u8], offset: usize) -> String {
    if offset >= table.len() {
        return String::from("???");
    }

    let remaining = &table[offset..];
    // Find the "/\n" terminator.
    let end = remaining.windows(2)
        .position(|w| w == b"/\n")
        .unwrap_or(remaining.len());

    let name_bytes = &remaining[..end];
    String::from(core::str::from_utf8(name_bytes).unwrap_or("???"))
}

/// Round up to 2-byte boundary.
fn align2(offset: usize) -> usize {
    (offset.wrapping_add(1)) & !1
}

// ---------------------------------------------------------------------------
// Archive creation
// ---------------------------------------------------------------------------

/// Create an ar archive from a list of members.
///
/// Produces a byte stream with the global "!<arch>\n" magic followed by
/// member headers and data.
pub fn mkar(entries: &[ArEntry]) -> KernelResult<Vec<u8>> {
    let mut buf = Vec::new();

    // Global magic.
    buf.extend_from_slice(AR_MAGIC);

    // Check if any names are longer than 15 chars (need GNU long name table).
    let needs_long_names = entries.iter().any(|e| e.name.len() > 15);

    let mut long_name_table = Vec::new();
    let mut name_offsets: Vec<Option<usize>> = Vec::new();

    if needs_long_names {
        // Build the long name table.
        for entry in entries {
            if entry.name.len() > 15 {
                let offset = long_name_table.len();
                name_offsets.push(Some(offset));
                long_name_table.extend_from_slice(entry.name.as_bytes());
                long_name_table.extend_from_slice(b"/\n");
            } else {
                name_offsets.push(None);
            }
        }

        // Write the "//" member containing the long name table.
        write_member_header(
            &mut buf,
            b"//",
            0, 0, 0, 0,
            long_name_table.len(),
        );
        buf.extend_from_slice(&long_name_table);
        // Pad to 2-byte boundary.
        if long_name_table.len() % 2 != 0 {
            buf.push(b'\n');
        }
    } else {
        for _ in entries {
            name_offsets.push(None);
        }
    }

    // Write each member.
    for (i, entry) in entries.iter().enumerate() {
        let name_bytes: Vec<u8> = match name_offsets.get(i).copied().flatten() {
            Some(offset) => {
                // GNU extended name reference: "/offset".
                let mut n = vec![b'/'];
                let offset_str = format_decimal(offset as u64, 14);
                n.extend_from_slice(&offset_str);
                // Trim trailing spaces for the header (will be space-padded by write_member_header).
                while n.last() == Some(&b' ') {
                    n.pop();
                }
                n
            }
            None => {
                // Short name with trailing '/'.
                let mut n = Vec::from(entry.name.as_bytes());
                n.push(b'/');
                n
            }
        };

        write_member_header(
            &mut buf,
            &name_bytes,
            entry.mtime,
            entry.uid,
            entry.gid,
            entry.mode,
            entry.data.len(),
        );

        buf.extend_from_slice(&entry.data);

        // Pad to 2-byte boundary.
        if entry.data.len() % 2 != 0 {
            buf.push(b'\n');
        }
    }

    Ok(buf)
}

/// Write a 60-byte ar member header.
fn write_member_header(
    buf: &mut Vec<u8>,
    name: &[u8],
    mtime: u64,
    uid: u32,
    gid: u32,
    mode: u32,
    size: usize,
) {
    // ar_name: 16 bytes, space-padded.
    let mut name_field = [b' '; 16];
    let copy_len = name.len().min(16);
    name_field[..copy_len].copy_from_slice(&name[..copy_len]);
    buf.extend_from_slice(&name_field);

    // ar_date: 12 bytes decimal.
    buf.extend_from_slice(&format_decimal(mtime, 12));

    // ar_uid: 6 bytes decimal.
    buf.extend_from_slice(&format_decimal(u64::from(uid), 6));

    // ar_gid: 6 bytes decimal.
    buf.extend_from_slice(&format_decimal(u64::from(gid), 6));

    // ar_mode: 8 bytes octal.
    buf.extend_from_slice(&format_octal(mode, 8));

    // ar_size: 10 bytes decimal.
    buf.extend_from_slice(&format_decimal(size as u64, 10));

    // ar_fmag: 2 bytes.
    buf.extend_from_slice(AR_FMAG);
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for ar archive parsing and creation.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[ar] Running self-test...");

    // Test 1: decimal and octal parsing.
    test_parsing()?;

    // Test 2: round-trip (create + extract).
    test_roundtrip()?;

    // Test 3: long names (GNU extended names).
    test_long_names()?;

    // Test 4: magic detection.
    test_magic()?;

    crate::serial_println!("[ar] Self-test passed.");
    Ok(())
}

fn test_parsing() -> KernelResult<()> {
    // Decimal parsing.
    if parse_decimal(b"42          ") != 42 {
        crate::serial_println!("[ar]   FAIL: parse_decimal(42)");
        return Err(KernelError::InternalError);
    }
    if parse_decimal(b"0           ") != 0 {
        crate::serial_println!("[ar]   FAIL: parse_decimal(0)");
        return Err(KernelError::InternalError);
    }
    if parse_decimal(b"            ") != 0 {
        crate::serial_println!("[ar]   FAIL: parse_decimal(empty)");
        return Err(KernelError::InternalError);
    }

    // Octal parsing.
    if parse_octal(b"100644  ") != 0o100644 {
        crate::serial_println!("[ar]   FAIL: parse_octal(100644) = {}", parse_octal(b"100644  "));
        return Err(KernelError::InternalError);
    }

    // format_decimal round-trip.
    let formatted = format_decimal(12345, 12);
    let reparsed = parse_decimal(&formatted);
    if reparsed != 12345 {
        crate::serial_println!("[ar]   FAIL: format_decimal round-trip: {}", reparsed);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ar]   parsing OK");
    Ok(())
}

fn test_roundtrip() -> KernelResult<()> {
    let entries = vec![
        ArEntry {
            name: String::from("hello.txt"),
            data: Vec::from(*b"Hello, ar!\n"),
            mtime: 1700000000,
            uid: 1000,
            gid: 1000,
            mode: 0o100644,
        },
        ArEntry {
            name: String::from("empty"),
            data: Vec::new(),
            mtime: 0,
            uid: 0,
            gid: 0,
            mode: 0o100644,
        },
    ];

    let archive = mkar(&entries)?;

    // Verify it starts with the right magic.
    if archive.get(..8) != Some(AR_MAGIC.as_slice()) {
        crate::serial_println!("[ar]   FAIL: archive doesn't start with ar magic");
        return Err(KernelError::InternalError);
    }

    // Extract and verify.
    let extracted = unar(&archive)?;
    if extracted.len() != 2 {
        crate::serial_println!("[ar]   FAIL: expected 2 entries, got {}", extracted.len());
        return Err(KernelError::InternalError);
    }

    if extracted[0].name != "hello.txt" {
        crate::serial_println!("[ar]   FAIL: entry 0 name: '{}'", extracted[0].name);
        return Err(KernelError::InternalError);
    }
    if extracted[0].data != b"Hello, ar!\n" {
        crate::serial_println!("[ar]   FAIL: entry 0 data mismatch");
        return Err(KernelError::InternalError);
    }
    if extracted[0].uid != 1000 || extracted[0].gid != 1000 {
        crate::serial_println!("[ar]   FAIL: entry 0 uid/gid");
        return Err(KernelError::InternalError);
    }

    if extracted[1].name != "empty" {
        crate::serial_println!("[ar]   FAIL: entry 1 name: '{}'", extracted[1].name);
        return Err(KernelError::InternalError);
    }
    if !extracted[1].data.is_empty() {
        crate::serial_println!("[ar]   FAIL: entry 1 should be empty");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ar]   round-trip OK (2 entries)");
    Ok(())
}

fn test_long_names() -> KernelResult<()> {
    // Create entries with names > 15 chars to exercise GNU extended names.
    let entries = vec![
        ArEntry {
            name: String::from("short.txt"),
            data: Vec::from(*b"short"),
            mtime: 0,
            uid: 0,
            gid: 0,
            mode: 0o100644,
        },
        ArEntry {
            name: String::from("this_is_a_very_long_filename.txt"),
            data: Vec::from(*b"long name data"),
            mtime: 0,
            uid: 0,
            gid: 0,
            mode: 0o100644,
        },
        ArEntry {
            name: String::from("another_extremely_long_name_here.rs"),
            data: Vec::from(*b"more data"),
            mtime: 0,
            uid: 0,
            gid: 0,
            mode: 0o100644,
        },
    ];

    let archive = mkar(&entries)?;
    let extracted = unar(&archive)?;

    if extracted.len() != 3 {
        crate::serial_println!("[ar]   FAIL: long names: expected 3 entries, got {}", extracted.len());
        return Err(KernelError::InternalError);
    }

    if extracted[0].name != "short.txt" {
        crate::serial_println!("[ar]   FAIL: entry 0: '{}'", extracted[0].name);
        return Err(KernelError::InternalError);
    }

    if extracted[1].name != "this_is_a_very_long_filename.txt" {
        crate::serial_println!("[ar]   FAIL: entry 1: '{}'", extracted[1].name);
        return Err(KernelError::InternalError);
    }

    if extracted[2].name != "another_extremely_long_name_here.rs" {
        crate::serial_println!("[ar]   FAIL: entry 2: '{}'", extracted[2].name);
        return Err(KernelError::InternalError);
    }

    // Verify data preserved.
    if extracted[1].data != b"long name data" {
        crate::serial_println!("[ar]   FAIL: entry 1 data mismatch");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ar]   GNU long names OK (3 entries, 2 extended)");
    Ok(())
}

fn test_magic() -> KernelResult<()> {
    // Too-short data should fail.
    if unar(&[0x21]).is_ok() {
        crate::serial_println!("[ar]   FAIL: should reject short data");
        return Err(KernelError::InternalError);
    }

    // Wrong magic should fail.
    if unar(b"NOTANAR\n").is_ok() {
        crate::serial_println!("[ar]   FAIL: should reject bad magic");
        return Err(KernelError::InternalError);
    }

    // Empty archive (just magic) should succeed with 0 entries.
    let empty = unar(AR_MAGIC)?;
    if !empty.is_empty() {
        crate::serial_println!("[ar]   FAIL: empty archive should have 0 entries");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ar]   magic detection OK");
    Ok(())
}
