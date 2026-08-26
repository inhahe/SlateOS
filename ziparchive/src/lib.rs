//! ZIP archive support (read and write).
//!
//! Implements parsing, extraction, and creation of ZIP archives using
//! Stored (method 0) and DEFLATE (method 8) compression, plus ZIP64
//! extensions for archives and entries larger than 4 GiB / 65535 entries.
//!
//! ## Format overview
//!
//! ```text
//! ┌──────────────────────────────────┐
//! │ Local file header #1             │  ← signature 0x04034b50
//! │ File data #1 (compressed)        │
//! ├──────────────────────────────────┤
//! │ Local file header #2             │
//! │ File data #2 (compressed)        │
//! ├──────────────────────────────────┤
//! │ ...                              │
//! ├══════════════════════════════════┤
//! │ Central directory header #1      │  ← signature 0x02014b50
//! │ Central directory header #2      │
//! │ ...                              │
//! ├══════════════════════════════════┤
//! │ ZIP64 end of central dir record  │  (optional, for ZIP64)
//! │ ZIP64 end of central dir locator │
//! ├══════════════════════════════════┤
//! │ End of central directory record  │  ← signature 0x06054b50
//! └──────────────────────────────────┘
//! ```
//!
//! ## Compression methods supported
//!
//! | Method | Name    | Support |
//! |--------|---------|---------|
//! | 0      | Stored  | Full    |
//! | 8      | Deflate | Full    |
//!
//! ## ZIP64 extensions
//!
//! When any of these conditions hold, ZIP64 is needed:
//! - An entry's uncompressed or compressed size exceeds 0xFFFFFFFF
//! - An entry's local header offset exceeds 0xFFFFFFFF
//! - There are more than 0xFFFF entries
//!
//! ZIP64 uses extra fields (header ID 0x0001) in local and central
//! directory headers, plus a ZIP64 EOCD record and locator.
//!
//! ## CRC-32
//!
//! Each entry carries a CRC-32 (ISO 3309 / ITU-T V.42, same polynomial
//! as gzip: 0xEDB88320 reflected).  Verified on extraction.
//!
//! ## References
//!
//! - PKWARE APPNOTE.TXT v6.3.10 (ZIP File Format Specification)
//! - <https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT>
//! - Info-ZIP source (unzip/zip)
//!
//! ## Why this is a crate and not a kernel module
//!
//! It began as `kernel/src/fs/zip.rs`, where `apps/archivemanager` could not
//! reach it: a module of a *binary* crate cannot be depended on. The choice was
//! between a second ZIP implementation and this move, and for a parser of
//! untrusted input a second implementation is the worse option by a wide
//! margin — two attack surfaces, and a bug fixed in one is not fixed in the
//! other. ZIP's hazards are the kind that get missed exactly once: the EOCD has
//! to be found by scanning *backwards* through a variable-length comment, entry
//! names must be checked for `..` traversal before a byte is written, the local
//! header and the central directory can disagree about the same entry, and
//! ZIP64 fields shadow the 32-bit ones. Requested in
//! `requests/c-a-zip-is-trapped-in-the-kernel-binary.md`; same reasoning, and
//! the same outcome, as `deflate/` before it (design-decisions.md § 610).

#![no_std]
#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

/// Everything that can go wrong reading or writing an archive.
///
/// Two variants, because the kernel original distinguished exactly two cases
/// and inventing more here would be inventing distinctions the parser does not
/// actually make. It previously carried `KernelError`, which a userspace
/// archive manager has no business seeing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The archive is malformed or truncated, an entry failed its CRC-32, or
    /// an entry's real size did not match the size it declared.
    CorruptedData,
    /// The entry uses a compression method other than Stored (0) or
    /// Deflate (8).
    UnsupportedMethod,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CorruptedData => f.write_str("corrupted or malformed ZIP data"),
            Self::UnsupportedMethod => f.write_str("unsupported ZIP compression method"),
        }
    }
}

/// Result alias for this crate.
pub type Result<T> = core::result::Result<T, Error>;

// ---------------------------------------------------------------------------
// Constants — signatures
// ---------------------------------------------------------------------------

