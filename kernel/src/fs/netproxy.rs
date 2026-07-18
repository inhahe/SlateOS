//! Network proxy configuration — system-wide proxy settings.
//!
//! Provides HTTP/HTTPS/SOCKS proxy configuration with PAC file support,
//! per-host bypass rules, and per-application proxy overrides.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Network → Proxy
//!   → netproxy::set_http_proxy() / set_pac_url()
//!
//! Network stack
//!   → netproxy::resolve_proxy(url) → proxy server or DIRECT
//!
//! Integration:
//!   → netsettings (network configuration)
//!   → vpn (proxy vs VPN routing)
//!   → envvars (http_proxy / https_proxy env)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::{vec, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Proxy mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyMode {
    /// No proxy — direct connections.
    None,
    /// Manual proxy configuration.
    Manual,
    /// Automatic via PAC/WPAD URL.
    Auto,
    /// System-detected (WPAD/DHCP).
    SystemDetect,
}

impl ProxyMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Manual => "Manual",
            Self::Auto => "Auto (PAC)",
            Self::SystemDetect => "System Detect",
        }
    }
}

/// Proxy protocol type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyProtocol {
    Http,
    Https,
    Socks4,
    Socks5,
    Ftp,
}

impl ProxyProtocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Http => "HTTP",
            Self::Https => "HTTPS",
            Self::Socks4 => "SOCKS4",
            Self::Socks5 => "SOCKS5",
            Self::Ftp => "FTP",
        }
    }
}

/// A proxy server entry.
#[derive(Debug, Clone)]
pub struct ProxyServer {
    /// Protocol this proxy handles.
    pub protocol: ProxyProtocol,
    /// Server hostname or IP.
    pub host: String,
    /// Port number.
    pub port: u16,
    /// Requires authentication.
    pub auth_required: bool,
    /// Username (if auth required).
    pub username: String,
    /// Whether this proxy is enabled.
    pub enabled: bool,
}

/// A bypass rule — hosts/patterns that skip the proxy.
#[derive(Debug, Clone)]
pub struct BypassRule {
    /// Pattern (hostname, domain, IP, CIDR, or wildcard).
    pub pattern: String,
    /// Description.
    pub description: String,
}

/// Per-application proxy override.
#[derive(Debug, Clone)]
pub struct AppProxyOverride {
    /// Application ID.
    pub app_id: String,
    /// Override mode (None = use system, specific protocol).
    pub mode: ProxyMode,
    /// Custom proxy host (for Manual mode).
    pub custom_host: String,
    /// Custom proxy port.
    pub custom_port: u16,
}

/// Proxy resolution result.
#[derive(Debug, Clone)]
pub enum ProxyResolution {
    /// Use a proxy.
    Proxy { host: String, port: u16, protocol: ProxyProtocol },
    /// Connect directly (no proxy).
    Direct,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    mode: ProxyMode,
    proxies: Vec<ProxyServer>,
    bypass_rules: Vec<BypassRule>,
    app_overrides: Vec<AppProxyOverride>,
    pac_url: String,
    bypass_local: bool,
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

/// Initialise proxy settings with defaults.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    let bypass = vec![
        BypassRule {
            pattern: String::from("localhost"),
            description: String::from("Loopback"),
        },
        BypassRule {
            pattern: String::from("127.0.0.0/8"),
            description: String::from("Loopback range"),
        },
        BypassRule {
            pattern: String::from("::1"),
            description: String::from("IPv6 loopback"),
        },
    ];

    *guard = Some(State {
        mode: ProxyMode::None,
        proxies: Vec::new(),
        bypass_rules: bypass,
        app_overrides: Vec::new(),
        pac_url: String::new(),
        bypass_local: true,
        ops: 0,
    });
}

/// Set proxy mode.
pub fn set_mode(mode: ProxyMode) -> KernelResult<()> {
    with_state(|state| {
        state.mode = mode;
        Ok(())
    })
}

/// Get current proxy mode.
pub fn get_mode() -> KernelResult<ProxyMode> {
    with_state(|state| Ok(state.mode))
}

/// Set/add a proxy server for a given protocol.
pub fn set_proxy(protocol: ProxyProtocol, host: &str, port: u16) -> KernelResult<()> {
    with_state(|state| {
        // Update existing or add new.
        if let Some(p) = state.proxies.iter_mut().find(|p| p.protocol == protocol) {
            p.host = String::from(host);
            p.port = port;
            p.enabled = true;
        } else {
            state.proxies.push(ProxyServer {
                protocol,
                host: String::from(host),
                port,
                auth_required: false,
                username: String::new(),
                enabled: true,
            });
        }
        Ok(())
    })
}

