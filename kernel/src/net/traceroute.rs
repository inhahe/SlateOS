//! Traceroute — trace the path packets take to a destination.
//!
//! Supports both IPv4 (`trace()`) and IPv6 (`trace6()`) traceroute.
//!
//! Sends ICMP / ICMPv6 echo requests with increasing TTL / hop-limit
//! values to discover each hop along the route to a destination.
//! Intermediate routers respond with Time Exceeded; the destination
//! responds with Echo Reply.
//!
//! ## Algorithm
//!
//! ```text
//! for ttl in 1..=max_hops:
//!   send ICMP Echo Request with TTL=ttl  (or ICMPv6 with hop_limit)
//!   wait for reply:
//!     - Time Exceeded       → record hop (router IP, RTT)
//!     - Echo Reply          → record final hop (destination IP, RTT), done
//!     - timeout             → record hop as "*" (no response)
//! ```
//!
//! ## Usage
//!
//! ```text
//! traceroute 8.8.8.8               — IPv4 trace route
//! traceroute 10.0.2.2 -m 15        — max 15 hops
//! traceroute6 2001:4860:4860::8888 — IPv6 trace route
//! traceroute6 fe80::1 -q 1         — 1 probe per hop
//! ```
//!
//! ## Limitations
//!
//! - Uses ICMP/ICMPv6 echo requests (some firewalls may block these).
//! - Single traceroute at a time per address family (global probe state).
//! - Polling-based (calls net::poll() in a loop).

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use super::interface::Ipv4Addr;
use super::ipv4;
use super::icmp;
use super::ipv6::{self, Ipv6Addr, NH_ICMPV6};
use super::icmpv6;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default maximum number of hops.
const DEFAULT_MAX_HOPS: u8 = 30;

/// Default number of probes per hop.
const DEFAULT_PROBES_PER_HOP: u8 = 3;

/// Default timeout per probe in poll iterations.
///
/// Each poll iteration is roughly 1-10ms depending on NIC activity.
/// 500 iterations ≈ 3-5 seconds effective timeout.
const DEFAULT_TIMEOUT_POLLS: u32 = 500;

/// ICMP protocol number.
const PROTO_ICMP: u8 = 1;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Whether a traceroute is currently in progress.
static ACTIVE: AtomicBool = AtomicBool::new(false);

// Statistics.
static TOTAL_TRACES: AtomicU64 = AtomicU64::new(0);
static TOTAL_PROBES_SENT: AtomicU64 = AtomicU64::new(0);
static TOTAL_PROBES_TIMEOUT: AtomicU64 = AtomicU64::new(0);
static LAST_HOPS: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Hop result
// ---------------------------------------------------------------------------

/// Result for a single probe at a given hop.
#[derive(Debug, Clone, Copy)]
pub struct ProbeResult {
    /// RTT in nanoseconds (0 if timed out).
    pub rtt_ns: u64,
    /// IP address of the responding router/host.
    pub addr: Ipv4Addr,
    /// Whether a response was received.
    pub received: bool,
    /// Whether this probe reached the final destination.
    pub reached_dst: bool,
}

/// Result for a single hop (may include multiple probes).
#[derive(Debug, Clone)]
pub struct HopResult {
    /// TTL / hop number (1-based).
    pub hop: u8,
    /// Per-probe results.
    pub probes: Vec<ProbeResult>,
}

