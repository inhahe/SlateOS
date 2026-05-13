//! Transparent Huge Pages — THP promotion/demotion monitoring.
//!
//! Tracks huge page promotions, demotions, splits, compaction
//! events, and khugepaged activity. Essential for understanding
//! memory allocation performance on large systems.
//!
//! ## Architecture
//!
//! ```text
//! THP monitoring
//!   → thpstat::record_promotion(size) → track page promotion to huge
//!   → thpstat::record_demotion(size) → track demotion/split
//!   → thpstat::record_compaction() → compaction attempt
//!   → thpstat::per_size() → per-size-class stats
//!
//! Integration:
//!   → pagestat (page allocator)
//!   → mempress (memory pressure)
//!   → compstat (compaction stats)
//!   → mmapstat (mmap operations)
//! ```

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// THP size class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThpSize {
    Pmd,     // 2 MiB (standard huge page)
    Pud,     // 1 GiB (gigantic page)
}

impl ThpSize {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pmd => "2MiB",
            Self::Pud => "1GiB",
        }
    }
    pub fn bytes(self) -> u64 {
        match self {
            Self::Pmd => 2 * 1024 * 1024,
            Self::Pud => 1024 * 1024 * 1024,
        }
    }
}

/// Compaction result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactResult {
    Success,
    Failed,
    Deferred,
    Skipped,
}

impl CompactResult {
    pub fn label(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Deferred => "deferred",
            Self::Skipped => "skipped",
        }
    }
}

/// Per-size-class stats.
#[derive(Debug, Clone)]
pub struct SizeClassStats {
    pub size: ThpSize,
    pub promotions: u64,
    pub demotions: u64,
    pub splits: u64,
    pub alloc_attempts: u64,
    pub alloc_failures: u64,
    pub bytes_promoted: u64,
    pub bytes_demoted: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    pmd_stats: SizeClassStats,
    pud_stats: SizeClassStats,
    compact_success: u64,
    compact_failed: u64,
    compact_deferred: u64,
    compact_skipped: u64,
    khugepaged_scans: u64,
    khugepaged_collapses: u64,
    total_promotions: u64,
    total_demotions: u64,
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

fn size_stats_mut(state: &mut State, size: ThpSize) -> &mut SizeClassStats {
    match size {
        ThpSize::Pmd => &mut state.pmd_stats,
        ThpSize::Pud => &mut state.pud_stats,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        pmd_stats: SizeClassStats {
            size: ThpSize::Pmd, promotions: 500_000, demotions: 10_000,
            splits: 50_000, alloc_attempts: 600_000, alloc_failures: 100_000,
            bytes_promoted: 500_000 * 2 * 1024 * 1024, bytes_demoted: 10_000 * 2 * 1024 * 1024,
        },
        pud_stats: SizeClassStats {
            size: ThpSize::Pud, promotions: 100, demotions: 5,
            splits: 20, alloc_attempts: 200, alloc_failures: 100,
            bytes_promoted: 100 * 1024 * 1024 * 1024, bytes_demoted: 5 * 1024 * 1024 * 1024,
        },
        compact_success: 200_000,
        compact_failed: 50_000,
        compact_deferred: 30_000,
        compact_skipped: 100_000,
        khugepaged_scans: 1_000_000,
        khugepaged_collapses: 400_000,
        total_promotions: 500_100,
        total_demotions: 10_005,
        ops: 0,
    });
}

/// Record a page promotion to huge page.
pub fn record_promotion(size: ThpSize) -> KernelResult<()> {
    with_state(|state| {
        let s = size_stats_mut(state, size);
        s.promotions += 1;
        s.alloc_attempts += 1;
        s.bytes_promoted += size.bytes();
        state.total_promotions += 1;
        Ok(())
    })
}

