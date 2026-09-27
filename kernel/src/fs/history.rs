//! File version history tracking.
//!
//! Provides automatic versioning of file contents.  When enabled for a
//! path, each save's result is stored in the CAS (content-addressed store)
//! and recorded as a version in a per-file history chain.
//!
//! ## When a version is taken (design-decisions §936, §971)
//!
//! A version is the file **as it stands after the save that made it**, and it
//! is taken **after the save has returned**, by a background worker
//! ([`record_after_save`]).  The save itself does no read-back and no
//! checksum; that was about half the cost of saving a small file.  After three
//! saves the history holds the file as it was after each of them, so going
//! back to "before my last save" is the entry before the newest.
//!
//! Two consequences, both accepted by §936 and §971:
//! - a crash between a save returning and the worker reaching it loses that
//!   one entry;
//! - saves to one file faster than the worker drains the queue collapse into
//!   one entry holding the latest content -- the worker reads the file when
//!   it gets to it, and reading it at save time would mean holding a copy,
//!   which is option B of A-Q14, the one not chosen.
//!
//! A **delete** still records synchronously, before the file goes: afterwards
//! there is nothing left to read.  And a version identical to the newest one
//! is not recorded twice.
//!
//! ## Design
//!
//! - **CAS-backed**: Old file versions are stored in `fs::cas`.  The CAS
//!   automatically deduplicates — if two files had identical content before
//!   being modified, only one copy of that content is stored.
//! - **Per-path history**: Each tracked path has a bounded list of version
//!   entries: `(timestamp, hash, size)`.  Older versions beyond the limit
//!   are evicted (and their CAS references released for GC).
//! - **Opt-in**: Not all paths are tracked: only those under a directory
//!   enrolled with [`enable_for_dir`].  The VFS queues a save's path once the
//!   write has succeeded ([`record_after_save`]) and records before a delete
//!   ([`try_auto_record`]).
//! - **Bounded**: configurable max versions per file and max total entries
//!   to prevent unbounded memory growth.
//!
//! ## Use cases
//!
//! - **Undo**: Restore a file to a previous version.
//! - **Diff**: Compare current file content with a previous version.
//! - **Audit**: Know when a file was modified and what it contained before.
//! - **Package rollback**: The package manager can use this to roll back
//!   individual file changes within a generation.
//!
//! ## Reference
//!
//! design.txt: "make a snapshot or restore from snapshot feature, with
//! branching like a VM does? options for what to include in the snapshot?"

use crate::sync::Mutex;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::cas::Hash256;
use crate::fs::path::{Path, PathBuf};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single version entry for a file.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct VersionEntry {
    /// SHA-256 hash of the file content (stored in CAS).
    pub hash: Hash256,
    /// File size in bytes at this version.
    pub size: u64,
    /// HPET timestamp (nanoseconds since boot) when this version was recorded.
    pub timestamp_ns: u64,
    /// Monotonically increasing version number per file.
    pub version: u64,
}

/// Configuration for the history system.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct HistoryConfig {
    /// Maximum number of versions to keep per file.
    /// Older versions are evicted when this is exceeded.
    pub max_versions_per_file: usize,
    /// Maximum total number of version entries across all files.
    pub max_total_entries: usize,
    /// Whether history tracking is enabled.
    pub enabled: bool,
    /// Whether VFS auto-versioning is active.
    ///
    /// When true, the VFS records each save's result after the save returns
    /// ([`record_after_save`]) and a file's content before it is removed
    /// ([`try_auto_record`]), for paths under an enrolled directory.
    /// Independent of `enabled` — manual recording via the kshell
    /// `fhist record` command works regardless.
    pub auto_version: bool,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            max_versions_per_file: 16,
            max_total_entries: 10_000,
            enabled: true,
            auto_version: true,
        }
    }
}

/// Statistics about the history system.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct HistoryStats {
    /// Number of tracked files (files with at least one version).
    pub tracked_files: usize,
    /// Total number of version entries across all files.
    pub total_versions: usize,
    /// Number of versions that were evicted (exceeded per-file limit).
    pub evicted_versions: u64,
    /// Number of record operations performed.
    pub record_count: u64,
    /// Number of restore operations performed.
    pub restore_count: u64,
    /// Whether history tracking is currently enabled.
    pub enabled: bool,
    /// Whether VFS auto-versioning is active.
    pub auto_version: bool,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Per-file version history.
struct FileHistory {
    /// Ordered list of versions, newest last.
    versions: Vec<VersionEntry>,
    /// Next version number.
    next_version: u64,
}

struct HistoryInner {
    /// Map from file path to its version history.
    files: BTreeMap<PathBuf, FileHistory>,
    /// Total number of version entries.
    total_entries: usize,
    /// Directories enrolled for automatic version recording (A-Q10).
    ///
    /// Empty means *nothing* is auto-versioned. A-Q10 was answered "on only
    /// where it is asked for", so a write is recorded only when its path lies
    /// under one of these prefixes.
    ///
    /// Deliberately here and not in `HistoryConfig`: `set_config` replaces the
    /// whole config, so a caller adjusting `max_versions_per_file` would
    /// silently un-enroll every directory as a side effect. Enrolment is
    /// state, not a tunable.
    opt_in_dirs: Vec<PathBuf>,
    /// Configuration.
    config: HistoryConfig,
    /// Statistics.
    evicted_versions: u64,
    record_count: u64,
    restore_count: u64,
}

