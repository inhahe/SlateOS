//! I/O priority management for filesystem operations.
//!
//! Provides per-task I/O priority classification and scheduling hints.
//! Used by the VFS layer and block device drivers to order requests
//! according to the design spec's I/O priority model:
//!
//! - **Realtime**: Audio/video playback, latency-critical streams.
//!   Requires `CAP_SYS_IO_REALTIME` capability.
//! - **BestEffort**: Normal application I/O, with 8 sub-priority
//!   levels (0 = highest, 7 = lowest).
//! - **Idle**: Background tasks — indexing, backup, dedup scans.
//!   Only serviced when no higher-priority I/O is pending.
//!
//! ## Architecture
//!
//! ```text
//! Application → set_ioprio(task_id, class, level)
//!                 ↓
//! VFS operation → get_current_ioprio()
//!                 ↓
//! Block layer uses priority to schedule request order
//! ```
//!
//! ## Design Decisions
//!
//! - Default priority: BestEffort level 4 (middle).
//! - Realtime requires a capability (prevents DoS by unprivileged tasks).
//! - Idle class uses BFQ-inspired "only when idle" semantics.
//! - Priority is inherited by child tasks at creation time.
//! - Per-task, not per-file (matches Linux ionice model).

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// I/O scheduling class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum IoClass {
    /// Highest priority — latency-critical streams (audio/video).
    /// Requires CAP_SYS_IO_REALTIME.
    Realtime = 0,
    /// Normal application I/O with sub-priority levels 0-7.
    BestEffort = 1,
    /// Background I/O — only serviced when no other I/O pending.
    Idle = 2,
}

impl IoClass {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Realtime => "realtime",
            Self::BestEffort => "best-effort",
            Self::Idle => "idle",
        }
    }

    /// Parse from string.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "realtime" | "rt" | "0" => Some(Self::Realtime),
            "best-effort" | "besteffort" | "be" | "normal" | "1" => Some(Self::BestEffort),
            "idle" | "bg" | "background" | "2" => Some(Self::Idle),
            _ => None,
        }
    }
}

/// Complete I/O priority specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoPriority {
    /// Scheduling class.
    pub class: IoClass,
    /// Sub-priority level within the class (0-7, lower = higher priority).
    /// Only meaningful for BestEffort; ignored for Realtime and Idle.
    pub level: u8,
}

impl IoPriority {
    /// Default priority: BestEffort level 4.
    pub const DEFAULT: Self = Self { class: IoClass::BestEffort, level: 4 };

    /// Highest best-effort priority.
    pub const HIGH: Self = Self { class: IoClass::BestEffort, level: 0 };

    /// Lowest best-effort priority.
    pub const LOW: Self = Self { class: IoClass::BestEffort, level: 7 };

    /// Realtime priority.
    pub const REALTIME: Self = Self { class: IoClass::Realtime, level: 0 };

    /// Idle (background) priority.
    pub const IDLE: Self = Self { class: IoClass::Idle, level: 0 };

    /// Create a new I/O priority.
    pub fn new(class: IoClass, level: u8) -> Self {
        Self { class, level: level.min(7) }
    }

    /// Pack into a u16 for compact storage.
    /// Format: [class:2][level:3][reserved:11]
    pub fn pack(self) -> u16 {
        ((self.class as u16) << 14) | ((self.level as u16 & 0x7) << 11)
    }

    /// Unpack from u16.
    pub fn unpack(val: u16) -> Self {
        let class_bits = (val >> 14) & 0x3;
        let level = ((val >> 11) & 0x7) as u8;
        let class = match class_bits {
            0 => IoClass::Realtime,
            1 => IoClass::BestEffort,
            _ => IoClass::Idle,
        };
        Self { class, level }
    }

    /// Comparison for scheduling: returns true if self has higher priority.
    pub fn is_higher_than(self, other: Self) -> bool {
        if self.class != other.class {
            return (self.class as u8) < (other.class as u8);
        }
        self.level < other.level
    }

    /// Human-readable display string.
    pub fn display(&self) -> String {
        use alloc::format;
        match self.class {
            IoClass::Realtime => String::from("realtime"),
            IoClass::BestEffort => format!("best-effort:{}", self.level),
            IoClass::Idle => String::from("idle"),
        }
    }
}

