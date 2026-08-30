//! Swap Monitor — swap space usage reporting.
//!
//! Reports swap device/file usage and swap-in activity by reading the
//! **real** swap subsystem (`crate::mm::swap`) and the page-fault counters
//! (`crate::mm::fault`).  It does NOT maintain its own swap state — there is
//! exactly one source of truth for swap (`mm::swap`), and `/proc/swapmon` plus
//! the `swapmon` kshell command are thin read-through views over it.
//!
//! ## Architecture
//!
//! ```text
//! Swap reporting (read-through; no local state)
//!   → swapmon::total_usage()  → mm::swap::summary()        (total/used bytes)
//!   → swapmon::list_devices() → mm::swap::list_devices()   (real devices)
//!   → swapmon::stats()        → mm::swap::device_count()
//!                               + mm::fault::fault_stats().swap_in
//!
//! Integration:
//!   → mm::swap   (the real swap subsystem — slots, devices, capacity)
//!   → mm::fault  (swap-in page-fault counter)
//!   → swapcfg    (swap configuration)
//!   → memdiag    (memory diagnostics)
//! ```
//!
//! ## Why this is a read-through, not a tracker
//!
//! This module previously kept its own `State` with a fabricated default swap
//! device (a fictional 4 GiB `/dev/sda2` partition seeded ~500 MiB used) plus
//! invented swap-in/out rate counters, and `/proc/swapmon` and the `swapmon`
//! kshell command displayed those phantom numbers as if they were real swap
//! usage — a violation of the kernel's hard "never invent data in procfs" rule.
//! None of its mutation APIs (`add_device`, `record_swap_in/out`,
//! `update_process`, `take_snapshot`, …) had a single real caller; they only
//! existed to be exercised by the self-test against fabricated fixtures.
//!
//! Meanwhile the kernel already has a real swap subsystem (`crate::mm::swap`,
//! which also backs `/proc/swaps`).  The proper fix is therefore to delete the
//! parallel fabricated store entirely and report the real subsystem's state.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Bytes per swap slot (1 slot = 1 frame = 16 KiB, see `mm::frame::FRAME_SIZE`).
const SLOT_BYTES: u64 = 16 * 1024;

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

/// A swap device/file entry (a read-through view of `mm::swap`).
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
        if self.total_bytes == 0 {
            0
        } else {
            // saturating: total_bytes != 0 here, and the ratio is <= 100.
            (self.used_bytes.saturating_mul(100) / self.total_bytes) as u32
        }
    }
}

/// Per-process swap usage.
///
/// Retained for API/type stability.  There is no per-process swap-residency
/// accounting in the pager yet, so [`list_processes`] always reports an empty
/// list rather than fabricating per-process figures.
#[derive(Debug, Clone)]
pub struct ProcessSwap {
    pub pid: u32,
    pub name: String,
    pub swap_bytes: u64,
}

/// Swap usage snapshot.
///
/// Retained for API/type stability.  No historical sampler is wired, so
/// [`history`] always reports an empty list rather than fabricating samples.
#[derive(Debug, Clone)]
pub struct SwapSnapshot {
    pub timestamp_ns: u64,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub swap_in_rate: u64,
    pub swap_out_rate: u64,
}

// ---------------------------------------------------------------------------
// Public API (read-through over the real swap subsystem)
// ---------------------------------------------------------------------------

/// No-op: this module holds no state of its own.
///
/// Kept so existing call sites (the `swapmon` kshell command) compile
/// unchanged; all reporting reads `crate::mm::swap` live.
pub fn init_defaults() {}

/// Overall swap usage `(total_bytes, used_bytes)` from the real subsystem.
pub fn total_usage() -> (u64, u64) {
    let (total, used, _devices) = crate::mm::swap::summary();
    (total as u64, used as u64)
}

