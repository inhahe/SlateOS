//! Zero-copy data transfer between files and pipes.
//!
//! Provides efficient data movement primitives that avoid copying data
//! through userspace buffers:
//!
//! | Operation        | Description                                       |
//! |------------------|---------------------------------------------------|
//! | `copy_file_range`| Copy between two files (server-side copy)         |
//! | `sendfile`       | Copy from file to pipe/destination (web serving)  |
//! | `splice`         | Move data between a pipe and a file               |
//! | `tee`            | Duplicate pipe data without consuming it          |
//!
//! ## Architecture
//!
//! ```text
//! Application → splice(src_file, dst_pipe, len)
//!   → reads from VFS into internal buffer
//!   → writes to destination without extra copy to user
//!   → returns bytes transferred
//!
//! Application → copy_file_range(src, src_off, dst, dst_off, len)
//!   → VFS reads chunk from src at src_off
//!   → VFS writes chunk to dst at dst_off
//!   → no user-visible intermediate buffer
//! ```
//!
//! ## Design Notes
//!
//! - In a full kernel with VM subsystem, splice would move page references
//!   between pipe buffers and page cache — true zero-copy. In our VFS
//!   model we still avoid the extra user↔kernel boundary crossing by
//!   keeping the transfer entirely within the kernel.
//! - Maximum single transfer: 1 MiB (chunked internally for memory
//!   pressure management).
//! - `copy_file_range` on the same filesystem can potentially be optimized
//!   to a metadata-only operation (reflink/CoW). Currently does data copy.
//! - Statistics track total bytes transferred and operation counts for
//!   performance monitoring.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum bytes per single splice/copy operation (1 MiB).
const MAX_TRANSFER: usize = 1024 * 1024;

/// Internal chunk size for large transfers (64 KiB).
/// Keeps peak memory usage bounded during multi-MiB copies.
const CHUNK_SIZE: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

static SPLICE_COUNT: AtomicU64 = AtomicU64::new(0);
static SPLICE_BYTES: AtomicU64 = AtomicU64::new(0);
static SENDFILE_COUNT: AtomicU64 = AtomicU64::new(0);
static SENDFILE_BYTES: AtomicU64 = AtomicU64::new(0);
static COPY_RANGE_COUNT: AtomicU64 = AtomicU64::new(0);
static COPY_RANGE_BYTES: AtomicU64 = AtomicU64::new(0);
static TEE_COUNT: AtomicU64 = AtomicU64::new(0);
static TEE_BYTES: AtomicU64 = AtomicU64::new(0);
static ERROR_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Result of a splice/transfer operation.
#[derive(Debug, Clone)]
pub struct TransferResult {
    /// Bytes successfully transferred.
    pub bytes_transferred: u64,
    /// Number of chunks used internally.
    pub chunks: u32,
}

/// Aggregate statistics for all splice operations.
#[derive(Debug, Clone)]
pub struct SpliceStats {
    pub splice_ops: u64,
    pub splice_bytes: u64,
    pub sendfile_ops: u64,
    pub sendfile_bytes: u64,
    pub copy_range_ops: u64,
    pub copy_range_bytes: u64,
    pub tee_ops: u64,
    pub tee_bytes: u64,
    pub errors: u64,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Copy data between two files without intermediate user-space buffer.
///
/// Reads from `src_path` at `src_offset` and writes to `dst_path` at
/// `dst_offset`. Transfers up to `len` bytes (capped at MAX_TRANSFER).
///
/// Returns the number of bytes actually transferred (may be less than
/// `len` if src is shorter than expected).
pub fn copy_file_range(
    src_path: &str,
    src_offset: u64,
    dst_path: &str,
    dst_offset: u64,
    len: usize,
) -> KernelResult<TransferResult> {
    use crate::fs::Vfs;

    if src_path.is_empty() || dst_path.is_empty() {
        ERROR_COUNT.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::InvalidArgument);
    }

