//! Filesystem I/O profiling and statistics.
//!
//! Tracks per-operation timing, throughput, and hot paths for
//! filesystem operations.  Used to identify performance bottlenecks
//! and verify that VFS path lookup meets the <500ns target
//! (design spec: "cached lookup ~200-500ns per component").
//!
//! ## Architecture
//!
//! ```text
//! profile::record(OpKind::Read, path, duration_ns, bytes)
//!   → accumulates in lock-free per-operation counters
//!
//! profile::report()
//!   → ProfileReport { per-operation stats, hot paths, throughput }
//! ```
//!
//! The profiler is designed to be low-overhead: atomic counters
//! for fast-path recording, no allocations on the hot path.

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Filesystem operation categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OpKind {
    Read,
    Write,
    Stat,
    Readdir,
    Open,
    Close,
    Create,
    Remove,
    Rename,
    Mkdir,
    Rmdir,
    Symlink,
    Readlink,
    Truncate,
    SetAttr,
    Xattr,
}

impl OpKind {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Stat => "stat",
            Self::Readdir => "readdir",
            Self::Open => "open",
            Self::Close => "close",
            Self::Create => "create",
            Self::Remove => "remove",
            Self::Rename => "rename",
            Self::Mkdir => "mkdir",
            Self::Rmdir => "rmdir",
            Self::Symlink => "symlink",
            Self::Readlink => "readlink",
            Self::Truncate => "truncate",
            Self::SetAttr => "setattr",
            Self::Xattr => "xattr",
        }
    }

    /// All operation kinds.
    pub const ALL: &'static [OpKind] = &[
        Self::Read, Self::Write, Self::Stat, Self::Readdir,
        Self::Open, Self::Close, Self::Create, Self::Remove,
        Self::Rename, Self::Mkdir, Self::Rmdir, Self::Symlink,
        Self::Readlink, Self::Truncate, Self::SetAttr, Self::Xattr,
    ];
}

/// Per-operation statistics.
#[derive(Debug, Clone, Default)]
pub struct OpStats {
    /// Number of operations.
    pub count: u64,
    /// Total bytes transferred (for read/write).
    pub bytes: u64,
    /// Total time spent in nanoseconds.
    pub total_ns: u64,
    /// Minimum operation time (ns).
    pub min_ns: u64,
    /// Maximum operation time (ns).
    pub max_ns: u64,
}

impl OpStats {
    /// Average operation time in nanoseconds.
    pub fn avg_ns(&self) -> u64 {
        if self.count == 0 { 0 } else { self.total_ns / self.count }
    }

    /// Throughput in bytes per second (for read/write).
    pub fn throughput_bps(&self) -> u64 {
        if self.total_ns == 0 { return 0; }
        // bytes / (total_ns / 1e9) = bytes * 1e9 / total_ns
        self.bytes.saturating_mul(1_000_000_000) / self.total_ns
    }
}

/// Complete profiling report.
#[derive(Debug, Clone)]
pub struct ProfileReport {
    /// Per-operation statistics.
    pub ops: Vec<(OpKind, OpStats)>,
    /// Top N most accessed paths.
    pub hot_paths: Vec<(String, u64)>,
    /// Total operations.
    pub total_ops: u64,
    /// Total bytes transferred.
    pub total_bytes: u64,
    /// Profiling duration (ns).
    pub duration_ns: u64,
}

// ---------------------------------------------------------------------------
// Lock-free counters (one set per OpKind)
// ---------------------------------------------------------------------------

// We use a fixed array of atomics indexed by OpKind discriminant.
// This avoids any locking on the hot path.
const NUM_OPS: usize = 16;

static OP_COUNT: [AtomicU64; NUM_OPS] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];

static OP_BYTES: [AtomicU64; NUM_OPS] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];

static OP_TOTAL_NS: [AtomicU64; NUM_OPS] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];

static OP_MIN_NS: [AtomicU64; NUM_OPS] = [
    AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX),
    AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX),
    AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX),
    AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX),
];

static OP_MAX_NS: [AtomicU64; NUM_OPS] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];

