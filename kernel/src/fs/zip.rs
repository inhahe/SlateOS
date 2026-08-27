//! ZIP archives for the kernel — a thin shim over the `zip` crate.
//!
//! The reader and writer used to live here, all eight hundred lines of them.
//! They were promoted to a root crate for the same reason `compress.rs`'s
//! DEFLATE codec was: a module of a *binary* crate cannot be depended on, so
//! `apps/archivemanager` could not name `crate::fs::zip` and was left listing
//! archives it had no way to open
//! (`requests/c-a-zip-is-trapped-in-the-kernel-binary.md`). The alternative to
//! promoting it was lane C writing a second ZIP parser — and a second parser
//! of untrusted input is not a second ordinary function. Each one has to get
//! the same central-directory, Zip64 and CRC handling right, and a bug fixed
//! in one is not fixed in the other.
//!
//! What remains here is the kernel's *view* of that crate:
//!
//! - the names the in-kernel call sites already use — [`parse`],
//!   [`extract_entry`], [`create`] and [`ZipWriteEntry`];
//! - [`to_kernel_error`], which maps [`ziparchive::Error`] onto [`KernelError`];
//! - [`self_test`], which stays in the kernel because `kernel/Cargo.toml` sets
//!   `test = false` — the boot battery is the only thing that runs an
//!   assertion in kernel context, and it is worth knowing that the parser
//!   works *there*, linked against the kernel's allocator, and not only on the
//!   build host.
//!
//! The crate's own `#[cfg(test)]` suite is the finer-grained one: it can reach
//! private internals and covers the size-limit and lying-header cases
//! exhaustively. This file's self-test is deliberately the coarse round-trip
//! battery.
//!
//! ## Entry names are bytes, not `Path`
//!
//! [`ziparchive::ZipEntry::name`] is a `Vec<u8>` rather than a `PathBuf`, because the
//! crate is `no_std` and cannot depend on the kernel's path type. That is not
//! a downgrade. A ZIP entry name is an arbitrary byte string taken from an
//! untrusted archive, and it is *not yet a path*: it becomes one only after
//! `fs/archive.rs` has confined it under a destination directory. Keeping the
//! two types distinct makes that step harder to skip by accident. Callers that
//! genuinely want a path wrap the bytes with `Path::new(&entry.name)`, which
//! costs nothing — the kernel's `Path` is a `[u8]` newtype.

use crate::error::{KernelError, KernelResult};
use alloc::vec::Vec;

// Only the names the kernel actually calls are re-exported. `ziparchive` also
// offers `extract_entry_limited` and `MAX_ENTRY_SIZE`, deliberately not
// mirrored here: no in-kernel caller has a bound tighter than the crate's own
// 64 MiB default, and a re-export nothing calls is a warning today and a
// misleading piece of API surface tomorrow. A kernel caller that acquires a
// real bound should add the wrapper then, with the bound's justification
// attached. Callers outside the kernel use the crate directly and already have
// both.
pub use ziparchive::{ZipEntry, ZipWriteEntry};

/// Map a [`ziparchive::Error`] onto the kernel's error type.
///
/// The distinction worth preserving is that [`ziparchive::Error::UnsupportedMethod`]
/// is not damage: the archive is well-formed and we simply do not implement
/// its codec (bzip2, LZMA, XZ…). A caller that reports "unsupported" to the
/// user is right; one that reports "corrupt" sends them looking for a fault in
/// a file that has none. The in-kernel copy collapsed both into
/// `CorruptedData` and could not tell them apart.
#[must_use]
pub fn to_kernel_error(err: ziparchive::Error) -> KernelError {
    match err {
        ziparchive::Error::UnsupportedMethod => KernelError::NotSupported,
        ziparchive::Error::CorruptedData => KernelError::CorruptedData,
    }
}

