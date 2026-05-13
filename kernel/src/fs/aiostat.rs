//! Async I/O Statistics — io_uring-style submission queue monitoring.
//!
//! Tracks async I/O submission/completion rates, queue depths,
//! CQE overflow events, and per-ring statistics. Essential
//! for high-throughput I/O workload tuning.
//!
//! ## Architecture
//!
//! ```text
//! Async I/O monitoring
//!   → aiostat::submit(ring_id, count) → track SQE submissions
//!   → aiostat::complete(ring_id, count) → track CQE completions
//!   → aiostat::overflow(ring_id) → CQ overflow event
//!   → aiostat::ring_stats() → per-ring statistics
//!
//! Integration:
//!   → iosched (I/O scheduler)
//!   → iolatency (I/O latency)
//!   → epollstat (event polling)
//!   → taskstats (per-task accounting)
//! ```

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Per-ring statistics.
#[derive(Debug, Clone)]
pub struct RingStats {
    pub ring_id: u32,
    pub pid: u32,
    pub sq_size: u32,
    pub cq_size: u32,
    pub sq_pending: u32,
    pub cq_pending: u32,
    pub submitted: u64,
    pub completed: u64,
    pub overflows: u64,
    pub sq_full_count: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_RINGS: usize = 256;

struct State {
    rings: Vec<RingStats>,
    next_id: u32,
    total_submitted: u64,
    total_completed: u64,
    total_overflows: u64,
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
        rings: alloc::vec![
            RingStats { ring_id: 1, pid: 1, sq_size: 256, cq_size: 512, sq_pending: 10, cq_pending: 5, submitted: 50_000_000, completed: 49_999_900, overflows: 100, sq_full_count: 5000 },
            RingStats { ring_id: 2, pid: 100, sq_size: 1024, cq_size: 2048, sq_pending: 64, cq_pending: 32, submitted: 200_000_000, completed: 199_999_500, overflows: 500, sq_full_count: 20000 },
        ],
        next_id: 3,
        total_submitted: 250_000_000,
        total_completed: 249_999_400,
        total_overflows: 600,
        ops: 0,
    });
}

/// Create a ring.
pub fn create_ring(pid: u32, sq_size: u32, cq_size: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.rings.len() >= MAX_RINGS { return Err(KernelError::ResourceExhausted); }
        let id = state.next_id;
        state.next_id += 1;
        state.rings.push(RingStats {
            ring_id: id, pid, sq_size, cq_size, sq_pending: 0, cq_pending: 0,
            submitted: 0, completed: 0, overflows: 0, sq_full_count: 0,
        });
        Ok(id)
    })
}

/// Destroy a ring.
pub fn destroy_ring(ring_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.rings.iter().position(|r| r.ring_id == ring_id)
            .ok_or(KernelError::NotFound)?;
        state.rings.remove(idx);
        Ok(())
    })
}

/// Submit entries.
pub fn submit(ring_id: u32, count: u32) -> KernelResult<()> {
    with_state(|state| {
        let r = state.rings.iter_mut().find(|r| r.ring_id == ring_id)
            .ok_or(KernelError::NotFound)?;
        r.submitted += count as u64;
        r.sq_pending += count;
        if r.sq_pending >= r.sq_size { r.sq_full_count += 1; }
        state.total_submitted += count as u64;
        Ok(())
    })
}

/// Complete entries.
pub fn complete(ring_id: u32, count: u32) -> KernelResult<()> {
    with_state(|state| {
        let r = state.rings.iter_mut().find(|r| r.ring_id == ring_id)
            .ok_or(KernelError::NotFound)?;
        r.completed += count as u64;
        r.sq_pending = r.sq_pending.saturating_sub(count);
        r.cq_pending += count;
        state.total_completed += count as u64;
        Ok(())
    })
}

/// CQ overflow.
pub fn overflow(ring_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let r = state.rings.iter_mut().find(|r| r.ring_id == ring_id)
            .ok_or(KernelError::NotFound)?;
        r.overflows += 1;
        state.total_overflows += 1;
        Ok(())
    })
}

/// Per-ring stats.
pub fn ring_stats() -> Vec<RingStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.rings.clone())
}

/// Statistics: (ring_count, total_submitted, total_completed, total_overflows, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.rings.len(), s.total_submitted, s.total_completed, s.total_overflows, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("aiostat::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(ring_stats().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Create ring.
    let id = create_ring(200, 128, 256).expect("create");
    assert!(id >= 3);
    assert_eq!(ring_stats().len(), 3);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Submit.
    submit(id, 10).expect("submit");
    let r = ring_stats().iter().find(|r| r.ring_id == id).cloned().unwrap();
    assert_eq!(r.submitted, 10);
    assert_eq!(r.sq_pending, 10);
    crate::serial_println!("  [3/8] submit: OK");

    // 4: Complete.
    complete(id, 5).expect("complete");
    let r = ring_stats().iter().find(|r| r.ring_id == id).cloned().unwrap();
    assert_eq!(r.completed, 5);
    assert_eq!(r.sq_pending, 5);
    crate::serial_println!("  [4/8] complete: OK");

    // 5: Overflow.
    overflow(id).expect("overflow");
    let r = ring_stats().iter().find(|r| r.ring_id == id).cloned().unwrap();
    assert_eq!(r.overflows, 1);
    crate::serial_println!("  [5/8] overflow: OK");

    // 6: Destroy.
    destroy_ring(id).expect("destroy");
    assert_eq!(ring_stats().len(), 2);
    assert!(destroy_ring(id).is_err());
    crate::serial_println!("  [6/8] destroy: OK");

    // 7: Not found.
    assert!(submit(999, 1).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (rings, submitted, completed, overflows, ops) = stats();
    assert_eq!(rings, 2);
    assert!(submitted > 250_000_000);
    assert!(completed > 249_999_400);
    assert!(overflows > 600);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("aiostat::self_test() — all 8 tests passed");
}
