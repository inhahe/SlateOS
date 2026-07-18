//! Network Diagnostics — network troubleshooting and analysis tools.
//!
//! Provides traceroute, connectivity checks, DNS diagnostics, speed
//! testing, and network path analysis from the OS level.
//!
//! ## Architecture
//!
//! ```text
//! User runs diagnostic
//!   → netdiag::ping(host) → latency measurement
//!   → netdiag::traceroute(host) → hop-by-hop path
//!   → netdiag::dns_lookup(name) → resolution test
//!   → netdiag::connectivity_check() → internet reachability
//!
//! Integration:
//!   → netsettings (network config)
//!   → sysdiag (system diagnostics)
//!   → crashreport (network failure info)
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

/// Diagnostic test type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagType {
    Ping,
    Traceroute,
    DnsLookup,
    ConnectivityCheck,
    PortScan,
    SpeedTest,
}

impl DiagType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ping => "Ping",
            Self::Traceroute => "Traceroute",
            Self::DnsLookup => "DNS Lookup",
            Self::ConnectivityCheck => "Connectivity",
            Self::PortScan => "Port Scan",
            Self::SpeedTest => "Speed Test",
        }
    }
}

/// Diagnostic result status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagStatus {
    Success,
    TimedOut,
    Unreachable,
    DnsFailure,
    Error,
    InProgress,
}

impl DiagStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Success => "Success",
            Self::TimedOut => "Timed Out",
            Self::Unreachable => "Unreachable",
            Self::DnsFailure => "DNS Failure",
            Self::Error => "Error",
            Self::InProgress => "In Progress",
        }
    }
}

/// A traceroute hop.
#[derive(Debug, Clone)]
pub struct TraceHop {
    pub hop_number: u8,
    pub address: String,
    pub hostname: String,
    /// Latency in microseconds.
    pub latency_us: u64,
    pub reached: bool,
}

/// A diagnostic result.
#[derive(Debug, Clone)]
pub struct DiagResult {
    pub id: u32,
    pub diag_type: DiagType,
    pub target: String,
    pub status: DiagStatus,
    /// Latency in microseconds (for ping).
    pub latency_us: u64,
    /// Hops (for traceroute).
    pub hops: Vec<TraceHop>,
    /// Resolved address (for DNS).
    pub resolved: String,
    /// Speed in kbps (for speed test).
    pub speed_kbps: u64,
    /// Additional info.
    pub info: String,
    pub timestamp_ns: u64,
}

/// Connectivity status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectivityStatus {
    Connected,
    LimitedConnectivity,
    NoInternet,
    Disconnected,
}

impl ConnectivityStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connected => "Connected",
            Self::LimitedConnectivity => "Limited",
            Self::NoInternet => "No Internet",
            Self::Disconnected => "Disconnected",
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_RESULTS: usize = 200;

struct State {
    results: Vec<DiagResult>,
    next_id: u32,
    connectivity: ConnectivityStatus,
    total_pings: u64,
    total_traces: u64,
    total_lookups: u64,
    total_checks: u64,
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

fn store_result(state: &mut State, result: DiagResult) {
    if state.results.len() >= MAX_RESULTS {
        state.results.remove(0);
    }
    state.results.push(result);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        results: Vec::new(),
        next_id: 1,
        connectivity: ConnectivityStatus::Connected,
        total_pings: 0,
        total_traces: 0,
        total_lookups: 0,
        total_checks: 0,
        ops: 0,
    });
}

/// Simulate a ping to a host.
pub fn ping(host: &str, count: u32) -> KernelResult<DiagResult> {
    with_state(|state| {
        let id = state.next_id;
        state.next_id += 1;
        state.total_pings += 1;

        // Simulate latency based on host type.
        let latency = if host.starts_with("127.") || host == "localhost" {
            50 // 0.05ms
        } else if host.starts_with("192.168.") || host.starts_with("10.") {
            1500 // 1.5ms
        } else {
            25000 // 25ms
        };

        let result = DiagResult {
            id,
            diag_type: DiagType::Ping,
            target: String::from(host),
            status: DiagStatus::Success,
            latency_us: latency,
            hops: Vec::new(),
            resolved: String::new(),
            speed_kbps: 0,
            info: format!("{} packets sent, {} received, avg {}us", count, count, latency),
            timestamp_ns: crate::hpet::elapsed_ns(),
        };
        store_result(state, result.clone());
        Ok(result)
    })
}

/// Simulate a traceroute.
pub fn traceroute(host: &str) -> KernelResult<DiagResult> {
    with_state(|state| {
        let id = state.next_id;
        state.next_id += 1;
        state.total_traces += 1;

        let hops = alloc::vec![
            TraceHop { hop_number: 1, address: String::from("192.168.1.1"), hostname: String::from("gateway"), latency_us: 800, reached: true },
            TraceHop { hop_number: 2, address: String::from("10.0.0.1"), hostname: String::from("isp-router"), latency_us: 5000, reached: true },
            TraceHop { hop_number: 3, address: String::from("72.14.233.1"), hostname: String::from("backbone"), latency_us: 12000, reached: true },
            TraceHop { hop_number: 4, address: String::from(host), hostname: String::from(host), latency_us: 25000, reached: true },
        ];

        let result = DiagResult {
            id,
            diag_type: DiagType::Traceroute,
            target: String::from(host),
            status: DiagStatus::Success,
            latency_us: 25000,
            hops,
            resolved: String::new(),
            speed_kbps: 0,
            info: format!("4 hops to {}", host),
            timestamp_ns: crate::hpet::elapsed_ns(),
        };
        store_result(state, result.clone());
        Ok(result)
    })
}

