//! Page Fault Tracker — page fault statistics and analysis.
//!
//! Tracks page faults per process and system-wide, distinguishing
//! between minor (soft) and major (hard) faults. Records fault
//! addresses, fault reasons, and provides hotspot analysis.
//!
//! ## Architecture
//!
//! ```text
//! Page fault tracking
//!   → pftrack::record(pid, addr, kind) → record a fault
//!   → pftrack::get_process(pid) → per-process stats
//!   → pftrack::hotspots(n) → top N faulting addresses
//!   → pftrack::system_stats() → system-wide summary
//!
//! Integration:
//!   → procstat (process statistics)
//!   → perfmon (performance monitor)
//!   → memdiag (memory diagnostics)
//!   → oomkiller (OOM scoring)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Page fault kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    Minor,       // Page in memory, just not mapped (soft fault).
    Major,       // Page on disk, must be read (hard fault).
    Invalid,     // Access to unmapped memory.
    Protection,  // Permission violation.
    CopyOnWrite, // CoW fault.
}

impl FaultKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Minor => "minor",
            Self::Major => "major",
            Self::Invalid => "invalid",
            Self::Protection => "protection",
            Self::CopyOnWrite => "cow",
        }
    }
}

/// A recorded page fault event.
#[derive(Debug, Clone)]
pub struct FaultEvent {
    pub pid: u32,
    pub address: u64,
    pub kind: FaultKind,
    pub timestamp_ns: u64,
    pub instruction_ptr: u64,
}

/// Per-process fault statistics.
#[derive(Debug, Clone)]
pub struct ProcessFaults {
    pub pid: u32,
    pub name: String,
    pub minor: u64,
    pub major: u64,
    pub invalid: u64,
    pub protection: u64,
    pub cow: u64,
    pub total: u64,
}

/// Address hotspot.
#[derive(Debug, Clone)]
pub struct Hotspot {
    pub address: u64,
    pub count: u64,
    pub last_pid: u32,
    pub last_kind: FaultKind,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_EVENTS: usize = 4096;
const MAX_PROCESSES: usize = 256;
const MAX_HOTSPOTS: usize = 256;

struct State {
    events: Vec<FaultEvent>,
    processes: Vec<ProcessFaults>,
    hotspots: Vec<Hotspot>,
    total_minor: u64,
    total_major: u64,
    total_faults: u64,
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

/// Initialise an **empty** page-fault tracking table.
///
/// Seeds NO processes, NO events, NO hotspots, and zero totals.  Faults are
/// tracked through [`record`] as the page-fault handler services them; until
/// that wiring exists, `/proc/pftrack` and the `pftrack` kshell command report an
/// empty table rather than fabricated activity — the kernel's hard "never invent
/// data in procfs" rule.
///
/// (Previously this seeded three fabricated processes — `init` pid 1 with 120
/// minor / 5 major / 10 cow / 135 total, `sshd` pid 100 with 450/20/30/500, and
/// `browser` pid 200 with 15000/500/2/800/16302 — plus invented system totals
/// (total_minor 15570, total_major 525, total_faults 16937), which `/proc/pftrack`
/// and the `hotspots`/`top_faulters` views then displayed as if they were real
/// measured fault activity.  None of [`record`]'s callers are real — the page-fault
/// handler does not yet call it — so the table is entirely unwired; see the
/// DEFERRED PROPER FIX note in todo.txt for wiring `record` from the fault handler
/// and reading the system-wide aggregate from [`crate::mm::fault::fault_stats`].
/// The self-test now builds its own fixtures via the real API — see [`self_test`].)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        events: Vec::new(),
        processes: Vec::new(),
        hotspots: Vec::new(),
        total_minor: 0,
        total_major: 0,
        total_faults: 0,
        ops: 0,
    });
}

/// Record a page fault.
pub fn record(pid: u32, address: u64, kind: FaultKind, ip: u64) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        // Update process stats.
        if let Some(p) = state.processes.iter_mut().find(|p| p.pid == pid) {
            match kind {
                FaultKind::Minor => p.minor += 1,
                FaultKind::Major => p.major += 1,
                FaultKind::Invalid => p.invalid += 1,
                FaultKind::Protection => p.protection += 1,
                FaultKind::CopyOnWrite => p.cow += 1,
            }
            p.total += 1;
        } else if state.processes.len() < MAX_PROCESSES {
            let mut pf = ProcessFaults {
                pid, name: format!("pid_{}", pid),
                minor: 0, major: 0, invalid: 0, protection: 0, cow: 0, total: 1,
            };
            match kind {
                FaultKind::Minor => pf.minor = 1,
                FaultKind::Major => pf.major = 1,
                FaultKind::Invalid => pf.invalid = 1,
                FaultKind::Protection => pf.protection = 1,
                FaultKind::CopyOnWrite => pf.cow = 1,
            }
            state.processes.push(pf);
        }
        // Update hotspot.
        let page_addr = address & !0xFFF; // Page-align.
        if let Some(h) = state.hotspots.iter_mut().find(|h| h.address == page_addr) {
            h.count += 1;
            h.last_pid = pid;
            h.last_kind = kind;
        } else if state.hotspots.len() < MAX_HOTSPOTS {
            state.hotspots.push(Hotspot { address: page_addr, count: 1, last_pid: pid, last_kind: kind });
        }
        // Global counters.
        match kind {
            FaultKind::Minor | FaultKind::CopyOnWrite => state.total_minor += 1,
            FaultKind::Major => state.total_major += 1,
            _ => {}
        }
        state.total_faults += 1;
        // Event log.
        if state.events.len() >= MAX_EVENTS { state.events.remove(0); }
        state.events.push(FaultEvent { pid, address, kind, timestamp_ns: now, instruction_ptr: ip });
        Ok(())
    })
}