static HISTORY: Mutex<HistoryInner> = Mutex::new(HistoryInner {
    files: BTreeMap::new(),
    total_entries: 0,
    opt_in_dirs: Vec::new(),
    config: HistoryConfig {
        max_versions_per_file: 16,
        max_total_entries: 10_000,
        enabled: true,
        // Auto-versioning starts DISABLED and is turned on at BOOT_OK by
        // `main.rs` (see `set_auto_version(true)` there). Rationale: during
        // boot the kernel stages its own system files (e.g. the glibc tree for
        // the Path Z self-tests) with interrupts disabled (IF=0), before
        // "Step 21: Enable hardware interrupts". Auto-versioning would read and
        // SHA-256-hash the *old* content of each overwritten file on that path;
        // for a multi-megabyte file in a debug build that hash can run for
        // several seconds. With IF=0 the timer-driven hard-lockup watchdog kick
        // is starved, so under host-scheduling jitter the ~9.8 s watchdog fired
        // a false positive that presented as an intermittent "BSP-dead
        // total-silence hang" (known-issues.md B-PTHREAD-YIELDBUDGET). Versioning
        // OS files as they are staged is also pointless — nobody rolls those
        // back. Enabling only post-boot (IF=1, preemptible, staging complete)
        // fixes both the latency defect and the wasted work.
        auto_version: false,
    },
    evicted_versions: 0,
    record_count: 0,
    restore_count: 0,
});

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Set the history configuration.
#[allow(dead_code)]
pub fn set_config(config: HistoryConfig) {
    HISTORY.lock().config = config;
}

/// Check if history tracking is enabled.
#[allow(dead_code)]
pub fn is_enabled() -> bool {
    HISTORY.lock().config.enabled
}

/// Enable or disable history tracking.
pub fn set_enabled(enabled: bool) {
    HISTORY.lock().config.enabled = enabled;
}

/// Check if VFS auto-versioning is active.
///
/// Auto-versioning requires both `enabled` and `auto_version` to be true.
pub fn is_auto_version_enabled() -> bool {
    let inner = HISTORY.lock();
    inner.config.enabled && inner.config.auto_version
}

/// Enable or disable VFS auto-versioning.
///
/// When enabled, the VFS automatically records old file content before
/// overwriting or removing files.
pub fn set_auto_version(enabled: bool) {
    HISTORY.lock().config.auto_version = enabled;
}

/// Check if a path is eligible for automatic version recording.
///
/// Returns `false` for paths on virtual filesystems (procfs, devfs, sysfs),
/// temporary files (/tmp), and internal metadata files.
pub fn should_auto_version(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();

    // Skip virtual filesystems (no real data to version) and /tmp
    // (ephemeral by nature).  `path_in_subtree` matches on component
    // boundaries, so `/proctor` is not mistaken for something under
    // `/proc` -- which the old byte-level `starts_with("/proc/")` also got
    // right, but only because of the trailing slash it was careful to
    // include.  Spelling it as a subtree test removes the need for that
    // care and additionally covers the directory itself.
    for virt in ["/proc", "/dev", "/sys", "/tmp"] {
        if crate::fs::pathutil::path_in_subtree(path, virt) {
            return false;
        }
    }

    // Skip internal metadata files.
    let bytes = path.as_bytes();
    if bytes.ends_with(b"/_TRASH/_INDEX") || bytes.ends_with(b"/_JOURNAL") {
        return false;
    }
    true
}

/// Whether `path` lies under an enrolled directory. Caller holds the lock.
fn enrolled_locked(inner: &HistoryInner, path: &Path) -> bool {
    inner
        .opt_in_dirs
        .iter()
        .any(|dir| crate::fs::pathutil::path_in_subtree(path, dir))
}

/// Enrol a directory for automatic version recording (A-Q10).
///
/// The entry is a path *prefix*: a directory is the normal case, but an exact
/// file path is accepted and matches only that file. Enrolling the same prefix
/// twice is a no-op rather than an error.
///
/// # Errors
///
/// `InvalidArgument` if `dir` is empty. This is the load-bearing check, not
/// defensive tidiness: `pathutil::path_in_subtree` returns `true` when the
/// prefix has no components -- deliberately, so that `/` can mean the whole
/// tree -- so one empty entry would match every path and silently turn the
/// history on for the entire filesystem. That is the exact state A-Q10 was
/// answered to get out of, and it would arrive with no diagnostic. Enrolling
/// `/` is still legal; it just has to be spelled rather than defaulted.
pub fn enable_for_dir(dir: impl AsRef<Path>) -> KernelResult<()> {
    let dir = dir.as_ref();
    if dir.as_bytes().is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    {
        let mut inner = HISTORY.lock();
        if inner
            .opt_in_dirs
            .iter()
            .any(|d| d.as_path().as_bytes() == dir.as_bytes())
        {
            return Ok(());
        }
        inner.opt_in_dirs.push(dir.to_path_buf());
    }
    // The first enrolment is the first moment a save can need recording, so
    // the worker starts here and a system with no history runs none.  If it
    // cannot start, the enrolment still stands: saves record synchronously,
    // as they did before the worker existed.
    if let Err(e) = spawn_worker() {
        serial_println!(
            "[history] the history worker did not start ({:?}); saves under {} record \
             synchronously",
            e,
            dir.display()
        );
    }
    Ok(())
}

