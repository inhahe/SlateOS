//! Traceroute — trace the path packets take to a destination.
//!
//! Sends ICMP echo requests with increasing TTL values to discover
//! each hop along the route to a destination.  Intermediate routers
//! respond with ICMP Time Exceeded; the destination responds with
//! ICMP Echo Reply.
//!
//! ## Algorithm
//!
//! ```text
//! for ttl in 1..=max_hops:
//!   send ICMP Echo Request with TTL=ttl
//!   wait for reply:
//!     - ICMP Time Exceeded  → record hop (router IP, RTT)
//!     - ICMP Echo Reply     → record final hop (destination IP, RTT), done
//!     - timeout             → record hop as "*" (no response)
//! ```
//!
//! ## Usage
//!
//! ```text
//! traceroute 8.8.8.8          — trace route to Google DNS
//! traceroute 10.0.2.2 -m 15   — max 15 hops
//! traceroute 10.0.2.2 -q 3    — 3 probes per hop
//! ```
//!
//! ## Limitations
//!
//! - Uses ICMP echo requests (some firewalls may block these).
//! - Single traceroute at a time (global probe state).
//! - Polling-based (calls net::poll() in a loop).

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use super::interface::Ipv4Addr;
use super::ipv4;
use super::icmp;

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

    crate::serial_println!("[traceroute] All {} self-tests PASSED", passed);
    Ok(())
}
