//! CPIO archive support (newc/SVR4 format).
//!
//! Implements parsing, extraction, and creation of CPIO archives in the
//! "newc" (SVR4 with no CRC) format, which is the standard format used
//! by Linux initramfs images.
//!
//! ## Format overview
//!
//! A CPIO archive is a stream of entries, each consisting of:
//! 1. A 110-byte header (ASCII hex fields)
//! 2. Filename (null-terminated, padded to 4-byte boundary)
//! 3. File data (padded to 4-byte boundary)
//!
//! The archive ends with a special trailer entry named "TRAILER!!!".
//!
//! ## Header layout (newc format)
//!
//! ```text
//! Offset  Len  Field
//! 0       6    magic: "070701" (newc) or "070702" (newc+CRC)
//! 6       8    inode number
//! 14      8    mode (file type + permissions)
//! 22      8    uid
//! 30      8    gid
//! 38      8    nlink
//! 46      8    mtime
//! 54      8    filesize
//! 62      8    devmajor
//! 70      8    devminor
//! 78      8    rdevmajor
//! 86      8    rdevminor
//! 94      8    namesize (includes null terminator)
//! 102     8    checksum (0 for "070701", CRC for "070702")
//! ```
//!
//! All numeric fields are 8 ASCII hex digits (big-endian text).
//!
//! ## Supported features
//!
//! - Regular files, directories, symlinks
//! - Read (extraction) and write (archive creation)
//! - Initramfs-compatible output
//!
//! ## References
//!
//! - `man 5 cpio` (GNU cpio)
//! - Linux `usr/gen_init_cpio.c`
//! - <https://man.freebsd.org/cgi/man.cgi?query=cpio&sektion=5>

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Magic number for the "newc" (SVR4 no-CRC) format.
const CPIO_NEWC_MAGIC: &[u8; 6] = b"070701";

/// Magic number for the "newc" format with CRC.
const CPIO_NEWC_CRC_MAGIC: &[u8; 6] = b"070702";

/// Old binary CPIO magic (0o70707 in octal = 0x71C7).
const CPIO_OLD_MAGIC: u16 = 0o070707;

/// Header size in bytes (fixed for newc format).
const HEADER_SIZE: usize = 110;

/// Trailer filename that signals end of archive.
const TRAILER: &str = "TRAILER!!!";

/// Safety limit on decompressed/extracted data (256 MiB).
const MAX_OUTPUT: usize = 256 * 1024 * 1024;

/// File type masks (from POSIX mode bits).
mod file_type {
    pub const S_IFMT: u32 = 0o170000; // File type mask
    pub const S_IFDIR: u32 = 0o040000; // Directory
    pub const S_IFREG: u32 = 0o100000; // Regular file
    pub const S_IFLNK: u32 = 0o120000; // Symbolic link
    pub const S_IFBLK: u32 = 0o060000; // Block device
    pub const S_IFCHR: u32 = 0o020000; // Character device
    pub const S_IFIFO: u32 = 0o010000; // FIFO/pipe
    pub const S_IFSOCK: u32 = 0o140000; // Socket
}

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// A single entry extracted from a CPIO archive.
pub struct CpioEntry {
    /// File path (no leading slash; e.g., "etc/passwd").
    pub name: String,
    /// File data (empty for directories and special files).
    pub data: Vec<u8>,
    /// Entry type.
    pub entry_type: CpioEntryType,
    /// POSIX mode bits (permissions portion, lower 12 bits).
    pub mode: u32,
    /// User ID.
    pub uid: u32,
    /// Group ID.
    pub gid: u32,
    /// Modification time (Unix timestamp).
    pub mtime: u32,
    /// Symlink target (only valid if entry_type is Symlink).
    pub link_target: String,
}

/// Types of CPIO archive entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpioEntryType {
    /// Regular file.
    File,
    /// Directory.
    Directory,
    /// Symbolic link (target stored in data).
    Symlink,
    /// Block device.
    BlockDevice,
    /// Character device.
    CharDevice,
    /// FIFO/pipe.
    Fifo,
    /// Socket (rarely seen in archives).
    Socket,
    /// Unknown/unsupported type.
    Unknown,
}