/// Remove a directory from the enrolment list. Returns whether it was present.
///
/// Matches the stored prefix exactly; it does not un-enroll subtrees of it.
pub fn disable_for_dir(dir: impl AsRef<Path>) -> bool {
    let dir = dir.as_ref();
    let mut inner = HISTORY.lock();
    let before = inner.opt_in_dirs.len();
    inner
        .opt_in_dirs
        .retain(|d| d.as_path().as_bytes() != dir.as_bytes());
    inner.opt_in_dirs.len() != before
}

/// Whether a write to `path` would be auto-recorded: eligible *and* enrolled.
///
/// Both halves matter and they are different questions -- see
/// `should_auto_version` for why they are kept apart.
pub fn is_enrolled(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    let inner = HISTORY.lock();
    enrolled_locked(&inner, path)
}

/// The enrolled prefixes, for `/proc` and the kshell.
pub fn enrolled_dirs() -> Vec<PathBuf> {
    HISTORY.lock().opt_in_dirs.clone()
}

/// Whether a write or delete at `path` should be versioned automatically:
/// the path is eligible, auto-versioning is on, and the path lies under an
/// enrolled directory.
fn auto_eligible(path: &Path) -> bool {
    // Pure and lock-free, so it runs first: a path on procfs, devfs, sysfs or
    // /tmp is ineligible however it was enrolled, and an internal metadata
    // file never gets a history.
    if !should_auto_version(path) {
        return false;
    }
    // One acquisition answers both remaining questions, then the lock is
    // dropped before any work -- `record_version` takes it again itself.
    //
    // `should_auto_version` deliberately does NOT consult the enrolment list.
    // "Is this path eligible at all" is a property of the path; "has someone
    // asked for history here" is configuration. Keeping them apart also keeps
    // that function pure and testable, and matters mechanically:
    // `crate::sync::Mutex` is not reentrant, so a registry lookup inside a
    // function this one calls while holding the lock would deadlock.
    let inner = HISTORY.lock();
    inner.config.enabled && inner.config.auto_version && enrolled_locked(&inner, path)
}

/// Record a version of a file **now**, before it is deleted.
///
/// Called by the VFS remove paths: once the file is gone there is nothing to
/// read, so this one cannot wait for the worker.  (A save goes through
/// [`record_after_save`] instead.)  Failures are ignored -- version history is
/// best-effort and must never prevent the operation.
pub fn try_auto_record(path: impl AsRef<Path>) {
    let path = path.as_ref();
    if !auto_eligible(path) {
        return;
    }
    // Non-fatal: ignore errors from recording.  The file operation
    // must succeed even if history recording fails (e.g., CAS full,
    // file too large, read error).
    let _ = record_version(path);
}

// ---------------------------------------------------------------------------
// Recording after the save returns (design-decisions §936, §971)
// ---------------------------------------------------------------------------

/// Paths whose post-save content is waiting to be recorded, oldest first.
/// A path appears at most once: a second save before the worker reaches the
/// first is the same job, since the worker records what the file holds when
/// it gets there.
static PENDING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// The most paths [`PENDING`] holds.  Past it a save records synchronously,
/// the old cost, rather than growing the queue without bound or dropping a
/// version.
const MAX_PENDING: usize = 1024;

/// The history worker's task id, 0 until [`spawn_worker`] has started it.
static WORKER_TID: AtomicU64 = AtomicU64::new(0);

/// Set when a job is queued, cleared by the worker before it looks: a wake
/// that lands while the worker is busy is not lost.
static WORKER_WAKE: AtomicBool = AtomicBool::new(false);

/// Test hook: while set, the worker leaves the queue alone, so the self-test
/// can see a save return with its version still pending.
static WORKER_PAUSED: AtomicBool = AtomicBool::new(false);

/// How long the worker sleeps when idle, in ticks, before looking again even
/// without a wake: a lost wake costs at most this long.
const WORKER_IDLE_TICKS: u64 = 100;

/// Queue a save's result to be recorded after the save has returned.
///
/// Called by the VFS once a whole-file write has succeeded.  Does nothing for
/// a path that is not auto-versioned.  Records synchronously -- the old cost,
/// never a lost version -- when the worker is not running (it starts with the
/// first enrolment, [`enable_for_dir`]) or the queue is full.
pub fn record_after_save(path: impl AsRef<Path>) {
    let path = path.as_ref();
    if !auto_eligible(path) {
        return;
    }
    let tid = WORKER_TID.load(Ordering::Acquire);
    let queued = tid != 0 && {
        let mut pending = PENDING.lock();
        if pending.iter().any(|p| p.as_path() == path) {
            true
        } else if pending.len() < MAX_PENDING {
            pending.push(path.to_path_buf());
            true
        } else {
            false
        }
    };
    if queued {
        WORKER_WAKE.store(true, Ordering::Release);
        crate::sched::try_wake(tid);
    } else {
        // Non-fatal, as in `try_auto_record`.
        let _ = record_version(path);
    }
}

/// Start the history worker, once.  Called when the first directory is
/// enrolled, so a system with no history enabled runs no worker at all.
///
/// # Errors
///
/// Whatever `sched::spawn` fails with.  Saves then record synchronously.
fn spawn_worker() -> KernelResult<()> {
    if WORKER_TID.load(Ordering::Acquire) != 0 {
        return Ok(());
    }
    let pml4 = crate::mm::page_table::active_pml4_phys();
    let priority = crate::sched::task::DEFAULT_PRIORITY.saturating_add(4);
    let tid = crate::sched::spawn(b"fs-history", priority, history_worker, 0, pml4)?;
    // A race between two first enrolments could spawn two workers; both would
    // drain the same queue correctly, and only the first id is kept for wakes.
    if WORKER_TID
        .compare_exchange(0, tid, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        serial_println!(
            "[history] a second history worker ({}) started; it is harmless",
            tid
        );
    }
    Ok(())
}

