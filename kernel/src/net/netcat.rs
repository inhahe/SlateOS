//! Netcat (nc) — TCP/UDP networking swiss army knife.
//!
//! A minimal implementation of the classic netcat utility for
//! testing and debugging network connections.
//!
//! ## Features
//!
//! - TCP client: connect to a remote host/port
//! - TCP server: listen on a port and accept one connection
//! - UDP client: send datagrams to a host/port
//! - Port scanning: test if TCP ports are open
//! - Banner grabbing: connect and display the server's greeting
//!
//! ## Usage
//!
//! ```text
//! nc <host> <port>           — TCP connect
//! nc -l <port>               — TCP listen
//! nc -u <host> <port>        — UDP send
//! nc -z <host> <port-range>  — port scan
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use super::interface::Ipv4Addr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default timeout for TCP connect (poll iterations).
const CONNECT_TIMEOUT_POLLS: u32 = 300;

/// Default timeout for data receive (poll iterations).
const RECV_TIMEOUT_POLLS: u32 = 100;

/// Maximum data to receive at once.
const MAX_RECV_SIZE: usize = 4096;

/// Maximum ports to scan in one range.
const MAX_SCAN_PORTS: u16 = 1024;

// Statistics.
static TCP_CONNECTS: AtomicU64 = AtomicU64::new(0);
static TCP_LISTENS: AtomicU64 = AtomicU64::new(0);
static UDP_SENDS: AtomicU64 = AtomicU64::new(0);
static PORT_SCANS: AtomicU64 = AtomicU64::new(0);
static BYTES_SENT: AtomicU64 = AtomicU64::new(0);
static BYTES_RECEIVED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// TCP client
// ---------------------------------------------------------------------------

/// Connect to a TCP host:port and return the initial data received.
///
/// Returns up to `MAX_RECV_SIZE` bytes of data (banner grab).
pub fn tcp_connect(host: Ipv4Addr, port: u16) -> KernelResult<(usize, Vec<u8>)> {
    TCP_CONNECTS.fetch_add(1, Ordering::Relaxed);

    let handle = super::tcp::connect(host, port)?;

    // Wait for connection to establish.
    for _ in 0..CONNECT_TIMEOUT_POLLS {
        super::poll();
    }

    // Try to read data (banner grab).
    let data = match super::tcp::read_up_to(handle, MAX_RECV_SIZE) {
        Ok(d) => {
            BYTES_RECEIVED.fetch_add(d.len() as u64, Ordering::Relaxed);
            d
        }
        Err(_) => Vec::new(),
    };

    Ok((handle, data))
}

/// Send data on a TCP connection.
pub fn tcp_send(handle: usize, data: &[u8]) -> KernelResult<usize> {
    let sent = super::tcp::send(handle, data)?;
    BYTES_SENT.fetch_add(sent as u64, Ordering::Relaxed);
    Ok(sent)
}

/// Receive data from a TCP connection.
pub fn tcp_recv(handle: usize) -> KernelResult<Vec<u8>> {
    // Poll to receive data.
    for _ in 0..RECV_TIMEOUT_POLLS {
        super::poll();
    }

    match super::tcp::read_up_to(handle, MAX_RECV_SIZE) {
        Ok(d) => {
            BYTES_RECEIVED.fetch_add(d.len() as u64, Ordering::Relaxed);
            Ok(d)
        }
        Err(e) => Err(e),
    }
}

/// Close a TCP connection.
pub fn tcp_close(handle: usize) {
    let _ = super::tcp::close(handle);
}

// ---------------------------------------------------------------------------
// TCP server (listen mode)
// ---------------------------------------------------------------------------

/// Listen on a TCP port and accept one connection.
///
/// Returns the accepted connection handle and any initial data received.
pub fn tcp_listen(port: u16) -> KernelResult<(usize, Ipv4Addr, u16)> {
    TCP_LISTENS.fetch_add(1, Ordering::Relaxed);

    let listener = super::tcp::bind(port)?;

    // Wait for a connection (long timeout — listen mode).
    for _ in 0..3000 {
        super::poll();

        if super::tcp::listener_has_pending(listener) {
            match super::tcp::accept(listener) {
                Ok(conn_handle) => {
                    // Get peer address.
                    let peer = super::tcp::peer_addr(conn_handle)
                        .unwrap_or((Ipv4Addr::UNSPECIFIED, 0));
                    let _ = super::tcp::close_listener(listener);
                    return Ok((conn_handle, peer.0, peer.1));
                }
                Err(e) => {
                    let _ = super::tcp::close_listener(listener);
                    return Err(e);
                }
            }
        }
    }

    let _ = super::tcp::close_listener(listener);
    Err(KernelError::TimedOut)
}

