//! LLDP (Link Layer Discovery Protocol) implementation.
//!
//! IEEE 802.1AB — discovers and advertises network device identity
//! and capabilities on the local link.  LLDP operates at Layer 2
//! (no IP required) using EtherType 0x88CC.
//!
//! ## Protocol overview
//!
//! LLDP uses a simple TLV (Type-Length-Value) encoding.  Each LLDP
//! frame (LLDPDU) contains:
//!
//! ```text
//! Chassis ID TLV (required)    — identifies the device
//! Port ID TLV (required)       — identifies the port
//! TTL TLV (required)           — neighbor lifetime in seconds
//! [optional TLVs]              — system name, description, capabilities, etc.
//! End of LLDPDU TLV (required) — type=0, length=0
//! ```
//!
//! ## TLV format
//!
//! ```text
//! ┌─────────────────────┬───────────────────┐
//! │ Type (7 bits) + Len (9 bits) = 2 bytes  │
//! ├─────────────────────────────────────────┤
//! │ Value (0..511 bytes)                    │
//! └─────────────────────────────────────────┘
//! ```
//!
//! ## Features
//!
//! - Receive and parse LLDP frames from network neighbors
//! - Send periodic LLDP advertisements (30-second interval)
//! - Maintain neighbor table (up to 16 entries, TTL-based expiry)
//! - Display discovered neighbors via kshell `lldp` command

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::KernelResult;
use super::interface::Ipv4Addr;
use super::ethernet;
use crate::virtio::net::MacAddress;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// LLDP EtherType.
pub const ETHERTYPE_LLDP: u16 = 0x88CC;

/// LLDP multicast destination MAC (01:80:C2:00:00:0E).
const LLDP_MULTICAST_MAC: MacAddress = MacAddress([0x01, 0x80, 0xC2, 0x00, 0x00, 0x0E]);

/// Maximum LLDP neighbors tracked.
const MAX_NEIGHBORS: usize = 16;

/// Default TTL for our LLDP advertisements (seconds).
const DEFAULT_TTL: u16 = 120;

/// Advertisement interval (ns) — 30 seconds per IEEE 802.1AB.
const TX_INTERVAL_NS: u64 = 30_000_000_000;

/// Tick interval for neighbor expiry (5 seconds).
const TICK_INTERVAL_NS: u64 = 5_000_000_000;

// TLV types.
const TLV_END: u8 = 0;
const TLV_CHASSIS_ID: u8 = 1;
const TLV_PORT_ID: u8 = 2;
const TLV_TTL: u8 = 3;
const TLV_PORT_DESC: u8 = 4;
const TLV_SYSTEM_NAME: u8 = 5;
const TLV_SYSTEM_DESC: u8 = 6;
const TLV_SYSTEM_CAP: u8 = 7;
const TLV_MGMT_ADDR: u8 = 8;

// Chassis ID subtypes.
const CHASSIS_MAC_ADDRESS: u8 = 4;

// Port ID subtypes.
const PORT_INTERFACE_NAME: u8 = 5;
const PORT_MAC_ADDRESS: u8 = 3;

// System capabilities bits.
const CAP_OTHER: u16 = 0x0001;
const CAP_REPEATER: u16 = 0x0002;
const CAP_BRIDGE: u16 = 0x0004;
const CAP_ROUTER: u16 = 0x0010;
const CAP_STATION: u16 = 0x0080;

// ---------------------------------------------------------------------------
// TLV parsing
// ---------------------------------------------------------------------------

/// A parsed TLV.
#[derive(Debug, Clone)]
struct Tlv<'a> {
    tlv_type: u8,
    value: &'a [u8],
}

/// Parse a TLV from raw bytes.
///
/// Returns the TLV and the number of bytes consumed.
#[allow(clippy::arithmetic_side_effects)]
fn parse_tlv(data: &[u8]) -> Option<(Tlv<'_>, usize)> {
    if data.len() < 2 {
        return None;
    }

    // Type (7 bits) and length (9 bits) packed in 2 bytes.
    let header = u16::from_be_bytes([data[0], data[1]]);
    let tlv_type = (header >> 9) as u8;
    let length = (header & 0x01FF) as usize;

    if data.len() < 2 + length {
        return None;
    }

    let value = &data[2..2 + length];
    Some((Tlv { tlv_type, value }, 2 + length))
}

