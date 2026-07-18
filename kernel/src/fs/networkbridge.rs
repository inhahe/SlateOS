//! Network Bridge — virtual network bridge management.
//!
//! Creates and manages network bridges that group physical and
//! virtual interfaces for network segmentation and sharing.
//!
//! ## Architecture
//!
//! ```text
//! Network bridge management
//!   → networkbridge::create(name) → new bridge
//!   → networkbridge::add_interface(bridge, iface) → attach NIC
//!   → networkbridge::get_status(bridge) → bridge state
//!
//! Integration:
//!   → netsettings (network configuration)
//!   → netprofile (network profiles)
//!   → netshare (network sharing)
//!   → vpn (VPN connections)
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

/// Bridge operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeMode {
    Transparent,   // Layer-2 pass-through.
    Nat,           // Network address translation.
    Routed,        // Layer-3 routing.
    Isolated,      // No external connectivity.
}

impl BridgeMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Transparent => "Transparent",
            Self::Nat => "NAT",
            Self::Routed => "Routed",
            Self::Isolated => "Isolated",
        }
    }
}

/// Bridge state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeState {
    Up,
    Down,
    Configuring,
    Error,
}

impl BridgeState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Configuring => "Configuring",
            Self::Error => "Error",
        }
    }
}

/// Interface type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IfaceType {
    Physical,
    Virtual,
    VlanTag,
    Loopback,
    Tap,
}

impl IfaceType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Physical => "Physical",
            Self::Virtual => "Virtual",
            Self::VlanTag => "VLAN",
            Self::Loopback => "Loopback",
            Self::Tap => "TAP",
        }
    }
}

/// An interface attached to a bridge.
#[derive(Debug, Clone)]
pub struct BridgeInterface {
    pub name: String,
    pub iface_type: IfaceType,
    pub mac_address: String,
    pub added_ns: u64,
}

/// A network bridge.
#[derive(Debug, Clone)]
pub struct NetworkBridge {
    pub id: u32,
    pub name: String,
    pub mode: BridgeMode,
    pub state: BridgeState,
    pub interfaces: Vec<BridgeInterface>,
    pub ip_address: Option<String>,
    pub subnet_mask: Option<String>,
    pub mtu: u32,
    pub stp_enabled: bool,
    pub created_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_BRIDGES: usize = 32;
const MAX_IFACES_PER_BRIDGE: usize = 16;

struct State {
    bridges: Vec<NetworkBridge>,
    next_id: u32,
    total_created: u64,
    total_ifaces_added: u64,
    total_packets_forwarded: u64,
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
        bridges: Vec::new(),
        next_id: 1,
        total_created: 0,
        total_ifaces_added: 0,
        total_packets_forwarded: 0,
        ops: 0,
    });
}

/// Create a new bridge.
pub fn create_bridge(name: &str, mode: BridgeMode) -> KernelResult<u32> {
    with_state(|state| {
        if state.bridges.len() >= MAX_BRIDGES {
            return Err(KernelError::ResourceExhausted);
        }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        state.bridges.push(NetworkBridge {
            id, name: String::from(name), mode, state: BridgeState::Down,
            interfaces: Vec::new(), ip_address: None, subnet_mask: None,
            mtu: 1500, stp_enabled: true, created_ns: now,
        });
        state.total_created += 1;
        Ok(id)
    })
}

