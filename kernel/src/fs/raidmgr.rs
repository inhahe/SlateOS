//! RAID Manager — software RAID array management.
//!
//! Manages software RAID configurations including array creation,
//! disk addition/removal, health monitoring, and rebuild operations.
//!
//! ## Architecture
//!
//! ```text
//! RAID management
//!   → raidmgr::create_array(level, disks) → new array
//!   → raidmgr::add_disk(array, disk) → expand array
//!   → raidmgr::get_health(array) → array status
//!
//! Integration:
//!   → partmgr (partition management)
//!   → disksmart (disk health)
//!   → diskencrypt (encryption)
//!   → fshealth (filesystem health)
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

/// RAID level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaidLevel {
    Raid0,   // Striping, no redundancy.
    Raid1,   // Mirroring.
    Raid5,   // Distributed parity.
    Raid6,   // Dual parity.
    Raid10,  // Mirrored stripes.
    Jbod,    // Just a bunch of disks.
}

impl RaidLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Raid0 => "RAID 0",
            Self::Raid1 => "RAID 1",
            Self::Raid5 => "RAID 5",
            Self::Raid6 => "RAID 6",
            Self::Raid10 => "RAID 10",
            Self::Jbod => "JBOD",
        }
    }

    /// Minimum disks required for this RAID level.
    pub fn min_disks(self) -> usize {
        match self {
            Self::Raid0 => 2,
            Self::Raid1 => 2,
            Self::Raid5 => 3,
            Self::Raid6 => 4,
            Self::Raid10 => 4,
            Self::Jbod => 1,
        }
    }
}

/// Array status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayStatus {
    Healthy,
    Degraded,
    Rebuilding,
    Failed,
    Inactive,
}

impl ArrayStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Healthy => "Healthy",
            Self::Degraded => "Degraded",
            Self::Rebuilding => "Rebuilding",
            Self::Failed => "Failed",
            Self::Inactive => "Inactive",
        }
    }
}

/// Disk role in an array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskRole {
    Active,
    Spare,
    Rebuilding,
    Failed,
}

impl DiskRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Spare => "Spare",
            Self::Rebuilding => "Rebuilding",
            Self::Failed => "Failed",
        }
    }
}

/// A disk member of an array.
#[derive(Debug, Clone)]
pub struct ArrayDisk {
    pub disk_id: String,
    pub role: DiskRole,
    pub size_bytes: u64,
    pub added_ns: u64,
}

/// A RAID array.
#[derive(Debug, Clone)]
pub struct RaidArray {
    pub id: u32,
    pub name: String,
    pub level: RaidLevel,
    pub status: ArrayStatus,
    pub disks: Vec<ArrayDisk>,
    pub total_bytes: u64,
    pub usable_bytes: u64,
    pub stripe_size_kb: u32,
    pub created_ns: u64,
    pub rebuild_progress_pct: Option<u32>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ARRAYS: usize = 32;
const MAX_DISKS_PER_ARRAY: usize = 32;

struct State {
    arrays: Vec<RaidArray>,
    next_id: u32,
    total_created: u64,
    total_rebuilds: u64,
    total_failures: u64,
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

fn compute_usable(level: RaidLevel, disks: &[ArrayDisk]) -> u64 {
    let active: Vec<&ArrayDisk> = disks.iter().filter(|d| d.role == DiskRole::Active || d.role == DiskRole::Rebuilding).collect();
    let n = active.len() as u64;
    if n == 0 { return 0; }
    let min_size = active.iter().map(|d| d.size_bytes).min().unwrap_or(0);
    match level {
        RaidLevel::Raid0 | RaidLevel::Jbod => active.iter().map(|d| d.size_bytes).sum(),
        RaidLevel::Raid1 => min_size,
        RaidLevel::Raid5 => if n > 1 { min_size * (n - 1) } else { 0 },
        RaidLevel::Raid6 => if n > 2 { min_size * (n - 2) } else { 0 },
        RaidLevel::Raid10 => min_size * (n / 2),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        arrays: Vec::new(),
        next_id: 1,
        total_created: 0,
        total_rebuilds: 0,
        total_failures: 0,
        ops: 0,
    });
}

/// Create a new RAID array.
pub fn create_array(name: &str, level: RaidLevel, disk_ids: &[&str], disk_size: u64, stripe_kb: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.arrays.len() >= MAX_ARRAYS {
            return Err(KernelError::ResourceExhausted);
        }
        if disk_ids.len() < level.min_disks() {
            return Err(KernelError::InvalidArgument);
        }
        let now = crate::hpet::elapsed_ns();
        let disks: Vec<ArrayDisk> = disk_ids.iter().map(|id| ArrayDisk {
            disk_id: String::from(*id), role: DiskRole::Active,
            size_bytes: disk_size, added_ns: now,
        }).collect();
        let total: u64 = disks.iter().map(|d| d.size_bytes).sum();
        let usable = compute_usable(level, &disks);
        let id = state.next_id;
        state.next_id += 1;
        state.arrays.push(RaidArray {
            id, name: String::from(name), level, status: ArrayStatus::Healthy,
            disks, total_bytes: total, usable_bytes: usable,
            stripe_size_kb: stripe_kb, created_ns: now, rebuild_progress_pct: None,
        });
        state.total_created += 1;
        Ok(id)
    })
}