/// Parse a ZIP archive's central directory into its entry list.
///
/// # Errors
///
/// [`KernelError::CorruptedData`] if the archive is truncated, has no
/// end-of-central-directory record, or its headers disagree with each other.
pub fn parse(data: &[u8]) -> KernelResult<Vec<ZipEntry>> {
    ziparchive::parse(data).map_err(to_kernel_error)
}

/// Borrow an entry's raw, still-compressed bytes from the archive.
///
/// # Errors
///
/// [`KernelError::CorruptedData`] if the entry's local header is missing or
/// its data runs past the end of the archive.
pub fn entry_data<'a>(data: &'a [u8], entry: &ZipEntry) -> KernelResult<&'a [u8]> {
    ziparchive::entry_data(data, entry).map_err(to_kernel_error)
}

/// Decompress one entry, capped at `ziparchive::MAX_ENTRY_SIZE` (64 MiB).
///
/// # Errors
///
/// [`KernelError::CorruptedData`] if the entry fails its CRC-32 or does not
/// decompress to the size its own header declared, or
/// [`KernelError::NotSupported`] if it uses a codec other than stored or
/// DEFLATE.
pub fn extract_entry(data: &[u8], entry: &ZipEntry) -> KernelResult<Vec<u8>> {
    ziparchive::extract_entry(data, entry).map_err(to_kernel_error)
}

/// Build a ZIP archive from `entries`.
#[must_use]
pub fn create(entries: &[ZipWriteEntry]) -> Vec<u8> {
    ziparchive::create(entries)
}