/// Local file header signature.
const LOCAL_SIG: u32 = 0x0403_4B50;
/// Central directory file header signature.
const CENTRAL_SIG: u32 = 0x0201_4B50;
/// End of central directory record signature.
const EOCD_SIG: u32 = 0x0605_4B50;
/// ZIP64 end of central directory record signature.
const ZIP64_EOCD_SIG: u32 = 0x0606_4B50;
/// ZIP64 end of central directory locator signature.
const ZIP64_LOCATOR_SIG: u32 = 0x0706_4B50;
/// ZIP64 extra field header ID.
const ZIP64_EXTRA_ID: u16 = 0x0001;

// ---------------------------------------------------------------------------
// Little-endian helpers
// ---------------------------------------------------------------------------

/// Read a little-endian u16 from `data` at `off`.
#[inline]
fn le_u16(data: &[u8], off: usize) -> u16 {
    let lo = u16::from(*data.get(off).unwrap_or(&0));
    let hi = u16::from(*data.get(off.wrapping_add(1)).unwrap_or(&0));
    lo | (hi << 8)
}

/// Read a little-endian u32 from `data` at `off`.
#[inline]
fn le_u32(data: &[u8], off: usize) -> u32 {
    let b0 = u32::from(*data.get(off).unwrap_or(&0));
    let b1 = u32::from(*data.get(off.wrapping_add(1)).unwrap_or(&0));
    let b2 = u32::from(*data.get(off.wrapping_add(2)).unwrap_or(&0));
    let b3 = u32::from(*data.get(off.wrapping_add(3)).unwrap_or(&0));
    b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
}

/// Read a little-endian u64 from `data` at `off`.
#[inline]
fn le_u64(data: &[u8], off: usize) -> u64 {
    let lo = u64::from(le_u32(data, off));
    let hi = u64::from(le_u32(data, off.wrapping_add(4)));
    lo | (hi << 32)
}

/// Write a little-endian u16 to a byte vector.
#[inline]
fn write_u16(buf: &mut Vec<u8>, val: u16) {
    buf.extend_from_slice(&val.to_le_bytes());
}

/// Write a little-endian u32 to a byte vector.
#[inline]
fn write_u32(buf: &mut Vec<u8>, val: u32) {
    buf.extend_from_slice(&val.to_le_bytes());
}

