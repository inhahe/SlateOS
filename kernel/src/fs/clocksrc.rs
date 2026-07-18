//! Clock Source Statistics — system clock source monitoring.
//!
//! Tracks clock source quality, skew corrections, frequency
//! adjustments, and read latency. Essential for timekeeping
//! accuracy diagnostics.
//!
//! ## Architecture
//!
//! ```text
//! Clock source monitoring
//!   → clocksrc::register(name, freq, rating) → add clock source
//!   → clocksrc::record_read(id) → track clock read
//!   → clocksrc::record_skew(id, ns) → track skew correction
//!   → clocksrc::list() → list all clock sources
//!
//! Integration:
//!   → timesync (time synchronization)
//!   → timerq (timer queue)
//!   → hpet (hardware timer)
//!   → cpustat (CPU utilization)
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

/// Clock source quality rating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClockRating {
    Unusable,   // 0
    Low,        // 100
    Medium,     // 200
    Good,       // 300
    Ideal,      // 400
}

impl ClockRating {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unusable => "unusable",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::Good => "good",
            Self::Ideal => "ideal",
        }
    }
    pub fn value(self) -> u32 {
        match self {
            Self::Unusable => 0,
            Self::Low => 100,
            Self::Medium => 200,
            Self::Good => 300,
            Self::Ideal => 400,
        }
    }
}

/// Clock source info.
#[derive(Debug, Clone)]
pub struct ClockSource {
    pub id: u32,
    pub name: String,
    pub freq_hz: u64,
    pub rating: ClockRating,
    pub is_current: bool,
    pub reads: u64,
    pub skew_corrections: u64,
    pub total_skew_ns: u64,
    pub max_skew_ns: u64,
    pub read_latency_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SOURCES: usize = 16;

struct State {
    sources: Vec<ClockSource>,
    next_id: u32,
    total_reads: u64,
    total_skew_corrections: u64,
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

/// Initialise the clock-source statistics state.
///
/// Starts with no clock sources and zero read/skew totals. Clock sources are
/// discovered hardware (TSC, HPET, ACPI PM timer, …) that the timekeeping
/// subsystem registers through [`register`] once it has probed and calibrated
/// them; the per-source read and skew counters advance only through real
/// [`record_read`] / [`record_skew`] calls. The `/proc/clocksrc` generator and
/// the `clocksrc` kshell command surface this list (and [`list`] / [`current`])
/// as if it reflects the real set of calibrated clock sources, so seeding it
/// with phantom sources would be fabricated procfs data — it would claim
/// timekeeping hardware is registered and has been read when nothing actually
/// programmed it.
///
/// (Previously this seeded three fictional sources — "tsc" (3GHz, Ideal,
/// current, 1B reads, 100 skew corrections), "hpet" (14.3MHz, Good, 500K reads,
/// 50 skews) and "acpi_pm" (3.58MHz, Medium, 10K reads, 200 skews) — plus
/// totals of 1,000,510,000 reads and 350 skew corrections.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        sources: Vec::new(),
        next_id: 1,
        total_reads: 0,
        total_skew_corrections: 0,
        ops: 0,
    });
}

/// Register a clock source.
pub fn register(name: &str, freq_hz: u64, rating: ClockRating) -> KernelResult<u32> {
    with_state(|state| {
        if state.sources.len() >= MAX_SOURCES { return Err(KernelError::ResourceExhausted); }
        let id = state.next_id;
        state.next_id += 1;
        state.sources.push(ClockSource {
            id, name: String::from(name), freq_hz, rating, is_current: false,
            reads: 0, skew_corrections: 0, total_skew_ns: 0, max_skew_ns: 0, read_latency_ns: 0,
        });
        Ok(id)
    })
}

/// Set current clock source.
pub fn set_current(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.sources.iter().any(|s| s.id == id) { return Err(KernelError::NotFound); }
        for s in &mut state.sources { s.is_current = s.id == id; }
        Ok(())
    })
}

/// Record a clock read.
pub fn record_read(id: u32, latency_ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let s = state.sources.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        s.reads += 1;
        s.read_latency_ns = latency_ns; // Latest latency
        state.total_reads += 1;
        Ok(())
    })
}

/// Record a skew correction.
pub fn record_skew(id: u32, skew_ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let s = state.sources.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        s.skew_corrections += 1;
        s.total_skew_ns += skew_ns;
        if skew_ns > s.max_skew_ns { s.max_skew_ns = skew_ns; }
        state.total_skew_corrections += 1;
        Ok(())
    })
}

/// List all clock sources.
pub fn list() -> Vec<ClockSource> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.sources.clone())
}

/// Get the current clock source.
pub fn current() -> Option<ClockSource> {
    STATE.lock().as_ref().and_then(|s| {
        s.sources.iter().find(|src| src.is_current).cloned()
    })
}

/// Statistics: (source_count, total_reads, total_skew_corrections, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.sources.len(), s.total_reads, s.total_skew_corrections, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("clocksrc::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live clock-source list afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no phantom sources, zero totals.
    assert_eq!(list().len(), 0);
    let (c0, r0, s0, _) = stats();
    assert_eq!((c0, r0, s0), (0, 0, 0));
    assert!(current().is_none());
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Register — source appears with monotonic id and zeroed counters.
    let id = register("test_clk", 1_000_000, ClockRating::Low).expect("register");
    assert_eq!(id, 1);
    assert_eq!(list().len(), 1);
    let s = list().into_iter().find(|s| s.id == id).expect("find");
    assert_eq!((s.reads, s.skew_corrections, s.total_skew_ns, s.max_skew_ns), (0, 0, 0, 0));
    assert!(!s.is_current);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Set current — the registered source becomes the active one.
    set_current(id).expect("set_current");
    let cur = current().expect("current");
    assert_eq!(cur.id, id);
    crate::serial_println!("  [3/8] set current: OK");

    // 4: Record read — per-source reads + latest latency and global total advance.
    record_read(id, 50).expect("read");
    let s = list().into_iter().find(|s| s.id == id).expect("p4");
    assert_eq!((s.reads, s.read_latency_ns), (1, 50));
    assert_eq!(stats().1, 1); // total_reads
    crate::serial_println!("  [4/8] read: OK");

    // 5: Record skew — corrections, total and max-skew accrue; global total too.
    record_skew(id, 200).expect("skew");
    record_skew(id, 80).expect("skew2");
    let s = list().into_iter().find(|s| s.id == id).expect("p5");
    assert_eq!((s.skew_corrections, s.total_skew_ns, s.max_skew_ns), (2, 280, 200));
    assert_eq!(stats().2, 2); // total_skew_corrections
    crate::serial_println!("  [5/8] skew: OK");

    // 6: Rating ordering holds across the quality ladder.
    assert!(ClockRating::Ideal > ClockRating::Good);
    assert!(ClockRating::Good > ClockRating::Medium);
    assert!(ClockRating::Medium > ClockRating::Low);
    crate::serial_println!("  [6/8] rating order: OK");

    // 7: Not found — set_current/record_read/record_skew on unknown id all error.
    assert!(set_current(99).is_err());
    assert!(record_read(99, 0).is_err());
    assert!(record_skew(99, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Final stats reflect only the real activity above: 1 source, 1 read,
    //    2 skew corrections.
    let (sources, reads, skews, ops) = stats();
    assert_eq!((sources, reads, skews), (1, 1, 2));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("clocksrc::self_test() — all 8 tests passed");
}
