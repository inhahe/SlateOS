//! MLD — Multicast Listener Discovery for IPv6.
//!
//! Implements MLDv1 (RFC 2710) and MLDv2 report format (RFC 3810)
//! for IPv6 multicast group membership management.  This is the IPv6
//! counterpart of IGMP (IPv4).
//!
//! ## Protocol overview
//!
//! MLD runs over ICMPv6 (Next Header 58) with these message types:
//!
//! ```text
//! ICMPv6 Type  Name                      RFC
//! ───────────  ────────────────────────  ────
//! 130          Multicast Listener Query  2710
//! 131          MLDv1 Report              2710
//! 132          MLDv1 Done (Leave)        2710
//! 143          MLDv2 Report              3810
//! ```
//!
//! ## Integration
//!
//! - When `udp::join_group_v6()` is called, this module sends an
//!   MLDv1 Multicast Listener Report to the group address.
//! - When `udp::leave_group_v6()` is called and refcount drops to 0,
//!   this module sends an MLDv1 Done message to ff02::2 (all-routers).
//! - When a Multicast Listener Query is received, this module
//!   schedules and sends reports for all active groups.
//! - Periodic unsolicited reports are sent for active groups.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::KernelResult;
use super::ipv6::{self, Ipv6Addr, Ipv6Packet, NH_ICMPV6};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// ICMPv6 type: Multicast Listener Query (MLDv1/v2).
const MLD_QUERY: u8 = 130;

/// ICMPv6 type: MLDv1 Multicast Listener Report.
const MLD_V1_REPORT: u8 = 131;

/// ICMPv6 type: MLDv1 Done (equivalent of IGMP Leave).
const MLD_V1_DONE: u8 = 132;

/// ICMPv6 type: MLDv2 Multicast Listener Report.
#[allow(dead_code)] // Protocol constant — we accept but send v1 reports.
const MLD_V2_REPORT: u8 = 143;

/// Minimum MLD message size (MLDv1: 24 bytes).
///
/// ```text
/// ┌──────────┬──────┬──────────────┐
/// │ Type (8) │Code  │ Checksum(16) │  (ICMPv6 header: 4 bytes)
/// ├──────────┴──────┴──────────────┤
/// │ Maximum Response Delay (16)    │
/// ├────────────────────────────────┤
/// │ Reserved (16)                  │
/// ├────────────────────────────────┤
/// │ Multicast Address (128)        │
/// └────────────────────────────────┘
/// ```
const MLD_V1_MSG_SIZE: usize = 24;

/// All-routers link-local multicast (ff02::2).
/// Done messages are sent here.
const ALL_ROUTERS_V6: Ipv6Addr = Ipv6Addr([
    0xFF, 0x02, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0x02,
]);

/// MLDv2 all-capable-routers (ff02::16).
/// MLDv2 reports are sent here (we note it but send v1 reports).
#[allow(dead_code)] // Protocol constant for future MLDv2 sending.
const MLDV2_ALL_ROUTERS: Ipv6Addr = Ipv6Addr([
    0xFF, 0x02, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0x16,
]);

/// Maximum multicast groups we track for MLD.
const MAX_GROUPS: usize = 32;

/// Default unsolicited report interval (ns).
/// RFC 2710 §7.10 recommends 10 seconds.
const UNSOLICITED_REPORT_INTERVAL_NS: u64 = 10_000_000_000;

/// Timer interval (ns) between periodic tick checks.
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
    DelayingListener,
    /// Stable listener (report already sent, no pending query).
    IdleListener,
}

/// A single tracked multicast group.
#[derive(Debug, Clone, Copy)]
struct GroupEntry {
    /// Multicast group address.
    addr: Ipv6Addr,
    /// Current state.
    state: GroupState,
    /// When to send the next report (monotonic ns).
    report_at_ns: u64,
    /// Last time an unsolicited report was sent.
    last_report_ns: u64,
}

