//! VLAN — IEEE 802.1Q Virtual LAN support.
//!
//! Handles VLAN-tagged Ethernet frames (802.1Q), allowing the
//! networking stack to participate in VLANs.
//!
//! ## Features
//!
//! - Parse and strip 802.1Q VLAN tags from incoming frames
//! - Add 802.1Q tags to outgoing frames
//! - VLAN interface management (up to 16 VLANs)
//! - Priority Code Point (PCP) / QoS support
//! - VLAN statistics tracking
//!
//! ## Frame format
//!
//! Standard Ethernet frame with 802.1Q tag inserted:
//! ```text
//! [Dst MAC 6B][Src MAC 6B][0x8100 2B][TCI 2B][EtherType 2B][Payload...]
//! ```
//!
//! TCI (Tag Control Information):
//! ```text
//! [PCP 3bits][DEI 1bit][VID 12bits]
//! ```

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// 802.1Q EtherType (Tag Protocol Identifier).
pub const ETHERTYPE_8021Q: u16 = 0x8100;

/// 802.1ad (QinQ) EtherType.
#[allow(dead_code)] // Public API.
pub const ETHERTYPE_QINQ: u16 = 0x88A8;

/// Maximum number of configured VLANs.
const MAX_VLANS: usize = 16;

/// Maximum VLAN ID.
const MAX_VLAN_ID: u16 = 4094;

/// Default VLAN (native/untagged traffic).
#[allow(dead_code)] // Public API.
pub const DEFAULT_VLAN: u16 = 1;

// ---------------------------------------------------------------------------
// VLAN tag
// ---------------------------------------------------------------------------

/// Parsed 802.1Q VLAN tag.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Public API.
pub struct VlanTag {
    /// Priority Code Point (0-7).
    pub pcp: u8,
    /// Drop Eligible Indicator.
    pub dei: bool,
    /// VLAN Identifier (0-4094).
    pub vid: u16,
}

impl VlanTag {
    /// Create a new VLAN tag with just a VID.
    #[allow(dead_code)] // Public API.
    pub const fn new(vid: u16) -> Self {
        Self {
            pcp: 0,
            dei: false,
            vid,
        }
    }

    /// Create a VLAN tag with PCP priority.
    #[allow(dead_code)] // Public API.
    pub const fn with_priority(vid: u16, pcp: u8) -> Self {
        Self {
            pcp,
            dei: false,
            vid,
        }
    }

    /// Encode the TCI (Tag Control Information) as 2 bytes.
    #[allow(dead_code)] // Public API.
    pub fn encode_tci(&self) -> [u8; 2] {
        let pcp = (self.pcp & 0x07) as u16;
        let dei = if self.dei { 1u16 } else { 0 };
        let vid = self.vid & 0x0FFF;
        let tci = (pcp << 13) | (dei << 12) | vid;
        tci.to_be_bytes()
    }

    /// Decode TCI from 2 bytes.
    #[allow(dead_code)] // Public API.
    pub fn decode_tci(bytes: [u8; 2]) -> Self {
        let tci = u16::from_be_bytes(bytes);
        Self {
            pcp: ((tci >> 13) & 0x07) as u8,
            dei: (tci >> 12) & 1 == 1,
            vid: tci & 0x0FFF,
        }
    }
}

// ---------------------------------------------------------------------------
// VLAN interface configuration
// ---------------------------------------------------------------------------

/// A configured VLAN interface.
#[derive(Debug, Clone)]
struct VlanInterface {
    /// VLAN ID.
    vid: u16,
    /// Whether this interface is active.
    active: bool,
    /// Optional name/description.
    name: [u8; 32],
    /// Name length.
    name_len: usize,
    /// Frames received on this VLAN.
    rx_frames: u64,
    /// Frames sent on this VLAN.
    tx_frames: u64,
    /// Bytes received.
    rx_bytes: u64,
    /// Bytes sent.
    tx_bytes: u64,
}