    let transfer_len = len.min(MAX_TRANSFER);
    if transfer_len == 0 {
        return Ok(TransferResult { bytes_transferred: 0, chunks: 0 });
    }

    COPY_RANGE_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut total_transferred: u64 = 0;
    let mut chunks: u32 = 0;
    let mut s_off = src_offset;
    let mut d_off = dst_offset;
    let mut remaining = transfer_len;

    while remaining > 0 {
        let chunk_len = remaining.min(CHUNK_SIZE);
        let data = Vfs::read_at(src_path, s_off, chunk_len)?;
        if data.is_empty() {
            break; // EOF
        }

        let actual_len = data.len();
        Vfs::write_at(dst_path, d_off, &data)?;

        total_transferred += actual_len as u64;
        s_off += actual_len as u64;
        d_off += actual_len as u64;
        remaining -= actual_len;
        chunks += 1;

        if actual_len < chunk_len {
            break; // Short read means EOF.
        }
    }

    COPY_RANGE_BYTES.fetch_add(total_transferred, Ordering::Relaxed);
    Ok(TransferResult { bytes_transferred: total_transferred, chunks })
}

/// Send file data to a destination path (sendfile equivalent).
///
/// Reads from `src_path` starting at `offset` and writes to `dst_path`
/// (appending). Useful for serving file content to pipes or network
/// destinations without user-space buffering.
pub fn sendfile(
    src_path: &str,
    dst_path: &str,
    offset: u64,
    len: usize,
) -> KernelResult<TransferResult> {
    use crate::fs::Vfs;

    if src_path.is_empty() || dst_path.is_empty() {
        ERROR_COUNT.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::InvalidArgument);
    }

    let transfer_len = len.min(MAX_TRANSFER);
    if transfer_len == 0 {
        return Ok(TransferResult { bytes_transferred: 0, chunks: 0 });
    }

    SENDFILE_COUNT.fetch_add(1, Ordering::Relaxed);

    // Determine current dst size for append position.
    let dst_meta = Vfs::metadata(dst_path);
    let mut dst_offset = match dst_meta {
        Ok(m) => m.size,
        Err(_) => {
            // File doesn't exist yet — create it.
            Vfs::write_file(dst_path, &[])?;
            0
        }
    };

    let mut total_transferred: u64 = 0;
    let mut chunks: u32 = 0;
    let mut s_off = offset;
    let mut remaining = transfer_len;

    while remaining > 0 {
        let chunk_len = remaining.min(CHUNK_SIZE);
        let data = Vfs::read_at(src_path, s_off, chunk_len)?;
        if data.is_empty() {
            break;
        }

        let actual_len = data.len();
        Vfs::write_at(dst_path, dst_offset, &data)?;

        total_transferred += actual_len as u64;
        s_off += actual_len as u64;
        dst_offset += actual_len as u64;
        remaining -= actual_len;
        chunks += 1;

        if actual_len < chunk_len {
            break;
        }
    }

    SENDFILE_BYTES.fetch_add(total_transferred, Ordering::Relaxed);
    Ok(TransferResult { bytes_transferred: total_transferred, chunks })
}

