//! Packet filtering firewall.
//!
//! A simple stateful firewall for inbound and outbound traffic.
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
//! ## Limitations
//!
//! - Maximum 32 rules and 64 tracked connections.
//! - No NAT or port forwarding.
//! - No per-process filtering (applies globally).
//!
//! ## Namespace note
//!
//! Firewall rules are currently global — they apply to all network
//! namespaces equally.  Per-namespace firewall state (independent rule
//! tables and connection tracking per namespace) is planned for the
//! future, as documented in `netns.rs`.  The `ipv4::send_ns()` function
//! already runs the global firewall check before sending; when
//! per-namespace firewall is implemented, it will check the namespace's
//! rule table instead.

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
// Public API
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

/// Match a packet against the rule table.
///
/// Returns `Some(action)` if a rule matches, `None` if no rule matches.
fn match_rules(direction: Direction, protocol: u8, ip: Ipv4Addr, port: u16) -> Option<Action> {
    let rules = RULES.lock();

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
            None => best = Some((rule.priority, rule.action)),
            Some((best_prio, _)) => {
                if rule.priority < best_prio {
                    best = Some((rule.priority, rule.action));
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

    serial_println!("[firewall] Firewall self-test PASSED");
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
