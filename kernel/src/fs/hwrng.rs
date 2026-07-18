//! Hardware RNG Statistics — hardware random number generator monitoring.
//!
//! Tracks entropy generation, pool fill rates, and RDRAND/RDSEED
//! usage. Essential for understanding randomness quality and
//! availability for cryptographic operations.
//!
//! ## Architecture
//!
//! ```text
//! Hardware RNG monitoring
//!   → hwrng::record_generation(source, bytes) → entropy generated
//!   → hwrng::record_request(bytes) → entropy consumed
//!   → hwrng::record_failure(source) → generation failure
//!   → hwrng::pool_status() → entropy pool status
//!
//! Integration:
//!   → entropy (entropy pool)
//!   → secmod (security module)
//!   → bpfstat (BPF programs)
//!   → clocksrc (clock sources)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Entropy source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntropySource {
    Rdrand,
    Rdseed,
    Interrupt,
    Disk,
    Input,
    Jitter,
}

impl EntropySource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rdrand => "rdrand",
            Self::Rdseed => "rdseed",
            Self::Interrupt => "interrupt",
            Self::Disk => "disk",
            Self::Input => "input",
            Self::Jitter => "jitter",
        }
    }
    pub fn index(self) -> usize {
        match self {
            Self::Rdrand => 0,
            Self::Rdseed => 1,
            Self::Interrupt => 2,
            Self::Disk => 3,
            Self::Input => 4,
            Self::Jitter => 5,
        }
    }
}

const NUM_SOURCES: usize = 6;

/// Entropy pool status.
#[derive(Debug, Clone)]
pub struct PoolStatus {
    pub entropy_bits: u32,
    pub pool_size_bits: u32,
    pub ready: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    source_bytes: [u64; NUM_SOURCES],
    source_failures: [u64; NUM_SOURCES],
    total_generated: u64,
    total_requested: u64,
    total_served: u64,
    entropy_bits: u32,
    pool_size_bits: u32,
    reseed_count: u64,
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

/// Entropy pool capacity, in bits. This is a structural design constant (the
/// pool's maximum), not an observation, so it is the one non-zero field a
/// freshly-initialised pool carries.
const POOL_SIZE_BITS: u32 = 4096;

/// Initialise the hardware-RNG statistics state.
///
/// Starts with zero bytes generated/requested/served per source, zero
/// failures, zero reseeds, and an EMPTY entropy pool (`entropy_bits = 0`, so
/// [`pool_status`] reports `ready = false`). The six entropy sources
/// (RDRAND, RDSEED, interrupt, disk, input, jitter) are a fixed dimension, so
/// [`source_breakdown`] always returns six rows — but with zeroed counters;
/// they advance only through real [`record_generation`] / [`record_request`] /
/// [`record_failure`] / [`record_reseed`] calls. The `/proc/hwrng` generator
/// and the `hwrng` kshell command surface the pool status and per-source
/// breakdown as if they reflect real entropy activity, so seeding them with
/// invented byte counts (and a full, "ready" pool) would be fabricated procfs
/// data — it would claim cryptographic-quality entropy had been gathered when
/// none has, which is a particularly dangerous lie for a security surface.
/// Only the pool capacity ([`POOL_SIZE_BITS`]) is non-zero, as that is a
/// structural constant rather than measured activity.
///
/// (Previously this seeded fabricated activity — per-source bytes of
/// [500M, 100M, 50M, 10M, 5M, 20M] and failures [100, 500, 0, 0, 0, 200],
/// 685,000,000 bytes generated, 600,000,000 requested/served, a full 4096-bit
/// pool reporting ready, and 100,000 reseeds.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        source_bytes: [0; NUM_SOURCES],
        source_failures: [0; NUM_SOURCES],
        total_generated: 0,
        total_requested: 0,
        total_served: 0,
        entropy_bits: 0,
        pool_size_bits: POOL_SIZE_BITS,
        reseed_count: 0,
        ops: 0,
    });
}

/// Record entropy generation.
pub fn record_generation(source: EntropySource, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        state.source_bytes[source.index()] += bytes;
        state.total_generated += bytes;
        let bits_added = (bytes * 8).min(state.pool_size_bits as u64 - state.entropy_bits as u64);
        state.entropy_bits += bits_added as u32;
        Ok(())
    })
}

/// Record an entropy request (consumption).
pub fn record_request(bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        state.total_requested += bytes;
        state.total_served += bytes;
        let bits_used = (bytes * 8).min(state.entropy_bits as u64);
        state.entropy_bits -= bits_used as u32;
        Ok(())
    })
}

