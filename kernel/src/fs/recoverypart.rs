//! Recovery Partition — recovery environment management.
//!
//! Manages the recovery partition containing tools for system
//! repair, factory reset, and emergency boot.
//!
//! ## Architecture
//!
//! ```text
//! Recovery management
//!   → recoverypart::get_status() → partition state
//!   → recoverypart::add_tool(name, size) → install recovery tool
//!   → recoverypart::verify_integrity() → check partition health
//!
//! Integration:
//!   → systemimage (system snapshots)
//!   → restorepoint (restore points)
//!   → bootcfg (boot configuration)
//!   → osreset (OS reset)
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

/// Recovery tool type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolType {
    SystemRepair,
    FactoryReset,
    DiskCheck,
    BootRepair,
    MemoryTest,
    CommandShell,
    NetworkDiag,
    FileRecovery,
}

impl ToolType {
    pub fn label(self) -> &'static str {
        match self {
            Self::SystemRepair => "System Repair",
            Self::FactoryReset => "Factory Reset",
            Self::DiskCheck => "Disk Check",
            Self::BootRepair => "Boot Repair",
            Self::MemoryTest => "Memory Test",
            Self::CommandShell => "Command Shell",
            Self::NetworkDiag => "Network Diagnostics",
            Self::FileRecovery => "File Recovery",
        }
    }
}

/// Partition status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionStatus {
    Healthy,
    NeedsUpdate,
    Corrupted,
    Missing,
    Rebuilding,
}

impl PartitionStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Healthy => "Healthy",
            Self::NeedsUpdate => "Needs Update",
            Self::Corrupted => "Corrupted",
            Self::Missing => "Missing",
            Self::Rebuilding => "Rebuilding",
        }
    }
}

/// A recovery tool entry.
#[derive(Debug, Clone)]
pub struct RecoveryTool {
    pub id: u32,
    pub name: String,
    pub tool_type: ToolType,
    pub version: String,
    pub size_bytes: u64,
    pub installed_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_TOOLS: usize = 50;

struct State {
    status: PartitionStatus,
    tools: Vec<RecoveryTool>,
    partition_size_bytes: u64,
    used_bytes: u64,
    next_id: u32,
    total_repairs: u64,
    total_verifications: u64,
    total_boots: u64,
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

/// Initialise the recovery-partition state.
///
/// Starts with NO recovery partition present: status [`PartitionStatus::Missing`],
/// no installed tools, zero partition/used bytes, and zeroed repair/verify/boot
/// counters. A real recovery partition is registered through
/// [`register_partition`] when one is actually detected on disk or created, and
/// tools are added through [`add_tool`]; the repair/verify/boot counters advance
/// only through real [`run_repair`] / [`verify_integrity`] / [`boot_recovery`]
/// calls. The `/proc/recoverypart` generator and the `recoverypart` kshell
/// command surface the partition status, tool list and space usage as if they
/// reflect a real recovery environment, so seeding a "Healthy" partition with
/// pre-installed tools would be fabricated procfs data — it would claim a
/// recovery partition and emergency-repair tools exist when none have been
/// detected or installed, which could lead an operator to believe recovery is
/// available when it is not.
///
/// (Previously this seeded a 500 MB "Healthy" partition with 85 MB used and four
/// fictional tools — System Repair (50 MB), Boot Repair (20 MB), Memory Test
/// (5 MB) and Command Shell (10 MB), all version "1.0" — none backed by a real
/// recovery partition or installed tool image.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        status: PartitionStatus::Missing,
        tools: Vec::new(),
        partition_size_bytes: 0,
        used_bytes: 0,
        next_id: 1,
        total_repairs: 0,
        total_verifications: 0,
        total_boots: 0,
        ops: 0,
    });
}

/// Register a real recovery partition with its on-disk capacity.
///
/// Called when a recovery partition is actually detected on disk or created.
/// Sets the partition capacity and marks the partition [`PartitionStatus::Healthy`]
/// so that tools can be installed via [`add_tool`]. Returns
/// [`KernelError::AlreadyExists`] if a partition is already registered (status
/// is anything other than `Missing`).
pub fn register_partition(size_bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        if state.status != PartitionStatus::Missing {
            return Err(KernelError::AlreadyExists);
        }
        state.partition_size_bytes = size_bytes;
        state.used_bytes = 0;
        state.status = PartitionStatus::Healthy;
        Ok(())
    })
}

/// Add a recovery tool.
pub fn add_tool(name: &str, tool_type: ToolType, version: &str, size_bytes: u64) -> KernelResult<u32> {
    with_state(|state| {
        if state.tools.len() >= MAX_TOOLS {
            return Err(KernelError::ResourceExhausted);
        }
        if state.used_bytes + size_bytes > state.partition_size_bytes {
            return Err(KernelError::DiskFull);
        }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        state.tools.push(RecoveryTool {
            id, name: String::from(name), tool_type,
            version: String::from(version), size_bytes, installed_ns: now,
        });
        state.used_bytes += size_bytes;
        Ok(id)
    })
}

