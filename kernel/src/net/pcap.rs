//! Network packet capture (pcap format).
//!
//! Captures network traffic in the standard pcap format for offline
//! analysis with Wireshark, tcpdump, or similar tools.  The capture
//! is stored in a kernel ring buffer and can be exported as a `.pcap`
//! file via the kshell.
//!
//! ## Architecture
//!
//! ```text
//! NIC → recv_frame() ──→ pcap::capture_rx()
//!     → send_frame() ──→ pcap::capture_tx()
//!                            ↓
//!                     ring buffer (up to 256 packets)
//!                            ↓
//!                     pcap::export() → pcap file format
//! ```
//!
//! ## Pcap file format (libpcap)
//!
//! ```text
//! File header (24 bytes):
//!   magic_number:  0xa1b2c3d4 (microsecond resolution)
//!   version_major: 2
//!   version_minor: 4
//!   thiszone:      0 (UTC)
//!   sigfigs:       0
//!   snaplen:       65535
//!   link_type:     1 (LINKTYPE_ETHERNET)
//!
//! Per-packet record:
//!   ts_sec:        timestamp seconds
//!   ts_usec:       timestamp microseconds
//!   incl_len:      captured length
//!   orig_len:      original length
//!   packet_data:   [incl_len bytes]
//! ```
//!
//! ## Features
//!
//! - Capture RX (incoming) and/or TX (outgoing) packets
//! - Configurable BPF-like filter (by protocol, port, IP)
//! - Ring buffer with configurable size (default 256 packets)
//! - Export to standard pcap format
//! - Per-capture statistics
//!
//! ## Limitations
//!
//! - Maximum 1500 bytes captured per packet (Ethernet MTU).
//! - Ring buffer wraps — oldest packets are overwritten.
//! - No BPF bytecode filter — only simple protocol/port matching.
//! - Single capture session at a time.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU32, Ordering};
use crate::sync::Mutex;

use crate::error::KernelResult;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum packets in capture ring buffer.
const DEFAULT_RING_SIZE: usize = 256;

/// Maximum bytes captured per packet (snaplen).
const MAX_SNAPLEN: usize = 1500;

/// Pcap magic number (microsecond resolution).
const PCAP_MAGIC: u32 = 0xa1b2c3d4;

/// Pcap version.
const PCAP_VERSION_MAJOR: u16 = 2;
const PCAP_VERSION_MINOR: u16 = 4;

/// Link type: Ethernet.
const LINKTYPE_ETHERNET: u32 = 1;

/// Pcap file header size.
const PCAP_FILE_HEADER_SIZE: usize = 24;

/// Pcap packet header size.
const PCAP_PKT_HEADER_SIZE: usize = 16;

// EtherType values for simple filtering.
const ETHERTYPE_IPV4: u16 = 0x0800;
const ETHERTYPE_IPV6: u16 = 0x86DD;
const ETHERTYPE_ARP: u16 = 0x0806;

// IP protocol numbers (shared between IPv4 and IPv6 Next Header).
const PROTO_ICMP: u8 = 1;
const PROTO_TCP: u8 = 6;
const PROTO_UDP: u8 = 17;
const PROTO_ICMPV6: u8 = 58;

// ---------------------------------------------------------------------------
// Filter types
// ---------------------------------------------------------------------------

/// Simple packet capture filter.
#[derive(Debug, Clone, Copy)]
pub struct CaptureFilter {
    /// Capture RX packets.
    pub capture_rx: bool,
    /// Capture TX packets.
    pub capture_tx: bool,
    /// Filter by EtherType (0 = any).
    pub ethertype: u16,
    /// Filter by IP protocol (0 = any, requires ethertype = 0x0800).
    pub ip_proto: u8,
    /// Filter by source or destination port (0 = any, TCP/UDP only).
    pub port: u16,
}

impl CaptureFilter {
    /// Default filter: capture everything.
    pub const fn all() -> Self {
        Self {
            capture_rx: true,
            capture_tx: true,
            ethertype: 0,
            ip_proto: 0,
            port: 0,
        }
    }

