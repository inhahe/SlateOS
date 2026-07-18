//! Dynamic DNS helper — set up and manage dynamic DNS services.
//!
//! Provides configuration for dynamic DNS providers so users with dynamic
//! IP addresses can maintain a stable hostname.  Also includes UPnP/NAT-PMP
//! port forwarding management.
//!
//! ## Design Reference
//!
//! design.txt line 1264: UPnP or NATPMP port range passthroughs in router
//! design.txt line 1301: dyndns setup helper (e.g., dynu.net)
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Network → Dynamic DNS
//!   → dyndns::list_providers() → configured providers
//!   → dyndns::update_now() → force IP update
//!
//! Settings panel → Network → Port Forwarding
//!   → dyndns::list_forwards() → UPnP/NAT-PMP port mappings
//!   → dyndns::add_forward(...) → create port mapping via UPnP
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

/// Dynamic DNS provider type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynDnsProvider {
    /// Dynu.com.
    Dynu,
    /// No-IP (noip.com).
    NoIp,
    /// DuckDNS.
    DuckDns,
    /// Cloudflare (DDNS via API).
    Cloudflare,
    /// FreeDNS (afraid.org).
    FreeDns,
    /// Custom provider (generic update URL).
    Custom,
}

/// Update status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStatus {
    /// Not yet attempted.
    Idle,
    /// Update in progress.
    Updating,
    /// Last update succeeded.
    Success,
    /// Last update failed.
    Failed,
    /// Provider reported no change needed.
    NoChange,
}

/// Port forwarding protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardProtocol {
    Tcp,
    Udp,
    Both,
}

/// NAT traversal method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatMethod {
    /// UPnP (Universal Plug and Play).
    Upnp,
    /// NAT-PMP (NAT Port Mapping Protocol).
    NatPmp,
    /// PCP (Port Control Protocol, successor to NAT-PMP).
    Pcp,
    /// Manual (user configured in router).
    Manual,
}

/// A dynamic DNS configuration entry.
#[derive(Debug, Clone)]
pub struct DynDnsEntry {
    /// Unique ID.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// Provider type.
    pub provider: DynDnsProvider,
    /// Hostname to update (e.g., "myhost.dynu.net").
    pub hostname: String,
    /// Username / API key.
    pub username: String,
    /// Update URL (for custom providers).
    pub update_url: String,
    /// Update interval in seconds.
    pub interval_s: u32,
    /// Whether enabled.
    pub enabled: bool,
    /// Last update status.
    pub status: UpdateStatus,
    /// Last update timestamp (ns).
    pub last_update_ns: u64,
    /// Last known IP.
    pub last_ip: String,
    /// Update count.
    pub update_count: u64,
    /// Failure count.
    pub fail_count: u64,
}

/// A port forwarding entry.
#[derive(Debug, Clone)]
pub struct PortForward {
    /// Unique ID.
    pub id: u64,
    /// Description.
    pub description: String,
    /// External port.
    pub external_port: u16,
    /// Internal port.
    pub internal_port: u16,
    /// Protocol.
    pub protocol: ForwardProtocol,
    /// Internal IP address.
    pub internal_ip: String,
    /// NAT traversal method used.
    pub method: NatMethod,
    /// Whether active/enabled.
    pub active: bool,
    /// Lease duration in seconds (0 = permanent).
    pub lease_s: u32,
    /// Created timestamp (ns).
    pub created_ns: u64,
}

/// Router detection info.
#[derive(Debug, Clone)]
pub struct RouterInfo {
    /// Router IP address.
    pub ip: String,
    /// Whether UPnP is available.
    pub upnp_available: bool,
    /// Whether NAT-PMP is available.
    pub natpmp_available: bool,
    /// External (WAN) IP address.
    pub external_ip: String,
    /// Router model (if detected).
    pub model: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    entries: Vec<DynDnsEntry>,
    forwards: Vec<PortForward>,
    router: Option<RouterInfo>,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    entries: Vec::new(),
    forwards: Vec::new(),
    router: None,
    changes: 0,
});

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Dynamic DNS management
// ---------------------------------------------------------------------------