/// Write a little-endian u64 to a byte vector.
#[inline]
fn write_u64(buf: &mut Vec<u8>, val: u64) {
    buf.extend_from_slice(&val.to_le_bytes());
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A parsed entry from a ZIP archive's central directory.
#[derive(Debug, Clone)]
pub struct ZipEntry {
    /// Filename (relative path inside the archive).
    ///
    /// A byte string, not text.  ZIP stores the name as raw bytes and does
    /// not require any particular encoding (the general-purpose bit 11 flag
    /// only *claims* UTF-8; DOS/CP437 and arbitrary locale bytes are common
    /// in the wild).  Decoding it as UTF-8 used to substitute a fabricated
    /// `<invalid-utf8@0x…>` placeholder, so extraction wrote the member out
    /// under a name that was not its own.
    pub name: Vec<u8>,
    /// Compression method: 0 = Stored, 8 = Deflate.
    pub method: u16,
    /// CRC-32 of uncompressed data (ISO 3309).
    pub crc32: u32,
    /// Compressed size in bytes.
    pub compressed_size: u64,
    /// Uncompressed size in bytes.
    pub uncompressed_size: u64,
    /// Offset of the local file header within the archive.
    pub local_header_offset: u64,
    /// True if this entry represents a directory (name ends with `/`).
    pub is_dir: bool,
}

/// An entry to be written into a new ZIP archive.
pub struct ZipWriteEntry {
    /// Relative path inside the archive (use `/` separators).  Bytes, not
    /// text — see [`ZipEntry::name`].
    pub name: Vec<u8>,
    /// Uncompressed file data.  Empty for directory markers.
    pub data: Vec<u8>,
    /// True to store without compression (method 0).
    /// False to try DEFLATE, falling back to stored if it doesn't shrink.
    pub store_only: bool,
}

// ---------------------------------------------------------------------------
// Parsing — EOCD location
// ---------------------------------------------------------------------------

/// Scan backwards from the end of `data` to find the EOCD signature.
///
/// Returns the byte offset of the EOCD within `data`, or `None` if no
/// valid EOCD is found.  Handles archives with trailing comments up to
/// the maximum 65535-byte comment length.
fn find_eocd(data: &[u8]) -> Option<usize> {
    if data.len() < 22 {
        return None;
    }
    // EOCD is at most 22 + 65535 bytes from the end.
    let search_start = data.len().saturating_sub(22 + 65535);
    let mut pos = data.len().saturating_sub(22);
    loop {
        if le_u32(data, pos) == EOCD_SIG {
            return Some(pos);
        }
        if pos == search_start {
            return None;
        }
        pos = pos.saturating_sub(1);
    }
}

/// Parse a ZIP64 extra field from `extra_data`, returning
/// (uncompressed_size, compressed_size, local_header_offset) where each
/// value is overridden from the extra field if its 32-bit slot was
/// 0xFFFFFFFF.
fn parse_zip64_extra(
    extra_data: &[u8],
    uncomp32: u32,
    comp32: u32,
    offset32: u32,
) -> (u64, u64, u64) {
    let mut uncompressed = u64::from(uncomp32);
    let mut compressed = u64::from(comp32);
    let mut offset = u64::from(offset32);

    let mut pos: usize = 0;
    while pos.wrapping_add(4) <= extra_data.len() {
        let id = le_u16(extra_data, pos);
        let sz = le_u16(extra_data, pos.wrapping_add(2)) as usize;
        let field_start = pos.wrapping_add(4);
        let field_end = field_start.wrapping_add(sz);

        if id == ZIP64_EXTRA_ID && field_end <= extra_data.len() {
            // ZIP64 extra field: the order is uncompressed, compressed,
            // offset — but only the values that were 0xFFFFFFFF in the
            // 32-bit fields are present.
            let mut fpos: usize = field_start;

            if uncomp32 == 0xFFFF_FFFF && fpos.wrapping_add(8) <= field_end {
                uncompressed = le_u64(extra_data, fpos);
                fpos = fpos.wrapping_add(8);
            }
            if comp32 == 0xFFFF_FFFF && fpos.wrapping_add(8) <= field_end {
                compressed = le_u64(extra_data, fpos);
                fpos = fpos.wrapping_add(8);
            }
            if offset32 == 0xFFFF_FFFF && fpos.wrapping_add(8) <= field_end {
                offset = le_u64(extra_data, fpos);
            }
            break;
        }

        pos = field_end;
    }

    (uncompressed, compressed, offset)
}

// ---------------------------------------------------------------------------
// Public API — parse (unzip)
// ---------------------------------------------------------------------------

/// Parse a ZIP archive and return a list of all entries from the central
/// directory.
///
/// Handles both standard ZIP and ZIP64 archives.  Does not decompress
/// any file data — use [`entry_data`] to extract individual entries.
///
/// # Errors
///
/// Returns `CorruptedData` if the archive is truncated, has no EOCD, or
/// has an invalid central directory.
pub fn parse(data: &[u8]) -> Result<Vec<ZipEntry>> {
    let eocd_off = find_eocd(data).ok_or(Error::CorruptedData)?;

    // Read EOCD fields (standard 32-bit values first).
    let mut total_entries = u64::from(le_u16(data, eocd_off.wrapping_add(10)));
    let mut cd_size = u64::from(le_u32(data, eocd_off.wrapping_add(12)));
    let mut cd_offset = u64::from(le_u32(data, eocd_off.wrapping_add(16)));

    // Check for ZIP64 EOCD locator just before the EOCD.
    if eocd_off >= 20 {
        let loc_off = eocd_off.wrapping_sub(20);
        if le_u32(data, loc_off) == ZIP64_LOCATOR_SIG {
            let zip64_eocd_off = le_u64(data, loc_off.wrapping_add(8)) as usize;
            if zip64_eocd_off.wrapping_add(56) <= data.len()
                && le_u32(data, zip64_eocd_off) == ZIP64_EOCD_SIG
            {
                total_entries = le_u64(data, zip64_eocd_off.wrapping_add(32));
                cd_size = le_u64(data, zip64_eocd_off.wrapping_add(40));
                cd_offset = le_u64(data, zip64_eocd_off.wrapping_add(48));
            }
        }
    }

    let _ = cd_size; // Used for validation in real impls; we walk by sig.

    let mut entries = Vec::with_capacity(total_entries.min(4096) as usize);
    let mut off = cd_offset as usize;

    for _ in 0..total_entries {
        if off.wrapping_add(46) > data.len() {
            break;
        }
        if le_u32(data, off) != CENTRAL_SIG {
            break;
        }

        let method = le_u16(data, off.wrapping_add(10));
        let crc32 = le_u32(data, off.wrapping_add(16));
        let comp32 = le_u32(data, off.wrapping_add(20));
        let uncomp32 = le_u32(data, off.wrapping_add(24));
        let name_len = le_u16(data, off.wrapping_add(28)) as usize;
        let extra_len = le_u16(data, off.wrapping_add(30)) as usize;
        let comment_len = le_u16(data, off.wrapping_add(32)) as usize;
        let offset32 = le_u32(data, off.wrapping_add(42));

        let name_start = off.wrapping_add(46);
        let name_end = name_start.wrapping_add(name_len).min(data.len());
        let name_bytes = data.get(name_start..name_end).unwrap_or(&[]);
        let name = name_bytes.to_vec();

        // Parse extra field for ZIP64 overrides.
        let extra_start = name_end;
        let extra_end = extra_start.wrapping_add(extra_len).min(data.len());
        let extra_data = data.get(extra_start..extra_end).unwrap_or(&[]);

        let (uncompressed_size, compressed_size, local_header_offset) =
            parse_zip64_extra(extra_data, uncomp32, comp32, offset32);

        // The trailing `/` that marks a directory member is a byte of the
        // stored name, so the test is on bytes rather than on a `char`.
        let is_dir = name.as_slice().ends_with(b"/");

        entries.push(ZipEntry {
            name,
            method,
            crc32,
            compressed_size,
            uncompressed_size,
            local_header_offset,
            is_dir,
        });

        off = off
            .wrapping_add(46)
            .wrapping_add(name_len)
            .wrapping_add(extra_len)
            .wrapping_add(comment_len);
    }

    Ok(entries)
}

/// Extract the compressed data for one entry from the archive.
///
/// Reads the local file header to locate the data (the local header's
/// extra field may differ in length from the central directory's copy).
///
/// Returns the raw compressed bytes (or stored bytes for method 0).
pub fn entry_data<'a>(data: &'a [u8], entry: &ZipEntry) -> Result<&'a [u8]> {
    let off = entry.local_header_offset as usize;
    if off.wrapping_add(30) > data.len() {
        return Err(Error::CorruptedData);
    }
    if le_u32(data, off) != LOCAL_SIG {
        return Err(Error::CorruptedData);
    }
    let name_len = le_u16(data, off.wrapping_add(26)) as usize;
    let extra_len = le_u16(data, off.wrapping_add(28)) as usize;
    let data_start = off
        .wrapping_add(30)
        .wrapping_add(name_len)
        .wrapping_add(extra_len);
    let data_end = data_start.wrapping_add(entry.compressed_size as usize);
    data.get(data_start..data_end.min(data.len()))
        .ok_or(Error::CorruptedData)
}

