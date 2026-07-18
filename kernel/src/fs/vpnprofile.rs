//! VPN Profile — VPN connection profile management.
//!
//! Manages multiple VPN configurations including protocol settings,
//! credentials, auto-connect rules, and connection statistics.
//!
//! ## Architecture
//!
//! ```text
//! VPN profile management
//!   → vpnprofile::create(params) → new VPN profile
//!   → vpnprofile::connect(id) → establish connection
//!   → vpnprofile::get_status(id) → connection state
//!
//! Integration:
//!   → vpn (VPN connections)
//!   → netprofile (network profiles)
//!   → netsettings (network settings)
//!   → credentials (credential storage)
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

/// VPN protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VpnProtocol {
    OpenVpn,
    WireGuard,
    IpSec,
    L2tp,
    Sstp,
    Pptp,
}

impl VpnProtocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::OpenVpn => "OpenVPN",
            Self::WireGuard => "WireGuard",
            Self::IpSec => "IPSec",
            Self::L2tp => "L2TP",
            Self::Sstp => "SSTP",
            Self::Pptp => "PPTP",
        }
    }
}

/// Connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Error,
}

impl ConnectionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Connecting => "Connecting",
            Self::Connected => "Connected",
            Self::Reconnecting => "Reconnecting",
            Self::Error => "Error",
        }
    }
}

/// A VPN profile.
#[derive(Debug, Clone)]
pub struct VpnProfile {
    pub id: u32,
    pub name: String,
    pub protocol: VpnProtocol,
    pub server: String,
    pub port: u16,
    pub state: ConnectionState,
    pub auto_connect: bool,
    pub kill_switch: bool,
    pub dns_override: Option<String>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub connected_ns: u64,
    pub total_connections: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PROFILES: usize = 50;

struct State {
    profiles: Vec<VpnProfile>,
    next_id: u32,
    total_created: u64,
    total_connects: u64,
    total_disconnects: u64,
    total_errors: u64,
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
    // Start with no profiles. A VPN profile is user-specific configuration —
    // a server address, protocol, port, and credentials the user entered.
    // There is no sensible "default" VPN, so seeding a "Work VPN" pointing at
    // vpn.work.example.com would surface a fabricated, never-created profile
    // (a privacy/security surface) through /proc and the `vpn` shell command as
    // if the user had configured it. Profiles appear only via create_profile().
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        profiles: Vec::new(),
        next_id: 1,
        total_created: 0,
        total_connects: 0,
        total_disconnects: 0,
        total_errors: 0,
        ops: 0,
    });
}

/// Create a VPN profile.
pub fn create_profile(name: &str, protocol: VpnProtocol, server: &str, port: u16) -> KernelResult<u32> {
    with_state(|state| {
        if state.profiles.len() >= MAX_PROFILES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.profiles.push(VpnProfile {
            id, name: String::from(name), protocol, server: String::from(server),
            port, state: ConnectionState::Disconnected, auto_connect: false,
            kill_switch: false, dns_override: None,
            bytes_sent: 0, bytes_received: 0, connected_ns: 0, total_connections: 0,
        });
        state.total_created += 1;
        Ok(id)
    })
}

/// Delete a profile.
pub fn delete_profile(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.profiles.len();
        state.profiles.retain(|p| p.id != id);
        if state.profiles.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Connect.
pub fn connect(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let profile = state.profiles.iter_mut().find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        profile.state = ConnectionState::Connected;
        profile.connected_ns = now;
        profile.total_connections += 1;
        state.total_connects += 1;
        Ok(())
    })
}

/// Disconnect.
pub fn disconnect(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        profile.state = ConnectionState::Disconnected;
        state.total_disconnects += 1;
        Ok(())
    })
}

/// Set auto-connect.
pub fn set_auto_connect(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        profile.auto_connect = enabled;
        Ok(())
    })
}

/// Set kill switch.
pub fn set_kill_switch(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        profile.kill_switch = enabled;
        Ok(())
    })
}

/// Record traffic (simulation).
pub fn record_traffic(id: u32, sent: u64, received: u64) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        profile.bytes_sent += sent;
        profile.bytes_received += received;
        Ok(())
    })
}

/// List profiles.
pub fn list_profiles() -> Vec<VpnProfile> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.profiles.clone())
}

/// Get a profile.
pub fn get_profile(id: u32) -> Option<VpnProfile> {
    STATE.lock().as_ref().and_then(|s| s.profiles.iter().find(|p| p.id == id).cloned())
}

/// Statistics: (profile_count, total_connects, total_disconnects, total_errors, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.profiles.len(), s.total_connects, s.total_disconnects, s.total_errors, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("vpnprofile::self_test() — running tests...");

    // Residue-free: start from a clean, controlled State so assertions hold
    // regardless of prior kshell/procfs activity.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no profiles until the user creates one.
    assert_eq!(list_profiles().len(), 0);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Create profile via the real API.
    let id = create_profile("Home VPN", VpnProtocol::OpenVpn, "home.vpn.net", 1194).expect("create");
    assert_eq!(list_profiles().len(), 1);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Connect.
    connect(id).expect("connect");
    let p = get_profile(id).expect("get");
    assert_eq!(p.state, ConnectionState::Connected);
    assert_eq!(p.total_connections, 1);
    crate::serial_println!("  [3/8] connect: OK");

    // 4: Traffic.
    record_traffic(id, 1_000_000, 5_000_000).expect("traffic");
    let p = get_profile(id).expect("get2");
    assert_eq!(p.bytes_sent, 1_000_000);
    assert_eq!(p.bytes_received, 5_000_000);
    crate::serial_println!("  [4/8] traffic: OK");

    // 5: Disconnect.
    disconnect(id).expect("disconnect");
    let p = get_profile(id).expect("get3");
    assert_eq!(p.state, ConnectionState::Disconnected);
    crate::serial_println!("  [5/8] disconnect: OK");

    // 6: Kill switch.
    set_kill_switch(id, true).expect("ks");
    set_auto_connect(id, true).expect("ac");
    let p = get_profile(id).expect("get4");
    assert!(p.kill_switch);
    assert!(p.auto_connect);
    crate::serial_println!("  [6/8] settings: OK");

    // 7: Delete — back to an empty profile set.
    delete_profile(id).expect("delete");
    assert_eq!(list_profiles().len(), 0);
    crate::serial_println!("  [7/8] delete: OK");

    // 8: Stats.
    let (count, connects, disconnects, _errors, ops) = stats();
    assert_eq!(count, 0);
    assert_eq!(connects, 1);
    assert_eq!(disconnects, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue for later callers / boot-time tests.
    *STATE.lock() = None;

    crate::serial_println!("vpnprofile::self_test() — all 8 tests passed");
}
