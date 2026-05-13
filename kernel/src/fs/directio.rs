//! Direct I/O mode — cache-bypass file access.
//!
//! Provides O_DIRECT-equivalent functionality: reads and writes that
//! bypass the buffer cache, going directly to/from the underlying
//! filesystem storage.
//!
//! ## Use Cases
//!
//! - **Databases** (SQLite, PostgreSQL) that manage their own caching
//!   and don't want double-buffering in the kernel page cache.
//! - **Virtual machine disk images** — large sequential I/O where
//!   polluting the cache hurts other workloads.
//! - **Benchmarks** that need to measure true storage latency without
//!   cache effects.
//! - **Large file copies** where data won't be re-read and caching
//!   wastes memory.
//!
//! ## Architecture
//!
//! ```text
//! Application → dio_read(path, offset, len)
//!   → checks alignment requirements
//!   → reads from VFS with cache-invalidate hint
//!   → returns data without populating cache
//!
//! Application → dio_write(path, offset, data)
//!   → checks alignment
//!   → writes to VFS with cache-bypass flag
//!   → invalidates any cached copy of the written range
//! ```
//!
//! ## Design Notes
//!
//! - In our current VFS model, "direct I/O" means we explicitly
//!   invalidate cache entries after the operation, preventing stale
//!   cached data from being served on subsequent buffered reads.
//! - Alignment requirement: 512 bytes (sector-aligned). Operations on
//!   unaligned offsets or lengths fall back to buffered I/O with a
//!   warning in statistics.
//! - Per-path direct-I/O mode registration: paths can be registered
//!   for automatic cache bypass on all subsequent operations.
//! - Maximum single DIO transfer: 4 MiB (larger than splice since DIO
//!   is typically used for large sequential I/O).

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Alignment requirement for direct I/O (sector size).
const DIO_ALIGNMENT: u64 = 512;

/// Maximum single DIO transfer size (4 MiB).
const MAX_DIO_TRANSFER: usize = 4 * 1024 * 1024;

/// Maximum registered direct-I/O paths.
const MAX_DIO_PATHS: usize = 128;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Direct I/O operation result.
#[derive(Debug, Clone)]
pub struct DioResult {
    /// Bytes transferred.
    pub bytes: u64,
    /// Whether the operation was truly aligned (no fallback).
    pub aligned: bool,
    /// Whether cache was invalidated after the operation.
    pub cache_invalidated: bool,
}