/// Hot path tracking (uses a mutex since it allocates).
static HOT_PATHS: spin::Mutex<Option<BTreeMap<String, u64>>> = spin::Mutex::new(None);

static PROFILE_ENABLED: AtomicU64 = AtomicU64::new(0);
static PROFILE_START_NS: AtomicU64 = AtomicU64::new(0);

fn op_index(kind: OpKind) -> usize {
    kind as usize
}

// ---------------------------------------------------------------------------
// Recording API
// ---------------------------------------------------------------------------

/// Record a filesystem operation.
///
/// This is the hot-path function — uses only atomic operations,
/// no allocations, no locks (except for hot path tracking).
pub fn record(kind: OpKind, path: &str, duration_ns: u64, bytes: u64) {
    if PROFILE_ENABLED.load(Ordering::Relaxed) == 0 {
        return;
    }

    let idx = op_index(kind);
    OP_COUNT[idx].fetch_add(1, Ordering::Relaxed);
    OP_BYTES[idx].fetch_add(bytes, Ordering::Relaxed);
    OP_TOTAL_NS[idx].fetch_add(duration_ns, Ordering::Relaxed);

    // Update min (CAS loop).
    let mut current_min = OP_MIN_NS[idx].load(Ordering::Relaxed);
    while duration_ns < current_min {
        match OP_MIN_NS[idx].compare_exchange_weak(
            current_min, duration_ns, Ordering::Relaxed, Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(v) => current_min = v,
        }
    }

    // Update max (CAS loop).
    let mut current_max = OP_MAX_NS[idx].load(Ordering::Relaxed);
    while duration_ns > current_max {
        match OP_MAX_NS[idx].compare_exchange_weak(
            current_max, duration_ns, Ordering::Relaxed, Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(v) => current_max = v,
        }
    }

    // Hot path tracking (only if path is short enough to be useful).
    if path.len() <= 128 {
        if let Some(ref mut map) = *HOT_PATHS.lock() {
            if map.len() < 10000 {
                *map.entry(String::from(path)).or_insert(0) += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Control API
// ---------------------------------------------------------------------------

/// Enable profiling.
pub fn enable() {
    PROFILE_ENABLED.store(1, Ordering::Relaxed);
    PROFILE_START_NS.store(crate::timekeeping::clock_monotonic(), Ordering::Relaxed);
    *HOT_PATHS.lock() = Some(BTreeMap::new());
    serial_println!("[profile] Filesystem profiling enabled");
}

/// Disable profiling.
pub fn disable() {
    PROFILE_ENABLED.store(0, Ordering::Relaxed);
    serial_println!("[profile] Filesystem profiling disabled");
}

/// Check if profiling is enabled.
pub fn is_enabled() -> bool {
    PROFILE_ENABLED.load(Ordering::Relaxed) != 0
}

/// Reset all counters.
pub fn reset() {
    for i in 0..NUM_OPS {
        OP_COUNT[i].store(0, Ordering::Relaxed);
        OP_BYTES[i].store(0, Ordering::Relaxed);
        OP_TOTAL_NS[i].store(0, Ordering::Relaxed);
        OP_MIN_NS[i].store(u64::MAX, Ordering::Relaxed);
        OP_MAX_NS[i].store(0, Ordering::Relaxed);
    }
    if let Some(ref mut map) = *HOT_PATHS.lock() {
        map.clear();
    }
    PROFILE_START_NS.store(crate::timekeeping::clock_monotonic(), Ordering::Relaxed);
    serial_println!("[profile] Counters reset");
}

// ---------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------

/// Generate a profiling report.
pub fn report() -> ProfileReport {
    let now = crate::timekeeping::clock_monotonic();
    let start = PROFILE_START_NS.load(Ordering::Relaxed);
    let duration_ns = now.saturating_sub(start);

    let mut ops = Vec::new();
    let mut total_ops: u64 = 0;
    let mut total_bytes: u64 = 0;

    for kind in OpKind::ALL {
        let idx = op_index(*kind);
        let count = OP_COUNT[idx].load(Ordering::Relaxed);
        if count == 0 {
            continue;
        }
        let min = OP_MIN_NS[idx].load(Ordering::Relaxed);
        let stats = OpStats {
            count,
            bytes: OP_BYTES[idx].load(Ordering::Relaxed),
            total_ns: OP_TOTAL_NS[idx].load(Ordering::Relaxed),
            min_ns: if min == u64::MAX { 0 } else { min },
            max_ns: OP_MAX_NS[idx].load(Ordering::Relaxed),
        };
        total_ops = total_ops.saturating_add(count);
        total_bytes = total_bytes.saturating_add(stats.bytes);
        ops.push((*kind, stats));
    }

    // Get top hot paths.
    let hot_paths = if let Some(ref map) = *HOT_PATHS.lock() {
        let mut entries: Vec<(String, u64)> = map.iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        entries.sort_by_key(|e| core::cmp::Reverse(e.1));
        entries.truncate(20);
        entries
    } else {
        Vec::new()
    };

    ProfileReport {
        ops,
        hot_paths,
        total_ops,
        total_bytes,
        duration_ns,
    }
}

/// Get quick summary: (total_ops, total_bytes, is_enabled).
pub fn stats() -> (u64, u64, bool) {
    let mut total_ops: u64 = 0;
    let mut total_bytes: u64 = 0;
    for i in 0..NUM_OPS {
        total_ops = total_ops.saturating_add(OP_COUNT[i].load(Ordering::Relaxed));
        total_bytes = total_bytes.saturating_add(OP_BYTES[i].load(Ordering::Relaxed));
    }
    (total_ops, total_bytes, is_enabled())
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> crate::error::KernelResult<()> {
    serial_println!("[profile] Running self-test...");

    test_enable_disable();
    test_record();
    test_min_max();
    test_report();
    test_reset();
    test_stats();

    serial_println!("[profile] Self-test passed (6 tests).");
    Ok(())
}

fn test_enable_disable() {
    assert!(!is_enabled());
    enable();
    assert!(is_enabled());
    disable();
    assert!(!is_enabled());
    serial_println!("[profile]   enable/disable: ok");
}

fn test_record() {
    enable();
    reset();

    record(OpKind::Read, "/test/file.txt", 500, 1024);
    record(OpKind::Read, "/test/file.txt", 300, 512);
    record(OpKind::Write, "/test/out.txt", 800, 2048);

    let idx = op_index(OpKind::Read);
    assert_eq!(OP_COUNT[idx].load(Ordering::Relaxed), 2);
    assert_eq!(OP_BYTES[idx].load(Ordering::Relaxed), 1536);

    disable();
    serial_println!("[profile]   record: ok");
}

fn test_min_max() {
    enable();
    reset();

    record(OpKind::Stat, "/a", 100, 0);
    record(OpKind::Stat, "/b", 500, 0);
    record(OpKind::Stat, "/c", 200, 0);

    let idx = op_index(OpKind::Stat);
    assert_eq!(OP_MIN_NS[idx].load(Ordering::Relaxed), 100);
    assert_eq!(OP_MAX_NS[idx].load(Ordering::Relaxed), 500);

    disable();
    serial_println!("[profile]   min/max: ok");
}

fn test_report() {
    enable();
    reset();

    record(OpKind::Read, "/data", 1000, 4096);
    record(OpKind::Write, "/data", 2000, 8192);

    let rpt = report();
    assert_eq!(rpt.total_ops, 2);
    assert_eq!(rpt.total_bytes, 4096 + 8192);
    assert!(!rpt.ops.is_empty());

    disable();
    serial_println!("[profile]   report: ok");
}

fn test_reset() {
    enable();
    record(OpKind::Create, "/x", 100, 0);
    reset();

    let idx = op_index(OpKind::Create);
    assert_eq!(OP_COUNT[idx].load(Ordering::Relaxed), 0);

    disable();
    serial_println!("[profile]   reset: ok");
}

fn test_stats() {
    enable();
    reset();
    record(OpKind::Readdir, "/dir", 400, 0);

    let (ops, bytes, enabled) = stats();
    assert!(ops >= 1);
    let _ = bytes;
    assert!(enabled);

    disable();
    serial_println!("[profile]   stats: ok");
}