// ---------------------------------------------------------------------------
// Per-task I/O priority table
// ---------------------------------------------------------------------------

/// Maximum number of tracked tasks.
const MAX_TASKS: usize = 256;

/// Per-task I/O priority entries.
/// Format: [task_id:48][packed_prio:16] in each AtomicU64.
/// Entry is "free" when the entire u64 == 0 (task_id 0 is never valid).
static TASK_PRIO: [AtomicU64; MAX_TASKS] = {
    const INIT: AtomicU64 = AtomicU64::new(0);
    [INIT; MAX_TASKS]
};

/// Encode task_id + priority into a single u64.
fn encode_entry(task_id: u64, prio: IoPriority) -> u64 {
    (task_id << 16) | (prio.pack() as u64)
}

/// Decode a u64 entry into (task_id, priority).
fn decode_entry(val: u64) -> (u64, IoPriority) {
    let task_id = val >> 16;
    let prio = IoPriority::unpack((val & 0xFFFF) as u16);
    (task_id, prio)
}

// ---------------------------------------------------------------------------
// Counters
// ---------------------------------------------------------------------------

static SET_COUNT: AtomicU64 = AtomicU64::new(0);
static GET_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Set I/O priority for a task.
///
/// Realtime class requires appropriate capability (not enforced here —
/// the syscall layer must check before calling).
pub fn set_ioprio(task_id: u64, prio: IoPriority) -> Result<(), &'static str> {
    if task_id == 0 {
        return Err("invalid task ID");
    }

    let encoded = encode_entry(task_id, prio);

    // Look for existing entry or free slot.
    let mut free_slot: Option<usize> = None;
    for i in 0..MAX_TASKS {
        let val = TASK_PRIO[i].load(Ordering::Relaxed);
        if val == 0 && free_slot.is_none() {
            free_slot = Some(i);
        } else if val != 0 {
            let (tid, _) = decode_entry(val);
            if tid == task_id {
                // Update existing entry.
                TASK_PRIO[i].store(encoded, Ordering::Relaxed);
                SET_COUNT.fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
        }
    }

    // Insert into free slot.
    if let Some(slot) = free_slot {
        TASK_PRIO[slot].store(encoded, Ordering::Relaxed);
        SET_COUNT.fetch_add(1, Ordering::Relaxed);
        Ok(())
    } else {
        Err("I/O priority table full")
    }
}

/// Get I/O priority for a task. Returns DEFAULT if not explicitly set.
pub fn get_ioprio(task_id: u64) -> IoPriority {
    GET_COUNT.fetch_add(1, Ordering::Relaxed);
    for i in 0..MAX_TASKS {
        let val = TASK_PRIO[i].load(Ordering::Relaxed);
        if val != 0 {
            let (tid, prio) = decode_entry(val);
            if tid == task_id {
                return prio;
            }
        }
    }
    IoPriority::DEFAULT
}

/// Remove I/O priority entry for a task (resets to default).
pub fn clear_ioprio(task_id: u64) {
    for i in 0..MAX_TASKS {
        let val = TASK_PRIO[i].load(Ordering::Relaxed);
        if val != 0 {
            let (tid, _) = decode_entry(val);
            if tid == task_id {
                TASK_PRIO[i].store(0, Ordering::Relaxed);
                return;
            }
        }
    }
}

/// Get I/O priority for the current running task.
pub fn current_ioprio() -> IoPriority {
    let task_id = crate::sched::current_task_id();
    get_ioprio(task_id)
}

/// List all explicitly-set I/O priorities.
pub fn list_all() -> Vec<(u64, IoPriority)> {
    let mut result = Vec::new();
    for i in 0..MAX_TASKS {
        let val = TASK_PRIO[i].load(Ordering::Relaxed);
        if val != 0 {
            let (tid, prio) = decode_entry(val);
            result.push((tid, prio));
        }
    }
    result
}

