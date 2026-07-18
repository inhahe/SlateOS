//! Entropy — system entropy pool management.
//!
//! Manages the kernel entropy pool used for random number generation,
//! tracks entropy sources and collection statistics, and provides
//! entropy quality monitoring.
//!
//! ## Architecture
//!
//! ```text
//! Entropy management
//!   → entropy::add(source, bits) → feed entropy
//!   → entropy::available() → available entropy bits
//!   → entropy::drain(bits) → consume entropy
//!   → entropy::sources() → list entropy sources
//!
//! Integration:
//!   → certmgr (certificate manager — key generation)
//!   → diskencrypt (disk encryption — key derivation)
//!   → credentials (credential store)
//!   → sysinfo (system information)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Entropy source type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntropySource {
    Hardware,        // RDRAND/RDSEED or TPM.
    Interrupt,       // Interrupt timing jitter.
    Disk,            // Disk I/O timing.
    Keyboard,        // Keystroke timing.
    Mouse,           // Mouse movement.
    Network,         // Network packet timing.
    Jitter,          // CPU execution jitter.
    Seed,            // Saved seed file.
}

impl EntropySource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hardware => "Hardware RNG",
            Self::Interrupt => "Interrupt jitter",
            Self::Disk => "Disk I/O",
            Self::Keyboard => "Keyboard",
            Self::Mouse => "Mouse",
            Self::Network => "Network",
            Self::Jitter => "CPU jitter",
            Self::Seed => "Seed file",
        }
    }
}

/// Pool quality level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolQuality {
    Empty,
    Low,
    Adequate,
    Full,
}

impl PoolQuality {
    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "Empty",
            Self::Low => "Low",
            Self::Adequate => "Adequate",
            Self::Full => "Full",
        }
    }
}

/// Entropy source statistics.
#[derive(Debug, Clone)]
pub struct SourceStats {
    pub source: EntropySource,
    pub bits_contributed: u64,
    pub events: u64,
    pub last_event_ns: u64,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const POOL_SIZE_BITS: u64 = 4096;
const LOW_THRESHOLD: u64 = 256;
const ADEQUATE_THRESHOLD: u64 = 1024;

struct State {
    available_bits: u64,
    sources: Vec<SourceStats>,
    total_added: u64,
    total_drained: u64,
    total_events: u64,
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
        available_bits: 256, // Start with some seed entropy.
        sources: alloc::vec![
            SourceStats { source: EntropySource::Hardware, bits_contributed: 256, events: 1, last_event_ns: 0, enabled: true },
            SourceStats { source: EntropySource::Interrupt, bits_contributed: 0, events: 0, last_event_ns: 0, enabled: true },
            SourceStats { source: EntropySource::Disk, bits_contributed: 0, events: 0, last_event_ns: 0, enabled: true },
            SourceStats { source: EntropySource::Keyboard, bits_contributed: 0, events: 0, last_event_ns: 0, enabled: true },
            SourceStats { source: EntropySource::Mouse, bits_contributed: 0, events: 0, last_event_ns: 0, enabled: true },
            SourceStats { source: EntropySource::Network, bits_contributed: 0, events: 0, last_event_ns: 0, enabled: true },
            SourceStats { source: EntropySource::Jitter, bits_contributed: 0, events: 0, last_event_ns: 0, enabled: true },
            SourceStats { source: EntropySource::Seed, bits_contributed: 0, events: 0, last_event_ns: 0, enabled: true },
        ],
        total_added: 256,
        total_drained: 0,
        total_events: 1,
        reseed_count: 0,
        ops: 0,
    });
}

/// Add entropy from a source.
pub fn add_entropy(source: EntropySource, bits: u64) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if let Some(src) = state.sources.iter_mut().find(|s| s.source == source) {
            if !src.enabled {
                return Err(KernelError::PermissionDenied);
            }
            src.bits_contributed += bits;
            src.events += 1;
            src.last_event_ns = now;
        }
        state.available_bits = (state.available_bits + bits).min(POOL_SIZE_BITS);
        state.total_added += bits;
        state.total_events += 1;
        Ok(())
    })
}