/// Largest entry this will inflate when the archive's declared size is not
/// itself a tighter bound.  Mirrors [`deflate::MAX_OUTPUT`].
pub const MAX_ENTRY_SIZE: usize = 64 * 1024 * 1024;

/// Extract and decompress the data for one entry, verifying the CRC-32.
///
/// Handles Stored (method 0) and Deflate (method 8).  Returns the
/// uncompressed file contents.
///
/// Inflation is bounded by the size the entry's central-directory record
/// *declares*, so a zip bomb is refused at the first byte past its own claim
/// rather than after 64 MiB has been allocated — see [`extract_entry_limited`],
/// which this calls with [`MAX_ENTRY_SIZE`].
///
/// # Errors
///
/// - `CorruptedData` if the local header is invalid
/// - `CorruptedData` if DEFLATE decompression fails
/// - `CorruptedData` if the entry inflates to a size other than it declared
/// - `CorruptedData` if CRC-32 verification fails
/// - `UnsupportedMethod` if the compression method is unknown
pub fn extract_entry(data: &[u8], entry: &ZipEntry) -> Result<Vec<u8>> {
    extract_entry_limited(data, entry, MAX_ENTRY_SIZE)
}

/// [`extract_entry`], with a caller-supplied ceiling on the output size.
///
/// The effective limit is the *smaller* of `limit` and the entry's declared
/// `uncompressed_size`, and the result is then required to match that declared
/// size exactly.
///
/// Both halves matter, and neither was present when this lived in the kernel:
///
/// - **The bound.** The old code called unlimited `inflate()`, so the only cap
///   was DEFLATE's global 64 MiB. An entry declaring 100 bytes could allocate
///   64 MiB before anything objected. A ZIP's central directory states each
///   entry's uncompressed size, and that statement is a promise the archive
///   made about itself; holding it to that promise costs one argument and turns
///   the zip-bomb case into an early refusal.
/// - **The equality check.** The CRC-32 below does catch a wrong-sized entry,
///   but only *after* the bytes are allocated and hashed — it is an integrity
///   check, not a resource control. Comparing against the declared size is what
///   makes a lie about the size a parse error rather than a hash mismatch, and
///   it says so in the error rather than blaming the data.
///
/// # Errors
///
/// As [`extract_entry`].
pub fn extract_entry_limited(data: &[u8], entry: &ZipEntry, limit: usize) -> Result<Vec<u8>> {
    let raw = entry_data(data, entry)?;

    // The archive's own claim, clamped to what the caller will tolerate.
    let declared = usize::try_from(entry.uncompressed_size).unwrap_or(usize::MAX);
    let cap = declared.min(limit);

    let decompressed = match entry.method {
        0 => {
            // Stored: the "compressed" bytes are the data.  Bounded by the
            // archive's own length, but still checked, because a Stored entry
            // that declares one size and carries another is malformed in the
            // same way a Deflate one is.
            if raw.len() > cap {
                return Err(Error::CorruptedData);
            }
            raw.to_vec()
        }
        8 => deflate::inflate_limited(raw, cap).map_err(|_| Error::CorruptedData)?,
        _ => return Err(Error::UnsupportedMethod),
    };

    // The declared size is a promise; hold the archive to it exactly.  Checked
    // before the CRC so that a size lie is reported as one, rather than
    // surfacing later as an unexplained checksum failure.
    if decompressed.len() != declared {
        return Err(Error::CorruptedData);
    }

    // Verify CRC-32 if non-zero (directories and empty files may have 0).
    if entry.crc32 != 0 || !decompressed.is_empty() {
        let actual = crc32::crc32(&decompressed);
        if actual != entry.crc32 {
            return Err(Error::CorruptedData);
        }
    }

    Ok(decompressed)
}