// ---------------------------------------------------------------------------
// UDP
// ---------------------------------------------------------------------------

/// Send a UDP datagram.
pub fn udp_send(dst: Ipv4Addr, port: u16, data: &[u8]) -> KernelResult<()> {
    UDP_SENDS.fetch_add(1, Ordering::Relaxed);
    BYTES_SENT.fetch_add(data.len() as u64, Ordering::Relaxed);

    // Use an ephemeral source port.
    let src_port = 49152 + (crate::hrtimer::now_ns() % 16384) as u16;
    super::udp::send(src_port, dst, port, data)
}

// ---------------------------------------------------------------------------
// Port scanning
// ---------------------------------------------------------------------------

/// Result of a port scan.
#[derive(Debug, Clone)]
pub struct PortScanResult {
    pub port: u16,
    pub open: bool,
}

/// Scan a range of TCP ports.
///
/// Tries to connect to each port with a short timeout.
/// Returns a list of results indicating which ports are open.
pub fn scan_ports(host: Ipv4Addr, start: u16, end: u16) -> Vec<PortScanResult> {
    PORT_SCANS.fetch_add(1, Ordering::Relaxed);

    let range_size = end.saturating_sub(start).saturating_add(1);
    let actual_end = if range_size > MAX_SCAN_PORTS {
        start.saturating_add(MAX_SCAN_PORTS).saturating_sub(1)
    } else {
        end
    };

    let mut results = Vec::with_capacity((actual_end.saturating_sub(start).saturating_add(1)) as usize);

    let mut port = start;
    while port <= actual_end {
        let open = check_port_open(host, port);
        results.push(PortScanResult { port, open });
        port = port.saturating_add(1);
        if port == 0 { break; } // Overflow guard.
    }

    results
}

/// Check if a single TCP port is open.
///
/// Attempts a quick TCP connect and returns true if the connection
/// succeeds within a short timeout.
fn check_port_open(host: Ipv4Addr, port: u16) -> bool {
    // Try to connect.
    let handle = match super::tcp::connect(host, port) {
        Ok(h) => h,
        Err(_) => return false,
    };

    // Short poll to wait for SYN-ACK.
    for _ in 0..50 {
        super::poll();
    }

    // Check if the connection established (try to peek).
    let is_open = super::tcp::peek(handle, 1).is_ok();

    // Also check state: if we can send without error, port is open.
    let is_open = is_open || super::tcp::send(handle, &[]).is_ok();

    let _ = super::tcp::close(handle);
    is_open
}

// ---------------------------------------------------------------------------
// Well-known ports
// ---------------------------------------------------------------------------