/// Drain entropy from the pool.
pub fn drain_entropy(bits: u64) -> KernelResult<u64> {
    with_state(|state| {
        let drained = bits.min(state.available_bits);
        state.available_bits -= drained;
        state.total_drained += drained;
        Ok(drained)
    })
}

/// Get available entropy bits.
pub fn available() -> u64 {
    STATE.lock().as_ref().map_or(0, |s| s.available_bits)
}

/// Get pool quality.
pub fn quality() -> PoolQuality {
    let bits = available();
    if bits == 0 { PoolQuality::Empty }
    else if bits < LOW_THRESHOLD { PoolQuality::Low }
    else if bits < ADEQUATE_THRESHOLD { PoolQuality::Adequate }
    else { PoolQuality::Full }
}

/// List entropy sources.
pub fn list_sources() -> Vec<SourceStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.sources.clone())
}

/// Enable/disable an entropy source.
pub fn set_source_enabled(source: EntropySource, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let src = state.sources.iter_mut().find(|s| s.source == source)
            .ok_or(KernelError::NotFound)?;
        src.enabled = enabled;
        Ok(())
    })
}

/// Force reseed from hardware RNG.
pub fn reseed() -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        // Simulate hardware reseed.
        let bits = 256u64;
        state.available_bits = (state.available_bits + bits).min(POOL_SIZE_BITS);
        state.total_added += bits;
        state.reseed_count += 1;
        if let Some(src) = state.sources.iter_mut().find(|s| s.source == EntropySource::Hardware) {
            src.bits_contributed += bits;
            src.events += 1;
            src.last_event_ns = now;
        }
        Ok(())
    })
}

/// Statistics: (available_bits, total_added, total_drained, total_events, reseed_count, ops).
pub fn stats() -> (u64, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.available_bits, s.total_added, s.total_drained, s.total_events, s.reseed_count, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("entropy::self_test() — running tests...");
    init_defaults();

    // 1: Initial state.
    assert_eq!(available(), 256);
    assert_eq!(quality(), PoolQuality::Adequate);
    crate::serial_println!("  [1/8] initial: OK");

    // 2: Add entropy.
    add_entropy(EntropySource::Interrupt, 128).expect("add");
    assert_eq!(available(), 384);
    crate::serial_println!("  [2/8] add: OK");

    // 3: Drain.
    let got = drain_entropy(100).expect("drain");
    assert_eq!(got, 100);
    assert_eq!(available(), 284);
    crate::serial_println!("  [3/8] drain: OK");

    // 4: Drain more than available.
    let excess = drain_entropy(10000).expect("drain2");
    assert_eq!(excess, 284);
    assert_eq!(available(), 0);
    assert_eq!(quality(), PoolQuality::Empty);
    crate::serial_println!("  [4/8] drain excess: OK");

    // 5: Reseed.
    reseed().expect("reseed");
    assert_eq!(available(), 256);
    crate::serial_println!("  [5/8] reseed: OK");

    // 6: Fill to cap.
    add_entropy(EntropySource::Disk, 10000).expect("fill");
    assert_eq!(available(), POOL_SIZE_BITS);
    assert_eq!(quality(), PoolQuality::Full);
    crate::serial_println!("  [6/8] pool cap: OK");

    // 7: Sources.
    let sources = list_sources();
    assert_eq!(sources.len(), 8);
    let hw = sources.iter().find(|s| s.source == EntropySource::Hardware).expect("hw");
    assert!(hw.bits_contributed > 0);
    crate::serial_println!("  [7/8] sources: OK");

    // 8: Stats.
    let (avail, added, drained, events, reseeds, ops) = stats();
    assert_eq!(avail, POOL_SIZE_BITS);
    assert!(added > 0);
    assert!(drained > 0);
    assert!(events > 0);
    assert!(reseeds >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("entropy::self_test() — all 8 tests passed");
}