    /// TCP-only filter (captures both IPv4 and IPv6 TCP).
    #[allow(dead_code)] // Public API.
    pub const fn tcp_only() -> Self {
        Self {
            capture_rx: true,
            capture_tx: true,
            ethertype: 0, // Any EtherType — protocol filter handles matching.
            ip_proto: PROTO_TCP,
            port: 0,
        }
    }

    /// UDP-only filter (captures both IPv4 and IPv6 UDP).
    #[allow(dead_code)] // Public API.
    pub const fn udp_only() -> Self {
        Self {
            capture_rx: true,
            capture_tx: true,
            ethertype: 0, // Any EtherType — protocol filter handles matching.
            ip_proto: PROTO_UDP,
            port: 0,
        }
    }

    /// IPv6-only filter.
    #[allow(dead_code)] // Public API.
    pub const fn ipv6_only() -> Self {
        Self {
            capture_rx: true,
            capture_tx: true,
            ethertype: ETHERTYPE_IPV6,
            ip_proto: 0,
            port: 0,
        }
    }

    /// ICMPv6-only filter.
    #[allow(dead_code)] // Public API.
    pub const fn icmpv6_only() -> Self {
        Self {
            capture_rx: true,
            capture_tx: true,
            ethertype: ETHERTYPE_IPV6,
            ip_proto: PROTO_ICMPV6,
            port: 0,
        }
    }

    /// Check whether a packet matches this filter.
    fn matches(&self, data: &[u8]) -> bool {
        if data.len() < 14 {
            return self.ethertype == 0 && self.ip_proto == 0 && self.port == 0;
        }

        let etype = (*data.get(12).unwrap_or(&0) as u16) << 8
            | *data.get(13).unwrap_or(&0) as u16;

        // EtherType filter.
        if self.ethertype != 0 && etype != self.ethertype {
            return false;
        }

        // IP protocol / port filtering — handle both IPv4 and IPv6.
        if self.ip_proto != 0 || self.port != 0 {
            if etype == ETHERTYPE_IPV4 {
                return self.matches_ipv4(data);
            } else if etype == ETHERTYPE_IPV6 {
                return self.matches_ipv6(data);
            }
            // Non-IP packet can't match protocol/port filters.
            return self.ip_proto == 0 && self.port == 0;
        }

        true
    }

    /// Match IPv4 packet protocol and port fields.
    fn matches_ipv4(&self, data: &[u8]) -> bool {
        // Minimum: Ethernet(14) + IPv4 header (20) = 34 bytes.
        if data.len() < 34 {
            return false;
        }

        let proto = *data.get(23).unwrap_or(&0);
        if self.ip_proto != 0 && proto != self.ip_proto {
            return false;
        }

        if self.port != 0 {
            let ihl = (*data.get(14).unwrap_or(&0) & 0x0F) as usize;
            // RFC 791: IHL minimum is 5 (20-byte header).
            if ihl < 5 {
                return false;
            }
            let transport_offset = 14usize.saturating_add(ihl.saturating_mul(4));
            if data.len() < transport_offset.saturating_add(4) {
                return false;
            }
            let src_port = (*data.get(transport_offset).unwrap_or(&0) as u16) << 8
                | *data.get(transport_offset.saturating_add(1)).unwrap_or(&0) as u16;
            let dst_port = (*data.get(transport_offset.saturating_add(2)).unwrap_or(&0) as u16) << 8
                | *data.get(transport_offset.saturating_add(3)).unwrap_or(&0) as u16;
            if src_port != self.port && dst_port != self.port {
                return false;
            }
        }

        true
    }

