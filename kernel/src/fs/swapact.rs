//! Swap Activity Statistics — swap in/out monitoring.
//!
//! Tracks swap-in and swap-out page counts, latencies, per-device
//! swap usage, and swap pressure. Essential for memory pressure
//! analysis and swap performance tuning.
//!
//! ## Architecture
//!
//! ```text
//! Swap activity monitoring
//!   → swapact::register(name, pages) → register swap area
//!   → swapact::record_in(name, pages, ns) → swap in
//!   → swapact::record_out(name, pages, ns) → swap out
//!   → swapact::per_area() → per-area stats
//!
//! Integration:
//!   → mempress (memory pressure)
//!   → pagestat (page allocator)
//!   → zramstat (ZRAM swap)
//!   → oomkiller (OOM killer)
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

/// Swap area type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapType {
    Partition,
    File,
    Zram,
}

impl SwapType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Partition => "partition",
            Self::File => "file",
            Self::Zram => "zram",
        }
    }
}

/// Per-area swap stats.
#[derive(Debug, Clone)]
pub struct SwapAreaStats {
    pub name: String,
    pub swap_type: SwapType,
    pub total_pages: u64,
    pub used_pages: u64,
    pub swap_in_count: u64,
    pub swap_in_pages: u64,
    pub swap_in_ns: u64,
    pub swap_out_count: u64,
    pub swap_out_pages: u64,
    pub swap_out_ns: u64,
    pub priority: i32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_AREAS: usize = 32;

struct State {
    areas: Vec<SwapAreaStats>,
    total_swap_in: u64,
    total_swap_out: u64,
    total_swap_in_pages: u64,
    total_swap_out_pages: u64,
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

/// Initialise an **empty** swap-activity table.
///
/// Seeds NO swap areas and zero counters.  Real swap accounting is wired through
/// [`register`] (one row per swap area the mm/swap layer activates) and the
/// `record_in`/`record_out` functions; until those are called the table is
/// genuinely empty, so `/proc/swapact` and the `swapact` kshell command report
/// zeros rather than fabricated numbers — the kernel's hard "never invent data in
/// procfs" rule.
///
/// NOTE: this previously seeded two fictional swap areas ("/dev/sda2" partition:
/// 2M total / 500k used pages / 1M swap-ins / 1.5M in-pages / 800k swap-outs /
/// 1.2M out-pages; "/dev/zram0" zram: 1M total / 300k used / 500k swap-ins / 400k
/// swap-outs) plus invented aggregate totals (total_swap_in 1.5M, total_swap_out
/// 1.2M, total_swap_in_pages 2.1M, total_swap_out_pages 1.7M), which
/// `/proc/swapact` (and the `per_area` view) then displayed as if they were real
/// measured swap traffic.  That demo data was removed; the self-test now builds
/// its own fixtures explicitly via the real API (see [`self_test`]).  The swap
/// layer is expected to call [`register`] when a swap area is activated and the
/// record functions on every swap-in/swap-out.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        areas: Vec::new(),
        total_swap_in: 0,
        total_swap_out: 0,
        total_swap_in_pages: 0,
        total_swap_out_pages: 0,
        ops: 0,
    });
}

/// Register a swap area.
pub fn register(name: &str, swap_type: SwapType, total_pages: u64, priority: i32) -> KernelResult<()> {
    with_state(|state| {
        if state.areas.len() >= MAX_AREAS { return Err(KernelError::ResourceExhausted); }
        if state.areas.iter().any(|a| a.name == name) { return Err(KernelError::AlreadyExists); }
        state.areas.push(SwapAreaStats {
            name: String::from(name), swap_type, total_pages, used_pages: 0,
            swap_in_count: 0, swap_in_pages: 0, swap_in_ns: 0,
            swap_out_count: 0, swap_out_pages: 0, swap_out_ns: 0, priority,
        });
        Ok(())
    })
}

/// Record swap-in.
pub fn record_in(name: &str, pages: u64, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let a = state.areas.iter_mut().find(|a| a.name == name)
            .ok_or(KernelError::NotFound)?;
        a.swap_in_count += 1;
        a.swap_in_pages += pages;
        a.swap_in_ns += ns;
        a.used_pages = a.used_pages.saturating_sub(pages);
        state.total_swap_in += 1;
        state.total_swap_in_pages += pages;
        Ok(())
    })
}

