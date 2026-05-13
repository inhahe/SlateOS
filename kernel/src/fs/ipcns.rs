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

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        namespaces: alloc::vec![
            IpcNamespace { ns_id: 1, name: String::from("init"), shm_segments: 50, shm_bytes: 500_000_000, sem_sets: 20, sem_total: 200, msg_queues: 10, msg_bytes: 1_000_000, created_ns: now },
            IpcNamespace { ns_id: 2, name: String::from("container-1"), shm_segments: 10, shm_bytes: 100_000_000, sem_sets: 5, sem_total: 50, msg_queues: 3, msg_bytes: 300_000, created_ns: now },
        ],
        next_id: 3,
        total_shm: 60,
        total_sem: 25,
        total_msg: 13,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(ns_list().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Create.
    let id = create_ns("test-ns").expect("create");
    assert!(id >= 3);
    assert_eq!(ns_list().len(), 3);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Shm.
    record_shm(id, 4096).expect("shm");
    let ns = ns_info(id).unwrap();
    assert_eq!(ns.shm_segments, 1);
    assert_eq!(ns.shm_bytes, 4096);
    crate::serial_println!("  [3/8] shm: OK");

    // 4: Sem.
    record_sem(id, 10).expect("sem");
    let ns = ns_info(id).unwrap();
    assert_eq!(ns.sem_sets, 1);
    assert_eq!(ns.sem_total, 10);
    crate::serial_println!("  [4/8] sem: OK");

    // 5: Msg.
    record_msg(id, 256).expect("msg");
    let ns = ns_info(id).unwrap();
    assert_eq!(ns.msg_queues, 1);
    assert_eq!(ns.msg_bytes, 256);
    crate::serial_println!("  [5/8] msg: OK");

    // 6: Destroy.
    destroy_ns(id).expect("destroy");
    assert_eq!(ns_list().len(), 2);
    assert!(destroy_ns(id).is_err());
    crate::serial_println!("  [6/8] destroy: OK");

    // 7: Not found.
    assert!(record_shm(999, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (nss, shm, sem, msg, ops) = stats();
    assert_eq!(nss, 2);
    assert!(shm > 60);
    assert!(sem > 25);
    assert!(msg > 13);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("ipcns::self_test() — all 8 tests passed");
}
