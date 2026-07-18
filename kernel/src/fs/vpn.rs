//! VPN management — detect, configure, and control VPN connections.
//!
//! Manages VPN connections including OpenVPN, WireGuard, and third-party
//! VPN clients.  Provides detection of active VPN tunnels and a settings
//! interface for configuration.
//!
//! ## Design Reference
//!
//! design.txt line 1268: VPN - OpenVPN settings, show if currently using a VPN,
//!   including any by any third-party app and what the app is and button to
//!   launch the app's interface
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Network → VPN
//!   → vpn::list_connections() → configured VPN profiles
//!   → vpn::status() → current connection state
//!   → vpn::connect(id) → initiate connection
//!   → vpn::disconnect() → tear down tunnel
//!
//! Network indicator
//!   → vpn::is_active() → show VPN icon in system tray
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
    /// OpenVPN (UDP or TCP).
    OpenVpn,
    /// WireGuard.
    WireGuard,
    /// IPSec/IKEv2.
    IpSec,
    /// L2TP over IPSec.
    L2tp,
    /// PPTP (legacy, insecure).
    Pptp,
    /// SSH tunnel.
    SshTunnel,
    /// Third-party app (unknown protocol).
    ThirdParty,
}

/// VPN connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VpnState {
    /// Not connected.
    Disconnected,
    /// Connection attempt in progress.
    Connecting,
    /// Connected and tunnel is active.
    Connected,
    /// Disconnecting.
    Disconnecting,
    /// Connection failed.
    Failed,
    /// Reconnecting after drop.
    Reconnecting,
}

/// OpenVPN transport protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Udp,
    Tcp,
}

/// Authentication method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    /// Username + password.
    UserPass,
    /// Certificate-based.
    Certificate,
    /// Pre-shared key.
    PreSharedKey,
    /// Token / OTP.
    Token,
    /// External (third-party app handles auth).
    External,
}

/// A VPN connection profile.
#[derive(Debug, Clone)]
pub struct VpnProfile {
    /// Unique ID.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// Protocol.
    pub protocol: VpnProtocol,
    /// Remote server address.
    pub server: String,
    /// Remote port.
    pub port: u16,
    /// Transport (for OpenVPN).
    pub transport: Transport,
    /// Authentication method.
    pub auth: AuthMethod,
    /// Username (if user/pass auth).
    pub username: String,
    /// Certificate path (if cert auth).
    pub cert_path: String,
    /// Key path.
    pub key_path: String,
    /// CA certificate path.
    pub ca_path: String,
    /// DNS servers to use when connected.
    pub dns_servers: Vec<String>,
    /// Whether to route all traffic through VPN.
    pub route_all: bool,
    /// Kill switch — block internet if VPN drops.
    pub kill_switch: bool,
    /// Auto-connect on startup.
    pub auto_connect: bool,
    /// Auto-reconnect on disconnect.
    pub auto_reconnect: bool,
    /// Reconnect delay in seconds.
    pub reconnect_delay_s: u32,
    /// Whether this is a system/built-in profile.
    pub system: bool,
}

/// Information about a detected third-party VPN.
#[derive(Debug, Clone)]
pub struct ThirdPartyVpn {
    /// Detected app name.
    pub app_name: String,
    /// Process ID (if running).
    pub pid: Option<u64>,
    /// Whether it's currently connected.
    pub connected: bool,
    /// Interface name (e.g., "tun0").
    pub interface: String,
    /// Path to the app binary.
    pub app_path: String,
}

/// Current VPN status snapshot.
#[derive(Debug, Clone)]
pub struct VpnStatus {
    /// State of our managed connection.
    pub state: VpnState,
    /// Active profile ID (if connected/connecting).
    pub active_profile: Option<u64>,
    /// Connected server address.
    pub connected_server: String,
    /// Our VPN IP address.
    pub vpn_ip: String,
    /// Connection uptime in seconds.
    pub uptime_s: u64,
    /// Bytes sent.
    pub bytes_sent: u64,
    /// Bytes received.
    pub bytes_received: u64,
    /// Detected third-party VPNs.
    pub third_party: Vec<ThirdPartyVpn>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    profiles: Vec<VpnProfile>,
    status: VpnStatus,
    third_party: Vec<ThirdPartyVpn>,
    changes: u64,
}

fn default_status() -> VpnStatus {
    VpnStatus {
        state: VpnState::Disconnected,
        active_profile: None,
        connected_server: String::new(),
        vpn_ip: String::new(),
        uptime_s: 0,
        bytes_sent: 0,
        bytes_received: 0,
        third_party: Vec::new(),
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    profiles: Vec::new(),
    status: VpnStatus {
        state: VpnState::Disconnected,
        active_profile: None,
        connected_server: String::new(),
        vpn_ip: String::new(),
        uptime_s: 0,
        bytes_sent: 0,
        bytes_received: 0,
        third_party: Vec::new(),
    },
    third_party: Vec::new(),
    changes: 0,
});

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Profile management
// ---------------------------------------------------------------------------

