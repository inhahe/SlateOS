//! `<linux/mdev.h>` — Mediated device (mdev) framework constants.
//!
//! Mediated devices allow a physical device to be partitioned into
//! multiple virtual devices without SR-IOV hardware support. The
//! parent driver (e.g., NVIDIA vGPU, Intel GVT-g) creates mdev
//! instances that appear as VFIO devices. Each mdev gets a portion
//! of the physical device's resources (GPU memory, compute units).
//! Used for GPU virtualization (vGPU), network virtualization, and
//! any device needing software-mediated partitioning.

// ---------------------------------------------------------------------------
// mdev sysfs operations
// ---------------------------------------------------------------------------

/// Create an mdev instance (write UUID to create file).
pub const MDEV_CREATE: u32 = 1;
/// Remove an mdev instance (write 1 to remove file).
pub const MDEV_REMOVE: u32 = 2;

// ---------------------------------------------------------------------------
// mdev types (exposed via sysfs type groups)
// ---------------------------------------------------------------------------

/// Available instances for this type.
pub const MDEV_TYPE_ATTR_AVAILABLE: u32 = 1;
/// Device API (vfio-pci, vfio-ap, etc.).
pub const MDEV_TYPE_ATTR_DEVICE_API: u32 = 2;
/// Type name (human-readable).
pub const MDEV_TYPE_ATTR_NAME: u32 = 3;
/// Type description.
pub const MDEV_TYPE_ATTR_DESCRIPTION: u32 = 4;

// ---------------------------------------------------------------------------
// VFIO mdev device API strings
// ---------------------------------------------------------------------------

/// VFIO PCI device API (most common for GPU mdev).
pub const VFIO_DEVICE_API_PCI: u32 = 0;
/// VFIO AP (crypto adapter) device API.
pub const VFIO_DEVICE_API_AP: u32 = 1;
/// VFIO CCW (channel I/O) device API.
pub const VFIO_DEVICE_API_CCW: u32 = 2;

// ---------------------------------------------------------------------------
// mdev states
// ---------------------------------------------------------------------------

/// mdev is available (created but not in use).
pub const MDEV_STATE_AVAILABLE: u32 = 0;
/// mdev is running (attached to a VM/container).
pub const MDEV_STATE_RUNNING: u32 = 1;
/// mdev is stopped.
pub const MDEV_STATE_STOPPED: u32 = 2;
/// mdev is in error state.
pub const MDEV_STATE_ERROR: u32 = 3;

// ---------------------------------------------------------------------------
// mdev migration states (for live migration support)
// ---------------------------------------------------------------------------

/// Device is running (normal operation).
pub const VFIO_DEVICE_STATE_RUNNING: u32 = 1 << 0;
/// Device state is being saved (migration source).
pub const VFIO_DEVICE_STATE_SAVING: u32 = 1 << 1;
/// Device state is being resumed (migration destination).
pub const VFIO_DEVICE_STATE_RESUMING: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operations_distinct() {
        assert_ne!(MDEV_CREATE, MDEV_REMOVE);
    }

    #[test]
    fn test_type_attrs_distinct() {
        let attrs = [
            MDEV_TYPE_ATTR_AVAILABLE, MDEV_TYPE_ATTR_DEVICE_API,
            MDEV_TYPE_ATTR_NAME, MDEV_TYPE_ATTR_DESCRIPTION,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_device_apis_distinct() {
        let apis = [VFIO_DEVICE_API_PCI, VFIO_DEVICE_API_AP, VFIO_DEVICE_API_CCW];
        for i in 0..apis.len() {
            for j in (i + 1)..apis.len() {
                assert_ne!(apis[i], apis[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            MDEV_STATE_AVAILABLE, MDEV_STATE_RUNNING,
            MDEV_STATE_STOPPED, MDEV_STATE_ERROR,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_migration_flags_no_overlap() {
        let flags = [
            VFIO_DEVICE_STATE_RUNNING,
            VFIO_DEVICE_STATE_SAVING,
            VFIO_DEVICE_STATE_RESUMING,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
