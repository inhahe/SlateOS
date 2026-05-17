//! `<linux/ras.h>` — Reliability, Availability, Serviceability (RAS) constants.
//!
//! RAS is the Linux framework for reporting and handling hardware
//! errors — memory ECC errors, PCIe AER events, disk predictive
//! failures, CPU machine checks, etc. The RAS subsystem collects
//! error reports from various sources (EDAC, MCE, AER, CXL) and
//! exposes them through tracepoints and /sys/kernel/debug/ras/.
//! The framework supports error counting, thresholding, and
//! corrective actions (page offlining, CPU isolation).

// ---------------------------------------------------------------------------
// RAS error types (high-level classification)
// ---------------------------------------------------------------------------

/// Memory error (ECC correctable or uncorrectable).
pub const RAS_TYPE_MEMORY: u32 = 0;
/// CPU/cache error (machine check exception).
pub const RAS_TYPE_CPU: u32 = 1;
/// PCIe error (AER — correctable, non-fatal, fatal).
pub const RAS_TYPE_PCIE: u32 = 2;
/// Platform error (firmware-reported, GHES).
pub const RAS_TYPE_PLATFORM: u32 = 3;
/// CXL error (CXL memory/protocol errors).
pub const RAS_TYPE_CXL: u32 = 4;
/// Disk/storage error (predictive failure, SMART).
pub const RAS_TYPE_DISK: u32 = 5;

// ---------------------------------------------------------------------------
// RAS error severity
// ---------------------------------------------------------------------------

/// Informational (logged but no action needed).
pub const RAS_SEVERITY_INFO: u32 = 0;
/// Corrected error (hardware corrected, no data loss).
pub const RAS_SEVERITY_CORRECTED: u32 = 1;
/// Recoverable error (OS can recover with action).
pub const RAS_SEVERITY_RECOVERABLE: u32 = 2;
/// Fatal error (system must be reset or component isolated).
pub const RAS_SEVERITY_FATAL: u32 = 3;

// ---------------------------------------------------------------------------
// RAS error action codes
// ---------------------------------------------------------------------------

/// No action needed (just log).
pub const RAS_ACTION_NONE: u32 = 0;
/// Offline the affected page (memory error).
pub const RAS_ACTION_OFFLINE_PAGE: u32 = 1;
/// Isolate the CPU (take offline).
pub const RAS_ACTION_ISOLATE_CPU: u32 = 2;
/// Reset the device.
pub const RAS_ACTION_RESET_DEVICE: u32 = 3;
/// Panic (error too severe to continue).
pub const RAS_ACTION_PANIC: u32 = 4;
/// Notify userspace (udev, rasdaemon).
pub const RAS_ACTION_NOTIFY: u32 = 5;

// ---------------------------------------------------------------------------
// RAS tracepoint event IDs
// ---------------------------------------------------------------------------

/// Memory corrected error event.
pub const RAS_EVENT_MC_CE: u32 = 0;
/// Memory uncorrectable error event.
pub const RAS_EVENT_MC_UE: u32 = 1;
/// PCIe AER correctable error event.
pub const RAS_EVENT_AER_CE: u32 = 2;
/// PCIe AER non-fatal error event.
pub const RAS_EVENT_AER_NONFATAL: u32 = 3;
/// PCIe AER fatal error event.
pub const RAS_EVENT_AER_FATAL: u32 = 4;
/// Machine check exception event.
pub const RAS_EVENT_MCE: u32 = 5;
/// CXL protocol error event.
pub const RAS_EVENT_CXL: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_types_distinct() {
        let types = [
            RAS_TYPE_MEMORY, RAS_TYPE_CPU, RAS_TYPE_PCIE,
            RAS_TYPE_PLATFORM, RAS_TYPE_CXL, RAS_TYPE_DISK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_severity_ordered() {
        assert!(RAS_SEVERITY_INFO < RAS_SEVERITY_CORRECTED);
        assert!(RAS_SEVERITY_CORRECTED < RAS_SEVERITY_RECOVERABLE);
        assert!(RAS_SEVERITY_RECOVERABLE < RAS_SEVERITY_FATAL);
    }

    #[test]
    fn test_actions_distinct() {
        let actions = [
            RAS_ACTION_NONE, RAS_ACTION_OFFLINE_PAGE,
            RAS_ACTION_ISOLATE_CPU, RAS_ACTION_RESET_DEVICE,
            RAS_ACTION_PANIC, RAS_ACTION_NOTIFY,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            RAS_EVENT_MC_CE, RAS_EVENT_MC_UE, RAS_EVENT_AER_CE,
            RAS_EVENT_AER_NONFATAL, RAS_EVENT_AER_FATAL,
            RAS_EVENT_MCE, RAS_EVENT_CXL,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }
}
