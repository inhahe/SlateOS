//! Inode Statistics — inode/dentry cache monitoring.
//!
//! Tracks inode allocations, dentry cache (dcache) hits/misses,
//! inode evictions, and per-filesystem inode counts. Essential
//! for diagnosing VFS performance and cache efficiency.
//!
//! ## Architecture
//!
//! ```text
//! Inode/dentry monitoring
//!   → inodestat::alloc_inode(fs) → track inode allocation
//!   → inodestat::free_inode(fs) → track inode free
//!   → inodestat::dcache_lookup(hit) → track dentry cache
//!   → inodestat::evict(fs, count) → track inode eviction
//!
//! Integration:
//!   → fscache (filesystem cache)
//!   → vfs (virtual filesystem)
//!   → slabstat (slab allocator — inode slabs)
//!   → memcg (memory cgroup — inode pressure)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Filesystem type for inode tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsType {
    Ext4,
    Fat32,
    Tmpfs,
    Procfs,
    Devfs,
    Overlayfs,
}

impl FsType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ext4 => "ext4",
            Self::Fat32 => "fat32",
            Self::Tmpfs => "tmpfs",
            Self::Procfs => "procfs",
            Self::Devfs => "devfs",
            Self::Overlayfs => "overlayfs",
        }
    }
}

/// Per-filesystem inode stats.
#[derive(Debug, Clone)]
pub struct FsInodeStats {
    pub fs_type: FsType,
    pub mount_point: String,
    pub allocated: u64,
    pub freed: u64,
    pub active: u64,
    pub evicted: u64,
    pub dirty: u64,
}

/// Dentry cache statistics.
#[derive(Debug, Clone)]
pub struct DcacheStats {
    pub entries: u64,
    pub lookups: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub negative_entries: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_FILESYSTEMS: usize = 32;

struct State {
    filesystems: Vec<FsInodeStats>,
    dcache: DcacheStats,
    total_allocs: u64,
    total_frees: u64,
    total_evictions: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise an **empty** inode/dentry statistics table.
///
/// Seeds NO filesystems and a zeroed dcache.  Real per-filesystem inode
/// accounting is wired through [`register_fs`] (one row per mounted filesystem
/// the VFS layer mounts) and the `alloc_inode`/`free_inode`/`evict`/`mark_dirty`
/// functions; `dcache_lookup` accumulates genuine dentry-cache activity.  Until
/// those are called the table is genuinely empty, so `/proc/inodestat` and the
/// `inodestat` kshell command report zeros rather than fabricated numbers — the
/// kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded three fictional filesystems (ext4 on `/`:
/// allocated 500k / freed 200k / active 300k / evicted 50k / dirty 1000; tmpfs
/// on `/tmp`: allocated 10k / freed 8k / active 2k / evicted 500; procfs on
/// `/proc`: allocated 5k / freed 3k / active 2k) plus an invented dcache
/// (entries 200k, lookups 50M, hits 47.5M, misses 2.5M, evictions 100k,
/// negative 10k) and aggregate totals (total_allocs 515k, total_frees 211k,
/// total_evictions 50.5k), which `/proc/inodestat` then displayed as if they
/// were real measured VFS cache activity.  That demo data was removed; the
/// self-test now builds its own fixtures explicitly via the real API (see
/// [`self_test`]).  The VFS is expected to call [`register_fs`] when a
/// filesystem is mounted and the record functions as inodes flow through it.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        filesystems: Vec::new(),
        dcache: DcacheStats { entries: 0, lookups: 0, hits: 0, misses: 0, evictions: 0, negative_entries: 0 },
        total_allocs: 0,
        total_frees: 0,
        total_evictions: 0,
        ops: 0,
    });
}

/// Register a mounted filesystem for inode accounting.
///
/// Adds one zeroed row the VFS can drive via the record functions.  Returns
/// [`KernelError::AlreadyExists`] if a row for `fs_type` is already present and
/// [`KernelError::ResourceExhausted`] once [`MAX_FILESYSTEMS`] rows exist.
pub fn register_fs(fs_type: FsType, mount_point: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.filesystems.len() >= MAX_FILESYSTEMS {
            return Err(KernelError::ResourceExhausted);
        }
        if state.filesystems.iter().any(|f| f.fs_type == fs_type) {
            return Err(KernelError::AlreadyExists);
        }
        state.filesystems.push(FsInodeStats {
            fs_type,
            mount_point: String::from(mount_point),
            allocated: 0,
            freed: 0,
            active: 0,
            evicted: 0,
            dirty: 0,
        });
        Ok(())
    })
}

/// Allocate an inode.
pub fn alloc_inode(fs_type: FsType) -> KernelResult<()> {
    with_state(|state| {
        let fs = state.filesystems.iter_mut().find(|f| f.fs_type == fs_type)
            .ok_or(KernelError::NotFound)?;
        fs.allocated += 1;
        fs.active += 1;
        state.total_allocs += 1;
        Ok(())
    })
}

/// Free an inode.
pub fn free_inode(fs_type: FsType) -> KernelResult<()> {
    with_state(|state| {
        let fs = state.filesystems.iter_mut().find(|f| f.fs_type == fs_type)
            .ok_or(KernelError::NotFound)?;
        fs.freed += 1;
        fs.active = fs.active.saturating_sub(1);
        state.total_frees += 1;
        Ok(())
    })
}

/// Record a dentry cache lookup.
pub fn dcache_lookup(hit: bool) -> KernelResult<()> {
    with_state(|state| {
        state.dcache.lookups += 1;
        if hit {
            state.dcache.hits += 1;
        } else {
            state.dcache.misses += 1;
        }
        Ok(())
    })
}