/// The history worker: record each queued path's current content, and sleep
/// when there is none.
extern "C" fn history_worker(_arg: u64) {
    loop {
        WORKER_WAKE.store(false, Ordering::Release);
        if !WORKER_PAUSED.load(Ordering::Acquire) {
            let next = {
                let mut pending = PENDING.lock();
                if pending.is_empty() {
                    None
                } else {
                    Some(pending.remove(0))
                }
            };
            if let Some(path) = next {
                // A failure is one lost version, not a failed save: the save
                // returned long ago.  A path deleted since its save reads as
                // NotFound, and the delete recorded the content itself.
                let _ = record_version(&path);
                continue;
            }
        }
        if !WORKER_WAKE.load(Ordering::Acquire) {
            let now = crate::apic::tick_count();
            crate::sched::sleep_until_tick_interruptible(now.saturating_add(WORKER_IDLE_TICKS));
        }
    }
}

/// Whether `path` is waiting for the worker.
fn is_pending(path: &Path) -> bool {
    PENDING.lock().iter().any(|p| p.as_path() == path)
}

// ---------------------------------------------------------------------------
// Core operations
// ---------------------------------------------------------------------------

/// Record a new version of a file before it is overwritten.
///
/// This should be called by the VFS (or other code) before modifying a file.
/// The current file contents are read, hashed, and stored in the CAS.
/// A version entry is added to the file's history.
///
/// Returns the CAS hash of the saved version.
///
/// If history is disabled or the file doesn't exist, returns Ok(None).
pub fn record_version(path: impl AsRef<Path>) -> KernelResult<Option<Hash256>> {
    use crate::fs::Vfs;

    let path = path.as_ref();

    // Check if enabled.
    let inner = HISTORY.lock();
    if !inner.config.enabled {
        return Ok(None);
    }
    let max_per_file = inner.config.max_versions_per_file;
    let max_total = inner.config.max_total_entries;
    drop(inner);

    // Read the current file content (outside the lock).
    let data = match Vfs::read_file(path) {
        Ok(d) => d,
        Err(KernelError::NotFound) => return Ok(None), // No file to version.
        Err(e) => return Err(e),
    };

    // Store in CAS.
    let hash = crate::fs::cas::put(&data)?;
    let size = data.len() as u64;
    // `clock_monotonic`, not `hpet::elapsed_ns`, for the same reason and with the same
    // evidence as `journal::record`. `VersionEntry::timestamp_ns` is documented as "HPET
    // timestamp (nanoseconds since boot)" and `clock_monotonic` satisfies that contract
    // -- it is monotonic nanoseconds since boot, from the TSC rather than the HPET.
    //
    // The HPET is an MMIO read, which under hardware virtualisation is a VM exit. When
    // the identical substitution was made in `journal::record`, that phase went from
    // 14,206 ns to 337 ns and the whole 256-byte file write dropped 29%. This call sits
    // on the same path: `write_file_resolved` reaches it through `try_auto_record` on
    // every write outside /proc, /dev, /sys and /tmp.
    //
    // WHAT THIS IS WORTH DEPENDS ON THE MACHINE, and that is worth knowing before anyone
    // re-measures and concludes the change did nothing. `hpet_read`'s accelerator ratio
    // is 0.03x -- about 30x *slower* under WHPX, where the access traps, than under TCG,
    // where it does not. So expect roughly 13,900 ns under hardware virtualisation and a
    // few hundred under emulation. The measured A/B that motivated this (run b6mifed3b)
    // ran under TCG, so it cannot show this improvement at all.
    let timestamp_ns = crate::timekeeping::clock_monotonic();

    // Add to history.
    let mut inner = HISTORY.lock();
    inner.record_count = inner.record_count.saturating_add(1);

    // A version identical to the newest is not a new version: a delete right
    // after a save, or a save that rewrote the same bytes, would otherwise
    // record the same content twice.  `cas::put` took a reference for this
    // entry, so it is released again.
    if inner
        .files
        .get(path)
        .and_then(|fh| fh.versions.last())
        .is_some_and(|newest| newest.hash == hash)
    {
        drop(inner);
        // Releasing the reference just taken cannot fail for a blob `put`
        // returned; a failure would only leave one reference too many.
        crate::fs::cas::release(&hash).ok();
        return Ok(Some(hash));
    }

    // Insert the version entry.
    // Scope the mutable borrow of inner.files so we can update counters after.
    {
        let fh = inner
            .files
            .entry(path.to_path_buf())
            .or_insert(FileHistory {
                versions: Vec::new(),
                next_version: 0,
            });

        let version = fh.next_version;
        fh.next_version = fh.next_version.saturating_add(1);

        fh.versions.push(VersionEntry {
            hash,
            size,
            timestamp_ns,
            version,
        });
    }
    inner.total_entries = inner.total_entries.saturating_add(1);

    // Evict oldest versions if over the per-file limit.
    // Collect hashes to release after dropping the borrow on inner.files.
    let mut evicted_hashes: Vec<Hash256> = Vec::new();

    // Per-file eviction: count how many to remove, then update counters.
    let per_file_evicted = {
        let mut count = 0usize;
        if let Some(fh) = inner.files.get_mut(path) {
            while fh.versions.len() > max_per_file && !fh.versions.is_empty() {
                if let Some(old) = fh.versions.first() {
                    evicted_hashes.push(old.hash);
                }
                fh.versions.remove(0);
                count += 1;
            }
        }
        count
    };
    inner.total_entries = inner.total_entries.saturating_sub(per_file_evicted);
    inner.evicted_versions = inner
        .evicted_versions
        .saturating_add(per_file_evicted as u64);

    // Global eviction if over total limit.
    while inner.total_entries > max_total {
        // Find the file with the oldest entry.
        let oldest_path: Option<PathBuf> = {
            let mut best_path: Option<PathBuf> = None;
            let mut best_ts = u64::MAX;
            for (p, fh) in inner.files.iter() {
                if let Some(first) = fh.versions.first() {
                    if first.timestamp_ns < best_ts {
                        best_ts = first.timestamp_ns;
                        best_path = Some(p.clone());
                    }
                }
            }
            best_path
        };

        if let Some(ref op) = oldest_path {
            let (evicted_one, should_remove) = {
                let mut evicted = false;
                let mut empty = false;
                if let Some(fh) = inner.files.get_mut(op.as_path()) {
                    if !fh.versions.is_empty() {
                        if let Some(old) = fh.versions.first() {
                            evicted_hashes.push(old.hash);
                        }
                        fh.versions.remove(0);
                        evicted = true;
                    }
                    empty = fh.versions.is_empty();
                }
                (evicted, empty)
            };

            if evicted_one {
                inner.total_entries = inner.total_entries.saturating_sub(1);
                inner.evicted_versions = inner.evicted_versions.saturating_add(1);
            }
            if should_remove {
                inner.files.remove(op.as_path());
            }
        } else {
            break;
        }
    }

    drop(inner);

    // Release CAS references outside the HISTORY lock.
    for h in &evicted_hashes {
        crate::fs::cas::release(h).ok();
    }

    Ok(Some(hash))
}

