//! Swap Monitor — swap space usage monitoring.
//!
//! Tracks swap device/file usage, swap-in/out rates, and per-process
//! swap consumption for memory pressure analysis.
//!
//! ## Architecture
//!
//! ```text
//! Swap monitoring
//!   → swapmon::usage() → current swap utilization
//!   → swapmon::swap_rate() → swap in/out rates
//!   → swapmon::per_process() → per-process swap usage
//!   → swapmon::history() → usage over time
//!
//! Integration:
//!   → swapcfg (swap configuration)
//!   → memdiag (memory diagnostics)
//!   → perfmon (performance monitor)
//!   → sysresource (system resources)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Swap device type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapType {
    Partition,
    File,
    Zram,
    Zswap,
}

impl SwapType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Partition => "Partition",
            Self::File => "File",
            Self::Zram => "Zram",
            Self::Zswap => "Zswap",
        }
    }
}

/// A swap device/file entry.
#[derive(Debug, Clone)]
pub struct SwapDevice {
    pub id: u32,
    pub path: String,
    pub swap_type: SwapType,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub priority: i32,
    pub enabled: bool,
}

impl SwapDevice {
    pub fn free_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.used_bytes)
    }

    pub fn usage_pct(&self) -> u32 {
        if self.total_bytes == 0 { 0 }
        else { (self.used_bytes * 100 / self.total_bytes) as u32 }
    }
}

/// Per-process swap usage.
#[derive(Debug, Clone)]
pub struct ProcessSwap {
    pub pid: u32,
    pub name: String,
    pub swap_bytes: u64,
}

/// Swap usage snapshot (for history).
#[derive(Debug, Clone)]
pub struct SwapSnapshot {
    pub timestamp_ns: u64,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub swap_in_rate: u64,
    pub swap_out_rate: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 16;
const MAX_PROCESSES: usize = 200;
const MAX_HISTORY: usize = 500;

struct State {
    devices: Vec<SwapDevice>,
    processes: Vec<ProcessSwap>,
    snapshots: Vec<SwapSnapshot>,
    next_id: u32,
    total_swap_in: u64,
    total_swap_out: u64,
    total_swap_in_bytes: u64,
    total_swap_out_bytes: u64,
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
        devices: alloc::vec![
            SwapDevice {
                id: 1, path: String::from("/dev/sda2"),
                swap_type: SwapType::Partition,
                total_bytes: 4_294_967_296, // 4 GiB
                used_bytes: 524_288_000,   // ~500 MiB
                priority: -1, enabled: true,
            },
        ],
        processes: Vec::new(),
        snapshots: Vec::new(),
        next_id: 2,
        total_swap_in: 0,
        total_swap_out: 0,
        total_swap_in_bytes: 0,
        total_swap_out_bytes: 0,
        ops: 0,
    });
}

/// Add a swap device.
pub fn add_device(path: &str, swap_type: SwapType, total_bytes: u64, priority: i32) -> KernelResult<u32> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }
        if state.devices.iter().any(|d| d.path == path) {
            return Err(KernelError::AlreadyExists);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.devices.push(SwapDevice {
            id, path: String::from(path), swap_type, total_bytes,
            used_bytes: 0, priority, enabled: true,
        });
        Ok(id)
    })
}

/// Remove a swap device.
pub fn remove_device(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.devices.len();
        state.devices.retain(|d| d.id != id);
        if state.devices.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Enable/disable a swap device.
pub fn set_enabled(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        dev.enabled = enabled;
        if !enabled { dev.used_bytes = 0; }
        Ok(())
    })
}

/// Record a swap-in event.
pub fn record_swap_in(bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        state.total_swap_in += 1;
        state.total_swap_in_bytes += bytes;
        // Decrease usage on the first enabled device.
        if let Some(dev) = state.devices.iter_mut().find(|d| d.enabled) {
            dev.used_bytes = dev.used_bytes.saturating_sub(bytes);
        }
        Ok(())
    })
}

/// Record a swap-out event.
pub fn record_swap_out(bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        state.total_swap_out += 1;
        state.total_swap_out_bytes += bytes;
        // Increase usage on the first enabled device with space.
        if let Some(dev) = state.devices.iter_mut().find(|d| d.enabled && d.free_bytes() >= bytes) {
            dev.used_bytes += bytes;
        }
        Ok(())
    })
}