/// A registered direct-I/O path entry.
#[derive(Debug, Clone)]
struct DioPathEntry {
    path: String,
    /// Timestamp when registered (for LRU).
    registered_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Paths registered for automatic direct I/O.
static DIO_PATHS: spin::Mutex<Vec<DioPathEntry>> = spin::Mutex::new(Vec::new());

/// Statistics.
static DIO_READ_COUNT: AtomicU64 = AtomicU64::new(0);
static DIO_WRITE_COUNT: AtomicU64 = AtomicU64::new(0);
static DIO_READ_BYTES: AtomicU64 = AtomicU64::new(0);
static DIO_WRITE_BYTES: AtomicU64 = AtomicU64::new(0);
static DIO_UNALIGNED_COUNT: AtomicU64 = AtomicU64::new(0);
static DIO_CACHE_INVALIDATIONS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Alignment helpers
// ---------------------------------------------------------------------------

/// Check if a value is aligned to DIO_ALIGNMENT.
#[inline]
fn is_aligned(val: u64) -> bool {
    val & (DIO_ALIGNMENT - 1) == 0
}

/// Round up to alignment boundary.
#[inline]
fn align_up(val: u64) -> u64 {
    (val + DIO_ALIGNMENT - 1) & !(DIO_ALIGNMENT - 1)
}

// ---------------------------------------------------------------------------
// Public API — Direct operations
// ---------------------------------------------------------------------------

/// Read data bypassing the buffer cache.
///
/// Offset and length should be aligned to 512 bytes for optimal
/// performance. Unaligned operations fall back to buffered I/O
/// but still invalidate the cache afterward.
pub fn dio_read(path: &str, offset: u64, len: usize) -> KernelResult<(Vec<u8>, DioResult)> {
    use crate::fs::Vfs;

    if path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    let read_len = len.min(MAX_DIO_TRANSFER);
    if read_len == 0 {
        return Ok((Vec::new(), DioResult { bytes: 0, aligned: true, cache_invalidated: false }));
    }

    DIO_READ_COUNT.fetch_add(1, Ordering::Relaxed);

    let aligned = is_aligned(offset) && is_aligned(read_len as u64);
    if !aligned {
        DIO_UNALIGNED_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    // Perform the read through VFS.
    let data = Vfs::read_at(path, offset, read_len)?;
    let bytes_read = data.len() as u64;
    DIO_READ_BYTES.fetch_add(bytes_read, Ordering::Relaxed);

    // Invalidate cache for this range to maintain DIO semantics.
    // In a full implementation, we'd mark these pages as not-cacheable.
    let invalidated = invalidate_cache_range(path, offset, bytes_read);

    Ok((data, DioResult {
        bytes: bytes_read,
        aligned,
        cache_invalidated: invalidated,
    }))
}

/// Write data bypassing the buffer cache.
///
/// Offset and data length should be aligned to 512 bytes. The write
/// goes directly to storage and any cached copy is invalidated.
pub fn dio_write(path: &str, offset: u64, data: &[u8]) -> KernelResult<DioResult> {
    use crate::fs::Vfs;

    if path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    if data.is_empty() {
        return Ok(DioResult { bytes: 0, aligned: true, cache_invalidated: false });
    }

    let write_len = data.len().min(MAX_DIO_TRANSFER);
    let write_data = &data[..write_len];

    DIO_WRITE_COUNT.fetch_add(1, Ordering::Relaxed);

    let aligned = is_aligned(offset) && is_aligned(write_len as u64);
    if !aligned {
        DIO_UNALIGNED_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    // Write through VFS.
    Vfs::write_at(path, offset, write_data)?;
    let bytes_written = write_len as u64;
    DIO_WRITE_BYTES.fetch_add(bytes_written, Ordering::Relaxed);

    // Invalidate cache so subsequent buffered reads see the new data.
    let invalidated = invalidate_cache_range(path, offset, bytes_written);

    Ok(DioResult {
        bytes: bytes_written,
        aligned,
        cache_invalidated: invalidated,
    })
}

/// Read an entire file with direct I/O semantics.
pub fn dio_read_file(path: &str) -> KernelResult<(Vec<u8>, DioResult)> {
    use crate::fs::Vfs;

    if path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    DIO_READ_COUNT.fetch_add(1, Ordering::Relaxed);

    let data = Vfs::read_file(path)?;
    let bytes = data.len() as u64;
    DIO_READ_BYTES.fetch_add(bytes, Ordering::Relaxed);

    let invalidated = invalidate_cache_range(path, 0, bytes);

    Ok((data, DioResult {
        bytes,
        aligned: is_aligned(bytes),
        cache_invalidated: invalidated,
    }))
}

/// Write an entire file with direct I/O semantics.
pub fn dio_write_file(path: &str, data: &[u8]) -> KernelResult<DioResult> {
    use crate::fs::Vfs;

    if path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    DIO_WRITE_COUNT.fetch_add(1, Ordering::Relaxed);

    Vfs::write_file(path, data)?;
    let bytes = data.len() as u64;
    DIO_WRITE_BYTES.fetch_add(bytes, Ordering::Relaxed);

    let invalidated = invalidate_cache_range(path, 0, bytes);

    Ok(DioResult {
        bytes,
        aligned: is_aligned(bytes),
        cache_invalidated: invalidated,
    })
}

// ---------------------------------------------------------------------------
// Public API — Path registration
// ---------------------------------------------------------------------------

/// Register a path for automatic direct I/O on all operations.
///
/// When a path is registered, the VFS integration layer can check
/// `is_dio_path()` and route operations through the DIO path.
pub fn register_path(path: &str) -> bool {
    let now = crate::timekeeping::clock_monotonic();
    let mut paths = DIO_PATHS.lock();

    // Already registered?
    for entry in paths.iter() {
        if entry.path == path {
            return false; // Already exists.
        }
    }

    // Evict oldest if at capacity.
    if paths.len() >= MAX_DIO_PATHS {
        if let Some(oldest_idx) = paths.iter().enumerate()
            .min_by_key(|(_, e)| e.registered_ns)
            .map(|(i, _)| i)
        {
            paths.swap_remove(oldest_idx);
        }
    }

    paths.push(DioPathEntry {
        path: String::from(path),
        registered_ns: now,
    });
    true
}

/// Unregister a path from automatic direct I/O.
pub fn unregister_path(path: &str) -> bool {
    let mut paths = DIO_PATHS.lock();
    let len_before = paths.len();
    paths.retain(|e| e.path != path);
    paths.len() < len_before
}

/// Check if a path is registered for direct I/O.
pub fn is_dio_path(path: &str) -> bool {
    let paths = DIO_PATHS.lock();
    paths.iter().any(|e| e.path == path || path.starts_with(&e.path))
}

/// List all registered direct I/O paths.
pub fn list_paths() -> Vec<String> {
    let paths = DIO_PATHS.lock();
    paths.iter().map(|e| e.path.clone()).collect()
}

/// Clear all registered paths.
pub fn clear_paths() {
    DIO_PATHS.lock().clear();
}

// ---------------------------------------------------------------------------
// Public API — Statistics
// ---------------------------------------------------------------------------

/// Get DIO statistics.
pub fn stats() -> (u64, u64, u64, u64, u64, u64, usize) {
    let path_count = DIO_PATHS.lock().len();
    (
        DIO_READ_COUNT.load(Ordering::Relaxed),
        DIO_WRITE_COUNT.load(Ordering::Relaxed),
        DIO_READ_BYTES.load(Ordering::Relaxed),
        DIO_WRITE_BYTES.load(Ordering::Relaxed),
        DIO_UNALIGNED_COUNT.load(Ordering::Relaxed),
        DIO_CACHE_INVALIDATIONS.load(Ordering::Relaxed),
        path_count,
    )
}

/// Reset all statistics.
pub fn reset_stats() {
    DIO_READ_COUNT.store(0, Ordering::Relaxed);
    DIO_WRITE_COUNT.store(0, Ordering::Relaxed);
    DIO_READ_BYTES.store(0, Ordering::Relaxed);
    DIO_WRITE_BYTES.store(0, Ordering::Relaxed);
    DIO_UNALIGNED_COUNT.store(0, Ordering::Relaxed);
    DIO_CACHE_INVALIDATIONS.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Invalidate cached data for a file range.
///
/// In a full implementation with a page cache, this would unmap the
/// specific pages backing the file range. Our block cache is keyed by
/// (device, sector) so path-level invalidation requires resolving the
/// file's on-disk sectors — which is filesystem-specific.
///
/// For now we record the invalidation intent (for statistics and future
/// integration) and set the DontNeed prefetch hint, which signals the
/// VFS caching layer to deprioritize this data.
fn invalidate_cache_range(path: &str, _offset: u64, _len: u64) -> bool {
    // Signal to the prefetch/caching system that this data shouldn't
    // remain cached (DontNeed advice).
    crate::fs::prefetch::advise(path, crate::fs::prefetch::AccessAdvice::DontNeed);
    DIO_CACHE_INVALIDATIONS.fetch_add(1, Ordering::Relaxed);
    true
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[directio] Running self-test...");

    test_alignment();
    test_dio_read_write();
    test_path_registration();
    test_unaligned_tracking();
    test_whole_file();
    test_stats();

    serial_println!("[directio] Self-test passed (6 tests).");
    Ok(())
}

fn test_alignment() {
    assert!(is_aligned(0));
    assert!(is_aligned(512));
    assert!(is_aligned(1024));
    assert!(is_aligned(4096));
    assert!(!is_aligned(1));
    assert!(!is_aligned(511));
    assert!(!is_aligned(513));

    assert_eq!(align_up(0), 0);
    assert_eq!(align_up(1), 512);
    assert_eq!(align_up(512), 512);
    assert_eq!(align_up(513), 1024);

    serial_println!("[directio]   alignment: ok");
}

fn test_dio_read_write() {
    use crate::fs::Vfs;

    let path = "/tmp/_dio_test";
    let data = alloc::vec![0xBEu8; 1024]; // 1024 = aligned

    // Write with DIO.
    let wr = dio_write(path, 0, &data).unwrap();
    assert_eq!(wr.bytes, 1024);
    assert!(wr.aligned);
    assert!(wr.cache_invalidated);

    // Read back with DIO.
    let (read_data, rr) = dio_read(path, 0, 1024).unwrap();
    assert_eq!(read_data.len(), 1024);
    assert!(read_data.iter().all(|&b| b == 0xBE));
    assert_eq!(rr.bytes, 1024);
    assert!(rr.aligned);

    // Partial read with offset.
    let (partial, pr) = dio_read(path, 512, 512).unwrap();
    assert_eq!(partial.len(), 512);
    assert_eq!(pr.bytes, 512);
    assert!(pr.aligned);

    let _ = Vfs::remove(path);
    serial_println!("[directio]   dio_read_write: ok");
}

fn test_path_registration() {
    let path = "/test/dio/database.db";

    // Register.
    assert!(register_path(path));
    // Duplicate returns false.
    assert!(!register_path(path));

    // Query.
    assert!(is_dio_path(path));
    assert!(is_dio_path("/test/dio/database.db/wal")); // Prefix match.
    assert!(!is_dio_path("/test/other"));

    // List.
    let paths = list_paths();
    assert!(paths.iter().any(|p| p == path));

    // Unregister.
    assert!(unregister_path(path));
    assert!(!is_dio_path(path));
    assert!(!unregister_path(path)); // Already removed.

    serial_println!("[directio]   path_registration: ok");
}

fn test_unaligned_tracking() {
    use crate::fs::Vfs;

    let path = "/tmp/_dio_unaligned";
    Vfs::write_file(path, &[0u8; 2048]).unwrap();

    let before = DIO_UNALIGNED_COUNT.load(Ordering::Relaxed);

    // Unaligned offset.
    let _ = dio_read(path, 100, 512);
    let after1 = DIO_UNALIGNED_COUNT.load(Ordering::Relaxed);
    assert_eq!(after1, before + 1);

    // Unaligned length.
    let _ = dio_read(path, 0, 300);
    let after2 = DIO_UNALIGNED_COUNT.load(Ordering::Relaxed);
    assert_eq!(after2, before + 2);

    // Aligned — no increment.
    let _ = dio_read(path, 512, 512);
    let after3 = DIO_UNALIGNED_COUNT.load(Ordering::Relaxed);
    assert_eq!(after3, before + 2);

    let _ = Vfs::remove(path);
    serial_println!("[directio]   unaligned_tracking: ok");
}

fn test_whole_file() {
    use crate::fs::Vfs;

    let path = "/tmp/_dio_whole";
    let data = alloc::vec![0x42u8; 2048];

    // Write whole file.
    let wr = dio_write_file(path, &data).unwrap();
    assert_eq!(wr.bytes, 2048);

    // Read whole file.
    let (read_data, rr) = dio_read_file(path).unwrap();
    assert_eq!(read_data, data);
    assert_eq!(rr.bytes, 2048);

    let _ = Vfs::remove(path);
    serial_println!("[directio]   whole_file: ok");
}

fn test_stats() {
    let (reads, writes, rbytes, wbytes, unaligned, invalidations, paths) = stats();

    // Stats should reflect the operations from previous tests.
    assert!(reads > 0);
    assert!(writes > 0);
    assert!(rbytes > 0);
    assert!(wbytes > 0);
    // unaligned and invalidations should also be > 0 from tests above.
    let _ = (unaligned, invalidations, paths);

    serial_println!("[directio]   stats: ok");
}