/// Get the version history for a file.
///
/// Returns the list of versions, newest last.
/// Returns an empty list if the file has no history.
pub fn get_history(path: impl AsRef<Path>) -> Vec<VersionEntry> {
    let inner = HISTORY.lock();
    inner
        .files
        .get(path.as_ref())
        .map(|fh| fh.versions.clone())
        .unwrap_or_default()
}

/// Get the most recent version of a file from history.
///
/// Returns the CAS hash and metadata, or None if no history exists.
#[allow(dead_code)]
pub fn latest_version(path: impl AsRef<Path>) -> Option<VersionEntry> {
    let inner = HISTORY.lock();
    inner
        .files
        .get(path.as_ref())
        .and_then(|fh| fh.versions.last().cloned())
}

/// Get the content of a specific version from history.
///
/// Retrieves the data from the CAS by hash.
pub fn get_version_data(hash: &Hash256) -> KernelResult<Vec<u8>> {
    crate::fs::cas::get(hash)
}

/// Restore a file to a specific version from its history.
///
/// Writes the version's content back to the file.  Before restoring,
/// records the current content as a new version (so the restore itself
/// is undoable).
pub fn restore_version(path: impl AsRef<Path>, version_hash: &Hash256) -> KernelResult<()> {
    use crate::fs::Vfs;

    let path = path.as_ref();

    // First, record the current version (if it exists) so restore is undoable.
    record_version(path)?;

    // Get the old version data from CAS.
    let data = crate::fs::cas::get(version_hash)?;

    // Write it back to the file.
    Vfs::write_file(path, &data)?;

    // Update stats.
    let mut inner = HISTORY.lock();
    inner.restore_count = inner.restore_count.saturating_add(1);

    Ok(())
}

/// Clear all history for a specific file.
///
/// Releases all CAS references for that file's versions.
pub fn clear_file(path: impl AsRef<Path>) {
    let mut inner = HISTORY.lock();
    if let Some(fh) = inner.files.remove(path.as_ref()) {
        for v in &fh.versions {
            crate::fs::cas::release(&v.hash).ok();
        }
        inner.total_entries = inner.total_entries.saturating_sub(fh.versions.len());
    }
}

/// Clear all history for all files.
pub fn clear_all() {
    let mut inner = HISTORY.lock();
    for fh in inner.files.values() {
        for v in &fh.versions {
            crate::fs::cas::release(&v.hash).ok();
        }
    }
    inner.files.clear();
    inner.total_entries = 0;
}

/// Get the number of files being tracked.
pub fn tracked_files() -> usize {
    HISTORY.lock().files.len()
}

/// Get the total number of version entries.
#[allow(dead_code)]
pub fn total_versions() -> usize {
    HISTORY.lock().total_entries
}

/// Get history statistics.
pub fn stats() -> HistoryStats {
    let inner = HISTORY.lock();
    HistoryStats {
        tracked_files: inner.files.len(),
        total_versions: inner.total_entries,
        evicted_versions: inner.evicted_versions,
        record_count: inner.record_count,
        restore_count: inner.restore_count,
        enabled: inner.config.enabled,
        auto_version: inner.config.auto_version,
    }
}

