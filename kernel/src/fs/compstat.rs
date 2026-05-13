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

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        zones: alloc::vec![
            ZoneCompactStats { zone: CompactZone::Dma32, attempts: 50, successes: 40, failures: 5, deferred: 5, pages_scanned_free: 10000, pages_scanned_migrate: 8000, pages_migrated: 3000, pages_failed: 200, stalls: 2 },
            ZoneCompactStats { zone: CompactZone::Normal, attempts: 500, successes: 400, failures: 50, deferred: 50, pages_scanned_free: 200000, pages_scanned_migrate: 150000, pages_migrated: 80000, pages_failed: 5000, stalls: 20 },
            ZoneCompactStats { zone: CompactZone::Movable, attempts: 100, successes: 90, failures: 5, deferred: 5, pages_scanned_free: 50000, pages_scanned_migrate: 40000, pages_migrated: 20000, pages_failed: 500, stalls: 3 },
        ],
        events: Vec::new(),
        active_zone: None,
        total_attempts: 650,
        total_migrations: 103000,
        total_stalls: 25,
        total_stall_ns: 500_000_000,
        ops: 0,
    });
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(zone_stats().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Start compaction.
    let before = zone_stats().iter().find(|z| z.zone == CompactZone::Normal).unwrap().attempts;
    start_compaction(CompactZone::Normal).expect("start");
    let after = zone_stats().iter().find(|z| z.zone == CompactZone::Normal).unwrap().attempts;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [2/8] start: OK");

    // 3: Finish compaction.
    finish_compaction(CompactResult::Success, 50, 200).expect("finish");
    let events = recent_events(5);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].pages_migrated, 50);
    crate::serial_println!("  [3/8] finish: OK");

    // 4: Failed compaction.
    start_compaction(CompactZone::Movable).expect("start2");
    finish_compaction(CompactResult::Failed, 0, 100).expect("finish2");
    let events = recent_events(5);
    assert_eq!(events.len(), 2);
    crate::serial_println!("  [4/8] failed: OK");

    // 5: Stall.
    start_compaction(CompactZone::Normal).expect("start3");
    record_stall(1_000_000).expect("stall");
    finish_compaction(CompactResult::Success, 10, 50).expect("finish3");
    let (_, _, _, stalls, stall_ns, _) = stats();
    assert!(stalls >= 26);
    assert!(stall_ns > 500_000_000);
    crate::serial_println!("  [5/8] stall: OK");

    // 6: Success rate.
    let rate = success_rate();
    assert!(rate > 50);
    crate::serial_println!("  [6/8] success rate: OK ({}%)", rate);

    // 7: Finish without start.
    assert!(finish_compaction(CompactResult::Success, 0, 0).is_err());
    crate::serial_println!("  [7/8] finish without start: OK");

    // 8: Stats.
    let (zones, attempts, migrations, _stalls, _stall_ns, ops) = stats();
    assert_eq!(zones, 3);
    assert!(attempts > 650);
    assert!(migrations > 103000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("compstat::self_test() — all 8 tests passed");
}