/// Add a dynamic DNS entry.
pub fn add_entry(
    name: &str,
    provider: DynDnsProvider,
    hostname: &str,
    username: &str,
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.entries.len() >= 16 {
        return Err(KernelError::ResourceExhausted);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.entries.push(DynDnsEntry {
        id,
        name: String::from(name),
        provider,
        hostname: String::from(hostname),
        username: String::from(username),
        update_url: String::new(),
        interval_s: 300,
        enabled: true,
        status: UpdateStatus::Idle,
        last_update_ns: 0,
        last_ip: String::new(),
        update_count: 0,
        fail_count: 0,
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Remove a DDNS entry.
pub fn remove_entry(entry_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    if !state.entries.iter().any(|e| e.id == entry_id) {
        return Err(KernelError::NotFound);
    }
    state.entries.retain(|e| e.id != entry_id);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get an entry.
pub fn get_entry(entry_id: u64) -> KernelResult<DynDnsEntry> {
    STATE.lock().entries.iter().find(|e| e.id == entry_id).cloned()
        .ok_or(KernelError::NotFound)
}

/// List all entries.
pub fn list_entries() -> Vec<DynDnsEntry> {
    STATE.lock().entries.clone()
}

/// Set enabled.
pub fn set_enabled(entry_id: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let e = state.entries.iter_mut().find(|e| e.id == entry_id)
        .ok_or(KernelError::NotFound)?;
    e.enabled = enabled;
    state.changes += 1;
    Ok(())
}

/// Set update interval.
pub fn set_interval(entry_id: u64, seconds: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let e = state.entries.iter_mut().find(|e| e.id == entry_id)
        .ok_or(KernelError::NotFound)?;
    e.interval_s = seconds.clamp(60, 86400);
    state.changes += 1;
    Ok(())
}

/// Set custom update URL.
pub fn set_update_url(entry_id: u64, url: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let e = state.entries.iter_mut().find(|e| e.id == entry_id)
        .ok_or(KernelError::NotFound)?;
    e.update_url = String::from(url);
    state.changes += 1;
    Ok(())
}

/// Simulate an update (records the IP and status).
pub fn update_now(entry_id: u64, ip: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let e = state.entries.iter_mut().find(|e| e.id == entry_id)
        .ok_or(KernelError::NotFound)?;
    e.last_ip = String::from(ip);
    e.last_update_ns = crate::hpet::elapsed_ns();
    e.update_count += 1;
    e.status = UpdateStatus::Success;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Record a failed update.
pub fn record_failure(entry_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let e = state.entries.iter_mut().find(|e| e.id == entry_id)
        .ok_or(KernelError::NotFound)?;
    e.fail_count += 1;
    e.status = UpdateStatus::Failed;
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Port forwarding
// ---------------------------------------------------------------------------

/// Add a port forward.
pub fn add_forward(
    description: &str,
    external_port: u16,
    internal_port: u16,
    protocol: ForwardProtocol,
    internal_ip: &str,
    method: NatMethod,
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.forwards.len() >= 128 {
        return Err(KernelError::ResourceExhausted);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.forwards.push(PortForward {
        id,
        description: String::from(description),
        external_port,
        internal_port,
        protocol,
        internal_ip: String::from(internal_ip),
        method,
        active: true,
        lease_s: 0,
        created_ns: crate::hpet::elapsed_ns(),
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Remove a port forward.
pub fn remove_forward(forward_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    if !state.forwards.iter().any(|f| f.id == forward_id) {
        return Err(KernelError::NotFound);
    }
    state.forwards.retain(|f| f.id != forward_id);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// List port forwards.
pub fn list_forwards() -> Vec<PortForward> {
    STATE.lock().forwards.clone()
}

/// Set forward active state.
pub fn set_forward_active(forward_id: u64, active: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let f = state.forwards.iter_mut().find(|f| f.id == forward_id)
        .ok_or(KernelError::NotFound)?;
    f.active = active;
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Router detection
// ---------------------------------------------------------------------------

/// Set detected router info.
pub fn set_router_info(ip: &str, upnp: bool, natpmp: bool, ext_ip: &str, model: &str) {
    let mut state = STATE.lock();
    state.router = Some(RouterInfo {
        ip: String::from(ip),
        upnp_available: upnp,
        natpmp_available: natpmp,
        external_ip: String::from(ext_ip),
        model: String::from(model),
    });
    state.changes += 1;
}

/// Get router info.
pub fn router_info() -> Option<RouterInfo> {
    STATE.lock().router.clone()
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

/// Initialise with example configuration.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.entries.is_empty() {
        return;
    }

    let id1 = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.entries.push(DynDnsEntry {
        id: id1,
        name: String::from("Home DDNS"),
        provider: DynDnsProvider::Dynu,
        hostname: String::from("myhome.dynu.net"),
        username: String::from("user@example.com"),
        update_url: String::new(),
        interval_s: 300,
        enabled: true,
        status: UpdateStatus::Success,
        last_update_ns: 0,
        last_ip: String::from("203.0.113.42"),
        update_count: 150,
        fail_count: 2,
    });

    // Default router info.
    state.router = Some(RouterInfo {
        ip: String::from("192.168.1.1"),
        upnp_available: true,
        natpmp_available: false,
        external_ip: String::from("203.0.113.42"),
        model: String::from("Generic Router"),
    });

    state.changes += 1;
}

/// Return (entry_count, forward_count, router_detected, ops).
pub fn stats() -> (usize, usize, bool, u64) {
    let state = STATE.lock();
    (state.entries.len(),
     state.forwards.len(),
     state.router.is_some(),
     OP_COUNT.load(Ordering::Relaxed))
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.entries.clear();
    state.forwards.clear();
    state.router = None;
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

    // Test 1: add entries.
    serial_println!("dyndns::self_test 1: add entries");
    let e1 = add_entry("Test1", DynDnsProvider::Dynu, "test.dynu.net", "user1")?;
    let e2 = add_entry("Test2", DynDnsProvider::DuckDns, "test.duckdns.org", "token123")?;
    assert_eq!(list_entries().len(), 2);

    // Test 2: configure.
    serial_println!("dyndns::self_test 2: configure");
    set_interval(e1, 600)?;
    set_update_url(e1, "https://custom.update/api")?;
    set_enabled(e2, false)?;
    let entry = get_entry(e1)?;
    assert_eq!(entry.interval_s, 600);
    let entry2 = get_entry(e2)?;
    assert!(!entry2.enabled);

    // Test 3: update.
    serial_println!("dyndns::self_test 3: update");
    update_now(e1, "198.51.100.5")?;
    let entry = get_entry(e1)?;
    assert_eq!(entry.last_ip, "198.51.100.5");
    assert_eq!(entry.status, UpdateStatus::Success);
    assert_eq!(entry.update_count, 1);

    // Test 4: failure tracking.
    serial_println!("dyndns::self_test 4: failure");
    record_failure(e1)?;
    let entry = get_entry(e1)?;
    assert_eq!(entry.status, UpdateStatus::Failed);
    assert_eq!(entry.fail_count, 1);

    // Test 5: port forwards.
    serial_println!("dyndns::self_test 5: port forwards");
    let f1 = add_forward("SSH", 22, 22, ForwardProtocol::Tcp, "192.168.1.100", NatMethod::Upnp)?;
    let f2 = add_forward("Web", 8080, 80, ForwardProtocol::Both, "192.168.1.100", NatMethod::Upnp)?;
    assert_eq!(list_forwards().len(), 2);
    set_forward_active(f1, false)?;
    remove_forward(f2)?;
    assert_eq!(list_forwards().len(), 1);

    // Test 6: router info.
    serial_println!("dyndns::self_test 6: router info");
    set_router_info("192.168.0.1", true, true, "203.0.113.1", "TestRouter");
    let ri = router_info().unwrap();
    assert_eq!(ri.ip, "192.168.0.1");
    assert!(ri.upnp_available);
    assert!(ri.natpmp_available);

    // Test 7: remove and init.
    serial_println!("dyndns::self_test 7: remove and init");
    remove_entry(e1)?;
    assert_eq!(list_entries().len(), 1);
    clear_all();
    init_defaults();
    assert!(!list_entries().is_empty());
    assert!(router_info().is_some());

    clear_all();
    serial_println!("dyndns::self_test: all 7 tests passed");
    Ok(())
}