/// List all tracked file paths.
///
/// Returns up to `max` paths, optionally filtered by prefix.
pub fn list_tracked(prefix: Option<&Path>, max: usize) -> Vec<(PathBuf, usize)> {
    let inner = HISTORY.lock();
    let mut results = Vec::new();

    for (path, fh) in inner.files.iter() {
        if let Some(pfx) = prefix {
            // Subtree filter, not a byte prefix: listing history under
            // `/home` must not also list `/homework`.
            if !crate::fs::pathutil::path_in_subtree(path, pfx) {
                continue;
            }
        }
        if results.len() >= max {
            break;
        }
        results.push((path.clone(), fh.versions.len()));
    }

    results
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the file version history module.
/// Test 7's body: a save to the enrolled `test_path` returns with its version
/// still pending, the worker then records it, and two saves leave the history
/// ending `[.., v1, v2]`.  The caller cleans up and un-pauses the worker however
/// this returns.
fn history_after_save_test(test_path: &str, v1: &[u8], v2: &[u8]) -> KernelResult<()> {
    use crate::fs::Vfs;

    let path = Path::new(test_path);
    let tid = WORKER_TID.load(Ordering::Acquire);
    if tid == 0 {
        serial_println!(
            "[history]   FAIL: enrolling {} did not start the history worker",
            test_path
        );
        return Err(KernelError::InternalError);
    }
    let newest_is = |want: &[u8]| {
        get_history(test_path)
            .last()
            .and_then(|e| get_version_data(&e.hash).ok())
            .is_some_and(|d| d.as_slice() == want)
    };
    // Up to 2000 yields for the worker to drain: it runs at a lower priority
    // than this test, and only when this test yields.
    let wait_drained = || {
        for _ in 0..2000 {
            if !is_pending(path) {
                return true;
            }
            crate::sched::try_wake(tid);
            crate::sched::yield_now();
        }
        !is_pending(path)
    };

    // 1. With the worker paused, a save returns with its version pending: the
    //    save path recorded nothing itself.
    WORKER_PAUSED.store(true, Ordering::Release);
    let before = get_history(test_path).len();
    if let Err(e) = Vfs::write_file(test_path, v1) {
        serial_println!(
            "[history]   FAIL: / is mounted read-write but writing {} failed: {:?}",
            test_path,
            e
        );
        return Err(KernelError::InternalError);
    }
    let grew = get_history(test_path).len() != before;
    let pending = is_pending(path);
    WORKER_PAUSED.store(false, Ordering::Release);
    WORKER_WAKE.store(true, Ordering::Release);
    if grew || !pending {
        serial_println!(
            "[history]   FAIL: the save recorded its own version (history grew: {}, queued: {})",
            grew,
            pending
        );
        return Err(KernelError::InternalError);
    }

    // 2. The worker records it, as the save left the file.
    if !wait_drained() || !newest_is(v1) {
        serial_println!("[history]   FAIL: the worker did not record v1 after the save");
        return Err(KernelError::InternalError);
    }

    // 3. A second save: v2 is the newest entry, and v1 the one before it.
    Vfs::write_file(test_path, v2)?;
    if !wait_drained() || !newest_is(v2) {
        serial_println!("[history]   FAIL: the worker did not record v2 after the save");
        return Err(KernelError::InternalError);
    }
    let history = get_history(test_path);
    let previous_is_v1 = history
        .len()
        .checked_sub(2)
        .and_then(|i| history.get(i))
        .and_then(|e| get_version_data(&e.hash).ok())
        .is_some_and(|d| d.as_slice() == v1);
    if !previous_is_v1 {
        serial_println!(
            "[history]   FAIL: after two saves the entry before the newest is not v1 ({} entries)",
            history.len()
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[history]   auto-version after the save OK (each save returned first; the worker \
         recorded v1, then v2, as each save left the file)"
    );
    Ok(())
}

pub fn self_test() -> KernelResult<()> {
    serial_println!("[history] Running self-test...");
    let mut skips = crate::fs::selftest::Skips::new();

    // --- Test 1: record and retrieve version ---
    {
        use crate::fs::Vfs;

        let test_path = "/tmp/_history_test_1";
        let original = b"Original content v1";

        // Ask the mount table whether /tmp exists, rather than inferring it
        // from a failed write. "The write failed" is true of a missing mount,
        // but it is equally true of a permission gate wrongly denying us, a
        // full disk, or the versioning bug this suite exists to catch -- and
        // under the old form every one of those skipped all eight tests and
        // returned success.
        if !Vfs::mounts()
            .iter()
            .any(|(p, _)| p.as_path() == crate::fs::path::Path::new("/tmp"))
        {
            serial_println!("[history] Self-test skipped (/tmp not mounted).");
            return Ok(());
        }
        if let Err(e) = Vfs::write_file(test_path, original) {
            serial_println!(
                "[history]   FAIL: /tmp is mounted but writing {} failed: {:?}",
                test_path,
                e
            );
            return Err(KernelError::InternalError);
        }

        // Record the version.
        let hash = record_version(test_path)?;
        if hash.is_none() {
            serial_println!("[history]   ERROR: record_version returned None");
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }
        let hash = hash.unwrap();

        // Verify the stored data matches.
        let stored = get_version_data(&hash)?;
        if stored.as_slice() != original {
            serial_println!("[history]   ERROR: stored data doesn't match original");
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        serial_println!("[history]   record + retrieve OK");

        // Modify the file and record another version.
        Vfs::write_file(test_path, b"Modified content v2")?;
        let _hash2 = record_version(test_path)?.unwrap();

        // Should now have 2 versions.
        let history = get_history(test_path);
        if history.len() != 2 {
            serial_println!(
                "[history]   ERROR: expected 2 versions, got {}",
                history.len()
            );
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        // First version should be the original.
        if history.first().map(|v| v.hash) != Some(hash) {
            serial_println!("[history]   ERROR: first version hash mismatch");
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        serial_println!("[history]   multi-version tracking OK");

        // Cleanup.
        clear_file(test_path);
        Vfs::remove(test_path).ok();
    }

    // --- Test 2: restore version ---
    {
        use crate::fs::Vfs;

        let test_path = "/tmp/_history_test_2";
        let v1_data = b"Version 1 data";
        let v2_data = b"Version 2 data";

        Vfs::write_file(test_path, v1_data)?;
        let v1_hash = record_version(test_path)?.unwrap();

        Vfs::write_file(test_path, v2_data)?;

        // Current content should be v2.
        let current = Vfs::read_file(test_path)?;
        if current.as_slice() != v2_data {
            serial_println!("[history]   ERROR: current data should be v2");
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        // Restore to v1.
        restore_version(test_path, &v1_hash)?;

        // Content should now be v1.
        let restored = Vfs::read_file(test_path)?;
        if restored.as_slice() != v1_data {
            serial_println!("[history]   ERROR: restored data should be v1");
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        serial_println!("[history]   restore OK");

        // Cleanup.
        clear_file(test_path);
        Vfs::remove(test_path).ok();
    }

    // --- Test 3: version eviction (per-file limit) ---
    {
        use crate::fs::Vfs;

        // Temporarily set a small limit.
        let old_config = HISTORY.lock().config.clone();
        {
            let mut inner = HISTORY.lock();
            inner.config.max_versions_per_file = 3;
        }

        let test_path = "/tmp/_history_test_3";

        for i in 0u32..6 {
            let data = alloc::format!("Version {}", i);
            Vfs::write_file(test_path, data.as_bytes())?;
            record_version(test_path)?;
        }

        // Should only have 3 versions (the 3 most recent).
        let history = get_history(test_path);
        if history.len() > 3 {
            serial_println!(
                "[history]   ERROR: expected <= 3 versions after eviction, got {}",
                history.len()
            );
            // Restore config.
            HISTORY.lock().config = old_config;
            clear_file(test_path);
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        serial_println!(
            "[history]   eviction OK (kept {}/6 versions)",
            history.len()
        );

        // Restore config.
        HISTORY.lock().config = old_config;
        clear_file(test_path);
        Vfs::remove(test_path).ok();
    }

    // --- Test 4: stats ---
    {
        let st = stats();
        if st.record_count < 1 {
            serial_println!("[history]   ERROR: record_count should be >= 1");
            return Err(KernelError::InternalError);
        }
        serial_println!(
            "[history]   stats OK (records: {}, restores: {}, evicted: {})",
            st.record_count,
            st.restore_count,
            st.evicted_versions
        );
    }

    // --- Test 5: clear all ---
    {
        use crate::fs::Vfs;

        let test_path = "/tmp/_history_test_5";
        Vfs::write_file(test_path, b"data")?;
        record_version(test_path)?;

        let before = tracked_files();
        clear_all();

        if tracked_files() != 0 {
            serial_println!("[history]   ERROR: tracked_files should be 0 after clear_all");
            Vfs::remove(test_path).ok();
            return Err(KernelError::InternalError);
        }

        serial_println!("[history]   clear_all OK (was {} files)", before);
        Vfs::remove(test_path).ok();
    }

    // --- Test 6: disabled tracking ---
    {
        set_enabled(false);

        let result = record_version("/tmp/nonexistent_disable_test")?;
        if result.is_some() {
            serial_println!("[history]   ERROR: should return None when disabled");
            set_enabled(true);
            return Err(KernelError::InternalError);
        }

        set_enabled(true);
        serial_println!("[history]   disabled tracking OK");
    }

    // --- Test 7: VFS auto-versioning, after the save (design-decisions §971) ---
    // A save to an enrolled path returns before its version is recorded, and the
    // version recorded is the file as that save left it: after writing v1 then v2
    // the history ends [.., v1, v2].  Until 2026-09-27 this asserted that the
    // newest entry after writing v2 held v1 -- the content v2's save replaced,
    // read back on the save path.  A-Q14 was answered A: the post-save content,
    // taken after the save returns, which is what makes §936's "never on the save
    // path" possible.  The assertion changed on purpose; the property tested (a
    // write to an enrolled path is versioned) did not.
    {
        use crate::fs::Vfs;

        // Temporarily ensure auto-versioning is on.
        let old_auto = HISTORY.lock().config.auto_version;
        set_auto_version(true);
        let test_path = "/_history_autoversion_test";

        // Clear any prior history for this path.
        clear_file(test_path);

        // Enrolment is what starts the worker, so it comes first.
        if enable_for_dir(test_path).is_err() {
            serial_println!("[history]   FAIL: enrolling {} was rejected", test_path);
            set_auto_version(old_auto);
            return Err(KernelError::InternalError);
        }

        // Whether `/` is mounted read-write is a fact the mount table holds;
        // a failed write is not that fact.
        let root_rw = Vfs::mounts_full()
            .iter()
            .any(|(p, _, opts)| p.as_path() == crate::fs::path::Path::new("/") && !opts.read_only);
        if root_rw {
            let outcome = history_after_save_test(
                test_path,
                b"Auto-versioned content v1",
                b"Auto-versioned content v2",
            );
            // Cleanup on every path, including a failed one.
            WORKER_PAUSED.store(false, Ordering::Release);
            clear_file(test_path);
            Vfs::remove(test_path).ok();
            set_auto_version(old_auto);
            disable_for_dir(test_path);
            outcome?;
        } else {
            skips.record("auto-version on write", "/ is not mounted read-write");
            serial_println!("[history]   SKIP auto-version test: / not mounted read-write");
            set_auto_version(old_auto);
            disable_for_dir(test_path);
        }
    }

    // --- Test 8: should_auto_version path filter ---
    {
        // Virtual filesystems excluded.
        if should_auto_version("/proc/meminfo") {
            serial_println!("[history]   ERROR: /proc/ should be excluded");
            return Err(KernelError::InternalError);
        }
        if should_auto_version("/dev/null") {
            serial_println!("[history]   ERROR: /dev/ should be excluded");
            return Err(KernelError::InternalError);
        }
        if should_auto_version("/sys/kernel/version") {
            serial_println!("[history]   ERROR: /sys/ should be excluded");
            return Err(KernelError::InternalError);
        }
        // Temp files excluded.
        if should_auto_version("/tmp/scratch.txt") {
            serial_println!("[history]   ERROR: /tmp/ should be excluded");
            return Err(KernelError::InternalError);
        }
        // Internal metadata excluded.
        if should_auto_version("/mnt/data/_TRASH/_INDEX") {
            serial_println!("[history]   ERROR: _TRASH/_INDEX should be excluded");
            return Err(KernelError::InternalError);
        }
        if should_auto_version("/mnt/data/_JOURNAL") {
            serial_println!("[history]   ERROR: _JOURNAL should be excluded");
            return Err(KernelError::InternalError);
        }
        // Normal paths included.
        if !should_auto_version("/home/user/document.txt") {
            serial_println!("[history]   ERROR: normal path should be included");
            return Err(KernelError::InternalError);
        }
        if !should_auto_version("/etc/config.yaml") {
            serial_println!("[history]   ERROR: /etc/ path should be included");
            return Err(KernelError::InternalError);
        }

        serial_println!("[history]   path filter OK");
    }

    // --- Test 9: A-Q10 opt-in enrolment ---
    // A-Q10 was answered "on only where it is asked for", so the property that
    // matters is a NEGATIVE: an ordinary document is not versioned. A negative
    // from a matcher that never matches anything is worth nothing, so this
    // establishes a positive control FIRST and asserts the negative second.
    {
        #[inline(never)]
        fn case() -> KernelResult<()> {
            let doc = "/home/user/document.txt";

            // Positive control: the matcher can fire at all. Without this, the
            // assertion further down is indistinguishable from a dead lookup.
            enable_for_dir("/home/user")?;
            if !is_enrolled(doc) {
                serial_println!("[history]   FAIL: {} not enrolled under /home/user", doc);
                return Err(KernelError::IoError);
            }
            // ...and it is scoped, not global.
            if is_enrolled("/etc/config.yaml") {
                serial_println!("[history]   FAIL: enrolling /home/user also matched /etc");
                return Err(KernelError::IoError);
            }
            if !disable_for_dir("/home/user") {
                serial_println!("[history]   FAIL: disable_for_dir removed nothing");
                return Err(KernelError::IoError);
            }
            if disable_for_dir("/home/user") {
                serial_println!("[history]   FAIL: disable_for_dir removed a prefix twice");
                return Err(KernelError::IoError);
            }

            // The answer itself: eligible, but not recorded, because nobody
            // asked for it here. These two being different is the whole design.
            if !should_auto_version(doc) {
                serial_println!("[history]   FAIL: {} should still be ELIGIBLE", doc);
                return Err(KernelError::IoError);
            }
            if is_enrolled(doc) {
                serial_println!("[history]   FAIL: {} enrolled with an empty registry", doc);
                return Err(KernelError::IoError);
            }

            // An empty prefix must be refused: `path_in_subtree` returns true
            // for a prefix with no components, so one empty entry would match
            // every path and turn the history on for the entire filesystem
            // with no diagnostic at all.
            if enable_for_dir("").is_ok() {
                serial_println!("[history]   FAIL: an empty prefix was accepted");
                return Err(KernelError::IoError);
            }
            if is_enrolled(doc) || !enrolled_dirs().is_empty() {
                serial_println!("[history]   FAIL: the refused empty prefix enrolled the tree");
                return Err(KernelError::IoError);
            }

            // Eligibility and enrolment are separate gates and the denylist
            // wins. Enrolling `/` does enrol procfs -- deliberately, `/` means
            // the whole tree -- and the pure filter still declines it.
            enable_for_dir("/")?;
            if !is_enrolled("/proc/meminfo") {
                serial_println!("[history]   FAIL: enrolling / did not cover /proc/meminfo");
                return Err(KernelError::IoError);
            }
            if should_auto_version("/proc/meminfo") {
                serial_println!("[history]   FAIL: procfs came back both enrolled and eligible");
                return Err(KernelError::IoError);
            }
            Ok(())
        }

        // Whatever a caller enrolled earlier is restored on every exit path,
        // including the failing ones -- a self-test that leaves the history
        // switched on for `/` would be worse than the bug it looks for.
        let saved = enrolled_dirs();
        for dir in &saved {
            disable_for_dir(dir.as_path());
        }
        let outcome = case();
        for dir in enrolled_dirs() {
            disable_for_dir(dir.as_path());
        }
        for dir in &saved {
            enable_for_dir(dir.as_path())?;
        }
        outcome?;
        serial_println!("[history]   opt-in enrolment OK (A-Q10): eligible is not enrolled");
    }

    skips.report("[history]");
    serial_println!("[history] Self-test passed (9 tests){}.", skips.suffix());
    Ok(())
}
