//! IPv4 datagram reassembly (RFC 791 §3.2).
//!
//! When an IP datagram is too large for a link's MTU, the sender (or an
//! intermediate router) splits it into *fragments*.  Each fragment carries
//! the same Identification field, the fragment offset (in 8-byte units),
//! and the More Fragments (MF) flag.  The receiver must reassemble the
//! original datagram before delivering it to the transport layer.
//!
//! ## Design
//!
//! - Up to [`MAX_REASSEMBLY_ENTRIES`] concurrent reassembly contexts.
//! - Each entry tracks received byte ranges via a bitmap (1 bit per
//!   8-byte block, matching the fragment-offset granularity).
//! - Total datagram length is determined when the last fragment arrives
//!   (MF = 0); its offset × 8 + data length = total payload length.
//! - Entries expire after [`REASSEMBLY_TIMEOUT_NS`] (30 seconds, per
//!   RFC 791 §3.2 recommendation of 15–60 s).
//! - Maximum reassembled payload: 65535 bytes (IPv4 total length limit
//!   minus the 20-byte header).
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
use spin::Mutex;

use super::interface::Ipv4Addr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum concurrent reassembly contexts.
///
/// Each context allocates up to ~65 KiB of buffer + 1 KiB bitmap, so
/// 8 contexts use at most ~528 KiB.  This is generous for a lightweight
/// OS where most traffic uses DF or fits in a single frame.
const MAX_REASSEMBLY_ENTRIES: usize = 8;

/// Reassembly timeout: 30 seconds.
///
/// RFC 791 §3.2 recommends 15–60 seconds.  Linux uses 30 seconds.
const REASSEMBLY_TIMEOUT_NS: u64 = 30_000_000_000;

/// Maximum reassembled payload size (IPv4 max total length − min header).
const MAX_PAYLOAD: usize = 65535 - 20;

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

/// One reassembly context.
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
        if end > MAX_PAYLOAD {
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
        let last_block = (end + FRAG_BLOCK_SIZE - 1) / FRAG_BLOCK_SIZE;

        // Grow bitmap if needed.
        let bitmap_needed = (last_block + 7) / 8;
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

        let total_blocks = (total + FRAG_BLOCK_SIZE - 1) / FRAG_BLOCK_SIZE;

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

/// Expire stale reassembly entries.
///
/// Called periodically from the network poll loop.  Entries older than
/// [`REASSEMBLY_TIMEOUT_NS`] are discarded (the original datagram is
/// considered lost).
pub fn tick_expire() {
    let now = crate::hrtimer::now_ns();
    let mut entries = ENTRIES.lock();

    for entry in entries.iter_mut() {
        if !entry.active {
            continue;
        }
        let age = now.saturating_sub(entry.created_ns);
        if age >= REASSEMBLY_TIMEOUT_NS {
            crate::serial_println!(
                "[frag] Reassembly timeout: id={} proto={} (age={}s)",
                entry.key.identification,
                entry.key.protocol,
                age / 1_000_000_000
            );
            entry.clear();
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test (in-kernel)
// ---------------------------------------------------------------------------

/// IPv4 reassembly unit tests — exercises FragEntry methods directly
/// without touching global state or the timer.
pub fn self_test() -> crate::error::KernelResult<()> {
    crate::serial_println!("[frag] Running self-test...");

    test_single_fragment()?;
    test_two_fragments_ordered()?;
    test_two_fragments_reversed()?;
    test_empty_fragment_rejected()?;
    test_oversized_fragment_rejected()?;

    crate::serial_println!("[frag] Self-test PASSED (5 tests)");
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
    if entry.buffer.get(0) != Some(&0x11) || entry.buffer.get(16) != Some(&0x22) {
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
    let complete = entry.add_fragment(MAX_PAYLOAD, &data, false);
    if complete {
        crate::serial_println!("[frag]   FAIL: oversized fragment accepted");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[frag]   oversized fragment rejected: OK");
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