/// Record a generation failure.
pub fn record_failure(source: EntropySource) -> KernelResult<()> {
    with_state(|state| {
        state.source_failures[source.index()] += 1;
        Ok(())
    })
}

/// Record a reseed event.
pub fn record_reseed() -> KernelResult<()> {
    with_state(|state| {
        state.reseed_count += 1;
        state.entropy_bits = state.pool_size_bits;
        Ok(())
    })
}

/// Get pool status.
pub fn pool_status() -> PoolStatus {
    let guard = STATE.lock();
    guard.as_ref().map_or(PoolStatus { entropy_bits: 0, pool_size_bits: 0, ready: false }, |s| {
        PoolStatus {
            entropy_bits: s.entropy_bits,
            pool_size_bits: s.pool_size_bits,
            ready: s.entropy_bits >= 256,
        }
    })
}

/// Per-source breakdown: Vec of (source, bytes, failures).
pub fn source_breakdown() -> Vec<(EntropySource, u64, u64)> {
    let guard = STATE.lock();
    guard.as_ref().map_or(Vec::new(), |s| {
        let sources = [EntropySource::Rdrand, EntropySource::Rdseed, EntropySource::Interrupt, EntropySource::Disk, EntropySource::Input, EntropySource::Jitter];
        sources.iter().enumerate().map(|(i, &src)| (src, s.source_bytes[i], s.source_failures[i])).collect()
    })
}

/// Statistics: (total_generated, total_requested, reseed_count, entropy_bits, ops).
pub fn stats() -> (u64, u64, u64, u32, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.total_generated, s.total_requested, s.reseed_count, s.entropy_bits, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("hwrng::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and no
    // fixtures leak into the live entropy stats afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — the pool reports its structural capacity but is EMPTY
    //    (0 bits, not ready); every source counter and global total is zero.
    let ps = pool_status();
    assert_eq!(ps.pool_size_bits, POOL_SIZE_BITS);
    assert_eq!(ps.entropy_bits, 0);
    assert!(!ps.ready);
    let (g0, rq0, rs0, eb0, _) = stats();
    assert_eq!((g0, rq0, rs0, eb0), (0, 0, 0, 0));
    let sources = source_breakdown();
    assert_eq!(sources.len(), NUM_SOURCES);
    for (_, bytes, fails) in &sources {
        assert_eq!((*bytes, *fails), (0, 0));
    }
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Generate — 32 bytes from RDRAND adds to that source, the global total,
    //    and the pool (256 bits, capped at capacity).
    record_generation(EntropySource::Rdrand, 32).expect("generate");
    assert_eq!(stats().0, 32); // total_generated
    assert_eq!(source_breakdown()[EntropySource::Rdrand.index()].1, 32);
    assert_eq!(pool_status().entropy_bits, 256); // 32 * 8
    crate::serial_println!("  [2/8] generate: OK");

    // 3: Request — 16 bytes consumed drains 128 bits from the pool.
    record_request(16).expect("request");
    assert_eq!(stats().1, 16); // total_requested
    assert_eq!(pool_status().entropy_bits, 128); // 256 - 128
    crate::serial_println!("  [3/8] request: OK");

    // 4: Failure — a RDRAND failure advances only that source's failure counter.
    record_failure(EntropySource::Rdrand).expect("failure");
    assert_eq!(source_breakdown()[EntropySource::Rdrand.index()].2, 1);
    crate::serial_println!("  [4/8] failure: OK");

    // 5: Reseed — bumps the reseed count and refills the pool to capacity.
    record_reseed().expect("reseed");
    let (_, _, reseeds, bits, _) = stats();
    assert_eq!(reseeds, 1);
    assert_eq!(bits, POOL_SIZE_BITS);
    crate::serial_println!("  [5/8] reseed: OK");

    // 6: Source breakdown — six rows; only RDRAND has accrued the real activity.
    let sources = source_breakdown();
    assert_eq!(sources.len(), NUM_SOURCES);
    assert_eq!((sources[0].1, sources[0].2), (32, 1));
    for s in &sources[1..] {
        assert_eq!((s.1, s.2), (0, 0));
    }
    crate::serial_println!("  [6/8] sources: OK");

    // 7: Pool ready — after the reseed the pool is full, so it reports ready.
    assert!(pool_status().ready);
    crate::serial_println!("  [7/8] pool ready: OK");

    // 8: Final stats reflect only the real activity above: 32 bytes generated,
    //    16 requested, 1 reseed, pool at capacity.
    let (generated, requested, reseeds, bits, ops) = stats();
    assert_eq!((generated, requested, reseeds, bits), (32, 16, 1, POOL_SIZE_BITS));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("hwrng::self_test() — all 8 tests passed");
}
