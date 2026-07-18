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

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise the THP statistics table with **zeroed** counters.
///
/// The two size-class rows (PMD 2 MiB, PUD 1 GiB) are real fixed structure — the
/// THP layer always has exactly these two classes, and the record functions index
/// them directly rather than looking them up — so they are kept, but every
/// counter starts at zero.  Real accounting is wired through the
/// `record_promotion`/`record_demotion`/`record_split`/`record_alloc_failure`/
/// `record_compaction`/`record_khugepaged_scan` functions; until those are called
/// the counters are genuinely zero, so `/proc/thpstat` and the `thpstat` kshell
/// command report zeros rather than fabricated numbers — the kernel's hard "never
/// invent data in procfs" rule.
///
/// NOTE: this previously seeded fabricated activity (PMD: 500k promotions / 10k
/// demotions / 50k splits / 600k alloc attempts / 100k failures / ~1 TiB promoted;
/// PUD: 100 promotions / ~100 GiB promoted; compaction 200k success / 50k failed /
/// 30k deferred / 100k skipped; khugepaged 1M scans / 400k collapses; totals
/// 500,100 promotions / 10,005 demotions), which `/proc/thpstat` (and the
/// `per_size`/`compaction_stats` views) then displayed as if they were real
/// measured huge-page activity.  That demo data was removed; the self-test now
/// builds its own fixtures explicitly via the real API (see [`self_test`]).  The
/// memory manager is expected to call the record functions on every huge-page
/// promotion/demotion/split/compaction/khugepaged event.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        pmd_stats: SizeClassStats {
            size: ThpSize::Pmd, promotions: 0, demotions: 0,
            splits: 0, alloc_attempts: 0, alloc_failures: 0,
            bytes_promoted: 0, bytes_demoted: 0,
        },
        pud_stats: SizeClassStats {
            size: ThpSize::Pud, promotions: 0, demotions: 0,
            splits: 0, alloc_attempts: 0, alloc_failures: 0,
            bytes_promoted: 0, bytes_demoted: 0,
        },
        compact_success: 0,
        compact_failed: 0,
        compact_deferred: 0,
        compact_skipped: 0,
        khugepaged_scans: 0,
        khugepaged_collapses: 0,
        total_promotions: 0,
        total_demotions: 0,
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
    // Begin from a clean, zeroed table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/thpstat must never surface).  Resetting
    // first clears any residue from a prior `thpstat test` run so the totals
    // asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Zeroed after init — two size classes exist but every counter is 0.
    let s = per_size();
    assert_eq!(s.len(), 2);
    assert_eq!((s[0].promotions, s[0].demotions, s[0].splits, s[0].alloc_failures), (0, 0, 0, 0));
    assert_eq!((s[1].promotions, s[1].bytes_promoted), (0, 0));
    assert_eq!(compaction_stats(), (0, 0, 0, 0));
    let (p, d, sc, co, _o) = stats();
    assert_eq!((p, d, sc, co), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] zeroed init: OK");

    // 2: PMD promotion — count + attempt rise, 2 MiB credited to bytes_promoted.
    record_promotion(ThpSize::Pmd).expect("promote");
    let s = per_size();
    assert_eq!(s[0].promotions, 1);
    assert_eq!(s[0].alloc_attempts, 1);
    assert_eq!(s[0].bytes_promoted, 2 * 1024 * 1024);
    crate::serial_println!("  [2/8] promotion: OK");

    // 3: PUD promotion — 1 GiB credited to the gigantic-page class.
    record_promotion(ThpSize::Pud).expect("promote_pud");
    let s = per_size();
    assert_eq!(s[1].promotions, 1);
    assert_eq!(s[1].bytes_promoted, 1024 * 1024 * 1024);
    crate::serial_println!("  [3/8] pud promotion: OK");

    // 4: PMD demotion — 2 MiB credited to bytes_demoted.
    record_demotion(ThpSize::Pmd).expect("demote");
    let s = per_size();
    assert_eq!(s[0].demotions, 1);
    assert_eq!(s[0].bytes_demoted, 2 * 1024 * 1024);
    crate::serial_println!("  [4/8] demotion: OK");

    // 5: Split + alloc failure — split counts; alloc failure bumps both the
    // attempt counter (now 2: one promotion + one failed) and the failure count.
    record_split(ThpSize::Pmd).expect("split");
    record_alloc_failure(ThpSize::Pmd).expect("alloc_fail");
    let s = per_size();
    assert_eq!(s[0].splits, 1);
    assert_eq!(s[0].alloc_attempts, 2);
    assert_eq!(s[0].alloc_failures, 1);
    crate::serial_println!("  [5/8] split + alloc failure: OK");

    // 6: Compaction — one of each result lands in its own bucket.
    record_compaction(CompactResult::Success).expect("c_ok");
    record_compaction(CompactResult::Failed).expect("c_fail");
    record_compaction(CompactResult::Deferred).expect("c_def");
    record_compaction(CompactResult::Skipped).expect("c_skip");
    assert_eq!(compaction_stats(), (1, 1, 1, 1));
    crate::serial_println!("  [6/8] compaction: OK");

    // 7: Khugepaged — two scans, one of which collapsed.
    record_khugepaged_scan(true).expect("khp1");
    record_khugepaged_scan(false).expect("khp2");
    let (_, _, scans, collapses, _) = stats();
    assert_eq!(scans, 2);
    assert_eq!(collapses, 1);
    crate::serial_println!("  [7/8] khugepaged: OK");

    // 8: Aggregate totals are exact: 2 promotions (PMD + PUD), 1 demotion.
    let (promos, demos, scans, collapses, ops) = stats();
    assert_eq!(promos, 2);
    assert_eq!(demos, 1);
    assert_eq!(scans, 2);
    assert_eq!(collapses, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/thpstat table.
    *STATE.lock() = None;

    crate::serial_println!("thpstat::self_test() — all 8 tests passed");
}