    /// Match IPv6 packet protocol and port fields.
    ///
    /// IPv6 fixed header: 40 bytes starting at offset 14 (after Ethernet).
    /// Next Header is at byte 6 of the IPv6 header (offset 14+6 = 20).
    /// Transport header starts at offset 14+40 = 54.
    fn matches_ipv6(&self, data: &[u8]) -> bool {
        // Minimum: Ethernet(14) + IPv6 header(40) = 54 bytes.
        if data.len() < 54 {
            return false;
        }

        // Next Header field is at IPv6 header byte 6.
        let next_header = *data.get(20).unwrap_or(&0);
        if self.ip_proto != 0 && next_header != self.ip_proto {
            return false;
        }

        if self.port != 0 {
            // Transport header starts at offset 54 for basic IPv6
            // (no extension headers).
            let transport_offset = 54usize;
            if data.len() < transport_offset.saturating_add(4) {
                return false;
            }
            let src_port = (*data.get(transport_offset).unwrap_or(&0) as u16) << 8
                | *data.get(transport_offset.saturating_add(1)).unwrap_or(&0) as u16;
            let dst_port = (*data.get(transport_offset.saturating_add(2)).unwrap_or(&0) as u16) << 8
                | *data.get(transport_offset.saturating_add(3)).unwrap_or(&0) as u16;
            if src_port != self.port && dst_port != self.port {
                return false;
            }
        }

        true
    }
}

// ---------------------------------------------------------------------------
// Captured packet
// ---------------------------------------------------------------------------

