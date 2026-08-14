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

use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
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

/// Sentinel name GNU tar gives its long-name / long-link records.
const GNU_LONGLINK_NAME: &[u8] = b"././@LongLink";
/// GNU typeflag: the record's data is the *name* of the next entry.
const LONGNAME_FLAG: u8 = b'L';
/// GNU typeflag: the record's data is the *link target* of the next entry.
const LONGLINK_FLAG: u8 = b'K';

/// Upper bound on a `@LongLink` payload, in bytes.
///
/// The record's size field is attacker-controlled, and the payload is copied
/// into a `Vec` before the entry it describes is even seen.  Without a cap, a
/// 12-digit octal size in a hand-crafted archive would ask the kernel heap for
/// gigabytes.  16 KiB is two orders of magnitude past any real path (Linux's
/// `PATH_MAX` is 4096) while bounding the allocation.
const MAX_LONG_FIELD: u64 = 16 * 1024;

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
    ///
    /// The ustar `name`/`prefix` fields are raw bytes, and the paths they
    /// name are byte strings.  Decoding them as UTF-8 made every member whose
    /// name contains a non-UTF-8 byte unpackable — it parsed to the empty
    /// name and then either collided with another entry or extracted to the
    /// archive root.
    pub name: PathBuf,
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
    pub link_target: PathBuf,
    /// Byte offset of the file data within the archive.
    /// Points to the first byte after the header block.
    pub data_offset: usize,
}

/// An entry to be written into a new tar archive.
pub struct TarWriteEntry {
    /// Path inside the archive (directories should end with `/`).
    pub name: PathBuf,
    /// File data.  Empty for directories and symlinks.
    pub data: Vec<u8>,
    /// Entry type.
    pub kind: EntryKind,
    /// Symlink target (only used when kind == Symlink).
    pub link_target: PathBuf,
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

    // Pending GNU `@LongLink` overrides, consumed by the next real header.
    let mut long_name: Option<PathBuf> = None;
    let mut long_link: Option<PathBuf> = None;

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

        // Parse name (prefix + name).  Both fields are raw NUL-padded bytes;
        // never decode them as UTF-8.
        let name_raw = &header[..100];
        let name_end = name_raw.iter().position(|&b| b == 0).unwrap_or(100);
        let name_part = &name_raw[..name_end];

        let prefix_raw = &header[345..500];
        let prefix_end = prefix_raw.iter().position(|&b| b == 0).unwrap_or(155);
        let prefix_part = &prefix_raw[..prefix_end];

        // Concatenated explicitly rather than via `Path::join`: ustar defines
        // the full name as `prefix + "/" + name`, and `join` would discard the
        // prefix for a member name that begins with `/`.  Path *safety* (`..`,
        // absolute names) is the extractor's job, not the parser's.
        let name = if prefix_part.is_empty() {
            PathBuf::from(name_part)
        } else {
            let mut n =
                PathBuf::with_capacity(prefix_part.len().saturating_add(name_part.len()).saturating_add(1));
            n.extend_bytes(prefix_part);
            n.extend_bytes(b"/");
            n.extend_bytes(name_part);
            n
        };

        let size = parse_octal(&header[124..136]);
        let mtime = parse_octal(&header[136..148]);
        let mode = parse_octal(&header[100..108]) as u32;
        let uid = parse_octal(&header[108..116]) as u32;
        let gid = parse_octal(&header[116..124]) as u32;
        let typeflag = header[156];

        let link_raw = &header[157..257];
        let link_end = link_raw.iter().position(|&b| b == 0).unwrap_or(100);
        let link_target = PathBuf::from(&link_raw[..link_end]);

        let data_offset = offset.wrapping_add(BLOCK_SIZE);
        let data_blocks = if size > 0 {
            (size as usize).wrapping_add(BLOCK_SIZE.wrapping_sub(1)) / BLOCK_SIZE
        } else {
            0
        };
        let next_offset = data_offset.wrapping_add(data_blocks.wrapping_mul(BLOCK_SIZE));

