//! Network Profile — network location and per-network settings.
//!
//! Manages per-network profiles with custom firewall rules,
//! proxy settings, DNS overrides, and security classifications.
//!
//! ## Architecture
//!
//! ```text
//! Network connected
//!   → netprofile::detect(ssid/adapter) → profile lookup
//!   → netprofile::apply(profile_id) → activates settings
//!
//! Integration:
//!   → netsettings (WiFi configuration)
//!   → fwsettings (firewall rules)
//!   → netproxy (proxy settings)
//!   → vpn (VPN auto-connect)
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

/// Network security classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkType {
    Private,
    Public,
    Domain,
    Trusted,
    Untrusted,
}

impl NetworkType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Private => "Private",
            Self::Public => "Public",
            Self::Domain => "Domain",
            Self::Trusted => "Trusted",
            Self::Untrusted => "Untrusted",
        }
    }
}

/// Connection type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionType {
    WiFi,
    Ethernet,
    Cellular,
    Vpn,
}

impl ConnectionType {
    pub fn label(self) -> &'static str {
        match self {
            Self::WiFi => "WiFi",
            Self::Ethernet => "Ethernet",
            Self::Cellular => "Cellular",
            Self::Vpn => "VPN",
        }
    }
}

/// Network profile.
#[derive(Debug, Clone)]
pub struct NetProfile {
    pub id: u32,
    pub name: String,
    pub ssid: String,
    pub network_type: NetworkType,
    pub connection_type: ConnectionType,
    pub metered: bool,
    /// Auto-connect VPN name (empty = none).
    pub auto_vpn: String,
    /// Custom DNS servers.
    pub dns_servers: Vec<String>,
    /// Firewall rule set name.
    pub firewall_profile: String,
    /// Proxy auto-config URL.
    pub proxy_pac: String,
    pub auto_connect: bool,
    pub priority: u32,
    pub last_connected_ns: u64,
    pub total_connections: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PROFILES: usize = 50;

struct State {
    profiles: Vec<NetProfile>,
    next_id: u32,
    active_profile_id: u32,
    total_switches: u64,
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

    let profiles = alloc::vec![
        NetProfile {
            id: 1, name: String::from("Home"),
            ssid: String::from("HomeWiFi"),
            network_type: NetworkType::Private,
            connection_type: ConnectionType::WiFi,
            metered: false,
            auto_vpn: String::new(),
            dns_servers: Vec::new(),
            firewall_profile: String::from("private"),
            proxy_pac: String::new(),
            auto_connect: true, priority: 100,
            last_connected_ns: 0, total_connections: 0,
        },
        NetProfile {
            id: 2, name: String::from("Public WiFi"),
            ssid: String::new(),
            network_type: NetworkType::Public,
            connection_type: ConnectionType::WiFi,
            metered: false,
            auto_vpn: String::from("default-vpn"),
            dns_servers: alloc::vec![String::from("1.1.1.1"), String::from("8.8.8.8")],
            firewall_profile: String::from("public"),
            proxy_pac: String::new(),
            auto_connect: false, priority: 50,
            last_connected_ns: 0, total_connections: 0,
        },
    ];

    *guard = Some(State {
        profiles,
        next_id: 3,
        active_profile_id: 0,
        total_switches: 0,
        ops: 0,
    });
}

/// Create a network profile.
pub fn create_profile(name: &str, ssid: &str, network_type: NetworkType, conn_type: ConnectionType) -> KernelResult<u32> {
    with_state(|state| {
        if state.profiles.len() >= MAX_PROFILES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.profiles.push(NetProfile {
            id, name: String::from(name),
            ssid: String::from(ssid),
            network_type, connection_type: conn_type,
            metered: false,
            auto_vpn: String::new(),
            dns_servers: Vec::new(),
            firewall_profile: String::from(network_type.label()),
            proxy_pac: String::new(),
            auto_connect: true, priority: 50,
            last_connected_ns: 0, total_connections: 0,
        });
        Ok(id)
    })
}

/// Apply (activate) a network profile.
pub fn apply_profile(profile_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        p.last_connected_ns = crate::hpet::elapsed_ns();
        p.total_connections += 1;
        state.active_profile_id = profile_id;
        state.total_switches += 1;
        Ok(())
    })
}

