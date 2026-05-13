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
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        status: BalloonStatus {
            current_pages: 100_000,
            target_pages: 100_000,
            max_pages: 1_000_000,
            inflates: 500,
            deflates: 300,
            inflate_pages_total: 5_000_000,
            deflate_pages_total: 4_900_000,
            oom_events: 2,
            free_page_hints: 10_000,
        },
        ops: 0,
    });
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
    init_defaults();

    // 1: Defaults.
    let s = status().unwrap();
    assert_eq!(s.current_pages, 100_000);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Inflate.
    inflate(1000).expect("inflate");
    let s = status().unwrap();
    assert_eq!(s.current_pages, 101_000);
    assert_eq!(s.inflates, 501);
    crate::serial_println!("  [2/8] inflate: OK");

    // 3: Deflate.
    deflate(500).expect("deflate");
    let s = status().unwrap();
    assert_eq!(s.current_pages, 100_500);
    assert_eq!(s.deflates, 301);
    crate::serial_println!("  [3/8] deflate: OK");

    // 4: Target.
    set_target(200_000).expect("target");
    let s = status().unwrap();
    assert_eq!(s.target_pages, 200_000);
    crate::serial_println!("  [4/8] target: OK");

    // 5: OOM.
    record_oom().expect("oom");
    let s = status().unwrap();
    assert_eq!(s.oom_events, 3);
    crate::serial_println!("  [5/8] oom: OK");

    // 6: Free hints.
    record_free_hint(100).expect("hint");
    let s = status().unwrap();
    assert_eq!(s.free_page_hints, 10_100);
    crate::serial_println!("  [6/8] hints: OK");

    // 7: Max cap.
    inflate(2_000_000).expect("big_inflate");
    let s = status().unwrap();
    assert_eq!(s.current_pages, 1_000_000); // capped at max
    crate::serial_println!("  [7/8] max cap: OK");

    // 8: Stats.
    let (cur, target, inf, def, oom, ops) = stats();
    assert_eq!(cur, 1_000_000);
    assert_eq!(target, 200_000);
    assert!(inf > 500);
    assert!(def > 300);
    assert!(oom > 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("vmballoon::self_test() — all 8 tests passed");
}
