//! Futex Statistics — fast userspace mutex monitoring.
//!
//! Tracks futex wait/wake operations, contention events,
//! and timeout statistics. Essential for diagnosing lock
//! contention in userspace applications.
//!
//! ## Architecture
//!
//! ```text
//! Futex statistics
//!   → futexstat::record_wait(addr) → track futex_wait
//!   → futexstat::record_wake(addr, n) → track futex_wake
//!   → futexstat::record_timeout(addr) → track wait timeout
//!   → futexstat::hotspots() → most contended futexes
//!
//! Integration:
//!   → ipclog (IPC logging)
//!   → procstat (process statistics)
//!   → perfmon (performance monitor)
//!   → tracemon (trace monitor)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Futex operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FutexOp {
    Wait,
    Wake,
    WaitBitset,
    WakeBitset,
    Requeue,
    CmpRequeue,
}

impl FutexOp {
    pub fn label(self) -> &'static str {
        match self {
            Self::Wait => "WAIT",
            Self::Wake => "WAKE",
            Self::WaitBitset => "WAIT_BITSET",
            Self::WakeBitset => "WAKE_BITSET",
            Self::Requeue => "REQUEUE",
            Self::CmpRequeue => "CMP_REQUEUE",
        }
    }
}

/// Per-futex address statistics.
#[derive(Debug, Clone)]
pub struct FutexAddr {
    pub address: u64,
    pub waits: u64,
    pub wakes: u64,
    pub timeouts: u64,
    pub requeues: u64,
    pub current_waiters: u32,
    pub max_waiters: u32,
    pub total_wait_ns: u64,
}

/// Per-process futex stats.
#[derive(Debug, Clone)]
pub struct ProcessFutexStats {
    pub pid: u32,
    pub total_waits: u64,
    pub total_wakes: u64,
    pub total_timeouts: u64,
    pub total_contention_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ADDRS: usize = 512;
const MAX_PROCS: usize = 256;

struct State {
    addrs: Vec<FutexAddr>,
    procs: Vec<ProcessFutexStats>,
    total_waits: u64,
    total_wakes: u64,
    total_timeouts: u64,
    total_requeues: u64,
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

/// Initialise an **empty** futex-statistics table.
///
/// Seeds NO address or per-process rows and zero totals.  Real accounting is
/// wired through [`record_wait`]/[`record_wake`]/[`record_timeout`]/
/// [`record_contention`]; until those are called the table is genuinely
/// empty, so the `/proc/futexstat` file and the `futexstat` kshell command
/// report zeros rather than fabricated numbers — the kernel's hard "never
/// invent data in procfs" rule.
///
/// NOTE: this previously seeded two fictional futex addresses (e.g.
/// 0x7FFF_0000_1000 with waits 500000, total_wait_ns 50_000_000_000) and two
/// fictional per-process rows (pid 1/100) plus invented aggregate totals
/// (total_waits 600000), which `/proc/futexstat` then displayed as if they
/// were real lock-contention statistics.  That demo data was removed; the
/// self-test now builds its own fixtures explicitly via the real API (see
/// [`self_test`]).  The futex syscall path is expected to call
/// [`record_wait`]/[`record_wake`] as userspace mutexes block and wake.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        addrs: Vec::new(),
        procs: Vec::new(),
        total_waits: 0,
        total_wakes: 0,
        total_timeouts: 0,
        total_requeues: 0,
        ops: 0,
    });
}

/// Record a futex wait.
pub fn record_wait(pid: u32, address: u64) -> KernelResult<()> {
    with_state(|state| {
        // Update address stats.
        let addr = if let Some(a) = state.addrs.iter_mut().find(|a| a.address == address) {
            a
        } else {
            if state.addrs.len() >= MAX_ADDRS { return Err(KernelError::ResourceExhausted); }
            state.addrs.push(FutexAddr {
                address, waits: 0, wakes: 0, timeouts: 0, requeues: 0,
                current_waiters: 0, max_waiters: 0, total_wait_ns: 0,
            });
            state.addrs.last_mut().ok_or(KernelError::InternalError)?
        };
        addr.waits += 1;
        addr.current_waiters += 1;
        if addr.current_waiters > addr.max_waiters {
            addr.max_waiters = addr.current_waiters;
        }
        // Update process stats.
        if let Some(p) = state.procs.iter_mut().find(|p| p.pid == pid) {
            p.total_waits += 1;
        } else if state.procs.len() < MAX_PROCS {
            state.procs.push(ProcessFutexStats {
                pid, total_waits: 1, total_wakes: 0, total_timeouts: 0, total_contention_ns: 0,
            });
        }
        state.total_waits += 1;
        Ok(())
    })
}

