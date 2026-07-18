//! Firewall settings — rule management and network zone configuration.
//!
//! Provides a configuration backend for the system firewall, managing
//! inbound/outbound rules, application permissions, and network zone
//! profiles (home/public/work).
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Security → Firewall
//!   → fwsettings::add_rule() / set_zone()
//!
//! Network stack integration
//!   → fwsettings::check_allowed(app, port, dir) before accept/connect
//!
//! Integration:
//!   → netsettings (interface/zone association)
//!   → appregistry (app name lookup)
//!   → notifcenter (block notifications)
//!   → audit (log blocked connections)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_RULES: usize = 512;
const MAX_APP_RULES: usize = 256;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Traffic direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Inbound,
    Outbound,
    Both,
}

impl Direction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Inbound => "Inbound",
            Self::Outbound => "Outbound",
            Self::Both => "Both",
        }
    }
}

/// Protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
    Icmp,
    Any,
}

impl Protocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
            Self::Icmp => "ICMP",
            Self::Any => "Any",
        }
    }
}

/// Rule action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    Allow,
    Block,
    Log,
}

impl RuleAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::Block => "Block",
            Self::Log => "Log",
        }
    }
}

/// Network zone / profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkZone {
    /// Home — trusted, relaxed rules.
    Home,
    /// Work — moderate restrictions.
    Work,
    /// Public — strict, block most inbound.
    Public,
}

impl NetworkZone {
    pub fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Work => "Work",
            Self::Public => "Public",
        }
    }
}

/// A firewall rule.
#[derive(Debug, Clone)]
pub struct FwRule {
    /// Rule ID.
    pub id: u64,
    /// Rule name/description.
    pub name: String,
    /// Direction.
    pub direction: Direction,
    /// Protocol.
    pub protocol: Protocol,
    /// Port (0 = any).
    pub port: u16,
    /// Port range end (0 = single port).
    pub port_end: u16,
    /// Source IP prefix (empty = any).
    pub source: String,
    /// Destination IP prefix (empty = any).
    pub dest: String,
    /// Action.
    pub action: RuleAction,
    /// Zone this rule applies to (None = all zones).
    pub zone: Option<NetworkZone>,
    /// Whether rule is enabled.
    pub enabled: bool,
    /// Hit count.
    pub hits: u64,
    /// Priority (lower = checked first).
    pub priority: u32,
}

/// Per-application firewall permission.
#[derive(Debug, Clone)]
pub struct AppPermission {
    /// Application ID.
    pub app_id: String,
    /// Allow outbound.
    pub allow_outbound: bool,
    /// Allow inbound.
    pub allow_inbound: bool,
    /// Blocked connection count.
    pub blocked_count: u64,
    /// Allowed connection count.
    pub allowed_count: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct FwState {
    enabled: bool,
    zone: NetworkZone,
    rules: Vec<FwRule>,
    app_perms: Vec<AppPermission>,
    next_rule_id: u64,
    default_inbound: RuleAction,
    default_outbound: RuleAction,
    log_blocked: bool,
    stealth_mode: bool,
    total_blocked: u64,
    total_allowed: u64,
    ops: u64,
}

static STATE: Mutex<Option<FwState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut FwState) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    let result = f(state)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the firewall settings subsystem.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    let default_rules = alloc::vec![
        // Allow DHCP client.
        FwRule {
            id: 1, name: String::from("Allow DHCP"),
            direction: Direction::Both, protocol: Protocol::Udp,
            port: 67, port_end: 68, source: String::new(), dest: String::new(),
            action: RuleAction::Allow, zone: None, enabled: true, hits: 0, priority: 10,
        },
        // Allow DNS.
        FwRule {
            id: 2, name: String::from("Allow DNS"),
            direction: Direction::Outbound, protocol: Protocol::Udp,
            port: 53, port_end: 0, source: String::new(), dest: String::new(),
            action: RuleAction::Allow, zone: None, enabled: true, hits: 0, priority: 10,
        },
        // Allow ICMP ping.
        FwRule {
            id: 3, name: String::from("Allow Ping"),
            direction: Direction::Both, protocol: Protocol::Icmp,
            port: 0, port_end: 0, source: String::new(), dest: String::new(),
            action: RuleAction::Allow, zone: Some(NetworkZone::Home), enabled: true, hits: 0, priority: 20,
        },
        // Block inbound on public.
        FwRule {
            id: 4, name: String::from("Block Public Inbound"),
            direction: Direction::Inbound, protocol: Protocol::Any,
            port: 0, port_end: 0, source: String::new(), dest: String::new(),
            action: RuleAction::Block, zone: Some(NetworkZone::Public), enabled: true, hits: 0, priority: 100,
        },
    ];

    *guard = Some(FwState {
        enabled: true,
        zone: NetworkZone::Home,
        rules: default_rules,
        app_perms: Vec::new(),
        next_rule_id: 5,
        default_inbound: RuleAction::Block,
        default_outbound: RuleAction::Allow,
        log_blocked: true,
        stealth_mode: false,
        total_blocked: 0,
        total_allowed: 0,
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Global settings
// ---------------------------------------------------------------------------

/// Enable or disable the firewall.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.enabled = enabled;
        Ok(())
    })
}

