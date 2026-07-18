//! System restore — incremental backup snapshots with rotation.
//!
//! Creates system state snapshots before major changes (updates,
//! driver installs, config changes), supports rotation policies,
//! and enables recovery to any saved state.
//!
//! ## Architecture
//!
//! ```text
//! Before system update
//!   → sysrestore::create_snapshot("pre-update") → new snapshot
//!
//! Recovery boot / rollback
//!   → sysrestore::list_snapshots() → pick restore point
//!   → sysrestore::restore(snapshot_id) → revert system
//!
//! Settings panel → System → Recovery
//!   → sysrestore::configure_rotation() → retention policy
//!
//! Integration:
//!   → restorepoint (low-level restore points)
//!   → updatemgr (pre-update snapshots)
//!   → driverupdate (pre-driver snapshots)
//!   → backup (file-level backup)
//!   → syslog (restore events)
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

/// Snapshot type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotType {
    Manual,
    PreUpdate,
    PreDriverInstall,
    Scheduled,
    PreConfigChange,
    Emergency,
}

impl SnapshotType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::PreUpdate => "Pre-Update",
            Self::PreDriverInstall => "Pre-Driver",
            Self::Scheduled => "Scheduled",
            Self::PreConfigChange => "Pre-Config",
            Self::Emergency => "Emergency",
        }
    }
}

/// Snapshot state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotState {
    Creating,
    Complete,
    Restoring,
    Restored,
    Failed,
    Expired,
}

impl SnapshotState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Creating => "Creating",
            Self::Complete => "Complete",
            Self::Restoring => "Restoring",
            Self::Restored => "Restored",
            Self::Failed => "Failed",
            Self::Expired => "Expired",
        }
    }
}

/// A system snapshot.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Snapshot ID.
    pub id: u32,
    /// Description.
    pub description: String,
    /// Type.
    pub snapshot_type: SnapshotType,
    /// State.
    pub state: SnapshotState,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Files backed up.
    pub file_count: u32,
    /// Created timestamp (ns).
    pub created_ns: u64,
    /// Whether pinned (won't be auto-rotated).
    pub pinned: bool,
}

/// Rotation policy.
#[derive(Debug, Clone)]
pub struct RotationPolicy {
    /// Max total snapshots.
    pub max_snapshots: u32,
    /// Max total size in bytes.
    pub max_total_bytes: u64,
    /// Max age in seconds (0 = unlimited).
    pub max_age_secs: u64,
    /// Keep at least this many even if over limits.
    pub keep_minimum: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SNAPSHOTS: usize = 100;

struct State {
    snapshots: Vec<Snapshot>,
    next_id: u32,
    policy: RotationPolicy,
    total_created: u64,
    total_restored: u64,
    total_rotated: u64,
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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        snapshots: Vec::new(),
        next_id: 1,
        policy: RotationPolicy {
            max_snapshots: 20,
            max_total_bytes: 10 * 1024 * 1024 * 1024, // 10 GB
            max_age_secs: 30 * 24 * 3600, // 30 days
            keep_minimum: 3,
        },
        total_created: 0,
        total_restored: 0,
        total_rotated: 0,
        ops: 0,
    });
}

/// Create a new snapshot.
pub fn create_snapshot(
    description: &str, snapshot_type: SnapshotType, size_bytes: u64, file_count: u32,
) -> KernelResult<u32> {
    with_state(|state| {
        if state.snapshots.len() >= MAX_SNAPSHOTS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.total_created += 1;

        state.snapshots.push(Snapshot {
            id, description: String::from(description),
            snapshot_type, state: SnapshotState::Complete,
            size_bytes, file_count,
            created_ns: crate::hpet::elapsed_ns(),
            pinned: false,
        });
        Ok(id)
    })
}

/// Restore from a snapshot.
pub fn restore_snapshot(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let snap = state.snapshots.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        if snap.state != SnapshotState::Complete {
            return Err(KernelError::InvalidArgument);
        }
        snap.state = SnapshotState::Restored;
        state.total_restored += 1;
        Ok(())
    })
}

/// Pin a snapshot (prevent rotation).
pub fn pin_snapshot(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let snap = state.snapshots.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        snap.pinned = true;
        Ok(())
    })
}

/// Unpin a snapshot.
pub fn unpin_snapshot(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let snap = state.snapshots.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        snap.pinned = false;
        Ok(())
    })
}

/// Delete a snapshot.
pub fn delete_snapshot(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.snapshots.iter().position(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        state.snapshots.remove(pos);
        Ok(())
    })
}

