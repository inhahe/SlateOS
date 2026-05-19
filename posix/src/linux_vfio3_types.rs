//! `<linux/vfio.h>` — Additional VFIO constants (part 3).
//!
//! Supplementary VFIO constants covering device info flags,
//! region types, and IRQ info flags.

// ---------------------------------------------------------------------------
// VFIO device info flags
// ---------------------------------------------------------------------------

/// Reset available.
pub const VFIO_DEVICE_FLAGS_RESET: u32 = 1 << 0;
/// PCI device.
pub const VFIO_DEVICE_FLAGS_PCI: u32 = 1 << 1;
/// Platform device.
pub const VFIO_DEVICE_FLAGS_PLATFORM: u32 = 1 << 2;
/// AMBA device.
pub const VFIO_DEVICE_FLAGS_AMBA: u32 = 1 << 3;
/// CCW device.
pub const VFIO_DEVICE_FLAGS_CCW: u32 = 1 << 4;
/// AP device.
pub const VFIO_DEVICE_FLAGS_AP: u32 = 1 << 5;
/// FSL MC device.
pub const VFIO_DEVICE_FLAGS_FSL_MC: u32 = 1 << 6;
/// CAPS available.
pub const VFIO_DEVICE_FLAGS_CAPS: u32 = 1 << 7;
/// CDX device.
pub const VFIO_DEVICE_FLAGS_CDX: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// VFIO region info flags
// ---------------------------------------------------------------------------

/// Read supported.
pub const VFIO_REGION_INFO_FLAG_READ: u32 = 1 << 0;
/// Write supported.
pub const VFIO_REGION_INFO_FLAG_WRITE: u32 = 1 << 1;
/// Mmap supported.
pub const VFIO_REGION_INFO_FLAG_MMAP: u32 = 1 << 2;
/// Caps available.
pub const VFIO_REGION_INFO_FLAG_CAPS: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// VFIO IRQ info flags
// ---------------------------------------------------------------------------

/// Eventfd supported.
pub const VFIO_IRQ_INFO_EVENTFD: u32 = 1 << 0;
/// Maskable.
pub const VFIO_IRQ_INFO_MASKABLE: u32 = 1 << 1;
/// Auto-masked.
pub const VFIO_IRQ_INFO_AUTOMASKED: u32 = 1 << 2;
/// Noresize.
pub const VFIO_IRQ_INFO_NORESIZE: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// VFIO IRQ set action flags
// ---------------------------------------------------------------------------

/// Mask IRQ.
pub const VFIO_IRQ_SET_ACTION_MASK: u32 = 1 << 0;
/// Unmask IRQ.
pub const VFIO_IRQ_SET_ACTION_UNMASK: u32 = 1 << 1;
/// Trigger IRQ.
pub const VFIO_IRQ_SET_ACTION_TRIGGER: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_flags_no_overlap() {
        let flags = [
            VFIO_DEVICE_FLAGS_RESET, VFIO_DEVICE_FLAGS_PCI,
            VFIO_DEVICE_FLAGS_PLATFORM, VFIO_DEVICE_FLAGS_AMBA,
            VFIO_DEVICE_FLAGS_CCW, VFIO_DEVICE_FLAGS_AP,
            VFIO_DEVICE_FLAGS_FSL_MC, VFIO_DEVICE_FLAGS_CAPS,
            VFIO_DEVICE_FLAGS_CDX,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_region_flags_no_overlap() {
        let flags = [
            VFIO_REGION_INFO_FLAG_READ, VFIO_REGION_INFO_FLAG_WRITE,
            VFIO_REGION_INFO_FLAG_MMAP, VFIO_REGION_INFO_FLAG_CAPS,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_irq_info_flags_no_overlap() {
        let flags = [
            VFIO_IRQ_INFO_EVENTFD, VFIO_IRQ_INFO_MASKABLE,
            VFIO_IRQ_INFO_AUTOMASKED, VFIO_IRQ_INFO_NORESIZE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_irq_set_actions_no_overlap() {
        let actions = [
            VFIO_IRQ_SET_ACTION_MASK, VFIO_IRQ_SET_ACTION_UNMASK,
            VFIO_IRQ_SET_ACTION_TRIGGER,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_eq!(actions[i] & actions[j], 0);
            }
        }
    }
}