/// Delete a bridge.
pub fn delete_bridge(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.bridges.len();
        state.bridges.retain(|b| b.id != id);
        if state.bridges.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Add an interface to a bridge.
pub fn add_interface(bridge_id: u32, iface_name: &str, iface_type: IfaceType, mac: &str) -> KernelResult<()> {
    with_state(|state| {
        let br = state.bridges.iter_mut().find(|b| b.id == bridge_id)
            .ok_or(KernelError::NotFound)?;
        if br.interfaces.len() >= MAX_IFACES_PER_BRIDGE {
            return Err(KernelError::ResourceExhausted);
        }
        if br.interfaces.iter().any(|i| i.name == iface_name) {
            return Err(KernelError::AlreadyExists);
        }
        let now = crate::hpet::elapsed_ns();
        br.interfaces.push(BridgeInterface {
            name: String::from(iface_name), iface_type,
            mac_address: String::from(mac), added_ns: now,
        });
        state.total_ifaces_added += 1;
        Ok(())
    })
}

/// Remove an interface from a bridge.
pub fn remove_interface(bridge_id: u32, iface_name: &str) -> KernelResult<()> {
    with_state(|state| {
        let br = state.bridges.iter_mut().find(|b| b.id == bridge_id)
            .ok_or(KernelError::NotFound)?;
        let before = br.interfaces.len();
        br.interfaces.retain(|i| i.name != iface_name);
        if br.interfaces.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Set bridge state (up/down).
pub fn set_state(bridge_id: u32, new_state: BridgeState) -> KernelResult<()> {
    with_state(|state| {
        let br = state.bridges.iter_mut().find(|b| b.id == bridge_id)
            .ok_or(KernelError::NotFound)?;
        br.state = new_state;
        Ok(())
    })
}

/// Configure bridge IP.
pub fn set_ip(bridge_id: u32, ip: &str, mask: &str) -> KernelResult<()> {
    with_state(|state| {
        let br = state.bridges.iter_mut().find(|b| b.id == bridge_id)
            .ok_or(KernelError::NotFound)?;
        br.ip_address = Some(String::from(ip));
        br.subnet_mask = Some(String::from(mask));
        Ok(())
    })
}

/// Set MTU.
pub fn set_mtu(bridge_id: u32, mtu: u32) -> KernelResult<()> {
    with_state(|state| {
        let br = state.bridges.iter_mut().find(|b| b.id == bridge_id)
            .ok_or(KernelError::NotFound)?;
        br.mtu = mtu;
        Ok(())
    })
}

/// Toggle STP.
pub fn set_stp(bridge_id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let br = state.bridges.iter_mut().find(|b| b.id == bridge_id)
            .ok_or(KernelError::NotFound)?;
        br.stp_enabled = enabled;
        Ok(())
    })
}

/// Record forwarded packets (simulation).
pub fn record_forwarded(count: u64) -> KernelResult<()> {
    with_state(|state| { state.total_packets_forwarded += count; Ok(()) })
}

/// List all bridges.
pub fn list_bridges() -> Vec<NetworkBridge> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.bridges.clone())
}

/// Get a specific bridge.
pub fn get_bridge(id: u32) -> Option<NetworkBridge> {
    STATE.lock().as_ref().and_then(|s| s.bridges.iter().find(|b| b.id == id).cloned())
}

/// Statistics: (bridge_count, total_created, total_ifaces, total_forwarded, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.bridges.len(), s.total_created, s.total_ifaces_added, s.total_packets_forwarded, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("networkbridge::self_test() — running tests...");
    init_defaults();

    // 1: Empty state.
    assert!(list_bridges().is_empty());
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Create bridge.
    let id = create_bridge("br0", BridgeMode::Transparent).expect("create");
    let br = get_bridge(id).expect("get");
    assert_eq!(br.name, "br0");
    assert_eq!(br.state, BridgeState::Down);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Add interfaces.
    add_interface(id, "eth0", IfaceType::Physical, "aa:bb:cc:dd:ee:01").expect("add1");
    add_interface(id, "eth1", IfaceType::Physical, "aa:bb:cc:dd:ee:02").expect("add2");
    let br = get_bridge(id).expect("get2");
    assert_eq!(br.interfaces.len(), 2);
    crate::serial_println!("  [3/8] add interfaces: OK");

    // 4: Bring up.
    set_state(id, BridgeState::Up).expect("up");
    let br = get_bridge(id).expect("get3");
    assert_eq!(br.state, BridgeState::Up);
    crate::serial_println!("  [4/8] bring up: OK");

    // 5: Configure IP.
    set_ip(id, "192.168.1.1", "255.255.255.0").expect("ip");
    let br = get_bridge(id).expect("get4");
    assert_eq!(br.ip_address.as_deref(), Some("192.168.1.1"));
    crate::serial_println!("  [5/8] configure IP: OK");

    // 6: Remove interface.
    remove_interface(id, "eth1").expect("remove");
    let br = get_bridge(id).expect("get5");
    assert_eq!(br.interfaces.len(), 1);
    crate::serial_println!("  [6/8] remove interface: OK");

    // 7: NAT bridge.
    let nat_id = create_bridge("br-nat", BridgeMode::Nat).expect("nat");
    add_interface(nat_id, "tap0", IfaceType::Tap, "aa:bb:cc:dd:ee:03").expect("add_tap");
    set_mtu(nat_id, 9000).expect("mtu");
    let br = get_bridge(nat_id).expect("get_nat");
    assert_eq!(br.mtu, 9000);
    crate::serial_println!("  [7/8] NAT bridge: OK");

    // 8: Stats.
    let (count, created, ifaces, _forwarded, ops) = stats();
    assert_eq!(count, 2);
    assert_eq!(created, 2);
    assert_eq!(ifaces, 3);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("networkbridge::self_test() — all 8 tests passed");
}