/// Splice data from a file into a pipe (or vice versa).
///
/// Reads from `src_path` at `src_offset` and writes to `dst_path` at
/// offset 0 (pipe-like append semantics). In a full implementation this
/// would move page references; here we transfer through an internal
/// kernel buffer.
pub fn splice(
    src_path: &str,
    src_offset: u64,
    dst_path: &str,
    len: usize,
) -> KernelResult<TransferResult> {
    use crate::fs::Vfs;

    if src_path.is_empty() || dst_path.is_empty() {
        ERROR_COUNT.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::InvalidArgument);
    }

    let transfer_len = len.min(MAX_TRANSFER);
    if transfer_len == 0 {
        return Ok(TransferResult { bytes_transferred: 0, chunks: 0 });
    }

    SPLICE_COUNT.fetch_add(1, Ordering::Relaxed);

    // For pipe destinations, write at end. For files, write at offset 0.
    let dst_meta = Vfs::metadata(dst_path);
    let mut dst_offset = match dst_meta {
        Ok(m) => m.size,
        Err(_) => {
            Vfs::write_file(dst_path, &[])?;
            0
        }
    };

    let mut total_transferred: u64 = 0;
    let mut chunks: u32 = 0;
    let mut s_off = src_offset;
    let mut remaining = transfer_len;

    while remaining > 0 {
        let chunk_len = remaining.min(CHUNK_SIZE);
        let data = Vfs::read_at(src_path, s_off, chunk_len)?;
        if data.is_empty() {
            break;
        }

        let actual_len = data.len();
        Vfs::write_at(dst_path, dst_offset, &data)?;

        total_transferred += actual_len as u64;
        s_off += actual_len as u64;
        dst_offset += actual_len as u64;
        remaining -= actual_len;
        chunks += 1;

        if actual_len < chunk_len {
            break;
        }
    }

    SPLICE_BYTES.fetch_add(total_transferred, Ordering::Relaxed);
    Ok(TransferResult { bytes_transferred: total_transferred, chunks })
}

/// Duplicate data from one file into another without consuming from source.
///
/// Like `splice` but the source offset is not advanced conceptually —
/// the data remains readable at the same position. Useful for tee-style
/// logging where data flows to both a pipe consumer and a log file.
pub fn tee(
    src_path: &str,
    src_offset: u64,
    dst_path: &str,
    len: usize,
) -> KernelResult<TransferResult> {
    use crate::fs::Vfs;

    if src_path.is_empty() || dst_path.is_empty() {
        ERROR_COUNT.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::InvalidArgument);
    }

    let transfer_len = len.min(MAX_TRANSFER);
    if transfer_len == 0 {
        return Ok(TransferResult { bytes_transferred: 0, chunks: 0 });
    }

    TEE_COUNT.fetch_add(1, Ordering::Relaxed);

    // Read source data.
    let data = Vfs::read_at(src_path, src_offset, transfer_len)?;
    if data.is_empty() {
        return Ok(TransferResult { bytes_transferred: 0, chunks: 0 });
    }

    let bytes = data.len() as u64;

    // Append to destination.
    let dst_meta = Vfs::metadata(dst_path);
    let dst_offset = match dst_meta {
        Ok(m) => m.size,
        Err(_) => {
            Vfs::write_file(dst_path, &[])?;
            0
        }
    };
    Vfs::write_at(dst_path, dst_offset, &data)?;

    TEE_BYTES.fetch_add(bytes, Ordering::Relaxed);
    Ok(TransferResult { bytes_transferred: bytes, chunks: 1 })
}

/// Get aggregate statistics for all splice operations.
pub fn stats() -> SpliceStats {
    SpliceStats {
        splice_ops: SPLICE_COUNT.load(Ordering::Relaxed),
        splice_bytes: SPLICE_BYTES.load(Ordering::Relaxed),
        sendfile_ops: SENDFILE_COUNT.load(Ordering::Relaxed),
        sendfile_bytes: SENDFILE_BYTES.load(Ordering::Relaxed),
        copy_range_ops: COPY_RANGE_COUNT.load(Ordering::Relaxed),
        copy_range_bytes: COPY_RANGE_BYTES.load(Ordering::Relaxed),
        tee_ops: TEE_COUNT.load(Ordering::Relaxed),
        tee_bytes: TEE_BYTES.load(Ordering::Relaxed),
        errors: ERROR_COUNT.load(Ordering::Relaxed),
    }
}