impl VlanInterface {
    const fn empty() -> Self {
        Self {
            vid: 0,
            active: false,
            name: [0u8; 32],
            name_len: 0,
            rx_frames: 0,
            tx_frames: 0,
            rx_bytes: 0,
            tx_bytes: 0,
        }
    }

    fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }
}

/// VLAN interface table.
static VLANS: Mutex<[VlanInterface; MAX_VLANS]> = Mutex::new([const { VlanInterface::empty() }; MAX_VLANS]);

// Statistics.
static TAGGED_RX: AtomicU64 = AtomicU64::new(0);
static TAGGED_TX: AtomicU64 = AtomicU64::new(0);
static UNTAGGED_RX: AtomicU64 = AtomicU64::new(0);
static UNKNOWN_VLAN_DROPS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Frame processing
// ---------------------------------------------------------------------------

/// Result of stripping a VLAN tag from a frame.
#[derive(Debug)]
#[allow(dead_code)] // Public API.
pub struct UntaggedFrame {
    /// The VLAN tag that was removed.
    pub tag: VlanTag,
    /// The original EtherType (from after the 802.1Q header).
    pub ethertype: u16,
    /// The frame with the VLAN tag stripped (standard Ethernet frame).
    pub frame: Vec<u8>,
}

/// Check if a frame has an 802.1Q VLAN tag.
///
/// Looks at bytes 12-13 of the Ethernet frame for TPID 0x8100.
#[allow(dead_code)] // Public API.
pub fn is_tagged(frame: &[u8]) -> bool {
    if frame.len() < 18 {
        return false;
    }
    let tpid = u16::from_be_bytes([frame[12], frame[13]]);
    tpid == ETHERTYPE_8021Q
}

/// Strip the 802.1Q tag from a tagged frame.
///
/// Returns the VLAN tag and reconstructed untagged frame.
/// The returned frame has: [Dst 6B][Src 6B][Inner EtherType 2B][Payload...].
#[allow(dead_code)] // Public API.
pub fn strip_tag(frame: &[u8]) -> Option<UntaggedFrame> {
    if frame.len() < 18 {
        return None;
    }

    let tpid = u16::from_be_bytes([frame[12], frame[13]]);
    if tpid != ETHERTYPE_8021Q {
        return None;
    }

    let tci = [frame[14], frame[15]];
    let tag = VlanTag::decode_tci(tci);
    let ethertype = u16::from_be_bytes([frame[16], frame[17]]);

    TAGGED_RX.fetch_add(1, Ordering::Relaxed);

    // Rebuild frame without the 802.1Q header:
    // [Dst 6B][Src 6B][EtherType 2B][Payload from byte 18...]
    let mut untagged = Vec::with_capacity(frame.len() - 4);
    untagged.extend_from_slice(&frame[..12]); // Dst + Src MACs.
    untagged.extend_from_slice(&frame[16..]); // EtherType + Payload.

    // Update per-VLAN stats.
    {
        let mut vlans = VLANS.lock();
        for vlan in vlans.iter_mut() {
            if vlan.active && vlan.vid == tag.vid {
                vlan.rx_frames = vlan.rx_frames.saturating_add(1);
                vlan.rx_bytes = vlan.rx_bytes.saturating_add(frame.len() as u64);
                break;
            }
        }
    }

    Some(UntaggedFrame {
        tag,
        ethertype,
        frame: untagged,
    })
}

