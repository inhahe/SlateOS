//! `<linux/virtio_scsi.h>` — Virtio SCSI device constants.
//!
//! Virtio-scsi provides a SCSI host adapter in VMs, allowing
//! multiple SCSI targets and LUNs with full SCSI command passthrough.

pub use crate::linux_virtio_types::VIRTIO_ID_SCSI;

// ---------------------------------------------------------------------------
// Feature bits
// ---------------------------------------------------------------------------

/// Hotplug support.
pub const VIRTIO_SCSI_F_HOTPLUG: u32 = 1;
/// Change support (target/LUN change).
pub const VIRTIO_SCSI_F_CHANGE: u32 = 2;
/// T10 PI (protection info).
pub const VIRTIO_SCSI_F_T10_PI: u32 = 3;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// Task management function.
pub const VIRTIO_SCSI_T_TMF: u32 = 0;
/// Async notification query.
pub const VIRTIO_SCSI_T_AN_QUERY: u32 = 1;
/// Async notification subscribe.
pub const VIRTIO_SCSI_T_AN_SUBSCRIBE: u32 = 2;

// ---------------------------------------------------------------------------
// TMF subtypes
// ---------------------------------------------------------------------------

/// Abort task.
pub const VIRTIO_SCSI_T_TMF_ABORT_TASK: u32 = 0;
/// Abort task set.
pub const VIRTIO_SCSI_T_TMF_ABORT_TASK_SET: u32 = 1;
/// Clear ACA.
pub const VIRTIO_SCSI_T_TMF_CLEAR_ACA: u32 = 2;
/// Clear task set.
pub const VIRTIO_SCSI_T_TMF_CLEAR_TASK_SET: u32 = 3;
/// I_T nexus reset.
pub const VIRTIO_SCSI_T_TMF_I_T_NEXUS_RESET: u32 = 4;
/// Logical unit reset.
pub const VIRTIO_SCSI_T_TMF_LOGICAL_UNIT_RESET: u32 = 5;
/// Query task.
pub const VIRTIO_SCSI_T_TMF_QUERY_TASK: u32 = 6;
/// Query task set.
pub const VIRTIO_SCSI_T_TMF_QUERY_TASK_SET: u32 = 7;

// ---------------------------------------------------------------------------
// Status codes
// ---------------------------------------------------------------------------

/// OK.
pub const VIRTIO_SCSI_S_OK: u8 = 0;
/// Overrun.
pub const VIRTIO_SCSI_S_OVERRUN: u8 = 1;
/// Request aborted.
pub const VIRTIO_SCSI_S_ABORTED: u8 = 2;
/// Bad target.
pub const VIRTIO_SCSI_S_BAD_TARGET: u8 = 3;
/// Reset.
pub const VIRTIO_SCSI_S_RESET: u8 = 4;
/// Busy.
pub const VIRTIO_SCSI_S_BUSY: u8 = 5;
/// Transport failure.
pub const VIRTIO_SCSI_S_TRANSPORT_FAILURE: u8 = 6;
/// Target failure.
pub const VIRTIO_SCSI_S_TARGET_FAILURE: u8 = 7;
/// Nexus failure.
pub const VIRTIO_SCSI_S_NEXUS_FAILURE: u8 = 8;
/// Failure.
pub const VIRTIO_SCSI_S_FAILURE: u8 = 9;

// ---------------------------------------------------------------------------
// Event types
// ---------------------------------------------------------------------------

/// No event.
pub const VIRTIO_SCSI_T_EVENTS_MISSED: u32 = 0x8000_0000;
/// Transport reset.
pub const VIRTIO_SCSI_T_TRANSPORT_RESET: u32 = 1;
/// Async notification.
pub const VIRTIO_SCSI_T_ASYNC_NOTIFY: u32 = 2;
/// Parameter change.
pub const VIRTIO_SCSI_T_PARAM_CHANGE: u32 = 3;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum CDB size.
pub const VIRTIO_SCSI_CDB_DEFAULT_SIZE: usize = 32;
/// Maximum sense data size.
pub const VIRTIO_SCSI_SENSE_DEFAULT_SIZE: usize = 96;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_features_distinct() {
        let feats = [
            VIRTIO_SCSI_F_HOTPLUG, VIRTIO_SCSI_F_CHANGE,
            VIRTIO_SCSI_F_T10_PI,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }

    #[test]
    fn test_tmf_subtypes_distinct() {
        let tmfs = [
            VIRTIO_SCSI_T_TMF_ABORT_TASK, VIRTIO_SCSI_T_TMF_ABORT_TASK_SET,
            VIRTIO_SCSI_T_TMF_CLEAR_ACA, VIRTIO_SCSI_T_TMF_CLEAR_TASK_SET,
            VIRTIO_SCSI_T_TMF_I_T_NEXUS_RESET, VIRTIO_SCSI_T_TMF_LOGICAL_UNIT_RESET,
            VIRTIO_SCSI_T_TMF_QUERY_TASK, VIRTIO_SCSI_T_TMF_QUERY_TASK_SET,
        ];
        for i in 0..tmfs.len() {
            for j in (i + 1)..tmfs.len() {
                assert_ne!(tmfs[i], tmfs[j]);
            }
        }
    }

    #[test]
    fn test_status_codes_distinct() {
        let codes = [
            VIRTIO_SCSI_S_OK, VIRTIO_SCSI_S_OVERRUN, VIRTIO_SCSI_S_ABORTED,
            VIRTIO_SCSI_S_BAD_TARGET, VIRTIO_SCSI_S_RESET, VIRTIO_SCSI_S_BUSY,
            VIRTIO_SCSI_S_TRANSPORT_FAILURE, VIRTIO_SCSI_S_TARGET_FAILURE,
            VIRTIO_SCSI_S_NEXUS_FAILURE, VIRTIO_SCSI_S_FAILURE,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_virtio_id() {
        assert_eq!(VIRTIO_ID_SCSI, 8);
    }

    #[test]
    fn test_sizes() {
        assert_eq!(VIRTIO_SCSI_CDB_DEFAULT_SIZE, 32);
        assert_eq!(VIRTIO_SCSI_SENSE_DEFAULT_SIZE, 96);
    }
}