/// Reset all statistics counters.
pub fn reset_stats() {
    SPLICE_COUNT.store(0, Ordering::Relaxed);
    SPLICE_BYTES.store(0, Ordering::Relaxed);
    SENDFILE_COUNT.store(0, Ordering::Relaxed);
    SENDFILE_BYTES.store(0, Ordering::Relaxed);
    COPY_RANGE_COUNT.store(0, Ordering::Relaxed);
    COPY_RANGE_BYTES.store(0, Ordering::Relaxed);
    TEE_COUNT.store(0, Ordering::Relaxed);
    TEE_BYTES.store(0, Ordering::Relaxed);
    ERROR_COUNT.store(0, Ordering::Relaxed);
}

/// Format a byte count for human-readable display.
fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        alloc::format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        alloc::format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        alloc::format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        alloc::format!("{} B", bytes)
    }
}

/// Human-readable summary of all splice statistics.
pub fn summary() -> String {
    let s = stats();
    let total_ops = s.splice_ops + s.sendfile_ops + s.copy_range_ops + s.tee_ops;
    let total_bytes = s.splice_bytes + s.sendfile_bytes + s.copy_range_bytes + s.tee_bytes;

    alloc::format!(
        "splice: {} ops ({}), sendfile: {} ops ({}), copy_range: {} ops ({}), tee: {} ops ({})\n\
         total: {} ops, {} transferred, {} errors",
        s.splice_ops, format_bytes(s.splice_bytes),
        s.sendfile_ops, format_bytes(s.sendfile_bytes),
        s.copy_range_ops, format_bytes(s.copy_range_bytes),
        s.tee_ops, format_bytes(s.tee_bytes),
        total_ops, format_bytes(total_bytes),
        s.errors,
    )
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[splice] Running self-test...");

    test_copy_file_range();
    test_sendfile();
    test_splice_basic();
    test_tee();
    test_empty_transfer();
    test_invalid_args();

    serial_println!("[splice] Self-test passed (6 tests).");
    Ok(())
}

fn test_copy_file_range() {
    use crate::fs::Vfs;

    let src = "/tmp/_splice_src";
    let dst = "/tmp/_splice_dst";
    let data = alloc::vec![0xCDu8; 4096];

    Vfs::write_file(src, &data).unwrap();
    Vfs::write_file(dst, &[]).unwrap();

    // Copy full file.
    let result = copy_file_range(src, 0, dst, 0, 4096).unwrap();
    assert_eq!(result.bytes_transferred, 4096);
    assert!(result.chunks >= 1);

    let readback = Vfs::read_file(dst).unwrap();
    assert_eq!(readback.len(), 4096);
    assert!(readback.iter().all(|&b| b == 0xCD));

    // Copy partial range with offset.
    let dst2 = "/tmp/_splice_dst2";
    Vfs::write_file(dst2, &[0u8; 8192]).unwrap();
    let result2 = copy_file_range(src, 1024, dst2, 2048, 2048).unwrap();
    assert_eq!(result2.bytes_transferred, 2048);

    let readback2 = Vfs::read_at(dst2, 2048, 2048).unwrap();
    assert_eq!(readback2.len(), 2048);
    assert!(readback2.iter().all(|&b| b == 0xCD));

    let _ = Vfs::remove(src);
    let _ = Vfs::remove(dst);
    let _ = Vfs::remove(dst2);
    serial_println!("[splice]   copy_file_range: ok");
}

fn test_sendfile() {
    use crate::fs::Vfs;

    let src = "/tmp/_splice_send_src";
    let dst = "/tmp/_splice_send_dst";
    let data: Vec<u8> = (0..=255u8).cycle().take(2048).collect();

    Vfs::write_file(src, &data).unwrap();

    // sendfile from offset 0.
    let result = sendfile(src, dst, 0, 2048).unwrap();
    assert_eq!(result.bytes_transferred, 2048);

    let readback = Vfs::read_file(dst).unwrap();
    assert_eq!(readback, data);

    // sendfile with offset (partial).
    let dst2 = "/tmp/_splice_send_dst2";
    let result2 = sendfile(src, dst2, 512, 1024).unwrap();
    assert_eq!(result2.bytes_transferred, 1024);

    let readback2 = Vfs::read_file(dst2).unwrap();
    assert_eq!(readback2, &data[512..1536]);

    let _ = Vfs::remove(src);
    let _ = Vfs::remove(dst);
    let _ = Vfs::remove(dst2);
    serial_println!("[splice]   sendfile: ok");
}

