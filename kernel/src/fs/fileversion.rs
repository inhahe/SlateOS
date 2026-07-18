//! File versioning — automatic version snapshots on file modification.
//!
//! Maintains a history of previous file versions with timestamps,
//! allowing users to browse and restore older versions. Integrates
//! with the filesystem write path to capture versions automatically.
//!
//! ## Architecture
//!
//! ```text
//! VFS write path
//!   → fileversion::capture_version(path, data) before overwrite
//!
//! File Explorer → Properties → Previous Versions
//!   → fileversion::list_versions(path)
//!   → fileversion::restore_version(path, version_id)
//!
//! Integration:
//!   → vfs (write hooks)
//!   → backup (version snapshots feed backup)
//!   → properties (version tab in file properties)
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

/// A stored file version.
#[derive(Debug, Clone)]
pub struct FileVersion {
    /// Version ID (globally unique).
    pub id: u64,
    /// File path.
    pub path: String,
    /// Version number within this file (1, 2, 3...).
    pub version: u32,
    /// Timestamp (ns since boot).
    pub timestamp_ns: u64,
    /// Size of this version in bytes.
    pub size: u64,
    /// Hash of content (simple checksum for dedup).
    pub checksum: u64,
    /// Optional change description/comment.
    pub comment: String,
    /// User who made the change.
    pub uid: u32,
}

/// Versioning policy for a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionPolicy {
    /// Keep all versions.
    KeepAll,
    /// Keep only the last N versions.
    KeepLast(u32),
    /// Keep versions from the last N hours.
    KeepHours(u32),
    /// Disabled — no versioning.
    Disabled,
}

impl VersionPolicy {
    pub fn label(self) -> &'static str {
        match self {
            Self::KeepAll => "Keep All",
            Self::KeepLast(_) => "Keep Last N",
            Self::KeepHours(_) => "Keep N Hours",
            Self::Disabled => "Disabled",
        }
    }
}

/// Configuration for a versioned path.
#[derive(Debug, Clone)]
pub struct VersionedPath {
    /// Directory or file path (prefix match).
    pub path: String,
    /// Versioning policy.
    pub policy: VersionPolicy,
    /// Minimum change size to create version (bytes).
    pub min_change_bytes: u64,
    /// Maximum single version size (bytes, 0 = no limit).
    pub max_version_size: u64,
}

const MAX_VERSIONS_TOTAL: usize = 10_000;
const MAX_VERSIONS_PER_FILE: u32 = 100;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    versions: Vec<FileVersion>,
    watched_paths: Vec<VersionedPath>,
    next_id: u64,
    global_enabled: bool,
    default_policy: VersionPolicy,
    total_captured: u64,
    total_restored: u64,
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

/// Simple hash for dedup detection.
fn simple_hash(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV offset basis
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3); // FNV prime
    }
    h
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise file versioning.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        versions: Vec::new(),
        watched_paths: Vec::new(),
        next_id: 1,
        global_enabled: true,
        default_policy: VersionPolicy::KeepLast(10),
        total_captured: 0,
        total_restored: 0,
        ops: 0,
    });
}

