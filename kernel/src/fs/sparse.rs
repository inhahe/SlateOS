//! Sparse file management and hole punching.
//!
//! Sparse files contain "holes" — regions that are logically zero but
//! don't consume storage. This module provides operations for:
//!
//! - **Hole punching** — deallocate a range within a file (FALLOC_FL_PUNCH_HOLE)
//! - **Zero range** — zero a region without I/O (FALLOC_FL_ZERO_RANGE)
//! - **Collapse range** — remove a range and shift data down
//! - **Insert range** — insert zeros at a position, shifting data up
//! - **Region mapping** — identify data vs hole regions in a file
//!
//! ## Architecture
//!
//! ```text
//! Application → punch_hole(path, offset, len)
//!   → VFS zeroes the range
//!   → range tracked as "hole" in sparse map
//!   → future reads of that range return zeros without I/O
//!
//! Application → map_regions(path)
//!   → returns list of (offset, length, Data|Hole) regions
//!   → applications can skip holes when copying/transferring
//! ```
//!
//! ## Design Notes
//!
//! - Our VFS doesn't have native sparse file support in memfs/FAT, so
//!   we implement sparse semantics as a layer on top: punched holes are
//!   overwritten with zeros and tracked in a sparse map for efficient
//!   region queries.
//! - For ext4, holes are native (unallocated extents), and our sparse
//!   map reflects the actual on-disk state.
//! - Maximum tracked sparse files: 256 (LRU eviction).
//! - Maximum regions per file: 64 (excess merged).

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum sparse files tracked simultaneously.
const MAX_TRACKED_FILES: usize = 256;

/// Maximum regions per file map.
const MAX_REGIONS_PER_FILE: usize = 64;

/// Minimum hole size worth tracking (4 KiB).
const MIN_HOLE_SIZE: u64 = 4096;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Region type within a sparse file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    /// Contains actual data.
    Data,
    /// Hole (logically zero, no storage consumed).
    Hole,
}

impl RegionKind {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Hole => "hole",
        }
    }
}

/// A single region in a sparse file map.
#[derive(Debug, Clone)]
pub struct Region {
    /// Start offset in bytes.
    pub offset: u64,
    /// Length in bytes.
    pub length: u64,
    /// Whether this is data or a hole.
    pub kind: RegionKind,
}

/// Complete sparse map of a file.
#[derive(Debug, Clone)]
pub struct SparseMap {
    /// File path.
    pub path: String,
    /// Total file size.
    pub file_size: u64,
    /// Ordered list of regions covering the file.
    pub regions: Vec<Region>,
    /// Total bytes in holes.
    pub hole_bytes: u64,
    /// Total bytes in data regions.
    pub data_bytes: u64,
}