/// Get fault stats for a process.
pub fn get_process(pid: u32) -> Option<ProcessFaults> {
    STATE.lock().as_ref().and_then(|s| s.processes.iter().find(|p| p.pid == pid).cloned())
}

/// List all process fault stats.
pub fn list_processes() -> Vec<ProcessFaults> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.processes.clone())
}

/// Top N faulting processes by total faults.
pub fn top_faulters(n: usize) -> Vec<ProcessFaults> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut sorted = s.processes.clone();
        sorted.sort_by_key(|e| core::cmp::Reverse(e.total));
        sorted.truncate(n);
        sorted
    })
}

/// Top N hotspot addresses.
pub fn hotspots(n: usize) -> Vec<Hotspot> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut sorted = s.hotspots.clone();
        sorted.sort_by_key(|e| core::cmp::Reverse(e.count));
        sorted.truncate(n);
        sorted
    })
}

/// Recent fault events.
pub fn recent_events(n: usize) -> Vec<FaultEvent> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = if n >= s.events.len() { 0 } else { s.events.len() - n };
        s.events[start..].to_vec()
    })
}

/// Clear all tracking data.
pub fn clear() -> KernelResult<()> {
    with_state(|state| {
        state.events.clear();
        state.hotspots.clear();
        for p in &mut state.processes {
            p.minor = 0; p.major = 0; p.invalid = 0;
            p.protection = 0; p.cow = 0; p.total = 0;
        }
        state.total_minor = 0;
        state.total_major = 0;
        state.total_faults = 0;
        Ok(())
    })
}

/// Statistics: (process_count, event_count, total_faults, total_minor, total_major, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.processes.len(), s.events.len(), s.total_faults, s.total_minor, s.total_major, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("pftrack::self_test() — running tests...");
    // Start from a clean slate so the fixtures built below can never leak into
    // the live /proc/pftrack table (this self-test now runs at boot).
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no fabricated per-process fault records.
    assert_eq!(list_processes().len(), 0);
    let (p0, e0, t0, mi0, ma0, _o0) = stats();
    assert_eq!((p0, e0, t0, mi0, ma0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Record a minor fault — auto-creates the process with minor=1.
    record(1, 0x1000, FaultKind::Minor, 0x400000).expect("record");
    let p = get_process(1).expect("get");
    assert_eq!(p.minor, 1);
    assert_eq!(p.total, 1);
    crate::serial_println!("  [2/8] record minor: OK");

    // 3: New process auto-create on a major fault.
    record(999, 0x2000, FaultKind::Major, 0x500000).expect("record2");
    let p = get_process(999).expect("get2");
    assert_eq!(p.major, 1);
    assert_eq!(p.total, 1);
    crate::serial_println!("  [3/8] auto-create: OK");

    // 4: Hotspots — three faults on the same page rank it first.
    record(1, 0x1000, FaultKind::Minor, 0x400004).expect("record3");
    record(1, 0x1000, FaultKind::Minor, 0x400008).expect("record4");
    let hs = hotspots(5);
    assert!(!hs.is_empty());
    assert_eq!(hs[0].address, 0x1000);
    assert_eq!(hs[0].count, 3);
    crate::serial_println!("  [4/8] hotspots: OK");

    // 5: Top faulters — pid 1 now has 3 faults, ahead of pid 999's 1.
    let top = top_faulters(2);
    assert_eq!(top.len(), 2);
    assert_eq!(top[0].pid, 1);
    assert_eq!(top[0].total, 3);
    crate::serial_println!("  [5/8] top faulters: OK");

    // 6: Recent events — exactly the four faults recorded above.
    let events = recent_events(10);
    assert_eq!(events.len(), 4);
    crate::serial_println!("  [6/8] recent events: OK");

    // 7: Clear — per-process counters and the event log reset to zero.
    clear().expect("clear");
    let p = get_process(1).expect("get3");
    assert_eq!(p.total, 0);
    crate::serial_println!("  [7/8] clear: OK");

    // 8: Stats — events cleared, totals zero; the two process rows remain
    //    (counters zeroed); ops bumped by the record/clear writes above.
    let (procs, evs, total, minor, major, ops) = stats();
    assert_eq!(procs, 2);
    assert_eq!(evs, 0);
    assert_eq!(total, 0);
    assert_eq!(minor, 0);
    assert_eq!(major, 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Reset so the boot self-test leaves no fixtures behind in /proc/pftrack.
    *STATE.lock() = None;

    crate::serial_println!("pftrack::self_test() — all 8 tests passed");
}
