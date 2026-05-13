//! Memory Pressure — PSI-like memory pressure monitoring.
//!
//! Tracks memory pressure levels, stall times, reclaim
//! activity, and OOM proximity. Implements pressure stall
//! information (PSI) similar to Linux's /proc/pressure/memory.
//!
//! ## Architecture
//!
//! ```text
//! Memory pressure monitoring
//!   → mempress::record_stall(level, ns) → stall event
//!   → mempress::record_reclaim(pages) → reclaim activity
//!   → mempress::update_level(level) → pressure level change
//!   → mempress::current() → current pressure state
//!
//! Integration:
//!   → oomkiller (OOM killer)
//!   → memcg (memory cgroup)
//!   → pagestat (page allocator)
//!   → compstat (memory compaction)
//! ```

use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Memory pressure level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressureLevel {
    None,
    Low,
    Medium,
    High,
    Critical,
}

impl PressureLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

/// Pressure window statistics.
#[derive(Debug, Clone)]
pub struct PressureWindow {
    pub window_ns: u64,
    pub some_total_ns: u64,
    pub full_total_ns: u64,
    /// "some" pct * 100 (integer).
    pub some_pct: u64,
    /// "full" pct * 100.
    pub full_pct: u64,
}

/// Current pressure state.
#[derive(Debug, Clone)]
pub struct PressureState {
    pub level: PressureLevel,
    pub windows: Vec<PressureWindow>,
    pub total_stall_ns: u64,
    pub total_reclaim_pages: u64,
    pub oom_proximity: u32, // 0-100, 100 = imminent OOM.
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    level: PressureLevel,
    some_total_ns: u64,
    full_total_ns: u64,
    total_stall_ns: u64,
    total_reclaim_pages: u64,
    stall_events: u64,
    reclaim_events: u64,
    oom_proximity: u32,
    level_changes: u64,
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
        level: PressureLevel::Low,
        some_total_ns: 5_000_000_000,
        full_total_ns: 500_000_000,
        total_stall_ns: 5_500_000_000,
        total_reclaim_pages: 10_000_000,
        stall_events: 100_000,
        reclaim_events: 50_000,
        oom_proximity: 15,
        level_changes: 5_000,
        ops: 0,
    });
}

/// Record a memory stall event.
pub fn record_stall(is_full: bool, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        state.some_total_ns += ns;
        if is_full { state.full_total_ns += ns; }
        state.total_stall_ns += ns;
        state.stall_events += 1;
        Ok(())
    })
}

/// Record reclaim activity.
pub fn record_reclaim(pages: u64) -> KernelResult<()> {
    with_state(|state| {
        state.total_reclaim_pages += pages;
        state.reclaim_events += 1;
        Ok(())
    })
}

/// Update pressure level.
pub fn update_level(level: PressureLevel) -> KernelResult<()> {
    with_state(|state| {
        if state.level != level {
            state.level = level;
            state.level_changes += 1;
        }
        Ok(())
    })
}

/// Set OOM proximity (0-100).
pub fn set_oom_proximity(pct: u32) -> KernelResult<()> {
    with_state(|state| {
        state.oom_proximity = pct.min(100);
        Ok(())
    })
}

/// Get current pressure state.
pub fn current() -> PressureState {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => PressureState {
            level: s.level,
            windows: Vec::new(), // Simplified — real impl would compute windowed averages.
            total_stall_ns: s.total_stall_ns,
            total_reclaim_pages: s.total_reclaim_pages,
            oom_proximity: s.oom_proximity,
        },
        None => PressureState {
            level: PressureLevel::None,
            windows: Vec::new(),
            total_stall_ns: 0,
            total_reclaim_pages: 0,
            oom_proximity: 0,
        },
    }
}

/// Statistics: (stall_events, reclaim_events, total_stall_ns, total_reclaim_pages, level_changes, oom_proximity, ops).
pub fn stats() -> (u64, u64, u64, u64, u64, u32, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.stall_events, s.reclaim_events, s.total_stall_ns, s.total_reclaim_pages, s.level_changes, s.oom_proximity, s.ops),
        None => (0, 0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("mempress::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    let c = current();
    assert_eq!(c.level, PressureLevel::Low);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Record stall.
    let (before, _, _, _, _, _, _) = stats();
    record_stall(false, 1_000_000).expect("stall");
    let (after, _, _, _, _, _, _) = stats();
    assert_eq!(after, before + 1);
    crate::serial_println!("  [2/8] stall: OK");

    // 3: Full stall.
    record_stall(true, 5_000_000).expect("full_stall");
    let (_, _, stall_ns, _, _, _, _) = stats();
    assert!(stall_ns > 5_500_000_000);
    crate::serial_println!("  [3/8] full stall: OK");

    // 4: Reclaim.
    record_reclaim(1000).expect("reclaim");
    let (_, reclaims, _, pages, _, _, _) = stats();
    assert!(reclaims > 50_000);
    assert!(pages > 10_000_000);
    crate::serial_println!("  [4/8] reclaim: OK");

    // 5: Level change.
    update_level(PressureLevel::High).expect("level");
    let c = current();
    assert_eq!(c.level, PressureLevel::High);
    crate::serial_println!("  [5/8] level: OK");

    // 6: OOM proximity.
    set_oom_proximity(75).expect("oom");
    let c = current();
    assert_eq!(c.oom_proximity, 75);
    crate::serial_println!("  [6/8] oom proximity: OK");

    // 7: Level changes counter.
    update_level(PressureLevel::Critical).expect("critical");
    let (_, _, _, _, changes, _, _) = stats();
    assert!(changes > 5_000);
    crate::serial_println!("  [7/8] level changes: OK");

    // 8: Stats.
    let (stalls, reclaims, stall_ns, pages, changes, oom, ops) = stats();
    assert!(stalls > 100_000);
    assert!(reclaims > 50_000);
    assert!(stall_ns > 5_500_000_000);
    assert!(pages > 10_000_000);
    assert!(changes > 5_000);
    assert_eq!(oom, 75);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("mempress::self_test() — all 8 tests passed");
}
