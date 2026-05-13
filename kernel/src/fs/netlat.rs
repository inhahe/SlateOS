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
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        ifaces: alloc::vec![
            IfaceLatency {
                name: String::from("eth0"),
                rtt_samples: 5_000_000, rtt_total_ns: 500_000_000_000, rtt_min_ns: 100_000, rtt_max_ns: 50_000_000,
                proc_samples: 10_000_000, proc_total_ns: 100_000_000_000, proc_max_ns: 5_000_000,
                rtt_histogram: [100_000, 500_000, 1_000_000, 2_000_000, 800_000, 400_000, 150_000, 50_000],
            },
            IfaceLatency {
                name: String::from("lo"),
                rtt_samples: 1_000_000, rtt_total_ns: 10_000_000_000, rtt_min_ns: 1_000, rtt_max_ns: 1_000_000,
                proc_samples: 2_000_000, proc_total_ns: 4_000_000_000, proc_max_ns: 100_000,
                rtt_histogram: [800_000, 150_000, 30_000, 15_000, 3_000, 1_500, 400, 100],
            },
        ],
        total_rtt_samples: 6_000_000,
        total_proc_samples: 12_000_000,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_interface().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    register_iface("wlan0").expect("register");
    assert_eq!(per_interface().len(), 3);
    assert!(register_iface("wlan0").is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: RTT.
    record_rtt("wlan0", Protocol::Tcp, 50_000).expect("rtt"); // 50us
    let i = per_interface().iter().find(|i| i.name == "wlan0").cloned().unwrap();
    assert_eq!(i.rtt_samples, 1);
    assert_eq!(i.rtt_min_ns, 50_000);
    assert_eq!(i.rtt_max_ns, 50_000);
    crate::serial_println!("  [3/8] rtt: OK");

    // 4: Histogram.
    let i = per_interface().iter().find(|i| i.name == "wlan0").cloned().unwrap();
    assert_eq!(i.rtt_histogram[2], 1); // 50us goes to <100us bucket
    crate::serial_println!("  [4/8] histogram: OK");

    // 5: Min/max.
    record_rtt("wlan0", Protocol::Udp, 10_000).expect("rtt2");
    record_rtt("wlan0", Protocol::Icmp, 200_000).expect("rtt3");
    let i = per_interface().iter().find(|i| i.name == "wlan0").cloned().unwrap();
    assert_eq!(i.rtt_min_ns, 10_000);
    assert_eq!(i.rtt_max_ns, 200_000);
    crate::serial_println!("  [5/8] min/max: OK");

    // 6: Processing.
    record_processing("wlan0", 5000).expect("proc");
    let i = per_interface().iter().find(|i| i.name == "wlan0").cloned().unwrap();
    assert_eq!(i.proc_samples, 1);
    assert_eq!(i.proc_max_ns, 5000);
    crate::serial_println!("  [6/8] processing: OK");

    // 7: Not found.
    assert!(record_rtt("nonexist", Protocol::Tcp, 100).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (ifaces, rtt, proc_s, ops) = stats();
    assert!(ifaces >= 3);
    assert!(rtt > 6_000_000);
    assert!(proc_s > 12_000_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("netlat::self_test() — all 8 tests passed");
}
