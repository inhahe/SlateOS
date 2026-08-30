//! Bounds-checked little-endian reads over untrusted on-disk bytes.
//!
//! Every field the NTFS parser reads comes from a disk that may be corrupt,
//! truncated, or hostile, so every read is fallible by construction: these
//! helpers return `None` past the end of the buffer rather than panicking.
//! That is not merely lint compliance (`indexing_slicing`,
//! `arithmetic_side_effects`) — a filesystem parser that panics on a short
//! buffer turns a bad USB stick into a kernel panic.
//!
//! NTFS is little-endian throughout, so there is no big-endian counterpart.

use alloc::string::String;

/// Read a `u8` at `off`.
pub fn u8_at(buf: &[u8], off: usize) -> Option<u8> {
    buf.get(off).copied()
}

/// Read an `i8` at `off`.
///
/// NTFS uses signed bytes for the "size is a power of two" encoding of
/// cluster and record sizes, so this is a real field type, not a cast.
pub fn i8_at(buf: &[u8], off: usize) -> Option<i8> {
    #[allow(clippy::cast_possible_wrap)] // Deliberate: the field is signed.
    buf.get(off).map(|b| *b as i8)
}

/// Read a little-endian `u16` at `off`.
pub fn u16_at(buf: &[u8], off: usize) -> Option<u16> {
    let end = off.checked_add(2)?;
    let bytes: [u8; 2] = buf.get(off..end)?.try_into().ok()?;
    Some(u16::from_le_bytes(bytes))
}

/// Read a little-endian `u32` at `off`.
pub fn u32_at(buf: &[u8], off: usize) -> Option<u32> {
    let end = off.checked_add(4)?;
    let bytes: [u8; 4] = buf.get(off..end)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

/// Read a little-endian `u64` at `off`.
pub fn u64_at(buf: &[u8], off: usize) -> Option<u64> {
    let end = off.checked_add(8)?;
    let bytes: [u8; 8] = buf.get(off..end)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

/// Read `len` UTF-16LE code units at `off` and decode to a `String`.
///
/// Unpaired surrogates are replaced with U+FFFD rather than rejected. NTFS
/// names are *not* required to be well-formed UTF-16 — Windows stores
/// arbitrary `u16` sequences — so rejecting would make a file with an odd
/// name unlistable, which is a worse outcome than a lossy display name for
/// that one entry. The names we cannot round-trip are exactly the ones no
/// portable tool can address anyway.
///
/// Returns `None` only if the range is out of bounds.
pub fn utf16le_at(buf: &[u8], off: usize, len_units: usize) -> Option<String> {
    let byte_len = len_units.checked_mul(2)?;
    let end = off.checked_add(byte_len)?;
    let slice = buf.get(off..end)?;

    let units = slice
        .chunks_exact(2)
        .filter_map(|c| Some(u16::from_le_bytes([*c.first()?, *c.get(1)?])));

    let mut out = String::with_capacity(len_units);
    for ch in char::decode_utf16(units) {
        out.push(ch.unwrap_or(char::REPLACEMENT_CHARACTER));
    }
    Some(out)
}

/// Convert a Windows `FILETIME` (100 ns ticks since 1601-01-01 UTC) to
/// nanoseconds since the Unix epoch, saturating at 0 for pre-1970 stamps.
///
/// A timestamp before 1970 is not corruption — NTFS genuinely represents it —
/// but [`crate::fs::vfs::FileMeta`] timestamps are unsigned nanoseconds since
/// the Unix epoch, so there is nowhere to put it. Saturating to 0 renders as
/// "not available", which is the honest answer for a value the VFS type
/// cannot hold; wrapping would render as a date in the far future.
pub fn filetime_to_unix_ns(filetime: u64) -> u64 {
    /// 100 ns ticks between 1601-01-01 and 1970-01-01.
    const EPOCH_DIFF_TICKS: u64 = 116_444_736_000_000_000;

    filetime
        .saturating_sub(EPOCH_DIFF_TICKS)
        .saturating_mul(100)
}