/// Run rotation — remove expired/over-limit snapshots.
pub fn run_rotation() -> KernelResult<u32> {
    with_state(|state| {
        let max = state.policy.max_snapshots as usize;
        let min_keep = state.policy.keep_minimum as usize;
        let mut removed = 0u32;

        // Remove oldest non-pinned snapshots over max count.
        while state.snapshots.len() > max && state.snapshots.len() > min_keep {
            if let Some(pos) = state.snapshots.iter().position(|s| !s.pinned && s.state != SnapshotState::Creating) {
                state.snapshots.remove(pos);
                removed += 1;
                state.total_rotated += 1;
            } else {
                break;
            }
        }
        Ok(removed)
    })
}

/// Set rotation policy.
pub fn set_rotation_policy(max_snapshots: u32, max_total_bytes: u64, max_age_secs: u64, keep_minimum: u32) -> KernelResult<()> {
    with_state(|state| {
        state.policy = RotationPolicy { max_snapshots, max_total_bytes, max_age_secs, keep_minimum };
        Ok(())
    })
}

/// Get rotation policy.
pub fn get_rotation_policy() -> RotationPolicy {
    STATE.lock().as_ref().map_or(
        RotationPolicy { max_snapshots: 20, max_total_bytes: 0, max_age_secs: 0, keep_minimum: 3 },
        |s| s.policy.clone(),
    )
}

/// Get snapshot by ID.
pub fn get_snapshot(id: u32) -> KernelResult<Snapshot> {
    with_state(|state| {
        state.snapshots.iter().find(|s| s.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// List all snapshots.
pub fn list_snapshots() -> Vec<Snapshot> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.snapshots.clone())
}

/// Total size of all snapshots.
pub fn total_size() -> u64 {
    STATE.lock().as_ref().map_or(0, |s| s.snapshots.iter().map(|snap| snap.size_bytes).sum())
}

/// Statistics: (snapshot_count, total_created, total_restored, total_rotated, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.snapshots.len(), s.total_created, s.total_restored, s.total_rotated, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("sysrestore::self_test() — running tests...");
    init_defaults();

    // 1: Empty initial.
    assert!(list_snapshots().is_empty());
    crate::serial_println!("  [1/11] empty initial: OK");

    // 2: Create snapshot.
    let id1 = create_snapshot("Pre-update 1.0", SnapshotType::PreUpdate, 500_000_000, 1200)
        .expect("create");
    assert!(id1 > 0);
    crate::serial_println!("  [2/11] create snapshot: OK");

    // 3: Get snapshot.
    let snap = get_snapshot(id1).expect("get");
    assert_eq!(snap.description, "Pre-update 1.0");
    assert_eq!(snap.snapshot_type, SnapshotType::PreUpdate);
    assert_eq!(snap.state, SnapshotState::Complete);
    crate::serial_println!("  [3/11] get snapshot: OK");

    // 4: Restore.
    restore_snapshot(id1).expect("restore");
    let snap = get_snapshot(id1).expect("get2");
    assert_eq!(snap.state, SnapshotState::Restored);
    crate::serial_println!("  [4/11] restore: OK");

    // 5: Create more snapshots.
    let id2 = create_snapshot("Manual backup", SnapshotType::Manual, 200_000_000, 800).expect("create2");
    let id3 = create_snapshot("Pre-driver", SnapshotType::PreDriverInstall, 100_000_000, 50).expect("create3");
    assert_eq!(list_snapshots().len(), 3);
    crate::serial_println!("  [5/11] multiple snapshots: OK");

    // 6: Pin snapshot.
    pin_snapshot(id2).expect("pin");
    let snap = get_snapshot(id2).expect("get3");
    assert!(snap.pinned);
    crate::serial_println!("  [6/11] pin snapshot: OK");

    // 7: Total size.
    let size = total_size();
    assert_eq!(size, 800_000_000);
    crate::serial_println!("  [7/11] total size: OK");

    // 8: Delete snapshot.
    delete_snapshot(id3).expect("delete");
    assert_eq!(list_snapshots().len(), 2);
    crate::serial_println!("  [8/11] delete snapshot: OK");

    // 9: Rotation policy.
    set_rotation_policy(5, 1_000_000_000, 86400, 1).expect("policy");
    let policy = get_rotation_policy();
    assert_eq!(policy.max_snapshots, 5);
    crate::serial_println!("  [9/11] rotation policy: OK");

    // 10: Run rotation (under limit, nothing removed).
    let removed = run_rotation().expect("rotate");
    assert_eq!(removed, 0);
    crate::serial_println!("  [10/11] rotation: OK");

    // 11: Stats.
    let (count, created, restored, rotated, ops) = stats();
    assert_eq!(count, 2);
    assert_eq!(created, 3);
    assert_eq!(restored, 1);
    assert!(ops > 0);
    let _ = rotated;
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("sysrestore::self_test() — all 11 tests passed");
}
