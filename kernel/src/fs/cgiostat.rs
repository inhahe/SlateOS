//! Cgroup I/O Statistics — per-cgroup disk I/O accounting.
//!
//! Tracks read/write bytes and IOPS per cgroup, bandwidth
//! throttling events, and I/O wait time. Essential for
//! container workload isolation and resource control.
//!
//! ## Architecture
//!
//! ```text
//! Cgroup I/O monitoring
//!   → cgiostat::record_read(cg_id, bytes) → track read I/O
//!   → cgiostat::record_write(cg_id, bytes) → track write I/O
//!   → cgiostat::record_throttle(cg_id) → bandwidth limit hit
//!   → cgiostat::per_cgroup() → per-cgroup I/O stats
//!
//! Integration:
//!   → memcg (memory cgroup)
//!   → iosched (I/O scheduler)
//!   → blkqueue (block I/O queue)
//!   → taskstats (per-task accounting)
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

/// Per-cgroup I/O stats.
#[derive(Debug, Clone)]
pub struct CgroupIoStats {
    pub cg_id: u32,
    pub name: String,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_ios: u64,
    pub write_ios: u64,
    pub throttle_count: u64,
    pub io_wait_ns: u64,
    pub bw_limit_bps: u64,   // 0 = unlimited
    pub iops_limit: u64,     // 0 = unlimited
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CGROUPS: usize = 256;

struct State {
    cgroups: Vec<CgroupIoStats>,
    next_id: u32,
    total_read_bytes: u64,
    total_write_bytes: u64,
    total_read_ios: u64,
    total_write_ios: u64,
    total_throttles: u64,
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

/// Initialise an **empty** cgroup I/O statistics table.
///
/// Seeds NO cgroups and zero totals.  Real per-cgroup I/O accounting is wired
/// through [`create_cgroup`] (one row per cgroup the controller creates) and
/// the `record_read`/`record_write`/`record_throttle`/`record_io_wait`
/// functions; until those are called the table is genuinely empty, so the
/// `/proc/cgiostat` file and the `cgiostat` kshell command report zeros rather
/// than fabricated numbers — the kernel's hard "never invent data in procfs"
/// rule.
///
/// NOTE: this previously seeded three fictional cgroups ("root" read 50GB /
/// write 30GB; "system.slice" read 20GB / write 15GB / 500 throttles;
/// "user.slice" read 10GB / write 8GB / 200 throttles) plus invented aggregate
/// totals (total_read_bytes 80GB, total_write_bytes 53GB, total_read_ios 8M,
/// total_write_ios 5.3M, total_throttles 700), which `/proc/cgiostat` then
/// displayed as if they were real per-cgroup I/O measurements.  That demo data
/// was removed; the self-test now builds its own fixtures explicitly via the
/// real API (see [`self_test`]).  The cgroup I/O controller is expected to call
/// [`create_cgroup`] when a cgroup is created and the record_* functions as I/O
/// flows through it.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cgroups: Vec::new(),
        next_id: 1,
        total_read_bytes: 0,
        total_write_bytes: 0,
        total_read_ios: 0,
        total_write_ios: 0,
        total_throttles: 0,
        ops: 0,
    });
}

/// Create a cgroup.
pub fn create_cgroup(name: &str, bw_limit: u64, iops_limit: u64) -> KernelResult<u32> {
    with_state(|state| {
        if state.cgroups.len() >= MAX_CGROUPS { return Err(KernelError::ResourceExhausted); }
        let id = state.next_id;
        state.next_id += 1;
        state.cgroups.push(CgroupIoStats {
            cg_id: id, name: String::from(name),
            read_bytes: 0, write_bytes: 0, read_ios: 0, write_ios: 0,
            throttle_count: 0, io_wait_ns: 0, bw_limit_bps: bw_limit, iops_limit,
        });
        Ok(id)
    })
}

/// Remove a cgroup.
pub fn remove_cgroup(cg_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.cgroups.iter().position(|c| c.cg_id == cg_id)
            .ok_or(KernelError::NotFound)?;
        state.cgroups.remove(idx);
        Ok(())
    })
}

/// Record a read I/O.
pub fn record_read(cg_id: u32, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let cg = state.cgroups.iter_mut().find(|c| c.cg_id == cg_id)
            .ok_or(KernelError::NotFound)?;
        cg.read_bytes += bytes;
        cg.read_ios += 1;
        state.total_read_bytes += bytes;
        state.total_read_ios += 1;
        Ok(())
    })
}

