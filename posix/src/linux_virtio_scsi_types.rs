//! `<linux/virtio_scsi.h>` — VirtIO SCSI host adapter constants.
//!
//! virtio-scsi provides a full SCSI host bus adapter to guest VMs,
//! supporting multiple LUNs, hot-plug, and SCSI pass-through. Unlike
//! virtio-blk (which exposes a single disk), virtio-scsi can expose
//! many disks, CD-ROMs, and tape devices via standard SCSI commands.
//! The guest uses its normal SCSI stack (sd, sr, st drivers). Used
//! when guests need multi-disk setups, SCSI features like persistent
//! reservations, or live migration with many devices.

// ---------------------------------------------------------------------------
// VirtIO SCSI request/response types
// ---------------------------------------------------------------------------

/// Command request (CDB + data).
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
// VirtIO SCSI response codes
// ---------------------------------------------------------------------------

/// Command completed successfully.
pub const VIRTIO_SCSI_S_OK: u32 = 0;
/// Command overrun (too much data).
pub const VIRTIO_SCSI_S_OVERRUN: u32 = 1;
/// Command aborted.
pub const VIRTIO_SCSI_S_ABORTED: u32 = 2;
/// Bad target (LUN doesn't exist).
pub const VIRTIO_SCSI_S_BAD_TARGET: u32 = 3;
/// Command reset.
pub const VIRTIO_SCSI_S_RESET: u32 = 4;
/// Busy.
pub const VIRTIO_SCSI_S_BUSY: u32 = 5;
/// Transport failure.
pub const VIRTIO_SCSI_S_TRANSPORT_FAILURE: u32 = 6;
/// Target failure.
pub const VIRTIO_SCSI_S_TARGET_FAILURE: u32 = 7;
/// Nexus failure.
pub const VIRTIO_SCSI_S_NEXUS_FAILURE: u32 = 8;
/// General failure.
pub const VIRTIO_SCSI_S_FAILURE: u32 = 9;
/// Function succeeded (for TMF responses).
pub const VIRTIO_SCSI_S_FUNCTION_SUCCEEDED: u32 = 0;
/// Function rejected.
pub const VIRTIO_SCSI_S_FUNCTION_REJECTED: u32 = 5;
/// Incorrect LUN.
pub const VIRTIO_SCSI_S_INCORRECT_LUN: u32 = 12;

// ---------------------------------------------------------------------------
// VirtIO SCSI feature bits (VIRTIO_SCSI_F_*)
// ---------------------------------------------------------------------------

/// Support hotplug events.
pub const VIRTIO_SCSI_F_HOTPLUG: u32 = 1;
/// Support change events (media change, etc.).
pub const VIRTIO_SCSI_F_CHANGE: u32 = 2;
/// Support T10-PI (DIF/DIX) data protection.
pub const VIRTIO_SCSI_F_T10_PI: u32 = 3;

// ---------------------------------------------------------------------------
// VirtIO SCSI event types
// ---------------------------------------------------------------------------

/// No event.
pub const VIRTIO_SCSI_T_NO_EVENT: u32 = 0;
/// Transport reset event.
pub const VIRTIO_SCSI_T_TRANSPORT_RESET: u32 = 1;
/// Async notification (unit attention).
pub const VIRTIO_SCSI_T_ASYNC_NOTIFY: u32 = 2;
/// Parameter change event.
pub const VIRTIO_SCSI_T_PARAM_CHANGE: u32 = 3;

// ---------------------------------------------------------------------------
// VirtIO SCSI event reasons
// ---------------------------------------------------------------------------

/// Target removed.
pub const VIRTIO_SCSI_EVT_RESET_REMOVED: u32 = 1;
/// Target rescan needed.
pub const VIRTIO_SCSI_EVT_RESET_RESCAN: u32 = 2;
/// Hard reset.
pub const VIRTIO_SCSI_EVT_RESET_HARD: u32 = 3;

// ---------------------------------------------------------------------------
// CDB and sense buffer sizes
// ---------------------------------------------------------------------------

/// Maximum CDB (Command Descriptor Block) size.
pub const VIRTIO_SCSI_CDB_SIZE: u32 = 32;
/// Maximum sense data size.
pub const VIRTIO_SCSI_SENSE_SIZE: u32 = 96;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tmf_types_distinct() {
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
    fn test_response_codes_distinct() {
        let codes = [
            VIRTIO_SCSI_S_OK, VIRTIO_SCSI_S_OVERRUN,
            VIRTIO_SCSI_S_ABORTED, VIRTIO_SCSI_S_BAD_TARGET,
            VIRTIO_SCSI_S_RESET, VIRTIO_SCSI_S_TRANSPORT_FAILURE,
            VIRTIO_SCSI_S_TARGET_FAILURE, VIRTIO_SCSI_S_NEXUS_FAILURE,
            VIRTIO_SCSI_S_FAILURE, VIRTIO_SCSI_S_INCORRECT_LUN,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_features_distinct() {
        let feats = [VIRTIO_SCSI_F_HOTPLUG, VIRTIO_SCSI_F_CHANGE, VIRTIO_SCSI_F_T10_PI];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            VIRTIO_SCSI_T_NO_EVENT, VIRTIO_SCSI_T_TRANSPORT_RESET,
            VIRTIO_SCSI_T_ASYNC_NOTIFY, VIRTIO_SCSI_T_PARAM_CHANGE,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_sizes() {
        assert_eq!(VIRTIO_SCSI_CDB_SIZE, 32);
        assert_eq!(VIRTIO_SCSI_SENSE_SIZE, 96);
    }
}
