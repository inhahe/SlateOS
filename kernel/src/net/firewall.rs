//! Packet filtering firewall.
//!
//! A simple stateful firewall for inbound and outbound traffic, with
//! per-namespace isolation for container support.
//!
//! ## Design
//!
//! Rules are evaluated in priority order (lower number = higher priority).
//! First matching rule wins.  If no rule matches, the default policy applies.
//!
//! Connection tracking provides stateful filtering: once a connection is
//! initiated outbound, reply packets are automatically allowed inbound
//! without needing an explicit rule.
//!
//! ## Rule format
//!
//! Each rule specifies:
//! - **Direction**: inbound, outbound, or both.
//! - **Action**: allow or deny.
//! - **Protocol**: TCP, UDP, ICMP, or any.
//! - **Source IP** (with optional prefix mask): 0.0.0.0/0 matches all.
//! - **Destination port** (for TCP/UDP): 0 matches any port.
//! - **Priority**: lower = evaluated first.
//!
//! ## Connection tracking
//!
//! Tracks active connections by (protocol, local_port, remote_ip, remote_port).
//! Inbound packets matching a tracked connection are allowed regardless of
//! rules.  Entries expire after 60 seconds of inactivity (but connections
//! refresh their entry on each packet).
//!
//! ## Per-namespace firewall
//!
//! Each network namespace can have its own independent firewall state:
//! rules, connection tracking, default policy, and enabled flag.  This
//! provides container-level network isolation — a container's firewall
//! rules don't affect the host or other containers.
//!
//! - **Root namespace (ID 0)**: Uses the global firewall state (the
//!   `ENABLED`, `RULES`, `CONNTRACK`, `DEFAULT_POLICY` statics).
//! - **Child namespaces (ID > 0)**: Use per-namespace state in
//!   `NS_FIREWALLS`.  If a namespace hasn't initialized its firewall
//!   (inactive state), all traffic passes through.
//!
//! The `check_outbound_ns()` and `check_inbound_ns()` functions select
//! the appropriate firewall state based on namespace ID.
//!
//! ## Limitations
//!
//! - Maximum 32 rules and 64 tracked connections for the global firewall.
//! - Maximum 16 rules and 32 tracked connections per namespace.
//! - No NAT or port forwarding.
//! - No per-process filtering (per-namespace only).

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use super::interface::Ipv4Addr;
use super::ipv4::{PROTO_ICMP, PROTO_TCP, PROTO_UDP};
use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of firewall rules.
const MAX_RULES: usize = 32;

/// Maximum tracked connections for stateful filtering.
const MAX_CONNTRACK: usize = 64;

/// Connection tracking expiry in nanoseconds (60 seconds).
const CONNTRACK_EXPIRY_NS: u64 = 60_000_000_000;

/// Maximum number of network namespaces with firewall state.
///
/// Must match `netns::MAX_NAMESPACES`.
const MAX_NS_FIREWALL: usize = 64;

/// Maximum firewall rules per namespace (smaller than global to save memory).
const MAX_NS_RULES: usize = 16;

/// Maximum tracked connections per namespace.
const MAX_NS_CONNTRACK: usize = 32;

// ---------------------------------------------------------------------------
// Firewall rule types
// ---------------------------------------------------------------------------

/// Traffic direction for a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Inbound traffic only.
    In,
    /// Outbound traffic only.
    Out,
    /// Both directions.
    Both,
}

/// Action to take when a rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Allow the packet through.
    Allow,
    /// Drop the packet silently.
    Deny,
}

/// Protocol selector for a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// Match any protocol.
    Any,
    /// Match TCP only.
    Tcp,
    /// Match UDP only.
    Udp,
    /// Match ICMP only.
    Icmp,
}

/// A firewall rule.
#[derive(Debug, Clone, Copy)]
pub struct Rule {
    /// Whether this rule slot is active.
    pub active: bool,
    /// Traffic direction.
    pub direction: Direction,
    /// Action to take.
    pub action: Action,
    /// Protocol filter.
    pub protocol: Protocol,
    /// Source IP (0.0.0.0 = any).
    pub src_ip: Ipv4Addr,
    /// Source IP prefix length (0 = match all, 32 = exact match).
    pub src_prefix: u8,
    /// Destination port (0 = any).
    pub dst_port: u16,
    /// Rule priority (lower = higher priority, evaluated first).
    pub priority: u16,
}

impl Rule {
    const fn empty() -> Self {
        Self {
            active: false,
            direction: Direction::Both,
            action: Action::Allow,
            protocol: Protocol::Any,
            src_ip: Ipv4Addr::UNSPECIFIED,
            src_prefix: 0,
            dst_port: 0,
            priority: u16::MAX,
        }
    }
}

/// Default policy when no rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultPolicy {
    /// Allow all traffic not matched by any rule.
    Accept,
    /// Drop all traffic not matched by any rule.
    Drop,
}

// ---------------------------------------------------------------------------
// Connection tracking
// ---------------------------------------------------------------------------

/// A tracked connection for stateful filtering.
#[derive(Clone, Copy)]
struct ConntrackEntry {
    /// Whether this entry is active.
    active: bool,
    /// Protocol (6=TCP, 17=UDP).
    protocol: u8,
    /// Local port.
    local_port: u16,
    /// Remote IP.
    remote_ip: Ipv4Addr,
    /// Remote port.
    remote_port: u16,
    /// Last activity timestamp (nanoseconds).
    last_seen_ns: u64,
}

