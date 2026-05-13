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

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        source_bytes: [500_000_000, 100_000_000, 50_000_000, 10_000_000, 5_000_000, 20_000_000],
        source_failures: [100, 500, 0, 0, 0, 200],
        total_generated: 685_000_000,
        total_requested: 600_000_000,
        total_served: 600_000_000,
        entropy_bits: 4096,
        pool_size_bits: 4096,
        reseed_count: 100_000,
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
    init_defaults();

    // 1: Defaults.
    let ps = pool_status();
    assert_eq!(ps.pool_size_bits, 4096);
    assert!(ps.ready);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Generate.
    let (gen_before, _, _, _, _) = stats();
    record_generation(EntropySource::Rdrand, 1024).expect("generate");
    let (gen_after, _, _, _, _) = stats();
    assert_eq!(gen_after, gen_before + 1024);
    crate::serial_println!("  [2/8] generate: OK");

    // 3: Request.
    let (_, req_before, _, _, _) = stats();
    record_request(512).expect("request");
    let (_, req_after, _, _, _) = stats();
    assert_eq!(req_after, req_before + 512);
    crate::serial_println!("  [3/8] request: OK");

    // 4: Failure.
    let failures_before = source_breakdown()[0].2;
    record_failure(EntropySource::Rdrand).expect("failure");
    let failures_after = source_breakdown()[0].2;
    assert_eq!(failures_after, failures_before + 1);
    crate::serial_println!("  [4/8] failure: OK");

    // 5: Reseed.
    let (_, _, reseed_before, _, _) = stats();
    record_reseed().expect("reseed");
    let (_, _, reseed_after, bits, _) = stats();
    assert_eq!(reseed_after, reseed_before + 1);
    assert_eq!(bits, 4096);
    crate::serial_println!("  [5/8] reseed: OK");

    // 6: Source breakdown.
    let sources = source_breakdown();
    assert_eq!(sources.len(), 6);
    assert!(sources[0].1 > 500_000_000);
    crate::serial_println!("  [6/8] sources: OK");

    // 7: Pool ready.
    let ps = pool_status();
    assert!(ps.ready);
    crate::serial_println!("  [7/8] pool ready: OK");

    // 8: Stats.
    let (generated, requested, reseeds, bits, ops) = stats();
    assert!(generated > 685_000_000);
    assert!(requested > 600_000_000);
    assert!(reseeds > 100_000);
    assert!(bits > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("hwrng::self_test() — all 8 tests passed");
}
