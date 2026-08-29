//! IPC Namespace Statistics — IPC namespace isolation monitoring.
//!
//! Tracks System V IPC namespaces: shared memory segments,
//! semaphore sets, message queues, and per-namespace resource
//! usage. Essential for container isolation diagnostics.
//!
//! ## Architecture
//!
//! ```text
//! IPC namespace monitoring
//!   → ipcns::create_ns(name) → create IPC namespace
//!   → ipcns::record_shm(ns_id) → shared memory segment created
//!   → ipcns::record_sem(ns_id) → semaphore set created
//!   → ipcns::record_msg(ns_id) → message queue created
//!   → ipcns::release_shm/_sem/_msg(ns_id) → the same resource destroyed
//!   → ipcns::ns_list() → list namespaces
//!
//! Integration:
//!   → shmem (shared memory)
//!   → pidstat (PID namespaces)
//!   → prociso (process isolation)
//!   → cgroupfs (cgroup filesystem)
//! ```
//!
//! Every `record_*` has a matching `release_*`, and the global totals reported
//! by [`stats`] are *live*: they are the sums of the per-namespace columns,
//! decremented by `release_*` and by [`destroy_ns`]. That is what makes the
//! header line of `/proc/ipcns` agree with the table printed beneath it.
//! `ops` is the one cumulative number in this module.

#![allow(dead_code)]

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// IPC namespace info.
#[derive(Debug, Clone)]
pub struct IpcNamespace {
    pub ns_id: u32,
    pub name: String,
    pub shm_segments: u64,
    pub shm_bytes: u64,
    pub sem_sets: u64,
    pub sem_total: u64,
    pub msg_queues: u64,
    pub msg_bytes: u64,
    pub created_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_NAMESPACES: usize = 256;

struct State {
    namespaces: Vec<IpcNamespace>,
    next_id: u32,
    total_shm: u64,
    total_sem: u64,
    total_msg: u64,
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

/// Initialise the IPC-namespace statistics state.
///
/// Starts with no namespaces and zero SHM/SEM/MSG totals. A namespace is added
/// through [`create_ns`] when the kernel actually creates a System V IPC
/// namespace, removed through [`destroy_ns`], and its per-namespace resource
/// counters advance only through real [`record_shm`] / [`record_sem`] /
/// [`record_msg`] calls. The `/proc/ipcns` generator and the `ipcns` kshell
/// command surface the namespace list (and [`ns_list`] / [`stats`]) as if it
/// reflects the real IPC-namespace layout and resource usage, so seeding it
/// with phantom namespaces would be fabricated procfs data — it would claim
/// containers and shared-memory segments exist when nothing created them.
///
/// (Previously this seeded two fictional namespaces — "init" (ns 1) with 50
/// shm segments / 500 MB, 20 sem sets / 200 sems, 10 msg queues / 1 MB, and
/// "container-1" (ns 2) with 10 shm / 100 MB, 5 sem sets / 50 sems, 3 msg
/// queues / 300 KB — plus global totals of 60 shm / 25 sem / 13 msg.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        namespaces: Vec::new(),
        next_id: 1,
        total_shm: 0,
        total_sem: 0,
        total_msg: 0,
        ops: 0,
    });
}

/// Create an IPC namespace.
pub fn create_ns(name: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.namespaces.len() >= MAX_NAMESPACES {
            return Err(KernelError::ResourceExhausted);
        }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        state.namespaces.push(IpcNamespace {
            ns_id: id,
            name: String::from(name),
            shm_segments: 0,
            shm_bytes: 0,
            sem_sets: 0,
            sem_total: 0,
            msg_queues: 0,
            msg_bytes: 0,
            created_ns: now,
        });
        Ok(id)
    })
}

