//! `<linux/virtio_ring.h>` — VirtIO virtqueue ring constants.
//!
//! The virtqueue is the data transport mechanism between guest
//! drivers and the hypervisor/host. It consists of a descriptor
//! table (scatter-gather buffers), an available ring (guest →
//! device), and a used ring (device → guest). The split layout is
//! the traditional format; packed layout (VirtIO 1.1+) uses a
//! single descriptor ring with in-place flags. These constants
//! define descriptor flags, ring alignment, and event suppression.

// ---------------------------------------------------------------------------
// Descriptor flags (VRING_DESC_F_*)
// ---------------------------------------------------------------------------

/// Buffer continues in the next descriptor (chained).
pub const VRING_DESC_F_NEXT: u16 = 1;
/// Buffer is device-writable (else device-readable).
pub const VRING_DESC_F_WRITE: u16 = 2;
/// Buffer contains a list of indirect descriptors.
pub const VRING_DESC_F_INDIRECT: u16 = 4;

// ---------------------------------------------------------------------------
// Available ring flags (VRING_AVAIL_F_*)
// ---------------------------------------------------------------------------

/// Don't interrupt (guest tells device: no interrupts needed).
pub const VRING_AVAIL_F_NO_INTERRUPT: u16 = 1;

// ---------------------------------------------------------------------------
// Used ring flags (VRING_USED_F_*)
// ---------------------------------------------------------------------------

/// Don't notify (device tells guest: no notifications needed).
pub const VRING_USED_F_NO_NOTIFY: u16 = 1;

// ---------------------------------------------------------------------------
// Ring alignment
// ---------------------------------------------------------------------------

/// Alignment of the used ring (4096 bytes for split layout).
pub const VRING_USED_ALIGN_SIZE: u32 = 4;
/// Alignment of the available ring (2 bytes for split layout).
pub const VRING_AVAIL_ALIGN_SIZE: u32 = 2;
/// Legacy: page-aligned ring (4096-byte pages).
pub const VRING_PAGE_SIZE: u32 = 4096;

// ---------------------------------------------------------------------------
// Packed ring descriptor flags
// ---------------------------------------------------------------------------

/// Packed: buffer is available (not yet consumed by device).
pub const VRING_PACKED_DESC_F_AVAIL: u16 = 1 << 7;
/// Packed: buffer is used (consumed by device).
pub const VRING_PACKED_DESC_F_USED: u16 = 1 << 15;

// ---------------------------------------------------------------------------
// Event suppression
// ---------------------------------------------------------------------------

/// Enable event (interrupt/notification) on this descriptor.
pub const VRING_PACKED_EVENT_FLAG_ENABLE: u16 = 0;
/// Disable event.
pub const VRING_PACKED_EVENT_FLAG_DISABLE: u16 = 1;
/// Enable event based on descriptor index.
pub const VRING_PACKED_EVENT_FLAG_DESC: u16 = 2;

// ---------------------------------------------------------------------------
// Maximum queue size
// ---------------------------------------------------------------------------

/// Maximum number of descriptors in a virtqueue (must be power of 2).
pub const VRING_MAX_SIZE: u16 = 32768;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desc_flags_no_overlap() {
        assert_eq!(VRING_DESC_F_NEXT & VRING_DESC_F_WRITE, 0);
        assert_eq!(VRING_DESC_F_WRITE & VRING_DESC_F_INDIRECT, 0);
        assert_eq!(VRING_DESC_F_NEXT & VRING_DESC_F_INDIRECT, 0);
    }

    #[test]
    fn test_desc_flags_are_powers() {
        assert!(VRING_DESC_F_NEXT.is_power_of_two());
        assert!(VRING_DESC_F_WRITE.is_power_of_two());
        assert!(VRING_DESC_F_INDIRECT.is_power_of_two());
    }

    #[test]
    fn test_packed_flags_no_overlap() {
        assert_eq!(VRING_PACKED_DESC_F_AVAIL & VRING_PACKED_DESC_F_USED, 0);
    }

    #[test]
    fn test_event_flags_distinct() {
        let flags = [
            VRING_PACKED_EVENT_FLAG_ENABLE,
            VRING_PACKED_EVENT_FLAG_DISABLE,
            VRING_PACKED_EVENT_FLAG_DESC,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_max_size_power_of_two() {
        assert!(VRING_MAX_SIZE.is_power_of_two());
    }

    #[test]
    fn test_page_size() {
        assert_eq!(VRING_PAGE_SIZE, 4096);
    }
}
