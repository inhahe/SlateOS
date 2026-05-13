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

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        addrs: alloc::vec![
            FutexAddr { address: 0x7FFF_0000_1000, waits: 500000, wakes: 499000, timeouts: 1000, requeues: 0, current_waiters: 2, max_waiters: 8, total_wait_ns: 50_000_000_000 },
            FutexAddr { address: 0x7FFF_0000_2000, waits: 100000, wakes: 100000, timeouts: 0, requeues: 500, current_waiters: 0, max_waiters: 4, total_wait_ns: 5_000_000_000 },
        ],
        procs: alloc::vec![
            ProcessFutexStats { pid: 1, total_waits: 300000, total_wakes: 200000, total_timeouts: 500, total_contention_ns: 30_000_000_000 },
            ProcessFutexStats { pid: 100, total_waits: 300000, total_wakes: 399000, total_timeouts: 500, total_contention_ns: 25_000_000_000 },
        ],
        total_waits: 600000,
        total_wakes: 599000,
        total_timeouts: 1000,
        total_requeues: 500,
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
        sorted.sort_by(|a, b| b.waits.cmp(&a.waits));
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(hotspots(10).len(), 2);
    assert_eq!(process_stats().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Record wait.
    record_wait(1, 0x7FFF_0000_1000).expect("wait");
    let top = hotspots(1);
    assert_eq!(top[0].address, 0x7FFF_0000_1000);
    assert_eq!(top[0].waits, 500001);
    crate::serial_println!("  [2/8] wait: OK");

    // 3: Record wake.
    record_wake(100, 0x7FFF_0000_1000, 1).expect("wake");
    let top = hotspots(1);
    assert_eq!(top[0].wakes, 499001);
    crate::serial_println!("  [3/8] wake: OK");

    // 4: Timeout.
    record_timeout(0x7FFF_0000_1000).expect("timeout");
    let (_, _, _, _, timeouts, _) = stats();
    assert!(timeouts > 1000);
    crate::serial_println!("  [4/8] timeout: OK");

    // 5: New address.
    record_wait(1, 0xDEAD_BEEF).expect("new_addr");
    assert_eq!(hotspots(10).len(), 3);
    crate::serial_println!("  [5/8] new address: OK");

    // 6: Contention.
    record_contention(1, 0x7FFF_0000_1000, 1_000_000).expect("contention");
    let ps = process_stats();
    let p1 = ps.iter().find(|p| p.pid == 1).unwrap();
    assert!(p1.total_contention_ns > 30_000_000_000);
    crate::serial_println!("  [6/8] contention: OK");

    // 7: Hotspots ordering.
    let top = hotspots(2);
    assert!(top[0].waits >= top[1].waits);
    crate::serial_println!("  [7/8] hotspot ordering: OK");

    // 8: Stats.
    let (addrs, procs, waits, wakes, _timeouts, ops) = stats();
    assert_eq!(addrs, 3);
    assert_eq!(procs, 2);
    assert!(waits > 600000);
    assert!(wakes > 599000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("futexstat::self_test() — all 8 tests passed");
}