/// List swap devices, mapping the real `mm::swap` devices into the reporting
/// view.  IDs are 1-based positional indices (the real subsystem keys devices
/// by name, not numeric id).
pub fn list_devices() -> Vec<SwapDevice> {
    crate::mm::swap::list_devices()
        .into_iter()
        .enumerate()
        .map(|(i, info)| {
            let swap_type = match info.device_type {
                "memory" => SwapType::Zram,
                // "disk" and any future backend → Partition (closest match for
                // a block-backed swap area).
                _ => SwapType::Partition,
            };
            SwapDevice {
                id: (i as u32).saturating_add(1),
                path: info.name,
                swap_type,
                total_bytes: (info.total_slots as u64).saturating_mul(SLOT_BYTES),
                used_bytes: (info.used_slots as u64).saturating_mul(SLOT_BYTES),
                priority: info.priority,
                // mm::swap only tracks active devices; an entry's presence means
                // it is in service.
                enabled: true,
            }
        })
        .collect()
}

/// Per-process swap usage — empty until the pager tracks swap residency per
/// address space (no fabrication).
pub fn list_processes() -> Vec<ProcessSwap> {
    Vec::new()
}

/// Historical usage snapshots — empty until a sampler is wired (no fabrication).
pub fn history() -> Vec<SwapSnapshot> {
    Vec::new()
}

/// Statistics:
/// `(device_count, process_count, total_swap_in, total_swap_out,
///   total_in_bytes, total_out_bytes, ops)`.
///
/// `total_swap_in` is the real count of swap-in page faults
/// (`mm::fault::fault_stats().swap_in`); `total_in_bytes` is derived from it
/// (each swap-in restores exactly one 16 KiB page).  `process_count`,
/// `total_swap_out`, `total_out_bytes`, and `ops` are reported as `0` because
/// the real swap subsystem does not yet track those — honest zeros rather than
/// invented figures.
pub fn stats() -> (usize, usize, u64, u64, u64, u64, u64) {
    let device_count = crate::mm::swap::device_count();
    let swap_in = crate::mm::fault::fault_stats().swap_in;
    let in_bytes = swap_in.saturating_mul(SLOT_BYTES);
    (device_count, 0, swap_in, 0, in_bytes, 0, 0)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("swapmon::self_test() — running tests...");

    // This module is a pure read-through with no state of its own, so there is
    // nothing to seed and nothing to leak.  The tests assert that the reporting
    // views are exactly consistent with the real swap subsystem rather than
    // checking fabricated fixtures.

    // 1: device list length matches the real device count.
    let devs = list_devices();
    let real_count = crate::mm::swap::device_count();
    assert_eq!(devs.len(), real_count);
    crate::serial_println!("  [1/5] device list matches mm::swap: OK");

    // 2: total_usage matches mm::swap::summary().
    let (total, used) = total_usage();
    let (real_total, real_used, _d) = crate::mm::swap::summary();
    assert_eq!(total, real_total as u64);
    assert_eq!(used, real_used as u64);
    assert!(used <= total);
    crate::serial_println!("  [2/5] total_usage matches summary: OK");

    // 3: each reported device is internally consistent (used <= total, pct sane).
    for d in &devs {
        assert!(d.used_bytes <= d.total_bytes);
        assert!(d.usage_pct() <= 100);
        assert_eq!(d.free_bytes(), d.total_bytes.saturating_sub(d.used_bytes));
    }
    crate::serial_println!("  [3/5] per-device consistency: OK");

    // 4: no fabricated per-process or historical data.
    assert!(list_processes().is_empty());
    assert!(history().is_empty());
    crate::serial_println!("  [4/5] no fabricated process/history rows: OK");

    // 5: stats are consistent with the real sources.
    let (dev_count, proc_count, swap_in, swap_out, in_bytes, out_bytes, ops) = stats();
    assert_eq!(dev_count, real_count);
    assert_eq!(proc_count, 0);
    assert_eq!(swap_in, crate::mm::fault::fault_stats().swap_in);
    assert_eq!(in_bytes, swap_in.saturating_mul(SLOT_BYTES));
    assert_eq!((swap_out, out_bytes, ops), (0, 0, 0));
    crate::serial_println!("  [5/5] stats match mm::swap + mm::fault: OK");

    crate::serial_println!("swapmon::self_test() — all 5 tests passed");
}