/// Remove a tool.
pub fn remove_tool(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.tools.iter().position(|t| t.id == id) {
            let size = state.tools[pos].size_bytes;
            state.tools.remove(pos);
            state.used_bytes = state.used_bytes.saturating_sub(size);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// Verify partition integrity.
pub fn verify_integrity() -> KernelResult<bool> {
    with_state(|state| {
        state.total_verifications += 1;
        let ok = state.status != PartitionStatus::Corrupted && state.status != PartitionStatus::Missing;
        Ok(ok)
    })
}

/// Simulate running a repair.
pub fn run_repair(tool_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let _tool = state.tools.iter().find(|t| t.id == tool_id)
            .ok_or(KernelError::NotFound)?;
        state.total_repairs += 1;
        Ok(())
    })
}

/// Boot into recovery (simulation).
pub fn boot_recovery() -> KernelResult<()> {
    with_state(|state| {
        if state.status == PartitionStatus::Missing {
            return Err(KernelError::NotFound);
        }
        state.total_boots += 1;
        Ok(())
    })
}

/// Set partition status.
pub fn set_status(new_status: PartitionStatus) -> KernelResult<()> {
    with_state(|state| {
        state.status = new_status;
        Ok(())
    })
}

/// Get partition status.
pub fn get_status() -> PartitionStatus {
    STATE.lock().as_ref().map_or(PartitionStatus::Missing, |s| s.status)
}

/// Get space usage: (total, used, free).
pub fn space_usage() -> (u64, u64, u64) {
    STATE.lock().as_ref().map_or((0, 0, 0), |s| {
        (s.partition_size_bytes, s.used_bytes, s.partition_size_bytes.saturating_sub(s.used_bytes))
    })
}

/// List tools.
pub fn list_tools() -> Vec<RecoveryTool> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.tools.clone())
}

/// Statistics: (tool_count, total_repairs, total_verifications, total_boots, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.tools.len(), s.total_repairs, s.total_verifications, s.total_boots, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("recoverypart::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and no
    // fixtures leak into the live partition state afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no partition present (Missing), no tools, no space,
    //    zeroed counters. boot_recovery fails while the partition is Missing.
    assert_eq!(get_status(), PartitionStatus::Missing);
    assert_eq!(list_tools().len(), 0);
    assert_eq!(space_usage(), (0, 0, 0));
    let (t0, r0, v0, b0, _) = stats();
    assert_eq!((t0, r0, v0, b0), (0, 0, 0, 0));
    assert!(boot_recovery().is_err()); // Missing → cannot boot recovery
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Register partition — a real 500 MB partition is detected; status goes
    //    Healthy and space becomes available. A second register is AlreadyExists.
    register_partition(500_000_000).expect("register");
    assert_eq!(get_status(), PartitionStatus::Healthy);
    assert_eq!(space_usage(), (500_000_000, 0, 500_000_000));
    assert!(register_partition(1).is_err());
    crate::serial_println!("  [2/8] register partition: OK");

    // 3: Verify — a Healthy partition verifies OK and bumps the verify counter.
    assert!(verify_integrity().expect("verify"));
    crate::serial_println!("  [3/8] verify: OK");

    // 4: Add tool — first tool gets id 1; used bytes and the tool list advance.
    let id = add_tool("File Recovery", ToolType::FileRecovery, "1.0", 30_000_000).expect("add");
    assert_eq!(id, 1);
    assert_eq!(list_tools().len(), 1);
    let (total, used, free) = space_usage();
    assert_eq!((total, used, free), (500_000_000, 30_000_000, 470_000_000));
    crate::serial_println!("  [4/8] add tool + space: OK");

    // 5: Run repair — repairing via a real tool id bumps the repair counter;
    //    an unknown tool id errors.
    run_repair(id).expect("repair");
    assert!(run_repair(999).is_err());
    crate::serial_println!("  [5/8] repair: OK");

    // 6: Boot recovery — now the partition is Healthy, booting succeeds.
    boot_recovery().expect("boot");
    crate::serial_println!("  [6/8] boot: OK");

    // 7: Remove tool — the tool drops out and its space is reclaimed; a second
    //    remove of the same id errors.
    remove_tool(id).expect("remove");
    assert_eq!(list_tools().len(), 0);
    assert_eq!(space_usage(), (500_000_000, 0, 500_000_000));
    assert!(remove_tool(id).is_err());
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Final stats reflect only the real activity above: 0 tools, 1 repair,
    //    1 verification, 1 boot.
    let (tools, repairs, verifications, boots, ops) = stats();
    assert_eq!((tools, repairs, verifications, boots), (0, 1, 1, 1));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("recoverypart::self_test() — all 8 tests passed");
}