/// Encode a TLV header (type + length) into 2 bytes.
///
/// The LLDP TLV format packs type (7 bits, max 127) and length
/// (9 bits, max 511) into a 2-byte header.  Values beyond 511
/// are silently truncated by the bit mask — callers must ensure
/// `length <= 511`.
#[allow(clippy::arithmetic_side_effects)]
fn encode_tlv_header(tlv_type: u8, length: usize) -> [u8; 2] {
    debug_assert!(length <= 511, "LLDP TLV length exceeds 9-bit maximum ({})", length);
    let header = ((tlv_type as u16) << 9) | (length as u16 & 0x01FF);
    header.to_be_bytes()
}

// ---------------------------------------------------------------------------
// Neighbor table
// ---------------------------------------------------------------------------

/// Information about an LLDP neighbor.
#[derive(Debug, Clone)]
pub struct Neighbor {
    /// MAC address of the neighbor (from Ethernet source).
    pub mac: MacAddress,
    /// Chassis ID string.
    pub chassis_id: String,
    /// Port ID string.
    pub port_id: String,
    /// System name (if provided).
    pub system_name: String,
    /// System description (if provided).
    pub system_desc: String,
    /// Port description (if provided).
    pub port_desc: String,
    /// Capabilities (bitfield).
    pub capabilities: u16,
    /// Enabled capabilities (bitfield).
    pub enabled_caps: u16,
    /// Management IP (if provided).
    pub mgmt_ip: Ipv4Addr,
    /// Time-to-live in seconds.
    pub ttl: u16,
    /// Timestamp when this entry was last updated (ns).
    pub last_seen_ns: u64,
}

impl Neighbor {
    fn new() -> Self {
        Self {
            mac: MacAddress([0; 6]),
            chassis_id: String::new(),
            port_id: String::new(),
            system_name: String::new(),
            system_desc: String::new(),
            port_desc: String::new(),
            capabilities: 0,
            enabled_caps: 0,
            mgmt_ip: Ipv4Addr::UNSPECIFIED,
            ttl: 0,
            last_seen_ns: 0,
        }
    }

    #[allow(dead_code)] // Public API.
    fn is_empty(&self) -> bool {
        self.ttl == 0 && self.last_seen_ns == 0
    }

    /// Check if this entry has expired.
    fn is_expired(&self, now_ns: u64) -> bool {
        if self.last_seen_ns == 0 {
            return true;
        }
        let elapsed_s = now_ns.saturating_sub(self.last_seen_ns) / 1_000_000_000;
        elapsed_s > self.ttl as u64
    }
}

/// Neighbor table.
static NEIGHBORS: Mutex<Vec<Neighbor>> = Mutex::new(Vec::new());

/// Whether LLDP sending is enabled.
static TX_ENABLED: AtomicBool = AtomicBool::new(false);

/// Last TX timestamp (ns).
static LAST_TX_NS: AtomicU64 = AtomicU64::new(0);

/// Last tick timestamp (ns).
static LAST_TICK_NS: AtomicU64 = AtomicU64::new(0);

// Statistics.
static FRAMES_RECEIVED: AtomicU64 = AtomicU64::new(0);
static FRAMES_SENT: AtomicU64 = AtomicU64::new(0);
static NEIGHBORS_EXPIRED: AtomicU64 = AtomicU64::new(0);
static PARSE_ERRORS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Frame processing (receive)
// ---------------------------------------------------------------------------

