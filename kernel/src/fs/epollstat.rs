//! Epoll Statistics — event polling infrastructure monitoring.
//!
//! Tracks epoll instance creation, event registration, wait calls,
//! and event delivery. Supports monitoring the kernel's event
//! notification subsystem for I/O multiplexing.
//!
//! ## Architecture
//!
//! ```text
//! Epoll statistics
//!   → epollstat::create_instance(pid) → track epoll_create
//!   → epollstat::add_fd(instance, fd) → track epoll_ctl ADD
//!   → epollstat::wait(instance) → track epoll_wait
//!   → epollstat::deliver(instance, count) → track event delivery
//!
//! Integration:
//!   → fdtable (FD table management)
//!   → ipclog (IPC logging)
//!   → perfmon (performance monitor)
//!   → procstat (process statistics)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Epoll event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpollEvent {
    In,
    Out,
    Err,
    Hup,
    RdHup,
    Pri,
    Et,
}

impl EpollEvent {
    pub fn label(self) -> &'static str {
        match self {
            Self::In => "EPOLLIN",
            Self::Out => "EPOLLOUT",
            Self::Err => "EPOLLERR",
            Self::Hup => "EPOLLHUP",
            Self::RdHup => "EPOLLRDHUP",
            Self::Pri => "EPOLLPRI",
            Self::Et => "EPOLLET",
        }
    }
}

/// An epoll instance.
#[derive(Debug, Clone)]
pub struct EpollInstance {
    pub id: u32,
    pub owner_pid: u32,
    pub registered_fds: u32,
    pub max_events: u32,
    pub wait_calls: u64,
    pub events_delivered: u64,
    pub timeouts: u64,
    pub created_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_INSTANCES: usize = 512;

struct State {
    instances: Vec<EpollInstance>,
    next_id: u32,
    total_creates: u64,
    total_waits: u64,
    total_events: u64,
    total_timeouts: u64,
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

/// Initialise an **empty** epoll-instance table.
///
/// Seeds NO instance rows and zero totals.  Real epoll accounting is wired
/// through [`create_instance`]/[`destroy_instance`]/[`add_fd`]/[`remove_fd`]/
/// [`record_wait`]; until those are called the table is genuinely empty, so
/// the `/proc/epollstat` file and the `epollstat` kshell command report zeros
/// rather than fabricated numbers — the kernel's hard "never invent data in
/// procfs" rule.
///
/// NOTE: this previously seeded two fictional epoll instances (id 1/2 with
/// wait_calls 100000, events_delivered 250000) plus invented aggregate totals
/// (total_waits 150000, total_events 330000), which `/proc/epollstat` then
/// displayed as if they were real event-multiplexing statistics.  That demo
/// data was removed; the self-test now builds its own fixtures explicitly via
/// the real API (see [`self_test`]).  The epoll syscall path is expected to
/// call [`create_instance`] on epoll_create and [`record_wait`] on each
/// epoll_wait.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        instances: Vec::new(),
        next_id: 1,
        total_creates: 0,
        total_waits: 0,
        total_events: 0,
        total_timeouts: 0,
        ops: 0,
    });
}

/// Create an epoll instance.
pub fn create_instance(pid: u32, max_events: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.instances.len() >= MAX_INSTANCES { return Err(KernelError::ResourceExhausted); }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        state.instances.push(EpollInstance {
            id, owner_pid: pid, registered_fds: 0, max_events,
            wait_calls: 0, events_delivered: 0, timeouts: 0, created_ns: now,
        });
        state.total_creates += 1;
        Ok(id)
    })
}

/// Destroy an epoll instance.
pub fn destroy_instance(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.instances.iter().position(|i| i.id == id)
            .ok_or(KernelError::NotFound)?;
        state.instances.remove(idx);
        Ok(())
    })
}

/// Add a file descriptor to an epoll instance.
pub fn add_fd(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let inst = state.instances.iter_mut().find(|i| i.id == id)
            .ok_or(KernelError::NotFound)?;
        inst.registered_fds += 1;
        Ok(())
    })
}