fn test_splice_basic() {
    use crate::fs::Vfs;

    let src = "/tmp/_splice_pipe_src";
    let dst = "/tmp/_splice_pipe_dst";
    let data = alloc::vec![0xABu8; 1024];

    Vfs::write_file(src, &data).unwrap();

    let result = splice(src, 0, dst, 1024).unwrap();
    assert_eq!(result.bytes_transferred, 1024);

    let readback = Vfs::read_file(dst).unwrap();
    assert_eq!(readback, data);

    // Splice with source offset.
    let dst2 = "/tmp/_splice_pipe_dst2";
    let result2 = splice(src, 512, dst2, 512).unwrap();
    assert_eq!(result2.bytes_transferred, 512);

    let readback2 = Vfs::read_file(dst2).unwrap();
    assert_eq!(readback2.len(), 512);
    assert!(readback2.iter().all(|&b| b == 0xAB));

    let _ = Vfs::remove(src);
    let _ = Vfs::remove(dst);
    let _ = Vfs::remove(dst2);
    serial_println!("[splice]   splice: ok");
}

fn test_tee() {
    use crate::fs::Vfs;

    let src = "/tmp/_splice_tee_src";
    let dst = "/tmp/_splice_tee_dst";
    let data = alloc::vec![0x55u8; 512];

    Vfs::write_file(src, &data).unwrap();

    // Tee copies without consuming.
    let result = tee(src, 0, dst, 512).unwrap();
    assert_eq!(result.bytes_transferred, 512);

    // Source unchanged.
    let src_data = Vfs::read_file(src).unwrap();
    assert_eq!(src_data, data);

    // Destination has copy.
    let dst_data = Vfs::read_file(dst).unwrap();
    assert_eq!(dst_data, data);

    // Tee again appends.
    let result2 = tee(src, 0, dst, 256).unwrap();
    assert_eq!(result2.bytes_transferred, 256);

    let dst_data2 = Vfs::read_file(dst).unwrap();
    assert_eq!(dst_data2.len(), 768); // 512 + 256

    let _ = Vfs::remove(src);
    let _ = Vfs::remove(dst);
    serial_println!("[splice]   tee: ok");
}

fn test_empty_transfer() {
    // Zero-length transfers succeed immediately.
    let result = copy_file_range("/tmp/_x", 0, "/tmp/_y", 0, 0).unwrap();
    assert_eq!(result.bytes_transferred, 0);
    assert_eq!(result.chunks, 0);

    let result2 = sendfile("/tmp/_x", "/tmp/_y", 0, 0).unwrap();
    assert_eq!(result2.bytes_transferred, 0);

    let result3 = splice("/tmp/_x", 0, "/tmp/_y", 0).unwrap();
    assert_eq!(result3.bytes_transferred, 0);

    let result4 = tee("/tmp/_x", 0, "/tmp/_y", 0).unwrap();
    assert_eq!(result4.bytes_transferred, 0);

    serial_println!("[splice]   empty_transfer: ok");
}

fn test_invalid_args() {
    // Empty paths should return InvalidArgument.
    assert!(copy_file_range("", 0, "/tmp/x", 0, 100).is_err());
    assert!(copy_file_range("/tmp/x", 0, "", 0, 100).is_err());
    assert!(sendfile("", "/tmp/x", 0, 100).is_err());
    assert!(splice("", 0, "/tmp/x", 100).is_err());
    assert!(tee("/tmp/x", 0, "", 100).is_err());

    serial_println!("[splice]   invalid_args: ok");
}