/// Process an incoming LLDP frame.
///
/// Called from the Ethernet layer when EtherType == 0x88CC.
pub fn process_frame(src_mac: &MacAddress, payload: &[u8]) -> KernelResult<()> {
    FRAMES_RECEIVED.fetch_add(1, Ordering::Relaxed);

    let mut neighbor = Neighbor::new();
    neighbor.mac = *src_mac;
    neighbor.last_seen_ns = crate::hrtimer::now_ns();

    let mut offset = 0;
    while offset < payload.len() {
        let (tlv, consumed) = match parse_tlv(&payload[offset..]) {
            Some(t) => t,
            None => {
                PARSE_ERRORS.fetch_add(1, Ordering::Relaxed);
                break;
            }
        };

        if tlv.tlv_type == TLV_END {
            break; // End of LLDPDU.
        }

        match tlv.tlv_type {
            TLV_CHASSIS_ID => {
                if tlv.value.len() >= 2 {
                    let subtype = tlv.value[0];
                    match subtype {
                        CHASSIS_MAC_ADDRESS if tlv.value.len() >= 7 => {
                            neighbor.chassis_id = format!(
                                "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                                tlv.value[1], tlv.value[2], tlv.value[3],
                                tlv.value[4], tlv.value[5], tlv.value[6],
                            );
                        }
                        _ => {
                            // Other subtypes — render as UTF-8 or hex.
                            neighbor.chassis_id = render_bytes(&tlv.value[1..]);
                        }
                    }
                }
            }
            TLV_PORT_ID => {
                if tlv.value.len() >= 2 {
                    let subtype = tlv.value[0];
                    match subtype {
                        PORT_INTERFACE_NAME => {
                            neighbor.port_id = render_bytes(&tlv.value[1..]);
                        }
                        PORT_MAC_ADDRESS if tlv.value.len() >= 7 => {
                            neighbor.port_id = format!(
                                "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                                tlv.value[1], tlv.value[2], tlv.value[3],
                                tlv.value[4], tlv.value[5], tlv.value[6],
                            );
                        }
                        _ => {
                            neighbor.port_id = render_bytes(&tlv.value[1..]);
                        }
                    }
                }
            }
            TLV_TTL => {
                if tlv.value.len() >= 2 {
                    neighbor.ttl = u16::from_be_bytes([tlv.value[0], tlv.value[1]]);
                }
            }
            TLV_PORT_DESC => {
                neighbor.port_desc = render_bytes(tlv.value);
            }
            TLV_SYSTEM_NAME => {
                neighbor.system_name = render_bytes(tlv.value);
            }
            TLV_SYSTEM_DESC => {
                neighbor.system_desc = render_bytes(tlv.value);
            }
            TLV_SYSTEM_CAP => {
                if tlv.value.len() >= 4 {
                    neighbor.capabilities =
                        u16::from_be_bytes([tlv.value[0], tlv.value[1]]);
                    neighbor.enabled_caps =
                        u16::from_be_bytes([tlv.value[2], tlv.value[3]]);
                }
            }
            TLV_MGMT_ADDR => {
                // Management Address TLV:
                // Byte 0: addr string length (= 1 subtype + N addr bytes)
                // Byte 1: addr subtype (1=IPv4, 2=IPv6)
                // Bytes 2..2+N-1: address bytes
                if tlv.value.len() >= 6 {
                    let addr_str_len = tlv.value[0] as usize;
                    let addr_subtype = tlv.value[1];
                    // Validate: addr_str_len must be 5 for IPv4
                    // (1 subtype byte + 4 address bytes), and the TLV
                    // must actually contain that many bytes.
                    if addr_subtype == 1
                        && addr_str_len >= 5
                        && tlv.value.len() > addr_str_len
                    {
                        // IPv4.
                        neighbor.mgmt_ip = Ipv4Addr([
                            tlv.value[2], tlv.value[3],
                            tlv.value[4], tlv.value[5],
                        ]);
                    }
                }
            }
            _ => {
                // Unknown TLV — skip.
            }
        }

        offset = offset.saturating_add(consumed);
    }

    // TTL=0 means the neighbor is withdrawing (shutdown LLDPDU).
    if neighbor.ttl == 0 {
        remove_neighbor(&neighbor.mac);
        crate::serial_println!("[lldp] Neighbor {} withdrew", neighbor.mac);
        return Ok(());
    }

    // Update or insert neighbor.
    update_neighbor(neighbor);

    Ok(())
}

/// Render a byte slice as a UTF-8 string (best-effort).
fn render_bytes(data: &[u8]) -> String {
    // Try UTF-8 first; fall back to hex.
    match core::str::from_utf8(data) {
        Ok(s) => String::from(s),
        Err(_) => {
            let mut hex = String::with_capacity(data.len().saturating_mul(3));
            for (i, b) in data.iter().enumerate() {
                if i > 0 {
                    hex.push(':');
                }
                hex.push_str(&format!("{:02X}", b));
            }
            hex
        }
    }
}

