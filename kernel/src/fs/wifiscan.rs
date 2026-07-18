//! Wi-Fi Scanner — wireless network scanning and management.
//!
//! Discovers, connects to, and manages Wi-Fi networks with
//! security classification and signal strength tracking.
//!
//! ## Architecture
//!
//! ```text
//! Wi-Fi management
//!   → wifiscan::scan() → discover networks
//!   → wifiscan::connect(ssid, password) → join network
//!   → wifiscan::disconnect() → leave network
//!
//! Integration:
//!   → netsettings (network configuration)
//!   → netindicator (status indicator)
//!   → credentials (stored passwords)
//!   → vpn (VPN overlay)
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

/// Wi-Fi security type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityType {
    Open,
    Wep,
    WpaPsk,
    Wpa2Psk,
    Wpa3Psk,
    Wpa2Enterprise,
    Wpa3Enterprise,
}

impl SecurityType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Wep => "WEP",
            Self::WpaPsk => "WPA-PSK",
            Self::Wpa2Psk => "WPA2-PSK",
            Self::Wpa3Psk => "WPA3-SAE",
            Self::Wpa2Enterprise => "WPA2-Enterprise",
            Self::Wpa3Enterprise => "WPA3-Enterprise",
        }
    }
}

/// Wi-Fi band.
//
// The `Band` prefix and `Ghz` suffix are the standard way to name Wi-Fi bands
// (2.4/5/6 GHz); stripping them would produce identifiers that cannot lead with
// a digit or that lose their unit, so the lint is suppressed here deliberately.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Band24Ghz,
    Band5Ghz,
    Band6Ghz,
}

impl Band {
    pub fn label(self) -> &'static str {
        match self {
            Self::Band24Ghz => "2.4 GHz",
            Self::Band5Ghz => "5 GHz",
            Self::Band6Ghz => "6 GHz",
        }
    }
}

/// Connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Scanning,
    Connecting,
    Authenticating,
    Connected,
    Failed,
}

impl ConnectionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Scanning => "Scanning",
            Self::Connecting => "Connecting",
            Self::Authenticating => "Authenticating",
            Self::Connected => "Connected",
            Self::Failed => "Failed",
        }
    }
}

/// A discovered Wi-Fi network.
#[derive(Debug, Clone)]
pub struct WifiNetwork {
    pub id: u32,
    pub ssid: String,
    pub bssid: String,
    pub security: SecurityType,
    pub band: Band,
    pub channel: u8,
    pub signal_dbm: i32,       // Negative dBm (e.g., -50 is strong).
    pub known: bool,           // Saved credentials.
    pub last_seen_ns: u64,
}

/// A saved network profile.
#[derive(Debug, Clone)]
pub struct SavedNetwork {
    pub ssid: String,
    pub security: SecurityType,
    pub auto_connect: bool,
    pub connect_count: u64,
    pub last_connected_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_NETWORKS: usize = 100;
const MAX_SAVED: usize = 50;

struct State {
    networks: Vec<WifiNetwork>,
    saved: Vec<SavedNetwork>,
    next_id: u32,
    connection_state: ConnectionState,
    connected_ssid: Option<String>,
    total_scans: u64,
    total_connections: u64,
    total_failures: u64,
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
        networks: Vec::new(),
        saved: Vec::new(),
        next_id: 1,
        connection_state: ConnectionState::Disconnected,
        connected_ssid: None,
        total_scans: 0,
        total_connections: 0,
        total_failures: 0,
        ops: 0,
    });
}

/// Simulate a scan (add discovered networks).
pub fn discover(ssid: &str, bssid: &str, security: SecurityType, band: Band, channel: u8, signal_dbm: i32) -> KernelResult<u32> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        // Update if already seen (by BSSID).
        if let Some(net) = state.networks.iter_mut().find(|n| n.bssid == bssid) {
            net.signal_dbm = signal_dbm;
            net.last_seen_ns = now;
            return Ok(net.id);
        }
        if state.networks.len() >= MAX_NETWORKS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        let known = state.saved.iter().any(|s| s.ssid == ssid);
        state.networks.push(WifiNetwork {
            id, ssid: String::from(ssid), bssid: String::from(bssid),
            security, band, channel, signal_dbm, known, last_seen_ns: now,
        });
        Ok(id)
    })
}

/// Run a scan (increment counter, mark state).
pub fn scan() -> KernelResult<usize> {
    with_state(|state| {
        state.total_scans += 1;
        Ok(state.networks.len())
    })
}