/// Get the service name for a well-known port.
pub fn service_name(port: u16) -> &'static str {
    match port {
        7 => "echo",
        9 => "discard",
        13 => "daytime",
        20 => "ftp-data",
        21 => "ftp",
        22 => "ssh",
        23 => "telnet",
        25 => "smtp",
        53 => "dns",
        67 => "dhcp-server",
        68 => "dhcp-client",
        69 => "tftp",
        80 => "http",
        110 => "pop3",
        123 => "ntp",
        143 => "imap",
        161 => "snmp",
        162 => "snmp-trap",
        443 => "https",
        514 => "syslog",
        993 => "imaps",
        995 => "pop3s",
        3306 => "mysql",
        5432 => "postgresql",
        5900 => "vnc",
        6379 => "redis",
        8080 => "http-alt",
        8443 => "https-alt",
        _ => "",
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Netcat statistics.
#[derive(Debug)]
pub struct NcStats {
    pub tcp_connects: u64,
    pub tcp_listens: u64,
    pub udp_sends: u64,
    pub port_scans: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

/// Get netcat statistics.
pub fn stats() -> NcStats {
    NcStats {
        tcp_connects: TCP_CONNECTS.load(Ordering::Relaxed),
        tcp_listens: TCP_LISTENS.load(Ordering::Relaxed),
        udp_sends: UDP_SENDS.load(Ordering::Relaxed),
        port_scans: PORT_SCANS.load(Ordering::Relaxed),
        bytes_sent: BYTES_SENT.load(Ordering::Relaxed),
        bytes_received: BYTES_RECEIVED.load(Ordering::Relaxed),
    }
}

/// Generate procfs content for `/proc/netcat`.
pub fn procfs_content() -> String {
    let s = stats();
    let mut out = String::with_capacity(256);
    out.push_str("Netcat (nc)\n");
    out.push_str("===========\n\n");
    out.push_str(&format!("TCP connects:  {}\n", s.tcp_connects));
    out.push_str(&format!("TCP listens:   {}\n", s.tcp_listens));
    out.push_str(&format!("UDP sends:     {}\n", s.udp_sends));
    out.push_str(&format!("Port scans:    {}\n", s.port_scans));
    out.push_str(&format!("Bytes sent:    {}\n", s.bytes_sent));
    out.push_str(&format!("Bytes received:{}\n", s.bytes_received));
    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run netcat self-tests.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[nc] Running netcat self-tests...");
    let mut passed = 0u32;

    // --- Test 1: Service name lookup ---
    {
        assert!(service_name(80) == "http", "port 80");
        assert!(service_name(443) == "https", "port 443");
        assert!(service_name(22) == "ssh", "port 22");
        assert!(service_name(53) == "dns", "port 53");
        assert!(service_name(23) == "telnet", "port 23");
        assert!(service_name(12345) == "", "unknown port");

        passed = passed.saturating_add(1);
        crate::serial_println!("[nc]   test 1 (service name) PASSED");
    }

    // --- Test 2: Port scan result structure ---
    {
        let result = PortScanResult { port: 80, open: true };
        assert!(result.port == 80, "port");
        assert!(result.open, "open");

        let result2 = PortScanResult { port: 81, open: false };
        assert!(!result2.open, "closed");

        passed = passed.saturating_add(1);
        crate::serial_println!("[nc]   test 2 (PortScanResult) PASSED");
    }

    // --- Test 3: Stats accessible ---
    {
        let s = stats();
        // Verify counters are accessible and u64-typed.
        let _ = s.tcp_connects;
        let _ = s.bytes_sent;

        passed = passed.saturating_add(1);
        crate::serial_println!("[nc]   test 3 (stats) PASSED");
    }

    // --- Test 4: Constants ---
    {
        assert!(CONNECT_TIMEOUT_POLLS > 0, "connect timeout");
        assert!(RECV_TIMEOUT_POLLS > 0, "recv timeout");
        assert!(MAX_RECV_SIZE > 0, "max recv");
        assert!(MAX_SCAN_PORTS > 0, "max scan ports");

        passed = passed.saturating_add(1);
        crate::serial_println!("[nc]   test 4 (constants) PASSED");
    }

    // --- Test 5: Procfs content ---
    {
        let content = procfs_content();
        assert!(content.contains("Netcat"), "header");
        assert!(content.contains("TCP connects:"), "connects field");
        assert!(content.contains("Bytes sent:"), "bytes field");

        passed = passed.saturating_add(1);
        crate::serial_println!("[nc]   test 5 (procfs content) PASSED");
    }

    // --- Test 6: Service name coverage ---
    {
        // Verify all well-known ports return non-empty.
        let known_ports = [7, 9, 13, 20, 21, 22, 23, 25, 53, 67, 68, 69,
            80, 110, 123, 143, 161, 162, 443, 514, 993, 995, 3306,
            5432, 5900, 6379, 8080, 8443];
        for &p in &known_ports {
            assert!(!service_name(p).is_empty(), "known port name");
        }

        passed = passed.saturating_add(1);
        crate::serial_println!("[nc]   test 6 (service name coverage) PASSED");
    }

    // --- Test 7: NcStats struct ---
    {
        let s = NcStats {
            tcp_connects: 10,
            tcp_listens: 2,
            udp_sends: 5,
            port_scans: 1,
            bytes_sent: 1024,
            bytes_received: 2048,
        };
        assert!(s.tcp_connects == 10, "connects");
        assert!(s.bytes_received == 2048, "received");

        passed = passed.saturating_add(1);
        crate::serial_println!("[nc]   test 7 (NcStats struct) PASSED");
    }

    // --- Test 8: MAX_SCAN_PORTS cap ---
    {
        assert!(MAX_SCAN_PORTS == 1024, "scan cap value");

        passed = passed.saturating_add(1);
        crate::serial_println!("[nc]   test 8 (scan port cap) PASSED");
    }

    crate::serial_println!("[nc] All {} self-tests PASSED", passed);
    Ok(())
}