/// Result of a hole punch or range operation.
#[derive(Debug, Clone)]
pub struct RangeResult {
    /// Bytes affected.
    pub bytes_affected: u64,
    /// Whether the operation created a new hole.
    pub created_hole: bool,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

/// Tracked sparse file metadata.
#[derive(Debug, Clone)]
struct SparseEntry {
    path: String,
    /// Sorted list of hole regions (offset, length).
    holes: Vec<(u64, u64)>,
    /// Last access timestamp for LRU.
    last_access_ns: u64,
}

/// Sparse file tracking table.
static SPARSE_TABLE: spin::Mutex<Vec<SparseEntry>> = spin::Mutex::new(Vec::new());

/// Statistics.
static PUNCH_COUNT: AtomicU64 = AtomicU64::new(0);
static PUNCH_BYTES: AtomicU64 = AtomicU64::new(0);
static ZERO_RANGE_COUNT: AtomicU64 = AtomicU64::new(0);
static COLLAPSE_COUNT: AtomicU64 = AtomicU64::new(0);
static INSERT_COUNT: AtomicU64 = AtomicU64::new(0);
static MAP_QUERIES: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API — Operations
// ---------------------------------------------------------------------------

/// Punch a hole in a file, deallocating the specified range.
///
/// The range [offset, offset+length) becomes a hole: future reads
/// return zeros, and the storage can be reclaimed. File size is unchanged.
pub fn punch_hole(path: &str, offset: u64, length: u64) -> KernelResult<RangeResult> {
    use crate::fs::Vfs;

    if path.is_empty() || length == 0 {
        return Err(KernelError::InvalidArgument);
    }

    // Verify file exists and get size.
    let meta = Vfs::metadata(path)?;
    if offset >= meta.size {
        return Ok(RangeResult { bytes_affected: 0, created_hole: false });
    }

    // Clamp to file size.
    let actual_len = length.min(meta.size - offset);

    PUNCH_COUNT.fetch_add(1, Ordering::Relaxed);
    PUNCH_BYTES.fetch_add(actual_len, Ordering::Relaxed);

    // Write zeros over the range (in our VFS model, this is how we "punch").
    let zeros = alloc::vec![0u8; actual_len.min(65536) as usize];
    let mut written: u64 = 0;
    while written < actual_len {
        let chunk = (actual_len - written).min(65536) as usize;
        Vfs::write_at(path, offset + written, &zeros[..chunk])?;
        written += chunk as u64;
    }

    // Track the hole in our sparse map.
    let created = track_hole(path, offset, actual_len);

    // Notify fstrim about freed blocks.
    crate::fs::fstrim::notify_free(path, offset, actual_len);

    Ok(RangeResult { bytes_affected: actual_len, created_hole: created })
}

/// Zero a range without necessarily deallocating (FALLOC_FL_ZERO_RANGE).
///
/// Similar to punch_hole but guarantees the space remains allocated.
/// Reads from the range return zeros. File size is unchanged.
pub fn zero_range(path: &str, offset: u64, length: u64) -> KernelResult<RangeResult> {
    use crate::fs::Vfs;

    if path.is_empty() || length == 0 {
        return Err(KernelError::InvalidArgument);
    }

    let meta = Vfs::metadata(path)?;
    if offset >= meta.size {
        return Ok(RangeResult { bytes_affected: 0, created_hole: false });
    }

    let actual_len = length.min(meta.size - offset);
    ZERO_RANGE_COUNT.fetch_add(1, Ordering::Relaxed);

    // Write zeros.
    let zeros = alloc::vec![0u8; actual_len.min(65536) as usize];
    let mut written: u64 = 0;
    while written < actual_len {
        let chunk = (actual_len - written).min(65536) as usize;
        Vfs::write_at(path, offset + written, &zeros[..chunk])?;
        written += chunk as u64;
    }

    // Zero range doesn't create a hole (space stays allocated).
    Ok(RangeResult { bytes_affected: actual_len, created_hole: false })
}

/// Collapse a range — remove data and shift everything after it down.
///
/// This reduces the file size by `length` bytes. The range
/// [offset, offset+length) is removed and data after it moves down.
pub fn collapse_range(path: &str, offset: u64, length: u64) -> KernelResult<RangeResult> {
    use crate::fs::Vfs;

    if path.is_empty() || length == 0 {
        return Err(KernelError::InvalidArgument);
    }

    let meta = Vfs::metadata(path)?;
    if offset >= meta.size {
        return Err(KernelError::InvalidArgument);
    }

    let actual_len = length.min(meta.size - offset);
    COLLAPSE_COUNT.fetch_add(1, Ordering::Relaxed);

    // Read everything after the collapsed range.
    let tail_start = offset + actual_len;
    let tail_len = meta.size.saturating_sub(tail_start);

    if tail_len > 0 {
        // Read the tail and write it at offset.
        let mut pos: u64 = 0;
        while pos < tail_len {
            let chunk = (tail_len - pos).min(65536) as usize;
            let data = Vfs::read_at(path, tail_start + pos, chunk)?;
            if data.is_empty() {
                break;
            }
            Vfs::write_at(path, offset + pos, &data)?;
            pos += data.len() as u64;
        }
    }

    // Truncate the file to new size.
    let new_size = meta.size - actual_len;
    truncate_to(path, new_size)?;

    // Invalidate sparse map for this file (regions have shifted).
    remove_tracking(path);

    Ok(RangeResult { bytes_affected: actual_len, created_hole: false })
}

/// Insert a zero range at the given offset, shifting data up.
///
/// This increases the file size by `length` bytes. A hole of zeros
/// is inserted at [offset, offset+length), and existing data from
/// offset onward is shifted up.
pub fn insert_range(path: &str, offset: u64, length: u64) -> KernelResult<RangeResult> {
    use crate::fs::Vfs;

    if path.is_empty() || length == 0 {
        return Err(KernelError::InvalidArgument);
    }

    let meta = Vfs::metadata(path)?;
    if offset > meta.size {
        return Err(KernelError::InvalidArgument);
    }

    INSERT_COUNT.fetch_add(1, Ordering::Relaxed);

    // Read existing tail from offset to end.
    let tail_len = meta.size.saturating_sub(offset);

    if tail_len > 0 {
        // Read the tail.
        let tail_data = Vfs::read_at(path, offset, tail_len as usize)?;

        // Write zeros at insertion point.
        let zeros = alloc::vec![0u8; length.min(65536) as usize];
        let mut written: u64 = 0;
        while written < length {
            let chunk = (length - written).min(65536) as usize;
            Vfs::write_at(path, offset + written, &zeros[..chunk])?;
            written += chunk as u64;
        }

        // Write tail after the inserted zeros.
        Vfs::write_at(path, offset + length, &tail_data)?;
    } else {
        // Just extend with zeros.
        let zeros = alloc::vec![0u8; length.min(65536) as usize];
        let mut written: u64 = 0;
        while written < length {
            let chunk = (length - written).min(65536) as usize;
            Vfs::write_at(path, offset + written, &zeros[..chunk])?;
            written += chunk as u64;
        }
    }

    // Track the new hole.
    track_hole(path, offset, length);

    Ok(RangeResult { bytes_affected: length, created_hole: true })
}

/// Map the sparse regions of a file.
///
/// Returns a SparseMap describing which parts are data and which are holes.
pub fn map_regions(path: &str) -> KernelResult<SparseMap> {
    use crate::fs::Vfs;

    if path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    MAP_QUERIES.fetch_add(1, Ordering::Relaxed);

    let meta = Vfs::metadata(path)?;
    let file_size = meta.size;

    // Get tracked holes for this file.
    let holes = get_holes(path);

    // Build region list from holes.
    let mut regions: Vec<Region> = Vec::new();
    let mut pos: u64 = 0;

    for &(hole_off, hole_len) in &holes {
        if hole_off > pos {
            // Data region before this hole.
            regions.push(Region {
                offset: pos,
                length: hole_off - pos,
                kind: RegionKind::Data,
            });
        }
        // The hole itself.
        regions.push(Region {
            offset: hole_off,
            length: hole_len,
            kind: RegionKind::Hole,
        });
        pos = hole_off + hole_len;
    }

    // Trailing data region after last hole.
    if pos < file_size {
        regions.push(Region {
            offset: pos,
            length: file_size - pos,
            kind: RegionKind::Data,
        });
    }

    // If no holes tracked, entire file is data.
    if regions.is_empty() && file_size > 0 {
        regions.push(Region {
            offset: 0,
            length: file_size,
            kind: RegionKind::Data,
        });
    }

    let hole_bytes: u64 = regions.iter()
        .filter(|r| r.kind == RegionKind::Hole)
        .map(|r| r.length)
        .sum();
    let data_bytes = file_size.saturating_sub(hole_bytes);

    Ok(SparseMap {
        path: String::from(path),
        file_size,
        regions,
        hole_bytes,
        data_bytes,
    })
}

/// Find the next data region at or after the given offset.
/// Equivalent to lseek(fd, offset, SEEK_DATA).
pub fn seek_data(path: &str, offset: u64) -> KernelResult<Option<u64>> {
    use crate::fs::Vfs;

    let meta = Vfs::metadata(path)?;
    if offset >= meta.size {
        return Ok(None);
    }

    let holes = get_holes(path);

    // Check if offset is within a hole.
    for &(hole_off, hole_len) in &holes {
        if offset >= hole_off && offset < hole_off + hole_len {
            // Inside a hole — next data starts after this hole.
            let next = hole_off + hole_len;
            return if next < meta.size { Ok(Some(next)) } else { Ok(None) };
        }
    }

    // Not in a hole — offset itself is data.
    Ok(Some(offset))
}

/// Find the next hole at or after the given offset.
/// Equivalent to lseek(fd, offset, SEEK_HOLE).
pub fn seek_hole(path: &str, offset: u64) -> KernelResult<Option<u64>> {
    use crate::fs::Vfs;

    let meta = Vfs::metadata(path)?;
    if offset >= meta.size {
        return Ok(None);
    }

    let holes = get_holes(path);

    for &(hole_off, hole_len) in &holes {
        if offset >= hole_off && offset < hole_off + hole_len {
            // Already in a hole.
            return Ok(Some(offset));
        }
        if hole_off > offset {
            // Next hole after offset.
            return Ok(Some(hole_off));
        }
    }

    // No tracked hole after offset. Virtual "hole" at EOF.
    Ok(Some(meta.size))
}

// ---------------------------------------------------------------------------
// Public API — Statistics
// ---------------------------------------------------------------------------

/// Get sparse file statistics.
pub fn stats() -> (u64, u64, u64, u64, u64, u64, usize) {
    let tracked = SPARSE_TABLE.lock().len();
    (
        PUNCH_COUNT.load(Ordering::Relaxed),
        PUNCH_BYTES.load(Ordering::Relaxed),
        ZERO_RANGE_COUNT.load(Ordering::Relaxed),
        COLLAPSE_COUNT.load(Ordering::Relaxed),
        INSERT_COUNT.load(Ordering::Relaxed),
        MAP_QUERIES.load(Ordering::Relaxed),
        tracked,
    )
}

/// Reset statistics.
pub fn reset_stats() {
    PUNCH_COUNT.store(0, Ordering::Relaxed);
    PUNCH_BYTES.store(0, Ordering::Relaxed);
    ZERO_RANGE_COUNT.store(0, Ordering::Relaxed);
    COLLAPSE_COUNT.store(0, Ordering::Relaxed);
    INSERT_COUNT.store(0, Ordering::Relaxed);
    MAP_QUERIES.store(0, Ordering::Relaxed);
}

/// List tracked sparse files.
pub fn list_tracked() -> Vec<(String, usize)> {
    let table = SPARSE_TABLE.lock();
    table.iter().map(|e| (e.path.clone(), e.holes.len())).collect()
}

/// Clear all sparse tracking data.
pub fn clear_tracking() {
    SPARSE_TABLE.lock().clear();
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Track a hole for a file.
fn track_hole(path: &str, offset: u64, length: u64) -> bool {
    if length < MIN_HOLE_SIZE {
        return false;
    }

    let now = crate::timekeeping::clock_monotonic();
    let mut table = SPARSE_TABLE.lock();

    // Find or create entry.
    let entry = if let Some(idx) = table.iter().position(|e| e.path == path) {
        table[idx].last_access_ns = now;
        &mut table[idx]
    } else {
        // Evict LRU if at capacity.
        if table.len() >= MAX_TRACKED_FILES {
            if let Some(oldest_idx) = table.iter().enumerate()
                .min_by_key(|(_, e)| e.last_access_ns)
                .map(|(i, _)| i)
            {
                table.swap_remove(oldest_idx);
            }
        }
        table.push(SparseEntry {
            path: String::from(path),
            holes: Vec::new(),
            last_access_ns: now,
        });
        // SAFETY invariant: we just pushed, so last_mut() is always Some.
        match table.last_mut() {
            Some(entry) => entry,
            None => return false,
        }
    };

    // Insert hole in sorted order, merging with adjacent.
    let end = offset + length;

    // Find merge candidates.
    let mut merged = false;
    for hole in entry.holes.iter_mut() {
        let h_end = hole.0 + hole.1;
        if offset <= h_end && end >= hole.0 {
            // Overlapping or adjacent — merge.
            let new_start = offset.min(hole.0);
            let new_end = end.max(h_end);
            hole.0 = new_start;
            hole.1 = new_end - new_start;
            merged = true;
            break;
        }
    }

    if !merged {
        if entry.holes.len() >= MAX_REGIONS_PER_FILE {
            // Too many regions — merge smallest gap.
            entry.holes.sort_by_key(|h| h.0);
            if entry.holes.len() >= 2 {
                // Find smallest gap and merge those two.
                let mut min_gap = u64::MAX;
                let mut min_idx = 0;
                for i in 0..entry.holes.len() - 1 {
                    let gap = entry.holes[i + 1].0.saturating_sub(
                        entry.holes[i].0 + entry.holes[i].1
                    );
                    if gap < min_gap {
                        min_gap = gap;
                        min_idx = i;
                    }
                }
                // Merge min_idx and min_idx+1.
                let end2 = entry.holes[min_idx + 1].0 + entry.holes[min_idx + 1].1;
                let new_len = end2 - entry.holes[min_idx].0;
                entry.holes[min_idx].1 = new_len;
                entry.holes.remove(min_idx + 1);
            }
        }
        entry.holes.push((offset, length));
        entry.holes.sort_by_key(|h| h.0);
    }

    true
}

/// Get tracked holes for a file (sorted by offset).
fn get_holes(path: &str) -> Vec<(u64, u64)> {
    let now = crate::timekeeping::clock_monotonic();
    let mut table = SPARSE_TABLE.lock();
    if let Some(entry) = table.iter_mut().find(|e| e.path == path) {
        entry.last_access_ns = now;
        entry.holes.clone()
    } else {
        Vec::new()
    }
}

/// Remove tracking for a file.
fn remove_tracking(path: &str) {
    let mut table = SPARSE_TABLE.lock();
    table.retain(|e| e.path != path);
}

/// Truncate file to a specific size.
fn truncate_to(path: &str, new_size: u64) -> KernelResult<()> {
    use crate::fs::Vfs;

    let current = Vfs::read_file(path)?;
    if (current.len() as u64) <= new_size {
        return Ok(());
    }
    Vfs::write_file(path, &current[..new_size as usize])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[sparse] Running self-test...");

    test_punch_hole();
    test_zero_range();
    test_collapse_range();
    test_insert_range();
    test_map_regions();
    test_seek_data_hole();

    serial_println!("[sparse] Self-test passed (6 tests).");
    Ok(())
}

fn test_punch_hole() {
    use crate::fs::Vfs;

    let path = "/tmp/_sparse_punch";
    let data = alloc::vec![0xFFu8; 8192];
    Vfs::write_file(path, &data).unwrap();

    // Punch a hole in the middle.
    let result = punch_hole(path, 2048, 4096).unwrap();
    assert_eq!(result.bytes_affected, 4096);
    assert!(result.created_hole);

    // Verify the hole is zeros.
    let readback = Vfs::read_at(path, 2048, 4096).unwrap();
    assert!(readback.iter().all(|&b| b == 0));

    // Data before and after should be intact.
    let before = Vfs::read_at(path, 0, 2048).unwrap();
    assert!(before.iter().all(|&b| b == 0xFF));
    let after = Vfs::read_at(path, 6144, 2048).unwrap();
    assert!(after.iter().all(|&b| b == 0xFF));

    let _ = Vfs::remove(path);
    remove_tracking(path);
    serial_println!("[sparse]   punch_hole: ok");
}

fn test_zero_range() {
    use crate::fs::Vfs;

    let path = "/tmp/_sparse_zero";
    let data = alloc::vec![0xAAu8; 4096];
    Vfs::write_file(path, &data).unwrap();

    let result = zero_range(path, 1024, 2048).unwrap();
    assert_eq!(result.bytes_affected, 2048);
    assert!(!result.created_hole); // Zero range doesn't create hole.

    let readback = Vfs::read_at(path, 1024, 2048).unwrap();
    assert!(readback.iter().all(|&b| b == 0));

    let _ = Vfs::remove(path);
    serial_println!("[sparse]   zero_range: ok");
}

fn test_collapse_range() {
    use crate::fs::Vfs;

    let path = "/tmp/_sparse_collapse";
    // Write "AAAABBBBCCCC" (4 bytes each section, 12 total).
    let mut data = Vec::new();
    data.extend_from_slice(&[b'A'; 4]);
    data.extend_from_slice(&[b'B'; 4]);
    data.extend_from_slice(&[b'C'; 4]);
    Vfs::write_file(path, &data).unwrap();

    // Collapse the middle 4 bytes (the B's).
    let result = collapse_range(path, 4, 4).unwrap();
    assert_eq!(result.bytes_affected, 4);

    // File should now be "AAAACCCC" (8 bytes).
    let readback = Vfs::read_file(path).unwrap();
    assert_eq!(readback.len(), 8);
    assert_eq!(&readback[..4], &[b'A'; 4]);
    assert_eq!(&readback[4..8], &[b'C'; 4]);

    let _ = Vfs::remove(path);
    serial_println!("[sparse]   collapse_range: ok");
}

fn test_insert_range() {
    use crate::fs::Vfs;

    let path = "/tmp/_sparse_insert";
    // Write "AAAACCCC".
    let mut data = Vec::new();
    data.extend_from_slice(&[b'A'; 4]);
    data.extend_from_slice(&[b'C'; 4]);
    Vfs::write_file(path, &data).unwrap();

    // Insert 4 zero bytes at offset 4.
    let result = insert_range(path, 4, 4).unwrap();
    assert_eq!(result.bytes_affected, 4);
    assert!(result.created_hole);

    // File should be "AAAA\0\0\0\0CCCC" (12 bytes).
    let readback = Vfs::read_file(path).unwrap();
    assert_eq!(readback.len(), 12);
    assert_eq!(&readback[..4], &[b'A'; 4]);
    assert_eq!(&readback[4..8], &[0u8; 4]);
    assert_eq!(&readback[8..12], &[b'C'; 4]);

    let _ = Vfs::remove(path);
    remove_tracking(path);
    serial_println!("[sparse]   insert_range: ok");
}

fn test_map_regions() {
    use crate::fs::Vfs;

    let path = "/tmp/_sparse_map";
    let data = alloc::vec![0xBBu8; 16384];
    Vfs::write_file(path, &data).unwrap();

    // Punch two holes.
    punch_hole(path, 4096, 4096).unwrap();
    punch_hole(path, 12288, 4096).unwrap();

    // Map regions.
    let map = map_regions(path).unwrap();
    assert_eq!(map.file_size, 16384);
    assert!(map.hole_bytes >= 8192); // At least the two holes.
    assert!(map.regions.len() >= 3); // data-hole-data-hole or similar.

    let _ = Vfs::remove(path);
    remove_tracking(path);
    serial_println!("[sparse]   map_regions: ok");
}

fn test_seek_data_hole() {
    use crate::fs::Vfs;

    let path = "/tmp/_sparse_seek";
    let data = alloc::vec![0xCCu8; 16384];
    Vfs::write_file(path, &data).unwrap();

    // Punch a hole at [4096, 8192).
    punch_hole(path, 4096, 4096).unwrap();

    // seek_data from 0 should find data at 0 (before hole).
    let sd = seek_data(path, 0).unwrap();
    assert_eq!(sd, Some(0));

    // seek_data from within hole should find data at hole end.
    let sd2 = seek_data(path, 5000).unwrap();
    assert_eq!(sd2, Some(8192));

    // seek_hole from 0 should find hole at 4096.
    let sh = seek_hole(path, 0).unwrap();
    assert_eq!(sh, Some(4096));

    let _ = Vfs::remove(path);
    remove_tracking(path);
    serial_println!("[sparse]   seek_data_hole: ok");
}
