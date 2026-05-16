//! iperf — Network bandwidth measurement tool.
//!
//! A minimal implementation of iperf for measuring TCP and UDP
//! network throughput between two endpoints.
//!
//! ## Features
//!
//! - TCP throughput test (client sends, server receives)
//! - UDP throughput test with configurable bandwidth (IPv4 + IPv6)
//! - Bandwidth calculation with human-readable output
//! - Jitter and packet loss tracking for UDP
//! - Server mode: listens and measures incoming throughput
//!
//! ## Usage
//!
//! ```text
//! iperf server <port>              — start iperf server
//! iperf client <host> <port>       — TCP throughput test
//! iperf udp <host> <port> [size]   — UDP throughput test
//! iperf udp6 <host> <port> [size]  — UDP6 throughput test
//! iperf status                     — show test statistics
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use super::interface::Ipv4Addr;
use super::ipv6::Ipv6Addr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default test duration in poll iterations (~5 seconds).
const DEFAULT_TEST_POLLS: u32 = 500;

/// Default TCP send buffer size.
const TCP_SEND_SIZE: usize = 1460;

/// Default UDP datagram size.
const UDP_SEND_SIZE: usize = 1400;

/// Maximum number of UDP datagrams per test.
const MAX_UDP_PACKETS: u32 = 10000;

/// Poll iterations between TCP send bursts.
const SEND_INTERVAL_POLLS: u32 = 2;

/// Poll iterations to wait for connection.
const CONNECT_TIMEOUT_POLLS: u32 = 300;

/// Maximum receive buffer size.
const MAX_RECV_SIZE: usize = 65536;

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

static TESTS_RUN: AtomicU64 = AtomicU64::new(0);
static TCP_TESTS: AtomicU64 = AtomicU64::new(0);
static UDP_TESTS: AtomicU64 = AtomicU64::new(0);
static SERVER_SESSIONS: AtomicU64 = AtomicU64::new(0);
static TOTAL_BYTES_TX: AtomicU64 = AtomicU64::new(0);
static TOTAL_BYTES_RX: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Test results
// ---------------------------------------------------------------------------

/// Result of a throughput test.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API.
pub struct ThroughputResult {
    /// Protocol used ("TCP" or "UDP").
    pub protocol: &'static str,
    /// Total bytes transferred.
    pub bytes_transferred: u64,
    /// Duration of the test in nanoseconds.
    pub duration_ns: u64,
    /// Calculated throughput in bits per second.
    pub throughput_bps: u64,
    /// Number of packets/segments sent (UDP only).
    pub packets_sent: u32,
    /// Number of packets received (UDP only).
    pub packets_received: u32,
    /// Packet loss percentage (UDP only).
    pub loss_percent: f32,
    /// Average jitter in nanoseconds (UDP only).
    pub avg_jitter_ns: u64,
}

impl ThroughputResult {
    /// Format throughput as a human-readable string.
    #[allow(dead_code)] // Public API.
    pub fn format_throughput(&self) -> String {
        format_bandwidth(self.throughput_bps)
    }
}

// ---------------------------------------------------------------------------
// TCP client test
// ---------------------------------------------------------------------------