/// A single captured packet.
struct CapturedPacket {
    /// Packet data (up to MAX_SNAPLEN bytes).
    data: Vec<u8>,
    /// Original packet length (before truncation).
    orig_len: u32,
    /// Timestamp: seconds since capture start.
    ts_sec: u32,
    /// Timestamp: microseconds within the second.
    ts_usec: u32,
    /// Direction: true = RX (received), false = TX (sent).
    #[allow(dead_code)] // Spec-defined field.
    is_rx: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct CaptureState {
    /// Ring buffer of captured packets.
    ring: Vec<CapturedPacket>,
    /// Write index.
    write_idx: usize,
    /// Number of packets stored (up to ring capacity).
    count: usize,
    /// Ring capacity.
    capacity: usize,
    /// Active capture filter.
    filter: CaptureFilter,
    /// Capture start timestamp (ns).
    start_ns: u64,
}

impl CaptureState {
    const fn new() -> Self {
        Self {
            ring: Vec::new(),
            write_idx: 0,
            count: 0,
            capacity: DEFAULT_RING_SIZE,
            filter: CaptureFilter::all(),
            start_ns: 0,
        }
    }
}

static STATE: Mutex<CaptureState> = Mutex::new(CaptureState::new());
static CAPTURING: AtomicBool = AtomicBool::new(false);
static SNAPLEN: AtomicU32 = AtomicU32::new(MAX_SNAPLEN as u32);

// Statistics.
static TOTAL_CAPTURED: AtomicU64 = AtomicU64::new(0);
static TOTAL_DROPPED: AtomicU64 = AtomicU64::new(0);
static TOTAL_FILTERED: AtomicU64 = AtomicU64::new(0);
static RX_CAPTURED: AtomicU64 = AtomicU64::new(0);
static TX_CAPTURED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Capture API
// ---------------------------------------------------------------------------

/// Start capturing packets with the given filter.
pub fn start(filter: CaptureFilter) {
    let now = crate::hrtimer::now_ns();
    let mut state = STATE.lock();

    state.ring.clear();
    state.write_idx = 0;
    state.count = 0;
    state.filter = filter;
    state.start_ns = now;

    // Pre-allocate ring buffer.
    let cap = state.capacity;
    state.ring.reserve(cap);

    // Reset statistics.
    TOTAL_CAPTURED.store(0, Ordering::Relaxed);
    TOTAL_DROPPED.store(0, Ordering::Relaxed);
    TOTAL_FILTERED.store(0, Ordering::Relaxed);
    RX_CAPTURED.store(0, Ordering::Relaxed);
    TX_CAPTURED.store(0, Ordering::Relaxed);

    CAPTURING.store(true, Ordering::Relaxed);
    crate::serial_println!("[pcap] Capture started (snaplen={}, ring={})",
        SNAPLEN.load(Ordering::Relaxed), state.capacity);
}

/// Stop capturing packets.
pub fn stop() {
    CAPTURING.store(false, Ordering::Relaxed);
    let state = STATE.lock();
    crate::serial_println!(
        "[pcap] Capture stopped ({} packets, {} filtered, {} dropped)",
        state.count,
        TOTAL_FILTERED.load(Ordering::Relaxed),
        TOTAL_DROPPED.load(Ordering::Relaxed),
    );
}

/// Check if capture is active.
pub fn is_capturing() -> bool {
    CAPTURING.load(Ordering::Relaxed)
}

/// Set the snap length (max bytes per packet).
pub fn set_snaplen(len: u32) {
    SNAPLEN.store(len.min(MAX_SNAPLEN as u32), Ordering::Relaxed);
}

/// Record a received (RX) packet.
///
/// Called from the frame receive path.  Must be fast when capture
/// is disabled (single atomic load).
pub fn capture_rx(data: &[u8]) {
    if !CAPTURING.load(Ordering::Relaxed) {
        return;
    }
    record_packet(data, true);
}

/// Record a transmitted (TX) packet.
///
/// Called from the frame send path.
pub fn capture_tx(data: &[u8]) {
    if !CAPTURING.load(Ordering::Relaxed) {
        return;
    }
    record_packet(data, false);
}

/// Record a packet into the ring buffer.
fn record_packet(data: &[u8], is_rx: bool) {
    let mut state = STATE.lock();

    // Check direction filter.
    if is_rx && !state.filter.capture_rx {
        return;
    }
    if !is_rx && !state.filter.capture_tx {
        return;
    }

    // Apply packet filter.
    if !state.filter.matches(data) {
        TOTAL_FILTERED.fetch_add(1, Ordering::Relaxed);
        return;
    }

    let now = crate::hrtimer::now_ns();
    let elapsed = now.saturating_sub(state.start_ns);
    let ts_sec = (elapsed / 1_000_000_000) as u32;
    let ts_usec = ((elapsed % 1_000_000_000) / 1_000) as u32;

    let snaplen = SNAPLEN.load(Ordering::Relaxed) as usize;
    let captured_len = data.len().min(snaplen);
    let captured_data = data.get(..captured_len).unwrap_or(data).to_vec();

    let pkt = CapturedPacket {
        data: captured_data,
        orig_len: data.len() as u32,
        ts_sec,
        ts_usec,
        is_rx,
    };

    // Insert into ring buffer.
    if state.ring.len() < state.capacity {
        state.ring.push(pkt);
        state.write_idx = state.ring.len();
    } else {
        let idx = state.write_idx % state.capacity;
        if let Some(slot) = state.ring.get_mut(idx) {
            *slot = pkt;
        }
        state.write_idx = state.write_idx.wrapping_add(1);
        TOTAL_DROPPED.fetch_add(1, Ordering::Relaxed);
    }
    state.count = state.count.saturating_add(1);

    TOTAL_CAPTURED.fetch_add(1, Ordering::Relaxed);
    if is_rx {
        RX_CAPTURED.fetch_add(1, Ordering::Relaxed);
    } else {
        TX_CAPTURED.fetch_add(1, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// Export captured packets as a pcap file.
///
/// Returns the complete pcap file contents (file header + packet records).
pub fn export() -> Vec<u8> {
    let state = STATE.lock();
    let pkt_count = state.ring.len();

    // Estimate size: file header + per-packet headers + data.
    let est_size = PCAP_FILE_HEADER_SIZE + pkt_count * (PCAP_PKT_HEADER_SIZE + 100);
    let mut buf = Vec::with_capacity(est_size);

    // Write pcap file header.
    write_file_header(&mut buf);

    // Write packets in order.
    // If the ring hasn't wrapped, packets are 0..count.
    // If wrapped, start from write_idx and go around.
    let capacity = state.capacity;
    let total = state.ring.len();

    if state.count <= capacity {
        // Haven't wrapped yet — all packets are in order.
        for pkt in &state.ring {
            write_packet_record(&mut buf, pkt);
        }
    } else {
        // Ring has wrapped — start from write_idx.
        let start = state.write_idx % capacity;
        for i in 0..total {
            let idx = (start + i) % capacity;
            if let Some(pkt) = state.ring.get(idx) {
                write_packet_record(&mut buf, pkt);
            }
        }
    }

    buf
}

/// Write the pcap file header.
fn write_file_header(buf: &mut Vec<u8>) {
    write_u32_le(buf, PCAP_MAGIC);
    write_u16_le(buf, PCAP_VERSION_MAJOR);
    write_u16_le(buf, PCAP_VERSION_MINOR);
    write_u32_le(buf, 0); // thiszone (UTC)
    write_u32_le(buf, 0); // sigfigs
    write_u32_le(buf, SNAPLEN.load(Ordering::Relaxed));
    write_u32_le(buf, LINKTYPE_ETHERNET);
}

/// Write a single packet record.
fn write_packet_record(buf: &mut Vec<u8>, pkt: &CapturedPacket) {
    write_u32_le(buf, pkt.ts_sec);
    write_u32_le(buf, pkt.ts_usec);
    write_u32_le(buf, pkt.data.len() as u32); // incl_len
    write_u32_le(buf, pkt.orig_len);          // orig_len
    buf.extend_from_slice(&pkt.data);
}

/// Write a u32 in little-endian.
fn write_u32_le(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

/// Write a u16 in little-endian.
fn write_u16_le(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

// ---------------------------------------------------------------------------
// Statistics and diagnostics
// ---------------------------------------------------------------------------

/// Pcap capture statistics.
#[derive(Debug)]
pub struct PcapStats {
    pub capturing: bool,
    pub total_captured: u64,
    pub total_filtered: u64,
    pub total_dropped: u64,
    pub rx_captured: u64,
    pub tx_captured: u64,
    pub ring_used: usize,
    pub ring_capacity: usize,
    pub snaplen: u32,
}

/// Get capture statistics.
pub fn stats() -> PcapStats {
    let state = STATE.lock();
    PcapStats {
        capturing: CAPTURING.load(Ordering::Relaxed),
        total_captured: TOTAL_CAPTURED.load(Ordering::Relaxed),
        total_filtered: TOTAL_FILTERED.load(Ordering::Relaxed),
        total_dropped: TOTAL_DROPPED.load(Ordering::Relaxed),
        rx_captured: RX_CAPTURED.load(Ordering::Relaxed),
        tx_captured: TX_CAPTURED.load(Ordering::Relaxed),
        ring_used: state.ring.len(),
        ring_capacity: state.capacity,
        snaplen: SNAPLEN.load(Ordering::Relaxed),
    }
}

/// Generate procfs content for `/proc/pcap`.
pub fn procfs_content() -> String {
    let s = stats();

    let mut out = String::with_capacity(256);
    out.push_str("Packet Capture\n");
    out.push_str("==============\n\n");

    out.push_str(&format!("Status:    {}\n",
        if s.capturing { "CAPTURING" } else { "idle" }));
    out.push_str(&format!("Snaplen:   {} bytes\n", s.snaplen));
    out.push_str(&format!("Ring:      {}/{} packets\n", s.ring_used, s.ring_capacity));
    out.push_str(&format!("Captured:  {} ({} RX, {} TX)\n",
        s.total_captured, s.rx_captured, s.tx_captured));
    out.push_str(&format!("Filtered:  {}\n", s.total_filtered));
    out.push_str(&format!("Dropped:   {}\n", s.total_dropped));

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run pcap self-tests.
// Self-tests deliberately runtime-assert pcap-format constants
// (magic numbers, link-type codes) as living documentation.
#[allow(clippy::assertions_on_constants)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[pcap] Running pcap self-tests...");
    let mut passed = 0u32;

    // --- Test 1: File header construction ---
    {
        let mut buf = Vec::new();
        write_file_header(&mut buf);
        assert!(buf.len() == PCAP_FILE_HEADER_SIZE, "file header size");

        // Check magic number (little-endian).
        assert!(buf.get(0..4) == Some(&[0xd4, 0xc3, 0xb2, 0xa1][..]), "magic number");

        // Version 2.4.
        assert!(buf.get(4..6) == Some(&[2, 0][..]), "version major");
        assert!(buf.get(6..8) == Some(&[4, 0][..]), "version minor");

        // Link type = 1 (Ethernet).
        assert!(*buf.get(20).unwrap_or(&0) == 1, "link type");

        passed = passed.saturating_add(1);
        crate::serial_println!("[pcap]   test 1 (file header) PASSED");
    }

    // --- Test 2: Packet record construction ---
    {
        let pkt = CapturedPacket {
            data: alloc::vec![0xAA, 0xBB, 0xCC],
            orig_len: 10,
            ts_sec: 5,
            ts_usec: 1000,
            is_rx: true,
        };
        let mut buf = Vec::new();
        write_packet_record(&mut buf, &pkt);

        assert!(buf.len() == PCAP_PKT_HEADER_SIZE + 3, "record size");

        // ts_sec = 5 (LE).
        assert!(buf.get(0..4) == Some(&5u32.to_le_bytes()[..]), "ts_sec");
        // ts_usec = 1000 (LE).
        assert!(buf.get(4..8) == Some(&1000u32.to_le_bytes()[..]), "ts_usec");
        // incl_len = 3.
        assert!(buf.get(8..12) == Some(&3u32.to_le_bytes()[..]), "incl_len");
        // orig_len = 10.
        assert!(buf.get(12..16) == Some(&10u32.to_le_bytes()[..]), "orig_len");
        // Data.
        assert!(buf.get(16..19) == Some(&[0xAA, 0xBB, 0xCC][..]), "data");

        passed = passed.saturating_add(1);
        crate::serial_println!("[pcap]   test 2 (packet record) PASSED");
    }

    // --- Test 3: Filter: match all ---
    {
        let f = CaptureFilter::all();
        let frame = [0u8; 64]; // Dummy Ethernet frame.
        assert!(f.matches(&frame), "all matches everything");

        passed = passed.saturating_add(1);
        crate::serial_println!("[pcap]   test 3 (filter: match all) PASSED");
    }

    // --- Test 4: EtherType filter ---
    {
        let f = CaptureFilter {
            capture_rx: true,
            capture_tx: true,
            ethertype: ETHERTYPE_ARP,
            ip_proto: 0,
            port: 0,
        };

        // ARP frame (ethertype at bytes 12-13 = 0x0806).
        let mut arp_frame = [0u8; 64];
        arp_frame[12] = 0x08;
        arp_frame[13] = 0x06;
        assert!(f.matches(&arp_frame), "ARP matches ARP filter");

        // IPv4 frame.
        let mut ipv4_frame = [0u8; 64];
        ipv4_frame[12] = 0x08;
        ipv4_frame[13] = 0x00;
        assert!(!f.matches(&ipv4_frame), "IPv4 doesn't match ARP filter");

        passed = passed.saturating_add(1);
        crate::serial_println!("[pcap]   test 4 (EtherType filter) PASSED");
    }

    // --- Test 5: IP protocol filter ---
    {
        let f = CaptureFilter {
            capture_rx: true,
            capture_tx: true,
            ethertype: ETHERTYPE_IPV4,
            ip_proto: PROTO_TCP,
            port: 0,
        };

        // TCP packet.
        let mut tcp_frame = [0u8; 64];
        tcp_frame[12] = 0x08; // IPv4
        tcp_frame[13] = 0x00;
        tcp_frame[14] = 0x45; // IP version 4, IHL 5
        tcp_frame[23] = PROTO_TCP;
        assert!(f.matches(&tcp_frame), "TCP matches TCP filter");

        // UDP packet.
        let mut udp_frame = [0u8; 64];
        udp_frame[12] = 0x08;
        udp_frame[13] = 0x00;
        udp_frame[14] = 0x45;
        udp_frame[23] = PROTO_UDP;
        assert!(!f.matches(&udp_frame), "UDP doesn't match TCP filter");

        passed = passed.saturating_add(1);
        crate::serial_println!("[pcap]   test 5 (IP protocol filter) PASSED");
    }

    // --- Test 6: Export empty capture ---
    {
        // Stop any running capture.
        CAPTURING.store(false, Ordering::Relaxed);
        let mut state = STATE.lock();
        state.ring.clear();
        state.count = 0;
        state.write_idx = 0;
        drop(state);

        let data = export();
        assert!(data.len() == PCAP_FILE_HEADER_SIZE, "empty export = header only");

        passed = passed.saturating_add(1);
        crate::serial_println!("[pcap]   test 6 (empty export) PASSED");
    }

    // --- Test 7: LE encoding ---
    {
        let mut buf = Vec::new();
        write_u32_le(&mut buf, 0x12345678);
        assert!(buf == [0x78, 0x56, 0x34, 0x12], "u32 LE");

        let mut buf2 = Vec::new();
        write_u16_le(&mut buf2, 0xABCD);
        assert!(buf2 == [0xCD, 0xAB], "u16 LE");

        passed = passed.saturating_add(1);
        crate::serial_println!("[pcap]   test 7 (LE encoding) PASSED");
    }

    // --- Test 8: Constants validation ---
    {
        assert!(PCAP_MAGIC == 0xa1b2c3d4, "magic");
        assert!(PCAP_FILE_HEADER_SIZE == 24, "file header size");
        assert!(PCAP_PKT_HEADER_SIZE == 16, "pkt header size");
        assert!(LINKTYPE_ETHERNET == 1, "link type");
        assert!(ETHERTYPE_IPV4 == 0x0800, "IPv4 ethertype");
        assert!(ETHERTYPE_ARP == 0x0806, "ARP ethertype");
        assert!(PROTO_TCP == 6, "TCP proto");
        assert!(PROTO_UDP == 17, "UDP proto");
        assert!(PROTO_ICMP == 1, "ICMP proto");

        passed = passed.saturating_add(1);
        crate::serial_println!("[pcap]   test 8 (constants) PASSED");
    }

    // --- Test 9: Port filter ---
    {
        let f = CaptureFilter {
            capture_rx: true,
            capture_tx: true,
            ethertype: 0,
            ip_proto: 0,
            port: 80,
        };

        // HTTP packet (dst port 80).
        let mut http_frame = [0u8; 64];
        http_frame[12] = 0x08; // IPv4
        http_frame[13] = 0x00;
        http_frame[14] = 0x45; // IHL=5
        http_frame[23] = PROTO_TCP;
        // Transport header at offset 34 (14 + 20).
        http_frame[34] = 0; // src port high
        http_frame[35] = 0; // src port low (port 0)
        http_frame[36] = 0; // dst port high
        http_frame[37] = 80; // dst port low
        assert!(f.matches(&http_frame), "port 80 matches");

        // Non-HTTP packet (port 443).
        let mut https_frame = http_frame;
        https_frame[36] = 0x01;
        https_frame[37] = 0xBB; // port 443
        https_frame[34] = 0x01;
        https_frame[35] = 0xBC; // src port 444
        assert!(!f.matches(&https_frame), "port 443 doesn't match port 80 filter");

        passed = passed.saturating_add(1);
        crate::serial_println!("[pcap]   test 9 (port filter) PASSED");
    }

    // --- Test 10: Short frame rejection ---
    {
        let f = CaptureFilter {
            capture_rx: true,
            capture_tx: true,
            ethertype: ETHERTYPE_IPV4,
            ip_proto: 0,
            port: 0,
        };

        // Frame too short for EtherType check.
        let short = [0u8; 10];
        assert!(!f.matches(&short), "short frame rejected");

        passed = passed.saturating_add(1);
        crate::serial_println!("[pcap]   test 10 (short frame) PASSED");
    }

    crate::serial_println!("[pcap] All {} self-tests PASSED", passed);
    Ok(())
}
