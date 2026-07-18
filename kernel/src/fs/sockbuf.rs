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

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise the socket-buffer pool statistics state.
///
/// Starts with the six buffer pools (tcp, udp, raw, icmp, mcast, general)
/// present but with all per-pool and global counters at zero. The six
/// pools are a fixed taxonomy, so they are always listed — but with zeroed
/// active-buffer/byte/alloc/free/drop/peak counters. The `/proc/sockbuf`
/// generator and the `sockbuf` kshell command surface this table (and
/// `pool_stats`) as if it reflects real buffer-pool activity, so seeding it
/// with invented allocations would be fabricated procfs data. The counters
/// advance only through real [`alloc`] / [`free`] / [`record_drop`] calls.
///
/// (Previously this seeded fabricated activity across all six pools —
/// e.g. TCP with 50,000 active buffers, 100M allocs and 200MB in flight —
/// with global totals of 176.5M allocs, 176.4M frees, 6,660 drops, and
/// 231.6MB allocated.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        pools: [
            make_pool(BufPool::Tcp, 0, 0, 0, 0, 0, 0),
            make_pool(BufPool::Udp, 0, 0, 0, 0, 0, 0),
            make_pool(BufPool::Raw, 0, 0, 0, 0, 0, 0),
            make_pool(BufPool::Icmp, 0, 0, 0, 0, 0, 0),
            make_pool(BufPool::Multicast, 0, 0, 0, 0, 0, 0),
            make_pool(BufPool::General, 0, 0, 0, 0, 0, 0),
        ],
        total_allocs: 0,
        total_frees: 0,
        total_drops: 0,
        total_bytes_allocated: 0,
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
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live buffer-pool table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — six zeroed pool rows, zero totals.
    let pools = pool_stats();
    assert_eq!(pools.len(), 6);
    for p in &pools {
        assert_eq!((p.active_buffers, p.total_bytes, p.allocs, p.frees, p.drops, p.peak_buffers),
                   (0, 0, 0, 0, 0, 0));
    }
    let (c0, a0, f0, d0, b0, _) = stats();
    assert_eq!((c0, a0, f0, d0, b0), (6, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Alloc — active/allocs/bytes/peak advance for the pool and globals.
    alloc(BufPool::Tcp, 1500).expect("alloc");
    let t = &pool_stats()[BufPool::Tcp.index()];
    assert_eq!((t.active_buffers, t.allocs, t.total_bytes, t.peak_buffers), (1, 1, 1500, 1));
    let (_, allocs, _, _, bytes, _) = stats();
    assert_eq!((allocs, bytes), (1, 1500));
    crate::serial_println!("  [2/8] alloc: OK");

    // 3: Free — active and total_bytes drop; frees and peak behaviour exact.
    free(BufPool::Tcp, 1500).expect("free");
    let t = &pool_stats()[BufPool::Tcp.index()];
    assert_eq!((t.active_buffers, t.frees, t.total_bytes, t.peak_buffers), (0, 1, 0, 1));
    assert_eq!(stats().2, 1); // total_frees
    crate::serial_println!("  [3/8] free: OK");

    // 4: Drop — per-pool and global drop counters advance.
    record_drop(BufPool::Udp).expect("drop");
    assert_eq!(pool_stats()[BufPool::Udp.index()].drops, 1);
    assert_eq!(stats().3, 1); // total_drops
    crate::serial_println!("  [4/8] drop: OK");

    // 5: Peak tracking — peak holds the high-water mark after frees.
    for _ in 0..5 { alloc(BufPool::Icmp, 64).expect("multi_alloc"); }
    for _ in 0..2 { free(BufPool::Icmp, 64).expect("multi_free"); }
    let p = &pool_stats()[BufPool::Icmp.index()];
    assert_eq!((p.active_buffers, p.peak_buffers), (3, 5));
    crate::serial_println!("  [5/8] peak: OK");

    // 6: Multiple pools — General alloc then free nets zero active buffers.
    alloc(BufPool::General, 4096).expect("gen_alloc");
    free(BufPool::General, 4096).expect("gen_free");
    assert_eq!(pool_stats()[BufPool::General.index()].active_buffers, 0);
    crate::serial_println!("  [6/8] multi pool: OK");

    // 7: Pool stats — TCP saw exactly one alloc.
    assert_eq!(pool_stats()[BufPool::Tcp.index()].allocs, 1);
    crate::serial_println!("  [7/8] pool stats: OK");

    // 8: Final stats reflect only the real activity above. allocs: tcp 1 +
    //    icmp 5 + general 1 = 7; frees: tcp 1 + icmp 2 + general 1 = 4; drops 1;
    //    bytes: 1500 + 5*64 + 4096 = 5916 (cumulative, not reduced by frees).
    let (count, allocs, frees, drops, bytes, ops) = stats();
    assert_eq!((count, allocs, frees, drops, bytes), (6, 7, 4, 1, 5916));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("sockbuf::self_test() — all 8 tests passed");
}