/// Run a TCP throughput test as client.
///
/// Connects to the server, sends data for the specified number of poll
/// iterations, and calculates throughput.
#[allow(dead_code)] // Public API.
pub fn tcp_client_test(host: Ipv4Addr, port: u16, duration_polls: u32) -> KernelResult<ThroughputResult> {
    TESTS_RUN.fetch_add(1, Ordering::Relaxed);
    TCP_TESTS.fetch_add(1, Ordering::Relaxed);

    let polls = if duration_polls == 0 { DEFAULT_TEST_POLLS } else { duration_polls };

    // Connect to server.
    let handle = super::tcp::connect(crate::netns::ROOT_NS, host.into(), port)?;

    // Wait for connection to establish.
    for _ in 0..CONNECT_TIMEOUT_POLLS {
        super::poll();
    }

    // Generate send buffer (repeating pattern).
    let send_buf = generate_pattern(TCP_SEND_SIZE);

    let start_ns = crate::hrtimer::now_ns();
    let mut total_sent: u64 = 0;

    // Send data in bursts.
    let mut poll_count: u32 = 0;
    while poll_count < polls {
        match super::tcp::send(handle, &send_buf) {
            Ok(sent) => {
                total_sent = total_sent.saturating_add(sent as u64);
            }
            Err(_) => {
                // Send buffer full or connection error — poll and retry.
            }
        }

        // Poll network stack.
        for _ in 0..SEND_INTERVAL_POLLS {
            super::poll();
        }
        poll_count = poll_count.saturating_add(1);
    }

    let end_ns = crate::hrtimer::now_ns();
    let duration_ns = end_ns.saturating_sub(start_ns);

    let _ = super::tcp::close(handle);

    TOTAL_BYTES_TX.fetch_add(total_sent, Ordering::Relaxed);

    // Calculate throughput (bits per second).
    let throughput_bps = if duration_ns > 0 {
        // (bytes * 8 * 1_000_000_000) / duration_ns
        total_sent
            .saturating_mul(8)
            .saturating_mul(1_000_000_000)
            .checked_div(duration_ns)
            .unwrap_or(0)
    } else {
        0
    };

    Ok(ThroughputResult {
        protocol: "TCP",
        bytes_transferred: total_sent,
        duration_ns,
        throughput_bps,
        packets_sent: 0,
        packets_received: 0,
        loss_percent: 0.0,
        avg_jitter_ns: 0,
    })
}

// ---------------------------------------------------------------------------
// TCP server test
// ---------------------------------------------------------------------------

/// Run a TCP throughput test as server.
///
/// Listens on a port, accepts one connection, receives data, and
/// calculates throughput.
#[allow(dead_code)] // Public API.
pub fn tcp_server_test(port: u16, max_polls: u32) -> KernelResult<ThroughputResult> {
    TESTS_RUN.fetch_add(1, Ordering::Relaxed);
    SERVER_SESSIONS.fetch_add(1, Ordering::Relaxed);

    let polls = if max_polls == 0 { 3000 } else { max_polls };

    let listener = super::tcp::bind(crate::netns::ROOT_NS, port)?;

    // Wait for a connection.
    let mut conn_handle = None;
    for _ in 0..polls {
        super::poll();
        if super::tcp::listener_has_pending(listener) {
            match super::tcp::accept(listener) {
                Ok(h) => {
                    conn_handle = Some(h);
                    break;
                }
                Err(_) => {}
            }
        }
    }

    let handle = match conn_handle {
        Some(h) => h,
        None => {
            let _ = super::tcp::close_listener(listener);
            return Err(KernelError::TimedOut);
        }
    };

    let _ = super::tcp::close_listener(listener);

    // Receive data and measure.
    let start_ns = crate::hrtimer::now_ns();
    let mut total_received: u64 = 0;
    let mut idle_polls: u32 = 0;
    let max_idle = 100; // Stop after 100 idle polls (no data).

    loop {
        super::poll();

        match super::tcp::read_up_to(handle, MAX_RECV_SIZE) {
            Ok(data) if !data.is_empty() => {
                total_received = total_received.saturating_add(data.len() as u64);
                idle_polls = 0;
            }
            _ => {
                idle_polls = idle_polls.saturating_add(1);
                if idle_polls >= max_idle {
                    break;
                }
            }
        }
    }

    let end_ns = crate::hrtimer::now_ns();
    let duration_ns = end_ns.saturating_sub(start_ns);

    let _ = super::tcp::close(handle);

    TOTAL_BYTES_RX.fetch_add(total_received, Ordering::Relaxed);

    let throughput_bps = if duration_ns > 0 {
        total_received
            .saturating_mul(8)
            .saturating_mul(1_000_000_000)
            .checked_div(duration_ns)
            .unwrap_or(0)
    } else {
        0
    };

    Ok(ThroughputResult {
        protocol: "TCP",
        bytes_transferred: total_received,
        duration_ns,
        throughput_bps,
        packets_sent: 0,
        packets_received: 0,
        loss_percent: 0.0,
        avg_jitter_ns: 0,
    })
}

// ---------------------------------------------------------------------------
// UDP throughput test
// ---------------------------------------------------------------------------