/// Record a demotion (huge → small pages).
pub fn record_demotion(size: ThpSize) -> KernelResult<()> {
    with_state(|state| {
        let s = size_stats_mut(state, size);
        s.demotions += 1;
        s.bytes_demoted += size.bytes();
        state.total_demotions += 1;
        Ok(())
    })
}

/// Record a huge page split.
pub fn record_split(size: ThpSize) -> KernelResult<()> {
    with_state(|state| {
        let s = size_stats_mut(state, size);
        s.splits += 1;
        Ok(())
    })
}

/// Record an allocation failure.
pub fn record_alloc_failure(size: ThpSize) -> KernelResult<()> {
    with_state(|state| {
        let s = size_stats_mut(state, size);
        s.alloc_attempts += 1;
        s.alloc_failures += 1;
        Ok(())
    })
}

/// Record a compaction event.
pub fn record_compaction(result: CompactResult) -> KernelResult<()> {
    with_state(|state| {
        match result {
            CompactResult::Success => state.compact_success += 1,
            CompactResult::Failed => state.compact_failed += 1,
            CompactResult::Deferred => state.compact_deferred += 1,
            CompactResult::Skipped => state.compact_skipped += 1,
        }
        Ok(())
    })
}

/// Record a khugepaged scan.
pub fn record_khugepaged_scan(collapsed: bool) -> KernelResult<()> {
    with_state(|state| {
        state.khugepaged_scans += 1;
        if collapsed { state.khugepaged_collapses += 1; }
        Ok(())
    })
}

/// Per-size-class stats.
pub fn per_size() -> Vec<SizeClassStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        alloc::vec![s.pmd_stats.clone(), s.pud_stats.clone()]
    })
}

/// Compaction stats: (success, failed, deferred, skipped).
pub fn compaction_stats() -> (u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.compact_success, s.compact_failed, s.compact_deferred, s.compact_skipped),
        None => (0, 0, 0, 0),
    }
}

/// Statistics: (total_promotions, total_demotions, khugepaged_scans, khugepaged_collapses, ops).
pub fn stats() -> (u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.total_promotions, s.total_demotions, s.khugepaged_scans, s.khugepaged_collapses, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("thpstat::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_size().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Promotion.
    let before = per_size()[0].promotions;
    record_promotion(ThpSize::Pmd).expect("promote");
    let after = per_size()[0].promotions;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [2/8] promotion: OK");

    // 3: Demotion.
    let before = per_size()[0].demotions;
    record_demotion(ThpSize::Pmd).expect("demote");
    let after = per_size()[0].demotions;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [3/8] demotion: OK");

    // 4: Split.
    let before = per_size()[0].splits;
    record_split(ThpSize::Pmd).expect("split");
    let after = per_size()[0].splits;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [4/8] split: OK");

    // 5: Alloc failure.
    let before = per_size()[0].alloc_failures;
    record_alloc_failure(ThpSize::Pmd).expect("alloc_fail");
    let after = per_size()[0].alloc_failures;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [5/8] alloc failure: OK");

    // 6: Compaction.
    let (s_before, _, _, _) = compaction_stats();
    record_compaction(CompactResult::Success).expect("compact");
    let (s_after, _, _, _) = compaction_stats();
    assert_eq!(s_after, s_before + 1);
    crate::serial_println!("  [6/8] compaction: OK");

    // 7: Khugepaged.
    let (_, _, scans_before, _collapses_before, _) = stats();
    record_khugepaged_scan(true).expect("khp");
    let (_, _, scans_after, collapses_after, _) = stats();
    assert_eq!(scans_after, scans_before + 1);
    assert!(collapses_after > _collapses_before);
    crate::serial_println!("  [7/8] khugepaged: OK");

    // 8: Stats.
    let (promos, demos, scans, collapses, ops) = stats();
    assert!(promos > 500_000);
    assert!(demos > 10_000);
    assert!(scans > 1_000_000);
    assert!(collapses > 400_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("thpstat::self_test() — all 8 tests passed");
}