impl ConntrackEntry {
    const fn empty() -> Self {
        Self {
            active: false,
            protocol: 0,
            local_port: 0,
            remote_ip: Ipv4Addr::UNSPECIFIED,
            remote_port: 0,
            last_seen_ns: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Whether the firewall is enabled.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Default policy.
static DEFAULT_POLICY: Mutex<DefaultPolicy> = Mutex::new(DefaultPolicy::Accept);

/// Rule table (sorted by priority on each modification).
static RULES: Mutex<[Rule; MAX_RULES]> = Mutex::new({
    const EMPTY: Rule = Rule::empty();
    [EMPTY; MAX_RULES]
});

/// Connection tracking table.
static CONNTRACK: Mutex<[ConntrackEntry; MAX_CONNTRACK]> = Mutex::new({
    const EMPTY: ConntrackEntry = ConntrackEntry::empty();
    [EMPTY; MAX_CONNTRACK]
});

/// Counters.
static PACKETS_ALLOWED: AtomicU64 = AtomicU64::new(0);
static PACKETS_DENIED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Per-namespace firewall state
// ---------------------------------------------------------------------------

/// Firewall state for a single network namespace.
///
/// Each non-root namespace can optionally have its own independent
/// firewall with rules, connection tracking, and default policy.
/// When `active` is `false`, no filtering is performed for that
/// namespace (all traffic passes through).
struct NsFirewallState {
    /// Whether this namespace has firewall state initialized.
    active: bool,
    /// Whether the firewall is enabled for this namespace.
    enabled: bool,
    /// Default policy when no rule matches.
    policy: DefaultPolicy,
    /// Rule table.
    rules: [Rule; MAX_NS_RULES],
    /// Connection tracking table.
    conntrack: [ConntrackEntry; MAX_NS_CONNTRACK],
    /// Packets allowed counter.
    allowed: u64,
    /// Packets denied counter.
    denied: u64,
}

impl NsFirewallState {
    const fn empty() -> Self {
        const EMPTY_RULE: Rule = Rule::empty();
        const EMPTY_CT: ConntrackEntry = ConntrackEntry::empty();
        Self {
            active: false,
            enabled: false,
            policy: DefaultPolicy::Accept,
            rules: [EMPTY_RULE; MAX_NS_RULES],
            conntrack: [EMPTY_CT; MAX_NS_CONNTRACK],
            allowed: 0,
            denied: 0,
        }
    }
}

/// Per-namespace firewall state table.
///
/// Index 0 (root namespace) is unused — the root namespace uses the
/// global `ENABLED`, `RULES`, `CONNTRACK`, and `DEFAULT_POLICY` statics
/// for backward compatibility.
static NS_FIREWALLS: Mutex<[NsFirewallState; MAX_NS_FIREWALL]> = Mutex::new({
    const EMPTY: NsFirewallState = NsFirewallState::empty();
    [EMPTY; MAX_NS_FIREWALL]
});

// ---------------------------------------------------------------------------
// Public API — global (root namespace) firewall
// ---------------------------------------------------------------------------

/// Enable the firewall.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
    serial_println!("[firewall] Enabled");
}

/// Disable the firewall (all traffic passes through).
pub fn disable() {
    ENABLED.store(false, Ordering::Relaxed);
    serial_println!("[firewall] Disabled");
}

/// Check if the firewall is enabled.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Set the default policy (when no rule matches).
pub fn set_default_policy(policy: DefaultPolicy) {
    *DEFAULT_POLICY.lock() = policy;
    serial_println!("[firewall] Default policy: {:?}", policy);
}

/// Get the default policy.
pub fn default_policy() -> DefaultPolicy {
    *DEFAULT_POLICY.lock()
}

/// Add a firewall rule.
///
/// Returns the rule index, or error if the table is full.
pub fn add_rule(rule: Rule) -> KernelResult<usize> {
    let mut rules = RULES.lock();
    let slot = rules.iter().position(|r| !r.active)
        .ok_or(KernelError::OutOfMemory)?;

    let mut new_rule = rule;
    new_rule.active = true;
    rules[slot] = new_rule;

    serial_println!(
        "[firewall] Rule added: {:?} {:?} {:?} port={} prio={}",
        rule.direction, rule.action, rule.protocol, rule.dst_port, rule.priority
    );
    Ok(slot)
}

/// Remove a firewall rule by index.
pub fn remove_rule(index: usize) -> KernelResult<()> {
    let mut rules = RULES.lock();
    let rule = rules.get_mut(index)
        .ok_or(KernelError::InvalidArgument)?;
    if !rule.active {
        return Err(KernelError::InvalidArgument);
    }
    rule.active = false;
    Ok(())
}

/// Get the number of active rules.
pub fn rule_count() -> usize {
    let rules = RULES.lock();
    rules.iter().filter(|r| r.active).count()
}

/// Clear all rules.
pub fn clear_rules() {
    let mut rules = RULES.lock();
    for rule in rules.iter_mut() {
        rule.active = false;
    }
}

/// Get firewall statistics.
pub fn stats() -> (u64, u64) {
    (
        PACKETS_ALLOWED.load(Ordering::Relaxed),
        PACKETS_DENIED.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    PACKETS_ALLOWED.store(0, Ordering::Relaxed);
    PACKETS_DENIED.store(0, Ordering::Relaxed);
}

/// Clear all connection tracking entries.
pub fn clear_conntrack() {
    let mut ct = CONNTRACK.lock();
    for entry in ct.iter_mut() {
        entry.active = false;
    }
}

/// Get number of active conntrack entries.
pub fn conntrack_count() -> usize {
    let ct = CONNTRACK.lock();
    ct.iter().filter(|e| e.active).count()
}

// ---------------------------------------------------------------------------
// Connection tracking management
// ---------------------------------------------------------------------------

/// Record an outbound connection for stateful tracking.
///
/// Called when an outbound packet is allowed so that reply packets
/// will be automatically accepted.
pub fn track_connection(protocol: u8, local_port: u16, remote_ip: Ipv4Addr, remote_port: u16) {
    let now = crate::hrtimer::now_ns();
    let mut ct = CONNTRACK.lock();

    // Check if already tracked (refresh timestamp).
    for entry in ct.iter_mut() {
        if entry.active
            && entry.protocol == protocol
            && entry.local_port == local_port
            && entry.remote_ip == remote_ip
            && entry.remote_port == remote_port
        {
            entry.last_seen_ns = now;
            return;
        }
    }

    // Find a free slot (or expire the oldest).
    let slot = ct.iter().position(|e| !e.active)
        .or_else(|| {
            // Expire oldest entry.
            ct.iter()
                .enumerate()
                .filter(|(_, e)| e.active)
                .min_by_key(|(_, e)| e.last_seen_ns)
                .map(|(i, _)| i)
        });

    if let Some(idx) = slot {
        ct[idx] = ConntrackEntry {
            active: true,
            protocol,
            local_port,
            remote_ip,
            remote_port,
            last_seen_ns: now,
        };
    }
}

/// Check if an inbound packet matches a tracked connection.
///
/// If it does, the entry's timestamp is refreshed.
fn is_tracked_reply(protocol: u8, src_ip: Ipv4Addr, src_port: u16, dst_port: u16) -> bool {
    let now = crate::hrtimer::now_ns();
    let mut ct = CONNTRACK.lock();

    for entry in ct.iter_mut() {
        if !entry.active {
            continue;
        }

        // Expire old entries.
        if now.saturating_sub(entry.last_seen_ns) > CONNTRACK_EXPIRY_NS {
            entry.active = false;
            continue;
        }

        // Match: reply from (remote_ip, remote_port) to our local_port.
        if entry.protocol == protocol
            && entry.remote_ip == src_ip
            && entry.remote_port == src_port
            && entry.local_port == dst_port
        {
            entry.last_seen_ns = now;
            return true;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Packet filtering (hook functions)
// ---------------------------------------------------------------------------

/// Check whether an inbound packet should be allowed.
///
/// Called from `ipv4::process_ipv4()` before dispatching to protocol handlers.
///
/// Returns `true` if the packet should be allowed through.
#[allow(clippy::arithmetic_side_effects)]
pub fn check_inbound(protocol: u8, src_ip: Ipv4Addr, payload: &[u8]) -> bool {
    if !ENABLED.load(Ordering::Relaxed) {
        return true;
    }

    // Extract port info from TCP/UDP headers.
    let (src_port, dst_port) = extract_ports(protocol, payload);

    // Check connection tracking first — tracked replies always pass.
    if protocol == PROTO_TCP || protocol == PROTO_UDP {
        if is_tracked_reply(protocol, src_ip, src_port, dst_port) {
            PACKETS_ALLOWED.fetch_add(1, Ordering::Relaxed);
            return true;
        }
    }

    // Check rules.
    let action = match_rules(Direction::In, protocol, src_ip, dst_port);

    match action {
        Some(Action::Allow) => {
            PACKETS_ALLOWED.fetch_add(1, Ordering::Relaxed);
            true
        }
        Some(Action::Deny) => {
            PACKETS_DENIED.fetch_add(1, Ordering::Relaxed);
            false
        }
        None => {
            // No matching rule — use default policy.
            let policy = *DEFAULT_POLICY.lock();
            match policy {
                DefaultPolicy::Accept => {
                    PACKETS_ALLOWED.fetch_add(1, Ordering::Relaxed);
                    true
                }
                DefaultPolicy::Drop => {
                    PACKETS_DENIED.fetch_add(1, Ordering::Relaxed);
                    false
                }
            }
        }
    }
}

/// Check whether an outbound packet should be allowed.
///
/// Called from `ipv4::send()` before constructing the frame.
/// If allowed, also registers the connection for stateful tracking.
///
/// Returns `true` if the packet should be sent.
#[allow(clippy::arithmetic_side_effects)]
pub fn check_outbound(protocol: u8, dst_ip: Ipv4Addr, payload: &[u8]) -> bool {
    if !ENABLED.load(Ordering::Relaxed) {
        return true;
    }

    // Extract port info.
    let (src_port, dst_port) = extract_ports(protocol, payload);

    // Check rules.
    let action = match_rules(Direction::Out, protocol, dst_ip, dst_port);

    let allowed = match action {
        Some(Action::Allow) => true,
        Some(Action::Deny) => false,
        None => {
            let policy = *DEFAULT_POLICY.lock();
            policy == DefaultPolicy::Accept
        }
    };

    if allowed {
        PACKETS_ALLOWED.fetch_add(1, Ordering::Relaxed);
        // Track the connection for stateful reply filtering.
        if (protocol == PROTO_TCP || protocol == PROTO_UDP) && src_port != 0 {
            track_connection(protocol, src_port, dst_ip, dst_port);
        }
    } else {
        PACKETS_DENIED.fetch_add(1, Ordering::Relaxed);
    }

    allowed
}

// ---------------------------------------------------------------------------
// Public API — per-namespace firewall
// ---------------------------------------------------------------------------

/// Initialize firewall state for a network namespace.
///
/// Must be called before any other `ns_*` firewall function for that
/// namespace.  Starts with the firewall disabled and an Accept default
/// policy.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `ns_id` is out of range.
pub fn ns_init(ns_id: u32) -> KernelResult<()> {
    let idx = ns_id as usize;
    if idx == 0 || idx >= MAX_NS_FIREWALL {
        // Root namespace uses the global firewall; out-of-range rejected.
        return Err(KernelError::InvalidArgument);
    }

    let mut table = NS_FIREWALLS.lock();
    table[idx] = NsFirewallState::empty();
    table[idx].active = true;
    serial_println!("[firewall] NS {} firewall initialized", ns_id);
    Ok(())
}

/// Tear down firewall state for a network namespace.
///
/// Clears all rules, connection tracking, and counters.
pub fn ns_destroy(ns_id: u32) {
    let idx = ns_id as usize;
    if idx == 0 || idx >= MAX_NS_FIREWALL {
        return;
    }
    let mut table = NS_FIREWALLS.lock();
    table[idx] = NsFirewallState::empty();
}

/// Enable the firewall for a namespace.
pub fn ns_enable(ns_id: u32) {
    let idx = ns_id as usize;
    if idx == 0 || idx >= MAX_NS_FIREWALL {
        return;
    }
    let mut table = NS_FIREWALLS.lock();
    if table[idx].active {
        table[idx].enabled = true;
        serial_println!("[firewall] NS {} enabled", ns_id);
    }
}

/// Disable the firewall for a namespace.
pub fn ns_disable(ns_id: u32) {
    let idx = ns_id as usize;
    if idx == 0 || idx >= MAX_NS_FIREWALL {
        return;
    }
    let mut table = NS_FIREWALLS.lock();
    if table[idx].active {
        table[idx].enabled = false;
        serial_println!("[firewall] NS {} disabled", ns_id);
    }
}

/// Check if a namespace's firewall is enabled.
pub fn ns_is_enabled(ns_id: u32) -> bool {
    let idx = ns_id as usize;
    if idx == 0 {
        return is_enabled();
    }
    if idx >= MAX_NS_FIREWALL {
        return false;
    }
    let table = NS_FIREWALLS.lock();
    table[idx].active && table[idx].enabled
}

/// Set the default policy for a namespace.
pub fn ns_set_default_policy(ns_id: u32, policy: DefaultPolicy) {
    let idx = ns_id as usize;
    if idx == 0 {
        set_default_policy(policy);
        return;
    }
    if idx >= MAX_NS_FIREWALL {
        return;
    }
    let mut table = NS_FIREWALLS.lock();
    if table[idx].active {
        table[idx].policy = policy;
        serial_println!("[firewall] NS {} default policy: {:?}", ns_id, policy);
    }
}

/// Add a firewall rule to a namespace.
///
/// Returns the rule slot index within the namespace's rule table.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the namespace is invalid.
/// - [`KernelError::OutOfMemory`] if the namespace's rule table is full.
pub fn ns_add_rule(ns_id: u32, rule: Rule) -> KernelResult<usize> {
    let idx = ns_id as usize;
    if idx == 0 {
        return add_rule(rule);
    }
    if idx >= MAX_NS_FIREWALL {
        return Err(KernelError::InvalidArgument);
    }

    let mut table = NS_FIREWALLS.lock();
    if !table[idx].active {
        return Err(KernelError::InvalidArgument);
    }

    let slot = table[idx].rules.iter().position(|r| !r.active)
        .ok_or(KernelError::OutOfMemory)?;

    let mut new_rule = rule;
    new_rule.active = true;
    table[idx].rules[slot] = new_rule;

    serial_println!(
        "[firewall] NS {} rule added: {:?} {:?} {:?} port={} prio={}",
        ns_id, rule.direction, rule.action, rule.protocol,
        rule.dst_port, rule.priority
    );
    Ok(slot)
}

/// Remove a firewall rule from a namespace by index.
pub fn ns_remove_rule(ns_id: u32, rule_idx: usize) -> KernelResult<()> {
    let idx = ns_id as usize;
    if idx == 0 {
        return remove_rule(rule_idx);
    }
    if idx >= MAX_NS_FIREWALL {
        return Err(KernelError::InvalidArgument);
    }

    let mut table = NS_FIREWALLS.lock();
    if !table[idx].active {
        return Err(KernelError::InvalidArgument);
    }
    let rule = table[idx].rules.get_mut(rule_idx)
        .ok_or(KernelError::InvalidArgument)?;
    if !rule.active {
        return Err(KernelError::InvalidArgument);
    }
    rule.active = false;
    Ok(())
}

/// Clear all rules for a namespace.
pub fn ns_clear_rules(ns_id: u32) {
    let idx = ns_id as usize;
    if idx == 0 {
        clear_rules();
        return;
    }
    if idx >= MAX_NS_FIREWALL {
        return;
    }
    let mut table = NS_FIREWALLS.lock();
    if table[idx].active {
        for rule in &mut table[idx].rules {
            rule.active = false;
        }
    }
}

/// Clear all connection tracking entries for a namespace.
pub fn ns_clear_conntrack(ns_id: u32) {
    let idx = ns_id as usize;
    if idx == 0 {
        clear_conntrack();
        return;
    }
    if idx >= MAX_NS_FIREWALL {
        return;
    }
    let mut table = NS_FIREWALLS.lock();
    if table[idx].active {
        for entry in &mut table[idx].conntrack {
            entry.active = false;
        }
    }
}

/// Get the number of active rules for a namespace.
pub fn ns_rule_count(ns_id: u32) -> usize {
    let idx = ns_id as usize;
    if idx == 0 {
        return rule_count();
    }
    if idx >= MAX_NS_FIREWALL {
        return 0;
    }
    let table = NS_FIREWALLS.lock();
    if !table[idx].active {
        return 0;
    }
    table[idx].rules.iter().filter(|r| r.active).count()
}

/// Get firewall statistics for a namespace (allowed, denied).
pub fn ns_stats(ns_id: u32) -> (u64, u64) {
    let idx = ns_id as usize;
    if idx == 0 {
        return stats();
    }
    if idx >= MAX_NS_FIREWALL {
        return (0, 0);
    }
    let table = NS_FIREWALLS.lock();
    if !table[idx].active {
        return (0, 0);
    }
    (table[idx].allowed, table[idx].denied)
}

/// Reset statistics for a namespace.
pub fn ns_reset_stats(ns_id: u32) {
    let idx = ns_id as usize;
    if idx == 0 {
        reset_stats();
        return;
    }
    if idx >= MAX_NS_FIREWALL {
        return;
    }
    let mut table = NS_FIREWALLS.lock();
    if table[idx].active {
        table[idx].allowed = 0;
        table[idx].denied = 0;
    }
}

// ---------------------------------------------------------------------------
// Namespace-aware packet filtering
// ---------------------------------------------------------------------------

/// Check whether an outbound packet should be allowed, using the
/// firewall state for the specified network namespace.
///
/// - For the root namespace (ID 0), delegates to `check_outbound()`.
/// - For child namespaces, checks the namespace's own rules and
///   connection tracking.
/// - If a namespace has no active firewall state, all traffic passes.
#[allow(clippy::arithmetic_side_effects)]
pub fn check_outbound_ns(
    ns_id: u32,
    protocol: u8,
    dst_ip: Ipv4Addr,
    payload: &[u8],
) -> bool {
    let idx = ns_id as usize;

    // Root namespace uses the global firewall.
    if idx == 0 {
        return check_outbound(protocol, dst_ip, payload);
    }

    // Out of range — allow by default.
    if idx >= MAX_NS_FIREWALL {
        return true;
    }

    let mut table = NS_FIREWALLS.lock();

    // No active firewall for this namespace — pass through.
    if !table[idx].active || !table[idx].enabled {
        return true;
    }

    let (src_port, dst_port) = extract_ports(protocol, payload);

    // Check rules.
    let action = match_rules_in_table(&table[idx].rules, Direction::Out, protocol, dst_ip, dst_port);

    let allowed = match action {
        Some(Action::Allow) => true,
        Some(Action::Deny) => false,
        None => table[idx].policy == DefaultPolicy::Accept,
    };

    if allowed {
        table[idx].allowed = table[idx].allowed.wrapping_add(1);
        // Track the connection for stateful reply filtering.
        if (protocol == PROTO_TCP || protocol == PROTO_UDP) && src_port != 0 {
            ns_track_connection(&mut table[idx].conntrack, protocol, src_port, dst_ip, dst_port);
        }
    } else {
        table[idx].denied = table[idx].denied.wrapping_add(1);
    }

    allowed
}

/// Check whether an inbound packet should be allowed, using the
/// firewall state for the specified network namespace.
///
/// - For the root namespace (ID 0), delegates to `check_inbound()`.
/// - For child namespaces, checks the namespace's own rules and
///   connection tracking.
/// - If a namespace has no active firewall state, all traffic passes.
#[allow(clippy::arithmetic_side_effects)]
pub fn check_inbound_ns(
    ns_id: u32,
    protocol: u8,
    src_ip: Ipv4Addr,
    payload: &[u8],
) -> bool {
    let idx = ns_id as usize;

    if idx == 0 {
        return check_inbound(protocol, src_ip, payload);
    }

    if idx >= MAX_NS_FIREWALL {
        return true;
    }

    let mut table = NS_FIREWALLS.lock();

    if !table[idx].active || !table[idx].enabled {
        return true;
    }

    let (src_port, dst_port) = extract_ports(protocol, payload);

    // Check connection tracking first — tracked replies always pass.
    if protocol == PROTO_TCP || protocol == PROTO_UDP {
        if ns_is_tracked_reply(&mut table[idx].conntrack, protocol, src_ip, src_port, dst_port) {
            table[idx].allowed = table[idx].allowed.wrapping_add(1);
            return true;
        }
    }

    // Check rules.
    let action = match_rules_in_table(&table[idx].rules, Direction::In, protocol, src_ip, dst_port);

    match action {
        Some(Action::Allow) => {
            table[idx].allowed = table[idx].allowed.wrapping_add(1);
            true
        }
        Some(Action::Deny) => {
            table[idx].denied = table[idx].denied.wrapping_add(1);
            false
        }
        None => {
            if table[idx].policy == DefaultPolicy::Accept {
                table[idx].allowed = table[idx].allowed.wrapping_add(1);
                true
            } else {
                table[idx].denied = table[idx].denied.wrapping_add(1);
                false
            }
        }
    }
}

/// Track a connection in a namespace's connection tracking table.
fn ns_track_connection(
    conntrack: &mut [ConntrackEntry; MAX_NS_CONNTRACK],
    protocol: u8,
    local_port: u16,
    remote_ip: Ipv4Addr,
    remote_port: u16,
) {
    let now = crate::hrtimer::now_ns();

    // Check if already tracked — refresh timestamp.
    for entry in conntrack.iter_mut() {
        if entry.active
            && entry.protocol == protocol
            && entry.local_port == local_port
            && entry.remote_ip == remote_ip
            && entry.remote_port == remote_port
        {
            entry.last_seen_ns = now;
            return;
        }
    }

    // Find a free slot (or expire the oldest).
    let slot = conntrack.iter().position(|e| !e.active)
        .or_else(|| {
            conntrack.iter()
                .enumerate()
                .filter(|(_, e)| e.active)
                .min_by_key(|(_, e)| e.last_seen_ns)
                .map(|(i, _)| i)
        });

    if let Some(i) = slot {
        conntrack[i] = ConntrackEntry {
            active: true,
            protocol,
            local_port,
            remote_ip,
            remote_port,
            last_seen_ns: now,
        };
    }
}

/// Check if an inbound packet matches a tracked connection in a
/// namespace's connection tracking table.
fn ns_is_tracked_reply(
    conntrack: &mut [ConntrackEntry; MAX_NS_CONNTRACK],
    protocol: u8,
    src_ip: Ipv4Addr,
    src_port: u16,
    dst_port: u16,
) -> bool {
    let now = crate::hrtimer::now_ns();

    for entry in conntrack.iter_mut() {
        if !entry.active {
            continue;
        }

        // Expire old entries.
        if now.saturating_sub(entry.last_seen_ns) > CONNTRACK_EXPIRY_NS {
            entry.active = false;
            continue;
        }

        if entry.protocol == protocol
            && entry.remote_ip == src_ip
            && entry.remote_port == src_port
            && entry.local_port == dst_port
        {
            entry.last_seen_ns = now;
            return true;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extract (src_port, dst_port) from TCP/UDP header.
///
/// Returns (0, 0) for ICMP or if payload is too short.
fn extract_ports(protocol: u8, payload: &[u8]) -> (u16, u16) {
    if (protocol == PROTO_TCP || protocol == PROTO_UDP) && payload.len() >= 4 {
        let src_port = u16::from_be_bytes([payload[0], payload[1]]);
        let dst_port = u16::from_be_bytes([payload[2], payload[3]]);
        (src_port, dst_port)
    } else {
        (0, 0)
    }
}

/// Match a packet against the global rule table.
///
/// Returns `Some(action)` if a rule matches, `None` if no rule matches.
fn match_rules(direction: Direction, protocol: u8, ip: Ipv4Addr, port: u16) -> Option<Action> {
    let rules = RULES.lock();
    match_rules_in_table(&*rules, direction, protocol, ip, port)
}

/// Match a packet against a given rule table.
///
/// Shared logic for both the global firewall and per-namespace firewalls.
/// Returns `Some(action)` if a rule matches, `None` if no rule matches.
fn match_rules_in_table(
    rules: &[Rule],
    direction: Direction,
    protocol: u8,
    ip: Ipv4Addr,
    port: u16,
) -> Option<Action> {
    // Find the highest-priority (lowest number) matching rule.
    let mut best: Option<(u16, Action)> = None;

    for rule in rules.iter() {
        if !rule.active {
            continue;
        }

        // Check direction.
        if rule.direction != Direction::Both && rule.direction != direction {
            continue;
        }

        // Check protocol.
        if !protocol_matches(rule.protocol, protocol) {
            continue;
        }

        // Check IP.
        if !ip_matches(ip, rule.src_ip, rule.src_prefix) {
            continue;
        }

        // Check port.
        if rule.dst_port != 0 && rule.dst_port != port {
            continue;
        }

        // This rule matches — keep the highest priority one.
        match best {
            None => {
                best = Some((rule.priority, rule.action));
                // Priority 0 is the highest possible — no rule can beat it.
                if rule.priority == 0 {
                    break;
                }
            }
            Some((best_prio, _)) => {
                if rule.priority < best_prio {
                    best = Some((rule.priority, rule.action));
                    if rule.priority == 0 {
                        break;
                    }
                }
            }
        }
    }

    best.map(|(_, action)| action)
}

/// Check if a protocol byte matches a protocol selector.
fn protocol_matches(selector: Protocol, proto_byte: u8) -> bool {
    match selector {
        Protocol::Any => true,
        Protocol::Tcp => proto_byte == PROTO_TCP,
        Protocol::Udp => proto_byte == PROTO_UDP,
        Protocol::Icmp => proto_byte == PROTO_ICMP,
    }
}

/// Check if an IP address matches a rule's IP/prefix.
#[allow(clippy::arithmetic_side_effects)]
fn ip_matches(packet_ip: Ipv4Addr, rule_ip: Ipv4Addr, prefix_len: u8) -> bool {
    if prefix_len == 0 || rule_ip == Ipv4Addr::UNSPECIFIED {
        return true; // Match any.
    }
    if prefix_len >= 32 {
        return packet_ip == rule_ip;
    }

    // Compare the first `prefix_len` bits.
    let mask = u32::MAX.checked_shl(32u32.saturating_sub(u32::from(prefix_len)))
        .unwrap_or(0);
    let pkt_u32 = u32::from_be_bytes(packet_ip.0);
    let rule_u32 = u32::from_be_bytes(rule_ip.0);

    (pkt_u32 & mask) == (rule_u32 & mask)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run firewall self-tests.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[firewall] Running firewall self-test...");

    test_disabled_passes_all()?;
    test_default_policy_drop()?;
    test_rule_matching()?;
    test_ip_prefix_match()?;
    test_conntrack()?;
    test_ns_firewall_isolation()?;
    test_ns_firewall_conntrack()?;
    test_ns_firewall_lifecycle()?;

    serial_println!("[firewall] Firewall self-test PASSED (8 tests)");
    Ok(())
}

/// Test 1: When disabled, all packets pass.
fn test_disabled_passes_all() -> KernelResult<()> {
    disable();

    let allowed = check_inbound(PROTO_TCP, Ipv4Addr([10, 0, 0, 1]), &[0, 80, 0, 22]);
    if !allowed {
        serial_println!("[firewall]   FAIL: disabled should allow all");
        return Err(KernelError::InternalError);
    }

    serial_println!("[firewall]   Disabled passes all: OK");
    Ok(())
}

/// Test 2: Default policy DROP blocks when no rules match.
fn test_default_policy_drop() -> KernelResult<()> {
    enable();
    set_default_policy(DefaultPolicy::Drop);
    clear_rules();
    clear_conntrack();
    reset_stats();

    let allowed = check_inbound(PROTO_TCP, Ipv4Addr([10, 0, 0, 1]), &[0, 80, 0, 22]);
    if allowed {
        serial_println!("[firewall]   FAIL: DROP policy should deny");
        disable();
        return Err(KernelError::InternalError);
    }

    let (_, denied) = stats();
    if denied == 0 {
        serial_println!("[firewall]   FAIL: denied counter not incremented");
        disable();
        return Err(KernelError::InternalError);
    }

    // Restore default.
    set_default_policy(DefaultPolicy::Accept);
    disable();
    serial_println!("[firewall]   Default DROP policy: OK");
    Ok(())
}

/// Test 3: Rule matching (allow port 80, deny all else).
fn test_rule_matching() -> KernelResult<()> {
    enable();
    set_default_policy(DefaultPolicy::Drop);
    clear_rules();
    clear_conntrack();
    reset_stats();

    // Allow inbound TCP port 80.
    let rule = Rule {
        active: true,
        direction: Direction::In,
        action: Action::Allow,
        protocol: Protocol::Tcp,
        src_ip: Ipv4Addr::UNSPECIFIED,
        src_prefix: 0,
        dst_port: 80,
        priority: 10,
    };
    add_rule(rule)?;

    // Inbound TCP to port 80 → allowed.
    // TCP header: src_port=12345 (bytes [48, 57]), dst_port=80 (bytes [0, 80]).
    let tcp_80 = [48u8, 57, 0, 80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let allowed = check_inbound(PROTO_TCP, Ipv4Addr([192, 168, 1, 50]), &tcp_80);
    if !allowed {
        serial_println!("[firewall]   FAIL: TCP port 80 should be allowed");
        disable();
        return Err(KernelError::InternalError);
    }

    // Inbound TCP to port 22 → denied (default DROP).
    let tcp_22 = [48u8, 57, 0, 22, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let allowed = check_inbound(PROTO_TCP, Ipv4Addr([192, 168, 1, 50]), &tcp_22);
    if allowed {
        serial_println!("[firewall]   FAIL: TCP port 22 should be denied");
        disable();
        return Err(KernelError::InternalError);
    }

    // Clean up.
    clear_rules();
    set_default_policy(DefaultPolicy::Accept);
    disable();
    serial_println!("[firewall]   Rule matching: OK");
    Ok(())
}

/// Test 4: IP prefix matching.
fn test_ip_prefix_match() -> KernelResult<()> {
    // /24 match: 192.168.1.0/24 should match 192.168.1.x.
    if !ip_matches(Ipv4Addr([192, 168, 1, 100]), Ipv4Addr([192, 168, 1, 0]), 24) {
        serial_println!("[firewall]   FAIL: /24 should match .100");
        return Err(KernelError::InternalError);
    }

    // /24 should NOT match 192.168.2.x.
    if ip_matches(Ipv4Addr([192, 168, 2, 100]), Ipv4Addr([192, 168, 1, 0]), 24) {
        serial_println!("[firewall]   FAIL: /24 should not match .2.100");
        return Err(KernelError::InternalError);
    }

    // /0 matches everything.
    if !ip_matches(Ipv4Addr([10, 0, 0, 1]), Ipv4Addr::UNSPECIFIED, 0) {
        serial_println!("[firewall]   FAIL: /0 should match any");
        return Err(KernelError::InternalError);
    }

    // /32 exact match.
    if !ip_matches(Ipv4Addr([10, 0, 0, 1]), Ipv4Addr([10, 0, 0, 1]), 32) {
        serial_println!("[firewall]   FAIL: /32 should exact match");
        return Err(KernelError::InternalError);
    }
    if ip_matches(Ipv4Addr([10, 0, 0, 2]), Ipv4Addr([10, 0, 0, 1]), 32) {
        serial_println!("[firewall]   FAIL: /32 should not match different IP");
        return Err(KernelError::InternalError);
    }

    serial_println!("[firewall]   IP prefix matching: OK");
    Ok(())
}

/// Test 5: Connection tracking (outbound creates entry, inbound reply passes).
fn test_conntrack() -> KernelResult<()> {
    enable();
    set_default_policy(DefaultPolicy::Drop);
    clear_rules();
    clear_conntrack();

    // Allow all outbound.
    let rule = Rule {
        active: true,
        direction: Direction::Out,
        action: Action::Allow,
        protocol: Protocol::Any,
        src_ip: Ipv4Addr::UNSPECIFIED,
        src_prefix: 0,
        dst_port: 0,
        priority: 1,
    };
    add_rule(rule)?;

    // Simulate outbound TCP from local port 49200 to 93.184.216.34:80.
    // TCP header: src=49200 (0xC030), dst=80 (0x0050).
    let tcp_out = [0xC0u8, 0x30, 0x00, 0x50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let allowed = check_outbound(PROTO_TCP, Ipv4Addr([93, 184, 216, 34]), &tcp_out);
    if !allowed {
        serial_println!("[firewall]   FAIL: outbound should be allowed");
        disable();
        return Err(KernelError::InternalError);
    }

    // Now inbound reply: from 93.184.216.34:80 → our port 49200.
    // TCP header: src=80, dst=49200.
    let tcp_reply = [0x00u8, 0x50, 0xC0, 0x30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let allowed = check_inbound(PROTO_TCP, Ipv4Addr([93, 184, 216, 34]), &tcp_reply);
    if !allowed {
        serial_println!("[firewall]   FAIL: reply should be allowed via conntrack");
        disable();
        return Err(KernelError::InternalError);
    }

    // Inbound from a different IP should NOT be tracked.
    let tcp_other = [0x00u8, 0x50, 0xC0, 0x30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let allowed = check_inbound(PROTO_TCP, Ipv4Addr([1, 2, 3, 4]), &tcp_other);
    if allowed {
        serial_println!("[firewall]   FAIL: untracked IP should be denied");
        disable();
        return Err(KernelError::InternalError);
    }

    // Clean up.
    clear_rules();
    clear_conntrack();
    set_default_policy(DefaultPolicy::Accept);
    disable();
    serial_println!("[firewall]   Connection tracking: OK");
    Ok(())
}

/// Test 6: Per-namespace firewall isolation.
///
/// Rules in one namespace don't affect another.
fn test_ns_firewall_isolation() -> KernelResult<()> {
    // Use namespace IDs 1 and 2 (not root).
    let ns1: u32 = 1;
    let ns2: u32 = 2;

    // Initialize both.
    ns_init(ns1)?;
    ns_init(ns2)?;

    // Enable both with DROP policy.
    ns_enable(ns1);
    ns_enable(ns2);
    ns_set_default_policy(ns1, DefaultPolicy::Drop);
    ns_set_default_policy(ns2, DefaultPolicy::Drop);

    // Add allow rule for port 80 in ns1 only.
    let rule = Rule {
        active: true,
        direction: Direction::In,
        action: Action::Allow,
        protocol: Protocol::Tcp,
        src_ip: Ipv4Addr::UNSPECIFIED,
        src_prefix: 0,
        dst_port: 80,
        priority: 10,
    };
    ns_add_rule(ns1, rule)?;

    // TCP to port 80: allowed in ns1, denied in ns2.
    let tcp_80 = [48u8, 57, 0, 80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let allowed_ns1 = check_inbound_ns(ns1, PROTO_TCP, Ipv4Addr([10, 0, 0, 1]), &tcp_80);
    let allowed_ns2 = check_inbound_ns(ns2, PROTO_TCP, Ipv4Addr([10, 0, 0, 1]), &tcp_80);

    if !allowed_ns1 {
        serial_println!("[firewall]   FAIL: ns1 port 80 should be allowed");
        ns_destroy(ns1);
        ns_destroy(ns2);
        return Err(KernelError::InternalError);
    }
    if allowed_ns2 {
        serial_println!("[firewall]   FAIL: ns2 port 80 should be denied");
        ns_destroy(ns1);
        ns_destroy(ns2);
        return Err(KernelError::InternalError);
    }

    // Verify stats are per-namespace.
    let (a1, _) = ns_stats(ns1);
    let (_, d2) = ns_stats(ns2);
    if a1 == 0 {
        serial_println!("[firewall]   FAIL: ns1 allowed counter should be > 0");
        ns_destroy(ns1);
        ns_destroy(ns2);
        return Err(KernelError::InternalError);
    }
    if d2 == 0 {
        serial_println!("[firewall]   FAIL: ns2 denied counter should be > 0");
        ns_destroy(ns1);
        ns_destroy(ns2);
        return Err(KernelError::InternalError);
    }

    ns_destroy(ns1);
    ns_destroy(ns2);
    serial_println!("[firewall]   Per-namespace isolation: OK");
    Ok(())
}

/// Test 7: Per-namespace connection tracking.
///
/// Outbound from a namespace creates a conntrack entry; reply passes.
fn test_ns_firewall_conntrack() -> KernelResult<()> {
    let ns: u32 = 3;
    ns_init(ns)?;
    ns_enable(ns);
    ns_set_default_policy(ns, DefaultPolicy::Drop);

    // Allow all outbound.
    let rule = Rule {
        active: true,
        direction: Direction::Out,
        action: Action::Allow,
        protocol: Protocol::Any,
        src_ip: Ipv4Addr::UNSPECIFIED,
        src_prefix: 0,
        dst_port: 0,
        priority: 1,
    };
    ns_add_rule(ns, rule)?;

    // Outbound TCP from port 50000 to 93.184.216.34:443.
    let tcp_out = [0xC3u8, 0x50, 0x01, 0xBB, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let allowed = check_outbound_ns(ns, PROTO_TCP, Ipv4Addr([93, 184, 216, 34]), &tcp_out);
    if !allowed {
        serial_println!("[firewall]   FAIL: ns outbound should be allowed");
        ns_destroy(ns);
        return Err(KernelError::InternalError);
    }

    // Inbound reply from 93.184.216.34:443 to our port 50000.
    let tcp_reply = [0x01u8, 0xBB, 0xC3, 0x50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let allowed = check_inbound_ns(ns, PROTO_TCP, Ipv4Addr([93, 184, 216, 34]), &tcp_reply);
    if !allowed {
        serial_println!("[firewall]   FAIL: ns reply should be allowed via conntrack");
        ns_destroy(ns);
        return Err(KernelError::InternalError);
    }

    // Inbound from a different IP should be denied.
    let tcp_other = [0x01u8, 0xBB, 0xC3, 0x50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let allowed = check_inbound_ns(ns, PROTO_TCP, Ipv4Addr([1, 2, 3, 4]), &tcp_other);
    if allowed {
        serial_println!("[firewall]   FAIL: ns untracked IP should be denied");
        ns_destroy(ns);
        return Err(KernelError::InternalError);
    }

    ns_destroy(ns);
    serial_println!("[firewall]   Per-namespace conntrack: OK");
    Ok(())
}

/// Test 8: Per-namespace firewall lifecycle.
///
/// Tests init, rule management, and destroy.
fn test_ns_firewall_lifecycle() -> KernelResult<()> {
    let ns: u32 = 4;

    // Before init, ns check passes through (no active state).
    let tcp = [0u8, 80, 0, 22, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let allowed = check_inbound_ns(ns, PROTO_TCP, Ipv4Addr([10, 0, 0, 1]), &tcp);
    if !allowed {
        serial_println!("[firewall]   FAIL: uninit ns should pass through");
        return Err(KernelError::InternalError);
    }

    // Init and enable.
    ns_init(ns)?;
    ns_enable(ns);

    // With Accept policy, everything passes.
    let allowed = check_inbound_ns(ns, PROTO_TCP, Ipv4Addr([10, 0, 0, 1]), &tcp);
    if !allowed {
        serial_println!("[firewall]   FAIL: Accept policy should allow");
        ns_destroy(ns);
        return Err(KernelError::InternalError);
    }

    // Add a deny rule for port 22.
    let rule = Rule {
        active: true,
        direction: Direction::In,
        action: Action::Deny,
        protocol: Protocol::Tcp,
        src_ip: Ipv4Addr::UNSPECIFIED,
        src_prefix: 0,
        dst_port: 22,
        priority: 5,
    };
    let idx = ns_add_rule(ns, rule)?;

    // Port 22 denied, port 80 still allowed (Accept policy).
    let tcp_22 = [0u8, 80, 0, 22, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let tcp_80 = [0u8, 80, 0, 80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    if check_inbound_ns(ns, PROTO_TCP, Ipv4Addr([10, 0, 0, 1]), &tcp_22) {
        serial_println!("[firewall]   FAIL: port 22 should be denied");
        ns_destroy(ns);
        return Err(KernelError::InternalError);
    }
    if !check_inbound_ns(ns, PROTO_TCP, Ipv4Addr([10, 0, 0, 1]), &tcp_80) {
        serial_println!("[firewall]   FAIL: port 80 should be allowed");
        ns_destroy(ns);
        return Err(KernelError::InternalError);
    }

    // Remove the rule — port 22 should be allowed again.
    ns_remove_rule(ns, idx)?;
    if !check_inbound_ns(ns, PROTO_TCP, Ipv4Addr([10, 0, 0, 1]), &tcp_22) {
        serial_println!("[firewall]   FAIL: port 22 should be allowed after rule removal");
        ns_destroy(ns);
        return Err(KernelError::InternalError);
    }

    // Rule count should be 0.
    if ns_rule_count(ns) != 0 {
        serial_println!("[firewall]   FAIL: rule count should be 0");
        ns_destroy(ns);
        return Err(KernelError::InternalError);
    }

    // Disable — all passes through again regardless of policy.
    ns_set_default_policy(ns, DefaultPolicy::Drop);
    ns_disable(ns);
    if !check_inbound_ns(ns, PROTO_TCP, Ipv4Addr([10, 0, 0, 1]), &tcp_22) {
        serial_println!("[firewall]   FAIL: disabled ns should pass through");
        ns_destroy(ns);
        return Err(KernelError::InternalError);
    }

    // Destroy clears state.
    ns_destroy(ns);
    if ns_is_enabled(ns) {
        serial_println!("[firewall]   FAIL: destroyed ns should not be enabled");
        return Err(KernelError::InternalError);
    }

    // Cannot init root namespace (ID 0).
    if ns_init(0).is_ok() {
        serial_println!("[firewall]   FAIL: should not init root ns");
        return Err(KernelError::InternalError);
    }

    serial_println!("[firewall]   Per-namespace lifecycle: OK");
    Ok(())
}
