//! Network Speed — bandwidth measurement and speed testing.
//!
//! Provides network speed testing, bandwidth monitoring per
//! interface, and historical speed data for connectivity analysis.
//!
//! ## Architecture
//!
//! ```text
//! Speed testing
//!   → netspeed::run_test(interface) → measure upload/download
//!   → netspeed::bandwidth(interface) → current throughput
//!   → netspeed::history() → past test results
//!
//! Integration:
//!   → netsettings (network configuration)
//!   → netdiag (network diagnostics)
//!   → datausage (data usage tracking)
//!   → netindicator (network indicator)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Speed test result.
#[derive(Debug, Clone)]
pub struct SpeedResult {
    pub id: u32,
    pub interface: String,
    pub download_bps: u64,
    pub upload_bps: u64,
    pub latency_ms: u32,
    pub jitter_ms: u32,
    pub timestamp_ns: u64,
    pub server: String,
}

/// Per-interface bandwidth snapshot.
#[derive(Debug, Clone)]
pub struct BandwidthSnapshot {
    pub interface: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub last_update_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_RESULTS: usize = 100;
const MAX_INTERFACES: usize = 16;

struct State {
    results: Vec<SpeedResult>,
    snapshots: Vec<BandwidthSnapshot>,
    next_id: u32,
    total_tests: u64,
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

/// Initialise an **empty** netspeed table.
///
/// Seeds NO interfaces, NO test results, and zero counters.  Real bandwidth
/// snapshots are wired through [`update_bandwidth`] (the net stack reports an
/// interface's true rx/tx byte/packet counters) and [`record_errors`]; until
/// those are called the table is genuinely empty, so `/proc/netspeed` and the
/// `netspeed` kshell command report nothing rather than fabricated numbers — the
/// kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded a single "eth0" [`BandwidthSnapshot`] with zeroed
/// traffic counters.  Although the counters were zero, the row fabricated the
/// *existence* of an eth0 interface that may not be present, which
/// `bandwidth_snapshots`/`interface_bandwidth` (and the `netspeed bandwidth`
/// command) then surfaced as a real interface.  That placeholder was removed;
/// interfaces now appear only once the net stack reports real traffic via
/// [`update_bandwidth`].  The self-test builds its own fixtures explicitly via
/// the real API (see [`self_test`]).
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        results: Vec::new(),
        snapshots: Vec::new(),
        next_id: 1,
        total_tests: 0,
        ops: 0,
    });
}

/// Run a network speed test.
///
/// Returns [`KernelError::NotSupported`]: a real speed test must saturate the
/// link to a measurement server and compute the actual achieved throughput,
/// latency, and jitter, which requires a network measurement backend the kernel
/// does not yet have.
///
/// NOTE: this previously **fabricated** results — it invented a ~100 Mbps
/// download / ~50 Mbps upload (plus latency/jitter) from the HPET timestamp and
/// recorded them in the history as if they were a genuine measurement.  The
/// `netspeed test` command then displayed those conjured speeds to the user as a
/// real result, violating the kernel's hard "never invent data" rule.  Until a
/// real measurement backend exists, this honestly reports that speed testing is
/// unavailable rather than returning invented numbers.  See todo.txt
/// ("netspeed::run_test needs a real throughput-measurement backend").
pub fn run_test(_interface: &str) -> KernelResult<SpeedResult> {
    Err(KernelError::NotSupported)
}

/// Update bandwidth snapshot for an interface.
pub fn update_bandwidth(interface: &str, rx_bytes: u64, tx_bytes: u64, rx_packets: u64, tx_packets: u64) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if let Some(snap) = state.snapshots.iter_mut().find(|s| s.interface == interface) {
            snap.rx_bytes = rx_bytes;
            snap.tx_bytes = tx_bytes;
            snap.rx_packets = rx_packets;
            snap.tx_packets = tx_packets;
            snap.last_update_ns = now;
        } else {
            if state.snapshots.len() >= MAX_INTERFACES {
                return Err(KernelError::ResourceExhausted);
            }
            state.snapshots.push(BandwidthSnapshot {
                interface: String::from(interface),
                rx_bytes, tx_bytes, rx_packets, tx_packets,
                rx_errors: 0, tx_errors: 0, last_update_ns: now,
            });
        }
        Ok(())
    })
}