/// Destroy an IPC namespace, returning everything it held to the global totals.
///
/// The globals are *live* counts — the sum of the per-namespace columns, which
/// is what `/proc/ipcns` prints them next to. Removing a namespace without
/// subtracting its rows would leave the header claiming segments that the table
/// underneath it no longer lists, which is exactly the disagreement this used
/// to have: `SHM: 1` above `Namespaces: 0`.
pub fn destroy_ns(ns_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state
            .namespaces
            .iter()
            .position(|n| n.ns_id == ns_id)
            .ok_or(KernelError::NotFound)?;
        let ns = state.namespaces.remove(idx);
        // Exact, not saturating: each global is the sum of the column it
        // totals, so the row being removed cannot exceed it. `saturating_sub`
        // here would silently absorb a skew rather than let it show up.
        state.total_shm -= ns.shm_segments;
        state.total_sem -= ns.sem_sets;
        state.total_msg -= ns.msg_queues;
        Ok(())
    })
}

/// Record a shared memory segment.
pub fn record_shm(ns_id: u32, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let ns = state
            .namespaces
            .iter_mut()
            .find(|n| n.ns_id == ns_id)
            .ok_or(KernelError::NotFound)?;
        ns.shm_segments += 1;
        ns.shm_bytes += bytes;
        state.total_shm += 1;
        Ok(())
    })
}

/// Record a semaphore set.
pub fn record_sem(ns_id: u32, count: u32) -> KernelResult<()> {
    with_state(|state| {
        let ns = state
            .namespaces
            .iter_mut()
            .find(|n| n.ns_id == ns_id)
            .ok_or(KernelError::NotFound)?;
        ns.sem_sets += 1;
        ns.sem_total += count as u64;
        state.total_sem += 1;
        Ok(())
    })
}

/// Record a message queue.
pub fn record_msg(ns_id: u32, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let ns = state
            .namespaces
            .iter_mut()
            .find(|n| n.ns_id == ns_id)
            .ok_or(KernelError::NotFound)?;
        ns.msg_queues += 1;
        ns.msg_bytes += bytes;
        state.total_msg += 1;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Inverses
// ---------------------------------------------------------------------------
//
// Every `record_*` above is `+=` into a running total. Until these existed the
// module published no way back: the only operation that removed anything was
// `destroy_ns`, which takes the whole namespace. So a segment recorded with the
// wrong size — a mistyped operand, a caller that double-counted — could not be
// corrected, only *added to*, and the repair for a one-character typo was to
// destroy the namespace and re-enter every other row in it.
//
// The three functions below are exact inverses, and they share one rule:
//
//   **A release that does not fit changes nothing.**
//
// Both halves of a row are checked before either is touched, and a release that
// would take more bytes than the namespace holds, or a set it does not have, is
// refused whole with `InvalidArgument`. The alternative — clamp with
// `saturating_sub` and carry on — is worse than the accumulator it replaces: it
// reports success, leaves the count and the byte total describing different
// histories, and the caller has no way to learn that its arithmetic was wrong.
// A refusal is a number the caller can act on; a clamp is a number that lies.

/// Release a shared-memory segment previously passed to [`record_shm`].
///
/// `bytes` must be the size the segment was recorded with. Refuses without
/// changing anything if the namespace holds no segments, or holds fewer than
/// `bytes` bytes across the ones it has.
pub fn release_shm(ns_id: u32, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let ns = state
            .namespaces
            .iter_mut()
            .find(|n| n.ns_id == ns_id)
            .ok_or(KernelError::NotFound)?;
        if ns.shm_segments == 0 || ns.shm_bytes < bytes {
            return Err(KernelError::InvalidArgument);
        }
        ns.shm_segments -= 1;
        ns.shm_bytes -= bytes;
        state.total_shm -= 1;
        Ok(())
    })
}

/// Release a semaphore set previously passed to [`record_sem`].
///
/// `count` must be the set's size. Refuses without changing anything if the
/// namespace holds no sets, or fewer than `count` semaphores across them.
pub fn release_sem(ns_id: u32, count: u32) -> KernelResult<()> {
    with_state(|state| {
        let ns = state
            .namespaces
            .iter_mut()
            .find(|n| n.ns_id == ns_id)
            .ok_or(KernelError::NotFound)?;
        let count = u64::from(count);
        if ns.sem_sets == 0 || ns.sem_total < count {
            return Err(KernelError::InvalidArgument);
        }
        ns.sem_sets -= 1;
        ns.sem_total -= count;
        state.total_sem -= 1;
        Ok(())
    })
}

