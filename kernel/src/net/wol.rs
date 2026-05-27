//! Wake-on-LAN (WoL) magic packet sender.
//!
//! Sends magic packets to wake up sleeping machines on the local network.
//! A magic packet is a UDP broadcast frame containing 6 bytes of 0xFF
//! followed by 16 repetitions of the target machine's MAC address (102
//! bytes total payload).
//!
//! ## Usage
//!
//! ```text
//! wol AA:BB:CC:DD:EE:FF        — send magic packet to wake machine
//! wol AA:BB:CC:DD:EE:FF 10.0.0.255  — send to specific broadcast address
//! ```
//!
//! ## Protocol
//!
//! WoL uses UDP port 9 (or 7).  The magic packet can be sent as a
//! Layer 2 broadcast or a Layer 3 subnet-directed broadcast.  This
//! implementation uses UDP broadcast for maximum compatibility.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::KernelResult;
use super::interface::Ipv4Addr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Standard WoL UDP port.
const WOL_PORT: u16 = 9;

/// Magic packet header: 6 bytes of 0xFF.
const MAGIC_HEADER: [u8; 6] = [0xFF; 6];

/// Number of MAC repetitions in the magic packet.
const MAC_REPETITIONS: usize = 16;

/// Total magic packet payload size: 6 + (16 × 6) = 102 bytes.
const MAGIC_PACKET_SIZE: usize = 6 + MAC_REPETITIONS * 6;

/// Default broadcast address.
const BROADCAST_IP: Ipv4Addr = Ipv4Addr([255, 255, 255, 255]);

// Statistics.
static PACKETS_SENT: AtomicU64 = AtomicU64::new(0);
static SEND_ERRORS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// MAC address type
// ---------------------------------------------------------------------------

/// A 6-byte MAC (Ethernet) address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacAddress(pub [u8; 6]);

impl MacAddress {
    /// Parse a MAC address from "AA:BB:CC:DD:EE:FF" or "AA-BB-CC-DD-EE-FF".
    pub fn parse(s: &str) -> Option<Self> {
        let sep = if s.contains(':') { ':' } else { '-' };
        let parts: Vec<&str> = s.split(sep).collect();
        if parts.len() != 6 {
            return None;
        }
        let mut bytes = [0u8; 6];
        for (i, part) in parts.iter().enumerate() {
            bytes[i] = u8::from_str_radix(part, 16).ok()?;
        }
        Some(MacAddress(bytes))
    }
}

impl core::fmt::Display for MacAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.0[0], self.0[1], self.0[2],
            self.0[3], self.0[4], self.0[5],
        )
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build a Wake-on-LAN magic packet payload.
///
/// The payload is 102 bytes: 6 × 0xFF followed by the target MAC
/// repeated 16 times.
pub fn build_magic_packet(target_mac: MacAddress) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(MAGIC_PACKET_SIZE);
    pkt.extend_from_slice(&MAGIC_HEADER);
    for _ in 0..MAC_REPETITIONS {
        pkt.extend_from_slice(&target_mac.0);
    }
    pkt
}

/// Send a Wake-on-LAN magic packet to wake a machine.
///
/// Sends the magic packet as a UDP broadcast on the standard WoL port.
/// Optionally specify a directed broadcast address (e.g., 10.0.0.255
/// for a /24 subnet).
pub fn wake(target_mac: MacAddress, broadcast_ip: Option<Ipv4Addr>) -> KernelResult<()> {
    let dest = broadcast_ip.unwrap_or(BROADCAST_IP);
    let payload = build_magic_packet(target_mac);

    // Use an ephemeral port as source.
    let src_port = WOL_PORT;
    match super::udp::send(src_port, dest, WOL_PORT, &payload) {
        Ok(()) => {
            PACKETS_SENT.fetch_add(1, Ordering::Relaxed);
            crate::serial_println!(
                "[wol] Magic packet sent to {} via {}",
                target_mac, dest
            );
            Ok(())
        }
        Err(e) => {
            SEND_ERRORS.fetch_add(1, Ordering::Relaxed);
            Err(e)
        }
    }
}

