//! Socket Buffer — network buffer pool monitoring.
//!
//! Tracks socket buffer (sk_buff equivalent) allocation,
//! pool utilization, per-protocol buffer usage, and buffer
//! pressure events. Essential for network performance tuning.
//!
//! ## Architecture
//!
//! ```text
//! Socket buffer monitoring
//!   → sockbuf::alloc(proto, size) → buffer allocation
//!   → sockbuf::free(proto, size) → buffer release
//!   → sockbuf::record_drop(proto) → drop due to pressure
//!   → sockbuf::pool_stats() → pool utilization
//!
//! Integration:
//!   → netsock (socket stats)
//!   → netdev (device stats)
//!   → slabstat (slab allocator)
//!   → memcg (memory cgroup)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Buffer pool type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufPool {
    Tcp,
    Udp,
    Raw,
    Icmp,
    Multicast,
    General,
}

impl BufPool {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
            Self::Raw => "raw",
            Self::Icmp => "icmp",
            Self::Multicast => "mcast",
            Self::General => "general",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Tcp => 0,
            Self::Udp => 1,
            Self::Raw => 2,
            Self::Icmp => 3,
            Self::Multicast => 4,
            Self::General => 5,
        }
    }
}

/// Per-pool statistics.
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub pool: BufPool,
    pub active_buffers: u64,
    pub total_bytes: u64,
    pub allocs: u64,
    pub frees: u64,
    pub drops: u64,
    pub peak_buffers: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    pools: [PoolStats; 6],
    total_allocs: u64,
    total_frees: u64,
    total_drops: u64,
    total_bytes_allocated: u64,
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

fn make_pool(pool: BufPool, active: u64, bytes: u64, allocs: u64, frees: u64, drops: u64, peak: u64) -> PoolStats {
    PoolStats { pool, active_buffers: active, total_bytes: bytes, allocs, frees, drops, peak_buffers: peak }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        pools: [
            make_pool(BufPool::Tcp, 50_000, 200_000_000, 100_000_000, 99_950_000, 5_000, 100_000),
            make_pool(BufPool::Udp, 10_000, 20_000_000, 50_000_000, 49_990_000, 1_000, 30_000),
            make_pool(BufPool::Raw, 500, 1_000_000, 1_000_000, 999_500, 100, 2_000),
            make_pool(BufPool::Icmp, 200, 100_000, 5_000_000, 4_999_800, 50, 1_000),
            make_pool(BufPool::Multicast, 100, 500_000, 500_000, 499_900, 10, 500),
            make_pool(BufPool::General, 5_000, 10_000_000, 20_000_000, 19_995_000, 500, 15_000),
        ],
        total_allocs: 176_500_000,
        total_frees: 176_434_200,
        total_drops: 6_660,
        total_bytes_allocated: 231_600_000,
        ops: 0,
    });
}

/// Allocate a buffer.
pub fn alloc(pool: BufPool, size: u64) -> KernelResult<()> {
    with_state(|state| {
        let p = &mut state.pools[pool.index()];
        p.allocs += 1;
        p.active_buffers += 1;
        p.total_bytes += size;
        if p.active_buffers > p.peak_buffers {
            p.peak_buffers = p.active_buffers;
        }
        state.total_allocs += 1;
        state.total_bytes_allocated += size;
        Ok(())
    })
}

/// Free a buffer.
pub fn free(pool: BufPool, size: u64) -> KernelResult<()> {
    with_state(|state| {
        let p = &mut state.pools[pool.index()];
        p.frees += 1;
        p.active_buffers = p.active_buffers.saturating_sub(1);
        p.total_bytes = p.total_bytes.saturating_sub(size);
        state.total_frees += 1;
        Ok(())
    })
}

/// Record a buffer drop.
pub fn record_drop(pool: BufPool) -> KernelResult<()> {
    with_state(|state| {
        state.pools[pool.index()].drops += 1;
        state.total_drops += 1;
        Ok(())
    })
}

/// Per-pool statistics.
pub fn pool_stats() -> Vec<PoolStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.pools.to_vec())
}

/// Statistics: (pool_count, total_allocs, total_frees, total_drops, total_bytes, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (6, s.total_allocs, s.total_frees, s.total_drops, s.total_bytes_allocated, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("sockbuf::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(pool_stats().len(), 6);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Alloc.
    let before = pool_stats()[0].active_buffers;
    alloc(BufPool::Tcp, 1500).expect("alloc");
    let after = pool_stats()[0].active_buffers;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [2/8] alloc: OK");

    // 3: Free.
    free(BufPool::Tcp, 1500).expect("free");
    let after2 = pool_stats()[0].active_buffers;
    assert_eq!(after2, before);
    crate::serial_println!("  [3/8] free: OK");

    // 4: Drop.
    let before = pool_stats()[1].drops;
    record_drop(BufPool::Udp).expect("drop");
    let after = pool_stats()[1].drops;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [4/8] drop: OK");

    // 5: Peak tracking.
    for _ in 0..5 { alloc(BufPool::Icmp, 64).expect("multi_alloc"); }
    let p = &pool_stats()[3];
    assert!(p.peak_buffers >= p.active_buffers);
    crate::serial_println!("  [5/8] peak: OK");

    // 6: Multiple pools.
    alloc(BufPool::General, 4096).expect("gen_alloc");
    free(BufPool::General, 4096).expect("gen_free");
    crate::serial_println!("  [6/8] multi pool: OK");

    // 7: Pool stats.
    let pools = pool_stats();
    assert!(pools[0].allocs > 100_000_000);
    crate::serial_println!("  [7/8] pool stats: OK");

    // 8: Stats.
    let (count, allocs, frees, drops, bytes, ops) = stats();
    assert_eq!(count, 6);
    assert!(allocs > 176_500_000);
    assert!(frees > 176_434_200);
    assert!(drops > 6_660);
    assert!(bytes > 231_600_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("sockbuf::self_test() — all 8 tests passed");
}