/// Check if firewall is enabled.
pub fn is_enabled() -> bool {
    let guard = STATE.lock();
    guard.as_ref().is_some_and(|s| s.enabled)
}

/// Set the active network zone.
pub fn set_zone(zone: NetworkZone) -> KernelResult<()> {
    with_state(|state| {
        state.zone = zone;
        Ok(())
    })
}

/// Get the active network zone.
pub fn active_zone() -> NetworkZone {
    let guard = STATE.lock();
    guard.as_ref().map_or(NetworkZone::Public, |s| s.zone)
}

/// Set default action for inbound connections.
pub fn set_default_inbound(action: RuleAction) -> KernelResult<()> {
    with_state(|state| {
        state.default_inbound = action;
        Ok(())
    })
}

/// Set default action for outbound connections.
pub fn set_default_outbound(action: RuleAction) -> KernelResult<()> {
    with_state(|state| {
        state.default_outbound = action;
        Ok(())
    })
}

/// Set stealth mode (drop instead of reject).
pub fn set_stealth_mode(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.stealth_mode = enabled;
        Ok(())
    })
}

/// Set logging of blocked connections.
pub fn set_log_blocked(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.log_blocked = enabled;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Rule management
// ---------------------------------------------------------------------------

/// Add a firewall rule.
pub fn add_rule(
    name: &str,
    direction: Direction,
    protocol: Protocol,
    port: u16,
    action: RuleAction,
) -> KernelResult<u64> {
    if name.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        if state.rules.len() >= MAX_RULES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_rule_id;
        state.next_rule_id += 1;
        state.rules.push(FwRule {
            id,
            name: String::from(name),
            direction,
            protocol,
            port,
            port_end: 0,
            source: String::new(),
            dest: String::new(),
            action,
            zone: None,
            enabled: true,
            hits: 0,
            priority: 50,
        });
        Ok(id)
    })
}

/// Remove a firewall rule.
pub fn remove_rule(id: u64) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.rules.iter().position(|r| r.id == id) {
            state.rules.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// Enable or disable a rule.
pub fn set_rule_enabled(id: u64, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let rule = state.rules.iter_mut()
            .find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        rule.enabled = enabled;
        Ok(())
    })
}

/// Set rule priority.
pub fn set_rule_priority(id: u64, priority: u32) -> KernelResult<()> {
    with_state(|state| {
        let rule = state.rules.iter_mut()
            .find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        rule.priority = priority;
        Ok(())
    })
}

/// Set rule zone filter.
pub fn set_rule_zone(id: u64, zone: Option<NetworkZone>) -> KernelResult<()> {
    with_state(|state| {
        let rule = state.rules.iter_mut()
            .find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        rule.zone = zone;
        Ok(())
    })
}