        // GNU long-name / long-link records are not entries in their own
        // right: their payload is the name (or link target) of the header that
        // follows, which could not fit in the fixed ustar fields.  Stash it
        // and move on without emitting anything.
        if typeflag == LONGNAME_FLAG || typeflag == LONGLINK_FLAG {
            if size > MAX_LONG_FIELD {
                return Err(KernelError::CorruptedData);
            }
            let end = data_offset.wrapping_add(size as usize);
            let payload = data.get(data_offset..end).ok_or(KernelError::CorruptedData)?;
            // The payload is NUL-terminated; keep only what precedes the NUL
            // (a truncated writer may omit it, hence the `unwrap_or`).
            let stop = payload.iter().position(|&b| b == 0).unwrap_or(payload.len());
            let value = PathBuf::from(payload.get(..stop).unwrap_or(payload));
            if typeflag == LONGNAME_FLAG {
                long_name = Some(value);
            } else {
                long_link = Some(value);
            }
            offset = next_offset;
            continue;
        }

        entries.push(TarEntry {
            // `take` rather than `clone`: an override applies to exactly one
            // entry, so a stray record cannot rename every member after it.
            name: long_name.take().unwrap_or(name),
            size,
            mtime,
            mode,
            uid,
            gid,
            kind: EntryKind::from_flag(typeflag),
            link_target: long_link.take().unwrap_or(link_target),
            data_offset,
        });

