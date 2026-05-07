//! Filesystem microbenchmark suite.
//!
//! Measures performance of core filesystem operations against baseline
//! targets derived from the design spec:
//!
//! - VFS path lookup: cached ~200-500ns per component
//! - Sequential read/write: within 20% of Linux ext4
//! - Metadata operations: create/stat/delete cycle
//! - Random I/O: small random reads/writes
//!
//! ## Usage
//!
//! ```text
//! fsbench all          Run full benchmark suite
//! fsbench read [path]  Sequential read throughput
//! fsbench write [dir]  Sequential write throughput
//! fsbench meta [dir]   Metadata operation benchmark
//! fsbench lookup [path] Path resolution latency
//! ```
//!
//! Results include operations/second, throughput (MB/s for data ops),
//! and per-operation latency in nanoseconds with comparison to targets.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::KernelResult;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result of a single benchmark run.
#[derive(Debug, Clone)]
pub struct BenchResult {
    /// Benchmark name.
    pub name: String,
    /// Number of operations performed.
    pub iterations: u64,
    /// Total elapsed time (nanoseconds).
    pub total_ns: u64,
    /// Bytes transferred (0 for metadata ops).
    pub bytes: u64,
    /// Target latency per-op in ns (0 = no target).
    pub target_ns: u64,
}

impl BenchResult {
    /// Average nanoseconds per operation.
    pub fn avg_ns(&self) -> u64 {
        if self.iterations == 0 { 0 } else { self.total_ns / self.iterations }
    }

    /// Operations per second.
    pub fn ops_per_sec(&self) -> u64 {
        if self.total_ns == 0 { return 0; }
        self.iterations.saturating_mul(1_000_000_000) / self.total_ns
    }

    /// Throughput in bytes per second.
    pub fn throughput_bps(&self) -> u64 {
        if self.total_ns == 0 { return 0; }
        self.bytes.saturating_mul(1_000_000_000) / self.total_ns
    }

    /// Whether the result meets the target (if one is set).
    pub fn meets_target(&self) -> Option<bool> {
        if self.target_ns == 0 { return None; }
        Some(self.avg_ns() <= self.target_ns)
    }
}

/// Complete benchmark report.
#[derive(Debug, Clone)]
pub struct BenchReport {
    /// Individual benchmark results.
    pub results: Vec<BenchResult>,
    /// Total elapsed time for entire suite (ns).
    pub total_ns: u64,
    /// Number of targets met / total targets.
    pub targets_met: (u64, u64),
}

// ---------------------------------------------------------------------------
// Counters
// ---------------------------------------------------------------------------

static BENCHMARKS_RUN: AtomicU64 = AtomicU64::new(0);
static LAST_SCORE_NS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Target constants (from design spec)
// ---------------------------------------------------------------------------

/// VFS cached path lookup: design target is 200-500ns per component.
const TARGET_PATH_LOOKUP_NS: u64 = 500;

/// Metadata operation (create + stat + delete): target under 10µs total.
const TARGET_META_CYCLE_NS: u64 = 10_000;

/// File open latency target: under 5µs for cached path.
const TARGET_OPEN_NS: u64 = 5_000;

/// Small file read (4 KiB): target under 2µs.
const TARGET_SMALL_READ_NS: u64 = 2_000;

// ---------------------------------------------------------------------------
// Timing helper
// ---------------------------------------------------------------------------

fn now_ns() -> u64 {
    crate::timekeeping::clock_monotonic()
}

// ---------------------------------------------------------------------------
// Benchmark implementations
// ---------------------------------------------------------------------------

/// Benchmark sequential read throughput.
///
/// Reads an existing file repeatedly to measure read bandwidth.
/// If the file doesn't exist or is empty, creates a temporary test file.
pub fn bench_sequential_read(path: &str, iterations: u64) -> KernelResult<BenchResult> {
    use crate::fs::Vfs;

    // Ensure we have something to read.
    let data = match Vfs::read_file(path) {
        Ok(d) if !d.is_empty() => d,
        _ => {
            // Create a test file with known content.
            let test_data: Vec<u8> = (0..4096u32).map(|i| (i & 0xFF) as u8).collect();
            Vfs::write_file(path, &test_data)?;
            test_data
        }
    };
    let file_size = data.len() as u64;
    drop(data);

    // Warm the cache.
    let _ = Vfs::read_file(path);

    let start = now_ns();
    let mut total_bytes: u64 = 0;
    for _ in 0..iterations {
        let d = Vfs::read_file(path)?;
        total_bytes = total_bytes.saturating_add(d.len() as u64);
    }
    let elapsed = now_ns().saturating_sub(start);

    Ok(BenchResult {
        name: String::from("sequential_read"),
        iterations,
        total_ns: elapsed,
        bytes: total_bytes,
        target_ns: if file_size <= 4096 { TARGET_SMALL_READ_NS } else { 0 },
    })
}

