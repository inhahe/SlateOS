//! Network Device — NIC-level packet/byte statistics.
//!
//! Tracks per-interface packet counts, byte totals, errors,
//! drops, and link state. Essential for diagnosing network
//! hardware and driver performance.
//!
//! ## Architecture
//!
//! ```text
//! Network device monitoring
//!   → netdev::record_rx(iface, bytes, pkts) → track received
//!   → netdev::record_tx(iface, bytes, pkts) → track transmitted
//!   → netdev::record_error(iface, dir) → track errors
//!   → netdev::set_link_state(iface, up) → link state change
//!
//! Integration:
//!   → netmon (network monitor)
//!   → netsock (socket stats)
//!   → netfilter (packet filtering)
//!   → sysdiag (diagnostics)
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

/// Network interface type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NicType {
    Ethernet,
    Wifi,
    Loopback,
    Bridge,
    Veth,
    Tun,
}

impl NicType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ethernet => "ethernet",
            Self::Wifi => "wifi",
            Self::Loopback => "loopback",
            Self::Bridge => "bridge",
            Self::Veth => "veth",
            Self::Tun => "tun",
        }
    }
}

/// Per-interface statistics.
#[derive(Debug, Clone)]
pub struct IfaceStats {
    pub name: String,
    pub nic_type: NicType,
    pub link_up: bool,
    pub speed_mbps: u32,
    pub mtu: u32,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub rx_drops: u64,
    pub tx_drops: u64,
    pub collisions: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_IFACES: usize = 32;

struct State {
    ifaces: Vec<IfaceStats>,
    total_rx_bytes: u64,
    total_tx_bytes: u64,
    total_errors: u64,
    total_drops: u64,
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

/// Initialise an **empty** network-device table.
///
/// Seeds NO interfaces and zero counters.  Real interfaces are wired through
/// [`register_iface`] (one row per NIC the network stack brings up) and their
/// counters through the `record_rx`/`record_tx`/`record_error`/`record_drop`
/// functions; until those are called the table is genuinely empty, so
/// `/proc/netdev` and the `netdev` kshell command report nothing rather than
/// fabricated numbers — the kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded three fictional interfaces ("lo": loopback /
/// 1 GB rx+tx / 5M packets each; "eth0": 1 Gbps ethernet / 50 GB rx / 10 GB tx /
/// 100M rx packets / 50M tx packets / 500 rx errors / 100 tx errors / 1000 rx
/// drops / 200 tx drops / 50 collisions; "wlan0": idle wifi) plus invented
/// aggregate totals (total_rx_bytes 51 GB, total_tx_bytes 11 GB, total_errors
/// 600, total_drops 1200), which `/proc/netdev` (and the `list`/`get` views) then
/// displayed as if they were real measured NIC traffic.  That demo data was
/// removed; the self-test now builds its own fixtures explicitly via the real API
/// (see [`self_test`]).  The network stack is expected to call [`register_iface`]
/// when an interface comes up and the record functions on every packet event.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        ifaces: Vec::new(),
        total_rx_bytes: 0,
        total_tx_bytes: 0,
        total_errors: 0,
        total_drops: 0,
        ops: 0,
    });
}

/// Register a network interface.
///
/// Creates a zeroed [`IfaceStats`] row with the supplied link parameters
/// (`nic_type`, `speed_mbps`, `mtu`); the link starts down with all traffic
/// counters at zero.  Duplicate interface names return
/// [`KernelError::AlreadyExists`]; exceeding [`MAX_IFACES`] returns
/// [`KernelError::ResourceExhausted`].
pub fn register_iface(name: &str, nic_type: NicType, speed_mbps: u32, mtu: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.ifaces.len() >= MAX_IFACES { return Err(KernelError::ResourceExhausted); }
        if state.ifaces.iter().any(|d| d.name == name) { return Err(KernelError::AlreadyExists); }
        state.ifaces.push(IfaceStats {
            name: String::from(name), nic_type, link_up: false, speed_mbps, mtu,
            rx_bytes: 0, tx_bytes: 0, rx_packets: 0, tx_packets: 0,
            rx_errors: 0, tx_errors: 0, rx_drops: 0, tx_drops: 0, collisions: 0,
        });
        Ok(())
    })
}

/// Record received traffic.
pub fn record_rx(iface: &str, bytes: u64, packets: u64) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.ifaces.iter_mut().find(|d| d.name == iface)
            .ok_or(KernelError::NotFound)?;
        dev.rx_bytes += bytes;
        dev.rx_packets += packets;
        state.total_rx_bytes += bytes;
        Ok(())
    })
}

/// Record transmitted traffic.
pub fn record_tx(iface: &str, bytes: u64, packets: u64) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.ifaces.iter_mut().find(|d| d.name == iface)
            .ok_or(KernelError::NotFound)?;
        dev.tx_bytes += bytes;
        dev.tx_packets += packets;
        state.total_tx_bytes += bytes;
        Ok(())
    })
}