/// Get Wake-on-LAN statistics.
pub fn stats() -> (u64, u64) {
    (
        PACKETS_SENT.load(Ordering::Relaxed),
        SEND_ERRORS.load(Ordering::Relaxed),
    )
}

/// Generate procfs content for `/proc/wol`.
pub fn procfs_content() -> String {
    let (sent, errors) = stats();
    let mut out = String::with_capacity(128);
    out.push_str("Wake-on-LAN\n");
    out.push_str("===========\n\n");
    out.push_str(&format!("Packets sent: {}\n", sent));
    out.push_str(&format!("Send errors:  {}\n", errors));
    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run Wake-on-LAN self-tests.
// Self-tests deliberately runtime-assert magic-packet format
// constants as living documentation.
#[allow(clippy::assertions_on_constants)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[wol] Running WoL self-tests...");
    let mut passed = 0u32;

    // --- Test 1: MAC address parsing ---
    {
        let mac = MacAddress::parse("AA:BB:CC:DD:EE:FF").unwrap();
        assert!(mac.0 == [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF], "parse colon");

        let mac2 = MacAddress::parse("01-23-45-67-89-ab").unwrap();
        assert!(mac2.0 == [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB], "parse dash");

        assert!(MacAddress::parse("invalid").is_none(), "reject invalid");
        assert!(MacAddress::parse("AA:BB:CC").is_none(), "reject short");
        assert!(MacAddress::parse("AA:BB:CC:DD:EE:FF:00").is_none(), "reject long");
        assert!(MacAddress::parse("GG:HH:II:JJ:KK:LL").is_none(), "reject non-hex");

        passed = passed.saturating_add(1);
        crate::serial_println!("[wol]   test 1 (MAC parsing) PASSED");
    }

    // --- Test 2: MAC address formatting ---
    {
        let mac = MacAddress([0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]);
        let s = format!("{}", mac);
        assert!(s == "12:34:56:78:9A:BC", "format MAC");

        passed = passed.saturating_add(1);
        crate::serial_println!("[wol]   test 2 (MAC formatting) PASSED");
    }

    // --- Test 3: Magic packet construction ---
    {
        let mac = MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let pkt = build_magic_packet(mac);

        assert!(pkt.len() == MAGIC_PACKET_SIZE, "packet size");
        assert!(pkt.len() == 102, "102 bytes");

        // Check header.
        for i in 0..6 {
            assert!(*pkt.get(i).unwrap_or(&0) == 0xFF, "header byte");
        }

        // Check MAC repetitions.
        for rep in 0..MAC_REPETITIONS {
            let offset = 6 + rep * 6;
            assert!(pkt.get(offset..offset + 6) == Some(&mac.0[..]), "MAC repetition");
        }

        passed = passed.saturating_add(1);
        crate::serial_println!("[wol]   test 3 (magic packet construction) PASSED");
    }

    // --- Test 4: All-zeros MAC ---
    {
        let mac = MacAddress([0, 0, 0, 0, 0, 0]);
        let pkt = build_magic_packet(mac);
        assert!(pkt.len() == 102, "size ok");
        // First 6 bytes are 0xFF, rest are all zeros.
        for i in 6..102 {
            assert!(*pkt.get(i).unwrap_or(&0xFF) == 0, "zero MAC");
        }

        passed = passed.saturating_add(1);
        crate::serial_println!("[wol]   test 4 (all-zeros MAC) PASSED");
    }

    // --- Test 5: Constants ---
    {
        assert!(WOL_PORT == 9, "WoL port");
        assert!(MAGIC_PACKET_SIZE == 102, "magic packet size");
        assert!(MAC_REPETITIONS == 16, "16 repetitions");

        passed = passed.saturating_add(1);
        crate::serial_println!("[wol]   test 5 (constants) PASSED");
    }

    // --- Test 6: MAC parse round-trip ---
    {
        let original = "DE:AD:BE:EF:CA:FE";
        let mac = MacAddress::parse(original).unwrap();
        let formatted = format!("{}", mac);
        assert!(formatted == original, "round-trip");

        passed = passed.saturating_add(1);
        crate::serial_println!("[wol]   test 6 (MAC round-trip) PASSED");
    }

    crate::serial_println!("[wol] All {} self-tests PASSED", passed);
    Ok(())
}