/// Simulate a DNS lookup.
pub fn dns_lookup(name: &str) -> KernelResult<DiagResult> {
    with_state(|state| {
        let id = state.next_id;
        state.next_id += 1;
        state.total_lookups += 1;

        let resolved = if name == "localhost" {
            String::from("127.0.0.1")
        } else {
            // Simulate resolved address.
            format!("93.184.{}.{}", name.len() % 256, (name.len() * 7) % 256)
        };

        let result = DiagResult {
            id,
            diag_type: DiagType::DnsLookup,
            target: String::from(name),
            status: DiagStatus::Success,
            latency_us: 3500,
            hops: Vec::new(),
            resolved: resolved.clone(),
            speed_kbps: 0,
            info: format!("{} → {}", name, resolved),
            timestamp_ns: crate::hpet::elapsed_ns(),
        };
        store_result(state, result.clone());
        Ok(result)
    })
}

/// Check connectivity status.
pub fn connectivity_check() -> KernelResult<ConnectivityStatus> {
    with_state(|state| {
        state.total_checks += 1;
        Ok(state.connectivity)
    })
}

/// Set connectivity status (for simulation/testing).
pub fn set_connectivity(status: ConnectivityStatus) -> KernelResult<()> {
    with_state(|state| {
        state.connectivity = status;
        Ok(())
    })
}

/// List recent results.
pub fn list_results(count: usize) -> Vec<DiagResult> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = s.results.len().saturating_sub(count);
        s.results[start..].to_vec()
    })
}

/// Get a specific result.
pub fn get_result(id: u32) -> KernelResult<DiagResult> {
    with_state(|state| {
        state.results.iter().find(|r| r.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// Clear all results.
pub fn clear_results() -> KernelResult<()> {
    with_state(|state| {
        state.results.clear();
        Ok(())
    })
}

/// Statistics: (result_count, total_pings, total_traces, total_lookups, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.results.len(), s.total_pings, s.total_traces, s.total_lookups, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("netdiag::self_test() — running tests...");
    init_defaults();

    // 1: No results initially.
    assert!(list_results(10).is_empty());
    crate::serial_println!("  [1/10] empty initial: OK");

    // 2: Ping localhost.
    let r = ping("127.0.0.1", 4).expect("ping");
    assert_eq!(r.status, DiagStatus::Success);
    assert_eq!(r.latency_us, 50);
    crate::serial_println!("  [2/10] ping localhost: OK");

    // 3: Ping remote.
    let r = ping("example.com", 4).expect("ping2");
    assert_eq!(r.latency_us, 25000);
    crate::serial_println!("  [3/10] ping remote: OK");

    // 4: Traceroute.
    let r = traceroute("example.com").expect("trace");
    assert_eq!(r.hops.len(), 4);
    assert!(r.hops[0].hop_number == 1);
    crate::serial_println!("  [4/10] traceroute: OK");

    // 5: DNS lookup.
    let r = dns_lookup("example.com").expect("dns");
    assert!(!r.resolved.is_empty());
    assert_eq!(r.status, DiagStatus::Success);
    crate::serial_println!("  [5/10] DNS lookup: OK");

    // 6: Connectivity check.
    let status = connectivity_check().expect("check");
    assert_eq!(status, ConnectivityStatus::Connected);
    crate::serial_println!("  [6/10] connectivity: OK");

    // 7: Set connectivity.
    set_connectivity(ConnectivityStatus::NoInternet).expect("set");
    let status = connectivity_check().expect("check2");
    assert_eq!(status, ConnectivityStatus::NoInternet);
    set_connectivity(ConnectivityStatus::Connected).expect("restore");
    crate::serial_println!("  [7/10] set connectivity: OK");

    // 8: List results.
    let results = list_results(10);
    assert_eq!(results.len(), 4); // ping×2 + trace + dns
    crate::serial_println!("  [8/10] list results: OK");

    // 9: Get specific result.
    let first_id = results[0].id;
    let r = get_result(first_id).expect("get");
    assert_eq!(r.id, first_id);
    crate::serial_println!("  [9/10] get result: OK");

    // 10: Stats.
    let (count, pings, traces, lookups, ops) = stats();
    assert_eq!(count, 4);
    assert_eq!(pings, 2);
    assert_eq!(traces, 1);
    assert_eq!(lookups, 1);
    assert!(ops > 0);
    crate::serial_println!("  [10/10] stats: OK");

    crate::serial_println!("netdiag::self_test() — all 10 tests passed");
}
