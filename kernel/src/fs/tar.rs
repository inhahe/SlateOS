//! USTAR (Unix Standard TAR) archive support (read and write).
//!
//! Implements parsing, extraction, and creation of tar archives in the
//! USTAR format (POSIX.1-1988 / IEEE Std 1003.1).  The command-level
//! compression handling (gzip, bzip2, xz, zstd) is done in kshell;
//! this module handles only the raw tar container format.
//!
//! ## Format overview
//!
//! A tar archive is a sequence of 512-byte blocks:
//!
//! ```text
//! ┌──────────────────────────────────┐
//! │ 512-byte USTAR header #1        │  ← name, size, mode, type, checksum
//! ├──────────────────────────────────┤
//! │ File data (padded to 512 bytes)  │
//! ├──────────────────────────────────┤
//! │ 512-byte USTAR header #2        │
//! │ File data ...                    │
//! ├──────────────────────────────────┤
//! │ Two 512-byte zero blocks         │  ← end-of-archive marker
//! └──────────────────────────────────┘
//! ```
//!
//! ## USTAR header layout (512 bytes)
//!
//! ```text
//! Offset  Len   Field
//! 0       100   name (NUL-terminated)
//! 100     8     mode (octal, NUL-terminated)
//! 108     8     uid (octal)
//! 116     8     gid (octal)
//! 124     12    size (octal)
//! 136     12    mtime (octal, seconds since epoch)
//! 148     8     checksum (octal, spaces during computation)
//! 156     1     typeflag ('0'=file, '5'=dir, '2'=symlink, ...)
//! 157     100   linkname (NUL-terminated)
//! 257     6     magic ("ustar\0")
//! 263     2     version ("00")
//! 265     32    uname
//! 297     32    gname
//! 329     8     devmajor
//! 337     8     devminor
//! 345     155   prefix (for paths > 100 bytes)
//! 500     12    (padding)
//! ```
//!
//! ## Supported entry types
//!
//! | Flag | Type          | Support |
//! |------|---------------|---------|
//! | '0'  | Regular file  | Full    |
//! | '\0' | Regular file  | Full (pre-POSIX compat) |
//! | '5'  | Directory     | Full    |
//! | '2'  | Symlink       | Read (link target preserved) |
//!
//! ## References
//!
//! - POSIX.1-1988, Section 10.1 (USTAR format)
//! - GNU tar info pages
//! - <https://www.gnu.org/software/tar/manual/html_node/Standard.html>

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Size of one tar block (header or data block).
pub const BLOCK_SIZE: usize = 512;

/// USTAR magic string.
const MAGIC: &[u8; 6] = b"ustar\0";
/// USTAR version.
const VERSION: &[u8; 2] = b"00";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Type flag for tar entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Regular file ('0' or '\0').
    File,
    /// Directory ('5').
    Directory,
    /// Symbolic link ('2').
    Symlink,
    /// Other/unknown type flag.
    Other(u8),
}

impl EntryKind {
    /// Convert from raw typeflag byte.
    fn from_flag(flag: u8) -> Self {
        match flag {
            b'0' | 0 => Self::File,
            b'5' => Self::Directory,
            b'2' => Self::Symlink,
            other => Self::Other(other),
        }
    }

    /// Convert to raw typeflag byte.
    fn to_flag(self) -> u8 {
        match self {
            Self::File => b'0',
            Self::Directory => b'5',
            Self::Symlink => b'2',
            Self::Other(b) => b,
        }
    }
}

/// A parsed tar archive entry.
#[derive(Debug, Clone)]
pub struct TarEntry {
    /// Full path (prefix + "/" + name if prefix is present).
    pub name: String,
    /// File size in bytes (0 for directories and symlinks).
    pub size: u64,
    /// Modification time in seconds since Unix epoch.
    pub mtime: u64,
    /// File mode / permissions (octal).
    pub mode: u32,
    /// Owner user ID.
    pub uid: u32,
    /// Owner group ID.
    pub gid: u32,
    /// Entry type.
    pub kind: EntryKind,
    /// Symlink target (empty for non-symlinks).
    pub link_target: String,
    /// Byte offset of the file data within the archive.
    /// Points to the first byte after the header block.
    pub data_offset: usize,
}