/// Record an error.
pub fn record_error(iface: &str, is_rx: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.ifaces.iter_mut().find(|d| d.name == iface)
            .ok_or(KernelError::NotFound)?;
        if is_rx { dev.rx_errors += 1; } else { dev.tx_errors += 1; }
        state.total_errors += 1;
        Ok(())
    })
}

/// Record a drop.
pub fn record_drop(iface: &str, is_rx: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.ifaces.iter_mut().find(|d| d.name == iface)
            .ok_or(KernelError::NotFound)?;
        if is_rx { dev.rx_drops += 1; } else { dev.tx_drops += 1; }
        state.total_drops += 1;
        Ok(())
    })
}

/// Set link state.
pub fn set_link_state(iface: &str, up: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.ifaces.iter_mut().find(|d| d.name == iface)
            .ok_or(KernelError::NotFound)?;
        dev.link_up = up;
        Ok(())
    })
}

/// List all interfaces.
pub fn list() -> Vec<IfaceStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.ifaces.clone())
}

/// Get specific interface.
pub fn get(iface: &str) -> Option<IfaceStats> {
    STATE.lock().as_ref().and_then(|s| {
        s.ifaces.iter().find(|d| d.name == iface).cloned()
    })
}

/// Statistics: (iface_count, total_rx_bytes, total_tx_bytes, total_errors, total_drops, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.ifaces.len(), s.total_rx_bytes, s.total_tx_bytes, s.total_errors, s.total_drops, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("netdev::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/netdev must never surface).  Resetting
    // first clears any residue from a prior `netdev test` run so the totals
    // asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated interfaces or counters; record on an
    // unregistered iface fails.
    assert_eq!(list().len(), 0);
    let (c0, rx0, tx0, e0, d0, _o0) = stats();
    assert_eq!((c0, rx0, tx0, e0, d0), (0, 0, 0, 0, 0));
    assert!(record_rx("eth0", 1, 1).is_err()); // no phantom iface exists yet
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register — zeroed counters, link down, params preserved; dup fails.
    register_iface("eth0", NicType::Ethernet, 1000, 1500).expect("register");
    let d = get("eth0").expect("get");
    assert_eq!(d.nic_type, NicType::Ethernet);
    assert_eq!((d.speed_mbps, d.mtu), (1000, 1500));
    assert!(!d.link_up);
    assert_eq!((d.rx_bytes, d.tx_bytes, d.rx_packets), (0, 0, 0));
    assert!(register_iface("eth0", NicType::Ethernet, 1000, 1500).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Record RX — bytes + packets accumulate; total_rx rises.
    record_rx("eth0", 1500, 1).expect("rx");
    record_rx("eth0", 500, 1).expect("rx2");
    let d = get("eth0").expect("get");
    assert_eq!(d.rx_bytes, 2000);
    assert_eq!(d.rx_packets, 2);
    crate::serial_println!("  [3/8] rx: OK");

    // 4: Record TX — independent counters.
    record_tx("eth0", 1000, 1).expect("tx");
    let d = get("eth0").expect("get");
    assert_eq!(d.tx_bytes, 1000);
    assert_eq!(d.tx_packets, 1);
    crate::serial_println!("  [4/8] tx: OK");

    // 5: Error/drop direction routing — rx vs tx counters update correctly.
    record_error("eth0", true).expect("rx error");
    record_error("eth0", false).expect("tx error");
    record_drop("eth0", true).expect("rx drop");
    record_drop("eth0", false).expect("tx drop");
    let d = get("eth0").expect("get");
    assert_eq!((d.rx_errors, d.tx_errors), (1, 1));
    assert_eq!((d.rx_drops, d.tx_drops), (1, 1));
    crate::serial_println!("  [5/8] error/drop: OK");

    // 6: Link state toggles.
    set_link_state("eth0", true).expect("link_up");
    assert!(get("eth0").expect("get").link_up);
    set_link_state("eth0", false).expect("link_down");
    assert!(!get("eth0").expect("get").link_up);
    crate::serial_println!("  [6/8] link state: OK");

    // 7: Unknown iface → NotFound on every record/link path.
    assert!(record_rx("fake", 0, 0).is_err());
    assert!(record_tx("fake", 0, 0).is_err());
    assert!(record_error("fake", true).is_err());
    assert!(record_drop("fake", true).is_err());
    assert!(set_link_state("fake", true).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate totals are exact: rx 2000, tx 1000, 2 errors, 2 drops.
    let (ifaces, rx, tx, errors, drops, ops) = stats();
    assert_eq!(ifaces, 1);
    assert_eq!(rx, 2000);
    assert_eq!(tx, 1000);
    assert_eq!(errors, 2);
    assert_eq!(drops, 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/netdev table.
    *STATE.lock() = None;

    crate::serial_println!("netdev::self_test() — all 8 tests passed");
}
