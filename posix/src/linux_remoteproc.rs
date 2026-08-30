//! `<linux/remoteproc.h>` — Remote processor framework constants.
//!
//! The remoteproc framework manages auxiliary processors (DSPs,
//! microcontrollers, GPUs) that run their own firmware. Used on
//! SoC platforms (TI OMAP, Qualcomm, NXP i.MX) for offloading.

// ---------------------------------------------------------------------------
// Resource types (in firmware resource table)
// ---------------------------------------------------------------------------

/// Carveout memory region.
pub const RSC_CARVEOUT: u32 = 0;
/// Device memory mapping.
pub const RSC_DEVMEM: u32 = 1;
/// Trace buffer.
pub const RSC_TRACE: u32 = 2;
/// VirtIO device.
pub const RSC_VDEV: u32 = 3;
/// Last standard resource type.
pub const RSC_LAST: u32 = 4;
/// Vendor-specific resource start.
pub const RSC_VENDOR_START: u32 = 128;
/// Vendor-specific resource end.
pub const RSC_VENDOR_END: u32 = 512;

// ---------------------------------------------------------------------------
// Remote processor states
// ---------------------------------------------------------------------------

/// Processor is offline.
pub const RPROC_OFFLINE: u32 = 0;
/// Processor is suspended.
pub const RPROC_SUSPENDED: u32 = 1;
/// Processor is running.
pub const RPROC_RUNNING: u32 = 2;
/// Processor crashed.
pub const RPROC_CRASHED: u32 = 3;
/// Processor deleted.
pub const RPROC_DELETED: u32 = 4;
/// Processor is attached (started by external entity).
pub const RPROC_ATTACHED: u32 = 5;
/// Processor detached.
pub const RPROC_DETACHED: u32 = 6;

// ---------------------------------------------------------------------------
// Crash reasons
// ---------------------------------------------------------------------------

/// Hardware error.
pub const RPROC_HW_ERROR: u32 = 0;
/// Watchdog bite.
pub const RPROC_WATCHDOG: u32 = 1;
/// Fatal error reported by firmware.
pub const RPROC_FATAL_ERROR: u32 = 2;

// ---------------------------------------------------------------------------
// Firmware flags
// ---------------------------------------------------------------------------

/// Run from device memory.
pub const RPROC_FLAGS_NONE: u32 = 0;
/// Firmware is already loaded by a previous boot stage.
pub const RPROC_FLAGS_FW_PRELOADED: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rsc_types_distinct() {
        let types = [RSC_CARVEOUT, RSC_DEVMEM, RSC_TRACE, RSC_VDEV, RSC_LAST];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_vendor_range() {
        assert!(RSC_VENDOR_START < RSC_VENDOR_END);
        assert!(RSC_LAST <= RSC_VENDOR_START);
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            RPROC_OFFLINE,
            RPROC_SUSPENDED,
            RPROC_RUNNING,
            RPROC_CRASHED,
            RPROC_DELETED,
            RPROC_ATTACHED,
            RPROC_DETACHED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_crash_reasons_distinct() {
        let reasons = [RPROC_HW_ERROR, RPROC_WATCHDOG, RPROC_FATAL_ERROR];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }

    #[test]
    fn test_rsc_values() {
        assert_eq!(RSC_CARVEOUT, 0);
        assert_eq!(RSC_VDEV, 3);
    }
}