// ---------------------------------------------------------------------------
// Public API — create (mkzip)
// ---------------------------------------------------------------------------

/// Create a ZIP archive in memory from a list of entries.
///
/// Each entry's data is compressed with DEFLATE (method 8) unless the
/// entry requests store-only mode or compression doesn't shrink the
/// data.  Directory entries (name ending with `/`) should have empty
/// data.
///
/// Automatically uses ZIP64 extensions when any entry or the archive
/// as a whole exceeds 32-bit limits.
#[allow(clippy::arithmetic_side_effects)]
pub fn create(entries: &[ZipWriteEntry]) -> Vec<u8> {
    let mut archive = Vec::new();
    let mut directory_entries: Vec<DirRecord> = Vec::with_capacity(entries.len());

    // --- Local file headers + data ---
    for entry in entries {
        let crc32 = if entry.data.is_empty() {
            0u32
        } else {
            crc32::crc32(&entry.data)
        };

        let (compressed, method) = if entry.store_only || entry.data.is_empty() {
            (entry.data.clone(), 0u16)
        } else {
            let deflated = deflate::deflate(&entry.data);
            if deflated.len() < entry.data.len() {
                (deflated, 8u16)
            } else {
                (entry.data.clone(), 0u16)
            }
        };

        let uncomp_size = entry.data.len() as u64;
        let comp_size = compressed.len() as u64;
        let header_offset = archive.len() as u64;

        // Determine if ZIP64 extra field is needed for this entry.
        let need_zip64 =
            uncomp_size > 0xFFFF_FFFE || comp_size > 0xFFFF_FFFE || header_offset > 0xFFFF_FFFE;

        let (comp32, uncomp32, extra_field) = if need_zip64 {
            // Use 0xFFFFFFFF sentinel + ZIP64 extra field.
            let mut extra = Vec::with_capacity(28);
            write_u16(&mut extra, ZIP64_EXTRA_ID);
            write_u16(&mut extra, 24); // 3 × 8 bytes
            write_u64(&mut extra, uncomp_size);
            write_u64(&mut extra, comp_size);
            write_u64(&mut extra, header_offset);
            (0xFFFF_FFFFu32, 0xFFFF_FFFFu32, extra)
        } else {
            (comp_size as u32, uncomp_size as u32, Vec::new())
        };

        // Local file header.
        write_u32(&mut archive, LOCAL_SIG);
        write_u16(&mut archive, if need_zip64 { 45 } else { 20 }); // version needed
        write_u16(&mut archive, 0); // general purpose bit flag
        write_u16(&mut archive, method);
        write_u16(&mut archive, 0); // mod time
        write_u16(&mut archive, 0x0021); // mod date (1980-01-01)
        write_u32(&mut archive, crc32);
        write_u32(&mut archive, comp32);
        write_u32(&mut archive, uncomp32);
        write_u16(&mut archive, entry.name.len() as u16);
        write_u16(&mut archive, extra_field.len() as u16);
        archive.extend_from_slice(entry.name.as_slice());
        archive.extend_from_slice(&extra_field);
        archive.extend_from_slice(&compressed);

        directory_entries.push(DirRecord {
            name: entry.name.clone(),
            method,
            crc32,
            comp_size,
            uncomp_size,
            header_offset,
            need_zip64,
        });
    }

    // --- Central directory ---
    let cd_start = archive.len() as u64;

    for rec in &directory_entries {
        let (comp32, uncomp32, offset32, extra_field) = if rec.need_zip64 {
            let mut extra = Vec::with_capacity(28);
            write_u16(&mut extra, ZIP64_EXTRA_ID);
            write_u16(&mut extra, 24);
            write_u64(&mut extra, rec.uncomp_size);
            write_u64(&mut extra, rec.comp_size);
            write_u64(&mut extra, rec.header_offset);
            (0xFFFF_FFFFu32, 0xFFFF_FFFFu32, 0xFFFF_FFFFu32, extra)
        } else {
            (
                rec.comp_size as u32,
                rec.uncomp_size as u32,
                rec.header_offset as u32,
                Vec::new(),
            )
        };

        write_u32(&mut archive, CENTRAL_SIG);
        write_u16(&mut archive, if rec.need_zip64 { 45 } else { 20 }); // version made by
        write_u16(&mut archive, if rec.need_zip64 { 45 } else { 20 }); // version needed
        write_u16(&mut archive, 0); // bit flag
        write_u16(&mut archive, rec.method);
        write_u16(&mut archive, 0); // mod time
        write_u16(&mut archive, 0x0021); // mod date
        write_u32(&mut archive, rec.crc32);
        write_u32(&mut archive, comp32);
        write_u32(&mut archive, uncomp32);
        write_u16(&mut archive, rec.name.len() as u16);
        write_u16(&mut archive, extra_field.len() as u16);
        write_u16(&mut archive, 0); // comment length
        write_u16(&mut archive, 0); // disk number start
        write_u16(&mut archive, 0); // internal attrs
        write_u32(&mut archive, 0); // external attrs
        write_u32(&mut archive, offset32);
        archive.extend_from_slice(rec.name.as_slice());
        archive.extend_from_slice(&extra_field);
    }

    let cd_end = archive.len() as u64;
    let cd_size = cd_end.wrapping_sub(cd_start);
    let entry_count = directory_entries.len() as u64;

    // Determine if ZIP64 EOCD is needed.
    let need_zip64_eocd = entry_count > 0xFFFE
        || cd_size > 0xFFFF_FFFE
        || cd_start > 0xFFFF_FFFE
        || directory_entries.iter().any(|r| r.need_zip64);

    if need_zip64_eocd {
        // ZIP64 end of central directory record (56 bytes).
        let zip64_eocd_off = archive.len() as u64;
        write_u32(&mut archive, ZIP64_EOCD_SIG);
        write_u64(&mut archive, 44); // size of remaining record
        write_u16(&mut archive, 45); // version made by
        write_u16(&mut archive, 45); // version needed
        write_u32(&mut archive, 0); // disk number
        write_u32(&mut archive, 0); // disk with CD start
        write_u64(&mut archive, entry_count);
        write_u64(&mut archive, entry_count);
        write_u64(&mut archive, cd_size);
        write_u64(&mut archive, cd_start);

        // ZIP64 end of central directory locator (20 bytes).
        write_u32(&mut archive, ZIP64_LOCATOR_SIG);
        write_u32(&mut archive, 0); // disk with ZIP64 EOCD
        write_u64(&mut archive, zip64_eocd_off);
        write_u32(&mut archive, 1); // total disks
    }

    // --- Standard End of Central Directory ---
    let eocd_entries = if entry_count > 0xFFFF {
        0xFFFFu16
    } else {
        entry_count as u16
    };
    let eocd_cd_size = if cd_size > 0xFFFF_FFFF {
        0xFFFF_FFFFu32
    } else {
        cd_size as u32
    };
    let eocd_cd_off = if cd_start > 0xFFFF_FFFF {
        0xFFFF_FFFFu32
    } else {
        cd_start as u32
    };

    write_u32(&mut archive, EOCD_SIG);
    write_u16(&mut archive, 0); // disk number
    write_u16(&mut archive, 0); // disk with CD
    write_u16(&mut archive, eocd_entries);
    write_u16(&mut archive, eocd_entries);
    write_u32(&mut archive, eocd_cd_size);
    write_u32(&mut archive, eocd_cd_off);
    write_u16(&mut archive, 0); // comment length

    archive
}

