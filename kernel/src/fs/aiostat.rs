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

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise an **empty** async-I/O ring table.
///
/// Seeds NO ring rows and zero totals.  Real ring accounting is wired through
/// [`create_ring`]/[`destroy_ring`]/[`submit`]/[`complete`]/[`overflow`];
/// until those are called the table is genuinely empty, so the
/// `/proc/aiostat` file and the `aiostat` kshell command report zeros rather
/// than fabricated numbers — the kernel's hard "never invent data in procfs"
/// rule.
///
/// NOTE: this previously seeded two fictional io_uring rings (id 1/2 with
/// submitted 50_000_000 / 200_000_000) plus invented aggregate totals
/// (total_submitted 250_000_000), which `/proc/aiostat` then displayed as if
/// they were real async-I/O throughput statistics.  That demo data was
/// removed; the self-test now builds its own fixtures explicitly via the real
/// API (see [`self_test`]).  The io_uring syscall path is expected to call
/// [`create_ring`] on ring setup and [`submit`]/[`complete`] as SQEs/CQEs
/// flow.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        rings: Vec::new(),
        next_id: 1,
        total_submitted: 0,
        total_completed: 0,
        total_overflows: 0,
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
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/aiostat must never surface).
    // Resetting first clears any residue from a prior `aiostat test` run so
    // the totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated rings.
    assert_eq!(ring_stats().len(), 0);
    let (rc0, s0, c0, o0, _ops0) = stats();
    assert_eq!((rc0, s0, c0, o0), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Create ring (ids start at 1).
    let id = create_ring(200, 128, 256).expect("create");
    assert_eq!(id, 1);
    assert_eq!(ring_stats().len(), 1);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Submit (exact, from zero; sq_pending tracks SQEs in flight).
    submit(id, 10).expect("submit");
    let r = ring_stats().iter().find(|r| r.ring_id == id).cloned().expect("ring");
    assert_eq!(r.submitted, 10);
    assert_eq!(r.sq_pending, 10);
    crate::serial_println!("  [3/8] submit: OK");

    // 4: Complete drains the submission queue.
    complete(id, 5).expect("complete");
    let r = ring_stats().iter().find(|r| r.ring_id == id).cloned().expect("ring");
    assert_eq!(r.completed, 5);
    assert_eq!(r.sq_pending, 5);
    assert_eq!(r.cq_pending, 5);
    crate::serial_println!("  [4/8] complete: OK");

    // 5: A CQ overflow bumps the overflow counter; submitting on an unknown id
    //    fails with NotFound.
    overflow(id).expect("overflow");
    let r = ring_stats().iter().find(|r| r.ring_id == id).cloned().expect("ring");
    assert_eq!(r.overflows, 1);
    assert!(submit(9999, 1).is_err());
    crate::serial_println!("  [5/8] overflow: OK");

    // 6: A second ring; create returns the next id.
    let id2 = create_ring(201, 64, 128).expect("create2");
    assert_eq!(id2, 2);
    assert_eq!(ring_stats().len(), 2);
    crate::serial_println!("  [6/8] second ring: OK");

    // 7: Destroy; double-destroy fails.
    destroy_ring(id).expect("destroy");
    assert_eq!(ring_stats().len(), 1); // only ring 2 remains
    assert!(destroy_ring(id).is_err());
    let _ = id2;
    crate::serial_println!("  [7/8] destroy: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (rings, submitted, completed, overflows, ops) = stats();
    assert_eq!(rings, 1); // ring 2
    assert_eq!(submitted, 10); // one submit of 10
    assert_eq!(completed, 5); // one complete of 5
    assert_eq!(overflows, 1); // one overflow
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/aiostat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the io_uring syscall path
    // wires real accounting.
    *STATE.lock() = None;

    crate::serial_println!("aiostat::self_test() — all 8 tests passed");
}