/// Record errors for an interface.
pub fn record_errors(interface: &str, rx_errors: u64, tx_errors: u64) -> KernelResult<()> {
    with_state(|state| {
        if let Some(snap) = state.snapshots.iter_mut().find(|s| s.interface == interface) {
            snap.rx_errors += rx_errors;
            snap.tx_errors += tx_errors;
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// Get speed test history.
pub fn test_history() -> Vec<SpeedResult> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.results.clone())
}

/// Get bandwidth snapshots.
pub fn bandwidth_snapshots() -> Vec<BandwidthSnapshot> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.snapshots.clone())
}

/// Get snapshot for a specific interface.
pub fn interface_bandwidth(name: &str) -> Option<BandwidthSnapshot> {
    STATE.lock().as_ref().and_then(|s| {
        s.snapshots.iter().find(|snap| snap.interface == name).cloned()
    })
}

/// Format bits per second as human-readable.
pub fn format_speed(bps: u64) -> String {
    if bps >= 1_000_000_000 {
        format!("{}.{} Gbps", bps / 1_000_000_000, (bps % 1_000_000_000) / 100_000_000)
    } else if bps >= 1_000_000 {
        format!("{}.{} Mbps", bps / 1_000_000, (bps % 1_000_000) / 100_000)
    } else if bps >= 1_000 {
        format!("{}.{} Kbps", bps / 1_000, (bps % 1_000) / 100)
    } else {
        format!("{} bps", bps)
    }
}

/// Statistics: (test_count, interface_count, total_tests, ops).
pub fn stats() -> (usize, usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.results.len(), s.snapshots.len(), s.total_tests, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("netspeed::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/netspeed must never surface).  Resetting
    // first clears any residue from a prior `netspeed test` run.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated interfaces or test results.
    assert_eq!(bandwidth_snapshots().len(), 0);
    assert_eq!(test_history().len(), 0);
    let (r0, i0, t0, _o0) = stats();
    assert_eq!((r0, i0, t0), (0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: run_test is honest — it no longer fabricates speeds; with no real
    // measurement backend it reports NotSupported instead of inventing numbers.
    assert!(matches!(run_test("eth0"), Err(KernelError::NotSupported)));
    assert_eq!(test_history().len(), 0); // nothing fabricated into history
    crate::serial_println!("  [2/8] run_test honest (NotSupported): OK");

    // 3: update_bandwidth creates a snapshot from REAL reported counters.
    update_bandwidth("eth0", 1_000_000, 500_000, 1000, 500).expect("update");
    let snap = interface_bandwidth("eth0").expect("get");
    assert_eq!(snap.rx_bytes, 1_000_000);
    assert_eq!(snap.tx_bytes, 500_000);
    assert_eq!(snap.rx_packets, 1000);
    assert_eq!(bandwidth_snapshots().len(), 1);
    crate::serial_println!("  [3/8] update bandwidth: OK");

    // 4: Re-update overwrites the same interface row (no duplicate).
    update_bandwidth("eth0", 3_000_000, 1_500_000, 3000, 1500).expect("update2");
    assert_eq!(bandwidth_snapshots().len(), 1);
    assert_eq!(interface_bandwidth("eth0").expect("get").rx_bytes, 3_000_000);
    crate::serial_println!("  [4/8] re-update: OK");

    // 5: A second interface adds a distinct row.
    update_bandwidth("wlan0", 2_000_000, 1_000_000, 2000, 1000).expect("new_iface");
    assert_eq!(bandwidth_snapshots().len(), 2);
    crate::serial_println!("  [5/8] new interface: OK");

    // 6: Record errors — accumulates on the matching interface; unknown fails.
    record_errors("eth0", 5, 2).expect("errors");
    let snap = interface_bandwidth("eth0").expect("get2");
    assert_eq!((snap.rx_errors, snap.tx_errors), (5, 2));
    assert!(record_errors("nonexist", 1, 1).is_err());
    crate::serial_println!("  [6/8] errors: OK");

    // 7: format_speed is a pure formatter (no fabricated state).
    assert_eq!(format_speed(100_000_000), "100.0 Mbps");
    assert_eq!(format_speed(1_500_000_000), "1.5 Gbps");
    assert_eq!(format_speed(500), "500 bps");
    crate::serial_println!("  [7/8] format: OK");

    // 8: Stats reflect exactly what we recorded: 0 test results, 2 interfaces.
    let (results, ifaces, total_tests, ops) = stats();
    assert_eq!(results, 0);
    assert_eq!(ifaces, 2);
    assert_eq!(total_tests, 0); // run_test never fabricates a recorded test
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/netspeed table.
    *STATE.lock() = None;

    crate::serial_println!("netspeed::self_test() — all 8 tests passed");
}
