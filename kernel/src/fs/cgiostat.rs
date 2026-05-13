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

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cgroups: alloc::vec![
            CgroupIoStats { cg_id: 1, name: String::from("root"), read_bytes: 50_000_000_000, write_bytes: 30_000_000_000, read_ios: 5_000_000, write_ios: 3_000_000, throttle_count: 0, io_wait_ns: 100_000_000_000, bw_limit_bps: 0, iops_limit: 0 },
            CgroupIoStats { cg_id: 2, name: String::from("system.slice"), read_bytes: 20_000_000_000, write_bytes: 15_000_000_000, read_ios: 2_000_000, write_ios: 1_500_000, throttle_count: 500, io_wait_ns: 50_000_000_000, bw_limit_bps: 100_000_000, iops_limit: 10000 },
            CgroupIoStats { cg_id: 3, name: String::from("user.slice"), read_bytes: 10_000_000_000, write_bytes: 8_000_000_000, read_ios: 1_000_000, write_ios: 800_000, throttle_count: 200, io_wait_ns: 30_000_000_000, bw_limit_bps: 200_000_000, iops_limit: 20000 },
        ],
        next_id: 4,
        total_read_bytes: 80_000_000_000,
        total_write_bytes: 53_000_000_000,
        total_read_ios: 8_000_000,
        total_write_ios: 5_300_000,
        total_throttles: 700,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_cgroup().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Create.
    let id = create_cgroup("test", 50_000_000, 5000).expect("create");
    assert!(id >= 4);
    assert_eq!(per_cgroup().len(), 4);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Read.
    record_read(id, 4096).expect("read");
    let cg = per_cgroup().iter().find(|c| c.cg_id == id).cloned().unwrap();
    assert_eq!(cg.read_bytes, 4096);
    assert_eq!(cg.read_ios, 1);
    crate::serial_println!("  [3/8] read: OK");

    // 4: Write.
    record_write(id, 8192).expect("write");
    let cg = per_cgroup().iter().find(|c| c.cg_id == id).cloned().unwrap();
    assert_eq!(cg.write_bytes, 8192);
    assert_eq!(cg.write_ios, 1);
    crate::serial_println!("  [4/8] write: OK");

    // 5: Throttle.
    record_throttle(id).expect("throttle");
    let cg = per_cgroup().iter().find(|c| c.cg_id == id).cloned().unwrap();
    assert_eq!(cg.throttle_count, 1);
    crate::serial_println!("  [5/8] throttle: OK");

    // 6: I/O wait.
    record_io_wait(id, 100_000).expect("io_wait");
    let cg = per_cgroup().iter().find(|c| c.cg_id == id).cloned().unwrap();
    assert_eq!(cg.io_wait_ns, 100_000);
    crate::serial_println!("  [6/8] io wait: OK");

    // 7: Remove.
    remove_cgroup(id).expect("remove");
    assert_eq!(per_cgroup().len(), 3);
    assert!(remove_cgroup(id).is_err());
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Stats.
    let (cgs, rbytes, wbytes, throttles, ops) = stats();
    assert_eq!(cgs, 3);
    assert!(rbytes > 80_000_000_000);
    assert!(wbytes > 53_000_000_000);
    assert!(throttles > 700);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("cgiostat::self_test() — all 8 tests passed");
}