/// Capture a version snapshot of a file's content before overwrite.
pub fn capture_version(path: &str, data: &[u8], uid: u32) -> KernelResult<u64> {
    with_state(|state| {
        if !state.global_enabled {
            return Err(KernelError::NotSupported);
        }

        // Check if path is covered by a watched path (canonical subtree
        // predicate; see fs::pathutil).
        let policy = state.watched_paths.iter()
            .find(|w| crate::fs::pathutil::path_in_subtree(path, w.path.as_str()))
            .map(|w| w.policy)
            .unwrap_or(state.default_policy);

        if policy == VersionPolicy::Disabled {
            return Err(KernelError::NotSupported);
        }

        // Check for max version size.
        if let Some(wp) = state.watched_paths.iter()
            .find(|w| crate::fs::pathutil::path_in_subtree(path, w.path.as_str()))
        {
            if wp.max_version_size > 0 && data.len() as u64 > wp.max_version_size {
                return Err(KernelError::FileTooLarge);
            }
        }

        let checksum = simple_hash(data);

        // Dedup: skip if last version has same checksum.
        if let Some(last) = state.versions.iter().rev().find(|v| v.path == path) {
            if last.checksum == checksum {
                return Ok(last.id); // Same content, skip.
            }
        }

        // Count existing versions for this file.
        let file_version_count = state.versions.iter().filter(|v| v.path == path).count() as u32;
        let version_num = file_version_count + 1;

        let id = state.next_id;
        state.next_id += 1;
        let now = crate::hpet::elapsed_ns();

        state.versions.push(FileVersion {
            id,
            path: String::from(path),
            version: version_num,
            timestamp_ns: now,
            size: data.len() as u64,
            checksum,
            comment: String::new(),
            uid,
        });

        state.total_captured += 1;

        // Enforce policy.
        match policy {
            VersionPolicy::KeepLast(n) => {
                // Remove oldest versions for this file beyond n.
                while state.versions.iter().filter(|v| v.path == path).count() as u32 > n {
                    if let Some(pos) = state.versions.iter().position(|v| v.path == path) {
                        state.versions.remove(pos);
                    }
                }
            }
            VersionPolicy::KeepAll => {
                // Cap at MAX_VERSIONS_PER_FILE.
                while state.versions.iter().filter(|v| v.path == path).count() > MAX_VERSIONS_PER_FILE as usize {
                    if let Some(pos) = state.versions.iter().position(|v| v.path == path) {
                        state.versions.remove(pos);
                    }
                }
            }
            _ => {}
        }

        // Global cap.
        while state.versions.len() > MAX_VERSIONS_TOTAL {
            state.versions.remove(0);
        }

        Ok(id)
    })
}

/// List all versions of a specific file.
pub fn list_versions(path: &str) -> Vec<FileVersion> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let mut versions: Vec<FileVersion> = s.versions.iter()
                .filter(|v| v.path == path)
                .cloned()
                .collect();
            versions.reverse(); // newest first
            versions
        }
        None => Vec::new(),
    }
}

/// Get a specific version by ID.
pub fn get_version(id: u64) -> KernelResult<FileVersion> {
    with_state(|state| {
        state.versions.iter().find(|v| v.id == id)
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// Mark a version as restored (increments counter, returns version info).
pub fn restore_version(id: u64) -> KernelResult<FileVersion> {
    with_state(|state| {
        let version = state.versions.iter().find(|v| v.id == id)
            .cloned()
            .ok_or(KernelError::NotFound)?;
        state.total_restored += 1;
        Ok(version)
    })
}

/// Delete a specific version.
pub fn delete_version(id: u64) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.versions.iter().position(|v| v.id == id)
            .ok_or(KernelError::NotFound)?;
        state.versions.remove(pos);
        Ok(())
    })
}

/// Delete all versions of a file.
pub fn purge_file_versions(path: &str) -> KernelResult<usize> {
    with_state(|state| {
        let before = state.versions.len();
        state.versions.retain(|v| v.path != path);
        Ok(before - state.versions.len())
    })
}

/// Add a comment/description to a version.
pub fn set_version_comment(id: u64, comment: &str) -> KernelResult<()> {
    with_state(|state| {
        let version = state.versions.iter_mut().find(|v| v.id == id)
            .ok_or(KernelError::NotFound)?;
        version.comment = String::from(comment);
        Ok(())
    })
}

/// Add a watched path with versioning policy.
pub fn add_watch(path: &str, policy: VersionPolicy) -> KernelResult<()> {
    with_state(|state| {
        if state.watched_paths.iter().any(|w| w.path == path) {
            return Err(KernelError::AlreadyExists);
        }
        state.watched_paths.push(VersionedPath {
            path: String::from(path),
            policy,
            min_change_bytes: 0,
            max_version_size: 0,
        });
        Ok(())
    })
}

/// Remove a watched path.
pub fn remove_watch(path: &str) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.watched_paths.iter().position(|w| w.path == path)
            .ok_or(KernelError::NotFound)?;
        state.watched_paths.remove(pos);
        Ok(())
    })
}

/// List watched paths.
pub fn list_watches() -> Vec<VersionedPath> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.watched_paths.clone(),
        None => Vec::new(),
    }
}