/// Create a VPN profile.
pub fn create_profile(
    name: &str,
    protocol: VpnProtocol,
    server: &str,
    port: u16,
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.profiles.len() >= 64 {
        return Err(KernelError::ResourceExhausted);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.profiles.push(VpnProfile {
        id,
        name: String::from(name),
        protocol,
        server: String::from(server),
        port,
        transport: Transport::Udp,
        auth: AuthMethod::UserPass,
        username: String::new(),
        cert_path: String::new(),
        key_path: String::new(),
        ca_path: String::new(),
        dns_servers: Vec::new(),
        route_all: true,
        kill_switch: false,
        auto_connect: false,
        auto_reconnect: true,
        reconnect_delay_s: 5,
        system: false,
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Remove a VPN profile.
pub fn remove_profile(profile_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    if !state.profiles.iter().any(|p| p.id == profile_id) {
        return Err(KernelError::NotFound);
    }
    // Cannot remove while connected.
    if state.status.active_profile == Some(profile_id) &&
       state.status.state == VpnState::Connected {
        return Err(KernelError::PermissionDenied);
    }
    state.profiles.retain(|p| p.id != profile_id);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get a profile.
pub fn get_profile(profile_id: u64) -> KernelResult<VpnProfile> {
    STATE.lock().profiles.iter().find(|p| p.id == profile_id).cloned()
        .ok_or(KernelError::NotFound)
}

/// List all profiles.
pub fn list_profiles() -> Vec<VpnProfile> {
    STATE.lock().profiles.clone()
}

// ---------------------------------------------------------------------------
// Profile configuration
// ---------------------------------------------------------------------------

/// Set authentication method.
pub fn set_auth(profile_id: u64, auth: AuthMethod) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.auth = auth;
    state.changes += 1;
    Ok(())
}

/// Set username.
pub fn set_username(profile_id: u64, username: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.username = String::from(username);
    state.changes += 1;
    Ok(())
}

/// Set certificate paths.
pub fn set_certs(profile_id: u64, cert: &str, key: &str, ca: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.cert_path = String::from(cert);
    p.key_path = String::from(key);
    p.ca_path = String::from(ca);
    state.changes += 1;
    Ok(())
}

/// Set transport (UDP/TCP).
pub fn set_transport(profile_id: u64, transport: Transport) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.transport = transport;
    state.changes += 1;
    Ok(())
}

/// Set route-all flag.
pub fn set_route_all(profile_id: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.route_all = enabled;
    state.changes += 1;
    Ok(())
}

/// Set kill switch.
pub fn set_kill_switch(profile_id: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.kill_switch = enabled;
    state.changes += 1;
    Ok(())
}

/// Set auto-connect.
pub fn set_auto_connect(profile_id: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.auto_connect = enabled;
    state.changes += 1;
    Ok(())
}

/// Set auto-reconnect.
pub fn set_auto_reconnect(profile_id: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.auto_reconnect = enabled;
    state.changes += 1;
    Ok(())
}

/// Add a DNS server to a profile.
pub fn add_dns(profile_id: u64, server: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    if p.dns_servers.len() >= 8 {
        return Err(KernelError::ResourceExhausted);
    }
    p.dns_servers.push(String::from(server));
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Connection management
// ---------------------------------------------------------------------------

/// Initiate a VPN connection.
pub fn connect(profile_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let server_name = state.profiles.iter().find(|p| p.id == profile_id)
        .map(|p| p.server.clone())
        .ok_or(KernelError::NotFound)?;
    if state.status.state == VpnState::Connected {
        return Err(KernelError::AlreadyExists);
    }
    // Simulate connection (would invoke actual VPN daemon in real system).
    state.status.state = VpnState::Connected;
    state.status.active_profile = Some(profile_id);
    state.status.connected_server = server_name;
    state.status.vpn_ip = String::from("10.8.0.2");
    state.status.uptime_s = 0;
    state.status.bytes_sent = 0;
    state.status.bytes_received = 0;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Disconnect the active VPN.
pub fn disconnect() -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.status.state != VpnState::Connected &&
       state.status.state != VpnState::Connecting {
        return Err(KernelError::NotFound);
    }
    state.status.state = VpnState::Disconnected;
    state.status.active_profile = None;
    state.status.connected_server = String::new();
    state.status.vpn_ip = String::new();
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get current VPN status.
pub fn status() -> VpnStatus {
    let state = STATE.lock();
    let mut s = state.status.clone();
    s.third_party = state.third_party.clone();
    s
}

/// Whether any VPN (managed or third-party) is active.
pub fn is_active() -> bool {
    let state = STATE.lock();
    state.status.state == VpnState::Connected ||
        state.third_party.iter().any(|t| t.connected)
}

/// Register a detected third-party VPN.
pub fn register_third_party(app_name: &str, app_path: &str, interface: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.third_party.len() >= 16 {
        return Err(KernelError::ResourceExhausted);
    }
    state.third_party.push(ThirdPartyVpn {
        app_name: String::from(app_name),
        pid: None,
        connected: false,
        interface: String::from(interface),
        app_path: String::from(app_path),
    });
    state.changes += 1;
    Ok(())
}

/// Update third-party VPN status.
pub fn update_third_party(app_name: &str, connected: bool, pid: Option<u64>) -> KernelResult<()> {
    let mut state = STATE.lock();
    let tp = state.third_party.iter_mut().find(|t| t.app_name == app_name)
        .ok_or(KernelError::NotFound)?;
    tp.connected = connected;
    tp.pid = pid;
    state.changes += 1;
    Ok(())
}

/// List third-party VPNs.
pub fn list_third_party() -> Vec<ThirdPartyVpn> {
    STATE.lock().third_party.clone()
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

/// Initialise with example profiles.
pub fn init_defaults() {
    // No default VPN profiles. A VPN profile is user-specific configuration —
    // a server address, protocol, port, and credentials/cert paths the user
    // supplies. The OS ships none. Seeding "Home OpenVPN"/"Work WireGuard"
    // pointing at example.com with /etc/vpn cert paths and system: true would
    // surface fabricated, never-created profiles (a privacy/security surface)
    // through /proc and the `vpn` shell command as if the user had configured
    // them. The static STATE already starts empty; profiles appear only via
    // create_profile(). This stays a documented no-op so the `vpn init` shell
    // command and existing call sites remain valid.
}

/// Return (profile_count, connected, third_party_count, ops).
pub fn conn_stats() -> (usize, bool, usize, u64) {
    let state = STATE.lock();
    let total = state.profiles.len();
    let connected = state.status.state == VpnState::Connected;
    let tp = state.third_party.len();
    let ops = OP_COUNT.load(Ordering::Relaxed);
    (total, connected, tp, ops)
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.profiles.clear();
    state.status = default_status();
    state.third_party.clear();
    state.changes = 0;
    NEXT_ID.store(1, Ordering::Relaxed);
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: create profiles.
    serial_println!("vpn::self_test 1: create profiles");
    let p1 = create_profile("Test1", VpnProtocol::OpenVpn, "vpn.test.com", 1194)?;
    let p2 = create_profile("Test2", VpnProtocol::WireGuard, "wg.test.com", 51820)?;
    assert_eq!(list_profiles().len(), 2);

    // Test 2: configure.
    serial_println!("vpn::self_test 2: configure");
    set_auth(p1, AuthMethod::Certificate)?;
    set_username(p1, "admin")?;
    set_certs(p1, "/cert", "/key", "/ca")?;
    set_transport(p1, Transport::Tcp)?;
    set_route_all(p1, false)?;
    set_kill_switch(p1, true)?;
    set_auto_connect(p1, true)?;
    add_dns(p1, "1.1.1.1")?;
    let prof = get_profile(p1)?;
    assert_eq!(prof.auth, AuthMethod::Certificate);
    assert_eq!(prof.transport, Transport::Tcp);
    assert!(prof.kill_switch);
    assert_eq!(prof.dns_servers.len(), 1);

    // Test 3: connect/disconnect.
    serial_println!("vpn::self_test 3: connect/disconnect");
    connect(p1)?;
    assert!(is_active());
    let s = status();
    assert_eq!(s.state, VpnState::Connected);
    assert_eq!(s.active_profile, Some(p1));
    // Cannot connect when already connected.
    assert!(connect(p2).is_err());
    disconnect()?;
    assert!(!is_active());

    // Test 4: cannot remove while connected.
    serial_println!("vpn::self_test 4: remove protection");
    connect(p1)?;
    assert!(remove_profile(p1).is_err());
    disconnect()?;
    remove_profile(p1)?;
    assert_eq!(list_profiles().len(), 1);

    // Test 5: third-party detection.
    serial_println!("vpn::self_test 5: third-party VPN");
    register_third_party("NordVPN", "/usr/bin/nordvpn", "tun1")?;
    update_third_party("NordVPN", true, Some(1234))?;
    assert!(is_active());
    let tp = list_third_party();
    assert_eq!(tp.len(), 1);
    assert!(tp[0].connected);

    // Test 6: init_defaults seeds NO fabricated profiles.
    serial_println!("vpn::self_test 6: init defaults");
    clear_all();
    init_defaults();
    assert_eq!(list_profiles().len(), 0);

    // Test 7: status snapshot after a user-created profile connects.
    serial_println!("vpn::self_test 7: status");
    let pid = create_profile("StatusTest", VpnProtocol::OpenVpn, "vpn.status.test", 1194)?;
    connect(pid)?;
    let s = status();
    assert_eq!(s.state, VpnState::Connected);
    assert!(!s.connected_server.is_empty());

    clear_all();
    serial_println!("vpn::self_test: all 7 tests passed");
    Ok(())
}
