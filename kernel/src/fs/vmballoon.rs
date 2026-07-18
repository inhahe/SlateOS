//! VM Memory Balloon — virtual machine balloon driver monitoring.
//!
//! Tracks balloon inflate/deflate operations, current balloon
//! size, and OOM events. Essential for VM memory management
//! and overcommit diagnostics.
//!
//! ## Architecture
//!
//! ```text
//! VM balloon monitoring
//!   → vmballoon::inflate(pages) → balloon inflated
//!   → vmballoon::deflate(pages) → balloon deflated
//!   → vmballoon::record_oom() → OOM from balloon pressure
//!   → vmballoon::status() → current balloon status
//!
//! Integration:
//!   → mempress (memory pressure)
//!   → pagestat (page allocator)
//!   → oomkiller (OOM killer)
//!   → thpstat (transparent huge pages)
//! ```

#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Balloon driver info.
#[derive(Debug, Clone)]
pub struct BalloonStatus {
    pub current_pages: u64,
    pub target_pages: u64,
    pub max_pages: u64,
    pub inflates: u64,
    pub deflates: u64,
    pub inflate_pages_total: u64,
    pub deflate_pages_total: u64,
    pub oom_events: u64,
    pub free_page_hints: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    status: BalloonStatus,
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

/// Initialise the VM-balloon statistics state.
///
/// Starts with no balloon attached: zero capacity and all activity
/// counters at zero. The `/proc/vmballoon` generator and the `vmballoon`
/// kshell command surface this status as if it reflects a real balloon
/// driver, so seeding it with invented inflate/deflate activity would be
/// fabricated procfs data. The balloon driver advertises its capacity
/// through [`configure`] when it attaches, and the counters advance only
/// through real [`inflate`] / [`deflate`] / [`record_oom`] /
/// [`record_free_hint`] calls.
///
/// (Previously this seeded a fictional balloon — 100k current/target
/// pages, 1M max, 500 inflates / 300 deflates, 5M/4.9M inflate/deflate
/// page totals, 2 OOM events, and 10k free-page hints.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        status: BalloonStatus {
            current_pages: 0,
            target_pages: 0,
            max_pages: 0,
            inflates: 0,
            deflates: 0,
            inflate_pages_total: 0,
            deflate_pages_total: 0,
            oom_events: 0,
            free_page_hints: 0,
        },
        ops: 0,
    });
}

/// Configure the balloon's capacity.
///
/// Called by the balloon driver when it attaches to advertise the maximum
/// number of pages the host may reclaim via the balloon. Resets the
/// balloon to a clean, fully-deflated state (current/target 0, all
/// activity counters 0) so the published status reflects only real
/// post-attach activity.
pub fn configure(max_pages: u64) -> KernelResult<()> {
    with_state(|state| {
        state.status = BalloonStatus {
            current_pages: 0,
            target_pages: 0,
            max_pages,
            inflates: 0,
            deflates: 0,
            inflate_pages_total: 0,
            deflate_pages_total: 0,
            oom_events: 0,
            free_page_hints: 0,
        };
        Ok(())
    })
}

/// Inflate the balloon (return pages to host).
pub fn inflate(pages: u64) -> KernelResult<()> {
    with_state(|state| {
        state.status.current_pages += pages;
        state.status.inflates += 1;
        state.status.inflate_pages_total += pages;
        if state.status.current_pages > state.status.max_pages {
            state.status.current_pages = state.status.max_pages;
        }
        Ok(())
    })
}

/// Deflate the balloon (reclaim pages from host).
pub fn deflate(pages: u64) -> KernelResult<()> {
    with_state(|state| {
        state.status.current_pages = state.status.current_pages.saturating_sub(pages);
        state.status.deflates += 1;
        state.status.deflate_pages_total += pages;
        Ok(())
    })
}

/// Set target pages.
pub fn set_target(pages: u64) -> KernelResult<()> {
    with_state(|state| {
        state.status.target_pages = pages;
        Ok(())
    })
}

/// Record an OOM event caused by balloon pressure.
pub fn record_oom() -> KernelResult<()> {
    with_state(|state| {
        state.status.oom_events += 1;
        Ok(())
    })
}

/// Record free page hints sent to host.
pub fn record_free_hint(count: u64) -> KernelResult<()> {
    with_state(|state| {
        state.status.free_page_hints += count;
        Ok(())
    })
}

/// Get current balloon status.
pub fn status() -> Option<BalloonStatus> {
    STATE.lock().as_ref().map(|s| s.status.clone())
}

/// Statistics: (current_pages, target_pages, inflates, deflates, oom_events, ops).
pub fn stats() -> (u64, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.status.current_pages, s.status.target_pages, s.status.inflates, s.status.deflates, s.status.oom_events, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("vmballoon::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live balloon status afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no balloon attached, all counters zero.
    let s = status().expect("status");
    assert_eq!(s.current_pages, 0);
    assert_eq!(s.max_pages, 0);
    assert_eq!(s.inflates, 0);
    assert_eq!(s.deflates, 0);
    assert_eq!(s.oom_events, 0);
    assert_eq!(s.free_page_hints, 0);
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Configure capacity — sets max, leaves the balloon fully deflated.
    configure(1_000_000).expect("configure");
    let s = status().expect("status");
    assert_eq!(s.max_pages, 1_000_000);
    assert_eq!(s.current_pages, 0);
    crate::serial_println!("  [2/8] configure: OK");

    // 3: Inflate adds pages and counts.
    inflate(1000).expect("inflate");
    let s = status().expect("status");
    assert_eq!(s.current_pages, 1000);
    assert_eq!(s.inflates, 1);
    assert_eq!(s.inflate_pages_total, 1000);
    crate::serial_println!("  [3/8] inflate: OK");

    // 4: Deflate removes pages and counts.
    deflate(500).expect("deflate");
    let s = status().expect("status");
    assert_eq!(s.current_pages, 500);
    assert_eq!(s.deflates, 1);
    assert_eq!(s.deflate_pages_total, 500);
    crate::serial_println!("  [4/8] deflate: OK");

    // 5: Target and OOM accounting.
    set_target(200_000).expect("target");
    record_oom().expect("oom");
    let s = status().expect("status");
    assert_eq!(s.target_pages, 200_000);
    assert_eq!(s.oom_events, 1);
    crate::serial_println!("  [5/8] target + oom: OK");

    // 6: Free-page hints accumulate exactly.
    record_free_hint(100).expect("hint");
    record_free_hint(50).expect("hint2");
    assert_eq!(status().expect("status").free_page_hints, 150);
    crate::serial_println!("  [6/8] hints: OK");

    // 7: Inflate caps current_pages at the configured max.
    inflate(2_000_000).expect("big_inflate");
    assert_eq!(status().expect("status").current_pages, 1_000_000);
    crate::serial_println!("  [7/8] max cap: OK");

    // 8: Final stats reflect only the real activity above.
    let (cur, target, inf, def, oom, ops) = stats();
    assert_eq!(cur, 1_000_000);
    assert_eq!(target, 200_000);
    assert_eq!(inf, 2); // test 3 + test 7
    assert_eq!(def, 1);
    assert_eq!(oom, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("vmballoon::self_test() — all 8 tests passed");
}