/// Quick summary stats.
pub fn stats() -> (u64, u64, usize) {
    let active = TASK_PRIO.iter()
        .filter(|a| a.load(Ordering::Relaxed) != 0)
        .count();
    (
        SET_COUNT.load(Ordering::Relaxed),
        GET_COUNT.load(Ordering::Relaxed),
        active,
    )
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> crate::error::KernelResult<()> {
    serial_println!("[ioprio] Running self-test...");

    test_priority_types();
    test_pack_unpack();
    test_set_get();
    test_clear();
    test_comparison();
    test_list();

    serial_println!("[ioprio] Self-test passed (6 tests).");
    Ok(())
}

fn test_priority_types() {
    assert_eq!(IoClass::Realtime.label(), "realtime");
    assert_eq!(IoClass::BestEffort.label(), "best-effort");
    assert_eq!(IoClass::Idle.label(), "idle");
    assert_eq!(IoClass::from_name("rt"), Some(IoClass::Realtime));
    assert_eq!(IoClass::from_name("be"), Some(IoClass::BestEffort));
    assert_eq!(IoClass::from_name("idle"), Some(IoClass::Idle));
    assert_eq!(IoClass::from_name("bogus"), None);
    serial_println!("[ioprio]   priority_types: ok");
}

fn test_pack_unpack() {
    let prio = IoPriority::new(IoClass::BestEffort, 3);
    let packed = prio.pack();
    let unpacked = IoPriority::unpack(packed);
    assert_eq!(unpacked.class, IoClass::BestEffort);
    assert_eq!(unpacked.level, 3);

    let rt = IoPriority::REALTIME;
    assert_eq!(IoPriority::unpack(rt.pack()).class, IoClass::Realtime);

    let idle = IoPriority::IDLE;
    assert_eq!(IoPriority::unpack(idle.pack()).class, IoClass::Idle);

    serial_println!("[ioprio]   pack_unpack: ok");
}

fn test_set_get() {
    let task_id = 99999; // Use a high task ID unlikely to conflict.
    let prio = IoPriority::new(IoClass::BestEffort, 2);

    // Default before setting.
    let default = get_ioprio(task_id);
    assert_eq!(default.class, IoClass::BestEffort);
    assert_eq!(default.level, 4);

    // Set and verify.
    assert!(set_ioprio(task_id, prio).is_ok());
    let got = get_ioprio(task_id);
    assert_eq!(got.class, IoClass::BestEffort);
    assert_eq!(got.level, 2);

    // Update.
    let new_prio = IoPriority::IDLE;
    assert!(set_ioprio(task_id, new_prio).is_ok());
    let got2 = get_ioprio(task_id);
    assert_eq!(got2.class, IoClass::Idle);

    // Cleanup.
    clear_ioprio(task_id);
    serial_println!("[ioprio]   set_get: ok");
}

fn test_clear() {
    let task_id = 99998;
    let prio = IoPriority::new(IoClass::Realtime, 0);

    assert!(set_ioprio(task_id, prio).is_ok());
    assert_eq!(get_ioprio(task_id).class, IoClass::Realtime);

    clear_ioprio(task_id);
    let default = get_ioprio(task_id);
    assert_eq!(default.class, IoClass::BestEffort);
    assert_eq!(default.level, 4);

    serial_println!("[ioprio]   clear: ok");
}

fn test_comparison() {
    let rt = IoPriority::REALTIME;
    let be_high = IoPriority::HIGH;
    let be_low = IoPriority::LOW;
    let idle = IoPriority::IDLE;

    assert!(rt.is_higher_than(be_high));
    assert!(be_high.is_higher_than(be_low));
    assert!(be_low.is_higher_than(idle));
    assert!(!idle.is_higher_than(rt));
    assert!(!be_low.is_higher_than(be_high));

    serial_println!("[ioprio]   comparison: ok");
}

fn test_list() {
    let task_a = 99997u64;
    let task_b = 99996u64;

    assert!(set_ioprio(task_a, IoPriority::HIGH).is_ok());
    assert!(set_ioprio(task_b, IoPriority::IDLE).is_ok());

    let all = list_all();
    let found_a = all.iter().any(|(t, p)| *t == task_a && p.class == IoClass::BestEffort && p.level == 0);
    let found_b = all.iter().any(|(t, p)| *t == task_b && p.class == IoClass::Idle);
    assert!(found_a);
    assert!(found_b);

    // Cleanup.
    clear_ioprio(task_a);
    clear_ioprio(task_b);

    serial_println!("[ioprio]   list: ok");
}
