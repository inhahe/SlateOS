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
//!   → ipcns::ns_list() → list namespaces
//!
//! Integration:
//!   → shmem (shared memory)
//!   → pidstat (PID namespaces)
//!   → prociso (process isolation)
//!   → cgroupfs (cgroup filesystem)
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
    if guard.is_some() { return; }
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
        if state.namespaces.len() >= MAX_NAMESPACES { return Err(KernelError::ResourceExhausted); }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        state.namespaces.push(IpcNamespace {
            ns_id: id, name: String::from(name),
            shm_segments: 0, shm_bytes: 0, sem_sets: 0, sem_total: 0,
            msg_queues: 0, msg_bytes: 0, created_ns: now,
        });
        Ok(id)
    })
}

/// Destroy an IPC namespace.
pub fn destroy_ns(ns_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.namespaces.iter().position(|n| n.ns_id == ns_id)
            .ok_or(KernelError::NotFound)?;
        state.namespaces.remove(idx);
        Ok(())
    })
}

/// Record a shared memory segment.
pub fn record_shm(ns_id: u32, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let ns = state.namespaces.iter_mut().find(|n| n.ns_id == ns_id)
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
        let ns = state.namespaces.iter_mut().find(|n| n.ns_id == ns_id)
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
        let ns = state.namespaces.iter_mut().find(|n| n.ns_id == ns_id)
            .ok_or(KernelError::NotFound)?;
        ns.msg_queues += 1;
        ns.msg_bytes += bytes;
        state.total_msg += 1;
        Ok(())
    })
}

/// List namespaces.
pub fn ns_list() -> Vec<IpcNamespace> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.namespaces.clone())
}

/// Get a specific namespace.
pub fn ns_info(ns_id: u32) -> Option<IpcNamespace> {
    STATE.lock().as_ref().and_then(|s| {
        s.namespaces.iter().find(|n| n.ns_id == ns_id).cloned()
    })
}

/// Statistics: (ns_count, total_shm, total_sem, total_msg, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.namespaces.len(), s.total_shm, s.total_sem, s.total_msg, s.ops),
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
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Create — first namespace gets id 1 and appears zeroed.
    let id = create_ns("test-ns").expect("create");
    assert_eq!(id, 1);
    assert_eq!(ns_list().len(), 1);
    let ns = ns_info(id).expect("info");
    assert_eq!((ns.shm_segments, ns.shm_bytes, ns.sem_sets, ns.sem_total, ns.msg_queues, ns.msg_bytes), (0, 0, 0, 0, 0, 0));
    crate::serial_println!("  [2/8] create: OK");

    // 3: Shm — per-namespace and global SHM counters advance.
    record_shm(id, 4096).expect("shm");
    let ns = ns_info(id).expect("info3");
    assert_eq!((ns.shm_segments, ns.shm_bytes), (1, 4096));
    assert_eq!(stats().1, 1); // total_shm
    crate::serial_println!("  [3/8] shm: OK");

    // 4: Sem — per-namespace and global SEM counters advance.
    record_sem(id, 10).expect("sem");
    let ns = ns_info(id).expect("info4");
    assert_eq!((ns.sem_sets, ns.sem_total), (1, 10));
    assert_eq!(stats().2, 1); // total_sem
    crate::serial_println!("  [4/8] sem: OK");

    // 5: Msg — per-namespace and global MSG counters advance.
    record_msg(id, 256).expect("msg");
    let ns = ns_info(id).expect("info5");
    assert_eq!((ns.msg_queues, ns.msg_bytes), (1, 256));
    assert_eq!(stats().3, 1); // total_msg
    crate::serial_println!("  [5/8] msg: OK");

    // 6: Destroy — namespace disappears; double destroy is NotFound.
    destroy_ns(id).expect("destroy");
    assert_eq!(ns_list().len(), 0);
    assert!(destroy_ns(id).is_err());
    crate::serial_println!("  [6/8] destroy: OK");

    // 7: Not found — recording into an unknown namespace errors.
    assert!(record_shm(999, 0).is_err());
    assert!(record_sem(999, 0).is_err());
    assert!(record_msg(999, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Final stats reflect only the real activity above. Global totals are
    //    cumulative and not decremented on destroy: 0 namespaces, 1 shm / 1 sem
    //    / 1 msg recorded.
    let (nss, shm, sem, msg, ops) = stats();
    assert_eq!((nss, shm, sem, msg), (0, 1, 1, 1));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("ipcns::self_test() — all 8 tests passed");
}