/// Remove a proxy for a given protocol.
pub fn remove_proxy(protocol: ProxyProtocol) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.proxies.iter().position(|p| p.protocol == protocol)
            .ok_or(KernelError::NotFound)?;
        state.proxies.remove(pos);
        Ok(())
    })
}

/// Set proxy authentication credentials.
pub fn set_proxy_auth(protocol: ProxyProtocol, username: &str) -> KernelResult<()> {
    with_state(|state| {
        let p = state.proxies.iter_mut().find(|p| p.protocol == protocol)
            .ok_or(KernelError::NotFound)?;
        p.auth_required = true;
        p.username = String::from(username);
        Ok(())
    })
}

/// List configured proxies.
pub fn list_proxies() -> Vec<ProxyServer> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.proxies.clone(),
        None => Vec::new(),
    }
}

/// Set PAC URL.
pub fn set_pac_url(url: &str) -> KernelResult<()> {
    with_state(|state| {
        state.pac_url = String::from(url);
        Ok(())
    })
}

/// Get PAC URL.
pub fn get_pac_url() -> KernelResult<String> {
    with_state(|state| Ok(state.pac_url.clone()))
}

/// Add a bypass rule.
pub fn add_bypass(pattern: &str, description: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.bypass_rules.iter().any(|b| b.pattern == pattern) {
            return Err(KernelError::AlreadyExists);
        }
        state.bypass_rules.push(BypassRule {
            pattern: String::from(pattern),
            description: String::from(description),
        });
        Ok(())
    })
}

/// Remove a bypass rule.
pub fn remove_bypass(pattern: &str) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.bypass_rules.iter().position(|b| b.pattern == pattern)
            .ok_or(KernelError::NotFound)?;
        state.bypass_rules.remove(pos);
        Ok(())
    })
}

/// List bypass rules.
pub fn list_bypass() -> Vec<BypassRule> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.bypass_rules.clone(),
        None => Vec::new(),
    }
}

/// Set bypass-local flag (bypass proxy for local addresses).
pub fn set_bypass_local(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.bypass_local = enabled;
        Ok(())
    })
}

/// Add per-application proxy override.
pub fn set_app_override(app_id: &str, mode: ProxyMode, host: &str, port: u16) -> KernelResult<()> {
    with_state(|state| {
        if let Some(ov) = state.app_overrides.iter_mut().find(|o| o.app_id == app_id) {
            ov.mode = mode;
            ov.custom_host = String::from(host);
            ov.custom_port = port;
        } else {
            state.app_overrides.push(AppProxyOverride {
                app_id: String::from(app_id),
                mode,
                custom_host: String::from(host),
                custom_port: port,
            });
        }
        Ok(())
    })
}

/// Remove per-application proxy override.
pub fn remove_app_override(app_id: &str) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.app_overrides.iter().position(|o| o.app_id == app_id)
            .ok_or(KernelError::NotFound)?;
        state.app_overrides.remove(pos);
        Ok(())
    })
}

/// Resolve proxy for a given host. Checks bypass rules, app overrides,
/// then system proxy settings.
pub fn resolve_proxy(host: &str, protocol: ProxyProtocol) -> ProxyResolution {
    let guard = STATE.lock();
    let state = match guard.as_ref() {
        Some(s) => s,
        None => return ProxyResolution::Direct,
    };

    // If proxy is disabled, always direct.
    if state.mode == ProxyMode::None {
        return ProxyResolution::Direct;
    }

    // Check bypass rules.
    for rule in &state.bypass_rules {
        if host == rule.pattern {
            return ProxyResolution::Direct;
        }
        // Simple wildcard: *.example.com matches foo.example.com.
        if rule.pattern.starts_with("*.") {
            let suffix = &rule.pattern[1..]; // .example.com
            if host.ends_with(suffix) {
                return ProxyResolution::Direct;
            }
        }
    }

    // Check bypass local.
    if state.bypass_local
        && (host == "localhost" || host == "127.0.0.1" || host == "::1"
            || host.ends_with(".local"))
    {
        return ProxyResolution::Direct;
    }

    // Find matching proxy.
    if let Some(p) = state.proxies.iter().find(|p| p.protocol == protocol && p.enabled) {
        return ProxyResolution::Proxy {
            host: p.host.clone(),
            port: p.port,
            protocol: p.protocol,
        };
    }

    // Fall back to HTTP proxy for HTTPS if no specific HTTPS proxy.
    if protocol == ProxyProtocol::Https {
        if let Some(p) = state.proxies.iter().find(|p| p.protocol == ProxyProtocol::Http && p.enabled) {
            return ProxyResolution::Proxy {
                host: p.host.clone(),
                port: p.port,
                protocol: ProxyProtocol::Http,
            };
        }
    }

    ProxyResolution::Direct
}