/// Record a futex wake.
pub fn record_wake(pid: u32, address: u64, count: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(addr) = state.addrs.iter_mut().find(|a| a.address == address) {
            addr.wakes += count as u64;
            addr.current_waiters = addr.current_waiters.saturating_sub(count);
        }
        if let Some(p) = state.procs.iter_mut().find(|p| p.pid == pid) {
            p.total_wakes += count as u64;
        }
        state.total_wakes += count as u64;
        Ok(())
    })
}

/// Record a futex wait timeout.
pub fn record_timeout(address: u64) -> KernelResult<()> {
    with_state(|state| {
        if let Some(addr) = state.addrs.iter_mut().find(|a| a.address == address) {
            addr.timeouts += 1;
            addr.current_waiters = addr.current_waiters.saturating_sub(1);
        }
        state.total_timeouts += 1;
        Ok(())
    })
}

/// Record wait contention time.
pub fn record_contention(pid: u32, address: u64, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        if let Some(addr) = state.addrs.iter_mut().find(|a| a.address == address) {
            addr.total_wait_ns += ns;
        }
        if let Some(p) = state.procs.iter_mut().find(|p| p.pid == pid) {
            p.total_contention_ns += ns;
        }
        Ok(())
    })
}

/// Top contended futex addresses.
pub fn hotspots(n: usize) -> Vec<FutexAddr> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut sorted: Vec<_> = s.addrs.clone();
        sorted.sort_by_key(|e| core::cmp::Reverse(e.waits));
        sorted.truncate(n);
        sorted
    })
}

/// Per-process stats.
pub fn process_stats() -> Vec<ProcessFutexStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.procs.clone())
}

/// Statistics: (addr_count, proc_count, total_waits, total_wakes, total_timeouts, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.addrs.len(), s.procs.len(), s.total_waits, s.total_wakes, s.total_timeouts, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("futexstat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/futexstat must never surface).
    // Resetting first clears any residue from a prior `futexstat test` run so
    // the totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    const ADDR_A: u64 = 0x7FFF_0000_1000;
    const ADDR_B: u64 = 0xDEAD_BEEF;

    // 1: Empty after init — no fabricated rows.
    assert_eq!(hotspots(10).len(), 0);
    assert_eq!(process_stats().len(), 0);
    let (a0, p0, w0, k0, t0, _o0) = stats();
    assert_eq!((a0, p0, w0, k0, t0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: record_wait auto-creates the address + process rows (exact, from 0).
    record_wait(1, ADDR_A).expect("wait");
    let top = hotspots(1);
    assert_eq!(top[0].address, ADDR_A);
    assert_eq!(top[0].waits, 1);
    assert_eq!(top[0].current_waiters, 1);
    assert_eq!(process_stats().len(), 1);
    crate::serial_println!("  [2/8] wait: OK");

    // 3: record_wake bumps wakes and clears the waiter.
    record_wake(1, ADDR_A, 1).expect("wake");
    let top = hotspots(1);
    assert_eq!(top[0].wakes, 1);
    assert_eq!(top[0].current_waiters, 0);
    crate::serial_println!("  [3/8] wake: OK");

    // 4: Timeout increments the aggregate count exactly.
    record_timeout(ADDR_A).expect("timeout");
    let (_, _, _, _, timeouts, _) = stats();
    assert_eq!(timeouts, 1);
    crate::serial_println!("  [4/8] timeout: OK");

    // 5: A wait on a new address creates a second row.
    record_wait(1, ADDR_B).expect("new_addr");
    assert_eq!(hotspots(10).len(), 2);
    crate::serial_println!("  [5/8] new address: OK");

    // 6: Contention time accrues to the process row (exact, from 0).
    record_contention(1, ADDR_A, 1_000_000).expect("contention");
    let ps = process_stats();
    let p1 = ps.iter().find(|p| p.pid == 1).expect("pid 1");
    assert_eq!(p1.total_contention_ns, 1_000_000);
    crate::serial_println!("  [6/8] contention: OK");

    // 7: A second waiter on ADDR_A (new pid 2) makes it the top hotspot.
    record_wait(2, ADDR_A).expect("wait2");
    let top = hotspots(2);
    assert_eq!(top[0].address, ADDR_A);
    assert_eq!(top[0].waits, 2);
    assert!(top[0].waits >= top[1].waits);
    crate::serial_println!("  [7/8] hotspot ordering: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (addrs, procs, waits, wakes, _timeouts, ops) = stats();
    assert_eq!(addrs, 2); // ADDR_A + ADDR_B
    assert_eq!(procs, 2); // pid 1 + pid 2
    assert_eq!(waits, 3); // wait(1,A) + wait(1,B) + wait(2,A)
    assert_eq!(wakes, 1); // wake(1,A,1)
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/futexstat table with its fixtures.  Reset to the uninitialised
    // state so production reads report an empty table until the futex syscall
    // path wires real accounting.
    *STATE.lock() = None;

    crate::serial_println!("futexstat::self_test() — all 8 tests passed");
}