/// Run a UDP throughput test.
///
/// Sends a series of UDP datagrams and tracks statistics.
/// Since UDP is connectionless, the client just sends; a separate
/// server would collect and report.
#[allow(dead_code)] // Public API.
pub fn udp_client_test(
    host: Ipv4Addr,
    port: u16,
    packet_count: u32,
    packet_size: usize,
) -> KernelResult<ThroughputResult> {
    TESTS_RUN.fetch_add(1, Ordering::Relaxed);
    UDP_TESTS.fetch_add(1, Ordering::Relaxed);

    let count = if packet_count == 0 { 100 } else { packet_count.min(MAX_UDP_PACKETS) };
    let size = if packet_size == 0 { UDP_SEND_SIZE } else { packet_size.min(1472) };

    // Generate payload with sequence numbers.
    let payload = generate_pattern(size);

    let start_ns = crate::hrtimer::now_ns();
    let mut sent: u32 = 0;
    let mut total_bytes: u64 = 0;
    let mut last_send_ns: u64 = start_ns;
    let mut jitter_sum: u64 = 0;

    for seq in 0..count {
        // Build datagram with sequence header.
        let mut dgram = alloc::vec![0u8; size.min(payload.len())];
        dgram[..payload.len().min(size)].copy_from_slice(&payload[..payload.len().min(size)]);

        // Embed sequence number in first 4 bytes.
        if dgram.len() >= 4 {
            let seq_bytes = seq.to_be_bytes();
            dgram[0] = seq_bytes[0];
            dgram[1] = seq_bytes[1];
            dgram[2] = seq_bytes[2];
            dgram[3] = seq_bytes[3];
        }

        // Use ephemeral source port.
        let src_port = 49152u16.saturating_add((crate::hrtimer::now_ns() % 16384) as u16);
        match super::udp::send(src_port, host, port, &dgram) {
            Ok(()) => {
                sent = sent.saturating_add(1);
                total_bytes = total_bytes.saturating_add(dgram.len() as u64);

                // Track inter-packet jitter.
                let now = crate::hrtimer::now_ns();
                let interval = now.saturating_sub(last_send_ns);
                jitter_sum = jitter_sum.saturating_add(interval);
                last_send_ns = now;
            }
            Err(_) => {
                // Send failed — count as lost.
            }
        }

        // Brief poll to avoid overwhelming the stack.
        if seq % 10 == 0 {
            super::poll();
        }
    }

    let end_ns = crate::hrtimer::now_ns();
    let duration_ns = end_ns.saturating_sub(start_ns);

    TOTAL_BYTES_TX.fetch_add(total_bytes, Ordering::Relaxed);

    let throughput_bps = if duration_ns > 0 {
        total_bytes
            .saturating_mul(8)
            .saturating_mul(1_000_000_000)
            .checked_div(duration_ns)
            .unwrap_or(0)
    } else {
        0
    };

    let avg_jitter = if sent > 1 {
        jitter_sum.checked_div(sent.saturating_sub(1) as u64).unwrap_or(0)
    } else {
        0
    };

    // Loss = (count - sent) / count * 100.
    let loss = if count > 0 {
        let lost = count.saturating_sub(sent);
        (lost as f32 / count as f32) * 100.0
    } else {
        0.0
    };

    Ok(ThroughputResult {
        protocol: "UDP",
        bytes_transferred: total_bytes,
        duration_ns,
        throughput_bps,
        packets_sent: count,
        packets_received: sent, // From client side, this is actually "sent successfully".
        loss_percent: loss,
        avg_jitter_ns: avg_jitter,
    })
}