/// Record swap-out.
pub fn record_out(name: &str, pages: u64, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let a = state.areas.iter_mut().find(|a| a.name == name)
            .ok_or(KernelError::NotFound)?;
        a.swap_out_count += 1;
        a.swap_out_pages += pages;
        a.swap_out_ns += ns;
        a.used_pages += pages;
        if a.used_pages > a.total_pages { a.used_pages = a.total_pages; }
        state.total_swap_out += 1;
        state.total_swap_out_pages += pages;
        Ok(())
    })
}

/// Per-area stats.
pub fn per_area() -> Vec<SwapAreaStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.areas.clone())
}

/// Statistics: (area_count, total_swap_in, total_swap_out, total_in_pages, total_out_pages, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.areas.len(), s.total_swap_in, s.total_swap_out, s.total_swap_in_pages, s.total_swap_out_pages, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("swapact::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/swapact must never surface).  Resetting
    // first clears any residue from a prior `swapact test` run so the totals
    // asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated areas or counters.
    assert_eq!(per_area().len(), 0);
    let (c0, si0, so0, sip0, sop0, _o0) = stats();
    assert_eq!((c0, si0, so0, sip0, sop0), (0, 0, 0, 0, 0));
    assert!(record_in("/swapfile", 1, 1).is_err()); // no phantom area exists yet
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register — zeroed counters, type/total/priority preserved; dup fails.
    register("/swapfile", SwapType::File, 500_000, -2).expect("register");
    let a = per_area().into_iter().find(|a| a.name == "/swapfile").expect("find");
    assert_eq!(a.swap_type, SwapType::File);
    assert_eq!(a.total_pages, 500_000);
    assert_eq!(a.priority, -2);
    assert_eq!((a.used_pages, a.swap_in_count, a.swap_out_count), (0, 0, 0));
    assert!(register("/swapfile", SwapType::File, 500_000, -2).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Swap out — count/pages/latency rise, used_pages grows.
    record_out("/swapfile", 100, 5000).expect("out");
    let a = per_area().into_iter().find(|a| a.name == "/swapfile").expect("find");
    assert_eq!(a.swap_out_count, 1);
    assert_eq!(a.swap_out_pages, 100);
    assert_eq!(a.swap_out_ns, 5000);
    assert_eq!(a.used_pages, 100);
    crate::serial_println!("  [3/8] swap out: OK");

    // 4: Swap in — used_pages drops by the paged-in count.
    record_in("/swapfile", 50, 3000).expect("in");
    let a = per_area().into_iter().find(|a| a.name == "/swapfile").expect("find");
    assert_eq!(a.swap_in_count, 1);
    assert_eq!(a.swap_in_pages, 50);
    assert_eq!(a.swap_in_ns, 3000);
    assert_eq!(a.used_pages, 50); // 100 - 50
    crate::serial_println!("  [4/8] swap in: OK");

    // 5: Swap-in underflow guard — paging in more than resident saturates to 0,
    // never underflows.
    record_in("/swapfile", 9999, 1).expect("in_over");
    let a = per_area().into_iter().find(|a| a.name == "/swapfile").expect("find");
    assert_eq!(a.used_pages, 0);
    crate::serial_println!("  [5/8] swap-in underflow guard: OK");

    // 6: Saturation — used_pages is clamped to total_pages on swap-out.
    record_out("/swapfile", 1_000_000, 100).expect("out_big");
    let a = per_area().into_iter().find(|a| a.name == "/swapfile").expect("find");
    assert_eq!(a.used_pages, a.total_pages); // clamped to 500_000
    crate::serial_println!("  [6/8] saturation: OK");

    // 7: Unknown area → NotFound on both record paths.
    assert!(record_in("nonexist", 1, 1).is_err());
    assert!(record_out("nonexist", 1, 1).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate totals are exact: 2 swap-ins (50 + 9999 pages), 2 swap-outs
    // (100 + 1,000,000 pages).
    let (areas, si, so, sip, sop, ops) = stats();
    assert_eq!(areas, 1);
    assert_eq!(si, 2);
    assert_eq!(so, 2);
    assert_eq!(sip, 50 + 9999);
    assert_eq!(sop, 100 + 1_000_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/swapact table.
    *STATE.lock() = None;

    crate::serial_println!("swapact::self_test() — all 8 tests passed");
}
