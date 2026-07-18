//! IGMP (Internet Group Management Protocol) implementation.
//!
//! Implements IGMPv2 (RFC 2236) for multicast group membership
//! management.  Sends membership reports when joining groups and
//! responds to queries from multicast routers so they know which
//! groups we're interested in.
//!
//! ## Protocol overview
//!
//! ```text
//! Type      Code  Use
//! ────────  ────  ──────────────────────────────────────
//! 0x11      -     Membership Query (General or Group-Specific)
//! 0x16      -     IGMPv2 Membership Report
//! 0x17      -     Leave Group
//! 0x12      -     IGMPv1 Membership Report (backward compat)
//! ```
//!
//! ## IGMP message format (8 bytes)
//!
//! ```text
//! ┌─────────┬─────────────────┬─────────────────────┐
//! │ Type(8) │ Max Resp Time(8)│ Checksum (16)       │
//! ├─────────┴─────────────────┴─────────────────────┤
//! │ Group Address (32)                              │
//! └─────────────────────────────────────────────────┘
//! ```
//!
//! ## Integration
//!
//! - When `udp::join_group()` is called, this module sends an
//!   IGMPv2 Membership Report to the group address.
//! - When `udp::leave_group()` is called and refcount drops to 0,
//!   this module sends a Leave Group message to 224.0.0.2 (all-routers).
//! - When a General or Group-Specific Query is received, this module
//!   schedules and sends Membership Reports for all active groups.
//! - Periodic unsolicited reports are sent for active groups.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::KernelResult;
use super::interface::Ipv4Addr;
use super::ipv4;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// IGMP protocol number in the IP header.
pub const PROTO_IGMP: u8 = 2;

/// IGMPv2 Membership Query.
const IGMP_MEMBERSHIP_QUERY: u8 = 0x11;

/// IGMPv1 Membership Report (backward compat — we accept but don't send).
const IGMP_V1_MEMBERSHIP_REPORT: u8 = 0x12;

/// IGMPv2 Membership Report.
const IGMP_V2_MEMBERSHIP_REPORT: u8 = 0x16;

/// Leave Group.
const IGMP_LEAVE_GROUP: u8 = 0x17;

/// IGMP message size.
const IGMP_MSG_SIZE: usize = 8;

/// All-hosts multicast address (224.0.0.1).
/// General Queries are sent to this address.
const ALL_HOSTS: Ipv4Addr = Ipv4Addr([224, 0, 0, 1]);

/// All-routers multicast address (224.0.0.2).
/// Leave Group messages are sent to this address.
const ALL_ROUTERS: Ipv4Addr = Ipv4Addr([224, 0, 0, 2]);

/// Maximum multicast groups we track for IGMP.
const MAX_GROUPS: usize = 32;

/// Default unsolicited report interval (ns).
/// RFC 2236 §8.4 recommends 10 seconds.
const UNSOLICITED_REPORT_INTERVAL_NS: u64 = 10_000_000_000;

/// Timer interval (ns) between periodic tick checks.
/// We check every 5 seconds (aligned with the net::poll tick).
const TICK_INTERVAL_NS: u64 = 5_000_000_000;

// ---------------------------------------------------------------------------
// Group membership state
// ---------------------------------------------------------------------------

/// State of a multicast group membership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupState {
    /// No membership (slot is free).
    Idle,
    /// Delay timer running (waiting to send a report).
    DelayingMember,
    /// Stable member (report already sent, no pending query).
    IdleMember,
}

/// A single tracked multicast group.
#[derive(Debug, Clone, Copy)]
struct GroupEntry {
    /// Multicast group address.
    addr: Ipv4Addr,
    /// Current state.
    state: GroupState,
    /// When to send the next report (monotonic ns).
    /// Used for DelayingMember: send report when now >= report_at.
    report_at_ns: u64,
    /// Last time an unsolicited report was sent.
    last_report_ns: u64,
}

impl GroupEntry {
    const fn empty() -> Self {
        Self {
            addr: Ipv4Addr::UNSPECIFIED,
            state: GroupState::Idle,
            report_at_ns: 0,
            last_report_ns: 0,
        }
    }
}

/// Global group membership table.
static GROUPS: Mutex<[GroupEntry; MAX_GROUPS]> =
    Mutex::new([GroupEntry::empty(); MAX_GROUPS]);

/// Last tick timestamp (for rate-limiting periodic work).
static LAST_TICK_NS: AtomicU64 = AtomicU64::new(0);

