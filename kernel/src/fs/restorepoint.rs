//! System restore points — snapshot and rollback system state.
//!
//! Creates named restore points before major system changes (updates,
//! driver installs, config changes) allowing rollback to a known-good
//! state if something goes wrong.
//!
//! ## Architecture
//!
//! ```text
//! Before system update
//!   → restorepoint::create("Before update 2.1")
//!
//! Recovery / rollback
//!   → restorepoint::list_points() → pick point
//!   → restorepoint::restore(id) → revert system state
//!
//! Integration:
//!   → updatemgr (auto-create before updates)
//!   → pkgmgr (auto-create before package installs)
//!   → snapshot (filesystem-level snapshots)
//!   → fileversion (file-level history)
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

/// Restore point type / trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreType {
    /// Manual user-created.
    Manual,
    /// Before system update.
    BeforeUpdate,
    /// Before package installation.
    BeforeInstall,
    /// Before driver change.
    BeforeDriver,
    /// Scheduled automatic checkpoint.
    Scheduled,
    /// Before configuration change.
    BeforeConfig,
}

impl RestoreType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::BeforeUpdate => "Before Update",
            Self::BeforeInstall => "Before Install",
            Self::BeforeDriver => "Before Driver",
            Self::Scheduled => "Scheduled",
            Self::BeforeConfig => "Before Config",
        }
    }
}

/// Restore point status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointStatus {
    /// Complete and usable.
    Complete,
    /// Being created.
    Creating,
    /// Corrupted / unusable.
    Corrupted,
    /// Being restored from.
    Restoring,
}

impl PointStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Complete => "Complete",
            Self::Creating => "Creating...",
            Self::Corrupted => "Corrupted",
            Self::Restoring => "Restoring...",
        }
    }
}

/// A system restore point.
#[derive(Debug, Clone)]
pub struct RestorePoint {
    /// Unique ID.
    pub id: u32,
    /// User-friendly description.
    pub description: String,
    /// Type / trigger.
    pub restore_type: RestoreType,
    /// Status.
    pub status: PointStatus,
    /// Timestamp (ns since boot).
    pub created_ns: u64,
    /// Approximate size in bytes.
    pub size_bytes: u64,
    /// OS version at time of creation.
    pub os_version: String,
    /// Number of packages installed at that point.
    pub package_count: u32,
    /// Whether this point includes filesystem snapshot.
    pub has_fs_snapshot: bool,
    /// Whether this point includes registry/config snapshot.
    pub has_config_snapshot: bool,
}

/// Configuration for automatic restore point creation.
#[derive(Debug, Clone)]
pub struct RestoreConfig {
    /// Enable automatic restore points.
    pub auto_create: bool,
    /// Create before system updates.
    pub before_updates: bool,
    /// Create before package installs.
    pub before_installs: bool,
    /// Create on schedule (interval in hours, 0 = disabled).
    pub schedule_hours: u32,
    /// Maximum disk space for restore points (bytes).
    pub max_space_bytes: u64,
    /// Maximum number of restore points.
    pub max_points: usize,
}

impl Default for RestoreConfig {
    fn default() -> Self {
        Self {
            auto_create: true,
            before_updates: true,
            before_installs: true,
            schedule_hours: 168, // Weekly.
            max_space_bytes: 10 * 1024 * 1024 * 1024, // 10 GiB.
            max_points: 20,
        }
    }
}

const MAX_POINTS: usize = 100;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    points: Vec<RestorePoint>,
    config: RestoreConfig,
    next_id: u32,
    total_created: u64,
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

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        points: Vec::new(),
        config: RestoreConfig::default(),
        next_id: 1,
        total_created: 0,
        total_restored: 0,
        ops: 0,
    });
}

/// Create a restore point.
pub fn create(description: &str, restore_type: RestoreType) -> KernelResult<u32> {
    with_state(|state| {
        let max = state.config.max_points.min(MAX_POINTS);

        // Enforce limit by removing oldest.
        while state.points.len() >= max {
            if let Some(pos) = state.points.iter().position(|p| p.status == PointStatus::Complete) {
                state.points.remove(pos);
            } else {
                break;
            }
        }

        let id = state.next_id;
        state.next_id += 1;
        let now = crate::hpet::elapsed_ns();

        state.points.push(RestorePoint {
            id,
            description: String::from(description),
            restore_type,
            status: PointStatus::Complete,
            created_ns: now,
            size_bytes: 256 * 1024 * 1024, // Placeholder size.
            os_version: String::from("0.1.0"),
            package_count: 0,
            has_fs_snapshot: true,
            has_config_snapshot: true,
        });

        state.total_created += 1;
        Ok(id)
    })
}