/// An entry to be written into a new tar archive.
pub struct TarWriteEntry {
    /// Path inside the archive (directories should end with `/`).
    pub name: String,
    /// File data.  Empty for directories and symlinks.
    pub data: Vec<u8>,
    /// Entry type.
    pub kind: EntryKind,
    /// Symlink target (only used when kind == Symlink).
    pub link_target: String,
    /// File mode (permissions).
    pub mode: u32,
    /// Owner UID.
    pub uid: u32,
    /// Owner GID.
    pub gid: u32,
    /// Modification time (seconds since epoch).
    pub mtime: u64,
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Parse an octal ASCII field (NUL/space terminated) into u64.
#[allow(clippy::arithmetic_side_effects)]
fn parse_octal(field: &[u8]) -> u64 {
    let mut val: u64 = 0;
    for &b in field {
        if b == 0 || b == b' ' {
            break;
        }
        if b >= b'0' && b <= b'7' {
            val = val.wrapping_mul(8).wrapping_add(u64::from(b.wrapping_sub(b'0')));
        }
    }
    val
}

/// Write an octal string into a buffer with NUL terminator.
///
/// The field is zero-padded to fill `buf.len() - 1` octal digits, then
/// NUL terminated.  E.g., for an 8-byte field: `"0000644\0"`.
#[allow(clippy::arithmetic_side_effects)]
fn write_octal(buf: &mut [u8], val: u64) {
    // Use explicit formatting instead of width$ named parameter
    // to avoid potential issues in no_std alloc::format.
    let digits = buf.len().saturating_sub(1);
    let s = match digits {
        7 => alloc::format!("{:07o}\0", val),
        11 => alloc::format!("{:011o}\0", val),
        _ => {
            // Generic fallback: manual zero-pad.
            let raw = alloc::format!("{:o}", val);
            let pad = digits.saturating_sub(raw.len());
            let mut out = alloc::string::String::with_capacity(digits.wrapping_add(1));
            for _ in 0..pad {
                out.push('0');
            }
            out.push_str(&raw);
            out.push('\0');
            out
        }
    };
    let bytes = s.as_bytes();
    let copy_len = bytes.len().min(buf.len());
    buf[..copy_len].copy_from_slice(&bytes[..copy_len]);
}

// ---------------------------------------------------------------------------
// Public API — parse
// ---------------------------------------------------------------------------

/// Parse a tar archive and return a list of all entries.
///
/// The returned entries reference data within the original `data` slice
/// via `data_offset` and `size`.
///
/// # Errors
///
/// Returns `CorruptedData` if the archive is too small or has invalid
/// checksums.
pub fn parse(data: &[u8]) -> KernelResult<Vec<TarEntry>> {
    if data.len() < BLOCK_SIZE {
        return Err(KernelError::CorruptedData);
    }

    let mut entries = Vec::new();
    let mut offset: usize = 0;

    while offset.wrapping_add(BLOCK_SIZE) <= data.len() {
        let header = &data[offset..offset.wrapping_add(BLOCK_SIZE)];

        // End-of-archive: all-zero block.
        if header.iter().all(|&b| b == 0) {
            break;
        }

        // Verify checksum.
        let stored_cksum = parse_octal(&header[148..156]);
        let mut computed: u32 = 0;
        for (i, &b) in header.iter().enumerate() {
            if (148..156).contains(&i) {
                computed = computed.wrapping_add(u32::from(b' '));
            } else {
                computed = computed.wrapping_add(u32::from(b));
            }
        }
        if stored_cksum != u64::from(computed) {
            return Err(KernelError::CorruptedData);
        }

        // Parse name (prefix + name).
        let name_raw = &header[..100];
        let name_end = name_raw.iter().position(|&b| b == 0).unwrap_or(100);
        let name_part = core::str::from_utf8(&name_raw[..name_end]).unwrap_or("");

        let prefix_raw = &header[345..500];
        let prefix_end = prefix_raw.iter().position(|&b| b == 0).unwrap_or(155);
        let prefix_part = core::str::from_utf8(&prefix_raw[..prefix_end]).unwrap_or("");

        let name = if prefix_part.is_empty() {
            String::from(name_part)
        } else {
            alloc::format!("{}/{}", prefix_part, name_part)
        };

        let size = parse_octal(&header[124..136]);
        let mtime = parse_octal(&header[136..148]);
        let mode = parse_octal(&header[100..108]) as u32;
        let uid = parse_octal(&header[108..116]) as u32;
        let gid = parse_octal(&header[116..124]) as u32;
        let typeflag = header[156];

        let link_raw = &header[157..257];
        let link_end = link_raw.iter().position(|&b| b == 0).unwrap_or(100);
        let link_target = String::from(
            core::str::from_utf8(&link_raw[..link_end]).unwrap_or(""),
        );

        let data_offset = offset.wrapping_add(BLOCK_SIZE);

        entries.push(TarEntry {
            name,
            size,
            mtime,
            mode,
            uid,
            gid,
            kind: EntryKind::from_flag(typeflag),
            link_target,
            data_offset,
        });

        // Skip past data blocks.
        let data_blocks = if size > 0 {
            (size as usize).wrapping_add(BLOCK_SIZE.wrapping_sub(1)) / BLOCK_SIZE
        } else {
            0
        };
        offset = data_offset.wrapping_add(data_blocks.wrapping_mul(BLOCK_SIZE));
    }

    Ok(entries)
}

/// Extract file data for one entry from the archive.
///
/// Returns a slice of the uncompressed file data within the archive.
pub fn entry_data<'a>(data: &'a [u8], entry: &TarEntry) -> KernelResult<&'a [u8]> {
    let end = entry.data_offset.wrapping_add(entry.size as usize);
    data.get(entry.data_offset..end.min(data.len()))
        .ok_or(KernelError::CorruptedData)
}