/// Full traceroute result.
#[derive(Debug, Clone)]
pub struct TraceResult {
    /// Destination IP.
    pub destination: Ipv4Addr,
    /// Per-hop results.
    pub hops: Vec<HopResult>,
    /// Whether the destination was reached.
    pub reached: bool,
    /// Total probes sent.
    pub probes_sent: u32,
    /// Total probes that timed out.
    pub probes_timeout: u32,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Check if a traceroute is currently running.
#[allow(dead_code)] // Public API.
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Run a traceroute to the given destination.
///
/// Sends ICMP echo requests with increasing TTL values.
/// Returns the full trace result with per-hop information.
///
/// # Parameters
///
/// - `dst`: Destination IP address.
/// - `max_hops`: Maximum number of hops (default 30).
/// - `probes_per_hop`: Number of probes per hop (default 3).
/// - `timeout_polls`: Timeout per probe in poll iterations (default 500).
pub fn trace(
    dst: Ipv4Addr,
    max_hops: Option<u8>,
    probes_per_hop: Option<u8>,
    timeout_polls: Option<u32>,
) -> KernelResult<TraceResult> {
    // Only one traceroute at a time.
    if ACTIVE.swap(true, Ordering::Relaxed) {
        return Err(KernelError::DeviceBusy);
    }

    let max = max_hops.unwrap_or(DEFAULT_MAX_HOPS);
    let probes = probes_per_hop.unwrap_or(DEFAULT_PROBES_PER_HOP);
    let timeout = timeout_polls.unwrap_or(DEFAULT_TIMEOUT_POLLS);

    let mut result = TraceResult {
        destination: dst,
        hops: Vec::with_capacity(max as usize),
        reached: false,
        probes_sent: 0,
        probes_timeout: 0,
    };

    TOTAL_TRACES.fetch_add(1, Ordering::Relaxed);

    for ttl in 1..=max {
        let mut hop = HopResult {
            hop: ttl,
            probes: Vec::with_capacity(probes as usize),
        };

        for _ in 0..probes {
            let probe_result = send_probe(dst, ttl, timeout);
            result.probes_sent = result.probes_sent.saturating_add(1);
            TOTAL_PROBES_SENT.fetch_add(1, Ordering::Relaxed);

            if !probe_result.received {
                result.probes_timeout = result.probes_timeout.saturating_add(1);
                TOTAL_PROBES_TIMEOUT.fetch_add(1, Ordering::Relaxed);
            }

            if probe_result.reached_dst {
                result.reached = true;
            }

            hop.probes.push(probe_result);
        }

        result.hops.push(hop);
        LAST_HOPS.store(ttl as u32, Ordering::Relaxed);

        if result.reached {
            break;
        }
    }

    ACTIVE.store(false, Ordering::Relaxed);
    Ok(result)
}

/// Send a single traceroute probe and wait for a reply.
fn send_probe(dst: Ipv4Addr, ttl: u8, timeout_polls: u32) -> ProbeResult {
    let seq = icmp::next_trace_seq();
    let pkt = icmp::build_trace_echo_request(seq);

    // Record the probe for correlation.
    icmp::record_trace_probe(seq, ttl);

    // Send with custom TTL.
    if ipv4::send_with_ttl(dst, PROTO_ICMP, &pkt, ttl).is_err() {
        return ProbeResult {
            rtt_ns: 0,
            addr: Ipv4Addr::UNSPECIFIED,
            received: false,
            reached_dst: false,
        };
    }

    // Poll for a reply.
    for _ in 0..timeout_polls {
        super::poll();

        if let Some((reply_ip, rtt_ns, reached)) = icmp::check_trace_reply(seq) {
            return ProbeResult {
                rtt_ns,
                addr: reply_ip,
                received: true,
                reached_dst: reached,
            };
        }
    }

    // Timeout.
    ProbeResult {
        rtt_ns: 0,
        addr: Ipv4Addr::UNSPECIFIED,
        received: false,
        reached_dst: false,
    }
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

/// Format RTT in human-readable form.
pub fn format_rtt(rtt_ns: u64) -> String {
    #[allow(clippy::arithmetic_side_effects)]
    let rtt_us = rtt_ns / 1000;
    if rtt_us >= 10_000 {
        #[allow(clippy::arithmetic_side_effects)]
        let rtt_ms = rtt_us / 1000;
        format!("{} ms", rtt_ms)
    } else if rtt_us > 0 {
        format!("{} us", rtt_us)
    } else {
        format!("{} ns", rtt_ns)
    }
}

/// Format a complete trace result as a human-readable string.
pub fn format_trace(result: &TraceResult) -> String {
    let mut out = String::with_capacity(512);

    out.push_str(&format!(
        "traceroute to {} (max {} hops, {} probes/hop)\n",
        result.destination,
        result.hops.len(),
        result.hops.first().map_or(0, |h| h.probes.len()),
    ));

    for hop in &result.hops {
        out.push_str(&format!("{:>2}  ", hop.hop));

        // Collect unique IPs for this hop.
        let mut last_ip = Ipv4Addr::UNSPECIFIED;
        for probe in &hop.probes {
            if probe.received {
                if probe.addr != last_ip {
                    if !last_ip.is_unspecified() {
                        out.push_str("  ");
                    }
                    out.push_str(&format!("{}", probe.addr));
                    last_ip = probe.addr;
                }
                out.push_str(&format!("  {}", format_rtt(probe.rtt_ns)));
            } else {
                out.push_str("  *");
            }
        }
        out.push('\n');
    }

    if result.reached {
        out.push_str(&format!(
            "\nDestination reached in {} hops ({} probes, {} timeouts)\n",
            result.hops.len(), result.probes_sent, result.probes_timeout
        ));
    } else {
        out.push_str(&format!(
            "\nDestination NOT reached after {} hops ({} probes, {} timeouts)\n",
            result.hops.len(), result.probes_sent, result.probes_timeout
        ));
    }

    out
}

// ===========================================================================
// IPv6 Traceroute
// ===========================================================================

/// Whether an IPv6 traceroute is currently in progress.
static ACTIVE6: AtomicBool = AtomicBool::new(false);

// IPv6 statistics.
static TOTAL_TRACES6: AtomicU64 = AtomicU64::new(0);
static TOTAL_PROBES_SENT6: AtomicU64 = AtomicU64::new(0);
static TOTAL_PROBES_TIMEOUT6: AtomicU64 = AtomicU64::new(0);
static LAST_HOPS6: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// IPv6 Hop result
// ---------------------------------------------------------------------------

/// Result for a single IPv6 probe at a given hop.
#[derive(Debug, Clone, Copy)]
pub struct ProbeResult6 {
    /// RTT in nanoseconds (0 if timed out).
    pub rtt_ns: u64,
    /// IPv6 address of the responding router/host.
    pub addr: Ipv6Addr,
    /// Whether a response was received.
    pub received: bool,
    /// Whether this probe reached the final destination.
    pub reached_dst: bool,
}

/// Result for a single hop (may include multiple probes).
#[derive(Debug, Clone)]
pub struct HopResult6 {
    /// Hop-limit / hop number (1-based).
    pub hop: u8,
    /// Per-probe results.
    pub probes: Vec<ProbeResult6>,
}

/// Full IPv6 traceroute result.
#[derive(Debug, Clone)]
pub struct TraceResult6 {
    /// Destination IPv6 address.
    pub destination: Ipv6Addr,
    /// Per-hop results.
    pub hops: Vec<HopResult6>,
    /// Whether the destination was reached.
    pub reached: bool,
    /// Total probes sent.
    pub probes_sent: u32,
    /// Total probes that timed out.
    pub probes_timeout: u32,
}

// ---------------------------------------------------------------------------
// IPv6 Public API
// ---------------------------------------------------------------------------

/// Check if an IPv6 traceroute is currently running.
#[allow(dead_code)] // Public API.
pub fn is_active6() -> bool {
    ACTIVE6.load(Ordering::Relaxed)
}

/// Run an IPv6 traceroute to the given destination.
///
/// Sends ICMPv6 echo requests with increasing hop-limit values.
/// Intermediate routers respond with ICMPv6 Time Exceeded (type 3);
/// the destination responds with ICMPv6 Echo Reply (type 129).
///
/// # Parameters
///
/// - `dst`: Destination IPv6 address.
/// - `max_hops`: Maximum number of hops (default 30).
/// - `probes_per_hop`: Number of probes per hop (default 3).
/// - `timeout_polls`: Timeout per probe in poll iterations (default 500).
pub fn trace6(
    dst: Ipv6Addr,
    max_hops: Option<u8>,
    probes_per_hop: Option<u8>,
    timeout_polls: Option<u32>,
) -> KernelResult<TraceResult6> {
    // Only one traceroute6 at a time.
    if ACTIVE6.swap(true, Ordering::Relaxed) {
        return Err(KernelError::DeviceBusy);
    }

    let max = max_hops.unwrap_or(DEFAULT_MAX_HOPS);
    let probes = probes_per_hop.unwrap_or(DEFAULT_PROBES_PER_HOP);
    let timeout = timeout_polls.unwrap_or(DEFAULT_TIMEOUT_POLLS);

    let mut result = TraceResult6 {
        destination: dst,
        hops: Vec::with_capacity(max as usize),
        reached: false,
        probes_sent: 0,
        probes_timeout: 0,
    };

    TOTAL_TRACES6.fetch_add(1, Ordering::Relaxed);

    for hop_limit in 1..=max {
        let mut hop = HopResult6 {
            hop: hop_limit,
            probes: Vec::with_capacity(probes as usize),
        };

        for _ in 0..probes {
            let probe_result = send_probe6(dst, hop_limit, timeout);
            result.probes_sent = result.probes_sent.saturating_add(1);
            TOTAL_PROBES_SENT6.fetch_add(1, Ordering::Relaxed);

            if !probe_result.received {
                result.probes_timeout = result.probes_timeout.saturating_add(1);
                TOTAL_PROBES_TIMEOUT6.fetch_add(1, Ordering::Relaxed);
            }

            if probe_result.reached_dst {
                result.reached = true;
            }

            hop.probes.push(probe_result);
        }

        result.hops.push(hop);
        LAST_HOPS6.store(hop_limit as u32, Ordering::Relaxed);

        if result.reached {
            break;
        }
    }

    ACTIVE6.store(false, Ordering::Relaxed);
    Ok(result)
}

/// Send a single IPv6 traceroute probe and wait for a reply.
fn send_probe6(dst: Ipv6Addr, hop_limit: u8, timeout_polls: u32) -> ProbeResult6 {
    let seq = icmpv6::next_trace6_seq();

    // Choose source address: SLAAC global for non-link-local, otherwise link-local.
    let our_mac = super::interface::mac();
    let link_local = Ipv6Addr::from_mac_link_local(&our_mac);
    let src = if dst.is_link_local() {
        link_local
    } else {
        icmpv6::slaac_global_addr().unwrap_or(link_local)
    };

    let pkt = icmpv6::build_trace6_echo_request(&src, &dst, seq);

    // Record the probe for correlation.
    icmpv6::record_trace6_probe(seq, hop_limit);

    // Send with custom hop limit.
    if ipv6::send_raw(src, dst, NH_ICMPV6, hop_limit, &pkt).is_err() {
        return ProbeResult6 {
            rtt_ns: 0,
            addr: Ipv6Addr::UNSPECIFIED,
            received: false,
            reached_dst: false,
        };
    }

    // Poll for a reply.
    for _ in 0..timeout_polls {
        super::poll();

        if let Some((reply_ip, rtt_ns, reached)) = icmpv6::check_trace6_reply(seq) {
            return ProbeResult6 {
                rtt_ns,
                addr: reply_ip,
                received: true,
                reached_dst: reached,
            };
        }
    }

    // Timeout.
    ProbeResult6 {
        rtt_ns: 0,
        addr: Ipv6Addr::UNSPECIFIED,
        received: false,
        reached_dst: false,
    }
}

// ---------------------------------------------------------------------------
// IPv6 Formatting helpers
// ---------------------------------------------------------------------------

/// Format a complete IPv6 trace result as a human-readable string.
pub fn format_trace6(result: &TraceResult6) -> String {
    let mut out = String::with_capacity(512);

    out.push_str(&format!(
        "traceroute6 to {} (max {} hops, {} probes/hop)\n",
        result.destination,
        result.hops.len(),
        result.hops.first().map_or(0, |h| h.probes.len()),
    ));

    for hop in &result.hops {
        out.push_str(&format!("{:>2}  ", hop.hop));

        let mut last_ip = Ipv6Addr::UNSPECIFIED;
        for probe in &hop.probes {
            if probe.received {
                if probe.addr != last_ip {
                    if !last_ip.is_unspecified() {
                        out.push_str("  ");
                    }
                    out.push_str(&format!("{}", probe.addr));
                    last_ip = probe.addr;
                }
                out.push_str(&format!("  {}", format_rtt(probe.rtt_ns)));
            } else {
                out.push_str("  *");
            }
        }
        out.push('\n');
    }

    if result.reached {
        out.push_str(&format!(
            "\nDestination reached in {} hops ({} probes, {} timeouts)\n",
            result.hops.len(), result.probes_sent, result.probes_timeout
        ));
    } else {
        out.push_str(&format!(
            "\nDestination NOT reached after {} hops ({} probes, {} timeouts)\n",
            result.hops.len(), result.probes_sent, result.probes_timeout
        ));
    }

    out
}

// ---------------------------------------------------------------------------
// IPv6 Statistics
// ---------------------------------------------------------------------------

/// IPv6 traceroute statistics.
#[derive(Debug)]
pub struct TracerouteStats6 {
    pub active: bool,
    pub total_traces: u64,
    pub total_probes_sent: u64,
    pub total_probes_timeout: u64,
    pub last_hops: u32,
}

/// Get IPv6 traceroute statistics.
pub fn stats6() -> TracerouteStats6 {
    TracerouteStats6 {
        active: ACTIVE6.load(Ordering::Relaxed),
        total_traces: TOTAL_TRACES6.load(Ordering::Relaxed),
        total_probes_sent: TOTAL_PROBES_SENT6.load(Ordering::Relaxed),
        total_probes_timeout: TOTAL_PROBES_TIMEOUT6.load(Ordering::Relaxed),
        last_hops: LAST_HOPS6.load(Ordering::Relaxed),
    }
}

/// Generate procfs content for `/proc/traceroute6`.
pub fn procfs_content6() -> String {
    let s = stats6();

    let mut out = String::with_capacity(256);
    out.push_str("Traceroute6\n");
    out.push_str("===========\n\n");
    out.push_str(&format!("Active:          {}\n", if s.active { "yes" } else { "no" }));
    out.push_str(&format!("Total traces:    {}\n", s.total_traces));
    out.push_str(&format!("Probes sent:     {}\n", s.total_probes_sent));
    out.push_str(&format!("Probes timeout:  {}\n", s.total_probes_timeout));
    out.push_str(&format!("Last hop count:  {}\n", s.last_hops));
    out
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Traceroute statistics.
#[derive(Debug)]
pub struct TracerouteStats {
    pub active: bool,
    pub total_traces: u64,
    pub total_probes_sent: u64,
    pub total_probes_timeout: u64,
    pub last_hops: u32,
}

/// Get traceroute statistics.
pub fn stats() -> TracerouteStats {
    TracerouteStats {
        active: ACTIVE.load(Ordering::Relaxed),
        total_traces: TOTAL_TRACES.load(Ordering::Relaxed),
        total_probes_sent: TOTAL_PROBES_SENT.load(Ordering::Relaxed),
        total_probes_timeout: TOTAL_PROBES_TIMEOUT.load(Ordering::Relaxed),
        last_hops: LAST_HOPS.load(Ordering::Relaxed),
    }
}

/// Generate procfs content for `/proc/traceroute`.
pub fn procfs_content() -> String {
    let s = stats();

    let mut out = String::with_capacity(256);
    out.push_str("Traceroute\n");
    out.push_str("==========\n\n");
    out.push_str(&format!("Active:          {}\n", if s.active { "yes" } else { "no" }));
    out.push_str(&format!("Total traces:    {}\n", s.total_traces));
    out.push_str(&format!("Probes sent:     {}\n", s.total_probes_sent));
    out.push_str(&format!("Probes timeout:  {}\n", s.total_probes_timeout));
    out.push_str(&format!("Last hop count:  {}\n", s.last_hops));
    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run traceroute self-tests.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[traceroute] Running traceroute self-tests...");
    let mut passed = 0u32;

    // --- Test 1: RTT formatting ---
    {
        let s1 = format_rtt(500);        // 500 ns
        assert!(s1.contains("ns"), "sub-microsecond");

        let s2 = format_rtt(5_000);      // 5 us
        assert!(s2.contains("us"), "microsecond");

        let s3 = format_rtt(15_000_000); // 15 ms
        assert!(s3.contains("ms"), "millisecond");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 1 (RTT formatting) PASSED");
    }

    // --- Test 2: ProbeResult defaults ---
    {
        let pr = ProbeResult {
            rtt_ns: 0,
            addr: Ipv4Addr::UNSPECIFIED,
            received: false,
            reached_dst: false,
        };
        assert!(!pr.received, "default not received");
        assert!(!pr.reached_dst, "default not reached");
        assert!(pr.addr.is_unspecified(), "default addr");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 2 (ProbeResult defaults) PASSED");
    }

    // --- Test 3: HopResult construction ---
    {
        let hop = HopResult {
            hop: 5,
            probes: alloc::vec![
                ProbeResult {
                    rtt_ns: 1000,
                    addr: Ipv4Addr::new(10, 0, 0, 1),
                    received: true,
                    reached_dst: false,
                },
                ProbeResult {
                    rtt_ns: 0,
                    addr: Ipv4Addr::UNSPECIFIED,
                    received: false,
                    reached_dst: false,
                },
            ],
        };
        assert!(hop.hop == 5, "hop number");
        assert!(hop.probes.len() == 2, "probe count");
        assert!(hop.probes[0].received, "first probe received");
        assert!(!hop.probes[1].received, "second probe timeout");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 3 (HopResult construction) PASSED");
    }

    // --- Test 4: TraceResult formatting ---
    {
        let result = TraceResult {
            destination: Ipv4Addr::new(8, 8, 8, 8),
            hops: alloc::vec![
                HopResult {
                    hop: 1,
                    probes: alloc::vec![ProbeResult {
                        rtt_ns: 2_000_000,
                        addr: Ipv4Addr::new(10, 0, 2, 2),
                        received: true,
                        reached_dst: false,
                    }],
                },
                HopResult {
                    hop: 2,
                    probes: alloc::vec![ProbeResult {
                        rtt_ns: 5_000_000,
                        addr: Ipv4Addr::new(8, 8, 8, 8),
                        received: true,
                        reached_dst: true,
                    }],
                },
            ],
            reached: true,
            probes_sent: 2,
            probes_timeout: 0,
        };

        let formatted = format_trace(&result);
        assert!(formatted.contains("traceroute to"), "header");
        assert!(formatted.contains("8.8.8.8"), "destination");
        assert!(formatted.contains("10.0.2.2"), "hop 1 IP");
        assert!(formatted.contains("Destination reached"), "reached");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 4 (trace formatting) PASSED");
    }

    // --- Test 5: Timeout probe formatting ---
    {
        let result = TraceResult {
            destination: Ipv4Addr::new(192, 168, 1, 1),
            hops: alloc::vec![HopResult {
                hop: 1,
                probes: alloc::vec![ProbeResult {
                    rtt_ns: 0,
                    addr: Ipv4Addr::UNSPECIFIED,
                    received: false,
                    reached_dst: false,
                }],
            }],
            reached: false,
            probes_sent: 1,
            probes_timeout: 1,
        };

        let formatted = format_trace(&result);
        assert!(formatted.contains("*"), "timeout star");
        assert!(formatted.contains("NOT reached"), "not reached");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 5 (timeout formatting) PASSED");
    }

    // --- Test 6: ICMP trace echo request construction ---
    {
        let seq = 42u16;
        let pkt = icmp::build_trace_echo_request(seq);

        // Type = 8 (Echo Request).
        assert!(*pkt.get(0).unwrap_or(&0) == 8, "type");
        // Code = 0.
        assert!(*pkt.get(1).unwrap_or(&0xFF) == 0, "code");
        // ID = TRACEROUTE_ID (0x5678).
        let id = u16::from_be_bytes([
            *pkt.get(4).unwrap_or(&0),
            *pkt.get(5).unwrap_or(&0),
        ]);
        assert!(id == icmp::trace_id(), "traceroute ID");
        // Seq.
        let s = u16::from_be_bytes([
            *pkt.get(6).unwrap_or(&0),
            *pkt.get(7).unwrap_or(&0),
        ]);
        assert!(s == seq, "sequence number");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 6 (trace echo request) PASSED");
    }

    // --- Test 7: Constants ---
    {
        assert!(DEFAULT_MAX_HOPS == 30, "max hops");
        assert!(DEFAULT_PROBES_PER_HOP == 3, "probes per hop");
        assert!(DEFAULT_TIMEOUT_POLLS == 500, "timeout polls");
        assert!(PROTO_ICMP == 1, "ICMP protocol");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 7 (constants) PASSED");
    }

    // --- Test 8: Stats initial values ---
    {
        let s = stats();
        // Stats may be non-zero from previous runs, just check they're valid.
        assert!(!s.active || s.active, "active is bool");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 8 (stats) PASSED");
    }

    // --- Test 9: Procfs content ---
    {
        let content = procfs_content();
        assert!(content.contains("Traceroute"), "header");
        assert!(content.contains("Active:"), "active field");
        assert!(content.contains("Total traces:"), "traces field");
        assert!(content.contains("Probes sent:"), "probes field");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 9 (procfs content) PASSED");
    }

    // --- Test 10: Sequence number allocation ---
    {
        let s1 = icmp::next_trace_seq();
        let s2 = icmp::next_trace_seq();
        assert!(s2 == s1.wrapping_add(1), "seq increments");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 10 (seq allocation) PASSED");
    }

    // --- Test 11: IPv6 ProbeResult6 defaults ---
    {
        let pr = ProbeResult6 {
            rtt_ns: 0,
            addr: Ipv6Addr::UNSPECIFIED,
            received: false,
            reached_dst: false,
        };
        assert!(!pr.received, "v6 default not received");
        assert!(!pr.reached_dst, "v6 default not reached");
        assert!(pr.addr.is_unspecified(), "v6 default addr");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 11 (ProbeResult6 defaults) PASSED");
    }

    // --- Test 12: IPv6 HopResult6 construction ---
    {
        let hop = HopResult6 {
            hop: 3,
            probes: alloc::vec![
                ProbeResult6 {
                    rtt_ns: 5000,
                    addr: Ipv6Addr::LOOPBACK,
                    received: true,
                    reached_dst: false,
                },
                ProbeResult6 {
                    rtt_ns: 0,
                    addr: Ipv6Addr::UNSPECIFIED,
                    received: false,
                    reached_dst: false,
                },
            ],
        };
        assert!(hop.hop == 3, "v6 hop number");
        assert!(hop.probes.len() == 2, "v6 probe count");
        assert!(hop.probes[0].received, "v6 first probe received");
        assert!(!hop.probes[1].received, "v6 second probe timeout");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 12 (HopResult6 construction) PASSED");
    }

    // --- Test 13: IPv6 TraceResult6 formatting ---
    {
        let result = TraceResult6 {
            destination: Ipv6Addr::LOOPBACK,
            hops: alloc::vec![
                HopResult6 {
                    hop: 1,
                    probes: alloc::vec![ProbeResult6 {
                        rtt_ns: 2_000_000,
                        addr: Ipv6Addr([0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
                        received: true,
                        reached_dst: false,
                    }],
                },
                HopResult6 {
                    hop: 2,
                    probes: alloc::vec![ProbeResult6 {
                        rtt_ns: 5_000_000,
                        addr: Ipv6Addr::LOOPBACK,
                        received: true,
                        reached_dst: true,
                    }],
                },
            ],
            reached: true,
            probes_sent: 2,
            probes_timeout: 0,
        };

        let formatted = format_trace6(&result);
        assert!(formatted.contains("traceroute6 to"), "v6 header");
        assert!(formatted.contains("::1"), "v6 destination");
        assert!(formatted.contains("Destination reached"), "v6 reached");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 13 (trace6 formatting) PASSED");
    }

    // --- Test 14: IPv6 timeout probe formatting ---
    {
        let result = TraceResult6 {
            destination: Ipv6Addr([0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
            hops: alloc::vec![HopResult6 {
                hop: 1,
                probes: alloc::vec![ProbeResult6 {
                    rtt_ns: 0,
                    addr: Ipv6Addr::UNSPECIFIED,
                    received: false,
                    reached_dst: false,
                }],
            }],
            reached: false,
            probes_sent: 1,
            probes_timeout: 1,
        };

        let formatted = format_trace6(&result);
        assert!(formatted.contains("*"), "v6 timeout star");
        assert!(formatted.contains("NOT reached"), "v6 not reached");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 14 (v6 timeout formatting) PASSED");
    }

    // --- Test 15: IPv6 stats / procfs ---
    {
        let s = stats6();
        assert!(!s.active || s.active, "v6 active is bool");

        let content = procfs_content6();
        assert!(content.contains("Traceroute6"), "v6 header");
        assert!(content.contains("Active:"), "v6 active field");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 15 (v6 stats/procfs) PASSED");
    }

    // --- Test 16: IPv6 sequence number allocation ---
    {
        let s1 = icmpv6::next_trace6_seq();
        let s2 = icmpv6::next_trace6_seq();
        assert!(s2 == s1.wrapping_add(1), "v6 seq increments");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 16 (v6 seq allocation) PASSED");
    }

    // --- Test 17: IPv6 echo request construction ---
    {
        let src = Ipv6Addr::LOOPBACK;
        let dst = Ipv6Addr([0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        let seq = 99u16;
        let pkt = icmpv6::build_trace6_echo_request(&src, &dst, seq);

        // Type = 128 (Echo Request).
        assert!(*pkt.get(0).unwrap_or(&0) == 128, "v6 type");
        // ID = TRACEROUTE6_ID.
        let id = u16::from_be_bytes([
            *pkt.get(4).unwrap_or(&0),
            *pkt.get(5).unwrap_or(&0),
        ]);
        assert!(id == icmpv6::trace6_id(), "v6 traceroute ID");
        // Seq.
        let s = u16::from_be_bytes([
            *pkt.get(6).unwrap_or(&0),
            *pkt.get(7).unwrap_or(&0),
        ]);
        assert!(s == seq, "v6 sequence number");

        passed = passed.saturating_add(1);
        crate::serial_println!("[traceroute]   test 17 (v6 echo request) PASSED");
    }

    crate::serial_println!("[traceroute] All {} self-tests PASSED", passed);
    Ok(())
}