/// Add an 802.1Q tag to an untagged frame.
///
/// Inserts the VLAN header after the source MAC.
/// Input: [Dst 6B][Src 6B][EtherType 2B][Payload...].
/// Output: [Dst 6B][Src 6B][0x8100 2B][TCI 2B][EtherType 2B][Payload...].
#[allow(dead_code)] // Public API.
pub fn add_tag(frame: &[u8], tag: &VlanTag) -> Vec<u8> {
    if frame.len() < 14 {
        return frame.to_vec();
    }

    TAGGED_TX.fetch_add(1, Ordering::Relaxed);

    let tci = tag.encode_tci();
    let tpid = ETHERTYPE_8021Q.to_be_bytes();

    let mut tagged = Vec::with_capacity(frame.len() + 4);
    tagged.extend_from_slice(&frame[..12]); // Dst + Src MACs.
    tagged.extend_from_slice(&tpid);        // TPID 0x8100.
    tagged.extend_from_slice(&tci);         // TCI (PCP + DEI + VID).
    tagged.extend_from_slice(&frame[12..]); // Original EtherType + Payload.

    // Update per-VLAN stats.
    {
        let mut vlans = VLANS.lock();
        for vlan in vlans.iter_mut() {
            if vlan.active && vlan.vid == tag.vid {
                vlan.tx_frames = vlan.tx_frames.saturating_add(1);
                vlan.tx_bytes = vlan.tx_bytes.saturating_add(tagged.len() as u64);
                break;
            }
        }
    }

    tagged
}

// ---------------------------------------------------------------------------
// VLAN management
// ---------------------------------------------------------------------------

/// Add a VLAN interface.
#[allow(dead_code)] // Public API.
pub fn add_vlan(vid: u16, name: &str) -> KernelResult<()> {
    if vid == 0 || vid > MAX_VLAN_ID {
        return Err(KernelError::InvalidArgument);
    }

    let mut vlans = VLANS.lock();

    // Check if already exists.
    for vlan in vlans.iter() {
        if vlan.active && vlan.vid == vid {
            return Err(KernelError::AlreadyExists);
        }
    }

    // Find empty slot.
    for vlan in vlans.iter_mut() {
        if !vlan.active {
            vlan.vid = vid;
            vlan.active = true;
            vlan.rx_frames = 0;
            vlan.tx_frames = 0;
            vlan.rx_bytes = 0;
            vlan.tx_bytes = 0;

            let name_bytes = name.as_bytes();
            let copy_len = name_bytes.len().min(32);
            vlan.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
            vlan.name_len = copy_len;

            return Ok(());
        }
    }

    Err(KernelError::OutOfMemory) // No slots available.
}

/// Remove a VLAN interface.
#[allow(dead_code)] // Public API.
pub fn remove_vlan(vid: u16) -> KernelResult<()> {
    let mut vlans = VLANS.lock();
    for vlan in vlans.iter_mut() {
        if vlan.active && vlan.vid == vid {
            vlan.active = false;
            return Ok(());
        }
    }
    Err(KernelError::NotFound)
}

/// List all configured VLANs.
#[allow(dead_code)] // Public API.
pub fn list_vlans() -> Vec<VlanInfo> {
    let vlans = VLANS.lock();
    let mut result = Vec::new();
    for vlan in vlans.iter() {
        if vlan.active {
            result.push(VlanInfo {
                vid: vlan.vid,
                name: String::from(vlan.name_str()),
                rx_frames: vlan.rx_frames,
                tx_frames: vlan.tx_frames,
                rx_bytes: vlan.rx_bytes,
                tx_bytes: vlan.tx_bytes,
            });
        }
    }
    result
}