// ---------------------------------------------------------------------------
// Public API — create
// ---------------------------------------------------------------------------

/// Build a USTAR header block for a single entry.
///
/// Returns a 512-byte header with checksum computed.
#[allow(clippy::arithmetic_side_effects)]
pub fn build_header(entry: &TarWriteEntry) -> [u8; BLOCK_SIZE] {
    let mut header = [0u8; BLOCK_SIZE];

    // Write name (split into prefix + name if needed).
    let path_bytes = entry.name.as_bytes();
    if path_bytes.len() <= 100 {
        let copy_len = path_bytes.len().min(100);
        header[..copy_len].copy_from_slice(&path_bytes[..copy_len]);
    } else {
        // Split at '/' boundary for USTAR prefix support.
        let mut split_at = None;
        for i in (0..path_bytes.len().saturating_sub(100)).rev() {
            if path_bytes[i] == b'/' {
                split_at = Some(i);
                break;
            }
        }
        if let Some(s) = split_at {
            let prefix = &path_bytes[..s];
            let name = &path_bytes[s + 1..];
            let plen = prefix.len().min(155);
            let nlen = name.len().min(100);
            header[..nlen].copy_from_slice(&name[..nlen]);
            header[345..345 + plen].copy_from_slice(&prefix[..plen]);
        } else {
            header[..100].copy_from_slice(&path_bytes[..100]);
        }
    }

    // Mode.
    write_octal(&mut header[100..108], u64::from(entry.mode & 0o7777));
    // UID.
    write_octal(&mut header[108..116], u64::from(entry.uid));
    // GID.
    write_octal(&mut header[116..124], u64::from(entry.gid));
    // Size.
    write_octal(&mut header[124..136], entry.data.len() as u64);
    // Mtime.
    write_octal(&mut header[136..148], entry.mtime);

    // Typeflag.
    header[156] = entry.kind.to_flag();

    // Linkname.
    if !entry.link_target.is_empty() {
        let lbytes = entry.link_target.as_bytes();
        let llen = lbytes.len().min(100);
        header[157..157 + llen].copy_from_slice(&lbytes[..llen]);
    }

    // Magic + version.
    header[257..263].copy_from_slice(MAGIC);
    header[263..265].copy_from_slice(VERSION);

    // Checksum: fill with spaces first, then compute.
    header[148..156].copy_from_slice(b"        ");

    let mut cksum: u32 = 0;
    for &b in header.iter() {
        cksum = cksum.wrapping_add(u32::from(b));
    }
    let cksum_str = alloc::format!("{:06o}\0 ", cksum);
    let cksum_bytes = cksum_str.as_bytes();
    let clen = cksum_bytes.len().min(8);
    header[148..148 + clen].copy_from_slice(&cksum_bytes[..clen]);

    header
}