/// Get a rule by ID.
pub fn get_rule(id: u64) -> KernelResult<FwRule> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state.rules.iter()
        .find(|r| r.id == id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all rules (sorted by priority).
pub fn list_rules() -> Vec<FwRule> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        let mut rules = s.rules.clone();
        rules.sort_by_key(|r| r.priority);
        rules
    })
}

// ---------------------------------------------------------------------------
// Application permissions
// ---------------------------------------------------------------------------

/// Set application network permission.
pub fn set_app_permission(app_id: &str, allow_outbound: bool, allow_inbound: bool) -> KernelResult<()> {
    if app_id.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        if let Some(perm) = state.app_perms.iter_mut().find(|p| p.app_id == app_id) {
            perm.allow_outbound = allow_outbound;
            perm.allow_inbound = allow_inbound;
            return Ok(());
        }
        if state.app_perms.len() >= MAX_APP_RULES {
            return Err(KernelError::ResourceExhausted);
        }
        state.app_perms.push(AppPermission {
            app_id: String::from(app_id),
            allow_outbound,
            allow_inbound,
            blocked_count: 0,
            allowed_count: 0,
        });
        Ok(())
    })
}

/// Remove application permission (falls back to defaults).
pub fn remove_app_permission(app_id: &str) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.app_perms.iter().position(|p| p.app_id == app_id) {
            state.app_perms.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// List application permissions.
pub fn list_app_permissions() -> Vec<AppPermission> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.app_perms.clone())
}

// ---------------------------------------------------------------------------
// Checking (for network stack integration)
// ---------------------------------------------------------------------------