/// VLAN interface info (for display).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API.
pub struct VlanInfo {
    pub vid: u16,
    pub name: String,
    pub rx_frames: u64,
    pub tx_frames: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

/// Check if a VLAN is configured.
#[allow(dead_code)] // Public API.
pub fn is_vlan_configured(vid: u16) -> bool {
    let vlans = VLANS.lock();
    vlans.iter().any(|v| v.active && v.vid == vid)
}

// ---------------------------------------------------------------------------
// PCP priority mapping
// ---------------------------------------------------------------------------

/// PCP (Priority Code Point) descriptions.
#[allow(dead_code)] // Public API.
pub fn pcp_name(pcp: u8) -> &'static str {
    match pcp {
        0 => "Best Effort (default)",
        1 => "Background",
        2 => "Excellent Effort",
        3 => "Critical Applications",
        4 => "Video (< 100ms latency)",
        5 => "Voice (< 10ms latency)",
        6 => "Internetwork Control",
        7 => "Network Control",
        _ => "Unknown",
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// VLAN statistics.
#[derive(Debug)]
#[allow(dead_code)] // Public API.
pub struct VlanStats {
    pub configured_vlans: usize,
    pub tagged_rx: u64,
    pub tagged_tx: u64,
    pub untagged_rx: u64,
    pub unknown_vlan_drops: u64,
}

/// Get VLAN statistics.
#[allow(dead_code)] // Public API.
pub fn stats() -> VlanStats {
    let vlans = VLANS.lock();
    let configured = vlans.iter().filter(|v| v.active).count();
    VlanStats {
        configured_vlans: configured,
        tagged_rx: TAGGED_RX.load(Ordering::Relaxed),
        tagged_tx: TAGGED_TX.load(Ordering::Relaxed),
        untagged_rx: UNTAGGED_RX.load(Ordering::Relaxed),
        unknown_vlan_drops: UNKNOWN_VLAN_DROPS.load(Ordering::Relaxed),
    }
}

/// Generate procfs content for `/proc/vlan`.
#[allow(dead_code)] // Public API.
pub fn procfs_content() -> String {
    let s = stats();
    let vlans = list_vlans();
    let mut out = String::with_capacity(512);
    out.push_str("VLAN (802.1Q)\n");
    out.push_str("=============\n\n");
    out.push_str(&format!("Configured VLANs: {}\n", s.configured_vlans));
    out.push_str(&format!("Tagged RX:        {}\n", s.tagged_rx));
    out.push_str(&format!("Tagged TX:        {}\n", s.tagged_tx));
    out.push_str(&format!("Untagged RX:      {}\n", s.untagged_rx));
    out.push_str(&format!("Unknown drops:    {}\n", s.unknown_vlan_drops));

    if !vlans.is_empty() {
        out.push_str("\nVLAN Interfaces:\n");
        for v in &vlans {
            out.push_str(&format!(
                "  VLAN {} ({}): RX {}/{} TX {}/{}\n",
                v.vid, v.name, v.rx_frames, v.rx_bytes, v.tx_frames, v.tx_bytes,
            ));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run VLAN self-tests.
#[allow(dead_code)] // Public API.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[vlan] Running VLAN self-tests...");
    let mut passed = 0u32;

    // --- Test 1: TCI encode/decode ---
    {
        let tag = VlanTag::new(100);
        let tci = tag.encode_tci();
        let decoded = VlanTag::decode_tci(tci);
        assert!(decoded.vid == 100, "vid round-trip");
        assert!(decoded.pcp == 0, "pcp default");
        assert!(!decoded.dei, "dei default");

        passed = passed.saturating_add(1);
        crate::serial_println!("[vlan]   test 1 (TCI encode/decode) PASSED");
    }

    // --- Test 2: TCI with priority ---
    {
        let tag = VlanTag::with_priority(200, 5);
        let tci = tag.encode_tci();
        let decoded = VlanTag::decode_tci(tci);
        assert!(decoded.vid == 200, "vid");
        assert!(decoded.pcp == 5, "pcp");

        passed = passed.saturating_add(1);
        crate::serial_println!("[vlan]   test 2 (TCI priority) PASSED");
    }

    // --- Test 3: TCI with DEI ---
    {
        let tag = VlanTag { pcp: 3, dei: true, vid: 4094 };
        let tci = tag.encode_tci();
        let decoded = VlanTag::decode_tci(tci);
        assert!(decoded.vid == 4094, "max vid");
        assert!(decoded.pcp == 3, "pcp");
        assert!(decoded.dei, "dei");

        passed = passed.saturating_add(1);
        crate::serial_println!("[vlan]   test 3 (TCI DEI) PASSED");
    }

    // --- Test 4: Frame tagging ---
    {
        // Build a minimal untagged frame: [6 dst][6 src][2 ethertype][payload].
        let mut frame = vec![0u8; 20];
        frame[12] = 0x08; // EtherType 0x0800 (IPv4).
        frame[13] = 0x00;
        frame[14..20].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE]);

        let tag = VlanTag::new(42);
        let tagged = add_tag(&frame, &tag);
        assert!(tagged.len() == frame.len() + 4, "tagged length");
        assert!(tagged[12] == 0x81, "TPID high");
        assert!(tagged[13] == 0x00, "TPID low");
        // Original EtherType should be at offset 16.
        assert!(tagged[16] == 0x08, "inner ethertype high");
        assert!(tagged[17] == 0x00, "inner ethertype low");

        passed = passed.saturating_add(1);
        crate::serial_println!("[vlan]   test 4 (frame tagging) PASSED");
    }

    // --- Test 5: Frame untagging ---
    {
        // Build a tagged frame.
        let mut tagged = vec![0u8; 24];
        tagged[12] = 0x81; // TPID 0x8100.
        tagged[13] = 0x00;
        let tag = VlanTag::new(42);
        let tci = tag.encode_tci();
        tagged[14] = tci[0];
        tagged[15] = tci[1];
        tagged[16] = 0x08; // Inner EtherType.
        tagged[17] = 0x00;

        assert!(is_tagged(&tagged), "detect tag");

        let result = strip_tag(&tagged);
        assert!(result.is_some(), "strip tag");
        let untagged = result.unwrap();
        assert!(untagged.tag.vid == 42, "stripped vid");
        assert!(untagged.ethertype == 0x0800, "inner ethertype");
        assert!(untagged.frame.len() == tagged.len() - 4, "untagged length");

        passed = passed.saturating_add(1);
        crate::serial_println!("[vlan]   test 5 (frame untagging) PASSED");
    }

    // --- Test 6: Untagged frame detection ---
    {
        let mut frame = vec![0u8; 20];
        frame[12] = 0x08;
        frame[13] = 0x00;
        assert!(!is_tagged(&frame), "not tagged");

        // Too short.
        assert!(!is_tagged(&[0u8; 10]), "too short");

        passed = passed.saturating_add(1);
        crate::serial_println!("[vlan]   test 6 (untagged detection) PASSED");
    }

    // --- Test 7: VLAN management ---
    {
        let _ = remove_vlan(999); // Clean up in case of prior test.

        assert!(add_vlan(999, "test-vlan").is_ok(), "add vlan");
        assert!(is_vlan_configured(999), "is configured");

        // Duplicate should fail.
        assert!(add_vlan(999, "dup").is_err(), "duplicate");

        // Remove.
        assert!(remove_vlan(999).is_ok(), "remove");
        assert!(!is_vlan_configured(999), "not configured after remove");

        // Remove non-existent.
        assert!(remove_vlan(999).is_err(), "remove non-existent");

        passed = passed.saturating_add(1);
        crate::serial_println!("[vlan]   test 7 (VLAN management) PASSED");
    }

    // --- Test 8: Invalid VLAN IDs ---
    {
        assert!(add_vlan(0, "zero").is_err(), "vid 0");
        assert!(add_vlan(4095, "too-high").is_err(), "vid 4095");
        assert!(add_vlan(4094, "max").is_ok(), "vid 4094");
        let _ = remove_vlan(4094);

        passed = passed.saturating_add(1);
        crate::serial_println!("[vlan]   test 8 (invalid VIDs) PASSED");
    }

    // --- Test 9: PCP names ---
    {
        assert!(pcp_name(0).contains("Best Effort"), "pcp 0");
        assert!(pcp_name(5).contains("Voice"), "pcp 5");
        assert!(pcp_name(7).contains("Network Control"), "pcp 7");
        assert!(pcp_name(8) == "Unknown", "pcp 8");

        passed = passed.saturating_add(1);
        crate::serial_println!("[vlan]   test 9 (PCP names) PASSED");
    }

    // --- Test 10: Procfs content ---
    {
        let content = procfs_content();
        assert!(content.contains("VLAN"), "header");
        assert!(content.contains("Tagged RX:"), "tagged rx");
        assert!(content.contains("Configured VLANs:"), "configured");

        passed = passed.saturating_add(1);
        crate::serial_println!("[vlan]   test 10 (procfs content) PASSED");
    }

    crate::serial_println!("[vlan] All {} self-tests PASSED", passed);
    Ok(())
}