/// Update an existing neighbor entry or insert a new one.
fn update_neighbor(new: Neighbor) {
    let mut neighbors = NEIGHBORS.lock();

    // Update existing entry.
    for entry in neighbors.iter_mut() {
        if entry.mac.0 == new.mac.0 {
            *entry = new;
            return;
        }
    }

    // Insert new entry.
    if neighbors.len() < MAX_NEIGHBORS {
        crate::serial_println!(
            "[lldp] New neighbor: {} ({})",
            new.mac,
            if new.system_name.is_empty() { &new.chassis_id } else { &new.system_name },
        );
        neighbors.push(new);
    } else {
        // Table full — evict the oldest.
        let mut oldest_idx = 0;
        let mut oldest_time = u64::MAX;
        for (i, entry) in neighbors.iter().enumerate() {
            if entry.last_seen_ns < oldest_time {
                oldest_time = entry.last_seen_ns;
                oldest_idx = i;
            }
        }
        if let Some(slot) = neighbors.get_mut(oldest_idx) {
            *slot = new;
        }
    }
}

/// Remove a neighbor by MAC address.
fn remove_neighbor(mac: &MacAddress) {
    let mut neighbors = NEIGHBORS.lock();
    neighbors.retain(|n| n.mac.0 != mac.0);
}

// ---------------------------------------------------------------------------
// Frame building (transmit)
// ---------------------------------------------------------------------------

/// Build an LLDP advertisement frame.
fn build_lldpdu() -> Vec<u8> {
    let our_mac = super::interface::mac();
    let our_ip = super::interface::ip();

    let mut payload = Vec::with_capacity(128);

    // Chassis ID TLV (subtype=MAC address).
    {
        let hdr = encode_tlv_header(TLV_CHASSIS_ID, 7);
        payload.extend_from_slice(&hdr);
        payload.push(CHASSIS_MAC_ADDRESS);
        payload.extend_from_slice(&our_mac.0);
    }

    // Port ID TLV (subtype=MAC address).
    {
        let hdr = encode_tlv_header(TLV_PORT_ID, 7);
        payload.extend_from_slice(&hdr);
        payload.push(PORT_MAC_ADDRESS);
        payload.extend_from_slice(&our_mac.0);
    }

    // TTL TLV.
    {
        let hdr = encode_tlv_header(TLV_TTL, 2);
        payload.extend_from_slice(&hdr);
        payload.extend_from_slice(&DEFAULT_TTL.to_be_bytes());
    }

    // System Name TLV.
    {
        let name = b"MintOS";
        let hdr = encode_tlv_header(TLV_SYSTEM_NAME, name.len());
        payload.extend_from_slice(&hdr);
        payload.extend_from_slice(name);
    }

    // System Description TLV.
    {
        let desc = b"MintOS Microkernel v0.1";
        let hdr = encode_tlv_header(TLV_SYSTEM_DESC, desc.len());
        payload.extend_from_slice(&hdr);
        payload.extend_from_slice(desc);
    }

    // System Capabilities TLV (station only).
    {
        let hdr = encode_tlv_header(TLV_SYSTEM_CAP, 4);
        payload.extend_from_slice(&hdr);
        payload.extend_from_slice(&CAP_STATION.to_be_bytes()); // Available.
        payload.extend_from_slice(&CAP_STATION.to_be_bytes()); // Enabled.
    }

    // Management Address TLV (IPv4, if configured).
    if !our_ip.is_unspecified() {
        // Addr string length = 1 (subtype) + 4 (IPv4) = 5.
        // Interface numbering subtype (2) + interface number (4) + OID length (1).
        let value_len = 1 + 1 + 4 + 1 + 4 + 1; // 12 bytes.
        let hdr = encode_tlv_header(TLV_MGMT_ADDR, value_len);
        payload.extend_from_slice(&hdr);
        payload.push(5);          // addr string length.
        payload.push(1);          // subtype: IPv4.
        payload.extend_from_slice(&our_ip.0);
        payload.push(2);          // interface numbering: ifIndex.
        payload.extend_from_slice(&1u32.to_be_bytes()); // interface number.
        payload.push(0);          // OID string length (none).
    }

    // End of LLDPDU TLV (type=0, length=0).
    {
        let hdr = encode_tlv_header(TLV_END, 0);
        payload.extend_from_slice(&hdr);
    }

    // Build Ethernet frame.
    ethernet::build_frame(&LLDP_MULTICAST_MAC, &our_mac, ETHERTYPE_LLDP, &payload)
}

