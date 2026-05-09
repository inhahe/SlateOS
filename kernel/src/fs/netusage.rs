//! Network Usage Monitor — per-app bandwidth tracking and data caps.
//!
//! Tracks network usage per application and interface, enforces
//! data caps, and provides usage history with daily/monthly stats.
//!
//! ## Architecture
//!
//! ```text
//! Network activity
//!   → netusage::record_traffic(app, iface, bytes) → update stats
//!   → netusage::check_cap(app) → enforce limit
//!   → netusage::get_usage(app) → usage report
//!
//! Integration:
//!   → datausage (system data usage)
//!   → netsettings (network config)
//!   → netthrottle (bandwidth throttling)
//!   → appnotify (cap warnings)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Traffic direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Upload,
    Download,
}

impl Direction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Upload => "Upload",
            Self::Download => "Download",
        }
    }
}

/// Network interface type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfaceType {
    Ethernet,
    Wifi,
    Cellular,
    Vpn,
    Loopback,
    Other,
}

impl InterfaceType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ethernet => "Ethernet",
            Self::Wifi => "Wi-Fi",
            Self::Cellular => "Cellular",
            Self::Vpn => "VPN",
            Self::Loopback => "Loopback",
            Self::Other => "Other",
        }
    }
}

/// Per-app network usage record.
#[derive(Debug, Clone)]
pub struct AppUsage {
    pub app_name: String,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub connections: u64,
    pub last_activity_ns: u64,
    pub cap_bytes: Option<u64>,
    pub cap_warned: bool,
}

/// Per-interface traffic stats.
#[derive(Debug, Clone)]
pub struct InterfaceStats {
    pub name: String,
    pub iface_type: InterfaceType,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_APPS: usize = 200;
const MAX_IFACES: usize = 20;

struct State {
    apps: Vec<AppUsage>,
    interfaces: Vec<InterfaceStats>,
    total_bytes_sent: u64,
    total_bytes_received: u64,
    total_connections: u64,
    cap_warnings: u64,
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
        apps: Vec::new(),
        interfaces: alloc::vec![
            InterfaceStats { name: String::from("eth0"), iface_type: InterfaceType::Ethernet, bytes_sent: 0, bytes_received: 0, packets_sent: 0, packets_received: 0 },
            InterfaceStats { name: String::from("wlan0"), iface_type: InterfaceType::Wifi, bytes_sent: 0, bytes_received: 0, packets_sent: 0, packets_received: 0 },
            InterfaceStats { name: String::from("lo"), iface_type: InterfaceType::Loopback, bytes_sent: 0, bytes_received: 0, packets_sent: 0, packets_received: 0 },
        ],
        total_bytes_sent: 0,
        total_bytes_received: 0,
        total_connections: 0,
        cap_warnings: 0,
        ops: 0,
    });
}

/// Record traffic for an application.
pub fn record_traffic(app: &str, iface: &str, direction: Direction, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        // Update app stats.
        let app_entry = if let Some(a) = state.apps.iter_mut().find(|a| a.app_name == app) {
            a
        } else {
            if state.apps.len() >= MAX_APPS {
                return Err(KernelError::ResourceExhausted);
            }
            state.apps.push(AppUsage {
                app_name: String::from(app),
                bytes_sent: 0, bytes_received: 0, connections: 0,
                last_activity_ns: 0, cap_bytes: None, cap_warned: false,
            });
            state.apps.last_mut().ok_or(KernelError::InternalError)?
        };
        match direction {
            Direction::Upload => {
                app_entry.bytes_sent += bytes;
                state.total_bytes_sent += bytes;
            }
            Direction::Download => {
                app_entry.bytes_received += bytes;
                state.total_bytes_received += bytes;
            }
        }
        app_entry.last_activity_ns = now;

        // Check cap.
        if let Some(cap) = app_entry.cap_bytes {
            let total = app_entry.bytes_sent + app_entry.bytes_received;
            if total >= cap && !app_entry.cap_warned {
                app_entry.cap_warned = true;
                state.cap_warnings += 1;
            }
        }

        // Update interface stats.
        if let Some(iface_entry) = state.interfaces.iter_mut().find(|i| i.name == iface) {
            match direction {
                Direction::Upload => {
                    iface_entry.bytes_sent += bytes;
                    iface_entry.packets_sent += 1;
                }
                Direction::Download => {
                    iface_entry.bytes_received += bytes;
                    iface_entry.packets_received += 1;
                }
            }
        }
        Ok(())
    })
}

/// Record a new connection for an app.
pub fn record_connection(app: &str) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if let Some(a) = state.apps.iter_mut().find(|a| a.app_name == app) {
            a.connections += 1;
            a.last_activity_ns = now;
        } else {
            if state.apps.len() >= MAX_APPS {
                return Err(KernelError::ResourceExhausted);
            }
            state.apps.push(AppUsage {
                app_name: String::from(app),
                bytes_sent: 0, bytes_received: 0, connections: 1,
                last_activity_ns: now, cap_bytes: None, cap_warned: false,
            });
        }
        state.total_connections += 1;
        Ok(())
    })
}