/// Create a tar archive in memory from a list of entries.
///
/// Produces a valid USTAR archive terminated by two zero blocks.
#[allow(clippy::arithmetic_side_effects)]
pub fn create(entries: &[TarWriteEntry]) -> Vec<u8> {
    let mut archive = Vec::new();

    for entry in entries {
        let header = build_header(entry);
        archive.extend_from_slice(&header);

        if !entry.data.is_empty() {
            archive.extend_from_slice(&entry.data);
            // Pad to 512-byte boundary.
            let remainder = entry.data.len() % BLOCK_SIZE;
            if remainder != 0 {
                let padding = BLOCK_SIZE.wrapping_sub(remainder);
                archive.extend_from_slice(&vec![0u8; padding]);
            }
        }
    }

    // End-of-archive: two zero blocks.
    archive.extend_from_slice(&[0u8; BLOCK_SIZE]);
    archive.extend_from_slice(&[0u8; BLOCK_SIZE]);

    archive
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run tar module self-tests.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[tar] Running self-test...");

    // --- Test 1: round-trip with a regular file ---
    {
        let entries = vec![TarWriteEntry {
            name: String::from("hello.txt"),
            data: b"Hello, world!".to_vec(),
            kind: EntryKind::File,
            link_target: String::new(),
            mode: 0o644,
            uid: 1000,
            gid: 1000,
            mtime: 1700000000,
        }];
        let archive = create(&entries);
        let parsed = parse(&archive)?;
        if parsed.len() != 1 {
            return Err(KernelError::CorruptedData);
        }
        if parsed[0].name != "hello.txt" {
            return Err(KernelError::CorruptedData);
        }
        if parsed[0].kind != EntryKind::File {
            return Err(KernelError::CorruptedData);
        }
        if parsed[0].size != 13 {
            return Err(KernelError::CorruptedData);
        }
        if parsed[0].mode != 0o644 {
            return Err(KernelError::CorruptedData);
        }
        let data = entry_data(&archive, &parsed[0])?;
        if data != b"Hello, world!" {
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[tar]   file round-trip OK");
    }

    // --- Test 2: directory + file + symlink ---
    {
        let entries = vec![
            TarWriteEntry {
                name: String::from("mydir/"),
                data: Vec::new(),
                kind: EntryKind::Directory,
                link_target: String::new(),
                mode: 0o755,
                uid: 0,
                gid: 0,
                mtime: 1700000000,
            },
            TarWriteEntry {
                name: String::from("mydir/data.bin"),
                data: vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x11, 0x22, 0x33],
                kind: EntryKind::File,
                link_target: String::new(),
                mode: 0o600,
                uid: 0,
                gid: 0,
                mtime: 1700000000,
            },
            TarWriteEntry {
                name: String::from("link"),
                data: Vec::new(),
                kind: EntryKind::Symlink,
                link_target: String::from("mydir/data.bin"),
                mode: 0o777,
                uid: 0,
                gid: 0,
                mtime: 1700000000,
            },
        ];
        let archive = create(&entries);
        let parsed = parse(&archive)?;
        if parsed.len() != 3 {
            return Err(KernelError::CorruptedData);
        }
        if parsed[0].kind != EntryKind::Directory || parsed[0].name != "mydir/" {
            return Err(KernelError::CorruptedData);
        }
        if parsed[1].kind != EntryKind::File || parsed[1].size != 8 {
            return Err(KernelError::CorruptedData);
        }
        if parsed[2].kind != EntryKind::Symlink || parsed[2].link_target != "mydir/data.bin" {
            return Err(KernelError::CorruptedData);
        }
        let data = entry_data(&archive, &parsed[1])?;
        if data != [0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x11, 0x22, 0x33] {
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[tar]   multi-entry round-trip OK (dir + file + symlink)");
    }

    // --- Test 3: empty archive ---
    {
        let entries: Vec<TarWriteEntry> = Vec::new();
        let archive = create(&entries);
        let parsed = parse(&archive)?;
        if !parsed.is_empty() {
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[tar]   empty archive OK");
    }

    // --- Test 4: checksum validation ---
    {
        let entries = vec![TarWriteEntry {
            name: String::from("test.dat"),
            data: b"checksum test".to_vec(),
            kind: EntryKind::File,
            link_target: String::new(),
            mode: 0o644,
            uid: 0,
            gid: 0,
            mtime: 0,
        }];
        let mut archive = create(&entries);

        // Corrupt the header (flip a byte in the name field).
        if let Some(byte) = archive.get_mut(0) {
            *byte ^= 0xFF;
        }
        match parse(&archive) {
            Err(KernelError::CorruptedData) => {}
            Ok(_) => {
                serial_println!("[tar]   ERROR: corruption not detected");
                return Err(KernelError::CorruptedData);
            }
            Err(e) => return Err(e),
        }
        serial_println!("[tar]   checksum validation OK");
    }

    // --- Test 5: magic validation ---
    {
        let garbage = [0xAA; 64];
        if parse(&garbage).is_ok() {
            return Err(KernelError::CorruptedData);
        }
        if parse(&[]).is_ok() {
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[tar]   magic validation OK");
    }

    // --- Test 6: uid/gid/mtime preservation ---
    {
        let entries = vec![TarWriteEntry {
            name: String::from("owned.txt"),
            data: b"data".to_vec(),
            kind: EntryKind::File,
            link_target: String::new(),
            mode: 0o755,
            uid: 1234,
            gid: 5678,
            mtime: 1609459200, // 2021-01-01 00:00:00 UTC
        }];
        let archive = create(&entries);
        let parsed = parse(&archive)?;
        if parsed.len() != 1 {
            return Err(KernelError::CorruptedData);
        }
        if parsed[0].uid != 1234 || parsed[0].gid != 5678 {
            serial_println!("[tar]   uid/gid mismatch: {}/{}", parsed[0].uid, parsed[0].gid);
            return Err(KernelError::CorruptedData);
        }
        if parsed[0].mtime != 1609459200 {
            serial_println!("[tar]   mtime mismatch: {}", parsed[0].mtime);
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[tar]   metadata preservation OK");
    }

    serial_println!("[tar] Self-test passed.");
    Ok(())
}