/// Connect to a network.
pub fn connect(ssid: &str) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        // Check network exists in scan results.
        let net = state.networks.iter().find(|n| n.ssid == ssid)
            .ok_or(KernelError::NotFound)?;
        // Simulate connection.
        state.connection_state = ConnectionState::Connected;
        state.connected_ssid = Some(String::from(ssid));
        state.total_connections += 1;

        // Update or create saved entry.
        if let Some(saved) = state.saved.iter_mut().find(|s| s.ssid == ssid) {
            saved.connect_count += 1;
            saved.last_connected_ns = now;
        } else {
            if state.saved.len() < MAX_SAVED {
                state.saved.push(SavedNetwork {
                    ssid: String::from(ssid),
                    security: net.security,
                    auto_connect: true,
                    connect_count: 1,
                    last_connected_ns: now,
                });
            }
        }
        // Mark network as known.
        for n in &mut state.networks {
            if n.ssid == ssid { n.known = true; }
        }
        Ok(())
    })
}

/// Disconnect from current network.
pub fn disconnect() -> KernelResult<()> {
    with_state(|state| {
        state.connection_state = ConnectionState::Disconnected;
        state.connected_ssid = None;
        Ok(())
    })
}

/// Forget a saved network.
pub fn forget(ssid: &str) -> KernelResult<()> {
    with_state(|state| {
        let before = state.saved.len();
        state.saved.retain(|s| s.ssid != ssid);
        if state.saved.len() == before { return Err(KernelError::NotFound); }
        for n in &mut state.networks {
            if n.ssid == ssid { n.known = false; }
        }
        Ok(())
    })
}

/// Get current connection state.
pub fn get_state() -> ConnectionState {
    STATE.lock().as_ref().map_or(ConnectionState::Disconnected, |s| s.connection_state)
}

/// Get connected SSID.
pub fn connected_ssid() -> Option<String> {
    STATE.lock().as_ref().and_then(|s| s.connected_ssid.clone())
}

/// List discovered networks (sorted by signal strength).
pub fn list_networks() -> Vec<WifiNetwork> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut nets = s.networks.clone();
        nets.sort_by_key(|e| core::cmp::Reverse(e.signal_dbm)); // Strongest first (least negative).
        nets
    })
}

/// List saved networks.
pub fn list_saved() -> Vec<SavedNetwork> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.saved.clone())
}

/// Statistics: (network_count, saved_count, total_scans, total_connections, total_failures, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.networks.len(), s.saved.len(), s.total_scans, s.total_connections, s.total_failures, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("wifiscan::self_test() — running tests...");

    // Residue-free: start from a clean, controlled State so assertions hold
    // regardless of prior kshell/procfs activity (init_defaults early-returns
    // when STATE is already populated).
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty initially.
    assert!(list_networks().is_empty());
    assert_eq!(get_state(), ConnectionState::Disconnected);
    crate::serial_println!("  [1/8] empty: OK");

    // 2: Discover networks.
    discover("HomeNet", "AA:BB:CC:DD:EE:01", SecurityType::Wpa2Psk, Band::Band5Ghz, 36, -45).expect("d1");
    discover("CoffeeShop", "AA:BB:CC:DD:EE:02", SecurityType::Open, Band::Band24Ghz, 6, -65).expect("d2");
    discover("Office5G", "AA:BB:CC:DD:EE:03", SecurityType::Wpa3Psk, Band::Band5Ghz, 149, -50).expect("d3");
    assert_eq!(list_networks().len(), 3);
    crate::serial_println!("  [2/8] discover: OK");

    // 3: Networks sorted by signal.
    let nets = list_networks();
    assert_eq!(nets[0].ssid, "HomeNet"); // -45 strongest.
    assert_eq!(nets[2].ssid, "CoffeeShop"); // -65 weakest.
    crate::serial_println!("  [3/8] sorting: OK");

    // 4: Connect.
    connect("HomeNet").expect("connect");
    assert_eq!(get_state(), ConnectionState::Connected);
    assert_eq!(connected_ssid(), Some(String::from("HomeNet")));
    crate::serial_println!("  [4/8] connect: OK");

    // 5: Saved networks.
    let saved = list_saved();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].ssid, "HomeNet");
    assert_eq!(saved[0].connect_count, 1);
    crate::serial_println!("  [5/8] saved: OK");

    // 6: Disconnect.
    disconnect().expect("disconnect");
    assert_eq!(get_state(), ConnectionState::Disconnected);
    assert_eq!(connected_ssid(), None);
    crate::serial_println!("  [6/8] disconnect: OK");

    // 7: Forget.
    forget("HomeNet").expect("forget");
    assert!(list_saved().is_empty());
    let nets = list_networks();
    let home = nets.iter().find(|n| n.ssid == "HomeNet").expect("home");
    assert!(!home.known);
    crate::serial_println!("  [7/8] forget: OK");

    // 8: Stats.
    let (networks, saved, _scans, connections, _failures, ops) = stats();
    assert_eq!(networks, 3);
    assert_eq!(saved, 0);
    assert_eq!(connections, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue for later callers / the live /proc/wifiscan view.
    *STATE.lock() = None;

    crate::serial_println!("wifiscan::self_test() — all 8 tests passed");
}
