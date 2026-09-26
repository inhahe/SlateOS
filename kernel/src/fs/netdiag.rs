//! Network Diagnostics — network troubleshooting and analysis tools.
//!
//! Provides traceroute, connectivity checks, DNS diagnostics, speed
//! testing, and network path analysis from the OS level.
//!
//! ## Architecture
//!
//! ```text
//! User runs diagnostic
//!   → netdiag::ping(host) → NotSupported (never implemented)
//!   → netdiag::traceroute(host) → NotSupported (never implemented)
//!   → netdiag::dns_lookup(name) → NotSupported (never implemented)
//!
//! Those three invented their answers until 2026-09-21: ping from the
//! spelling of the host, traceroute from a fixed hop list, dns_lookup from
//! a hardcoded case. They now refuse, because a diagnostic that answers
//! from the shape of its input cannot report the condition it exists to
//! detect. `connectivity_check` below is unaffected: it reports a stored
//! field faithfully, and its own gap is that nothing updates that field.
//!   → netdiag::connectivity_check() → internet reachability
//!
//! Integration:
//!   → netsettings (network config)
//!   → sysdiag (system diagnostics)
//!   → crashreport (network failure info)
//! ```

#![allow(dead_code)]

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

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
    if guard.is_some() {
        return;
    }
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
pub fn ping(_host: &str, _count: u32) -> KernelResult<DiagResult> {
    // Refuses because no ICMP path exists. Until 2026-09-21 this returned
    // a latency derived from the SPELLING of the host -- `127.*` or
    // `localhost` got 50us, `192.168.*`/`10.*` got 1500us, anything else
    // 25000us -- and reported Success. No packet was ever sent.
    //
    // A diagnostic that answers from the shape of its input cannot report
    // the one condition it exists to detect: `ping 10.0.0.99` on a network
    // with no such host reported 1.5ms and success.
    //
    // The counter is deliberately not incremented. A refused ping is not a
    // ping, and `/proc/netdiag` showing `total_pings` above zero for work
    // never attempted would be this defect one level down.
    Err(KernelError::NotSupported)
}

pub fn traceroute(_host: &str) -> KernelResult<DiagResult> {
    // Refuses. Until 2026-09-21 this returned a fixed four-hop list --
    // 192.168.1.1 labelled `gateway`, then 10.0.0.1 -- regardless of the
    // destination asked for. It carried no `Simulate` comment, which is why
    // a scan for that marker missed it: the comment is a proxy, the code is
    // the signal.
    Err(KernelError::NotSupported)
}

pub fn dns_lookup(_name: &str) -> KernelResult<DiagResult> {
    // Refuses. Until 2026-09-21 this hardcoded `localhost` -> 127.0.0.1 and
    // invented an address for everything else. The hardcoded pair duplicated
    // the hosts table, so the tool agreed with the real resolver by
    // coincidence of two constants and would have kept agreeing after
    // someone edited the hosts file.
    //
    // Real resolution lives behind `SYS_DNS_RESOLVE`, which consults
    // `fs::nameservice` (Cache, Files, Dns) as of the same day.
    Err(KernelError::NotSupported)
}

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
        state
            .results
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or(KernelError::NotFound)
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
        Some(s) => (
            s.results.len(),
            s.total_pings,
            s.total_traces,
            s.total_lookups,
            s.ops,
        ),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run the module's self-test suite against a table of its own.
///
/// The suite mutates module state and asserts exact contents, and it used to
/// do that to the *live* table -- which, since it is also a kernel-shell
/// subcommand, changed or destroyed whatever the user had here and then
/// reported success.  The live state is moved aside for the duration and put
/// back afterwards; `crate::fs::selftest` records why this shape rather than
/// the alternatives.
///
/// The pristine value is `None` rather than a table: this module initialises
/// lazily, and `None` is exactly what a fresh boot holds.
pub fn self_test() -> crate::error::KernelResult<()> {
    // `OPS` is a lock-free mirror of `state.ops`, which lives *inside* the
    // table. `with_pristine` restores the table and so restores `state.ops`,
    // but it cannot know about the mirror -- leave it and the two disagree
    // permanently, with `<module> stats` reporting the suite's activity as
    // the user's.
    let saved_ops = OPS.load(Ordering::Relaxed);
    crate::fs::selftest::with_pristine(&STATE, None, self_test_inner);
    OPS.store(saved_ops, Ordering::Relaxed);
    Ok(())
}

fn self_test_inner() {
    crate::serial_println!("netdiag::self_test() — running tests...");
    init_defaults();

    // 1: No results initially.
    assert!(list_results(10).is_empty());
    crate::serial_println!("  [1/10] empty initial: OK");

    // 2-5: the three inventing diagnostics must REFUSE.
    //
    // These rungs used to assert the fabricated constants -- latency 50 for
    // anything spelled `127.*`, 25000 for anything else, a four-hop list, a
    // non-empty resolved address. They were green on every boot and would
    // have stayed green if the network stack were deleted, because they
    // tested a lookup table.
    //
    // Asserting the refusal is a real assertion: it fails if someone
    // reintroduces an invented answer, which is the regression that matters.
    assert!(ping("127.0.0.1", 4).is_err());
    assert!(ping("example.com", 4).is_err());
    crate::serial_println!("  [2/10] ping refuses (no ICMP path): OK");
    crate::serial_println!("  [3/10] ping refuses for remote too: OK");

    assert!(traceroute("example.com").is_err());
    crate::serial_println!("  [4/10] traceroute refuses (no hop discovery): OK");

    assert!(dns_lookup("example.com").is_err());
    crate::serial_println!("  [5/10] dns_lookup refuses (use SYS_DNS_RESOLVE): OK");

    // And a refusal must not have counted as work: `/proc/netdiag` reporting
    // pings it never sent would be the same defect one level down.
    let (_, pings_after, traces_after, lookups_after, _) = stats();
    assert_eq!(pings_after, 0);
    assert_eq!(traces_after, 0);
    assert_eq!(lookups_after, 0);
    crate::serial_println!("  [5b/10] refusals did not increment the counters: OK");

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

    // 8: The refusals recorded nothing.
    //
    // Steps 8-10 used to assert four stored results and ping/trace/lookup
    // counts of 2/1/1 -- the invented answers' bookkeeping. When the three
    // diagnostics began refusing (2026-09-21) step 5b was added to assert the
    // opposite and these three were left as they were, so the suite asserted
    // both "nothing was counted" and "four results were counted": the first
    // boot to reach it would have panicked on `assert_eq!(0, 4)`. None did --
    // the two boots after the change stopped earlier -- which is how it
    // survived four days.
    let results = list_results(10);
    assert!(results.is_empty(), "a refused diagnostic stored a result");
    crate::serial_println!("  [8/10] refusals stored no results: OK");

    // 9: Looking up a result that was never stored is refused, not invented.
    assert!(matches!(get_result(1), Err(KernelError::NotFound)));
    crate::serial_println!("  [9/10] get_result of a missing id is NotFound: OK");

    // 10: Stats count only what happened: no results and no diagnostics, but
    // the connectivity calls above did go through the table.
    let (count, pings, traces, lookups, ops) = stats();
    assert_eq!(count, 0);
    assert_eq!(pings, 0);
    assert_eq!(traces, 0);
    assert_eq!(lookups, 0);
    assert!(ops > 0);
    crate::serial_println!("  [10/10] stats: OK");

    crate::serial_println!("netdiag::self_test() — all 10 tests passed");
}