/// Set network type.
pub fn set_network_type(profile_id: u32, net_type: NetworkType) -> KernelResult<()> {
    with_state(|state| {
        let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        p.network_type = net_type;
        Ok(())
    })
}

/// Set metered status.
pub fn set_metered(profile_id: u32, metered: bool) -> KernelResult<()> {
    with_state(|state| {
        let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        p.metered = metered;
        Ok(())
    })
}

/// Set auto-connect VPN.
pub fn set_auto_vpn(profile_id: u32, vpn_name: &str) -> KernelResult<()> {
    with_state(|state| {
        let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        p.auto_vpn = String::from(vpn_name);
        Ok(())
    })
}

/// Set custom DNS servers.
pub fn set_dns(profile_id: u32, servers: Vec<String>) -> KernelResult<()> {
    with_state(|state| {
        let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        p.dns_servers = servers;
        Ok(())
    })
}

/// Remove a profile.
pub fn remove_profile(profile_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.profiles.iter().position(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        state.profiles.remove(pos);
        if state.active_profile_id == profile_id {
            state.active_profile_id = 0;
        }
        Ok(())
    })
}

/// Find profile by SSID.
pub fn find_by_ssid(ssid: &str) -> Option<u32> {
    STATE.lock().as_ref().and_then(|s| {
        s.profiles.iter().find(|p| p.ssid == ssid).map(|p| p.id)
    })
}

/// List all profiles.
pub fn list_profiles() -> Vec<NetProfile> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.profiles.clone())
}

/// Get profile.
pub fn get_profile(id: u32) -> KernelResult<NetProfile> {
    with_state(|state| {
        state.profiles.iter().find(|p| p.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// Active profile ID.
pub fn active_id() -> u32 {
    STATE.lock().as_ref().map_or(0, |s| s.active_profile_id)
}

/// Statistics: (profile_count, active_id, total_switches, ops).
pub fn stats() -> (usize, u32, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.profiles.len(), s.active_profile_id, s.total_switches, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("netprofile::self_test() — running tests...");
    init_defaults();

    // 1: Default profiles.
    let profiles = list_profiles();
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].network_type, NetworkType::Private);
    crate::serial_println!("  [1/8] default profiles: OK");

    // 2: Apply profile.
    apply_profile(1).expect("apply");
    assert_eq!(active_id(), 1);
    let p = get_profile(1).expect("get");
    assert_eq!(p.total_connections, 1);
    crate::serial_println!("  [2/8] apply profile: OK");

    // 3: Create profile.
    let id = create_profile("Office", "CorpWiFi", NetworkType::Domain, ConnectionType::WiFi).expect("create");
    assert_eq!(list_profiles().len(), 3);
    crate::serial_println!("  [3/8] create profile: OK");

    // 4: Set metered.
    set_metered(id, true).expect("metered");
    let p = get_profile(id).expect("get2");
    assert!(p.metered);
    crate::serial_println!("  [4/8] set metered: OK");

    // 5: Set DNS.
    set_dns(id, alloc::vec![String::from("10.0.0.1"), String::from("10.0.0.2")]).expect("dns");
    let p = get_profile(id).expect("get3");
    assert_eq!(p.dns_servers.len(), 2);
    crate::serial_println!("  [5/8] set DNS: OK");

    // 6: Auto VPN.
    set_auto_vpn(id, "corp-vpn").expect("vpn");
    let p = get_profile(id).expect("get4");
    assert_eq!(p.auto_vpn, "corp-vpn");
    crate::serial_println!("  [6/8] auto VPN: OK");

    // 7: Find by SSID.
    let found = find_by_ssid("HomeWiFi");
    assert_eq!(found, Some(1));
    let not_found = find_by_ssid("nonexistent");
    assert!(not_found.is_none());
    crate::serial_println!("  [7/8] find by SSID: OK");

    // 8: Stats.
    let (count, active, switches, ops) = stats();
    assert_eq!(count, 3);
    assert_eq!(active, 1);
    assert_eq!(switches, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("netprofile::self_test() — all 8 tests passed");
}