// Statistics.
static REPORTS_SENT: AtomicU64 = AtomicU64::new(0);
static LEAVES_SENT: AtomicU64 = AtomicU64::new(0);
static QUERIES_RECEIVED: AtomicU64 = AtomicU64::new(0);
static ERRORS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Packet building
// ---------------------------------------------------------------------------

/// Build an IGMPv2 Membership Report message.
#[allow(clippy::arithmetic_side_effects)]
fn build_report(group: Ipv4Addr) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(IGMP_MSG_SIZE);

    pkt.push(IGMP_V2_MEMBERSHIP_REPORT);
    pkt.push(0); // Max Resp Time (unused in reports).
    pkt.extend_from_slice(&[0, 0]); // Checksum placeholder.
    pkt.extend_from_slice(&group.0);

    // Compute checksum.
    let cksum = ipv4::ip_checksum(&pkt);
    pkt[2] = (cksum >> 8) as u8;
    pkt[3] = cksum as u8;

    pkt
}

/// Build an IGMPv2 Leave Group message.
#[allow(clippy::arithmetic_side_effects)]
fn build_leave(group: Ipv4Addr) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(IGMP_MSG_SIZE);

    pkt.push(IGMP_LEAVE_GROUP);
    pkt.push(0); // Max Resp Time (unused).
    pkt.extend_from_slice(&[0, 0]); // Checksum placeholder.
    pkt.extend_from_slice(&group.0);

    let cksum = ipv4::ip_checksum(&pkt);
    pkt[2] = (cksum >> 8) as u8;
    pkt[3] = cksum as u8;

    pkt
}

// ---------------------------------------------------------------------------
// Sending
// ---------------------------------------------------------------------------

/// Send an IGMPv2 Membership Report for a group.
///
/// The report is sent to the group address itself (not all-hosts),
/// per RFC 2236 §3.
fn send_report(group: Ipv4Addr) -> KernelResult<()> {
    let pkt = build_report(group);
    // IGMPv2 reports should use TTL=1 (link-local scope).
    ipv4::send_with_ttl(group, PROTO_IGMP, &pkt, 1)?;
    REPORTS_SENT.fetch_add(1, Ordering::Relaxed);
    crate::serial_println!("[igmp] Sent membership report for {}", group);
    Ok(())
}