/// Remove a file descriptor from an epoll instance.
pub fn remove_fd(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let inst = state.instances.iter_mut().find(|i| i.id == id)
            .ok_or(KernelError::NotFound)?;
        inst.registered_fds = inst.registered_fds.saturating_sub(1);
        Ok(())
    })
}

/// Record an epoll_wait call.
pub fn record_wait(id: u32, events_returned: u32, timed_out: bool) -> KernelResult<()> {
    with_state(|state| {
        let inst = state.instances.iter_mut().find(|i| i.id == id)
            .ok_or(KernelError::NotFound)?;
        inst.wait_calls += 1;
        inst.events_delivered += events_returned as u64;
        state.total_waits += 1;
        state.total_events += events_returned as u64;
        if timed_out {
            inst.timeouts += 1;
            state.total_timeouts += 1;
        }
        Ok(())
    })
}

/// Get all instances.
pub fn list_instances() -> Vec<EpollInstance> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.instances.clone())
}

/// Get instances for a specific PID.
pub fn instances_for_pid(pid: u32) -> Vec<EpollInstance> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.instances.iter().filter(|i| i.owner_pid == pid).cloned().collect()
    })
}

/// Statistics: (instance_count, total_creates, total_waits, total_events, total_timeouts, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.instances.len(), s.total_creates, s.total_waits, s.total_events, s.total_timeouts, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("epollstat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/epollstat must never surface).
    // Resetting first clears any residue from a prior `epollstat test` run so
    // the totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated rows.
    assert_eq!(list_instances().len(), 0);
    let (c0, cr0, w0, e0, t0, _o0) = stats();
    assert_eq!((c0, cr0, w0, e0, t0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Create instance (ids start at 1).
    let id = create_instance(200, 64).expect("create");
    assert_eq!(id, 1);
    assert_eq!(list_instances().len(), 1);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Add FDs (exact, from zero).
    add_fd(id).expect("add");
    add_fd(id).expect("add2");
    let inst = list_instances().iter().find(|i| i.id == id).cloned().expect("inst");
    assert_eq!(inst.registered_fds, 2);
    assert!(add_fd(9999).is_err()); // NotFound on unknown instance
    crate::serial_println!("  [3/8] add fd: OK");

    // 4: Remove FD.
    remove_fd(id).expect("remove");
    let inst = list_instances().iter().find(|i| i.id == id).cloned().expect("inst");
    assert_eq!(inst.registered_fds, 1);
    crate::serial_println!("  [4/8] remove fd: OK");

    // 5: Record wait (delivers 3 events).
    record_wait(id, 3, false).expect("wait");
    let inst = list_instances().iter().find(|i| i.id == id).cloned().expect("inst");
    assert_eq!(inst.wait_calls, 1);
    assert_eq!(inst.events_delivered, 3);
    crate::serial_println!("  [5/8] wait: OK");

    // 6: A timed-out wait bumps the timeout counter exactly.
    record_wait(id, 0, true).expect("timeout");
    let inst = list_instances().iter().find(|i| i.id == id).cloned().expect("inst");
    assert_eq!(inst.timeouts, 1);
    assert_eq!(inst.wait_calls, 2);
    crate::serial_println!("  [6/8] timeout: OK");

    // 7: A second instance for a different pid; instances_for_pid is exact.
    let id2 = create_instance(201, 128).expect("create2");
    assert_eq!(instances_for_pid(200).len(), 1);
    assert_eq!(instances_for_pid(201).len(), 1);
    destroy_instance(id).expect("destroy");
    assert_eq!(list_instances().len(), 1); // only id2 remains
    assert!(destroy_instance(id).is_err());
    let _ = id2;
    crate::serial_println!("  [7/8] destroy: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (count, creates, waits, events, timeouts, ops) = stats();
    assert_eq!(count, 1); // id2
    assert_eq!(creates, 2); // two create_instance calls
    assert_eq!(waits, 2); // two record_wait calls
    assert_eq!(events, 3); // 3 + 0
    assert_eq!(timeouts, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/epollstat table with its fixtures.  Reset to the uninitialised
    // state so production reads report an empty table until the epoll syscall
    // path wires real accounting.
    *STATE.lock() = None;

    crate::serial_println!("epollstat::self_test() — all 8 tests passed");
}