/// Add a disk to an array.
pub fn add_disk(array_id: u32, disk_id: &str, size_bytes: u64, as_spare: bool) -> KernelResult<()> {
    with_state(|state| {
        let arr = state.arrays.iter_mut().find(|a| a.id == array_id)
            .ok_or(KernelError::NotFound)?;
        if arr.disks.len() >= MAX_DISKS_PER_ARRAY {
            return Err(KernelError::ResourceExhausted);
        }
        let now = crate::hpet::elapsed_ns();
        let role = if as_spare { DiskRole::Spare } else { DiskRole::Active };
        arr.disks.push(ArrayDisk {
            disk_id: String::from(disk_id), role, size_bytes, added_ns: now,
        });
        arr.total_bytes += size_bytes;
        arr.usable_bytes = compute_usable(arr.level, &arr.disks);
        Ok(())
    })
}

/// Remove a disk from an array.
pub fn remove_disk(array_id: u32, disk_id: &str) -> KernelResult<()> {
    with_state(|state| {
        let arr = state.arrays.iter_mut().find(|a| a.id == array_id)
            .ok_or(KernelError::NotFound)?;
        let before = arr.disks.len();
        arr.disks.retain(|d| d.disk_id != disk_id);
        if arr.disks.len() == before { return Err(KernelError::NotFound); }
        let active = arr.disks.iter().filter(|d| d.role == DiskRole::Active).count();
        if active < arr.level.min_disks() {
            arr.status = ArrayStatus::Degraded;
        }
        arr.total_bytes = arr.disks.iter().map(|d| d.size_bytes).sum();
        arr.usable_bytes = compute_usable(arr.level, &arr.disks);
        Ok(())
    })
}

/// Simulate marking a disk as failed.
pub fn fail_disk(array_id: u32, disk_id: &str) -> KernelResult<()> {
    with_state(|state| {
        let arr = state.arrays.iter_mut().find(|a| a.id == array_id)
            .ok_or(KernelError::NotFound)?;
        let disk = arr.disks.iter_mut().find(|d| d.disk_id == disk_id)
            .ok_or(KernelError::NotFound)?;
        disk.role = DiskRole::Failed;
        state.total_failures += 1;
        // Check if array is degraded or failed.
        let active = arr.disks.iter().filter(|d| d.role == DiskRole::Active).count();
        if active < arr.level.min_disks() {
            arr.status = if active == 0 { ArrayStatus::Failed } else { ArrayStatus::Degraded };
        }
        arr.usable_bytes = compute_usable(arr.level, &arr.disks);
        Ok(())
    })
}