/// Evict inodes from a filesystem.
pub fn evict(fs_type: FsType, count: u64) -> KernelResult<()> {
    with_state(|state| {
        let fs = state.filesystems.iter_mut().find(|f| f.fs_type == fs_type)
            .ok_or(KernelError::NotFound)?;
        fs.evicted += count;
        fs.active = fs.active.saturating_sub(count);
        state.total_evictions += count;
        Ok(())
    })
}

/// Mark inodes dirty.
pub fn mark_dirty(fs_type: FsType, count: u64) -> KernelResult<()> {
    with_state(|state| {
        let fs = state.filesystems.iter_mut().find(|f| f.fs_type == fs_type)
            .ok_or(KernelError::NotFound)?;
        fs.dirty += count;
        Ok(())
    })
}

/// Get per-filesystem inode stats.
pub fn fs_stats() -> Vec<FsInodeStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.filesystems.clone())
}

/// Get dcache stats.
pub fn dcache_stats() -> DcacheStats {
    STATE.lock().as_ref().map_or(
        DcacheStats { entries: 0, lookups: 0, hits: 0, misses: 0, evictions: 0, negative_entries: 0 },
        |s| s.dcache.clone(),
    )
}

/// Dcache hit rate as percentage * 100 (integer math).
pub fn dcache_hit_rate() -> u64 {
    let d = dcache_stats();
    if d.lookups == 0 { return 0; }
    d.hits * 10000 / d.lookups
}

/// Statistics: (fs_count, total_allocs, total_frees, total_evictions, dcache_lookups, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.filesystems.len(), s.total_allocs, s.total_frees, s.total_evictions, s.dcache.lookups, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("inodestat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/inodestat must never surface).
    // Resetting first clears any residue from a prior `inodestat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated filesystems, dcache, or totals.
    assert_eq!(fs_stats().len(), 0);
    let d0 = dcache_stats();
    assert_eq!((d0.lookups, d0.hits, d0.misses), (0, 0, 0));
    let (c0, a0, f0, e0, l0, _o0) = stats();
    assert_eq!((c0, a0, f0, e0, l0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register filesystems — rows created with zeroed counters; dup fails.
    register_fs(FsType::Ext4, "/").expect("reg ext4");
    register_fs(FsType::Tmpfs, "/tmp").expect("reg tmpfs");
    assert!(register_fs(FsType::Ext4, "/").is_err()); // AlreadyExists
    assert_eq!(fs_stats().len(), 2);
    let ext4 = fs_stats().iter().find(|f| f.fs_type == FsType::Ext4).cloned().expect("ext4");
    assert_eq!((ext4.allocated, ext4.active, ext4.evicted), (0, 0, 0));
    crate::serial_println!("  [2/8] register: OK");

    // 3: Alloc inode increments allocated + active exactly from zero.
    alloc_inode(FsType::Ext4).expect("alloc");
    let ext4 = fs_stats().iter().find(|f| f.fs_type == FsType::Ext4).cloned().expect("ext4");
    assert_eq!(ext4.allocated, 1);
    assert_eq!(ext4.active, 1);
    crate::serial_println!("  [3/8] alloc: OK");

    // 4: Free inode increments freed, decrements active back to zero.
    free_inode(FsType::Ext4).expect("free");
    let ext4 = fs_stats().iter().find(|f| f.fs_type == FsType::Ext4).cloned().expect("ext4");
    assert_eq!(ext4.freed, 1);
    assert_eq!(ext4.active, 0);
    crate::serial_println!("  [4/8] free: OK");

    // 5: Dcache lookups + hit rate computed exactly from zero (3 hits / 1 miss).
    dcache_lookup(true).expect("hit");
    dcache_lookup(true).expect("hit");
    dcache_lookup(true).expect("hit");
    dcache_lookup(false).expect("miss");
    let d = dcache_stats();
    assert_eq!((d.lookups, d.hits, d.misses), (4, 3, 1));
    assert_eq!(dcache_hit_rate(), 7500); // 3/4 = 75.00%
    crate::serial_println!("  [5/8] dcache + hit rate: OK");

    // 6: Evict decrements active (alloc 2, evict 2 → active 0) and counts.
    alloc_inode(FsType::Tmpfs).expect("a1");
    alloc_inode(FsType::Tmpfs).expect("a2");
    evict(FsType::Tmpfs, 2).expect("evict");
    let tmpfs = fs_stats().iter().find(|f| f.fs_type == FsType::Tmpfs).cloned().expect("tmpfs");
    assert_eq!(tmpfs.evicted, 2);
    assert_eq!(tmpfs.active, 0);
    crate::serial_println!("  [6/8] evict: OK");

    // 7: Unregistered filesystem → NotFound.
    assert!(alloc_inode(FsType::Overlayfs).is_err());
    assert!(evict(FsType::Overlayfs, 1).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (fss, allocs, frees, evictions, lookups, ops) = stats();
    assert_eq!(fss, 2);
    assert_eq!(allocs, 3);     // 1 ext4 + 2 tmpfs
    assert_eq!(frees, 1);      // 1 ext4
    assert_eq!(evictions, 2);  // 2 tmpfs
    assert_eq!(lookups, 4);    // 3 hits + 1 miss
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/inodestat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the VFS wires real
    // accounting.
    *STATE.lock() = None;

    crate::serial_println!("inodestat::self_test() — all 8 tests passed");
}
