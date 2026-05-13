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
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        status: PartitionStatus::Healthy,
        tools: alloc::vec![
            RecoveryTool { id: 1, name: String::from("System Repair"), tool_type: ToolType::SystemRepair, version: String::from("1.0"), size_bytes: 50_000_000, installed_ns: now },
            RecoveryTool { id: 2, name: String::from("Boot Repair"), tool_type: ToolType::BootRepair, version: String::from("1.0"), size_bytes: 20_000_000, installed_ns: now },
            RecoveryTool { id: 3, name: String::from("Memory Test"), tool_type: ToolType::MemoryTest, version: String::from("1.0"), size_bytes: 5_000_000, installed_ns: now },
            RecoveryTool { id: 4, name: String::from("Command Shell"), tool_type: ToolType::CommandShell, version: String::from("1.0"), size_bytes: 10_000_000, installed_ns: now },
        ],
        partition_size_bytes: 500_000_000,
        used_bytes: 85_000_000,
        next_id: 5,
        total_repairs: 0,
        total_verifications: 0,
        total_boots: 0,
        ops: 0,
    });
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(get_status(), PartitionStatus::Healthy);
    assert_eq!(list_tools().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Verify.
    let ok = verify_integrity().expect("verify");
    assert!(ok);
    crate::serial_println!("  [2/8] verify: OK");

    // 3: Add tool.
    let id = add_tool("File Recovery", ToolType::FileRecovery, "1.0", 30_000_000).expect("add");
    assert_eq!(list_tools().len(), 5);
    crate::serial_println!("  [3/8] add tool: OK");

    // 4: Space check.
    let (total, used, free) = space_usage();
    assert_eq!(total, 500_000_000);
    assert_eq!(used, 115_000_000);
    assert_eq!(free, 385_000_000);
    crate::serial_println!("  [4/8] space: OK");

    // 5: Run repair.
    run_repair(1).expect("repair");
    crate::serial_println!("  [5/8] repair: OK");

    // 6: Boot recovery.
    boot_recovery().expect("boot");
    crate::serial_println!("  [6/8] boot: OK");

    // 7: Remove tool.
    remove_tool(id).expect("remove");
    assert_eq!(list_tools().len(), 4);
    let (_, used2, _) = space_usage();
    assert_eq!(used2, 85_000_000);
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Stats.
    let (tools, repairs, verifications, boots, ops) = stats();
    assert_eq!(tools, 4);
    assert_eq!(repairs, 1);
    assert_eq!(verifications, 1);
    assert_eq!(boots, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("recoverypart::self_test() — all 8 tests passed");
}
