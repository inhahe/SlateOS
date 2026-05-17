//! `<linux/remoteproc.h>` — Remote Processor framework constants.
//!
//! Remoteproc manages auxiliary processors on heterogeneous SoCs
//! (DSPs, MCUs, GPU compute cores). It handles firmware loading,
//! boot, crash recovery, and lifecycle management of remote cores.
//! The framework supports resource tables in firmware images that
//! describe memory requirements, virtio devices, and trace buffers.
//! Used for Cortex-M cores on i.MX/STM32, DSPs on OMAP/TI, and
//! Qualcomm's ADSP/CDSP/SLPI subsystems.

// ---------------------------------------------------------------------------
// Remote processor states
// ---------------------------------------------------------------------------

/// Processor is offline (not loaded).
pub const RPROC_STATE_OFFLINE: u32 = 0;
/// Firmware is being loaded.
pub const RPROC_STATE_LOADING: u32 = 1;
/// Processor is running.
pub const RPROC_STATE_RUNNING: u32 = 2;
/// Processor crashed (recovery pending).
pub const RPROC_STATE_CRASHED: u32 = 3;
/// Processor is being stopped.
pub const RPROC_STATE_STOPPING: u32 = 4;
/// Processor is suspended.
pub const RPROC_STATE_SUSPENDED: u32 = 5;

// ---------------------------------------------------------------------------
// Resource table entry types
// ---------------------------------------------------------------------------

/// Carveout memory (physically contiguous).
pub const RSC_CARVEOUT: u32 = 0;
/// Device memory (MMIO mapping for remote).
pub const RSC_DEVMEM: u32 = 1;
/// Trace buffer (remote → host debug log).
pub const RSC_TRACE: u32 = 2;
/// VirtIO device (virtio transport for rpmsg).
pub const RSC_VDEV: u32 = 3;

// ---------------------------------------------------------------------------
// Remote processor flags
// ---------------------------------------------------------------------------

/// Auto-boot: start processor as soon as firmware is loaded.
pub const RPROC_FLAG_AUTO_BOOT: u32 = 1 << 0;
/// Crash recovery: automatically restart on crash.
pub const RPROC_FLAG_RECOVERY: u32 = 1 << 1;
/// Power domain: processor has its own power domain.
pub const RPROC_FLAG_POWER_DOMAIN: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Remote processor crash reasons
// ---------------------------------------------------------------------------

/// Watchdog timeout.
pub const RPROC_CRASH_WATCHDOG: u32 = 0;
/// Exception/fault on remote processor.
pub const RPROC_CRASH_EXCEPTION: u32 = 1;
/// Bus error.
pub const RPROC_CRASH_BUS_ERROR: u32 = 2;
/// Unrecoverable error.
pub const RPROC_CRASH_FATAL: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [
            RPROC_STATE_OFFLINE, RPROC_STATE_LOADING,
            RPROC_STATE_RUNNING, RPROC_STATE_CRASHED,
            RPROC_STATE_STOPPING, RPROC_STATE_SUSPENDED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_resource_types_distinct() {
        let types = [RSC_CARVEOUT, RSC_DEVMEM, RSC_TRACE, RSC_VDEV];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            RPROC_FLAG_AUTO_BOOT, RPROC_FLAG_RECOVERY,
            RPROC_FLAG_POWER_DOMAIN,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_crash_reasons_distinct() {
        let reasons = [
            RPROC_CRASH_WATCHDOG, RPROC_CRASH_EXCEPTION,
            RPROC_CRASH_BUS_ERROR, RPROC_CRASH_FATAL,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }
}