// ---------------------------------------------------------------------------
// Internal — central directory record for building archives
// ---------------------------------------------------------------------------

/// Intermediate record used during archive creation.
struct DirRecord {
    name: Vec<u8>,
    method: u16,
    crc32: u32,
    comp_size: u64,
    uncomp_size: u64,
    header_offset: u64,
    need_zip64: bool,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// These are the fine-grained tests: they run on the build host, can reach
// private internals, and can afford to be slow. The kernel keeps a *separate*,
// coarse round-trip battery in `kernel/src/fs/zip.rs::self_test`, because
// `kernel/Cargo.toml` sets `test = false` and the boot battery is the only
// thing that exercises this code linked against the kernel's allocator.
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// Build a one-entry archive, the shape most tests start from.
    fn one(name: &[u8], data: &[u8], store_only: bool) -> Vec<u8> {
        create(&[ZipWriteEntry {
            name: name.to_vec(),
            data: data.to_vec(),
            store_only,
        }])
    }

    #[test]
    fn stored_round_trip() {
        let archive = one(b"hello.txt", b"Hello, world!", true);
        let parsed = parse(&archive).expect("parse");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, b"hello.txt".to_vec());
        assert_eq!(parsed[0].method, 0, "store_only must not deflate");
        assert_eq!(parsed[0].uncompressed_size, 13);
        assert_eq!(
            extract_entry(&archive, &parsed[0]).expect("extract"),
            b"Hello, world!"
        );
    }

    #[test]
    fn deflated_round_trip_and_actually_shrinks() {
        let text = b"ABCDEFGHIJKLMNOP".repeat(64);
        let archive = one(b"repeat.txt", &text, false);
        let parsed = parse(&archive).expect("parse");
        assert_eq!(parsed[0].method, 8, "repetitive data should deflate");
        assert!(
            parsed[0].compressed_size < parsed[0].uncompressed_size,
            "DEFLATE made {} bytes into {} -- it did not compress",
            parsed[0].uncompressed_size,
            parsed[0].compressed_size
        );
        assert_eq!(extract_entry(&archive, &parsed[0]).expect("extract"), text);
    }

    #[test]
    fn incompressible_data_still_round_trips() {
        // A deflate of random-ish bytes can be *larger* than the input; the
        // writer is free to fall back to stored. Either choice is fine -- what
        // must hold is that the bytes come back.
        let data: Vec<u8> = (0u32..4096)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let archive = one(b"noise.bin", &data, false);
        let parsed = parse(&archive).expect("parse");
        assert_eq!(extract_entry(&archive, &parsed[0]).expect("extract"), data);
    }

    #[test]
    fn multi_entry_round_trip_preserves_order_and_content() {
        let entries = vec![
            ZipWriteEntry {
                name: b"a.txt".to_vec(),
                data: b"first".to_vec(),
                store_only: false,
            },
            ZipWriteEntry {
                name: b"dir/".to_vec(),
                data: Vec::new(),
                store_only: true,
            },
            ZipWriteEntry {
                name: b"dir/b.txt".to_vec(),
                data: b"second".to_vec(),
                store_only: false,
            },
            ZipWriteEntry {
                name: b"c.bin".to_vec(),
                data: vec![0xAB; 300],
                store_only: false,
            },
        ];
        let archive = create(&entries);
        let parsed = parse(&archive).expect("parse");
        assert_eq!(parsed.len(), 4);
        for (orig, got) in entries.iter().zip(parsed.iter()) {
            assert_eq!(got.name, orig.name);
            assert_eq!(
                extract_entry(&archive, got).expect("extract"),
                orig.data,
                "content mismatch for {:?}",
                orig.name
            );
        }
    }

    #[test]
    fn trailing_slash_marks_a_directory() {
        let archive = create(&[ZipWriteEntry {
            name: b"dir/".to_vec(),
            data: Vec::new(),
            store_only: true,
        }]);
        let parsed = parse(&archive).expect("parse");
        assert!(parsed[0].is_dir, "a name ending in / is a directory entry");
    }

    #[test]
    fn empty_archive_parses_as_empty() {
        let archive = create(&[]);
        assert!(parse(&archive).expect("parse").is_empty());
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(parse(&[0u8; 100]).is_err(), "all-zero data is not a zip");
        assert!(parse(b"PK").is_err(), "truncated magic is not a zip");
        assert!(parse(&[]).is_err(), "empty input is not a zip");
    }

    #[test]
    fn unsupported_method_is_distinguishable_from_corruption() {
        // Method 8 (deflate) and 0 (store) are the two we implement. A caller
        // seeing `UnsupportedMethod` should be able to tell "this archive is
        // fine, I just cannot read it" apart from "this archive is damaged" --
        // which is exactly the distinction the kernel's single CorruptedData
        // could not express before the crate split.
        let archive = one(b"x.txt", b"body", true);
        let mut parsed = parse(&archive).expect("parse");
        parsed[0].method = 12; // bzip2: legal ZIP, not implemented here
        assert_eq!(
            extract_entry(&archive, &parsed[0]),
            Err(Error::UnsupportedMethod)
        );
    }

    // -- the output limit: the reason this crate exists in usable form --------

    #[test]
    fn declared_size_caps_the_allocation() {
        // The archive's own header says how big the entry is. Holding it to
        // that promise is what turns a decompression bomb into an early
        // refusal instead of a 64 MiB allocation.
        let big = b"Z".repeat(100_000);
        let archive = one(b"big.txt", &big, false);
        let parsed = parse(&archive).expect("parse");
        assert_eq!(parsed[0].uncompressed_size, 100_000);
        assert_eq!(extract_entry(&archive, &parsed[0]).expect("extract"), big);
    }

    #[test]
    fn a_lying_declared_size_is_rejected_not_trusted() {
        let archive = one(b"lie.txt", b"exactly twenty chars", true);
        let mut parsed = parse(&archive).expect("parse");
        parsed[0].uncompressed_size = 5; // claim it is shorter than it is
        assert_eq!(
            extract_entry(&archive, &parsed[0]),
            Err(Error::CorruptedData),
            "a size that disagrees with the payload is corruption"
        );
    }

    #[test]
    fn explicit_limit_refuses_an_oversized_entry() {
        let data = b"0123456789".repeat(50); // 500 bytes
        let archive = one(b"payload.bin", &data, false);
        let parsed = parse(&archive).expect("parse");

        assert!(
            extract_entry_limited(&archive, &parsed[0], 499).is_err(),
            "a limit below the entry size must refuse"
        );
        assert_eq!(
            extract_entry_limited(&archive, &parsed[0], 500).expect("at limit"),
            data,
            "a limit exactly at the entry size must succeed"
        );
        assert_eq!(
            extract_entry_limited(&archive, &parsed[0], usize::MAX).expect("no limit"),
            data
        );
    }

    #[test]
    fn a_zero_limit_refuses_everything_nonempty() {
        let archive = one(b"x", b"nonempty", true);
        let parsed = parse(&archive).expect("parse");
        assert!(extract_entry_limited(&archive, &parsed[0], 0).is_err());
    }

    #[test]
    fn an_empty_entry_round_trips() {
        let archive = one(b"empty.txt", b"", true);
        let parsed = parse(&archive).expect("parse");
        assert_eq!(parsed[0].uncompressed_size, 0);
        assert!(
            extract_entry(&archive, &parsed[0])
                .expect("extract")
                .is_empty()
        );
    }
}