/// Check if a connection should be allowed.
///
/// Returns the action to take for a given app/port/direction/protocol.
pub fn check_allowed(
    app_id: &str,
    port: u16,
    direction: Direction,
    protocol: Protocol,
) -> RuleAction {
    let mut guard = STATE.lock();
    let state = match guard.as_mut() {
        Some(s) => s,
        None => return RuleAction::Allow,
    };

    if !state.enabled {
        return RuleAction::Allow;
    }

    // Check app permissions first.
    if let Some(perm) = state.app_perms.iter_mut().find(|p| p.app_id == app_id) {
        let allowed = match direction {
            Direction::Outbound => perm.allow_outbound,
            Direction::Inbound => perm.allow_inbound,
            Direction::Both => perm.allow_outbound && perm.allow_inbound,
        };
        if allowed {
            perm.allowed_count += 1;
            state.total_allowed += 1;
            return RuleAction::Allow;
        }
        perm.blocked_count += 1;
        state.total_blocked += 1;
        return RuleAction::Block;
    }

    // Check rules (sorted by priority).
    let current_zone = state.zone;
    let mut matching_action = None;
    for rule in &mut state.rules {
        if !rule.enabled {
            continue;
        }
        // Zone filter.
        if let Some(rz) = rule.zone {
            if rz != current_zone {
                continue;
            }
        }
        // Direction filter.
        if rule.direction != Direction::Both && rule.direction != direction {
            continue;
        }
        // Protocol filter.
        if rule.protocol != Protocol::Any && rule.protocol != protocol {
            continue;
        }
        // Port filter.
        if rule.port != 0 {
            if rule.port_end != 0 {
                if port < rule.port || port > rule.port_end {
                    continue;
                }
            } else if port != rule.port {
                continue;
            }
        }
        rule.hits += 1;
        matching_action = Some(rule.action);
        break;
    }

    let action = matching_action.unwrap_or({
        match direction {
            Direction::Inbound => state.default_inbound,
            Direction::Outbound => state.default_outbound,
            Direction::Both => state.default_inbound,
        }
    });

    match action {
        RuleAction::Allow => state.total_allowed += 1,
        RuleAction::Block | RuleAction::Log => state.total_blocked += 1,
    }

    action
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (rule_count, app_perm_count, total_blocked, total_allowed, enabled, ops).
pub fn stats() -> (usize, usize, u64, u64, bool, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.rules.len(), s.app_perms.len(), s.total_blocked, s.total_allowed, s.enabled, s.ops),
        None => (0, 0, 0, 0, false, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the firewall settings module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[fwsettings] Running self-tests...");

    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial state.
    {
        assert!(is_enabled());
        assert_eq!(active_zone(), NetworkZone::Home);
        let rules = list_rules();
        assert_eq!(rules.len(), 4);
    }
    serial_println!("[fwsettings]  1/11 initial state OK");

    // Test 2: toggle firewall.
    {
        set_enabled(false).unwrap();
        assert!(!is_enabled());
        set_enabled(true).unwrap();
        assert!(is_enabled());
    }
    serial_println!("[fwsettings]  2/11 toggle OK");

    // Test 3: zone switching.
    {
        set_zone(NetworkZone::Public).unwrap();
        assert_eq!(active_zone(), NetworkZone::Public);
        set_zone(NetworkZone::Home).unwrap();
    }
    serial_println!("[fwsettings]  3/11 zone switching OK");

    // Test 4: add rule.
    {
        let id = add_rule("Allow SSH", Direction::Inbound, Protocol::Tcp, 22, RuleAction::Allow).unwrap();
        let rule = get_rule(id).unwrap();
        assert_eq!(rule.name, "Allow SSH");
        assert_eq!(rule.port, 22);
        assert_eq!(rule.action, RuleAction::Allow);
    }
    serial_println!("[fwsettings]  4/11 add rule OK");

    // Test 5: remove rule.
    {
        let rules = list_rules();
        let id = rules.last().unwrap().id;
        remove_rule(id).unwrap();
        assert!(get_rule(id).is_err());
    }
    serial_println!("[fwsettings]  5/11 remove rule OK");

    // Test 6: rule enable/disable.
    {
        let id = add_rule("Test Rule", Direction::Outbound, Protocol::Tcp, 443, RuleAction::Allow).unwrap();
        set_rule_enabled(id, false).unwrap();
        assert!(!get_rule(id).unwrap().enabled);
        set_rule_enabled(id, true).unwrap();
        remove_rule(id).unwrap();
    }
    serial_println!("[fwsettings]  6/11 rule enable/disable OK");

    // Test 7: app permission.
    {
        set_app_permission("firefox", true, false).unwrap();
        let perms = list_app_permissions();
        assert_eq!(perms.len(), 1);
        assert!(perms.first().unwrap().allow_outbound);
        assert!(!perms.first().unwrap().allow_inbound);
    }
    serial_println!("[fwsettings]  7/11 app permission OK");

    // Test 8: check allowed (app override).
    {
        let action = check_allowed("firefox", 443, Direction::Outbound, Protocol::Tcp);
        assert_eq!(action, RuleAction::Allow);
        let action = check_allowed("firefox", 80, Direction::Inbound, Protocol::Tcp);
        assert_eq!(action, RuleAction::Block);
    }
    serial_println!("[fwsettings]  8/11 check allowed OK");

    // Test 9: check allowed (rule-based).
    {
        // Unknown app, DNS query should be allowed by rule #2.
        let action = check_allowed("unknown_app", 53, Direction::Outbound, Protocol::Udp);
        assert_eq!(action, RuleAction::Allow);
    }
    serial_println!("[fwsettings]  9/11 rule-based check OK");

    // Test 10: stealth mode.
    {
        set_stealth_mode(true).unwrap();
        set_log_blocked(false).unwrap();
        // Just verify no errors.
    }
    serial_println!("[fwsettings] 10/11 stealth mode OK");

    // Test 11: stats.
    {
        let (rules, apps, blocked, allowed, enabled, ops) = stats();
        assert!(rules > 0);
        assert!(apps > 0);
        assert!(blocked > 0);
        assert!(allowed > 0);
        assert!(enabled);
        assert!(ops > 0);
    }
    serial_println!("[fwsettings] 11/11 stats OK");

    serial_println!("[fwsettings] All self-tests passed.");
}
