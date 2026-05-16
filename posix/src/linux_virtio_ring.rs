//! `<linux/virtio_ring.h>` — Virtio ring (virtqueue) constants.
//!
//! The virtio ring (split and packed) is the shared memory data
//! structure for communication between guest drivers and hypervisor.
//! Every virtio device uses one or more virtqueues.

// ---------------------------------------------------------------------------
// Descriptor flags
// ---------------------------------------------------------------------------

/// Buffer continues via the `next` field.
pub const VRING_DESC_F_NEXT: u16 = 1;
/// Buffer is device-writable (host reads → write flag set).
pub const VRING_DESC_F_WRITE: u16 = 2;
/// Buffer contains a list of indirect descriptors.
pub const VRING_DESC_F_INDIRECT: u16 = 4;

// ---------------------------------------------------------------------------
// Used ring flags
// ---------------------------------------------------------------------------

/// Host should not send interrupts (driver→host notification suppression).
pub const VRING_USED_F_NO_NOTIFY: u16 = 1;

// ---------------------------------------------------------------------------
// Avail ring flags
// ---------------------------------------------------------------------------

/// Driver should not receive notifications (host→driver suppression).
pub const VRING_AVAIL_F_NO_INTERRUPT: u16 = 1;

// ---------------------------------------------------------------------------
// Virtio ring alignment
// ---------------------------------------------------------------------------

/// Default virtio ring alignment (4 KiB page for legacy).
pub const VRING_ALIGNMENT: usize = 4096;

// ---------------------------------------------------------------------------
// Packed ring flags (virtio 1.1+)
// ---------------------------------------------------------------------------

/// Available flag bit in packed descriptor.
pub const VRING_PACKED_DESC_F_AVAIL: u16 = 1 << 7;
/// Used flag bit in packed descriptor.
pub const VRING_PACKED_DESC_F_USED: u16 = 1 << 15;

// ---------------------------------------------------------------------------
// Event idx feature
// ---------------------------------------------------------------------------

/// Suppresses interrupts until idx reached.
pub const VRING_PACKED_EVENT_FLAG_ENABLE: u16 = 0x0;
/// Disable interrupts.
pub const VRING_PACKED_EVENT_FLAG_DISABLE: u16 = 0x1;
/// Enable at descriptor.
pub const VRING_PACKED_EVENT_FLAG_DESC: u16 = 0x2;

// ---------------------------------------------------------------------------
// Descriptor structures
// ---------------------------------------------------------------------------

/// Virtio ring descriptor (split layout).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VringDesc {
    /// Guest physical address.
    pub addr: u64,
    /// Length in bytes.
    pub len: u32,
    /// Descriptor flags (VRING_DESC_F_*).
    pub flags: u16,
    /// Next descriptor index (if VRING_DESC_F_NEXT).
    pub next: u16,
}

impl VringDesc {
    /// Create a zeroed descriptor.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// Virtio used element.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VringUsedElem {
    /// Index of start of used descriptor chain.
    pub id: u32,
    /// Total length of descriptor chain written to.
    pub len: u32,
}

impl VringUsedElem {
    /// Create a zeroed used element.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desc_flags_are_powers_of_two() {
        assert!(VRING_DESC_F_NEXT.is_power_of_two());
        assert!(VRING_DESC_F_WRITE.is_power_of_two());
        assert!(VRING_DESC_F_INDIRECT.is_power_of_two());
    }

    #[test]
    fn test_desc_flags_no_overlap() {
        assert_eq!(VRING_DESC_F_NEXT & VRING_DESC_F_WRITE, 0);
        assert_eq!(VRING_DESC_F_WRITE & VRING_DESC_F_INDIRECT, 0);
        assert_eq!(VRING_DESC_F_NEXT & VRING_DESC_F_INDIRECT, 0);
    }

    #[test]
    fn test_vring_desc_size() {
        assert_eq!(core::mem::size_of::<VringDesc>(), 16);
    }

    #[test]
    fn test_vring_used_elem_size() {
        assert_eq!(core::mem::size_of::<VringUsedElem>(), 8);
    }

    #[test]
    fn test_alignment() {
        assert_eq!(VRING_ALIGNMENT, 4096);
    }

    #[test]
    fn test_packed_flags() {
        assert_ne!(VRING_PACKED_DESC_F_AVAIL, VRING_PACKED_DESC_F_USED);
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
    fn test_desc_zeroed() {
        let d = VringDesc::zeroed();
        assert_eq!(d.addr, 0);
        assert_eq!(d.len, 0);
        assert_eq!(d.flags, 0);
        assert_eq!(d.next, 0);
    }
}