/// Run a UDP bandwidth test to a remote host over IPv6.
///
/// Same measurement methodology as [`udp_client_test`] but using
/// IPv6 UDP transport.
pub fn udp_client_test_v6(
    host: Ipv6Addr,
    port: u16,
    packet_count: u32,
    packet_size: usize,
) -> KernelResult<ThroughputResult> {
    TESTS_RUN.fetch_add(1, Ordering::Relaxed);
    UDP_TESTS.fetch_add(1, Ordering::Relaxed);

    let count = if packet_count == 0 { 100 } else { packet_count.min(MAX_UDP_PACKETS) };
    let size = if packet_size == 0 { UDP_SEND_SIZE } else { packet_size.min(1472) };

    let payload = generate_pattern(size);

    let start_ns = crate::hrtimer::now_ns();
    let mut sent: u32 = 0;
    let mut total_bytes: u64 = 0;
    let mut last_send_ns: u64 = start_ns;
    let mut jitter_sum: u64 = 0;

    for seq in 0..count {
        let mut dgram = alloc::vec![0u8; size.min(payload.len())];
        dgram[..payload.len().min(size)].copy_from_slice(&payload[..payload.len().min(size)]);

        // Embed sequence number in first 4 bytes.
        if dgram.len() >= 4 {
            let seq_bytes = seq.to_be_bytes();
            dgram[0] = seq_bytes[0];
            dgram[1] = seq_bytes[1];
            dgram[2] = seq_bytes[2];
            dgram[3] = seq_bytes[3];
        }

        let src_port = 49152u16.saturating_add((crate::hrtimer::now_ns() % 16384) as u16);
        match super::udp::send_v6(src_port, host, port, &dgram) {
            Ok(()) => {
                sent = sent.saturating_add(1);
                total_bytes = total_bytes.saturating_add(dgram.len() as u64);

                let now = crate::hrtimer::now_ns();
                let interval = now.saturating_sub(last_send_ns);
                jitter_sum = jitter_sum.saturating_add(interval);
                last_send_ns = now;
            }
            Err(_) => {
                // Send failed — count as lost.
            }
        }

        if seq % 10 == 0 {
            super::poll();
        }
    }

    let end_ns = crate::hrtimer::now_ns();
    let duration_ns = end_ns.saturating_sub(start_ns);

    TOTAL_BYTES_TX.fetch_add(total_bytes, Ordering::Relaxed);

    let throughput_bps = if duration_ns > 0 {
        total_bytes
            .saturating_mul(8)
            .saturating_mul(1_000_000_000)
            .checked_div(duration_ns)
            .unwrap_or(0)
    } else {
        0
    };

    let avg_jitter = if sent > 1 {
        jitter_sum.checked_div(sent.saturating_sub(1) as u64).unwrap_or(0)
    } else {
        0
    };

    let loss = if count > 0 {
        let lost = count.saturating_sub(sent);
        (lost as f32 / count as f32) * 100.0
    } else {
        0.0
    };

    Ok(ThroughputResult {
        protocol: "UDP",
        bytes_transferred: total_bytes,
        duration_ns,
        throughput_bps,
        packets_sent: count,
        packets_received: sent,
        loss_percent: loss,
        avg_jitter_ns: avg_jitter,
    })
}

// ---------------------------------------------------------------------------
// Bandwidth formatting
// ---------------------------------------------------------------------------

/// Format bits per second as human-readable bandwidth.
#[allow(dead_code)] // Public API.
pub fn format_bandwidth(bps: u64) -> String {
    if bps >= 1_000_000_000 {
        let gbps = bps / 1_000_000;
        let frac = (bps % 1_000_000_000) / 10_000_000;
        format!("{}.{:02} Gbps", gbps / 1000, frac)
    } else if bps >= 1_000_000 {
        let mbps = bps / 1_000;
        let frac = (bps % 1_000_000) / 10_000;
        format!("{}.{:02} Mbps", mbps / 1000, frac)
    } else if bps >= 1_000 {
        let kbps = bps;
        let frac = (bps % 1_000) / 10;
        format!("{}.{:02} Kbps", kbps / 1000, frac)
    } else {
        format!("{} bps", bps)
    }
}

/// Format bytes as human-readable size.
#[allow(dead_code)] // Public API.
pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        let gb = bytes / 1_048_576;
        let frac = (bytes % 1_073_741_824) / 10_737_418;
        format!("{}.{:02} GB", gb / 1024, frac)
    } else if bytes >= 1_048_576 {
        let mb = bytes / 1024;
        let frac = (bytes % 1_048_576) / 10_485;
        format!("{}.{:02} MB", mb / 1024, frac)
    } else if bytes >= 1024 {
        let kb = bytes;
        let frac = (bytes % 1024) / 10;
        format!("{}.{:02} KB", kb / 1024, frac)
    } else {
        format!("{} B", bytes)
    }
}