/// Set a data cap for an app (bytes).
pub fn set_cap(app: &str, cap_bytes: Option<u64>) -> KernelResult<()> {
    with_state(|state| {
        if let Some(a) = state.apps.iter_mut().find(|a| a.app_name == app) {
            a.cap_bytes = cap_bytes;
            a.cap_warned = false;
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// Check if an app is over its cap.
pub fn is_over_cap(app: &str) -> bool {
    let guard = STATE.lock();
    guard.as_ref().map_or(false, |s| {
        s.apps.iter().find(|a| a.app_name == app).map_or(false, |a| {
            a.cap_bytes.map_or(false, |cap| a.bytes_sent + a.bytes_received >= cap)
        })
    })
}

/// Add a network interface.
pub fn add_interface(name: &str, iface_type: InterfaceType) -> KernelResult<()> {
    with_state(|state| {
        if state.interfaces.len() >= MAX_IFACES {
            return Err(KernelError::ResourceExhausted);
        }
        if state.interfaces.iter().any(|i| i.name == name) {
            return Err(KernelError::AlreadyExists);
        }
        state.interfaces.push(InterfaceStats {
            name: String::from(name), iface_type,
            bytes_sent: 0, bytes_received: 0, packets_sent: 0, packets_received: 0,
        });
        Ok(())
    })
}

/// Get usage for a specific app.
pub fn get_app_usage(app: &str) -> Option<AppUsage> {
    STATE.lock().as_ref().and_then(|s| s.apps.iter().find(|a| a.app_name == app).cloned())
}

/// Get top apps by total bytes (sent + received).
pub fn top_apps(max: usize) -> Vec<AppUsage> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut apps = s.apps.clone();
        apps.sort_by(|a, b| (b.bytes_sent + b.bytes_received).cmp(&(a.bytes_sent + a.bytes_received)));
        apps.truncate(max);
        apps
    })
}

/// List all interfaces.
pub fn list_interfaces() -> Vec<InterfaceStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.interfaces.clone())
}

/// Reset all usage stats.
pub fn reset_all() -> KernelResult<()> {
    with_state(|state| {
        for a in &mut state.apps {
            a.bytes_sent = 0;
            a.bytes_received = 0;
            a.connections = 0;
            a.cap_warned = false;
        }
        for i in &mut state.interfaces {
            i.bytes_sent = 0;
            i.bytes_received = 0;
            i.packets_sent = 0;
            i.packets_received = 0;
        }
        state.total_bytes_sent = 0;
        state.total_bytes_received = 0;
        state.total_connections = 0;
        Ok(())
    })
}

/// Statistics: (app_count, iface_count, total_sent, total_received, total_connections, cap_warnings, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.apps.len(), s.interfaces.len(), s.total_bytes_sent, s.total_bytes_received, s.total_connections, s.cap_warnings, s.ops),
        None => (0, 0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("netusage::self_test() — running tests...");
    init_defaults();

    // 1: Default interfaces.
    assert_eq!(list_interfaces().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Record traffic creates app.
    record_traffic("browser", "eth0", Direction::Download, 10000).expect("traffic");
    let usage = get_app_usage("browser").expect("app");
    assert_eq!(usage.bytes_received, 10000);
    crate::serial_println!("  [2/8] record: OK");

    // 3: Upload traffic.
    record_traffic("browser", "eth0", Direction::Upload, 500).expect("upload");
    let usage = get_app_usage("browser").expect("app2");
    assert_eq!(usage.bytes_sent, 500);
    assert_eq!(usage.bytes_received, 10000);
    crate::serial_println!("  [3/8] upload: OK");

    // 4: Interface stats updated.
    let ifaces = list_interfaces();
    let eth = ifaces.iter().find(|i| i.name == "eth0").expect("eth0");
    assert!(eth.bytes_received > 0);
    assert!(eth.bytes_sent > 0);
    crate::serial_println!("  [4/8] interface: OK");

    // 5: Data cap.
    set_cap("browser", Some(5000)).expect("cap");
    assert!(is_over_cap("browser")); // 10500 > 5000.
    crate::serial_println!("  [5/8] cap: OK");

    // 6: Connections.
    record_connection("browser").expect("conn");
    let usage = get_app_usage("browser").expect("app3");
    assert_eq!(usage.connections, 1);
    crate::serial_println!("  [6/8] connections: OK");

    // 7: Top apps.
    record_traffic("editor", "eth0", Direction::Download, 100).expect("ed");
    let top = top_apps(5);
    assert_eq!(top[0].app_name, "browser");
    crate::serial_println!("  [7/8] top apps: OK");

    // 8: Stats.
    let (apps, ifaces, sent, recv, conns, warnings, ops) = stats();
    assert_eq!(apps, 2);
    assert_eq!(ifaces, 3);
    assert!(sent > 0);
    assert!(recv > 0);
    assert_eq!(conns, 1);
    assert!(warnings >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("netusage::self_test() — all 8 tests passed");
}