/// Start a rebuild operation.
pub fn start_rebuild(array_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let arr = state.arrays.iter_mut().find(|a| a.id == array_id)
            .ok_or(KernelError::NotFound)?;
        // Promote a spare to rebuilding.
        if let Some(spare) = arr.disks.iter_mut().find(|d| d.role == DiskRole::Spare) {
            spare.role = DiskRole::Rebuilding;
            arr.status = ArrayStatus::Rebuilding;
            arr.rebuild_progress_pct = Some(0);
            state.total_rebuilds += 1;
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// Complete a rebuild (simulate).
pub fn complete_rebuild(array_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let arr = state.arrays.iter_mut().find(|a| a.id == array_id)
            .ok_or(KernelError::NotFound)?;
        for disk in &mut arr.disks {
            if disk.role == DiskRole::Rebuilding {
                disk.role = DiskRole::Active;
            }
        }
        arr.status = ArrayStatus::Healthy;
        arr.rebuild_progress_pct = None;
        arr.usable_bytes = compute_usable(arr.level, &arr.disks);
        Ok(())
    })
}

/// Delete an array.
pub fn delete_array(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.arrays.len();
        state.arrays.retain(|a| a.id != id);
        if state.arrays.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// List all arrays.
pub fn list_arrays() -> Vec<RaidArray> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.arrays.clone())
}

/// Get a specific array.
pub fn get_array(id: u32) -> Option<RaidArray> {
    STATE.lock().as_ref().and_then(|s| s.arrays.iter().find(|a| a.id == id).cloned())
}

/// Statistics: (array_count, total_created, total_rebuilds, total_failures, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.arrays.len(), s.total_created, s.total_rebuilds, s.total_failures, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("raidmgr::self_test() — running tests...");
    init_defaults();

    // 1: Empty state.
    assert!(list_arrays().is_empty());
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Create RAID 1 (mirror).
    let id = create_array("Mirror", RaidLevel::Raid1, &["sda", "sdb"], 1_000_000_000, 64).expect("create");
    let arr = get_array(id).expect("get");
    assert_eq!(arr.level, RaidLevel::Raid1);
    assert_eq!(arr.disks.len(), 2);
    assert_eq!(arr.usable_bytes, 1_000_000_000); // Mirror = 1 disk size.
    crate::serial_println!("  [2/8] create RAID1: OK");

    // 3: Create RAID 5.
    let r5 = create_array("Data", RaidLevel::Raid5, &["sdc", "sdd", "sde"], 500_000_000, 64).expect("raid5");
    let arr5 = get_array(r5).expect("get5");
    assert_eq!(arr5.usable_bytes, 1_000_000_000); // 3 disks, parity = 2 disks usable.
    crate::serial_println!("  [3/8] create RAID5: OK");

    // 4: Add spare disk.
    add_disk(r5, "sdf", 500_000_000, true).expect("add_spare");
    let arr5 = get_array(r5).expect("get5b");
    assert_eq!(arr5.disks.len(), 4);
    crate::serial_println!("  [4/8] add spare: OK");

    // 5: Fail a disk.
    fail_disk(r5, "sdc").expect("fail");
    let arr5 = get_array(r5).expect("get5c");
    assert_eq!(arr5.status, ArrayStatus::Degraded);
    crate::serial_println!("  [5/8] fail disk: OK");

    // 6: Rebuild with spare.
    start_rebuild(r5).expect("rebuild");
    let arr5 = get_array(r5).expect("get5d");
    assert_eq!(arr5.status, ArrayStatus::Rebuilding);
    complete_rebuild(r5).expect("complete");
    let arr5 = get_array(r5).expect("get5e");
    assert_eq!(arr5.status, ArrayStatus::Healthy);
    crate::serial_println!("  [6/8] rebuild: OK");

    // 7: Delete array.
    delete_array(id).expect("delete");
    assert_eq!(list_arrays().len(), 1);
    crate::serial_println!("  [7/8] delete: OK");

    // 8: Stats.
    let (count, created, rebuilds, failures, ops) = stats();
    assert_eq!(count, 1);
    assert_eq!(created, 2);
    assert_eq!(rebuilds, 1);
    assert_eq!(failures, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("raidmgr::self_test() — all 8 tests passed");
}