/// Benchmark sequential write throughput.
///
/// Writes data of the specified size repeatedly to measure write bandwidth.
pub fn bench_sequential_write(dir: &str, size: usize, iterations: u64) -> KernelResult<BenchResult> {
    use alloc::format;
    use crate::fs::Vfs;

    // Generate test data.
    let data: Vec<u8> = (0..size).map(|i| (i & 0xFF) as u8).collect();
    let path = format!("{}/_bench_write_tmp", dir);

    let start = now_ns();
    let mut total_bytes: u64 = 0;
    for _ in 0..iterations {
        Vfs::write_file(&path, &data)?;
        total_bytes = total_bytes.saturating_add(size as u64);
    }
    let elapsed = now_ns().saturating_sub(start);

    // Clean up.
    let _ = Vfs::remove(&path);

    Ok(BenchResult {
        name: String::from("sequential_write"),
        iterations,
        total_ns: elapsed,
        bytes: total_bytes,
        target_ns: 0,
    })
}

/// Benchmark metadata operations (create → stat → delete cycle).
///
/// Measures the overhead of filesystem metadata operations without
/// data transfer.
pub fn bench_metadata(dir: &str, iterations: u64) -> KernelResult<BenchResult> {
    use alloc::format;
    use crate::fs::Vfs;

    let start = now_ns();
    for i in 0..iterations {
        let path = format!("{}/_bench_meta_{}", dir, i);
        // Create (1-byte file to avoid special-casing empty files).
        Vfs::write_file(&path, &[0x42])?;
        // Stat.
        let _ = Vfs::metadata(&path)?;
        // Delete.
        Vfs::remove(&path)?;
    }
    let elapsed = now_ns().saturating_sub(start);

    Ok(BenchResult {
        name: String::from("metadata_cycle"),
        iterations,
        total_ns: elapsed,
        bytes: 0,
        target_ns: TARGET_META_CYCLE_NS,
    })
}

/// Benchmark VFS path resolution latency.
///
/// Measures how long it takes to resolve a path through the VFS layer.
/// This tests the dcache (directory entry cache) effectiveness.
pub fn bench_path_lookup(path: &str, iterations: u64) -> KernelResult<BenchResult> {
    use crate::fs::Vfs;

    // Verify path exists.
    Vfs::metadata(path)?;

    // Warm the dcache.
    for _ in 0..10 {
        let _ = Vfs::metadata(path);
    }

    let start = now_ns();
    for _ in 0..iterations {
        let _ = Vfs::metadata(path);
    }
    let elapsed = now_ns().saturating_sub(start);

    // Count path components for per-component target.
    let components = path.split('/').filter(|s| !s.is_empty()).count() as u64;
    let target = if components > 0 {
        TARGET_PATH_LOOKUP_NS.saturating_mul(components)
    } else {
        TARGET_PATH_LOOKUP_NS
    };

    Ok(BenchResult {
        name: String::from("path_lookup"),
        iterations,
        total_ns: elapsed,
        bytes: 0,
        target_ns: target,
    })
}

/// Benchmark file open/close cycle.
///
/// Measures handle creation and destruction overhead.
pub fn bench_open_close(path: &str, iterations: u64) -> KernelResult<BenchResult> {
    use crate::fs::{Vfs, handle};

    // Ensure path exists.
    let _ = Vfs::metadata(path)?;

    let start = now_ns();
    for _ in 0..iterations {
        let h = handle::open(path, handle::OpenFlags::READ)?;
        handle::close(h)?;
    }
    let elapsed = now_ns().saturating_sub(start);

    Ok(BenchResult {
        name: String::from("open_close"),
        iterations,
        total_ns: elapsed,
        bytes: 0,
        target_ns: TARGET_OPEN_NS,
    })
}