impl GroupEntry {
    const fn empty() -> Self {
        Self {
            addr: Ipv6Addr::UNSPECIFIED,
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
static DONES_SENT: AtomicU64 = AtomicU64::new(0);
static QUERIES_RECEIVED: AtomicU64 = AtomicU64::new(0);
static ERRORS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Packet building
// ---------------------------------------------------------------------------

/// Build an MLDv1 Multicast Listener Report message.
///
/// Format (24 bytes):
/// - Type (130 for Query, 131 for Report, 132 for Done)
/// - Code = 0
/// - Checksum (filled later)
/// - Maximum Response Delay (0 for reports)
/// - Reserved (0)
/// - Multicast Address (16 bytes)
fn build_report(group: Ipv6Addr) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(MLD_V1_MSG_SIZE);

    pkt.push(MLD_V1_REPORT); // Type.
    pkt.push(0);              // Code.
    pkt.extend_from_slice(&[0, 0]); // Checksum placeholder.
    pkt.extend_from_slice(&[0, 0]); // Maximum Response Delay (0 for report).
    pkt.extend_from_slice(&[0, 0]); // Reserved.
    pkt.extend_from_slice(&group.0); // Multicast Address.

    pkt
}

/// Build an MLDv1 Done message (equivalent to IGMP Leave Group).
fn build_done(group: Ipv6Addr) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(MLD_V1_MSG_SIZE);

    pkt.push(MLD_V1_DONE);   // Type.
    pkt.push(0);              // Code.
    pkt.extend_from_slice(&[0, 0]); // Checksum placeholder.
    pkt.extend_from_slice(&[0, 0]); // Maximum Response Delay (0 for done).
    pkt.extend_from_slice(&[0, 0]); // Reserved.
    pkt.extend_from_slice(&group.0); // Multicast Address.

    pkt
}

/// Compute ICMPv6 checksum for an MLD message and write it into bytes 2-3.
fn finalize_mld_checksum(src: &Ipv6Addr, dst: &Ipv6Addr, mut msg: Vec<u8>) -> Vec<u8> {
    // Zero the checksum field before computing.
    if msg.len() >= 4 {
        msg[2] = 0;
        msg[3] = 0;
    }
    let cksum = ipv6::compute_transport_checksum(src, dst, NH_ICMPV6, &msg);
    if msg.len() >= 4 {
        msg[2] = (cksum >> 8) as u8;
        msg[3] = cksum as u8;
    }
    msg
}

// ---------------------------------------------------------------------------
// Sending
// ---------------------------------------------------------------------------

/// Send an MLDv1 Multicast Listener Report for a group.
///
/// Per RFC 2710 §4, the report is sent to the multicast group address
/// itself, with hop limit 1 (link-local scope).
fn send_report(group: Ipv6Addr) -> KernelResult<()> {
    let our_mac = super::interface::mac();
    let our_ip = super::icmpv6::slaac_global_addr()
        .unwrap_or_else(|| Ipv6Addr::from_mac_link_local(&our_mac));

    let pkt = build_report(group);
    let pkt = finalize_mld_checksum(&our_ip, &group, pkt);

    // MLD reports use hop limit 1 (link-local scope).
    ipv6::send_raw(our_ip, group, NH_ICMPV6, 1, &pkt)?;
    REPORTS_SENT.fetch_add(1, Ordering::Relaxed);
    crate::serial_println!("[mld] Sent listener report for {}", group);
    Ok(())
}

/// Send an MLDv1 Done message.
///
/// Done messages are sent to ff02::2 (all-routers), per RFC 2710 §4.
fn send_done(group: Ipv6Addr) -> KernelResult<()> {
    let our_mac = super::interface::mac();
    let our_ip = super::icmpv6::slaac_global_addr()
        .unwrap_or_else(|| Ipv6Addr::from_mac_link_local(&our_mac));

    let pkt = build_done(group);
    let pkt = finalize_mld_checksum(&our_ip, &ALL_ROUTERS_V6, pkt);

    ipv6::send_raw(our_ip, ALL_ROUTERS_V6, NH_ICMPV6, 1, &pkt)?;
    DONES_SENT.fetch_add(1, Ordering::Relaxed);
    crate::serial_println!("[mld] Sent done for {}", group);
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API — called from udp module
// ---------------------------------------------------------------------------

/// Notify MLD that we've joined a multicast group.
///
/// Sends an unsolicited Multicast Listener Report and adds the group
/// to the tracking table.
pub fn join(group: Ipv6Addr) {
    if !group.is_multicast() {
        return;
    }

    let now = crate::hrtimer::now_ns();
    let mut groups = GROUPS.lock();

    // Check if already tracked.
    for entry in groups.iter() {
        if entry.state != GroupState::Idle && entry.addr == group {
            // Already a listener — just send another report.
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
                state: GroupState::IdleListener,
                report_at_ns: 0,
                last_report_ns: now,
            };
            drop(groups);

            // Send initial unsolicited report (RFC 2710 §6).
            let _ = send_report(group);
            return;
        }
    }

    // Table full — still send the report, just don't track.
    drop(groups);
    ERRORS.fetch_add(1, Ordering::Relaxed);
    let _ = send_report(group);
}

/// Notify MLD that we've left a multicast group.
///
/// Sends a Done message and removes the group from tracking.
pub fn leave(group: Ipv6Addr) {
    if !group.is_multicast() {
        return;
    }

    let mut groups = GROUPS.lock();
    for entry in groups.iter_mut() {
        if entry.state != GroupState::Idle && entry.addr == group {
            entry.state = GroupState::Idle;
            entry.addr = Ipv6Addr::UNSPECIFIED;
            drop(groups);

            // Send Done (RFC 2710 §6).
            let _ = send_done(group);
            return;
        }
    }
    // Not tracked — nothing to do.
}

// ---------------------------------------------------------------------------
// Incoming MLD processing
// ---------------------------------------------------------------------------

/// Process an incoming MLD message.
///
/// Called from the ICMPv6 layer when type is 130, 131, 132, or 143.
pub fn process(ip_packet: &Ipv6Packet<'_>, data: &[u8]) -> KernelResult<()> {
    if data.len() < MLD_V1_MSG_SIZE {
        return Ok(());
    }

    // Verify ICMPv6 checksum.
    if !ipv6::verify_transport_checksum(&ip_packet.src, &ip_packet.dst, NH_ICMPV6, data) {
        crate::serial_println!("[mld] Dropped — bad checksum");
        return Ok(());
    }

    let msg_type = data[0];

    match msg_type {
        MLD_QUERY => {
            QUERIES_RECEIVED.fetch_add(1, Ordering::Relaxed);

            // Extract Maximum Response Delay (bytes 4-5, in milliseconds).
            let max_resp_ms = u16::from_be_bytes([
                *data.get(4).unwrap_or(&0),
                *data.get(5).unwrap_or(&0),
            ]);

            // Extract multicast address (bytes 8-23).
            let mut group_bytes = [0u8; 16];
            if data.len() >= 24 {
                group_bytes.copy_from_slice(&data[8..24]);
            }
            let group_addr = Ipv6Addr(group_bytes);

            if group_addr.is_unspecified() {
                // General Query — respond for all groups.
                handle_general_query(max_resp_ms);
            } else {
                // Multicast-Address-Specific Query.
                handle_specific_query(group_addr, max_resp_ms);
            }
        }
        MLD_V1_REPORT => {
            // Another listener on the network joined this group.
            // Suppress our own pending report (RFC 2710 §5).
            if data.len() >= 24 {
                let mut group_bytes = [0u8; 16];
                group_bytes.copy_from_slice(&data[8..24]);
                let group_addr = Ipv6Addr(group_bytes);
                suppress_report(group_addr);
            }
        }
        MLD_V1_DONE => {
            // Another listener left — informational.
            crate::serial_println!("[mld] Done from {} for a group", ip_packet.src);
        }
        MLD_V2_REPORT => {
            // MLDv2 Report from another host — informational.
            // We accept but don't need to act on it.
            crate::serial_println!("[mld] MLDv2 report from {}", ip_packet.src);
        }
        _ => {
            // Not an MLD message type we handle.
        }
    }

    Ok(())
}

/// Handle a General Query: schedule reports for all active groups.
fn handle_general_query(max_resp_ms: u16) {
    let now = crate::hrtimer::now_ns();
    // Convert max response delay from milliseconds to nanoseconds.
    let max_delay_ns = (max_resp_ms as u64).saturating_mul(1_000_000);
    // Use half the max delay as our response time (simple deterministic
    // approach — a full implementation would use random delay per RFC 2710).
    let delay_ns = max_delay_ns / 2;

    let mut groups = GROUPS.lock();
    for entry in groups.iter_mut() {
        if entry.state != GroupState::Idle {
            entry.state = GroupState::DelayingListener;
            entry.report_at_ns = now.saturating_add(delay_ns);
        }
    }

    crate::serial_println!(
        "[mld] General query (max_resp={}ms), scheduling reports",
        max_resp_ms
    );
}

/// Handle a Multicast-Address-Specific Query: schedule report for one group.
fn handle_specific_query(group: Ipv6Addr, max_resp_ms: u16) {
    let now = crate::hrtimer::now_ns();
    let max_delay_ns = (max_resp_ms as u64).saturating_mul(1_000_000);
    let delay_ns = max_delay_ns / 2;

    let mut groups = GROUPS.lock();
    for entry in groups.iter_mut() {
        if entry.state != GroupState::Idle && entry.addr == group {
            entry.state = GroupState::DelayingListener;
            entry.report_at_ns = now.saturating_add(delay_ns);
            break;
        }
    }

    crate::serial_println!(
        "[mld] Specific query for {} (max_resp={}ms)",
        group, max_resp_ms
    );
}

/// Suppress a pending report for a group (RFC 2710 report suppression).
///
/// If another listener on the network sends a report for the same group,
/// we cancel our pending report to avoid duplicates on the link.
fn suppress_report(group: Ipv6Addr) {
    let mut groups = GROUPS.lock();
    for entry in groups.iter_mut() {
        if entry.state == GroupState::DelayingListener && entry.addr == group {
            entry.state = GroupState::IdleListener;
            crate::serial_println!("[mld] Suppressed report for {} (heard from peer)", group);
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Periodic tick
// ---------------------------------------------------------------------------

/// Periodic MLD maintenance.
///
/// - Sends delayed reports for groups in `DelayingListener` state.
/// - Sends periodic unsolicited reports for all active groups.
///
/// Called from `net::poll()` via the maintenance tick.
pub fn tick() {
    let now = crate::hrtimer::now_ns();
    let last = LAST_TICK_NS.load(Ordering::Relaxed);
    if now.saturating_sub(last) < TICK_INTERVAL_NS {
        return;
    }
    LAST_TICK_NS.store(now, Ordering::Relaxed);

    // Collect groups that need reports (to avoid holding lock while sending).
    let mut to_report: [Ipv6Addr; MAX_GROUPS] = [Ipv6Addr::UNSPECIFIED; MAX_GROUPS];
    let mut report_count = 0usize;

    {
        let mut groups = GROUPS.lock();
        for entry in groups.iter_mut() {
            match entry.state {
                GroupState::DelayingListener => {
                    if now >= entry.report_at_ns {
                        // Timer expired — send report.
                        if report_count < MAX_GROUPS {
                            to_report[report_count] = entry.addr;
                            report_count = report_count.saturating_add(1);
                        }
                        entry.state = GroupState::IdleListener;
                        entry.last_report_ns = now;
                    }
                }
                GroupState::IdleListener => {
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

/// MLD statistics.
#[derive(Debug)]
pub struct MldStats {
    pub active_groups: usize,
    pub reports_sent: u64,
    pub dones_sent: u64,
    pub queries_received: u64,
    pub errors: u64,
}

/// Get MLD statistics.
pub fn stats() -> MldStats {
    let groups = GROUPS.lock();
    let active = groups.iter().filter(|e| e.state != GroupState::Idle).count();
    MldStats {
        active_groups: active,
        reports_sent: REPORTS_SENT.load(Ordering::Relaxed),
        dones_sent: DONES_SENT.load(Ordering::Relaxed),
        queries_received: QUERIES_RECEIVED.load(Ordering::Relaxed),
        errors: ERRORS.load(Ordering::Relaxed),
    }
}

/// Information about a tracked multicast group (for display).
#[derive(Debug)]
pub struct GroupInfo {
    pub addr: Ipv6Addr,
    pub state: &'static str,
}

/// List all active MLD group memberships.
pub fn list_groups() -> (Vec<GroupInfo>, usize) {
    let groups = GROUPS.lock();
    let mut infos = Vec::new();
    for entry in groups.iter() {
        if entry.state != GroupState::Idle {
            infos.push(GroupInfo {
                addr: entry.addr,
                state: match entry.state {
                    GroupState::Idle => "idle",
                    GroupState::DelayingListener => "delaying",
                    GroupState::IdleListener => "listener",
                },
            });
        }
    }
    let count = infos.len();
    (infos, count)
}

/// Generate procfs content for `/proc/mld`.
pub fn procfs_content() -> String {
    let s = stats();
    let (groups, count) = list_groups();

    let mut out = String::with_capacity(512);
    out.push_str("MLD (Multicast Listener Discovery)\n");
    out.push_str("===================================\n\n");
    out.push_str(&format!("Active groups:    {}\n", s.active_groups));
    out.push_str(&format!("Reports sent:     {}\n", s.reports_sent));
    out.push_str(&format!("Dones sent:       {}\n", s.dones_sent));
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

/// Run MLD self-tests.
// Self-tests deliberately runtime-assert RFC-defined constants
// (ICMPv6 message types, protocol numbers) as living documentation.
#[allow(clippy::assertions_on_constants)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[mld] Running MLD self-tests...");
    let mut passed = 0u32;

    // --- Test 1: Report construction ---
    {
        let group = Ipv6Addr([
            0xFF, 0x02, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0xFB,
        ]); // ff02::fb (mDNS)
        let pkt = build_report(group);
        assert!(pkt.len() == MLD_V1_MSG_SIZE, "report size");
        assert!(pkt[0] == MLD_V1_REPORT, "report type");
        assert!(pkt[1] == 0, "code");
        // Max Response Delay should be 0.
        assert!(pkt[4] == 0, "max resp hi");
        assert!(pkt[5] == 0, "max resp lo");
        // Group address at bytes 8-23.
        assert!(pkt[8] == 0xFF, "group byte 0");
        assert!(pkt[9] == 0x02, "group byte 1");
        assert!(pkt[23] == 0xFB, "group last byte");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mld]   test 1 (report construction) PASSED");
    }

    // --- Test 2: Done construction ---
    {
        let group = Ipv6Addr([
            0xFF, 0x02, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0xFB,
        ]);
        let pkt = build_done(group);
        assert!(pkt.len() == MLD_V1_MSG_SIZE, "done size");
        assert!(pkt[0] == MLD_V1_DONE, "done type");
        assert!(pkt[1] == 0, "code");
        // Group address at bytes 8-23.
        assert!(pkt[8] == 0xFF, "group byte 0");
        assert!(pkt[23] == 0xFB, "group last byte");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mld]   test 2 (done construction) PASSED");
    }

    // --- Test 3: Constants ---
    {
        assert!(MLD_QUERY == 130, "query type");
        assert!(MLD_V1_REPORT == 131, "report type");
        assert!(MLD_V1_DONE == 132, "done type");
        assert!(MLD_V2_REPORT == 143, "v2 report type");
        assert!(MLD_V1_MSG_SIZE == 24, "message size");

        // Verify well-known multicast addresses.
        assert!(ALL_ROUTERS_V6.0[0] == 0xFF, "all-routers ff");
        assert!(ALL_ROUTERS_V6.0[1] == 0x02, "all-routers scope");
        assert!(ALL_ROUTERS_V6.0[15] == 0x02, "all-routers last");

        assert!(MLDV2_ALL_ROUTERS.0[15] == 0x16, "mldv2 routers last");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mld]   test 3 (constants) PASSED");
    }

    // --- Test 4: GroupEntry default ---
    {
        let entry = GroupEntry::empty();
        assert!(entry.state == GroupState::Idle, "default state");
        assert!(entry.addr == Ipv6Addr::UNSPECIFIED, "default addr");
        assert!(entry.report_at_ns == 0, "default report_at");
        assert!(entry.last_report_ns == 0, "default last_report");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mld]   test 4 (GroupEntry default) PASSED");
    }

    // --- Test 5: Checksum computation ---
    {
        let src = Ipv6Addr([
            0xFE, 0x80, 0, 0, 0, 0, 0, 0,
            0x02, 0x01, 0x02, 0xFF, 0xFE, 0x03, 0x04, 0x05,
        ]);
        let group = Ipv6Addr([
            0xFF, 0x02, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0xFB,
        ]);
        let pkt = build_report(group);
        let pkt = finalize_mld_checksum(&src, &group, pkt);
        // Verify the checksum field is non-zero (was computed).
        let cksum_val = u16::from_be_bytes([pkt[2], pkt[3]]);
        assert!(cksum_val != 0, "checksum should be non-zero");
        // Verify the checksum is valid (re-computing over the whole packet
        // with checksum included should yield 0).
        assert!(
            ipv6::verify_transport_checksum(&src, &group, NH_ICMPV6, &pkt),
            "checksum verification"
        );

        passed = passed.saturating_add(1);
        crate::serial_println!("[mld]   test 5 (checksum computation) PASSED");
    }

    // --- Test 6: Report vs Done have different type bytes ---
    {
        let group = Ipv6Addr([
            0xFF, 0x02, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0x01,
        ]);
        let report = build_report(group);
        let done = build_done(group);
        assert!(report[0] != done[0], "different type bytes");
        assert!(report[0] == MLD_V1_REPORT, "report is 131");
        assert!(done[0] == MLD_V1_DONE, "done is 132");
        // But the group address part should be identical.
        assert!(report[8..24] == done[8..24], "same group addr");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mld]   test 6 (report vs done types) PASSED");
    }

    // --- Test 7: Stats accessible ---
    {
        let s = stats();
        let _ = s.reports_sent;
        let _ = s.dones_sent;
        let _ = s.queries_received;
        assert!(s.active_groups <= MAX_GROUPS, "active within bounds");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mld]   test 7 (stats accessible) PASSED");
    }

    // --- Test 8: Procfs content ---
    {
        let content = procfs_content();
        assert!(content.contains("MLD"), "header");
        assert!(content.contains("Active groups:"), "active groups");
        assert!(content.contains("Reports sent:"), "reports sent");
        assert!(content.contains("Queries received:"), "queries");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mld]   test 8 (procfs content) PASSED");
    }

    // --- Test 9: Group list ---
    {
        let (_, count) = list_groups();
        assert!(count <= MAX_GROUPS, "count within bounds");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mld]   test 9 (group list) PASSED");
    }

    // --- Test 10: Message format lengths ---
    {
        let group = Ipv6Addr([
            0xFF, 0x05, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0x01, 0x00,
        ]);
        let r = build_report(group);
        let d = build_done(group);
        assert!(r.len() == 24, "report 24 bytes");
        assert!(d.len() == 24, "done 24 bytes");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mld]   test 10 (message format) PASSED");
    }

    crate::serial_println!("[mld] All {} self-tests PASSED", passed);
    Ok(())
}