/// Parsed newc header.
struct CpioHeader {
    ino: u32,
    mode: u32,
    uid: u32,
    gid: u32,
    nlink: u32,
    mtime: u32,
    filesize: u32,
    devmajor: u32,
    devminor: u32,
    rdevmajor: u32,
    rdevminor: u32,
    namesize: u32,
    checksum: u32,
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Parse an 8-character ASCII hex field into u32.
fn parse_hex8(data: &[u8]) -> KernelResult<u32> {
    if data.len() < 8 {
        return Err(KernelError::CorruptedData);
    }
    let mut val = 0u32;
    for &b in &data[..8] {
        let digit = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return Err(KernelError::CorruptedData),
        };
        val = val.checked_mul(16)
            .and_then(|v| v.checked_add(u32::from(digit)))
            .ok_or(KernelError::CorruptedData)?;
    }
    Ok(val)
}

/// Format a u32 as 8 ASCII hex characters.
fn format_hex8(val: u32) -> [u8; 8] {
    let mut buf = [b'0'; 8];
    let mut v = val;
    for i in (0..8).rev() {
        let digit = (v & 0xF) as u8;
        buf[i] = if digit < 10 { b'0' + digit } else { b'A' + digit - 10 };
        v >>= 4;
    }
    buf
}

/// Round `offset` up to the next multiple of 4.
fn align4(offset: usize) -> usize {
    (offset.wrapping_add(3)) & !3
}

/// Parse a newc header from the given data slice.
fn parse_header(data: &[u8]) -> KernelResult<CpioHeader> {
    if data.len() < HEADER_SIZE {
        return Err(KernelError::CorruptedData);
    }

    // Verify magic.
    let magic = &data[..6];
    if magic != CPIO_NEWC_MAGIC && magic != CPIO_NEWC_CRC_MAGIC {
        return Err(KernelError::CorruptedData);
    }

    Ok(CpioHeader {
        ino:        parse_hex8(&data[6..14])?,
        mode:       parse_hex8(&data[14..22])?,
        uid:        parse_hex8(&data[22..30])?,
        gid:        parse_hex8(&data[30..38])?,
        nlink:      parse_hex8(&data[38..46])?,
        mtime:      parse_hex8(&data[46..54])?,
        filesize:   parse_hex8(&data[54..62])?,
        devmajor:   parse_hex8(&data[62..70])?,
        devminor:   parse_hex8(&data[70..78])?,
        rdevmajor:  parse_hex8(&data[78..86])?,
        rdevminor:  parse_hex8(&data[86..94])?,
        namesize:   parse_hex8(&data[94..102])?,
        checksum:   parse_hex8(&data[102..110])?,
    })
}