/// Benchmark small random reads (4 KiB blocks at random offsets).
pub fn bench_random_read(path: &str, iterations: u64) -> KernelResult<BenchResult> {
    use crate::fs::Vfs;

    let meta = Vfs::metadata(path)?;
    let file_size = meta.size;
    if file_size < 4096 {
        return Ok(BenchResult {
            name: String::from("random_read"),
            iterations: 0,
            total_ns: 0,
            bytes: 0,
            target_ns: 0,
        });
    }

    let max_offset = file_size.saturating_sub(4096);
    // Simple PRNG for offset selection (xorshift32).
    let mut rng: u32 = 0xDEAD_BEEF;

    let start = now_ns();
    let mut total_bytes: u64 = 0;
    for _ in 0..iterations {
        // xorshift32.
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let offset = (rng as u64) % (max_offset + 1);
        let chunk = Vfs::read_at(path, offset, 4096)?;
        total_bytes = total_bytes.saturating_add(chunk.len() as u64);
    }
    let elapsed = now_ns().saturating_sub(start);

    Ok(BenchResult {
        name: String::from("random_read"),
        iterations,
        total_ns: elapsed,
        bytes: total_bytes,
        target_ns: 0,
    })
}

/// Benchmark directory listing (readdir).
pub fn bench_readdir(dir: &str, iterations: u64) -> KernelResult<BenchResult> {
    use crate::fs::Vfs;

    // Verify directory exists.
    let _ = Vfs::readdir(dir)?;

    let start = now_ns();
    for _ in 0..iterations {
        let _ = Vfs::readdir(dir)?;
    }
    let elapsed = now_ns().saturating_sub(start);

    Ok(BenchResult {
        name: String::from("readdir"),
        iterations,
        total_ns: elapsed,
        bytes: 0,
        target_ns: 0,
    })
}

/// Run the full benchmark suite.
///
/// Creates temporary files in `dir` for testing, then cleans up.
pub fn run_all(dir: &str) -> KernelResult<BenchReport> {
    use alloc::format;
    use crate::fs::Vfs;

    serial_println!("[fsbench] Starting full benchmark suite in {}", dir);
    let suite_start = now_ns();
    let mut results = Vec::new();

    // Prepare test file for read benchmarks.
    let test_path = format!("{}/_bench_testfile", dir);
    let test_data: Vec<u8> = (0..16384u32).map(|i| (i & 0xFF) as u8).collect();
    Vfs::write_file(&test_path, &test_data)?;

    // 1. Path lookup (1000 iterations).
    match bench_path_lookup(&test_path, 1000) {
        Ok(r) => results.push(r),
        Err(e) => serial_println!("[fsbench]   path_lookup: FAILED ({:?})", e),
    }

    // 2. Open/close cycle (500 iterations).
    match bench_open_close(&test_path, 500) {
        Ok(r) => results.push(r),
        Err(e) => serial_println!("[fsbench]   open_close: FAILED ({:?})", e),
    }

    // 3. Sequential read — small file 4 KiB (500 iterations).
    let small_path = format!("{}/_bench_small", dir);
    let small_data: Vec<u8> = (0..4096u32).map(|i| (i & 0xFF) as u8).collect();
    Vfs::write_file(&small_path, &small_data)?;
    match bench_sequential_read(&small_path, 500) {
        Ok(r) => results.push(r),
        Err(e) => serial_println!("[fsbench]   seq_read: FAILED ({:?})", e),
    }

    // 4. Sequential read — larger file 16 KiB (200 iterations).
    match bench_sequential_read(&test_path, 200) {
        Ok(r) => results.push(r),
        Err(e) => serial_println!("[fsbench]   seq_read_16k: FAILED ({:?})", e),
    }

    // 5. Sequential write 4 KiB (200 iterations).
    match bench_sequential_write(dir, 4096, 200) {
        Ok(r) => results.push(r),
        Err(e) => serial_println!("[fsbench]   seq_write: FAILED ({:?})", e),
    }

    // 6. Sequential write 16 KiB (100 iterations).
    match bench_sequential_write(dir, 16384, 100) {
        Ok(r) => results.push(r),
        Err(e) => serial_println!("[fsbench]   seq_write_16k: FAILED ({:?})", e),
    }

    // 7. Metadata cycle (200 iterations).
    match bench_metadata(dir, 200) {
        Ok(r) => results.push(r),
        Err(e) => serial_println!("[fsbench]   metadata: FAILED ({:?})", e),
    }

    // 8. Random read 4 KiB blocks (200 iterations).
    match bench_random_read(&test_path, 200) {
        Ok(r) => results.push(r),
        Err(e) => serial_println!("[fsbench]   random_read: FAILED ({:?})", e),
    }

    // 9. Directory listing (200 iterations).
    match bench_readdir(dir, 200) {
        Ok(r) => results.push(r),
        Err(e) => serial_println!("[fsbench]   readdir: FAILED ({:?})", e),
    }

    // Cleanup test files.
    let _ = Vfs::remove(&test_path);
    let _ = Vfs::remove(&small_path);

    let suite_elapsed = now_ns().saturating_sub(suite_start);

    // Count targets met.
    let mut met = 0u64;
    let mut total_targets = 0u64;
    for r in &results {
        if let Some(passed) = r.meets_target() {
            total_targets += 1;
            if passed { met += 1; }
        }
    }

    BENCHMARKS_RUN.fetch_add(1, Ordering::Relaxed);
    LAST_SCORE_NS.store(suite_elapsed, Ordering::Relaxed);

    serial_println!("[fsbench] Suite complete: {} benchmarks, {}/{} targets met",
        results.len(), met, total_targets);

    Ok(BenchReport {
        results,
        total_ns: suite_elapsed,
        targets_met: (met, total_targets),
    })
}

