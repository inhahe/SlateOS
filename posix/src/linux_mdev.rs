//! `<linux/mdev.h>` — Mediated device (mdev) framework constants.
//!
//! Mediated devices allow a physical device to be partitioned into
//! multiple virtual devices (e.g., GPU SR-IOV, vGPU) that can be
//! assigned to VMs via VFIO. The mdev framework manages creation,
//! removal, and lifecycle of these virtual device instances.

// ---------------------------------------------------------------------------
// Mdev types
// ---------------------------------------------------------------------------

/// Type 1: SR-IOV Virtual Function-like.
pub const MDEV_TYPE_VF: u8 = 0;
/// Type 2: Software-mediated (vGPU style).
pub const MDEV_TYPE_SW: u8 = 1;
/// Type 3: Sub-device (partial resource).
pub const MDEV_TYPE_SUBDEV: u8 = 2;

// ---------------------------------------------------------------------------
// Mdev states
// ---------------------------------------------------------------------------

/// Device not created.
pub const MDEV_STATE_NONE: u8 = 0;
/// Device created but not opened.
pub const MDEV_STATE_CREATED: u8 = 1;
/// Device opened (VFIO group assigned).
pub const MDEV_STATE_OPENED: u8 = 2;
/// Device running (VM active).
pub const MDEV_STATE_RUNNING: u8 = 3;
/// Device in error state.
pub const MDEV_STATE_ERROR: u8 = 4;

// ---------------------------------------------------------------------------
// Mdev capabilities
// ---------------------------------------------------------------------------

/// Supports live migration.
pub const MDEV_CAP_MIGRATION: u32 = 1 << 0;
/// Supports dirty page tracking.
pub const MDEV_CAP_DIRTY_TRACKING: u32 = 1 << 1;
/// Supports device reset.
pub const MDEV_CAP_RESET: u32 = 1 << 2;
/// Supports save/restore.
pub const MDEV_CAP_SAVE_RESTORE: u32 = 1 << 3;
/// Supports multiple instances.
pub const MDEV_CAP_MULTI_INSTANCE: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Mdev resource types
// ---------------------------------------------------------------------------

/// Memory resource.
pub const MDEV_RES_MEM: u8 = 0;
/// I/O port resource.
pub const MDEV_RES_IO: u8 = 1;
/// IRQ resource.
pub const MDEV_RES_IRQ: u8 = 2;
/// DMA resource.
pub const MDEV_RES_DMA: u8 = 3;

// ---------------------------------------------------------------------------
// VFIO mdev region types
// ---------------------------------------------------------------------------

/// PCI config space region.
pub const VFIO_MDEV_REGION_CONFIG: u8 = 0;
/// BAR region.
pub const VFIO_MDEV_REGION_BAR: u8 = 1;
/// VGA region.
pub const VFIO_MDEV_REGION_VGA: u8 = 2;
/// Vendor-specific region.
pub const VFIO_MDEV_REGION_VENDOR: u8 = 3;
/// Migration region.
pub const VFIO_MDEV_REGION_MIGRATION: u8 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [MDEV_TYPE_VF, MDEV_TYPE_SW, MDEV_TYPE_SUBDEV];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            MDEV_STATE_NONE, MDEV_STATE_CREATED, MDEV_STATE_OPENED,
            MDEV_STATE_RUNNING, MDEV_STATE_ERROR,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_caps_no_overlap() {
        let caps = [
            MDEV_CAP_MIGRATION, MDEV_CAP_DIRTY_TRACKING,
            MDEV_CAP_RESET, MDEV_CAP_SAVE_RESTORE,
            MDEV_CAP_MULTI_INSTANCE,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }

    #[test]
    fn test_resource_types_distinct() {
        let res = [MDEV_RES_MEM, MDEV_RES_IO, MDEV_RES_IRQ, MDEV_RES_DMA];
        for i in 0..res.len() {
            for j in (i + 1)..res.len() {
                assert_ne!(res[i], res[j]);
            }
        }
    }

    #[test]
    fn test_vfio_regions_distinct() {
        let regions = [
            VFIO_MDEV_REGION_CONFIG, VFIO_MDEV_REGION_BAR,
            VFIO_MDEV_REGION_VGA, VFIO_MDEV_REGION_VENDOR,
            VFIO_MDEV_REGION_MIGRATION,
        ];
        for i in 0..regions.len() {
            for j in (i + 1)..regions.len() {
                assert_ne!(regions[i], regions[j]);
            }
        }
    }
}
