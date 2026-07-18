//! Network Latency Statistics — network round-trip/processing latency.
//!
//! Tracks per-interface and per-protocol network latency
//! histograms, jitter, and outlier detection. Essential for
//! network performance diagnosis.
//!
//! ## Architecture
//!
//! ```text
//! Network latency monitoring
//!   → netlat::record_rtt(iface, proto, ns) → RTT sample
//!   → netlat::record_processing(iface, ns) → processing latency
//!   → netlat::per_interface() → per-interface stats
//!   → netlat::histogram(iface) → latency histogram
//!
//! Integration:
//!   → netmon (network monitoring)
//!   → netsock (network sockets)
//!   → netqueue (network queues)
//!   → netdev (network devices)
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

/// Protocol type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
    Icmp,
    Other,
}

impl Protocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
            Self::Icmp => "ICMP",
            Self::Other => "other",
        }
    }
}

/// Histogram buckets (microseconds): <10, <50, <100, <500, <1000, <5000, <10000, >=10000
const BUCKET_COUNT: usize = 8;
const BUCKET_BOUNDS_US: [u64; 7] = [10, 50, 100, 500, 1000, 5000, 10000];

/// Per-interface latency stats.
#[derive(Debug, Clone)]
pub struct IfaceLatency {
    pub name: String,
    pub rtt_samples: u64,
    pub rtt_total_ns: u64,
    pub rtt_min_ns: u64,
    pub rtt_max_ns: u64,
    pub proc_samples: u64,
    pub proc_total_ns: u64,
    pub proc_max_ns: u64,
    pub rtt_histogram: [u64; BUCKET_COUNT],
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_IFACES: usize = 64;

struct State {
    ifaces: Vec<IfaceLatency>,
    total_rtt_samples: u64,
    total_proc_samples: u64,
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

fn bucket_index(ns: u64) -> usize {
    let us = ns / 1000;
    for (i, &bound) in BUCKET_BOUNDS_US.iter().enumerate() {
        if us < bound { return i; }
    }
    BUCKET_COUNT - 1
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise an **empty** network-latency table.
///
/// Seeds NO interface rows and zero totals.  Real latency accounting is wired
/// through [`register_iface`]/[`record_rtt`]/[`record_processing`]; until those
/// are called the table is genuinely empty, so the `/proc/netlat` file and the
/// `netlat` kshell command report zeros rather than fabricated numbers — the
/// kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded two fictional interfaces (eth0 with rtt_samples
/// 5_000_000, rtt_total_ns 500_000_000_000 and fabricated histogram buckets;
/// lo with rtt_samples 1_000_000) plus invented aggregate totals
/// (total_rtt_samples 6_000_000, total_proc_samples 12_000_000), which
/// `/proc/netlat` then displayed as if they were real per-interface latency
/// measurements.  That demo data was removed; the self-test now builds its own
/// fixtures explicitly via the real API (see [`self_test`]).  The network stack
/// is expected to call [`register_iface`] on interface bring-up and
/// [`record_rtt`]/[`record_processing`] as packets flow.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        ifaces: Vec::new(),
        total_rtt_samples: 0,
        total_proc_samples: 0,
        ops: 0,
    });
}

/// Register an interface.
pub fn register_iface(name: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.ifaces.len() >= MAX_IFACES { return Err(KernelError::ResourceExhausted); }
        if state.ifaces.iter().any(|i| i.name == name) { return Err(KernelError::AlreadyExists); }
        state.ifaces.push(IfaceLatency {
            name: String::from(name),
            rtt_samples: 0, rtt_total_ns: 0, rtt_min_ns: u64::MAX, rtt_max_ns: 0,
            proc_samples: 0, proc_total_ns: 0, proc_max_ns: 0,
            rtt_histogram: [0; BUCKET_COUNT],
        });
        Ok(())
    })
}