/// Send an IGMPv2 Leave Group message.
///
/// Leave messages are sent to 224.0.0.2 (all-routers), per RFC 2236 §3.
fn send_leave(group: Ipv4Addr) -> KernelResult<()> {
    let pkt = build_leave(group);
    ipv4::send_with_ttl(ALL_ROUTERS, PROTO_IGMP, &pkt, 1)?;
    LEAVES_SENT.fetch_add(1, Ordering::Relaxed);
    crate::serial_println!("[igmp] Sent leave for {}", group);
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API — called from udp module
// ---------------------------------------------------------------------------

/// Notify IGMP that we've joined a multicast group.
///
/// Sends an unsolicited Membership Report and adds the group
/// to the tracking table.
pub fn join(group: Ipv4Addr) {
    if !group.is_multicast() {
        return;
    }

    let now = crate::hrtimer::now_ns();
    let mut groups = GROUPS.lock();

    // Check if already tracked.
    for entry in groups.iter() {
        if entry.state != GroupState::Idle && entry.addr == group {
            // Already a member — just send another report.
            drop(groups);
            let _ = send_report(group);
            return;
        }
    }

    // Find a free slot.
    for entry in groups.iter_mut() {
        if entry.state == GroupState::Idle {
            *entry = GroupEntry {
                addr: group,
                state: GroupState::IdleMember,
                report_at_ns: 0,
                last_report_ns: now,
            };
            drop(groups);

            // Send initial unsolicited report (RFC 2236 §6).
            let _ = send_report(group);
            return;
        }
    }

    // Table full — still send the report, just don't track.
    drop(groups);
    ERRORS.fetch_add(1, Ordering::Relaxed);
    let _ = send_report(group);
}

/// Notify IGMP that we've left a multicast group.
///
/// Sends a Leave Group message and removes the group from tracking.
pub fn leave(group: Ipv4Addr) {
    if !group.is_multicast() {
        return;
    }

    let mut groups = GROUPS.lock();
    for entry in groups.iter_mut() {
        if entry.state != GroupState::Idle && entry.addr == group {
            entry.state = GroupState::Idle;
            entry.addr = Ipv4Addr::UNSPECIFIED;
            drop(groups);

            // Send Leave Group (RFC 2236 §6).
            let _ = send_leave(group);
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Incoming IGMP processing
// ---------------------------------------------------------------------------

/// Process an incoming IGMP message.
///
/// Called from the IPv4 layer when protocol == 2 (IGMP).
pub fn process(ip_packet: &ipv4::Ipv4Packet<'_>, data: &[u8]) -> KernelResult<()> {
    if data.len() < IGMP_MSG_SIZE {
        return Ok(());
    }

    // Verify checksum.
    if ipv4::ip_checksum(data) != 0 {
        crate::serial_println!("[igmp] Dropped — bad checksum");
        return Ok(());
    }

    let msg_type = data[0];
    let max_resp_time = data[1]; // In units of 1/10 second.
    let group_addr = Ipv4Addr([
        *data.get(4).unwrap_or(&0),
        *data.get(5).unwrap_or(&0),
        *data.get(6).unwrap_or(&0),
        *data.get(7).unwrap_or(&0),
    ]);

    match msg_type {
        IGMP_MEMBERSHIP_QUERY => {
            QUERIES_RECEIVED.fetch_add(1, Ordering::Relaxed);

            if group_addr.is_unspecified() {
                // General Query — respond for all groups.
                handle_general_query(max_resp_time);
            } else {
                // Group-Specific Query.
                handle_group_query(group_addr, max_resp_time);
            }
        }
        IGMP_V1_MEMBERSHIP_REPORT | IGMP_V2_MEMBERSHIP_REPORT => {
            // Another host on the network is also a member of this group.
            // Suppress our own report (RFC 2236 §3) if we have a pending
            // timer for this group.
            suppress_report(group_addr);
        }
        IGMP_LEAVE_GROUP => {
            // Leave from another host — informational.
            crate::serial_println!("[igmp] Leave from {} for {}", ip_packet.src, group_addr);
        }
        _ => {
            // Unknown type — ignore.
        }
    }

    Ok(())
}

/// Handle a General Query: schedule reports for all active groups.
fn handle_general_query(max_resp_time: u8) {
    let now = crate::hrtimer::now_ns();
    // max_resp_time is in 1/10 second units.
    let max_delay_ns = (max_resp_time as u64).saturating_mul(100_000_000);
    // Use half the max delay as our response time (simple deterministic
    // approach — a full implementation would use random delay).
    let delay_ns = max_delay_ns / 2;

    let mut groups = GROUPS.lock();
    for entry in groups.iter_mut() {
        if entry.state != GroupState::Idle {
            entry.state = GroupState::DelayingMember;
            entry.report_at_ns = now.saturating_add(delay_ns);
        }
    }

    crate::serial_println!(
        "[igmp] General query (max_resp={}ds), scheduling reports",
        max_resp_time
    );
}

/// Handle a Group-Specific Query: schedule report for one group.
fn handle_group_query(group: Ipv4Addr, max_resp_time: u8) {
    let now = crate::hrtimer::now_ns();
    let max_delay_ns = (max_resp_time as u64).saturating_mul(100_000_000);
    let delay_ns = max_delay_ns / 2;

    let mut groups = GROUPS.lock();
    for entry in groups.iter_mut() {
        if entry.state != GroupState::Idle && entry.addr == group {
            entry.state = GroupState::DelayingMember;
            entry.report_at_ns = now.saturating_add(delay_ns);
            break;
        }
    }

    crate::serial_println!(
        "[igmp] Group-specific query for {} (max_resp={}ds)",
        group, max_resp_time
    );
}

/// Suppress a pending report for a group (RFC 2236 report suppression).
///
/// If another host on the network sends a report for the same group,
/// we cancel our pending report to avoid duplicates.
fn suppress_report(group: Ipv4Addr) {
    let mut groups = GROUPS.lock();
    for entry in groups.iter_mut() {
        if entry.state == GroupState::DelayingMember && entry.addr == group {
            entry.state = GroupState::IdleMember;
            crate::serial_println!("[igmp] Suppressed report for {} (heard from peer)", group);
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Periodic tick
// ---------------------------------------------------------------------------

/// Periodic IGMP maintenance.
///
/// - Sends delayed reports for groups in `DelayingMember` state.
/// - Sends periodic unsolicited reports for all active groups.
///
/// Called from `net::poll()` via the 5-second maintenance tick.
pub fn tick() {
    let now = crate::hrtimer::now_ns();
    let last = LAST_TICK_NS.load(Ordering::Relaxed);
    if now.saturating_sub(last) < TICK_INTERVAL_NS {
        return;
    }
    LAST_TICK_NS.store(now, Ordering::Relaxed);

    // Collect groups that need reports (to avoid holding lock while sending).
    let mut to_report: [Ipv4Addr; MAX_GROUPS] = [Ipv4Addr::UNSPECIFIED; MAX_GROUPS];
    let mut report_count = 0usize;

    {
        let mut groups = GROUPS.lock();
        for entry in groups.iter_mut() {
            match entry.state {
                GroupState::DelayingMember => {
                    if now >= entry.report_at_ns {
                        // Timer expired — send report.
                        if report_count < MAX_GROUPS {
                            to_report[report_count] = entry.addr;
                            report_count = report_count.saturating_add(1);
                        }
                        entry.state = GroupState::IdleMember;
                        entry.last_report_ns = now;
                    }
                }
                GroupState::IdleMember => {
                    // Periodic unsolicited report.
                    if now.saturating_sub(entry.last_report_ns) >= UNSOLICITED_REPORT_INTERVAL_NS {
                        if report_count < MAX_GROUPS {
                            to_report[report_count] = entry.addr;
                            report_count = report_count.saturating_add(1);
                        }
                        entry.last_report_ns = now;
                    }
                }
                GroupState::Idle => {}
            }
        }
    }

    // Send the collected reports (lock released).
    for i in 0..report_count {
        let _ = send_report(to_report[i]);
    }
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// IGMP statistics.
#[derive(Debug)]
pub struct IgmpStats {
    pub active_groups: usize,
    pub reports_sent: u64,
    pub leaves_sent: u64,
    pub queries_received: u64,
    pub errors: u64,
}

/// Get IGMP statistics.
pub fn stats() -> IgmpStats {
    let groups = GROUPS.lock();
    let active = groups.iter().filter(|e| e.state != GroupState::Idle).count();
    IgmpStats {
        active_groups: active,
        reports_sent: REPORTS_SENT.load(Ordering::Relaxed),
        leaves_sent: LEAVES_SENT.load(Ordering::Relaxed),
        queries_received: QUERIES_RECEIVED.load(Ordering::Relaxed),
        errors: ERRORS.load(Ordering::Relaxed),
    }
}

/// Information about a tracked multicast group (for display).
#[derive(Debug)]
pub struct GroupInfo {
    pub addr: Ipv4Addr,
    pub state: &'static str,
}

/// List all active IGMP group memberships.
pub fn list_groups() -> (Vec<GroupInfo>, usize) {
    let groups = GROUPS.lock();
    let mut infos = Vec::new();
    for entry in groups.iter() {
        if entry.state != GroupState::Idle {
            infos.push(GroupInfo {
                addr: entry.addr,
                state: match entry.state {
                    GroupState::Idle => "idle",
                    GroupState::DelayingMember => "delaying",
                    GroupState::IdleMember => "member",
                },
            });
        }
    }
    let count = infos.len();
    (infos, count)
}

/// Generate procfs content for `/proc/igmp`.
pub fn procfs_content() -> String {
    let s = stats();
    let (groups, count) = list_groups();

    let mut out = String::with_capacity(512);
    out.push_str("IGMP (Internet Group Management Protocol)\n");
    out.push_str("=========================================\n\n");
    out.push_str(&format!("Active groups:    {}\n", s.active_groups));
    out.push_str(&format!("Reports sent:     {}\n", s.reports_sent));
    out.push_str(&format!("Leaves sent:      {}\n", s.leaves_sent));
    out.push_str(&format!("Queries received: {}\n", s.queries_received));
    out.push_str(&format!("Errors:           {}\n", s.errors));

    if count > 0 {
        out.push_str("\nGroup Memberships:\n");
        for info in &groups {
            out.push_str(&format!("  {}  ({})\n", info.addr, info.state));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run IGMP self-tests.
// Self-tests deliberately runtime-assert RFC-defined constants
// (message-type codes, protocol numbers) as living documentation.
#[allow(clippy::assertions_on_constants)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[igmp] Running IGMP self-tests...");
    let mut passed = 0u32;

    // --- Test 1: Report construction ---
    {
        let group = Ipv4Addr::new(239, 1, 1, 1);
        let pkt = build_report(group);
        assert!(pkt.len() == IGMP_MSG_SIZE, "report size");
        assert!(pkt[0] == IGMP_V2_MEMBERSHIP_REPORT, "report type");
        assert!(pkt[1] == 0, "max resp time");
        // Group address at bytes 4-7.
        assert!(pkt[4] == 239, "group byte 0");
        assert!(pkt[5] == 1, "group byte 1");
        assert!(pkt[6] == 1, "group byte 2");
        assert!(pkt[7] == 1, "group byte 3");
        // Checksum verifies.
        assert!(ipv4::ip_checksum(&pkt) == 0, "report checksum");

        passed = passed.saturating_add(1);
        crate::serial_println!("[igmp]   test 1 (report construction) PASSED");
    }

    // --- Test 2: Leave construction ---
    {
        let group = Ipv4Addr::new(239, 2, 2, 2);
        let pkt = build_leave(group);
        assert!(pkt.len() == IGMP_MSG_SIZE, "leave size");
        assert!(pkt[0] == IGMP_LEAVE_GROUP, "leave type");
        assert!(ipv4::ip_checksum(&pkt) == 0, "leave checksum");

        passed = passed.saturating_add(1);
        crate::serial_println!("[igmp]   test 2 (leave construction) PASSED");
    }

    // --- Test 3: Constants ---
    {
        assert!(PROTO_IGMP == 2, "IGMP protocol number");
        assert!(IGMP_MEMBERSHIP_QUERY == 0x11, "query type");
        assert!(IGMP_V2_MEMBERSHIP_REPORT == 0x16, "report type");
        assert!(IGMP_LEAVE_GROUP == 0x17, "leave type");
        assert!(IGMP_MSG_SIZE == 8, "message size");
        assert!(ALL_HOSTS == Ipv4Addr([224, 0, 0, 1]), "all-hosts");
        assert!(ALL_ROUTERS == Ipv4Addr([224, 0, 0, 2]), "all-routers");

        passed = passed.saturating_add(1);
        crate::serial_println!("[igmp]   test 3 (constants) PASSED");
    }

    // --- Test 4: GroupEntry default ---
    {
        let entry = GroupEntry::empty();
        assert!(entry.state == GroupState::Idle, "default state");
        assert!(entry.addr.is_unspecified(), "default addr");
        assert!(entry.report_at_ns == 0, "default report_at");
        assert!(entry.last_report_ns == 0, "default last_report");

        passed = passed.saturating_add(1);
        crate::serial_println!("[igmp]   test 4 (GroupEntry default) PASSED");
    }

    // --- Test 5: Report checksum independence ---
    {
        // Different groups should produce different checksums.
        let r1 = build_report(Ipv4Addr::new(239, 0, 0, 1));
        let r2 = build_report(Ipv4Addr::new(239, 0, 0, 2));
        // Both must have valid checksums.
        assert!(ipv4::ip_checksum(&r1) == 0, "r1 checksum");
        assert!(ipv4::ip_checksum(&r2) == 0, "r2 checksum");
        // But the raw bytes differ.
        assert!(r1 != r2, "different groups → different packets");

        passed = passed.saturating_add(1);
        crate::serial_println!("[igmp]   test 5 (checksum independence) PASSED");
    }

    // --- Test 6: Stats accessible ---
    {
        let s = stats();
        let _ = s.reports_sent; // Verify counter is accessible and u64-typed.
        let _ = s.leaves_sent;

        passed = passed.saturating_add(1);
        crate::serial_println!("[igmp]   test 6 (stats accessible) PASSED");
    }

    // --- Test 7: Procfs content ---
    {
        let content = procfs_content();
        assert!(content.contains("IGMP"), "header");
        assert!(content.contains("Active groups:"), "active groups");
        assert!(content.contains("Reports sent:"), "reports sent");
        assert!(content.contains("Queries received:"), "queries");

        passed = passed.saturating_add(1);
        crate::serial_println!("[igmp]   test 7 (procfs content) PASSED");
    }

    // --- Test 8: Leave packet type ---
    {
        let pkt = build_leave(Ipv4Addr::new(224, 0, 0, 251));
        assert!(pkt[0] == 0x17, "leave type byte");
        // Group bytes.
        assert!(pkt[4] == 224, "g0");
        assert!(pkt[5] == 0, "g1");
        assert!(pkt[6] == 0, "g2");
        assert!(pkt[7] == 251, "g3");

        passed = passed.saturating_add(1);
        crate::serial_println!("[igmp]   test 8 (leave packet type) PASSED");
    }

    // --- Test 9: Message format lengths ---
    {
        let r = build_report(Ipv4Addr::new(239, 1, 2, 3));
        let l = build_leave(Ipv4Addr::new(239, 1, 2, 3));
        assert!(r.len() == 8, "report 8 bytes");
        assert!(l.len() == 8, "leave 8 bytes");

        passed = passed.saturating_add(1);
        crate::serial_println!("[igmp]   test 9 (message format) PASSED");
    }

    // --- Test 10: Group list ---
    {
        let (_, count) = list_groups();
        // Just verify it doesn't panic and returns valid data.
        assert!(count <= MAX_GROUPS, "count within bounds");

        passed = passed.saturating_add(1);
        crate::serial_println!("[igmp]   test 10 (group list) PASSED");
    }

    crate::serial_println!("[igmp] All {} self-tests PASSED", passed);
    Ok(())
}
