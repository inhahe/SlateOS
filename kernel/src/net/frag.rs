//! IP datagram reassembly (IPv4 RFC 791 §3.2, IPv6 RFC 8200 §4.5).
//!
//! When an IP datagram is too large for a link's MTU, the sender (or an
//! intermediate router for IPv4) splits it into *fragments*.  Each
//! fragment carries the same Identification field, the fragment offset
//! (in 8-byte units), and the More Fragments (MF) flag.  The receiver
//! must reassemble the original datagram before delivering it to the
//! transport layer.
//!
//! ## Design
//!
//! - Separate reassembly tables for IPv4 and IPv6 (up to
//!   [`MAX_REASSEMBLY_ENTRIES`] concurrent contexts each).
//! - Each entry tracks received byte ranges via a bitmap (1 bit per
//!   8-byte block, matching the fragment-offset granularity).
//! - Total datagram length is determined when the last fragment arrives
//!   (MF = 0); its offset × 8 + data length = total payload length.
//! - Entries expire after [`REASSEMBLY_TIMEOUT_NS`] (30 s for IPv4 per
//!   RFC 791 §3.2; 60 s for IPv6 per RFC 8200 §4.5).
//! - Maximum reassembled payload: 65515 bytes (IPv4) / 65535 bytes (IPv6).
//!
//! ## IPv6 Fragment Header (RFC 8200 §4.5)
//!
//! ```text
//!  0                   1                   2                   3
//!  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |  Next Header  |   Reserved    |      Fragment Offset    |Res|M|
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                         Identification                        |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! ```
//!
//! Key differences from IPv4 fragmentation:
//! - Identification is 32 bits (vs 16 in IPv4).
//! - Only the source host fragments (never intermediate routers).
//! - The "next header" field of the fragment header indicates the
//!   upper-layer protocol, not the IPv6 base header's next-header.
//! - Fragment key: (src, dst, identification) — protocol is carried
//!   inside the fragment header, not as a separate key component.
//!
//! ## Security
//!
//! - Overlapping fragments are accepted (later data overwrites earlier
//!   data at the same offset).  This matches Linux's behavior and avoids
//!   covert-channel attacks via overlap ambiguity.
//! - Tiny fragments (offset 0, MF = 1, data < 8 bytes) could be used to
//!   evade firewalls by splitting the transport header.  We accept them
//!   for reassembly but the firewall runs on the reassembled datagram.

use alloc::vec::Vec;
use crate::sync::Mutex;

use super::interface::Ipv4Addr;
use super::ipv6::Ipv6Addr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum concurrent reassembly contexts.
///
/// Each context allocates up to ~65 KiB of buffer + 1 KiB bitmap, so
/// 8 contexts use at most ~528 KiB.  This is generous for a lightweight
/// OS where most traffic uses DF or fits in a single frame.
const MAX_REASSEMBLY_ENTRIES: usize = 8;

/// IPv4 reassembly timeout: 30 seconds.
///
/// RFC 791 §3.2 recommends 15–60 seconds.  Linux uses 30 seconds.
const REASSEMBLY_TIMEOUT_V4_NS: u64 = 30_000_000_000;

/// IPv6 reassembly timeout: 60 seconds.
///
/// RFC 8200 §4.5 specifies 60 seconds.  If the first fragment (offset 0)
/// has been received, the host should send an ICMPv6 Time Exceeded
/// (type 3, code 1) before discarding.
const REASSEMBLY_TIMEOUT_V6_NS: u64 = 60_000_000_000;

/// Maximum reassembled payload size for IPv4 (max total length − min header).
const MAX_PAYLOAD_V4: usize = 65535 - 20;

/// Maximum reassembled payload size for IPv6.
///
/// IPv6 payload length field is 16 bits (65535), and the fragment header
/// is 8 bytes, but the reassembled payload is the original unfragmented
/// part minus extension headers.  Use 65535 as a safe upper bound.
const MAX_PAYLOAD_V6: usize = 65535;

/// Fragment offset granularity: each offset unit = 8 bytes.
const FRAG_BLOCK_SIZE: usize = 8;

// Maximum number of 8-byte blocks = MAX_PAYLOAD / FRAG_BLOCK_SIZE + 1.
// Maximum bitmap size = (MAX_BLOCKS + 7) / 8.
// Both are computed dynamically as fragments arrive; no pre-allocated
// bitmap array is needed.

// ---------------------------------------------------------------------------
// Reassembly entry
// ---------------------------------------------------------------------------

/// Key identifying all fragments of the same original datagram.
///
/// Per RFC 791 §3.2, the combination of source, destination,
/// identification, and protocol uniquely identifies a datagram.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FragKey {
    src: Ipv4Addr,
    dst: Ipv4Addr,
    identification: u16,
    protocol: u8,
}

/// One IPv4 reassembly context.
struct FragEntry {
    /// Whether this slot is in use.
    active: bool,
    /// Datagram identification key.
    key: FragKey,
    /// Reassembly buffer — indexed by byte offset from the start of the
    /// transport payload.  Grown lazily as fragments arrive.
    buffer: Vec<u8>,
    /// Bitmap tracking which 8-byte blocks have been received.
    /// Bit `i` of byte `i/8` is set when block `i` has data.
    received: Vec<u8>,
    /// Total payload length, known once the last fragment (MF = 0) arrives.
    /// `None` until then.
    total_len: Option<usize>,
    /// Timestamp (monotonic ns) when this entry was created.
    created_ns: u64,
}

impl FragEntry {
    const fn empty() -> Self {
        Self {
            active: false,
            key: FragKey {
                src: Ipv4Addr::UNSPECIFIED,
                dst: Ipv4Addr::UNSPECIFIED,
                identification: 0,
                protocol: 0,
            },
            buffer: Vec::new(),
            received: Vec::new(),
            total_len: None,
            created_ns: 0,
        }
    }

    /// Record a fragment's data in this entry.
    ///
    /// `byte_offset` is the fragment offset in bytes (not 8-byte units).
    /// Returns `true` if the datagram is now complete (all blocks received).
    #[allow(clippy::arithmetic_side_effects)]
    fn add_fragment(
        &mut self,
        byte_offset: usize,
        data: &[u8],
        more_fragments: bool,
    ) -> bool {
        if data.is_empty() {
            return false;
        }

        let end = byte_offset.saturating_add(data.len());

        // Sanity: reject fragments that would exceed IPv4 max payload.
        if end > MAX_PAYLOAD_V4 {
            return false;
        }

        // If this is the last fragment (MF = 0), record total length.
        if !more_fragments {
            self.total_len = Some(end);
        }

        // Grow buffer if needed.
        if end > self.buffer.len() {
            self.buffer.resize(end, 0);
        }

        // Copy fragment data into the buffer (overlapping writes are fine
        // — last writer wins, matching Linux behavior).
        self.buffer[byte_offset..end].copy_from_slice(data);

        // Mark received blocks in the bitmap.
        let first_block = byte_offset / FRAG_BLOCK_SIZE;
        // Round up for the end block to handle partial final blocks.
        let last_block = end.div_ceil(FRAG_BLOCK_SIZE);

        // Grow bitmap if needed.
        let bitmap_needed = last_block.div_ceil(8);
        if bitmap_needed > self.received.len() {
            self.received.resize(bitmap_needed, 0);
        }

        for block in first_block..last_block {
            let byte_idx = block / 8;
            let bit_idx = block % 8;
            if let Some(b) = self.received.get_mut(byte_idx) {
                *b |= 1 << bit_idx;
            }
        }

        // Check completeness: do we know the total length and have all
        // blocks from 0 to the end?
        self.is_complete()
    }

    /// Check if all blocks from 0 to `total_len` are received.
    #[allow(clippy::arithmetic_side_effects)]
    fn is_complete(&self) -> bool {
        let total = match self.total_len {
            Some(t) => t,
            None => return false,
        };

        if total == 0 {
            return true;
        }

        let total_blocks = total.div_ceil(FRAG_BLOCK_SIZE);

        // Check all full bytes in the bitmap.
        let full_bytes = total_blocks / 8;
        for i in 0..full_bytes {
            match self.received.get(i) {
                Some(&0xFF) => {}
                _ => return false,
            }
        }

        // Check remaining bits in the last partial byte.
        let remaining_bits = total_blocks % 8;
        if remaining_bits > 0 {
            let mask = (1u8 << remaining_bits) - 1;
            match self.received.get(full_bytes) {
                Some(&b) if b & mask == mask => {}
                _ => return false,
            }
        }

        true
    }

    /// Reset this entry for reuse.
    fn clear(&mut self) {
        self.active = false;
        self.buffer.clear();
        self.received.clear();
        self.total_len = None;
    }
}

// ---------------------------------------------------------------------------
// Global reassembly table
// ---------------------------------------------------------------------------

/// Global reassembly table protected by a spinlock.
static ENTRIES: Mutex<[FragEntry; MAX_REASSEMBLY_ENTRIES]> = Mutex::new([
    FragEntry::empty(),
    FragEntry::empty(),
    FragEntry::empty(),
    FragEntry::empty(),
    FragEntry::empty(),
    FragEntry::empty(),
    FragEntry::empty(),
    FragEntry::empty(),
]);

/// Result of fragment processing.
pub struct ReassembledPacket {
    /// Source IP of the original datagram.
    pub src: Ipv4Addr,
    /// Destination IP of the original datagram.
    pub dst: Ipv4Addr,
    /// IP protocol number.
    pub protocol: u8,
    /// Complete reassembled transport-layer payload.
    pub payload: Vec<u8>,
}

/// Add a fragment and return the complete datagram if reassembly finishes.
///
/// # Parameters
///
/// - `src`, `dst`: IP addresses from the fragment's header.
/// - `identification`: IP identification field (bytes 4–5).
/// - `protocol`: IP protocol number.
/// - `fragment_offset_units`: Fragment offset in 8-byte units (13-bit field).
/// - `more_fragments`: MF flag from the IP header.
/// - `data`: Fragment payload (transport data, not including IP header).
///
/// # Returns
///
/// `Some(ReassembledPacket)` when the datagram is complete, `None` otherwise.
#[allow(clippy::arithmetic_side_effects)]
pub fn add_fragment(
    src: Ipv4Addr,
    dst: Ipv4Addr,
    identification: u16,
    protocol: u8,
    fragment_offset_units: u16,
    more_fragments: bool,
    data: &[u8],
) -> Option<ReassembledPacket> {
    let byte_offset = (fragment_offset_units as usize) * FRAG_BLOCK_SIZE;

    let key = FragKey {
        src,
        dst,
        identification,
        protocol,
    };

    let mut entries = ENTRIES.lock();

    // Look for an existing entry with the same key.
    let mut found_idx = None;
    for (i, entry) in entries.iter().enumerate() {
        if entry.active && entry.key == key {
            found_idx = Some(i);
            break;
        }
    }

    // If no existing entry, allocate a new one.
    let idx = match found_idx {
        Some(i) => i,
        None => {
            // Find a free slot (or evict the oldest entry).
            let mut free_idx = None;
            let mut oldest_idx = 0;
            let mut oldest_ts = u64::MAX;

            for (i, entry) in entries.iter().enumerate() {
                if !entry.active {
                    free_idx = Some(i);
                    break;
                }
                if entry.created_ns < oldest_ts {
                    oldest_ts = entry.created_ns;
                    oldest_idx = i;
                }
            }

            let slot = free_idx.unwrap_or(oldest_idx);

            // If evicting, log it.
            if entries[slot].active {
                crate::serial_println!(
                    "[frag] Evicting reassembly entry (id={}) to make room",
                    entries[slot].key.identification
                );
                entries[slot].clear();
            }

            let now = crate::hrtimer::now_ns();
            entries[slot].active = true;
            entries[slot].key = key;
            entries[slot].created_ns = now;
            slot
        }
    };

    let entry = &mut entries[idx];

    // Add the fragment data.
    let complete = entry.add_fragment(byte_offset, data, more_fragments);

    if complete {
        // Datagram is complete — extract payload and free the slot.
        let total = entry.total_len.unwrap_or(entry.buffer.len());
        let payload = entry.buffer[..total].to_vec();
        entry.clear();

        crate::serial_println!(
            "[frag] Reassembled datagram: {}→{} proto={} id={} len={}",
            src, dst, protocol, identification, total
        );

        Some(ReassembledPacket {
            src,
            dst,
            protocol,
            payload,
        })
    } else {
        None
    }
}

/// Expire stale reassembly entries (both IPv4 and IPv6).
///
/// Called periodically from the network poll loop.  IPv4 entries older
/// than 30 s and IPv6 entries older than 60 s are discarded.
pub fn tick_expire() {
    let now = crate::hrtimer::now_ns();

    // IPv4 entries.
    {
        let mut entries = ENTRIES.lock();
        for entry in entries.iter_mut() {
            if !entry.active {
                continue;
            }
            let age = now.saturating_sub(entry.created_ns);
            if age >= REASSEMBLY_TIMEOUT_V4_NS {
                crate::serial_println!(
                    "[frag] IPv4 reassembly timeout: id={} proto={} (age={}s)",
                    entry.key.identification,
                    entry.key.protocol,
                    age / 1_000_000_000
                );
                entry.clear();
            }
        }
    }

    // IPv6 entries.
    {
        let mut entries = ENTRIES_V6.lock();
        for entry in entries.iter_mut() {
            if !entry.active {
                continue;
            }
            let age = now.saturating_sub(entry.created_ns);
            if age >= REASSEMBLY_TIMEOUT_V6_NS {
                crate::serial_println!(
                    "[frag] IPv6 reassembly timeout: id={} (age={}s)",
                    entry.key.identification,
                    age / 1_000_000_000
                );
                entry.clear();
            }
        }
    }
}

// ===========================================================================
// IPv6 fragment reassembly (RFC 8200 §4.5)
// ===========================================================================

/// Key identifying all fragments of the same original IPv6 datagram.
///
/// Per RFC 8200 §4.5, the combination of source, destination, and the
/// 32-bit Identification field uniquely identifies a datagram.  The
/// upper-layer protocol is encoded in the fragment header's Next Header
/// field and stored alongside the entry.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FragKeyV6 {
    src: Ipv6Addr,
    dst: Ipv6Addr,
    identification: u32,
}

/// One IPv6 reassembly context.
struct FragEntryV6 {
    /// Whether this slot is in use.
    active: bool,
    /// Datagram identification key.
    key: FragKeyV6,
    /// Upper-layer protocol (from fragment header's Next Header field).
    /// Set by the first fragment received.
    upper_protocol: u8,
    /// Reassembly buffer — indexed by byte offset from the start of the
    /// upper-layer payload.  Grown lazily as fragments arrive.
    buffer: Vec<u8>,
    /// Bitmap tracking which 8-byte blocks have been received.
    received: Vec<u8>,
    /// Total payload length, known once the last fragment (M = 0) arrives.
    total_len: Option<usize>,
    /// Timestamp (monotonic ns) when this entry was created.
    created_ns: u64,
    /// Whether the first fragment (offset 0) has been received.
    /// Needed to decide whether to send ICMPv6 Time Exceeded on timeout.
    has_first: bool,
}

impl FragEntryV6 {
    const fn empty() -> Self {
        Self {
            active: false,
            key: FragKeyV6 {
                src: Ipv6Addr::UNSPECIFIED,
                dst: Ipv6Addr::UNSPECIFIED,
                identification: 0,
            },
            upper_protocol: 0,
            buffer: Vec::new(),
            received: Vec::new(),
            total_len: None,
            created_ns: 0,
            has_first: false,
        }
    }

    /// Record a fragment's data in this entry.
    ///
    /// Returns `true` if the datagram is now complete.
    #[allow(clippy::arithmetic_side_effects)]
    fn add_fragment(
        &mut self,
        byte_offset: usize,
        data: &[u8],
        more_fragments: bool,
    ) -> bool {
        if data.is_empty() {
            return false;
        }

        let end = byte_offset.saturating_add(data.len());

        // Reject fragments exceeding IPv6 max payload.
        if end > MAX_PAYLOAD_V6 {
            return false;
        }

        // Track whether we've received the first fragment.
        if byte_offset == 0 {
            self.has_first = true;
        }

        // If this is the last fragment (M = 0), record total length.
        if !more_fragments {
            self.total_len = Some(end);
        }

        // Grow buffer if needed.
        if end > self.buffer.len() {
            self.buffer.resize(end, 0);
        }

        // Copy fragment data (overlapping writes: last writer wins).
        self.buffer[byte_offset..end].copy_from_slice(data);

        // Mark received blocks in the bitmap.
        let first_block = byte_offset / FRAG_BLOCK_SIZE;
        let last_block = end.div_ceil(FRAG_BLOCK_SIZE);

        let bitmap_needed = last_block.div_ceil(8);
        if bitmap_needed > self.received.len() {
            self.received.resize(bitmap_needed, 0);
        }

        for block in first_block..last_block {
            let byte_idx = block / 8;
            let bit_idx = block % 8;
            if let Some(b) = self.received.get_mut(byte_idx) {
                *b |= 1 << bit_idx;
            }
        }

        self.is_complete()
    }

    /// Check if all blocks from 0 to `total_len` are received.
    #[allow(clippy::arithmetic_side_effects)]
    fn is_complete(&self) -> bool {
        let total = match self.total_len {
            Some(t) => t,
            None => return false,
        };

        if total == 0 {
            return true;
        }

        let total_blocks = total.div_ceil(FRAG_BLOCK_SIZE);

        let full_bytes = total_blocks / 8;
        for i in 0..full_bytes {
            match self.received.get(i) {
                Some(&0xFF) => {}
                _ => return false,
            }
        }

        let remaining_bits = total_blocks % 8;
        if remaining_bits > 0 {
            let mask = (1u8 << remaining_bits) - 1;
            match self.received.get(full_bytes) {
                Some(&b) if b & mask == mask => {}
                _ => return false,
            }
        }

        true
    }

    /// Reset this entry for reuse.
    fn clear(&mut self) {
        self.active = false;
        self.buffer.clear();
        self.received.clear();
        self.total_len = None;
        self.has_first = false;
    }
}

// ---------------------------------------------------------------------------
// Global IPv6 reassembly table
// ---------------------------------------------------------------------------

static ENTRIES_V6: Mutex<[FragEntryV6; MAX_REASSEMBLY_ENTRIES]> = Mutex::new([
    FragEntryV6::empty(),
    FragEntryV6::empty(),
    FragEntryV6::empty(),
    FragEntryV6::empty(),
    FragEntryV6::empty(),
    FragEntryV6::empty(),
    FragEntryV6::empty(),
    FragEntryV6::empty(),
]);

/// Result of IPv6 fragment reassembly.
pub struct ReassembledPacketV6 {
    /// Source IPv6 address.
    pub src: Ipv6Addr,
    /// Destination IPv6 address.
    pub dst: Ipv6Addr,
    /// Upper-layer protocol (from fragment header's Next Header).
    pub upper_protocol: u8,
    /// Complete reassembled upper-layer payload.
    pub payload: Vec<u8>,
}

/// Add an IPv6 fragment and return the complete datagram if reassembly finishes.
///
/// # Parameters
///
/// - `src`, `dst`: IPv6 addresses from the packet header.
/// - `identification`: 32-bit identification from the Fragment header.
/// - `upper_protocol`: Next Header field from the Fragment header (the
///   protocol of the fragmentable part, e.g. TCP=6, UDP=17, ICMPv6=58).
/// - `fragment_offset_units`: Fragment offset in 8-byte units (13-bit field).
/// - `more_fragments`: M flag from the Fragment header.
/// - `data`: Fragment payload (upper-layer data after the Fragment header).
///
/// # Returns
///
/// `Some(ReassembledPacketV6)` when the datagram is complete, `None` otherwise.
#[allow(clippy::arithmetic_side_effects)]
pub fn add_fragment_v6(
    src: Ipv6Addr,
    dst: Ipv6Addr,
    identification: u32,
    upper_protocol: u8,
    fragment_offset_units: u16,
    more_fragments: bool,
    data: &[u8],
) -> Option<ReassembledPacketV6> {
    let byte_offset = (fragment_offset_units as usize) * FRAG_BLOCK_SIZE;

    let key = FragKeyV6 {
        src,
        dst,
        identification,
    };

    let mut entries = ENTRIES_V6.lock();

    // Look for an existing entry with the same key.
    let mut found_idx = None;
    for (i, entry) in entries.iter().enumerate() {
        if entry.active && entry.key == key {
            found_idx = Some(i);
            break;
        }
    }

    // If no existing entry, allocate a new one.
    let idx = match found_idx {
        Some(i) => i,
        None => {
            let mut free_idx = None;
            let mut oldest_idx = 0;
            let mut oldest_ts = u64::MAX;

            for (i, entry) in entries.iter().enumerate() {
                if !entry.active {
                    free_idx = Some(i);
                    break;
                }
                if entry.created_ns < oldest_ts {
                    oldest_ts = entry.created_ns;
                    oldest_idx = i;
                }
            }

            let slot = free_idx.unwrap_or(oldest_idx);

            if entries[slot].active {
                crate::serial_println!(
                    "[frag] Evicting IPv6 reassembly entry (id={}) to make room",
                    entries[slot].key.identification
                );
                entries[slot].clear();
            }

            let now = crate::hrtimer::now_ns();
            entries[slot].active = true;
            entries[slot].key = key;
            entries[slot].upper_protocol = upper_protocol;
            entries[slot].created_ns = now;
            slot
        }
    };

    let entry = &mut entries[idx];

    let complete = entry.add_fragment(byte_offset, data, more_fragments);

    if complete {
        let total = entry.total_len.unwrap_or(entry.buffer.len());
        let payload = entry.buffer[..total].to_vec();
        let proto = entry.upper_protocol;
        entry.clear();

        crate::serial_println!(
            "[frag] Reassembled IPv6 datagram: {}→{} proto={} id={} len={}",
            src, dst, proto, identification, total
        );

        Some(ReassembledPacketV6 {
            src,
            dst,
            upper_protocol: proto,
            payload,
        })
    } else {
        None
    }
}

/// Parse an IPv6 Fragment header from raw bytes.
///
/// Returns `(next_header, fragment_offset_units, more_fragments, identification)`
/// or `None` if the data is too short (< 8 bytes).
///
/// Fragment header layout (8 bytes):
/// - Byte 0: Next Header
/// - Byte 1: Reserved
/// - Bytes 2-3: Fragment Offset (13 bits) | Res (2 bits) | M flag (1 bit)
/// - Bytes 4-7: Identification (32 bits)
#[allow(clippy::arithmetic_side_effects)]
pub fn parse_fragment_header(data: &[u8]) -> Option<(u8, u16, bool, u32)> {
    if data.len() < 8 {
        return None;
    }
    let next_header = data[0];
    // Bytes 2-3: upper 13 bits = offset, bit 0 = M flag.
    let offset_field = u16::from_be_bytes([data[2], data[3]]);
    let fragment_offset = offset_field >> 3;
    let more_fragments = (offset_field & 0x01) != 0;
    let identification = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);

    Some((next_header, fragment_offset, more_fragments, identification))
}

// ---------------------------------------------------------------------------
// Self-test (in-kernel)
// ---------------------------------------------------------------------------

/// IPv4 + IPv6 reassembly unit tests — exercises FragEntry/FragEntryV6
/// methods directly without touching global state or the timer.
pub fn self_test() -> crate::error::KernelResult<()> {
    crate::serial_println!("[frag] Running self-test...");

    // IPv4 tests.
    test_single_fragment()?;
    test_two_fragments_ordered()?;
    test_two_fragments_reversed()?;
    test_empty_fragment_rejected()?;
    test_oversized_fragment_rejected()?;

    // IPv6 tests.
    test_v6_single_fragment()?;
    test_v6_two_fragments_ordered()?;
    test_v6_two_fragments_reversed()?;
    test_v6_three_fragments_middle_last()?;
    test_v6_empty_fragment_rejected()?;
    test_v6_oversized_fragment_rejected()?;
    test_v6_parse_fragment_header()?;

    crate::serial_println!("[frag] Self-test PASSED (12 tests)");
    Ok(())
}

/// A single fragment with MF=false at offset 0 should complete immediately.
fn test_single_fragment() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    let mut entry = FragEntry::empty();
    let data = [0xAA; 100];
    let complete = entry.add_fragment(0, &data, false);

    if !complete {
        crate::serial_println!("[frag]   FAIL: single fragment not complete");
        return Err(KernelError::InternalError);
    }
    if entry.total_len != Some(100) {
        crate::serial_println!(
            "[frag]   FAIL: total_len = {:?}", entry.total_len
        );
        return Err(KernelError::InternalError);
    }
    // Verify buffer content.
    if entry.buffer.len() < 100 || entry.buffer[0] != 0xAA || entry.buffer[99] != 0xAA {
        crate::serial_println!("[frag]   FAIL: buffer content");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[frag]   single fragment: OK");
    Ok(())
}

/// Two ordered fragments: first(offset=0, MF=true) + last(offset=16, MF=false).
fn test_two_fragments_ordered() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    let mut entry = FragEntry::empty();

    // Fragment 1: 16 bytes at offset 0, more coming.
    let frag1 = [0x11u8; 16];
    let complete = entry.add_fragment(0, &frag1, true);
    if complete {
        crate::serial_println!("[frag]   FAIL: completed too early");
        return Err(KernelError::InternalError);
    }

    // Fragment 2: 8 bytes at offset 16, last fragment.
    let frag2 = [0x22u8; 8];
    let complete = entry.add_fragment(16, &frag2, false);
    if !complete {
        crate::serial_println!("[frag]   FAIL: not complete after last fragment");
        return Err(KernelError::InternalError);
    }
    if entry.total_len != Some(24) {
        crate::serial_println!(
            "[frag]   FAIL: total_len = {:?}", entry.total_len
        );
        return Err(KernelError::InternalError);
    }
    // Verify buffer: first 16 bytes = 0x11, next 8 = 0x22.
    if entry.buffer.first() != Some(&0x11) || entry.buffer.get(16) != Some(&0x22) {
        crate::serial_println!("[frag]   FAIL: buffer content mixed");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[frag]   two fragments ordered: OK");
    Ok(())
}

/// Two fragments received in reverse order.
fn test_two_fragments_reversed() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    let mut entry = FragEntry::empty();

    // Last fragment first: 8 bytes at offset 16, MF=false.
    let frag2 = [0xBBu8; 8];
    let complete = entry.add_fragment(16, &frag2, false);
    if complete {
        // total_len is known but offset 0-15 not received yet.
        crate::serial_println!("[frag]   FAIL: completed too early (reversed)");
        return Err(KernelError::InternalError);
    }

    // First fragment: 16 bytes at offset 0, MF=true.
    let frag1 = [0xAAu8; 16];
    let complete = entry.add_fragment(0, &frag1, true);
    if !complete {
        crate::serial_println!("[frag]   FAIL: not complete after reversed assembly");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[frag]   two fragments reversed: OK");
    Ok(())
}

/// Empty data should not complete (and should not panic).
fn test_empty_fragment_rejected() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    let mut entry = FragEntry::empty();
    let complete = entry.add_fragment(0, &[], false);
    if complete {
        crate::serial_println!("[frag]   FAIL: empty fragment completed");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[frag]   empty fragment rejected: OK");
    Ok(())
}

/// Fragments exceeding MAX_PAYLOAD should be rejected.
fn test_oversized_fragment_rejected() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    let mut entry = FragEntry::empty();
    // offset near max + data that would exceed MAX_PAYLOAD.
    let data = [0u8; 100];
    let complete = entry.add_fragment(MAX_PAYLOAD_V4, &data, false);
    if complete {
        crate::serial_println!("[frag]   FAIL: oversized fragment accepted");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[frag]   oversized fragment rejected: OK");
    Ok(())
}

// ---------------------------------------------------------------------------
// IPv6 self-tests
// ---------------------------------------------------------------------------

/// IPv6: A single fragment with M=false at offset 0 should complete immediately.
fn test_v6_single_fragment() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    let mut entry = FragEntryV6::empty();
    entry.upper_protocol = 17; // UDP
    let data = [0xAA; 100];
    let complete = entry.add_fragment(0, &data, false);

    if !complete {
        crate::serial_println!("[frag]   FAIL: v6 single fragment not complete");
        return Err(KernelError::InternalError);
    }
    if entry.total_len != Some(100) {
        crate::serial_println!(
            "[frag]   FAIL: v6 total_len = {:?}", entry.total_len
        );
        return Err(KernelError::InternalError);
    }
    if !entry.has_first {
        crate::serial_println!("[frag]   FAIL: v6 has_first should be true");
        return Err(KernelError::InternalError);
    }
    if entry.buffer.len() < 100 || entry.buffer[0] != 0xAA || entry.buffer[99] != 0xAA {
        crate::serial_println!("[frag]   FAIL: v6 buffer content");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[frag]   v6 single fragment: OK");
    Ok(())
}

/// IPv6: Two ordered fragments.
fn test_v6_two_fragments_ordered() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    let mut entry = FragEntryV6::empty();
    entry.upper_protocol = 58; // ICMPv6

    let frag1 = [0x11u8; 16];
    let complete = entry.add_fragment(0, &frag1, true);
    if complete {
        crate::serial_println!("[frag]   FAIL: v6 completed too early");
        return Err(KernelError::InternalError);
    }

    let frag2 = [0x22u8; 8];
    let complete = entry.add_fragment(16, &frag2, false);
    if !complete {
        crate::serial_println!("[frag]   FAIL: v6 not complete after last fragment");
        return Err(KernelError::InternalError);
    }
    if entry.total_len != Some(24) {
        crate::serial_println!(
            "[frag]   FAIL: v6 total_len = {:?}", entry.total_len
        );
        return Err(KernelError::InternalError);
    }
    if entry.buffer.first() != Some(&0x11) || entry.buffer.get(16) != Some(&0x22) {
        crate::serial_println!("[frag]   FAIL: v6 buffer content");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[frag]   v6 two fragments ordered: OK");
    Ok(())
}

/// IPv6: Two fragments received in reverse order.
fn test_v6_two_fragments_reversed() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    let mut entry = FragEntryV6::empty();
    entry.upper_protocol = 17;

    // Last fragment first: 8 bytes at offset 16, M=false.
    let frag2 = [0xBBu8; 8];
    let complete = entry.add_fragment(16, &frag2, false);
    if complete {
        crate::serial_println!("[frag]   FAIL: v6 completed too early (reversed)");
        return Err(KernelError::InternalError);
    }
    if entry.has_first {
        crate::serial_println!("[frag]   FAIL: v6 has_first should be false");
        return Err(KernelError::InternalError);
    }

    // First fragment: 16 bytes at offset 0, M=true.
    let frag1 = [0xAAu8; 16];
    let complete = entry.add_fragment(0, &frag1, true);
    if !complete {
        crate::serial_println!("[frag]   FAIL: v6 not complete after reversed assembly");
        return Err(KernelError::InternalError);
    }
    if !entry.has_first {
        crate::serial_println!("[frag]   FAIL: v6 has_first should be true after offset=0");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[frag]   v6 two fragments reversed: OK");
    Ok(())
}

/// IPv6: Three fragments with middle arriving last.
fn test_v6_three_fragments_middle_last() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    let mut entry = FragEntryV6::empty();
    entry.upper_protocol = 6; // TCP

    // Fragment 1: offset 0, 8 bytes, M=true.
    let frag1 = [1u8; 8];
    if entry.add_fragment(0, &frag1, true) {
        crate::serial_println!("[frag]   FAIL: v6 three frags: completed after first");
        return Err(KernelError::InternalError);
    }

    // Fragment 3: offset 16, 8 bytes, M=false (last).
    let frag3 = [3u8; 8];
    if entry.add_fragment(16, &frag3, false) {
        crate::serial_println!("[frag]   FAIL: v6 three frags: completed after third");
        return Err(KernelError::InternalError);
    }

    // Fragment 2: offset 8, 8 bytes, M=true (middle).
    let frag2 = [2u8; 8];
    if !entry.add_fragment(8, &frag2, true) {
        crate::serial_println!("[frag]   FAIL: v6 three frags: not complete after middle");
        return Err(KernelError::InternalError);
    }

    // Verify buffer content.
    if entry.total_len != Some(24) {
        crate::serial_println!("[frag]   FAIL: v6 total_len = {:?}", entry.total_len);
        return Err(KernelError::InternalError);
    }
    for i in 0..8 {
        if entry.buffer.get(i) != Some(&1) {
            crate::serial_println!("[frag]   FAIL: v6 three frags: bad byte at {}", i);
            return Err(KernelError::InternalError);
        }
    }
    for i in 8..16 {
        if entry.buffer.get(i) != Some(&2) {
            crate::serial_println!("[frag]   FAIL: v6 three frags: bad byte at {}", i);
            return Err(KernelError::InternalError);
        }
    }
    for i in 16..24 {
        if entry.buffer.get(i) != Some(&3) {
            crate::serial_println!("[frag]   FAIL: v6 three frags: bad byte at {}", i);
            return Err(KernelError::InternalError);
        }
    }

    crate::serial_println!("[frag]   v6 three fragments (middle last): OK");
    Ok(())
}

/// IPv6: Empty data should not complete.
fn test_v6_empty_fragment_rejected() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    let mut entry = FragEntryV6::empty();
    let complete = entry.add_fragment(0, &[], false);
    if complete {
        crate::serial_println!("[frag]   FAIL: v6 empty fragment completed");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[frag]   v6 empty fragment rejected: OK");
    Ok(())
}

/// IPv6: Fragments exceeding MAX_PAYLOAD_V6 should be rejected.
fn test_v6_oversized_fragment_rejected() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    let mut entry = FragEntryV6::empty();
    let data = [0u8; 100];
    let complete = entry.add_fragment(MAX_PAYLOAD_V6, &data, false);
    if complete {
        crate::serial_println!("[frag]   FAIL: v6 oversized fragment accepted");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[frag]   v6 oversized fragment rejected: OK");
    Ok(())
}

/// Test Fragment header parsing.
fn test_v6_parse_fragment_header() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    // Build a Fragment header:
    // byte 0: Next Header = 17 (UDP)
    // byte 1: Reserved = 0
    // bytes 2-3: Fragment Offset = 100 (in 8-byte units), Res=0, M=1
    //   offset 100 = 0x64, shifted left 3 = 0x0320, with M bit = 0x0321
    // bytes 4-7: Identification = 0xDEADBEEF
    let hdr: [u8; 8] = [
        17,   // Next Header (UDP)
        0,    // Reserved
        0x03, // Fragment Offset high byte (100 << 3 >> 8 = 3)
        0x21, // Fragment Offset low + M flag (100 << 3 & 0xFF = 0x20, | M=1 = 0x21)
        0xDE, 0xAD, 0xBE, 0xEF, // Identification
    ];

    let result = parse_fragment_header(&hdr);
    match result {
        Some((nh, offset, mf, id)) => {
            if nh != 17 {
                crate::serial_println!("[frag]   FAIL: v6 frag hdr: nh={} expected 17", nh);
                return Err(KernelError::InternalError);
            }
            if offset != 100 {
                crate::serial_println!(
                    "[frag]   FAIL: v6 frag hdr: offset={} expected 100", offset
                );
                return Err(KernelError::InternalError);
            }
            if !mf {
                crate::serial_println!("[frag]   FAIL: v6 frag hdr: M flag should be true");
                return Err(KernelError::InternalError);
            }
            if id != 0xDEADBEEF {
                crate::serial_println!(
                    "[frag]   FAIL: v6 frag hdr: id=0x{:08X} expected 0xDEADBEEF", id
                );
                return Err(KernelError::InternalError);
            }
        }
        None => {
            crate::serial_println!("[frag]   FAIL: v6 frag hdr: parse returned None");
            return Err(KernelError::InternalError);
        }
    }

    // Test with M=0.
    let hdr_last: [u8; 8] = [
        58,   // Next Header (ICMPv6)
        0,
        0x00, 0x28, // offset = 5 (0x28 >> 3 = 5), M=0 (bit 0 = 0)
        0x00, 0x00, 0x00, 0x42, // id = 66
    ];
    match parse_fragment_header(&hdr_last) {
        Some((nh, offset, mf, id)) => {
            if nh != 58 || offset != 5 || mf || id != 66 {
                crate::serial_println!(
                    "[frag]   FAIL: v6 frag hdr last: nh={} off={} mf={} id={}",
                    nh, offset, mf, id
                );
                return Err(KernelError::InternalError);
            }
        }
        None => {
            crate::serial_println!("[frag]   FAIL: v6 frag hdr last: parse returned None");
            return Err(KernelError::InternalError);
        }
    }

    // Too-short header should return None.
    if parse_fragment_header(&[0; 7]).is_some() {
        crate::serial_println!("[frag]   FAIL: v6 frag hdr: short data accepted");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[frag]   v6 fragment header parse: OK");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests (#[cfg(test)] — only runs with `cargo test`, not in kernel)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn make_src() -> Ipv4Addr {
        Ipv4Addr([10, 0, 0, 1])
    }

    fn make_dst() -> Ipv4Addr {
        Ipv4Addr([10, 0, 0, 2])
    }

    #[test]
    fn test_single_unfragmented() {
        // A single fragment with MF=0 and offset=0 should complete
        // immediately.
        let data = vec![1u8; 100];
        let result = add_fragment(
            make_src(), make_dst(), 42, 17, 0, false, &data,
        );
        assert!(result.is_some());
        let pkt = result.unwrap();
        assert_eq!(pkt.payload.len(), 100);
        assert_eq!(pkt.protocol, 17);
    }

    #[test]
    fn test_two_fragments() {
        // Fragment 1: offset=0, MF=1, 16 bytes
        let frag1 = vec![0xAAu8; 16];
        let result = add_fragment(
            make_src(), make_dst(), 100, 6, 0, true, &frag1,
        );
        assert!(result.is_none());

        // Fragment 2: offset=2 (16 bytes / 8), MF=0, 8 bytes
        let frag2 = vec![0xBBu8; 8];
        let result = add_fragment(
            make_src(), make_dst(), 100, 6, 2, false, &frag2,
        );
        assert!(result.is_some());
        let pkt = result.unwrap();
        assert_eq!(pkt.payload.len(), 24);
        assert_eq!(&pkt.payload[..16], &[0xAA; 16]);
        assert_eq!(&pkt.payload[16..24], &[0xBB; 8]);
    }

    #[test]
    fn test_out_of_order_fragments() {
        // Receive last fragment first.
        let frag2 = vec![0xCCu8; 8];
        let result = add_fragment(
            make_src(), make_dst(), 200, 17, 2, false, &frag2,
        );
        assert!(result.is_none());

        // Now receive first fragment.
        let frag1 = vec![0xDDu8; 16];
        let result = add_fragment(
            make_src(), make_dst(), 200, 17, 0, true, &frag1,
        );
        assert!(result.is_some());
        let pkt = result.unwrap();
        assert_eq!(pkt.payload.len(), 24);
        assert_eq!(&pkt.payload[..16], &[0xDD; 16]);
        assert_eq!(&pkt.payload[16..24], &[0xCC; 8]);
    }

    #[test]
    fn test_three_fragments() {
        let frag1 = vec![1u8; 8];
        assert!(add_fragment(make_src(), make_dst(), 300, 6, 0, true, &frag1).is_none());

        let frag3 = vec![3u8; 8];
        assert!(add_fragment(make_src(), make_dst(), 300, 6, 2, false, &frag3).is_none());

        let frag2 = vec![2u8; 8];
        let result = add_fragment(make_src(), make_dst(), 300, 6, 1, true, &frag2);
        assert!(result.is_some());
        let pkt = result.unwrap();
        assert_eq!(pkt.payload.len(), 24);
        assert_eq!(&pkt.payload[0..8], &[1; 8]);
        assert_eq!(&pkt.payload[8..16], &[2; 8]);
        assert_eq!(&pkt.payload[16..24], &[3; 8]);
    }

    #[test]
    fn test_different_ids_dont_mix() {
        // Two concurrent datagrams with different IDs.
        let frag_a1 = vec![0xAAu8; 8];
        assert!(add_fragment(make_src(), make_dst(), 400, 17, 0, true, &frag_a1).is_none());

        let frag_b1 = vec![0xBBu8; 8];
        assert!(add_fragment(make_src(), make_dst(), 401, 17, 0, true, &frag_b1).is_none());

        // Complete B.
        let frag_b2 = vec![0xCCu8; 8];
        let result = add_fragment(make_src(), make_dst(), 401, 17, 1, false, &frag_b2);
        assert!(result.is_some());
        let pkt = result.unwrap();
        assert_eq!(&pkt.payload[..8], &[0xBB; 8]);
        assert_eq!(&pkt.payload[8..16], &[0xCC; 8]);

        // A is still pending.
        let frag_a2 = vec![0xDDu8; 8];
        let result = add_fragment(make_src(), make_dst(), 400, 17, 1, false, &frag_a2);
        assert!(result.is_some());
        let pkt = result.unwrap();
        assert_eq!(&pkt.payload[..8], &[0xAA; 8]);
        assert_eq!(&pkt.payload[8..16], &[0xDD; 8]);
    }

    #[test]
    fn test_oversized_fragment_rejected() {
        // Fragment that would put data past MAX_PAYLOAD.
        let data = vec![0u8; 100];
        // offset = 8190 blocks * 8 = 65520 bytes, + 100 = 65620 > MAX_PAYLOAD
        let result = add_fragment(
            make_src(), make_dst(), 500, 17, 8190, false, &data,
        );
        // Should not complete (fragment rejected).
        assert!(result.is_none());
    }

    #[test]
    fn test_empty_fragment_ignored() {
        let result = add_fragment(
            make_src(), make_dst(), 600, 17, 0, true, &[],
        );
        assert!(result.is_none());
    }
}