/// Record a write I/O.
pub fn record_write(cg_id: u32, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let cg = state.cgroups.iter_mut().find(|c| c.cg_id == cg_id)
            .ok_or(KernelError::NotFound)?;
        cg.write_bytes += bytes;
        cg.write_ios += 1;
        state.total_write_bytes += bytes;
        state.total_write_ios += 1;
        Ok(())
    })
}

/// Record a throttle event.
pub fn record_throttle(cg_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let cg = state.cgroups.iter_mut().find(|c| c.cg_id == cg_id)
            .ok_or(KernelError::NotFound)?;
        cg.throttle_count += 1;
        state.total_throttles += 1;
        Ok(())
    })
}

/// Record I/O wait time.
pub fn record_io_wait(cg_id: u32, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let cg = state.cgroups.iter_mut().find(|c| c.cg_id == cg_id)
            .ok_or(KernelError::NotFound)?;
        cg.io_wait_ns += ns;
        Ok(())
    })
}

/// Set bandwidth limit.
pub fn set_bw_limit(cg_id: u32, bps: u64) -> KernelResult<()> {
    with_state(|state| {
        let cg = state.cgroups.iter_mut().find(|c| c.cg_id == cg_id)
            .ok_or(KernelError::NotFound)?;
        cg.bw_limit_bps = bps;
        Ok(())
    })
}

/// Per-cgroup stats.
pub fn per_cgroup() -> Vec<CgroupIoStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cgroups.clone())
}

/// Statistics: (cgroup_count, total_read_bytes, total_write_bytes, total_throttles, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.cgroups.len(), s.total_read_bytes, s.total_write_bytes, s.total_throttles, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("cgiostat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/cgiostat must never surface).
    // Resetting first clears any residue from a prior `cgiostat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated cgroups or totals.
    assert_eq!(per_cgroup().len(), 0);
    let (c0, rb0, wb0, t0, _o0) = stats();
    assert_eq!((c0, rb0, wb0, t0), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Create cgroups — ids start at 1 and increment; limits preserved.
    let id1 = create_cgroup("test", 50_000_000, 5000).expect("create1");
    let id2 = create_cgroup("other", 0, 0).expect("create2");
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(per_cgroup().len(), 2);
    let cg = per_cgroup().iter().find(|c| c.cg_id == id1).cloned().expect("cg");
    assert_eq!(cg.bw_limit_bps, 50_000_000);
    assert_eq!(cg.iops_limit, 5000);
    assert_eq!(cg.read_bytes, 0);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Read records bytes + IO count exactly from zero.
    record_read(id1, 4096).expect("read");
    let cg = per_cgroup().iter().find(|c| c.cg_id == id1).cloned().expect("cg");
    assert_eq!(cg.read_bytes, 4096);
    assert_eq!(cg.read_ios, 1);
    assert!(record_read(9999, 1).is_err()); // unknown cgroup
    crate::serial_println!("  [3/8] read: OK");

    // 4: Write records bytes + IO count exactly from zero.
    record_write(id1, 8192).expect("write");
    let cg = per_cgroup().iter().find(|c| c.cg_id == id1).cloned().expect("cg");
    assert_eq!(cg.write_bytes, 8192);
    assert_eq!(cg.write_ios, 1);
    crate::serial_println!("  [4/8] write: OK");

    // 5: Throttle increments exactly from zero.
    record_throttle(id1).expect("throttle");
    let cg = per_cgroup().iter().find(|c| c.cg_id == id1).cloned().expect("cg");
    assert_eq!(cg.throttle_count, 1);
    crate::serial_println!("  [5/8] throttle: OK");

    // 6: I/O wait accumulates exactly from zero.
    record_io_wait(id1, 100_000).expect("io_wait");
    let cg = per_cgroup().iter().find(|c| c.cg_id == id1).cloned().expect("cg");
    assert_eq!(cg.io_wait_ns, 100_000);
    crate::serial_println!("  [6/8] io wait: OK");

    // 7: Remove drops the cgroup; removing again fails with NotFound.
    remove_cgroup(id1).expect("remove");
    assert_eq!(per_cgroup().len(), 1);
    assert!(remove_cgroup(id1).is_err());
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (cgs, rbytes, wbytes, throttles, ops) = stats();
    assert_eq!(cgs, 1);
    assert_eq!(rbytes, 4096);   // one read
    assert_eq!(wbytes, 8192);   // one write
    assert_eq!(throttles, 1);   // one throttle
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/cgiostat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the cgroup I/O controller
    // wires real accounting.
    *STATE.lock() = None;

    crate::serial_println!("cgiostat::self_test() — all 8 tests passed");
}
