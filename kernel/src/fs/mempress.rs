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

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise the memory-pressure (PSI) statistics state.
///
/// Starts with no observed pressure: level `None`, all stall/reclaim
/// times and event counters at zero, and OOM proximity 0. The
/// `/proc/pressure/memory`-style generator and the kshell view surface
/// this state as if it reflects real kernel memory-pressure activity, so
/// seeding it with invented stall times would be fabricated procfs data.
/// The counters are advanced only by real [`record_stall`],
/// [`record_reclaim`], [`update_level`], and [`set_oom_proximity`] calls
/// from the reclaim / OOM paths.
///
/// (Previously this seeded level `Low`, 5.5s total stall (5s some / 0.5s
/// full), 10M reclaim pages, 100k stall events, 50k reclaim events, OOM
/// proximity 15, and 5000 level changes — all fictional.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        level: PressureLevel::None,
        some_total_ns: 0,
        full_total_ns: 0,
        total_stall_ns: 0,
        total_reclaim_pages: 0,
        stall_events: 0,
        reclaim_events: 0,
        oom_proximity: 0,
        level_changes: 0,
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
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live pressure table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no observed pressure.
    let c = current();
    assert_eq!(c.level, PressureLevel::None);
    assert_eq!(c.total_stall_ns, 0);
    assert_eq!(c.total_reclaim_pages, 0);
    assert_eq!(c.oom_proximity, 0);
    let (stalls0, reclaims0, ns0, pages0, changes0, oom0, _) = stats();
    assert_eq!((stalls0, reclaims0, ns0, pages0, changes0, oom0), (0, 0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: "some" stall (not full) — counts a stall event and accrues time.
    record_stall(false, 1_000_000).expect("stall");
    let (stalls, _, stall_ns, _, _, _, _) = stats();
    assert_eq!(stalls, 1);
    assert_eq!(stall_ns, 1_000_000);
    crate::serial_println!("  [2/8] some stall: OK");

    // 3: Full stall accrues both some+full totals; total stall is exact sum.
    record_stall(true, 5_000_000).expect("full_stall");
    let (stalls, _, stall_ns, _, _, _, _) = stats();
    assert_eq!(stalls, 2);
    assert_eq!(stall_ns, 6_000_000);
    crate::serial_println!("  [3/8] full stall: OK");

    // 4: Reclaim accounting is exact.
    record_reclaim(1000).expect("reclaim");
    record_reclaim(500).expect("reclaim2");
    let (_, reclaims, _, pages, _, _, _) = stats();
    assert_eq!(reclaims, 2);
    assert_eq!(pages, 1500);
    crate::serial_println!("  [4/8] reclaim: OK");

    // 5: Level change updates current level and counts the transition.
    update_level(PressureLevel::High).expect("level");
    let c = current();
    assert_eq!(c.level, PressureLevel::High);
    let (_, _, _, _, changes, _, _) = stats();
    assert_eq!(changes, 1);
    crate::serial_println!("  [5/8] level: OK");

    // 6: Re-setting the same level does NOT count as a change.
    update_level(PressureLevel::High).expect("level same");
    let (_, _, _, _, changes, _, _) = stats();
    assert_eq!(changes, 1);
    crate::serial_println!("  [6/8] idempotent level: OK");

    // 7: OOM proximity clamps to 100.
    set_oom_proximity(75).expect("oom");
    assert_eq!(current().oom_proximity, 75);
    set_oom_proximity(250).expect("oom clamp");
    assert_eq!(current().oom_proximity, 100);
    crate::serial_println!("  [7/8] oom proximity: OK");

    // 8: Final stats reflect only the real activity above.
    update_level(PressureLevel::Critical).expect("critical");
    let (stalls, reclaims, stall_ns, pages, changes, oom, ops) = stats();
    assert_eq!(stalls, 2);
    assert_eq!(reclaims, 2);
    assert_eq!(stall_ns, 6_000_000);
    assert_eq!(pages, 1500);
    assert_eq!(changes, 2);
    assert_eq!(oom, 100);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("mempress::self_test() — all 8 tests passed");
}
