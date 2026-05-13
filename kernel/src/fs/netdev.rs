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

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        ifaces: alloc::vec![
            IfaceStats { name: String::from("lo"), nic_type: NicType::Loopback, link_up: true, speed_mbps: 0, mtu: 65536, rx_bytes: 1_000_000_000, tx_bytes: 1_000_000_000, rx_packets: 5_000_000, tx_packets: 5_000_000, rx_errors: 0, tx_errors: 0, rx_drops: 0, tx_drops: 0, collisions: 0 },
            IfaceStats { name: String::from("eth0"), nic_type: NicType::Ethernet, link_up: true, speed_mbps: 1000, mtu: 1500, rx_bytes: 50_000_000_000, tx_bytes: 10_000_000_000, rx_packets: 100_000_000, tx_packets: 50_000_000, rx_errors: 500, tx_errors: 100, rx_drops: 1000, tx_drops: 200, collisions: 50 },
            IfaceStats { name: String::from("wlan0"), nic_type: NicType::Wifi, link_up: false, speed_mbps: 0, mtu: 1500, rx_bytes: 0, tx_bytes: 0, rx_packets: 0, tx_packets: 0, rx_errors: 0, tx_errors: 0, rx_drops: 0, tx_drops: 0, collisions: 0 },
        ],
        total_rx_bytes: 51_000_000_000,
        total_tx_bytes: 11_000_000_000,
        total_errors: 600,
        total_drops: 1200,
        ops: 0,
    });
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(list().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Record RX.
    let before = get("eth0").unwrap().rx_bytes;
    record_rx("eth0", 1500, 1).expect("rx");
    let after = get("eth0").unwrap().rx_bytes;
    assert_eq!(after, before + 1500);
    crate::serial_println!("  [2/8] rx: OK");

    // 3: Record TX.
    let before = get("eth0").unwrap().tx_bytes;
    record_tx("eth0", 1000, 1).expect("tx");
    let after = get("eth0").unwrap().tx_bytes;
    assert_eq!(after, before + 1000);
    crate::serial_println!("  [3/8] tx: OK");

    // 4: Error.
    let before = get("eth0").unwrap().rx_errors;
    record_error("eth0", true).expect("error");
    let after = get("eth0").unwrap().rx_errors;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [4/8] error: OK");

    // 5: Drop.
    let before = get("eth0").unwrap().tx_drops;
    record_drop("eth0", false).expect("drop");
    let after = get("eth0").unwrap().tx_drops;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [5/8] drop: OK");

    // 6: Link state.
    set_link_state("wlan0", true).expect("link_up");
    assert!(get("wlan0").unwrap().link_up);
    set_link_state("wlan0", false).expect("link_down");
    assert!(!get("wlan0").unwrap().link_up);
    crate::serial_println!("  [6/8] link state: OK");

    // 7: Not found.
    assert!(record_rx("fake", 0, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (ifaces, rx, tx, errors, drops, ops) = stats();
    assert_eq!(ifaces, 3);
    assert!(rx > 51_000_000_000);
    assert!(tx > 11_000_000_000);
    assert!(errors > 600);
    assert!(drops > 1200);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("netdev::self_test() — all 8 tests passed");
}
