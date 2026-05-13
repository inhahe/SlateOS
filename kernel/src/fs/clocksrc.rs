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

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        sources: alloc::vec![
            ClockSource { id: 1, name: String::from("tsc"), freq_hz: 3_000_000_000, rating: ClockRating::Ideal, is_current: true, reads: 1_000_000_000, skew_corrections: 100, total_skew_ns: 50_000, max_skew_ns: 1000, read_latency_ns: 20 },
            ClockSource { id: 2, name: String::from("hpet"), freq_hz: 14_318_180, rating: ClockRating::Good, is_current: false, reads: 500_000, skew_corrections: 50, total_skew_ns: 25_000, max_skew_ns: 500, read_latency_ns: 300 },
            ClockSource { id: 3, name: String::from("acpi_pm"), freq_hz: 3_579_545, rating: ClockRating::Medium, is_current: false, reads: 10_000, skew_corrections: 200, total_skew_ns: 500_000, max_skew_ns: 5000, read_latency_ns: 800 },
        ],
        next_id: 4,
        total_reads: 1_000_510_000,
        total_skew_corrections: 350,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(list().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    let id = register("test_clk", 1_000_000, ClockRating::Low).expect("register");
    assert!(id >= 4);
    assert_eq!(list().len(), 4);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Set current.
    set_current(id).expect("set_current");
    let cur = current().unwrap();
    assert_eq!(cur.id, id);
    crate::serial_println!("  [3/8] set current: OK");

    // 4: Record read.
    record_read(id, 50).expect("read");
    let s = list().iter().find(|s| s.id == id).cloned().unwrap();
    assert_eq!(s.reads, 1);
    assert_eq!(s.read_latency_ns, 50);
    crate::serial_println!("  [4/8] read: OK");

    // 5: Record skew.
    record_skew(id, 200).expect("skew");
    let s = list().iter().find(|s| s.id == id).cloned().unwrap();
    assert_eq!(s.skew_corrections, 1);
    assert_eq!(s.max_skew_ns, 200);
    crate::serial_println!("  [5/8] skew: OK");

    // 6: Rating ordering.
    assert!(ClockRating::Ideal > ClockRating::Good);
    assert!(ClockRating::Good > ClockRating::Medium);
    crate::serial_println!("  [6/8] rating order: OK");

    // 7: Not found.
    assert!(set_current(99).is_err());
    assert!(record_read(99, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (sources, reads, skews, ops) = stats();
    assert_eq!(sources, 4);
    assert!(reads > 1_000_000_000);
    assert!(skews > 350);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("clocksrc::self_test() — all 8 tests passed");
}
