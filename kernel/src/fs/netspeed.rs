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

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        results: Vec::new(),
        snapshots: alloc::vec![
            BandwidthSnapshot {
                interface: String::from("eth0"),
                rx_bytes: 0, tx_bytes: 0, rx_packets: 0, tx_packets: 0,
                rx_errors: 0, tx_errors: 0, last_update_ns: now,
            },
        ],
        next_id: 1,
        total_tests: 0,
        ops: 0,
    });
}

/// Run a speed test (simulated).
pub fn run_test(interface: &str) -> KernelResult<SpeedResult> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        // Simulate realistic speeds (100 Mbps down, 50 Mbps up).
        let result = SpeedResult {
            id, interface: String::from(interface),
            download_bps: 100_000_000 + (now % 20_000_000),
            upload_bps: 50_000_000 + (now % 10_000_000),
            latency_ms: 12 + (now % 8) as u32,
            jitter_ms: 2 + (now % 3) as u32,
            timestamp_ns: now,
            server: String::from("speedtest.local"),
        };
        if state.results.len() >= MAX_RESULTS {
            state.results.remove(0);
        }
        state.results.push(result.clone());
        state.total_tests += 1;
        Ok(result)
    })
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
    init_defaults();

    // 1: Default interface.
    assert_eq!(bandwidth_snapshots().len(), 1);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Run speed test.
    let result = run_test("eth0").expect("test");
    assert!(result.download_bps > 0);
    assert!(result.upload_bps > 0);
    assert!(result.latency_ms > 0);
    crate::serial_println!("  [2/8] speed test: OK");

    // 3: Test history.
    assert_eq!(test_history().len(), 1);
    crate::serial_println!("  [3/8] history: OK");

    // 4: Update bandwidth.
    update_bandwidth("eth0", 1_000_000, 500_000, 1000, 500).expect("update");
    let snap = interface_bandwidth("eth0").expect("get");
    assert_eq!(snap.rx_bytes, 1_000_000);
    crate::serial_println!("  [4/8] update bandwidth: OK");

    // 5: New interface.
    update_bandwidth("wlan0", 2_000_000, 1_000_000, 2000, 1000).expect("new_iface");
    assert_eq!(bandwidth_snapshots().len(), 2);
    crate::serial_println!("  [5/8] new interface: OK");

    // 6: Record errors.
    record_errors("eth0", 5, 2).expect("errors");
    let snap = interface_bandwidth("eth0").expect("get2");
    assert_eq!(snap.rx_errors, 5);
    crate::serial_println!("  [6/8] errors: OK");

    // 7: Format speed.
    assert_eq!(format_speed(100_000_000), "100.0 Mbps");
    assert_eq!(format_speed(1_500_000_000), "1.5 Gbps");
    crate::serial_println!("  [7/8] format: OK");

    // 8: Stats.
    let (results, ifaces, total_tests, ops) = stats();
    assert!(results >= 1);
    assert!(ifaces >= 2);
    assert!(total_tests >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("netspeed::self_test() — all 8 tests passed");
}