/// Send an LLDP advertisement.
fn send_advertisement() -> KernelResult<()> {
    let frame = build_lldpdu();
    super::send_frame(&frame)?;
    FRAMES_SENT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Enable periodic LLDP advertisements.
pub fn enable() {
    TX_ENABLED.store(true, Ordering::Relaxed);
    // Send an immediate advertisement.
    let _ = send_advertisement();
    crate::serial_println!("[lldp] TX enabled (interval=30s, TTL={}s)", DEFAULT_TTL);
}

/// Disable LLDP advertisements.
pub fn disable() {
    TX_ENABLED.store(false, Ordering::Relaxed);
    crate::serial_println!("[lldp] TX disabled");
}

/// Check if LLDP TX is enabled.
#[allow(dead_code)] // Public API.
pub fn is_enabled() -> bool {
    TX_ENABLED.load(Ordering::Relaxed)
}

/// Get the current neighbor table.
pub fn neighbors() -> Vec<Neighbor> {
    let table = NEIGHBORS.lock();
    table.clone()
}

/// Clear the neighbor table.
pub fn clear_neighbors() {
    let mut table = NEIGHBORS.lock();
    table.clear();
}

// ---------------------------------------------------------------------------
// Periodic tick
// ---------------------------------------------------------------------------

/// Periodic LLDP maintenance.
///
/// - Expires stale neighbors.
/// - Sends periodic advertisements (if enabled).
pub fn tick() {
    let now = crate::hrtimer::now_ns();
    let last = LAST_TICK_NS.load(Ordering::Relaxed);
    if now.saturating_sub(last) < TICK_INTERVAL_NS {
        return;
    }
    LAST_TICK_NS.store(now, Ordering::Relaxed);

    // Expire stale neighbors.
    {
        let mut table = NEIGHBORS.lock();
        let before = table.len();
        table.retain(|n| !n.is_expired(now));
        let expired = before.saturating_sub(table.len());
        if expired > 0 {
            NEIGHBORS_EXPIRED.fetch_add(expired as u64, Ordering::Relaxed);
        }
    }

    // Send periodic advertisement.
    if TX_ENABLED.load(Ordering::Relaxed) {
        let last_tx = LAST_TX_NS.load(Ordering::Relaxed);
        if now.saturating_sub(last_tx) >= TX_INTERVAL_NS {
            LAST_TX_NS.store(now, Ordering::Relaxed);
            let _ = send_advertisement();
        }
    }
}

// ---------------------------------------------------------------------------
// Capabilities formatting
// ---------------------------------------------------------------------------

/// Format system capabilities as a human-readable string.
pub fn format_capabilities(caps: u16) -> String {
    let mut parts = Vec::new();
    if caps & CAP_OTHER != 0 { parts.push("Other"); }
    if caps & CAP_REPEATER != 0 { parts.push("Repeater"); }
    if caps & CAP_BRIDGE != 0 { parts.push("Bridge"); }
    if caps & CAP_ROUTER != 0 { parts.push("Router"); }
    if caps & CAP_STATION != 0 { parts.push("Station"); }
    if parts.is_empty() {
        String::from("none")
    } else {
        let mut s = String::new();
        for (i, p) in parts.iter().enumerate() {
            if i > 0 { s.push_str(", "); }
            s.push_str(p);
        }
        s
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// LLDP statistics.
#[derive(Debug)]
pub struct LldpStats {
    pub tx_enabled: bool,
    pub neighbor_count: usize,
    pub frames_received: u64,
    pub frames_sent: u64,
    pub neighbors_expired: u64,
    pub parse_errors: u64,
}

/// Get LLDP statistics.
pub fn stats() -> LldpStats {
    let table = NEIGHBORS.lock();
    LldpStats {
        tx_enabled: TX_ENABLED.load(Ordering::Relaxed),
        neighbor_count: table.len(),
        frames_received: FRAMES_RECEIVED.load(Ordering::Relaxed),
        frames_sent: FRAMES_SENT.load(Ordering::Relaxed),
        neighbors_expired: NEIGHBORS_EXPIRED.load(Ordering::Relaxed),
        parse_errors: PARSE_ERRORS.load(Ordering::Relaxed),
    }
}

/// Generate procfs content for `/proc/lldp`.
pub fn procfs_content() -> String {
    let s = stats();
    let nbrs = neighbors();

    let mut out = String::with_capacity(512);
    out.push_str("LLDP (Link Layer Discovery Protocol)\n");
    out.push_str("====================================\n\n");
    out.push_str(&format!("TX enabled:       {}\n", if s.tx_enabled { "yes" } else { "no" }));
    out.push_str(&format!("Neighbors:        {}\n", s.neighbor_count));
    out.push_str(&format!("Frames received:  {}\n", s.frames_received));
    out.push_str(&format!("Frames sent:      {}\n", s.frames_sent));
    out.push_str(&format!("Expired:          {}\n", s.neighbors_expired));
    out.push_str(&format!("Parse errors:     {}\n", s.parse_errors));

    if !nbrs.is_empty() {
        out.push_str("\nNeighbors:\n");
        for n in &nbrs {
            out.push_str(&format!("  MAC: {}\n", n.mac));
            if !n.system_name.is_empty() {
                out.push_str(&format!("    Name: {}\n", n.system_name));
            }
            if !n.chassis_id.is_empty() {
                out.push_str(&format!("    Chassis: {}\n", n.chassis_id));
            }
            if !n.port_id.is_empty() {
                out.push_str(&format!("    Port: {}\n", n.port_id));
            }
            if n.capabilities != 0 {
                out.push_str(&format!(
                    "    Capabilities: {} (enabled: {})\n",
                    format_capabilities(n.capabilities),
                    format_capabilities(n.enabled_caps),
                ));
            }
            if !n.mgmt_ip.is_unspecified() {
                out.push_str(&format!("    Mgmt IP: {}\n", n.mgmt_ip));
            }
            out.push_str(&format!("    TTL: {}s\n", n.ttl));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run LLDP self-tests.
// Self-tests deliberately runtime-assert RFC-defined constants
// (TLV type codes, EtherType) as living documentation.
#[allow(clippy::assertions_on_constants)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[lldp] Running LLDP self-tests...");
    let mut passed = 0u32;

    // --- Test 1: TLV header encoding ---
    {
        // Type=1 (Chassis ID), Length=7.
        let hdr = encode_tlv_header(1, 7);
        let decoded = u16::from_be_bytes(hdr);
        let t = (decoded >> 9) as u8;
        let l = (decoded & 0x01FF) as usize;
        assert!(t == 1, "type");
        assert!(l == 7, "length");

        passed = passed.saturating_add(1);
        crate::serial_println!("[lldp]   test 1 (TLV header encoding) PASSED");
    }

    // --- Test 2: TLV parsing ---
    {
        // Build a TLV: type=5 (System Name), value="test".
        let hdr = encode_tlv_header(5, 4);
        let mut data = Vec::from(&hdr[..]);
        data.extend_from_slice(b"test");

        let (tlv, consumed) = parse_tlv(&data).unwrap();
        assert!(tlv.tlv_type == 5, "parsed type");
        assert!(tlv.value == b"test", "parsed value");
        assert!(consumed == 6, "consumed bytes");

        passed = passed.saturating_add(1);
        crate::serial_println!("[lldp]   test 2 (TLV parsing) PASSED");
    }

    // --- Test 3: End-of-LLDPDU ---
    {
        let hdr = encode_tlv_header(TLV_END, 0);
        let (tlv, consumed) = parse_tlv(&hdr).unwrap();
        assert!(tlv.tlv_type == 0, "end type");
        assert!(tlv.value.is_empty(), "end empty");
        assert!(consumed == 2, "end consumed");

        passed = passed.saturating_add(1);
        crate::serial_println!("[lldp]   test 3 (End-of-LLDPDU) PASSED");
    }

    // --- Test 4: Constants ---
    {
        assert!(ETHERTYPE_LLDP == 0x88CC, "ethertype");
        assert!(DEFAULT_TTL == 120, "default TTL");
        assert!(MAX_NEIGHBORS == 16, "max neighbors");
        assert!(TLV_CHASSIS_ID == 1, "chassis ID type");
        assert!(TLV_PORT_ID == 2, "port ID type");
        assert!(TLV_TTL == 3, "TTL type");
        assert!(TLV_SYSTEM_NAME == 5, "system name type");

        passed = passed.saturating_add(1);
        crate::serial_println!("[lldp]   test 4 (constants) PASSED");
    }

    // --- Test 5: Capabilities formatting ---
    {
        let s = format_capabilities(CAP_BRIDGE | CAP_ROUTER);
        assert!(s.contains("Bridge"), "bridge cap");
        assert!(s.contains("Router"), "router cap");

        let s2 = format_capabilities(0);
        assert!(s2 == "none", "no caps");

        let s3 = format_capabilities(CAP_STATION);
        assert!(s3 == "Station", "station only");

        passed = passed.saturating_add(1);
        crate::serial_println!("[lldp]   test 5 (capabilities formatting) PASSED");
    }

    // --- Test 6: render_bytes ---
    {
        let s = render_bytes(b"Hello");
        assert!(s == "Hello", "ASCII render");

        let hex = render_bytes(&[0xFF, 0x00, 0xAB]);
        assert!(hex.contains("FF"), "hex render");

        passed = passed.saturating_add(1);
        crate::serial_println!("[lldp]   test 6 (render_bytes) PASSED");
    }

    // --- Test 7: LLDPDU construction ---
    {
        let frame = build_lldpdu();
        // Should be an Ethernet frame (at least 14 bytes header).
        assert!(frame.len() >= 14, "frame size");

        // Destination MAC = LLDP multicast.
        assert!(frame[0] == 0x01, "dst mac 0");
        assert!(frame[1] == 0x80, "dst mac 1");
        assert!(frame[2] == 0xC2, "dst mac 2");
        assert!(frame[3] == 0x00, "dst mac 3");
        assert!(frame[4] == 0x00, "dst mac 4");
        assert!(frame[5] == 0x0E, "dst mac 5");

        // EtherType = 0x88CC.
        assert!(frame[12] == 0x88, "ethertype high");
        assert!(frame[13] == 0xCC, "ethertype low");

        passed = passed.saturating_add(1);
        crate::serial_println!("[lldp]   test 7 (LLDPDU construction) PASSED");
    }

    // --- Test 8: TLV round-trip ---
    {
        // Encode several TLV types and verify parsing.
        for tlv_type in [1u8, 3, 5, 7] {
            let value = b"data";
            let hdr = encode_tlv_header(tlv_type, value.len());
            let mut buf = Vec::from(&hdr[..]);
            buf.extend_from_slice(value);

            let (parsed, _) = parse_tlv(&buf).unwrap();
            assert!(parsed.tlv_type == tlv_type, "round-trip type");
            assert!(parsed.value == value, "round-trip value");
        }

        passed = passed.saturating_add(1);
        crate::serial_println!("[lldp]   test 8 (TLV round-trip) PASSED");
    }

    // --- Test 9: Stats accessible ---
    {
        let s = stats();
        assert!(s.neighbor_count <= MAX_NEIGHBORS, "count valid");

        passed = passed.saturating_add(1);
        crate::serial_println!("[lldp]   test 9 (stats) PASSED");
    }

    // --- Test 10: Procfs content ---
    {
        let content = procfs_content();
        assert!(content.contains("LLDP"), "header");
        assert!(content.contains("TX enabled:"), "tx field");
        assert!(content.contains("Neighbors:"), "neighbors field");
        assert!(content.contains("Frames received:"), "rx field");

        passed = passed.saturating_add(1);
        crate::serial_println!("[lldp]   test 10 (procfs content) PASSED");
    }

    crate::serial_println!("[lldp] All {} self-tests PASSED", passed);
    Ok(())
}