/// Release a message queue previously passed to [`record_msg`].
///
/// `bytes` must be the size the queue was recorded with. Refuses without
/// changing anything if the namespace holds no queues, or fewer than `bytes`
/// bytes across the ones it has.
pub fn release_msg(ns_id: u32, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let ns = state
            .namespaces
            .iter_mut()
            .find(|n| n.ns_id == ns_id)
            .ok_or(KernelError::NotFound)?;
        if ns.msg_queues == 0 || ns.msg_bytes < bytes {
            return Err(KernelError::InvalidArgument);
        }
        ns.msg_queues -= 1;
        ns.msg_bytes -= bytes;
        state.total_msg -= 1;
        Ok(())
    })
}

/// List namespaces.
pub fn ns_list() -> Vec<IpcNamespace> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.namespaces.clone())
}

/// Get a specific namespace.
pub fn ns_info(ns_id: u32) -> Option<IpcNamespace> {
    STATE
        .lock()
        .as_ref()
        .and_then(|s| s.namespaces.iter().find(|n| n.ns_id == ns_id).cloned())
}

/// Statistics: (`ns_count`, `live_shm`, `live_sem`, `live_msg`, `ops`).
///
/// The three middle numbers are *live* counts of segments / semaphore sets /
/// message queues across every namespace — i.e. the column sums of [`ns_list`],
/// so the header of `/proc/ipcns` and the table under it always agree. Only
/// `ops` is cumulative (it counts state accesses, including failed ones).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.namespaces.len(),
            s.total_shm,
            s.total_sem,
            s.total_msg,
            s.ops,
        ),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("ipcns::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and no
    // fixtures leak into the live namespace table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no phantom namespaces, zero totals.
    assert_eq!(ns_list().len(), 0);
    let (c0, shm0, sem0, msg0, _) = stats();
    assert_eq!((c0, shm0, sem0, msg0), (0, 0, 0, 0));
    crate::serial_println!("  [1/10] empty defaults: OK");

    // 2: Create — first namespace gets id 1 and appears zeroed.
    let id = create_ns("test-ns").expect("create");
    assert_eq!(id, 1);
    assert_eq!(ns_list().len(), 1);
    let ns = ns_info(id).expect("info");
    assert_eq!(
        (
            ns.shm_segments,
            ns.shm_bytes,
            ns.sem_sets,
            ns.sem_total,
            ns.msg_queues,
            ns.msg_bytes
        ),
        (0, 0, 0, 0, 0, 0)
    );
    crate::serial_println!("  [2/10] create: OK");

    // 3: Shm — per-namespace and global SHM counters advance.
    record_shm(id, 4096).expect("shm");
    let ns = ns_info(id).expect("info3");
    assert_eq!((ns.shm_segments, ns.shm_bytes), (1, 4096));
    assert_eq!(stats().1, 1); // total_shm
    crate::serial_println!("  [3/10] shm: OK");

    // 4: Sem — per-namespace and global SEM counters advance.
    record_sem(id, 10).expect("sem");
    let ns = ns_info(id).expect("info4");
    assert_eq!((ns.sem_sets, ns.sem_total), (1, 10));
    assert_eq!(stats().2, 1); // total_sem
    crate::serial_println!("  [4/10] sem: OK");

    // 5: Msg — per-namespace and global MSG counters advance.
    record_msg(id, 256).expect("msg");
    let ns = ns_info(id).expect("info5");
    assert_eq!((ns.msg_queues, ns.msg_bytes), (1, 256));
    assert_eq!(stats().3, 1); // total_msg
    crate::serial_println!("  [5/10] msg: OK");

    // 6: Release is an exact inverse. Each `release_*` undoes the matching
    //    `record_*` in both columns and in the global, so the namespace and the
    //    totals come back to the zeroed state case 2 asserted. Before the
    //    inverses existed this was unreachable: the only way to remove a row
    //    was to destroy the whole namespace.
    release_shm(id, 4096).expect("release shm");
    release_sem(id, 10).expect("release sem");
    release_msg(id, 256).expect("release msg");
    let ns = ns_info(id).expect("info6");
    assert_eq!(
        (
            ns.shm_segments,
            ns.shm_bytes,
            ns.sem_sets,
            ns.sem_total,
            ns.msg_queues,
            ns.msg_bytes
        ),
        (0, 0, 0, 0, 0, 0)
    );
    let (_, shm6, sem6, msg6, _) = stats();
    assert_eq!((shm6, sem6, msg6), (0, 0, 0));
    crate::serial_println!("  [6/10] release inverts record: OK");

    // 7: A release that does not fit changes *nothing*. Two ways to not fit:
    //    releasing from an empty column, and releasing more bytes than the
    //    column holds. Both are refused whole with `InvalidArgument` — the
    //    assertion that matters is the read-back, which pins that the count was
    //    not decremented while the byte total was found to be too small. A
    //    `saturating_sub` implementation would pass the `Err` check and fail
    //    here, leaving `shm=0(100 B)`: a namespace with no segments and 100
    //    bytes in them.
    assert_eq!(release_shm(id, 1), Err(KernelError::InvalidArgument));
    assert_eq!(release_sem(id, 1), Err(KernelError::InvalidArgument));
    assert_eq!(release_msg(id, 1), Err(KernelError::InvalidArgument));
    record_shm(id, 100).expect("shm7");
    assert_eq!(release_shm(id, 101), Err(KernelError::InvalidArgument));
    let ns = ns_info(id).expect("info7");
    assert_eq!((ns.shm_segments, ns.shm_bytes), (1, 100));
    assert_eq!(stats().1, 1);
    crate::serial_println!("  [7/10] an unfitting release changes nothing: OK");

    // 8: Destroy hands the namespace's rows back to the globals. The namespace
    //    still holds one 100-byte segment from case 7, and `total_shm` must
    //    return to 0 with it — otherwise `/proc/ipcns` prints `SHM: 1` in its
    //    header above a table that lists no namespace holding it.
    destroy_ns(id).expect("destroy");
    assert_eq!(ns_list().len(), 0);
    let (_, shm8, sem8, msg8, _) = stats();
    assert_eq!((shm8, sem8, msg8), (0, 0, 0));
    assert_eq!(destroy_ns(id), Err(KernelError::NotFound));
    crate::serial_println!("  [8/10] destroy returns its rows to the globals: OK");

    // 9: Not found — recording into or releasing from an unknown namespace
    //    errors, and says the namespace is missing rather than the amount.
    assert_eq!(record_shm(999, 0), Err(KernelError::NotFound));
    assert_eq!(record_sem(999, 0), Err(KernelError::NotFound));
    assert_eq!(record_msg(999, 0), Err(KernelError::NotFound));
    assert_eq!(release_shm(999, 0), Err(KernelError::NotFound));
    assert_eq!(release_sem(999, 0), Err(KernelError::NotFound));
    assert_eq!(release_msg(999, 0), Err(KernelError::NotFound));
    crate::serial_println!("  [9/10] not found: OK");

    // 10: Final stats. Everything recorded above was released or destroyed, and
    //     the globals are live rather than cumulative, so all four are zero.
    //     `ops` is the one number that only ever climbs.
    let (nss, shm, sem, msg, ops) = stats();
    assert_eq!((nss, shm, sem, msg), (0, 0, 0, 0));
    assert!(ops > 0);
    crate::serial_println!("  [10/10] stats: OK");

    // Leave the table EMPTY, not DEAD: clear the fixtures, then re-open it.
    // Clearing alone would switch this module off for the rest of the boot
    // -- `init_defaults` runs once, that once is here, and every later write
    // would take the `NotSupported` arm and be dropped by a caller that must
    // not let statistics fail a real operation.  known-issues.md:
    // A-FS-ACCOUNTING-TABLES-ARE-CLOSED-FOR-THE-WHOLE-BOOT.
    *STATE.lock() = None;
    init_defaults();
    crate::serial_println!("ipcns::self_test() — all 10 tests passed");
}