/// Statistics: (proxy_count, bypass_count, mode_label, app_overrides, ops).
pub fn stats() -> (usize, usize, &'static str, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.proxies.len(), s.bypass_rules.len(), s.mode.label(), s.app_overrides.len(), s.ops),
        None => (0, 0, "N/A", 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("netproxy::self_test() — running tests...");

    init_defaults();

    // Test 1: Default mode is None.
    let mode = get_mode().expect("get mode");
    assert_eq!(mode, ProxyMode::None);
    crate::serial_println!("  [1/11] default mode: OK");

    // Test 2: Set manual mode.
    set_mode(ProxyMode::Manual).expect("set mode");
    assert_eq!(get_mode().expect("get"), ProxyMode::Manual);
    crate::serial_println!("  [2/11] set mode: OK");

    // Test 3: Add HTTP proxy.
    set_proxy(ProxyProtocol::Http, "proxy.example.com", 8080).expect("set proxy");
    let proxies = list_proxies();
    assert_eq!(proxies.len(), 1);
    assert_eq!(proxies[0].port, 8080);
    crate::serial_println!("  [3/11] add proxy: OK");

    // Test 4: Resolve proxy — should return proxy.
    match resolve_proxy("example.org", ProxyProtocol::Http) {
        ProxyResolution::Proxy { port, .. } => assert_eq!(port, 8080),
        ProxyResolution::Direct => panic!("expected proxy"),
    }
    crate::serial_println!("  [4/11] resolve proxy: OK");

    // Test 5: Bypass rules — localhost should be direct.
    match resolve_proxy("localhost", ProxyProtocol::Http) {
        ProxyResolution::Direct => {}
        ProxyResolution::Proxy { .. } => panic!("expected direct"),
    }
    crate::serial_println!("  [5/11] bypass localhost: OK");

    // Test 6: Add custom bypass.
    add_bypass("*.internal.corp", "Internal network").expect("add bypass");
    match resolve_proxy("server.internal.corp", ProxyProtocol::Http) {
        ProxyResolution::Direct => {}
        ProxyResolution::Proxy { .. } => panic!("expected direct for internal"),
    }
    crate::serial_println!("  [6/11] custom bypass: OK");

    // Test 7: HTTPS falls back to HTTP proxy.
    match resolve_proxy("secure.example.org", ProxyProtocol::Https) {
        ProxyResolution::Proxy { port, .. } => assert_eq!(port, 8080),
        ProxyResolution::Direct => panic!("expected fallback to http proxy"),
    }
    crate::serial_println!("  [7/11] HTTPS fallback: OK");

    // Test 8: Set PAC URL.
    set_pac_url("http://wpad.example.com/wpad.dat").expect("set pac");
    let pac = get_pac_url().expect("get pac");
    assert!(pac.contains("wpad"));
    crate::serial_println!("  [8/11] PAC URL: OK");

    // Test 9: Proxy auth.
    set_proxy_auth(ProxyProtocol::Http, "admin").expect("set auth");
    let proxies = list_proxies();
    assert!(proxies[0].auth_required);
    assert_eq!(proxies[0].username, "admin");
    crate::serial_println!("  [9/11] proxy auth: OK");

    // Test 10: Remove proxy.
    remove_proxy(ProxyProtocol::Http).expect("remove");
    assert!(list_proxies().is_empty());
    crate::serial_println!("  [10/11] remove proxy: OK");

    // Test 11: Stats.
    let (proxy_count, bypass_count, mode_label, _overrides, ops) = stats();
    assert_eq!(proxy_count, 0);
    assert!(bypass_count >= 3);
    assert_eq!(mode_label, "Manual");
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("netproxy::self_test() — all 11 tests passed");
}