/// Update per-process swap usage.
pub fn update_process(pid: u32, name: &str, swap_bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        if let Some(proc) = state.processes.iter_mut().find(|p| p.pid == pid) {
            proc.swap_bytes = swap_bytes;
            if swap_bytes == 0 {
                state.processes.retain(|p| p.pid != pid);
            }
        } else if swap_bytes > 0 {
            if state.processes.len() >= MAX_PROCESSES {
                return Err(KernelError::ResourceExhausted);
            }
            state.processes.push(ProcessSwap {
                pid, name: String::from(name), swap_bytes,
            });
        }
        Ok(())
    })
}

/// Take a usage snapshot.
pub fn take_snapshot() -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let total: u64 = state.devices.iter().filter(|d| d.enabled).map(|d| d.total_bytes).sum();
        let used: u64 = state.devices.iter().filter(|d| d.enabled).map(|d| d.used_bytes).sum();
        if state.snapshots.len() >= MAX_HISTORY {
            state.snapshots.remove(0);
        }
        state.snapshots.push(SwapSnapshot {
            timestamp_ns: now, total_bytes: total, used_bytes: used,
            swap_in_rate: state.total_swap_in, swap_out_rate: state.total_swap_out,
        });
        Ok(())
    })
}

/// Get overall swap usage.
pub fn total_usage() -> (u64, u64) {
    STATE.lock().as_ref().map_or((0, 0), |s| {
        let total: u64 = s.devices.iter().filter(|d| d.enabled).map(|d| d.total_bytes).sum();
        let used: u64 = s.devices.iter().filter(|d| d.enabled).map(|d| d.used_bytes).sum();
        (total, used)
    })
}

/// List swap devices.
pub fn list_devices() -> Vec<SwapDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// List per-process swap, sorted by usage.
pub fn list_processes() -> Vec<ProcessSwap> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut procs = s.processes.clone();
        procs.sort_by(|a, b| b.swap_bytes.cmp(&a.swap_bytes));
        procs
    })
}

/// Get snapshots.
pub fn history() -> Vec<SwapSnapshot> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.snapshots.clone())
}

/// Statistics: (device_count, process_count, total_swap_in, total_swap_out, total_in_bytes, total_out_bytes, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.processes.len(), s.total_swap_in, s.total_swap_out,
                     s.total_swap_in_bytes, s.total_swap_out_bytes, s.ops),
        None => (0, 0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("swapmon::self_test() — running tests...");
    init_defaults();

    // 1: Default device.
    assert_eq!(list_devices().len(), 1);
    let (total, used) = total_usage();
    assert!(total > 0);
    assert!(used > 0);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Add device.
    let id = add_device("/swapfile", SwapType::File, 2_000_000_000, -2).expect("add");
    assert_eq!(list_devices().len(), 2);
    crate::serial_println!("  [2/8] add device: OK");

    // 3: Swap out.
    record_swap_out(4096).expect("swap_out");
    let dev = list_devices().iter().find(|d| d.id == 1).cloned().expect("dev1");
    assert!(dev.used_bytes > 524_288_000);
    crate::serial_println!("  [3/8] swap out: OK");

    // 4: Swap in.
    record_swap_in(4096).expect("swap_in");
    crate::serial_println!("  [4/8] swap in: OK");

    // 5: Per-process.
    update_process(100, "firefox", 50_000_000).expect("proc");
    update_process(200, "vscode", 30_000_000).expect("proc2");
    let procs = list_processes();
    assert_eq!(procs.len(), 2);
    assert_eq!(procs[0].pid, 100); // Sorted by usage, firefox first.
    crate::serial_println!("  [5/8] per-process: OK");

    // 6: Snapshot.
    take_snapshot().expect("snapshot");
    assert_eq!(history().len(), 1);
    crate::serial_println!("  [6/8] snapshot: OK");

    // 7: Disable/remove.
    set_enabled(id, false).expect("disable");
    let dev = list_devices().iter().find(|d| d.id == id).cloned().expect("dev2");
    assert!(!dev.enabled);
    remove_device(id).expect("remove");
    assert_eq!(list_devices().len(), 1);
    crate::serial_println!("  [7/8] disable/remove: OK");

    // 8: Stats.
    let (dev_count, proc_count, swap_in, swap_out, in_bytes, out_bytes, ops) = stats();
    assert_eq!(dev_count, 1);
    assert_eq!(proc_count, 2);
    assert!(swap_in >= 1);
    assert!(swap_out >= 1);
    assert!(in_bytes > 0);
    assert!(out_bytes > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("swapmon::self_test() — all 8 tests passed");
}
