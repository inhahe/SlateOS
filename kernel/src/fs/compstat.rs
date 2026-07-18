//! Compaction Statistics — memory compaction and defragmentation monitoring.
//!
//! Tracks memory compaction attempts, page migrations, scan
//! activity, and stall events. Essential for tuning huge page
//! assembly and diagnosing memory fragmentation.
//!
//! ## Architecture
//!
//! ```text
//! Compaction statistics
//!   → compstat::start_compaction(zone) → begin compaction run
//!   → compstat::finish_compaction(zone, ok) → end compaction
//!   → compstat::record_migration(pages) → track page moves
//!   → compstat::record_stall(pid) → track user stall
//!
//! Integration:
//!   → pagestat (page statistics)
//!   → numastat (NUMA statistics)
//!   → memcg (memory cgroup)
//!   → slabstat (slab allocator stats)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Compaction zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactZone {
    Dma,
    Dma32,
    Normal,
    HighMem,
    Movable,
}

impl CompactZone {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dma => "DMA",
            Self::Dma32 => "DMA32",
            Self::Normal => "Normal",
            Self::HighMem => "HighMem",
            Self::Movable => "Movable",
        }
    }
}

/// Compaction result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactResult {
    Success,
    Skipped,
    Failed,
    Deferred,
}

impl CompactResult {
    pub fn label(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
            Self::Deferred => "deferred",
        }
    }
}

/// Per-zone compaction stats.
#[derive(Debug, Clone)]
pub struct ZoneCompactStats {
    pub zone: CompactZone,
    pub attempts: u64,
    pub successes: u64,
    pub failures: u64,
    pub deferred: u64,
    pub pages_scanned_free: u64,
    pub pages_scanned_migrate: u64,
    pub pages_migrated: u64,
    pub pages_failed: u64,
    pub stalls: u64,
}

/// A compaction event.
#[derive(Debug, Clone)]
pub struct CompactionEvent {
    pub zone: CompactZone,
    pub result: CompactResult,
    pub pages_migrated: u64,
    pub pages_scanned: u64,
    pub duration_ns: u64,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_EVENTS: usize = 256;

struct State {
    zones: Vec<ZoneCompactStats>,
    events: Vec<CompactionEvent>,
    active_zone: Option<(CompactZone, u64)>, // (zone, start_ns).
    total_attempts: u64,
    total_migrations: u64,
    total_stalls: u64,
    total_stall_ns: u64,
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

/// Initialise an **empty** compaction statistics table.
///
/// Seeds NO zones, NO events, and zero totals.  Real compaction accounting is
/// wired through [`register_zone`] (one zeroed row per memory zone the page
/// allocator brings online) and the `start_compaction`/`finish_compaction`/
/// `record_stall` functions; until those are called the table is genuinely
/// empty, so the `/proc/compstat` file and the `compstat` kshell command report
/// zeros rather than fabricated numbers — the kernel's hard "never invent data
/// in procfs" rule.
///
/// NOTE: this previously seeded three fictional zones (DMA32 attempts 50,
/// Normal attempts 500 / pages_migrated 80000, Movable attempts 100) plus
/// invented aggregate totals (total_attempts 650, total_migrations 103000,
/// total_stalls 25, total_stall_ns 500_000_000), which `/proc/compstat` then
/// displayed as if they were real compaction measurements.  That demo data was
/// removed; the self-test now builds its own fixtures explicitly via the real
/// API (see [`self_test`]).  The page allocator is expected to call
/// [`register_zone`] per online zone and the start/finish/stall functions as
/// compaction runs.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        zones: Vec::new(),
        events: Vec::new(),
        active_zone: None,
        total_attempts: 0,
        total_migrations: 0,
        total_stalls: 0,
        total_stall_ns: 0,
        ops: 0,
    });
}

/// Register a memory zone for compaction tracking.
///
/// The page allocator calls this once per online zone at bring-up so the
/// per-zone compaction table reflects the real topology with all counters
/// zeroed.  [`start_compaction`] returns `NotFound` for an unregistered zone.
pub fn register_zone(zone: CompactZone) -> KernelResult<()> {
    with_state(|state| {
        if state.zones.iter().any(|z| z.zone == zone) { return Err(KernelError::AlreadyExists); }
        state.zones.push(ZoneCompactStats {
            zone, attempts: 0, successes: 0, failures: 0, deferred: 0,
            pages_scanned_free: 0, pages_scanned_migrate: 0, pages_migrated: 0,
            pages_failed: 0, stalls: 0,
        });
        Ok(())
    })
}

/// Start a compaction run in a zone.
pub fn start_compaction(zone: CompactZone) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        state.active_zone = Some((zone, now));
        let zs = state.zones.iter_mut().find(|z| z.zone == zone)
            .ok_or(KernelError::NotFound)?;
        zs.attempts += 1;
        state.total_attempts += 1;
        Ok(())
    })
}