/// Delete a restore point.
pub fn delete(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.points.iter().position(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        state.points.remove(pos);
        Ok(())
    })
}

/// Initiate restore from a point (marks it as restoring).
pub fn restore(id: u32) -> KernelResult<RestorePoint> {
    with_state(|state| {
        let point = state.points.iter_mut().find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        if point.status != PointStatus::Complete {
            return Err(KernelError::InvalidArgument);
        }
        point.status = PointStatus::Restoring;
        state.total_restored += 1;
        Ok(point.clone())
    })
}

/// Get a restore point.
pub fn get_point(id: u32) -> KernelResult<RestorePoint> {
    with_state(|state| {
        state.points.iter().find(|p| p.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// List all restore points (newest first).
pub fn list_points() -> Vec<RestorePoint> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let mut pts = s.points.clone();
            pts.reverse();
            pts
        }
        None => Vec::new(),
    }
}

/// Total disk space used by restore points.
pub fn total_space_used() -> u64 {
    let guard = STATE.lock();
    guard.as_ref().map_or(0, |s| s.points.iter().map(|p| p.size_bytes).sum())
}

/// Update configuration.
pub fn set_config(config: RestoreConfig) -> KernelResult<()> {
    with_state(|state| { state.config = config; Ok(()) })
}

pub fn get_config() -> KernelResult<RestoreConfig> {
    with_state(|state| Ok(state.config.clone()))
}

pub fn set_auto_create(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.auto_create = enabled; Ok(()) })
}

/// Check if a restore point should be created (for integration hooks).
pub fn should_auto_create(trigger: RestoreType) -> bool {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            if !s.config.auto_create { return false; }
            match trigger {
                RestoreType::BeforeUpdate => s.config.before_updates,
                RestoreType::BeforeInstall => s.config.before_installs,
                _ => true,
            }
        }
        None => false,
    }
}

/// Statistics: (point_count, total_created, total_restored, space_used_bytes, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let space: u64 = s.points.iter().map(|p| p.size_bytes).sum();
            (s.points.len(), s.total_created, s.total_restored, space, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("restorepoint::self_test() — running tests...");
    init_defaults();

    // 1: Create a restore point.
    let id1 = create("Before update 1.0", RestoreType::BeforeUpdate).expect("create");
    assert!(id1 > 0);
    crate::serial_println!("  [1/11] create restore point: OK");

    // 2: Get restore point.
    let p = get_point(id1).expect("get");
    assert_eq!(p.restore_type, RestoreType::BeforeUpdate);
    assert_eq!(p.status, PointStatus::Complete);
    crate::serial_println!("  [2/11] get restore point: OK");

    // 3: Create more points.
    let id2 = create("Manual checkpoint", RestoreType::Manual).expect("create 2");
    let id3 = create("Before driver install", RestoreType::BeforeDriver).expect("create 3");
    crate::serial_println!("  [3/11] multiple points: OK");

    // 4: List (newest first).
    let points = list_points();
    assert_eq!(points.len(), 3);
    assert_eq!(points[0].id, id3);
    crate::serial_println!("  [4/11] list points: OK");

    // 5: Total space.
    let space = total_space_used();
    assert!(space > 0);
    crate::serial_println!("  [5/11] space tracking: OK");

    // 6: Delete a point.
    delete(id2).expect("delete");
    assert_eq!(list_points().len(), 2);
    crate::serial_println!("  [6/11] delete point: OK");

    // 7: Restore from a point.
    let restored = restore(id1).expect("restore");
    assert_eq!(restored.description, "Before update 1.0");
    let p = get_point(id1).expect("get after restore");
    assert_eq!(p.status, PointStatus::Restoring);
    crate::serial_println!("  [7/11] restore: OK");

    // 8: Auto-create check.
    assert!(should_auto_create(RestoreType::BeforeUpdate));
    crate::serial_println!("  [8/11] auto-create check: OK");

    // 9: Disable auto-create.
    set_auto_create(false).expect("disable auto");
    assert!(!should_auto_create(RestoreType::BeforeUpdate));
    set_auto_create(true).expect("re-enable");
    crate::serial_println!("  [9/11] auto-create toggle: OK");

    // 10: Config.
    let cfg = get_config().expect("get config");
    assert!(cfg.before_updates);
    assert_eq!(cfg.schedule_hours, 168);
    crate::serial_println!("  [10/11] config: OK");

    // 11: Stats.
    let (count, created, restored_count, space, ops) = stats();
    assert_eq!(count, 2);
    assert!(created >= 3);
    assert!(restored_count >= 1);
    assert!(space > 0);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("restorepoint::self_test() — all 11 tests passed");
}