/// Determine entry type from the mode field.
fn entry_type_from_mode(mode: u32) -> CpioEntryType {
    match mode & file_type::S_IFMT {
        file_type::S_IFREG => CpioEntryType::File,
        file_type::S_IFDIR => CpioEntryType::Directory,
        file_type::S_IFLNK => CpioEntryType::Symlink,
        file_type::S_IFBLK => CpioEntryType::BlockDevice,
        file_type::S_IFCHR => CpioEntryType::CharDevice,
        file_type::S_IFIFO => CpioEntryType::Fifo,
        file_type::S_IFSOCK => CpioEntryType::Socket,
        _ => CpioEntryType::Unknown,
    }
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// Extract all entries from a CPIO archive (newc format).
///
/// Returns a list of entries.  The trailer entry is not included.
/// Supports both "070701" (no CRC) and "070702" (with CRC) variants.
///
/// If the input is gzip-compressed (e.g., an initramfs .cpio.gz),
/// decompress it first before calling this function.
pub fn uncpio(data: &[u8]) -> KernelResult<Vec<CpioEntry>> {
    // Minimum: need at least one header to be a valid CPIO archive.
    if data.len() < HEADER_SIZE {
        return Err(KernelError::CorruptedData);
    }

    // Check for old binary format and reject with clear error.
    if data.len() >= 2 {
        let magic16 = u16::from_le_bytes([
            *data.first().ok_or(KernelError::CorruptedData)?,
            *data.get(1).ok_or(KernelError::CorruptedData)?,
        ]);
        // Old binary format uses 0o070707 in native byte order.
        if magic16 == CPIO_OLD_MAGIC || magic16 == CPIO_OLD_MAGIC.swap_bytes() {
            return Err(KernelError::NotSupported);
        }
    }

    // Validate that the first entry starts with a valid newc magic.
    if data.get(..6) != Some(CPIO_NEWC_MAGIC.as_slice())
        && data.get(..6) != Some(CPIO_NEWC_CRC_MAGIC.as_slice())
    {
        return Err(KernelError::CorruptedData);
    }

    let mut entries = Vec::new();
    let mut pos = 0usize;
    let mut total_data = 0usize;

    loop {
        if pos >= data.len() {
            break;
        }

        // Need at least a header.
        if pos.wrapping_add(HEADER_SIZE) > data.len() {
            break;
        }

        let hdr = parse_header(&data[pos..])?;
        let namesize = hdr.namesize as usize;
        let filesize = hdr.filesize as usize;

        // Filename starts right after the header.
        let name_start = pos.wrapping_add(HEADER_SIZE);
        let name_end = name_start.wrapping_add(namesize);
        if name_end > data.len() {
            return Err(KernelError::CorruptedData);
        }

        // Parse filename (strip null terminator).
        let name_bytes = &data[name_start..name_end];
        let name_len = name_bytes.iter().position(|&b| b == 0).unwrap_or(namesize);
        let name = String::from(
            core::str::from_utf8(name_bytes.get(..name_len).unwrap_or(b""))
                .unwrap_or("")
        );

        // Check for trailer.
        if name == TRAILER {
            break;
        }

        // Data starts after the filename, aligned to 4 bytes.
        let data_start = align4(name_end);
        let data_end = data_start.wrapping_add(filesize);
        if data_end > data.len() {
            return Err(KernelError::CorruptedData);
        }

        // Safety limit.
        total_data = total_data.wrapping_add(filesize);
        if total_data > MAX_OUTPUT {
            return Err(KernelError::OutOfMemory);
        }

        let file_data = data.get(data_start..data_end)
            .ok_or(KernelError::CorruptedData)?
            .to_vec();

        let etype = entry_type_from_mode(hdr.mode);

        // For symlinks, the file data IS the link target.
        let link_target = if etype == CpioEntryType::Symlink {
            String::from(core::str::from_utf8(&file_data).unwrap_or(""))
        } else {
            String::new()
        };

        // Strip leading "./" or "/" from name for consistency.
        let clean_name = name.strip_prefix("./").unwrap_or(&name);
        let clean_name = clean_name.strip_prefix('/').unwrap_or(clean_name);

        // Skip the "." root directory entry.
        if !clean_name.is_empty() {
            entries.push(CpioEntry {
                name: String::from(clean_name),
                data: file_data,
                entry_type: etype,
                mode: hdr.mode & 0o7777, // Permission bits only
                uid: hdr.uid,
                gid: hdr.gid,
                mtime: hdr.mtime,
                link_target,
            });
        }

        // Next entry starts after the file data, aligned to 4 bytes.
        pos = align4(data_end);
    }

    Ok(entries)
}

// ---------------------------------------------------------------------------
// Archive creation
// ---------------------------------------------------------------------------

/// Build a CPIO newc archive from a list of entries.
///
/// Produces a byte stream in "070701" format suitable for use as a
/// Linux initramfs image.  The archive ends with a proper TRAILER!!!
/// entry.
///
/// ## Example
///
/// ```ignore
/// let entries = vec![
///     CpioEntry {
///         name: "hello.txt".into(),
///         data: b"Hello, world!\n".to_vec(),
///         entry_type: CpioEntryType::File,
///         mode: 0o644,
///         uid: 0, gid: 0, mtime: 0,
///         link_target: String::new(),
///     },
/// ];
/// let archive = mkcpio(&entries)?;
/// ```
pub fn mkcpio(entries: &[CpioEntry]) -> KernelResult<Vec<u8>> {
    let mut buf = Vec::new();
    let mut ino = 1u32; // Start inode numbers at 1.

    for entry in entries {
        let mode = match entry.entry_type {
            CpioEntryType::File => file_type::S_IFREG | (entry.mode & 0o7777),
            CpioEntryType::Directory => file_type::S_IFDIR | (entry.mode & 0o7777),
            CpioEntryType::Symlink => file_type::S_IFLNK | (entry.mode & 0o7777),
            CpioEntryType::BlockDevice => file_type::S_IFBLK | (entry.mode & 0o7777),
            CpioEntryType::CharDevice => file_type::S_IFCHR | (entry.mode & 0o7777),
            CpioEntryType::Fifo => file_type::S_IFIFO | (entry.mode & 0o7777),
            CpioEntryType::Socket => file_type::S_IFSOCK | (entry.mode & 0o7777),
            CpioEntryType::Unknown => file_type::S_IFREG | (entry.mode & 0o7777),
        };

        // For symlinks, the data is the link target.
        let file_data = if entry.entry_type == CpioEntryType::Symlink {
            entry.link_target.as_bytes()
        } else {
            &entry.data
        };

        let nlink = match entry.entry_type {
            CpioEntryType::Directory => 2u32,
            _ => 1u32,
        };

        // Filename with null terminator.
        let name_with_null = entry.name.len().wrapping_add(1);

        // Write header.
        write_header(
            &mut buf,
            ino,
            mode,
            entry.uid,
            entry.gid,
            nlink,
            entry.mtime,
            file_data.len() as u32,
            0, 0, 0, 0, // dev/rdev
            name_with_null as u32,
        );

        // Write filename + null terminator.
        buf.extend_from_slice(entry.name.as_bytes());
        buf.push(0);

        // Pad filename to 4-byte boundary.
        let name_total = HEADER_SIZE.wrapping_add(name_with_null);
        let name_padded = align4(name_total);
        let name_pad = name_padded.saturating_sub(name_total);
        buf.resize(buf.len() + name_pad, 0);

        // Write file data.
        buf.extend_from_slice(file_data);

        // Pad data to 4-byte boundary.
        let data_pad = align4(file_data.len()).saturating_sub(file_data.len());
        buf.resize(buf.len() + data_pad, 0);

        ino = ino.wrapping_add(1);
    }

    // Write trailer entry.
    let trailer_name = TRAILER;
    let trailer_namesize = (trailer_name.len().wrapping_add(1)) as u32;

    write_header(
        &mut buf,
        0, 0, 0, 0, 1, 0, 0, // ino=0, mode=0, uid=0, gid=0, nlink=1, mtime=0, filesize=0
        0, 0, 0, 0,           // dev/rdev
        trailer_namesize,
    );

    buf.extend_from_slice(trailer_name.as_bytes());
    buf.push(0);

    // Pad trailer filename.
    let trailer_total = HEADER_SIZE.wrapping_add(trailer_namesize as usize);
    let trailer_padded = align4(trailer_total);
    let trailer_pad = trailer_padded.saturating_sub(trailer_total);
    buf.resize(buf.len() + trailer_pad, 0);

    // Pad entire archive to 512-byte block boundary (common convention).
    let block_pad = align_block(buf.len(), 512).saturating_sub(buf.len());
    buf.resize(buf.len() + block_pad, 0);

    Ok(buf)
}

/// Round `offset` up to the next multiple of `block`.
fn align_block(offset: usize, block: usize) -> usize {
    if block == 0 {
        return offset;
    }
    let rem = offset % block;
    if rem == 0 { offset } else { offset.wrapping_add(block.wrapping_sub(rem)) }
}

/// Write a newc header to the output buffer.
#[allow(clippy::too_many_arguments)]
fn write_header(
    buf: &mut Vec<u8>,
    ino: u32,
    mode: u32,
    uid: u32,
    gid: u32,
    nlink: u32,
    mtime: u32,
    filesize: u32,
    devmajor: u32,
    devminor: u32,
    rdevmajor: u32,
    rdevminor: u32,
    namesize: u32,
) {
    buf.extend_from_slice(CPIO_NEWC_MAGIC);
    buf.extend_from_slice(&format_hex8(ino));
    buf.extend_from_slice(&format_hex8(mode));
    buf.extend_from_slice(&format_hex8(uid));
    buf.extend_from_slice(&format_hex8(gid));
    buf.extend_from_slice(&format_hex8(nlink));
    buf.extend_from_slice(&format_hex8(mtime));
    buf.extend_from_slice(&format_hex8(filesize));
    buf.extend_from_slice(&format_hex8(devmajor));
    buf.extend_from_slice(&format_hex8(devminor));
    buf.extend_from_slice(&format_hex8(rdevmajor));
    buf.extend_from_slice(&format_hex8(rdevminor));
    buf.extend_from_slice(&format_hex8(namesize));
    buf.extend_from_slice(&format_hex8(0)); // checksum = 0 for newc
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for CPIO parsing and creation.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[cpio] Running self-test...");

    // Test 1: hex parsing.
    test_hex()?;

    // Test 2: alignment.
    test_alignment()?;

    // Test 3: round-trip (create + extract).
    test_roundtrip()?;

    // Test 4: empty archive.
    test_empty_archive()?;

    // Test 5: magic detection.
    test_magic()?;

    crate::serial_println!("[cpio] Self-test passed.");
    Ok(())
}

fn test_hex() -> KernelResult<()> {
    // parse_hex8
    let val = parse_hex8(b"0000002A")?;
    if val != 42 {
        crate::serial_println!("[cpio]   FAIL: hex(0000002A) = {} (expected 42)", val);
        return Err(KernelError::InternalError);
    }

    let val2 = parse_hex8(b"FFFFFFFF")?;
    if val2 != 0xFFFF_FFFF {
        crate::serial_println!("[cpio]   FAIL: hex(FFFFFFFF) = {} (expected {})", val2, 0xFFFF_FFFFu32);
        return Err(KernelError::InternalError);
    }

    let val3 = parse_hex8(b"00000000")?;
    if val3 != 0 {
        crate::serial_println!("[cpio]   FAIL: hex(00000000) = {}", val3);
        return Err(KernelError::InternalError);
    }

    // format_hex8 round-trip
    let formatted = format_hex8(42);
    let reparsed = parse_hex8(&formatted)?;
    if reparsed != 42 {
        crate::serial_println!("[cpio]   FAIL: format_hex8 round-trip: {} != 42", reparsed);
        return Err(KernelError::InternalError);
    }

    let formatted2 = format_hex8(0xDEAD_BEEF);
    let reparsed2 = parse_hex8(&formatted2)?;
    if reparsed2 != 0xDEAD_BEEF {
        crate::serial_println!("[cpio]   FAIL: format_hex8 round-trip: 0x{:X} != 0xDEADBEEF", reparsed2);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[cpio]   hex parsing OK");
    Ok(())
}

fn test_alignment() -> KernelResult<()> {
    if align4(0) != 0 {
        crate::serial_println!("[cpio]   FAIL: align4(0)={}", align4(0));
        return Err(KernelError::InternalError);
    }
    if align4(1) != 4 {
        crate::serial_println!("[cpio]   FAIL: align4(1)={}", align4(1));
        return Err(KernelError::InternalError);
    }
    if align4(4) != 4 {
        crate::serial_println!("[cpio]   FAIL: align4(4)={}", align4(4));
        return Err(KernelError::InternalError);
    }
    if align4(5) != 8 {
        crate::serial_println!("[cpio]   FAIL: align4(5)={}", align4(5));
        return Err(KernelError::InternalError);
    }
    if align4(110) != 112 {
        crate::serial_println!("[cpio]   FAIL: align4(110)={}", align4(110));
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[cpio]   alignment OK");
    Ok(())
}

fn test_roundtrip() -> KernelResult<()> {
    // Create a small archive with a file and a directory.
    let entries = vec![
        CpioEntry {
            name: String::from("testdir"),
            data: Vec::new(),
            entry_type: CpioEntryType::Directory,
            mode: 0o755,
            uid: 0,
            gid: 0,
            mtime: 1700000000,
            link_target: String::new(),
        },
        CpioEntry {
            name: String::from("testdir/hello.txt"),
            data: Vec::from(*b"Hello, CPIO!\n"),
            entry_type: CpioEntryType::File,
            mode: 0o644,
            uid: 1000,
            gid: 1000,
            mtime: 1700000000,
            link_target: String::new(),
        },
        CpioEntry {
            name: String::from("testdir/link"),
            data: Vec::new(),
            entry_type: CpioEntryType::Symlink,
            mode: 0o777,
            uid: 0,
            gid: 0,
            mtime: 1700000000,
            link_target: String::from("hello.txt"),
        },
    ];

    let archive = mkcpio(&entries)?;

    // Verify archive starts with the right magic.
    if archive.get(..6) != Some(CPIO_NEWC_MAGIC.as_slice()) {
        crate::serial_println!("[cpio]   FAIL: archive doesn't start with newc magic");
        return Err(KernelError::InternalError);
    }

    // Extract and verify.
    let extracted = uncpio(&archive)?;
    if extracted.len() != 3 {
        crate::serial_println!("[cpio]   FAIL: expected 3 entries, got {}", extracted.len());
        return Err(KernelError::InternalError);
    }

    // Check directory.
    let dir = &extracted[0];
    if dir.name != "testdir" || dir.entry_type != CpioEntryType::Directory {
        crate::serial_println!("[cpio]   FAIL: entry 0: name='{}', type={:?}", dir.name, dir.entry_type);
        return Err(KernelError::InternalError);
    }

    // Check file.
    let file = &extracted[1];
    if file.name != "testdir/hello.txt" || file.entry_type != CpioEntryType::File {
        crate::serial_println!("[cpio]   FAIL: entry 1: name='{}', type={:?}", file.name, file.entry_type);
        return Err(KernelError::InternalError);
    }
    if file.data != b"Hello, CPIO!\n" {
        crate::serial_println!("[cpio]   FAIL: entry 1 data mismatch (len={})", file.data.len());
        return Err(KernelError::InternalError);
    }
    if file.uid != 1000 || file.gid != 1000 {
        crate::serial_println!("[cpio]   FAIL: entry 1 uid/gid: {}/{}", file.uid, file.gid);
        return Err(KernelError::InternalError);
    }

    // Check symlink.
    let link = &extracted[2];
    if link.name != "testdir/link" || link.entry_type != CpioEntryType::Symlink {
        crate::serial_println!("[cpio]   FAIL: entry 2: name='{}', type={:?}", link.name, link.entry_type);
        return Err(KernelError::InternalError);
    }
    if link.link_target != "hello.txt" {
        crate::serial_println!("[cpio]   FAIL: entry 2 link target: '{}'", link.link_target);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[cpio]   round-trip OK (3 entries: dir + file + symlink)");
    Ok(())
}

fn test_empty_archive() -> KernelResult<()> {
    // Create an empty archive (just the trailer).
    let archive = mkcpio(&[])?;
    let entries = uncpio(&archive)?;
    if !entries.is_empty() {
        crate::serial_println!("[cpio]   FAIL: empty archive produced {} entries", entries.len());
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[cpio]   empty archive OK");
    Ok(())
}

fn test_magic() -> KernelResult<()> {
    // Too-short data should fail.
    if uncpio(&[0x07, 0x07]).is_ok() {
        crate::serial_println!("[cpio]   FAIL: should reject short data");
        return Err(KernelError::InternalError);
    }

    // Wrong magic should fail.
    let bad = [b'X'; 110];
    if uncpio(&bad).is_ok() {
        crate::serial_println!("[cpio]   FAIL: should reject bad magic");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[cpio]   magic detection OK");
    Ok(())
}