/// Enable/disable versioning globally.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.global_enabled = enabled;
        Ok(())
    })
}

/// Set default policy for unwatched paths.
pub fn set_default_policy(policy: VersionPolicy) -> KernelResult<()> {
    with_state(|state| {
        state.default_policy = policy;
        Ok(())
    })
}

/// Total bytes stored across all versions.
pub fn total_storage() -> u64 {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.versions.iter().map(|v| v.size).sum(),
        None => 0,
    }
}

/// Number of distinct files with versions.
pub fn versioned_file_count() -> usize {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let mut paths: Vec<&str> = s.versions.iter().map(|v| v.path.as_str()).collect();
            paths.sort_unstable();
            paths.dedup();
            paths.len()
        }
        None => 0,
    }
}

/// Statistics: (version_count, file_count, total_captured, total_restored, watch_count, ops).
pub fn stats() -> (usize, usize, u64, u64, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let mut paths: Vec<&str> = s.versions.iter().map(|v| v.path.as_str()).collect();
            paths.sort_unstable();
            paths.dedup();
            (s.versions.len(), paths.len(), s.total_captured, s.total_restored, s.watched_paths.len(), s.ops)
        }
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("fileversion::self_test() — running tests...");

    init_defaults();

    // Test 1: Capture a version.
    let data = b"Hello, world!";
    let id1 = capture_version("/home/user/test.txt", data, 1000).expect("capture v1");
    assert!(id1 > 0);
    crate::serial_println!("  [1/11] capture version: OK");

    // Test 2: Capture a different version.
    let data2 = b"Hello, world! Updated.";
    let id2 = capture_version("/home/user/test.txt", data2, 1000).expect("capture v2");
    assert_ne!(id1, id2);
    crate::serial_println!("  [2/11] capture second version: OK");

    // Test 3: Dedup — same content should return existing ID.
    let id_dup = capture_version("/home/user/test.txt", data2, 1000).expect("capture dup");
    assert_eq!(id_dup, id2); // Same content, returns previous ID.
    crate::serial_println!("  [3/11] dedup detection: OK");

    // Test 4: List versions (newest first).
    let versions = list_versions("/home/user/test.txt");
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].id, id2); // newest
    crate::serial_println!("  [4/11] list versions: OK");

    // Test 5: Get specific version.
    let v = get_version(id1).expect("get version");
    assert_eq!(v.size, data.len() as u64);
    crate::serial_println!("  [5/11] get version: OK");

    // Test 6: Set comment.
    set_version_comment(id1, "initial version").expect("set comment");
    let v = get_version(id1).expect("get after comment");
    assert_eq!(v.comment, "initial version");
    crate::serial_println!("  [6/11] set comment: OK");

    // Test 7: Restore version (just increments counter, returns info).
    let restored = restore_version(id1).expect("restore");
    assert_eq!(restored.path, "/home/user/test.txt");
    crate::serial_println!("  [7/11] restore version: OK");

    // Test 8: Add watch with policy.
    add_watch("/home/user/documents", VersionPolicy::KeepLast(5)).expect("add watch");
    let watches = list_watches();
    assert_eq!(watches.len(), 1);
    crate::serial_println!("  [8/11] add watch: OK");

    // Test 9: Delete a version.
    delete_version(id1).expect("delete version");
    let versions = list_versions("/home/user/test.txt");
    assert_eq!(versions.len(), 1);
    crate::serial_println!("  [9/11] delete version: OK");

    // Test 10: Purge all versions of a file.
    let purged = purge_file_versions("/home/user/test.txt").expect("purge");
    assert_eq!(purged, 1);
    let versions = list_versions("/home/user/test.txt");
    assert!(versions.is_empty());
    crate::serial_println!("  [10/11] purge file versions: OK");

    // Test 11: Stats.
    let (ver_count, _file_count, captured, restored, watch_count, ops) = stats();
    assert_eq!(ver_count, 0);
    assert!(captured >= 2);
    assert!(restored >= 1);
    assert_eq!(watch_count, 1);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("fileversion::self_test() — all 11 tests passed");
}