/// Coarse round-trip battery, run during kernel boot.
///
/// Deliberately coarse: the exhaustive cases — size limits, lying headers,
/// unsupported codecs — are in the crate's `#[cfg(test)]` suite, which runs on
/// the build host. What this checks is that the same code still works *here*,
/// linked against the kernel's allocator and the kernel's `deflate`.
///
/// # Errors
///
/// [`KernelError::CorruptedData`] if any round trip does not return what went
/// into it, or if a deliberately-damaged archive is accepted.
pub fn self_test() -> KernelResult<()> {
    use crate::fs::path::Path;
    use alloc::vec;

    crate::serial_println!("[zip] Running self-test...");

    // --- 1: stored round trip -------------------------------------------
    {
        let archive = create(&[ZipWriteEntry {
            name: b"hello.txt".to_vec(),
            data: b"Hello, world!".to_vec(),
            store_only: true,
            dos_datetime: 0, // no mtime to record; see rung 3 for the other case
        }]);
        let parsed = parse(&archive)?;
        let entry = parsed.first().ok_or(KernelError::CorruptedData)?;
        if Path::new(&entry.name) != Path::new("hello.txt")
            || entry.method != 0
            || entry.uncompressed_size != 13
        {
            return Err(KernelError::CorruptedData);
        }
        if extract_entry(&archive, entry)? != b"Hello, world!" {
            return Err(KernelError::CorruptedData);
        }
        crate::serial_println!("[zip]   stored round-trip OK");
    }

    // --- 2: deflated round trip, and it must actually compress -----------
    {
        let mut text = Vec::with_capacity(1024);
        for _ in 0..64 {
            text.extend_from_slice(b"ABCDEFGHIJKLMNOP");
        }
        let archive = create(&[ZipWriteEntry {
            name: b"repeat.txt".to_vec(),
            data: text.clone(),
            store_only: false,
            dos_datetime: 0,
        }]);
        let parsed = parse(&archive)?;
        let entry = parsed.first().ok_or(KernelError::CorruptedData)?;
        if entry.method != 8 || entry.compressed_size >= entry.uncompressed_size {
            return Err(KernelError::CorruptedData);
        }
        if extract_entry(&archive, entry)? != text {
            return Err(KernelError::CorruptedData);
        }
        crate::serial_println!(
            "[zip]   deflated round-trip OK ({} -> {} bytes)",
            entry.uncompressed_size,
            entry.compressed_size
        );
    }

    // --- 3: several entries, including a directory and a binary payload --
    //
    // Also the one rung that records modification times. The stamps differ per
    // entry, and one of them is deliberately `0` ("not recorded") in the
    // *middle* of the archive rather than at an end: a writer that stamped
    // every member from a single variable, or that skipped the field for one
    // entry and shifted the rest, would still pass if the absent one were
    // first or last.
    {
        // 2026-08-26 14:30:00 and 1999-12-31 23:59:58, packed as
        // `(date << 16) | time`; see `ziparchive::ZipEntry::dos_datetime` for
        // the field layout. Both are far from the 1980 epoch, and the second
        // is the last second of a year with an odd second count -- DOS stores
        // seconds halved, so 58 is chosen because it survives the division
        // exactly and a mangled field cannot hide behind rounding.
        const T_2026: u32 = (0x5D1A_u32 << 16) | 0x73C0;
        const T_1999: u32 = (0x279F_u32 << 16) | 0xBF7D;
        let originals = [
            (b"a.txt".to_vec(), b"first".to_vec(), false, T_2026),
            (b"dir/".to_vec(), Vec::new(), true, 0),
            (b"dir/b.txt".to_vec(), b"second".to_vec(), false, T_1999),
            (b"c.bin".to_vec(), vec![0xAB_u8; 300], false, T_2026),
        ];
        let writes: Vec<ZipWriteEntry> = originals
            .iter()
            .map(|(name, data, store_only, dos_datetime)| ZipWriteEntry {
                name: name.clone(),
                data: data.clone(),
                store_only: *store_only,
                dos_datetime: *dos_datetime,
            })
            .collect();
        let archive = create(&writes);
        let parsed = parse(&archive)?;
        if parsed.len() != originals.len() {
            return Err(KernelError::CorruptedData);
        }
        for ((name, data, _, dos_datetime), entry) in originals.iter().zip(parsed.iter()) {
            if &entry.name != name
                || &extract_entry(&archive, entry)? != data
                || entry.dos_datetime != *dos_datetime
            {
                return Err(KernelError::CorruptedData);
            }
        }
        crate::serial_println!("[zip]   multi-entry round-trip OK (4 entries, 3 timestamped)");
    }

    // --- 4: a damaged payload must be caught, not returned ---------------
    //
    // The CRC is the only thing between a corrupted archive and a caller that
    // trusts what it extracts, so the boot battery checks it here and not only
    // on the host.
    {
        let archive = create(&[ZipWriteEntry {
            name: b"data.txt".to_vec(),
            data: b"The quick brown fox jumps over the lazy dog".to_vec(),
            store_only: true,
            dos_datetime: 0,
        }]);
        let parsed = parse(&archive)?;
        let entry = parsed.first().ok_or(KernelError::CorruptedData)?;
        // Find the stored payload by where `entry_data` borrowed it from,
        // rather than by recomputing the local header's length here -- a
        // second copy of that arithmetic could drift from the real one and
        // leave this test flipping a byte that is not payload at all.
        let offset = {
            let raw = entry_data(&archive, entry)?;
            (raw.as_ptr() as usize).saturating_sub(archive.as_ptr() as usize)
        };
        let mut corrupt = archive.clone();
        *corrupt.get_mut(offset).ok_or(KernelError::CorruptedData)? ^= 0xFF;
        if extract_entry(&corrupt, entry).is_ok() {
            crate::serial_println!("[zip]   ERROR: corruption not detected");
            return Err(KernelError::CorruptedData);
        }
        crate::serial_println!("[zip]   CRC-32 corruption detection OK");
    }

    // --- 5: an empty archive is valid; non-archives are not --------------
    {
        if !parse(&create(&[]))?.is_empty() {
            return Err(KernelError::CorruptedData);
        }
        if parse(&[0_u8; 100]).is_ok() || parse(b"PK").is_ok() {
            return Err(KernelError::CorruptedData);
        }
        crate::serial_println!("[zip]   magic validation OK");
    }

    crate::serial_println!("[zip] Self-test passed.");
    Ok(())
}