        offset = next_offset;
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
/// Split `path` into the ustar `prefix` and `name` header fields.
///
/// ustar stores a long member name as `prefix` (155 bytes) + `/` + `name`
/// (100 bytes), so the split must land **on a `/`** and leave at most 100
/// bytes after it and at most 155 before it.  Returns `None` when no such
/// split point exists — a single component longer than 100 bytes, or a path
/// whose only separators are too early — in which case the caller must fall
/// back to the GNU long-name record.
///
/// The valid split indices are exactly `len - 101 ..= 155`: any earlier `/`
/// leaves more than 100 bytes for `name`, any later one overflows `prefix`.
/// Scanning that window forwards picks the shortest legal prefix, i.e. keeps
/// as much of the path as possible in the field every tar reader understands.
///
/// The previous implementation scanned `0 .. len - 100` and took the *last*
/// hit, which is the complementary set — every index it could return left
/// `name` longer than 100 bytes, and the surplus was then silently chopped by
/// a `min(100)`.  A 140-byte member name round-tripped as a 133-byte one.
fn split_ustar_name(path: &[u8]) -> Option<(&[u8], &[u8])> {
    // `name` must be non-empty, so the split cannot be the final byte.
    let last = path.len().checked_sub(1)?;
    let lo = path.len().saturating_sub(101);
    let hi = 155.min(last);
    for i in lo..=hi {
        if path.get(i) == Some(&b'/') {
            let prefix = path.get(..i)?;
            let name = path.get(i.saturating_add(1)..)?;
            // A leading `/` would give an empty prefix, which the reader
            // treats as "no prefix" and would silently drop the separator.
            if !prefix.is_empty() && !name.is_empty() {
                return Some((prefix, name));
            }
        }
    }
    None
}

pub fn build_header(entry: &TarWriteEntry) -> [u8; BLOCK_SIZE] {
    let mut header = [0u8; BLOCK_SIZE];

    // Write name (split into prefix + name if needed).
    let path_bytes = entry.name.as_bytes();
    if path_bytes.len() <= 100 {
        let copy_len = path_bytes.len().min(100);
        header[..copy_len].copy_from_slice(&path_bytes[..copy_len]);
    } else if let Some((prefix, name)) = split_ustar_name(path_bytes) {
        header[..name.len()].copy_from_slice(name);
        header[345..345 + prefix.len()].copy_from_slice(prefix);
    } else {
        // Not representable in plain ustar.  `create` emits a GNU `L`
        // (`@LongLink`) record ahead of this header carrying the real name;
        // what goes in the 100-byte field is the truncated form GNU tar
        // itself writes, purely so a reader that ignores the extension still
        // sees *something* rather than an empty name.
        header[..100].copy_from_slice(&path_bytes[..100]);
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

    // Linkname.  There is no prefix field for this one — 100 bytes is the
    // whole budget — so anything longer rides in a GNU `K` record that
    // `create` emits ahead of this header, and the truncated copy left here
    // is only for readers that ignore the extension.
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

/// Append `data` to `archive`, then zero-pad to the next 512-byte boundary.
#[allow(clippy::arithmetic_side_effects)]
fn push_padded(archive: &mut Vec<u8>, data: &[u8]) {
    archive.extend_from_slice(data);
    let remainder = data.len() % BLOCK_SIZE;
    if remainder != 0 {
        archive.extend_from_slice(&vec![0u8; BLOCK_SIZE.wrapping_sub(remainder)]);
    }
}

/// Emit a GNU `@LongLink` record carrying `value` for the entry that follows.
///
/// `flag` is `b'L'` for a long member name or `b'K'` for a long link target.
/// The record is an ordinary header whose name is the sentinel
/// `././@LongLink`, whose size is the byte length of `value` including its
/// terminating NUL, and whose data blocks are `value`.  The *next* real header
/// then supersedes its own truncated field with this value.
///
/// This is GNU tar's extension, not POSIX ustar, and it is the reason a member
/// name that cannot be split across the 155/100 header fields (a single
/// component over 100 bytes, say) is still stored *exactly* rather than
/// truncated.  Silently shortening a member name would mean extracting a file
/// under a name that is not its own — the same class of bug as fabricating one.
fn push_long_field(archive: &mut Vec<u8>, flag: u8, value: &[u8]) {
    let mut payload = Vec::with_capacity(value.len().saturating_add(1));
    payload.extend_from_slice(value);
    payload.push(0);

    let record = TarWriteEntry {
        name: PathBuf::from(GNU_LONGLINK_NAME),
        data: payload,
        kind: EntryKind::Other(flag),
        link_target: PathBuf::new(),
        mode: 0o644,
        uid: 0,
        gid: 0,
        mtime: 0,
    };
    archive.extend_from_slice(&build_header(&record));
    push_padded(archive, &record.data);
}

/// Create a tar archive in memory from a list of entries.
///
/// Produces a valid USTAR archive terminated by two zero blocks.  Member names
/// too long for the ustar header fields are preserved via GNU `@LongLink`
/// records rather than truncated.
#[allow(clippy::arithmetic_side_effects)]
pub fn create(entries: &[TarWriteEntry]) -> Vec<u8> {
    let mut archive = Vec::new();

    for entry in entries {
        // Long fields are emitted *before* the header they describe, in GNU
        // tar's order: name record, then link record, then the entry.
        let name_bytes = entry.name.as_bytes();
        if name_bytes.len() > 100 && split_ustar_name(name_bytes).is_none() {
            push_long_field(&mut archive, LONGNAME_FLAG, name_bytes);
        }
        let link_bytes = entry.link_target.as_bytes();
        if link_bytes.len() > 100 {
            push_long_field(&mut archive, LONGLINK_FLAG, link_bytes);
        }

        let header = build_header(entry);
        archive.extend_from_slice(&header);

        if !entry.data.is_empty() {
            push_padded(&mut archive, &entry.data);
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
            name: PathBuf::from("hello.txt"),
            data: b"Hello, world!".to_vec(),
            kind: EntryKind::File,
            link_target: PathBuf::new(),
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
        if parsed[0].name.as_path() != Path::new("hello.txt") {
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
                name: PathBuf::from("mydir/"),
                data: Vec::new(),
                kind: EntryKind::Directory,
                link_target: PathBuf::new(),
                mode: 0o755,
                uid: 0,
                gid: 0,
                mtime: 1700000000,
            },
            TarWriteEntry {
                name: PathBuf::from("mydir/data.bin"),
                data: vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x11, 0x22, 0x33],
                kind: EntryKind::File,
                link_target: PathBuf::new(),
                mode: 0o600,
                uid: 0,
                gid: 0,
                mtime: 1700000000,
            },
            TarWriteEntry {
                name: PathBuf::from("link"),
                data: Vec::new(),
                kind: EntryKind::Symlink,
                link_target: PathBuf::from("mydir/data.bin"),
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
        if parsed[0].kind != EntryKind::Directory || parsed[0].name.as_path() != Path::new("mydir/")
        {
            return Err(KernelError::CorruptedData);
        }
        if parsed[1].kind != EntryKind::File || parsed[1].size != 8 {
            return Err(KernelError::CorruptedData);
        }
        if parsed[2].kind != EntryKind::Symlink
            || parsed[2].link_target.as_path() != Path::new("mydir/data.bin")
        {
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
            name: PathBuf::from("test.dat"),
            data: b"checksum test".to_vec(),
            kind: EntryKind::File,
            link_target: PathBuf::new(),
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
            name: PathBuf::from("owned.txt"),
            data: b"data".to_vec(),
            kind: EntryKind::File,
            link_target: PathBuf::new(),
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

    // --- Test 7: non-UTF-8 member name and link target round-trip ---
    //
    // The ustar name/linkname fields are raw bytes.  Decoding them as UTF-8
    // turned a member like `re\xffport.txt` into the empty name, so it
    // extracted over the archive root instead of as a file.
    {
        let odd = Path::new(b"dir/re\xffport.txt".as_slice());
        let entries = vec![
            TarWriteEntry {
                name: odd.to_path_buf(),
                data: b"raw".to_vec(),
                kind: EntryKind::File,
                link_target: PathBuf::new(),
                mode: 0o644,
                uid: 0,
                gid: 0,
                mtime: 0,
            },
            TarWriteEntry {
                name: PathBuf::from("alias"),
                data: Vec::new(),
                kind: EntryKind::Symlink,
                link_target: odd.to_path_buf(),
                mode: 0o777,
                uid: 0,
                gid: 0,
                mtime: 0,
            },
        ];
        let archive = create(&entries);
        let parsed = parse(&archive)?;
        if parsed.len() != 2 {
            return Err(KernelError::CorruptedData);
        }
        if parsed[0].name.as_path() != odd {
            return Err(KernelError::CorruptedData);
        }
        if entry_data(&archive, &parsed[0])? != b"raw" {
            return Err(KernelError::CorruptedData);
        }
        if parsed[1].link_target.as_path() != odd {
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[tar]   non-UTF-8 name round-trip OK");
    }

    // --- Test 8: >100-byte name uses the ustar prefix field ---
    //
    // The prefix/name split and the `prefix + "/" + name` rejoin are separate
    // code paths from the short-name case, and the rejoin must not be done
    // with `Path::join` (which would drop the prefix for a rooted name).
    {
        let mut long = PathBuf::new();
        for _ in 0..12 {
            long.extend_bytes(b"abcdefghij/");
        }
        long.extend_bytes(b"leaf.txt");
        if long.len() <= 100 {
            return Err(KernelError::InternalError);
        }
        let entries = vec![TarWriteEntry {
            name: long.clone(),
            data: b"deep".to_vec(),
            kind: EntryKind::File,
            link_target: PathBuf::new(),
            mode: 0o644,
            uid: 0,
            gid: 0,
            mtime: 0,
        }];
        let archive = create(&entries);
        // 4 blocks: header, one data block, two zero blocks.  Anything longer
        // means the name went out in a `@LongLink` record instead of the
        // prefix field, which for a splittable name would be a regression to
        // a non-POSIX encoding.
        if archive.len() != 4 * BLOCK_SIZE {
            serial_println!("[tar]   long-name did not use the prefix field");
            return Err(KernelError::CorruptedData);
        }
        let parsed = parse(&archive)?;
        let Some(first) = parsed.first() else {
            return Err(KernelError::CorruptedData);
        };
        if parsed.len() != 1 || first.name != long {
            serial_println!("[tar]   long-name mismatch: {}", first.name.display());
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[tar]   long-name prefix split OK");
    }

    // --- Test 9: a name that ustar cannot split rides a GNU `@LongLink` ---
    //
    // A single component longer than 100 bytes has no `/` in the legal split
    // window, so there is no prefix/name division that stores it.  The writer
    // must emit the GNU long-name record rather than chop the name; the old
    // code truncated silently, which extracts a file under a name that is not
    // its own.
    {
        let mut huge = PathBuf::from("dir/");
        for _ in 0..13 {
            huge.extend_bytes(b"0123456789");
        }
        // Non-UTF-8 tail: the long record is a byte payload like every other
        // name field, so this must survive too.
        huge.extend_bytes(b"\xff.bin");
        if split_ustar_name(huge.as_bytes()).is_some() {
            // The test would not be exercising the long-record path.
            return Err(KernelError::InternalError);
        }

        let entries = vec![TarWriteEntry {
            name: huge.clone(),
            data: b"payload".to_vec(),
            kind: EntryKind::File,
            link_target: PathBuf::new(),
            mode: 0o600,
            uid: 7,
            gid: 8,
            mtime: 42,
        }];
        let archive = create(&entries);
        let parsed = parse(&archive)?;
        let Some(first) = parsed.first() else {
            return Err(KernelError::CorruptedData);
        };
        // The `L` record must not surface as an entry of its own.
        if parsed.len() != 1 {
            serial_println!("[tar]   long-name record leaked as an entry");
            return Err(KernelError::CorruptedData);
        }
        if first.name != huge {
            serial_println!("[tar]   long-name record mismatch: {}", first.name.display());
            return Err(KernelError::CorruptedData);
        }
        // The metadata belongs to the real header, not the record.
        if first.mode != 0o600 || first.uid != 7 || first.gid != 8 || first.mtime != 42 {
            return Err(KernelError::CorruptedData);
        }
        if entry_data(&archive, first)? != b"payload" {
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[tar]   GNU long-name record OK");
    }

    // --- Test 10: a >100-byte symlink target rides a GNU `K` record ---
    //
    // The linkname field has no prefix companion, so 100 bytes is the entire
    // ustar budget for it.
    {
        let mut target = PathBuf::from("/very/long");
        for _ in 0..12 {
            target.extend_bytes(b"/abcdefghij");
        }
        if target.len() <= 100 {
            return Err(KernelError::InternalError);
        }
        let entries = vec![TarWriteEntry {
            name: PathBuf::from("link"),
            data: Vec::new(),
            kind: EntryKind::Symlink,
            link_target: target.clone(),
            mode: 0o777,
            uid: 0,
            gid: 0,
            mtime: 0,
        }];
        let archive = create(&entries);
        let parsed = parse(&archive)?;
        let Some(first) = parsed.first() else {
            return Err(KernelError::CorruptedData);
        };
        if parsed.len() != 1 || first.link_target != target {
            serial_println!("[tar]   long-link mismatch: {}", first.link_target.display());
            return Err(KernelError::CorruptedData);
        }
        if first.name.as_path() != Path::new("link") || first.kind != EntryKind::Symlink {
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[tar]   GNU long-link record OK");
    }

    // --- Test 11: the split window's boundaries ---
    {
        // Exactly 100 bytes of name after the separator is the largest legal
        // `name` field; 101 is not, and must push the split later.
        let mut p = PathBuf::from("aa/");
        p.extend_bytes(&[b'x'; 100]);
        let bytes = p.as_bytes().to_vec();
        let Some((prefix, name)) = split_ustar_name(&bytes) else {
            return Err(KernelError::CorruptedData);
        };
        if prefix != b"aa" || name.len() != 100 {
            return Err(KernelError::CorruptedData);
        }
        // A prefix that would exceed 155 bytes has no legal split.
        let mut deep = PathBuf::new();
        for _ in 0..40 {
            deep.extend_bytes(b"abcd/");
        }
        deep.extend_bytes(&[b'y'; 60]);
        if deep.len() <= 256 {
            return Err(KernelError::InternalError);
        }
        if split_ustar_name(deep.as_bytes()).is_some() {
            return Err(KernelError::CorruptedData);
        }
        // …and it round-trips anyway, through the long-name record.
        let entries = vec![TarWriteEntry {
            name: deep.clone(),
            data: Vec::new(),
            kind: EntryKind::File,
            link_target: PathBuf::new(),
            mode: 0o644,
            uid: 0,
            gid: 0,
            mtime: 0,
        }];
        let archive = create(&entries);
        let parsed = parse(&archive)?;
        let Some(first) = parsed.first() else {
            return Err(KernelError::CorruptedData);
        };
        if first.name != deep {
            return Err(KernelError::CorruptedData);
        }
        serial_println!("[tar]   split-window boundaries OK");
    }

    serial_println!("[tar] Self-test passed.");
    Ok(())
}