/// Quick stats: (benchmarks_run, last_suite_time_ns).
pub fn stats() -> (u64, u64) {
    (
        BENCHMARKS_RUN.load(Ordering::Relaxed),
        LAST_SCORE_NS.load(Ordering::Relaxed),
    )
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[fsbench] Running self-test...");

    test_bench_result();
    test_path_lookup();
    test_metadata();
    test_sequential_rw();
    test_readdir();
    test_stats();

    serial_println!("[fsbench] Self-test passed (6 tests).");
    Ok(())
}

fn test_bench_result() {
    let r = BenchResult {
        name: String::from("test"),
        iterations: 100,
        total_ns: 1_000_000, // 1ms for 100 ops = 10µs each
        bytes: 409600,       // 4 KiB × 100
        target_ns: 15_000,   // 15µs target
    };
    assert_eq!(r.avg_ns(), 10_000);
    assert_eq!(r.ops_per_sec(), 100_000); // 100 ops / 1ms = 100k ops/s
    assert!(r.throughput_bps() > 0);
    assert_eq!(r.meets_target(), Some(true)); // 10µs < 15µs
    serial_println!("[fsbench]   bench_result: ok");
}

fn test_path_lookup() {
    // Benchmark path lookup on root (always exists).
    let result = bench_path_lookup("/", 10);
    assert!(result.is_ok());
    let r = result.unwrap();
    assert_eq!(r.iterations, 10);
    assert!(r.total_ns > 0);
    serial_println!("[fsbench]   path_lookup: ok (avg {}ns)", r.avg_ns());
}

fn test_metadata() {
    // Metadata benchmark on /tmp (memfs, fast).
    let result = bench_metadata("/tmp", 5);
    assert!(result.is_ok());
    let r = result.unwrap();
    assert_eq!(r.iterations, 5);
    assert!(r.total_ns > 0);
    serial_println!("[fsbench]   metadata: ok (avg {}ns/cycle)", r.avg_ns());
}

fn test_sequential_rw() {
    use crate::fs::Vfs;

    // Write a test file.
    let path = "/tmp/_bench_selftest";
    let data: Vec<u8> = alloc::vec![0xAB; 1024];
    Vfs::write_file(path, &data).unwrap();

    // Sequential read.
    let rr = bench_sequential_read(path, 5);
    assert!(rr.is_ok());
    let r = rr.unwrap();
    assert!(r.bytes >= 1024 * 5);

    // Sequential write.
    let wr = bench_sequential_write("/tmp", 1024, 5);
    assert!(wr.is_ok());
    let w = wr.unwrap();
    assert!(w.bytes >= 1024 * 5);

    let _ = Vfs::remove(path);
    serial_println!("[fsbench]   sequential_rw: ok");
}

fn test_readdir() {
    let result = bench_readdir("/", 5);
    assert!(result.is_ok());
    let r = result.unwrap();
    assert_eq!(r.iterations, 5);
    assert!(r.total_ns > 0);
    serial_println!("[fsbench]   readdir: ok (avg {}ns)", r.avg_ns());
}

fn test_stats() {
    let (runs, _) = stats();
    let _ = runs; // Just verify it doesn't panic.
    serial_println!("[fsbench]   stats: ok");
}