/// Finish a compaction run.
pub fn finish_compaction(result: CompactResult, pages_migrated: u64, pages_scanned: u64) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let (zone, start) = state.active_zone.take().ok_or(KernelError::InvalidArgument)?;
        let duration = now.saturating_sub(start);
        let zs = state.zones.iter_mut().find(|z| z.zone == zone)
            .ok_or(KernelError::NotFound)?;
        match result {
            CompactResult::Success => zs.successes += 1,
            CompactResult::Failed => zs.failures += 1,
            CompactResult::Deferred => zs.deferred += 1,
            CompactResult::Skipped => {}
        }
        zs.pages_migrated += pages_migrated;
        zs.pages_scanned_migrate += pages_scanned;
        state.total_migrations += pages_migrated;
        if state.events.len() >= MAX_EVENTS { state.events.remove(0); }
        state.events.push(CompactionEvent {
            zone, result, pages_migrated, pages_scanned,
            duration_ns: duration, timestamp_ns: now,
        });
        Ok(())
    })
}

/// Record a process stall due to compaction.
pub fn record_stall(duration_ns: u64) -> KernelResult<()> {
    with_state(|state| {
        if let Some((zone, _)) = state.active_zone {
            if let Some(zs) = state.zones.iter_mut().find(|z| z.zone == zone) {
                zs.stalls += 1;
            }
        }
        state.total_stalls += 1;
        state.total_stall_ns += duration_ns;
        Ok(())
    })
}

/// Get per-zone compaction stats.
pub fn zone_stats() -> Vec<ZoneCompactStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.zones.clone())
}

/// Recent compaction events.
pub fn recent_events(n: usize) -> Vec<CompactionEvent> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = if n >= s.events.len() { 0 } else { s.events.len() - n };
        s.events[start..].to_vec()
    })
}

/// Compaction success rate as integer percentage (0-100).
pub fn success_rate() -> u64 {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            if s.total_attempts == 0 { 100 }
            else {
                let successes: u64 = s.zones.iter().map(|z| z.successes).sum();
                successes * 100 / s.total_attempts
            }
        }
        None => 0,
    }
}

/// Statistics: (zone_count, total_attempts, total_migrations, total_stalls, total_stall_ns, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.zones.len(), s.total_attempts, s.total_migrations, s.total_stalls, s.total_stall_ns, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("compstat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/compstat must never surface).
    // Resetting first clears any residue from a prior `compstat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated zones, events, or totals. With zero
    //    attempts, success_rate() is defined as 100 (vacuously perfect).
    assert_eq!(zone_stats().len(), 0);
    let (z0, a0, m0, s0, sn0, _o0) = stats();
    assert_eq!((z0, a0, m0, s0, sn0), (0, 0, 0, 0, 0));
    assert_eq!(success_rate(), 100);
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register zones (zeroed); duplicate fails; start before register fails.
    assert!(start_compaction(CompactZone::Normal).is_err()); // not registered yet
    register_zone(CompactZone::Normal).expect("reg normal");
    register_zone(CompactZone::Movable).expect("reg movable");
    assert!(register_zone(CompactZone::Normal).is_err());
    assert_eq!(zone_stats().len(), 2);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Start increments attempts exactly from zero.
    start_compaction(CompactZone::Normal).expect("start");
    let z = zone_stats().iter().find(|z| z.zone == CompactZone::Normal).cloned().expect("z");
    assert_eq!(z.attempts, 1);
    crate::serial_println!("  [3/8] start: OK");

    // 4: Finish records a success + event with exact migrated pages.
    finish_compaction(CompactResult::Success, 50, 200).expect("finish");
    let events = recent_events(5);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].pages_migrated, 50);
    let z = zone_stats().iter().find(|z| z.zone == CompactZone::Normal).cloned().expect("z");
    assert_eq!(z.successes, 1);
    assert_eq!(z.pages_migrated, 50);
    crate::serial_println!("  [4/8] finish: OK");

    // 5: A failed run on another zone records a failure + second event.
    start_compaction(CompactZone::Movable).expect("start2");
    finish_compaction(CompactResult::Failed, 0, 100).expect("finish2");
    let z = zone_stats().iter().find(|z| z.zone == CompactZone::Movable).cloned().expect("z");
    assert_eq!(z.failures, 1);
    assert_eq!(recent_events(5).len(), 2);
    crate::serial_println!("  [5/8] failed: OK");

    // 6: Stall accumulates exactly from zero (one stall, 1ms).
    start_compaction(CompactZone::Normal).expect("start3");
    record_stall(1_000_000).expect("stall");
    finish_compaction(CompactResult::Success, 10, 50).expect("finish3");
    let (_, _, _, stalls, stall_ns, _) = stats();
    assert_eq!(stalls, 1);
    assert_eq!(stall_ns, 1_000_000);
    crate::serial_println!("  [6/8] stall: OK");

    // 7: Finishing with no active run fails with InvalidArgument.
    assert!(finish_compaction(CompactResult::Success, 0, 0).is_err());
    crate::serial_println!("  [7/8] finish without start: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    //    Three starts (Normal x2, Movable x1) → attempts 3. Migrations 50+0+10
    //    = 60. Two Normal successes / 3 attempts → success_rate 66%.
    let (zones, attempts, migrations, _stalls, _stall_ns, ops) = stats();
    assert_eq!(zones, 2);
    assert_eq!(attempts, 3);
    assert_eq!(migrations, 60);
    assert_eq!(success_rate(), 66); // 2 successes * 100 / 3 attempts
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/compstat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the page allocator wires
    // real accounting.
    *STATE.lock() = None;

    crate::serial_println!("compstat::self_test() — all 8 tests passed");
}