/// Record an RTT sample.
pub fn record_rtt(name: &str, _proto: Protocol, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let iface = state.ifaces.iter_mut().find(|i| i.name == name)
            .ok_or(KernelError::NotFound)?;
        iface.rtt_samples += 1;
        iface.rtt_total_ns += ns;
        if ns < iface.rtt_min_ns { iface.rtt_min_ns = ns; }
        if ns > iface.rtt_max_ns { iface.rtt_max_ns = ns; }
        iface.rtt_histogram[bucket_index(ns)] += 1;
        state.total_rtt_samples += 1;
        Ok(())
    })
}

/// Record a processing latency sample.
pub fn record_processing(name: &str, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let iface = state.ifaces.iter_mut().find(|i| i.name == name)
            .ok_or(KernelError::NotFound)?;
        iface.proc_samples += 1;
        iface.proc_total_ns += ns;
        if ns > iface.proc_max_ns { iface.proc_max_ns = ns; }
        state.total_proc_samples += 1;
        Ok(())
    })
}

/// Per-interface stats.
pub fn per_interface() -> Vec<IfaceLatency> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.ifaces.clone())
}

/// Get histogram bucket labels.
pub fn bucket_labels() -> [&'static str; BUCKET_COUNT] {
    ["<10us", "<50us", "<100us", "<500us", "<1ms", "<5ms", "<10ms", ">=10ms"]
}

/// Statistics: (iface_count, total_rtt_samples, total_proc_samples, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.ifaces.len(), s.total_rtt_samples, s.total_proc_samples, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("netlat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/netlat must never surface).
    // Resetting first clears any residue from a prior `netlat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated interfaces.
    assert_eq!(per_interface().len(), 0);
    let (c0, r0, p0, _o0) = stats();
    assert_eq!((c0, r0, p0), (0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register; duplicate registration fails.
    register_iface("wlan0").expect("register");
    assert_eq!(per_interface().len(), 1);
    assert!(register_iface("wlan0").is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: First RTT sample seeds min/max exactly (from the empty min sentinel).
    record_rtt("wlan0", Protocol::Tcp, 50_000).expect("rtt"); // 50us
    let i = per_interface().iter().find(|i| i.name == "wlan0").cloned().expect("iface");
    assert_eq!(i.rtt_samples, 1);
    assert_eq!(i.rtt_min_ns, 50_000);
    assert_eq!(i.rtt_max_ns, 50_000);
    assert_eq!(i.rtt_total_ns, 50_000);
    crate::serial_println!("  [3/8] rtt: OK");

    // 4: Histogram bucketing is exact (50us → <100us bucket, index 2).
    let i = per_interface().iter().find(|i| i.name == "wlan0").cloned().expect("iface");
    assert_eq!(i.rtt_histogram[2], 1);
    crate::serial_println!("  [4/8] histogram: OK");

    // 5: Min/max track across samples.
    record_rtt("wlan0", Protocol::Udp, 10_000).expect("rtt2");
    record_rtt("wlan0", Protocol::Icmp, 200_000).expect("rtt3");
    let i = per_interface().iter().find(|i| i.name == "wlan0").cloned().expect("iface");
    assert_eq!(i.rtt_samples, 3);
    assert_eq!(i.rtt_min_ns, 10_000);
    assert_eq!(i.rtt_max_ns, 200_000);
    crate::serial_println!("  [5/8] min/max: OK");

    // 6: Processing latency recorded exactly.
    record_processing("wlan0", 5000).expect("proc");
    let i = per_interface().iter().find(|i| i.name == "wlan0").cloned().expect("iface");
    assert_eq!(i.proc_samples, 1);
    assert_eq!(i.proc_max_ns, 5000);
    crate::serial_println!("  [6/8] processing: OK");

    // 7: Recording on an unknown interface fails with NotFound.
    assert!(record_rtt("nonexist", Protocol::Tcp, 100).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (ifaces, rtt, proc_s, ops) = stats();
    assert_eq!(ifaces, 1); // wlan0
    assert_eq!(rtt, 3); // three record_rtt calls
    assert_eq!(proc_s, 1); // one record_processing call
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/netlat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the network stack wires
    // real accounting.
    *STATE.lock() = None;

    crate::serial_println!("netlat::self_test() — all 8 tests passed");
}