/// Format duration in nanoseconds as human-readable time.
#[allow(dead_code)] // Public API.
pub fn format_duration(ns: u64) -> String {
    if ns >= 1_000_000_000 {
        let secs = ns / 1_000_000;
        let frac = (ns % 1_000_000_000) / 10_000_000;
        format!("{}.{:02}s", secs / 1000, frac)
    } else if ns >= 1_000_000 {
        let ms = ns / 1_000;
        let frac = (ns % 1_000_000) / 10_000;
        format!("{}.{:02}ms", ms / 1000, frac)
    } else if ns >= 1_000 {
        let us = ns;
        let frac = (ns % 1_000) / 10;
        format!("{}.{:02}us", us / 1000, frac)
    } else {
        format!("{}ns", ns)
    }
}

// ---------------------------------------------------------------------------
// Helper: pattern generator
// ---------------------------------------------------------------------------

/// Generate a repeating byte pattern for sending.
fn generate_pattern(size: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(size);
    for i in 0..size {
        buf.push((i & 0xFF) as u8);
    }
    buf
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// iperf statistics.
#[derive(Debug)]
#[allow(dead_code)] // Public API.
pub struct IperfStats {
    pub tests_run: u64,
    pub tcp_tests: u64,
    pub udp_tests: u64,
    pub server_sessions: u64,
    pub total_bytes_tx: u64,
    pub total_bytes_rx: u64,
}

/// Get iperf statistics.
#[allow(dead_code)] // Public API.
pub fn stats() -> IperfStats {
    IperfStats {
        tests_run: TESTS_RUN.load(Ordering::Relaxed),
        tcp_tests: TCP_TESTS.load(Ordering::Relaxed),
        udp_tests: UDP_TESTS.load(Ordering::Relaxed),
        server_sessions: SERVER_SESSIONS.load(Ordering::Relaxed),
        total_bytes_tx: TOTAL_BYTES_TX.load(Ordering::Relaxed),
        total_bytes_rx: TOTAL_BYTES_RX.load(Ordering::Relaxed),
    }
}

/// Generate procfs content for `/proc/iperf`.
#[allow(dead_code)] // Public API.
pub fn procfs_content() -> String {
    let s = stats();
    let mut out = String::with_capacity(256);
    out.push_str("iperf (Network Bandwidth Measurement)\n");
    out.push_str("=====================================\n\n");
    out.push_str(&format!("Tests run:       {}\n", s.tests_run));
    out.push_str(&format!("TCP tests:       {}\n", s.tcp_tests));
    out.push_str(&format!("UDP tests:       {}\n", s.udp_tests));
    out.push_str(&format!("Server sessions: {}\n", s.server_sessions));
    out.push_str(&format!("Total TX:        {}\n", format_bytes(s.total_bytes_tx)));
    out.push_str(&format!("Total RX:        {}\n", format_bytes(s.total_bytes_rx)));
    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run iperf self-tests.
#[allow(dead_code)] // Public API.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[iperf] Running iperf self-tests...");
    let mut passed = 0u32;

    // --- Test 1: Bandwidth formatting ---
    {
        assert!(format_bandwidth(0) == "0 bps", "0 bps");
        assert!(format_bandwidth(500) == "500 bps", "500 bps");
        assert!(format_bandwidth(1_000_000).contains("Mbps"), "1 Mbps");
        assert!(format_bandwidth(1_000_000_000).contains("Gbps"), "1 Gbps");

        passed = passed.saturating_add(1);
        crate::serial_println!("[iperf]   test 1 (bandwidth formatting) PASSED");
    }

    // --- Test 2: Byte formatting ---
    {
        assert!(format_bytes(0) == "0 B", "0 bytes");
        assert!(format_bytes(500) == "500 B", "500 bytes");
        assert!(format_bytes(1_048_576).contains("MB"), "1 MB");
        assert!(format_bytes(1_073_741_824).contains("GB"), "1 GB");

        passed = passed.saturating_add(1);
        crate::serial_println!("[iperf]   test 2 (byte formatting) PASSED");
    }

    // --- Test 3: Duration formatting ---
    {
        assert!(format_duration(500) == "500ns", "500ns");
        assert!(format_duration(1_500_000).contains("ms"), "1.5ms");
        assert!(format_duration(2_000_000_000).contains("s"), "2s");

        passed = passed.saturating_add(1);
        crate::serial_println!("[iperf]   test 3 (duration formatting) PASSED");
    }

    // --- Test 4: Pattern generation ---
    {
        let pat = generate_pattern(256);
        assert!(pat.len() == 256, "pattern length");
        assert!(pat[0] == 0, "pattern[0]");
        assert!(pat[1] == 1, "pattern[1]");
        assert!(pat[255] == 255, "pattern[255]");

        let pat2 = generate_pattern(0);
        assert!(pat2.is_empty(), "empty pattern");

        passed = passed.saturating_add(1);
        crate::serial_println!("[iperf]   test 4 (pattern generation) PASSED");
    }

    // --- Test 5: ThroughputResult struct ---
    {
        let result = ThroughputResult {
            protocol: "TCP",
            bytes_transferred: 1_000_000,
            duration_ns: 1_000_000_000,
            throughput_bps: 8_000_000,
            packets_sent: 0,
            packets_received: 0,
            loss_percent: 0.0,
            avg_jitter_ns: 0,
        };
        assert!(result.protocol == "TCP", "protocol");
        assert!(result.bytes_transferred == 1_000_000, "bytes");
        assert!(result.throughput_bps == 8_000_000, "throughput");
        let fmt = result.format_throughput();
        assert!(fmt.contains("Mbps"), "formatted throughput");

        passed = passed.saturating_add(1);
        crate::serial_println!("[iperf]   test 5 (ThroughputResult) PASSED");
    }

    // --- Test 6: UDP ThroughputResult ---
    {
        let result = ThroughputResult {
            protocol: "UDP",
            bytes_transferred: 140_000,
            duration_ns: 500_000_000,
            throughput_bps: 2_240_000,
            packets_sent: 100,
            packets_received: 95,
            loss_percent: 5.0,
            avg_jitter_ns: 500_000,
        };
        assert!(result.packets_sent == 100, "sent");
        assert!(result.packets_received == 95, "received");
        assert!(result.loss_percent > 4.0, "loss");
        assert!(result.avg_jitter_ns == 500_000, "jitter");

        passed = passed.saturating_add(1);
        crate::serial_println!("[iperf]   test 6 (UDP result) PASSED");
    }

    // --- Test 7: Stats accessible ---
    {
        let s = stats();
        // Just verify the struct can be created and accessed.
        let _ = s.tests_run;
        let _ = s.tcp_tests;
        let _ = s.udp_tests;
        let _ = s.server_sessions;
        let _ = s.total_bytes_tx;
        let _ = s.total_bytes_rx;

        passed = passed.saturating_add(1);
        crate::serial_println!("[iperf]   test 7 (stats) PASSED");
    }

    // --- Test 8: Procfs content ---
    {
        let content = procfs_content();
        assert!(content.contains("iperf"), "header");
        assert!(content.contains("Tests run:"), "tests field");
        assert!(content.contains("Total TX:"), "tx field");
        assert!(content.contains("Total RX:"), "rx field");

        passed = passed.saturating_add(1);
        crate::serial_println!("[iperf]   test 8 (procfs content) PASSED");
    }

    // --- Test 9: Constants ---
    {
        assert!(DEFAULT_TEST_POLLS > 0, "test polls");
        assert!(TCP_SEND_SIZE > 0, "tcp send size");
        assert!(UDP_SEND_SIZE > 0, "udp send size");
        assert!(MAX_UDP_PACKETS > 0, "max udp");
        assert!(MAX_RECV_SIZE > 0, "max recv");

        passed = passed.saturating_add(1);
        crate::serial_println!("[iperf]   test 9 (constants) PASSED");
    }

    // --- Test 10: Large bandwidth formatting ---
    {
        // 10 Gbps
        let s = format_bandwidth(10_000_000_000);
        assert!(s.contains("Gbps"), "10 Gbps");

        // 100 Kbps
        let s = format_bandwidth(100_000);
        assert!(s.contains("Kbps"), "100 Kbps");

        // Edge: exactly 1000 bps
        let s = format_bandwidth(1000);
        assert!(s.contains("Kbps"), "1 Kbps");

        passed = passed.saturating_add(1);
        crate::serial_println!("[iperf]   test 10 (edge case formatting) PASSED");
    }

    crate::serial_println!("[iperf] All {} self-tests PASSED", passed);
    Ok(())
}
