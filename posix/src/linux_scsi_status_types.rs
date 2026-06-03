//! `<scsi/scsi_proto.h>` (status subset) — SCSI status byte codes.
//!
//! After executing a command, a SCSI target returns a status byte
//! indicating the outcome. GOOD means success, CHECK CONDITION
//! means the command failed and sense data is available, BUSY
//! means the device is temporarily unavailable, etc.

// ---------------------------------------------------------------------------
// SCSI status codes (SAM status)
// ---------------------------------------------------------------------------

/// Command completed successfully.
pub const SAM_STAT_GOOD: u8 = 0x00;
/// Check condition: sense data available (error details).
pub const SAM_STAT_CHECK_CONDITION: u8 = 0x02;
/// Condition met (used with SEARCH DATA commands).
pub const SAM_STAT_CONDITION_MET: u8 = 0x04;
/// Device is busy (try again later).
pub const SAM_STAT_BUSY: u8 = 0x08;
/// Intermediate result (linked commands).
pub const SAM_STAT_INTERMEDIATE: u8 = 0x10;
/// Intermediate, condition met.
pub const SAM_STAT_INTERMEDIATE_CONDITION_MET: u8 = 0x14;
/// Reservation conflict (another initiator has reservation).
pub const SAM_STAT_RESERVATION_CONFLICT: u8 = 0x18;
/// Command terminated (deprecated in newer specs).
pub const SAM_STAT_COMMAND_TERMINATED: u8 = 0x22;
/// Task set full (queue is full).
pub const SAM_STAT_TASK_SET_FULL: u8 = 0x28;
/// ACA active (auto contingent allegiance).
pub const SAM_STAT_ACA_ACTIVE: u8 = 0x30;
/// Task aborted.
pub const SAM_STAT_TASK_ABORTED: u8 = 0x40;

// ---------------------------------------------------------------------------
// Host byte (scsi_cmnd.result >> 16)
// ---------------------------------------------------------------------------

/// No error from host adapter.
pub const DID_OK: u32 = 0x00;
/// Host adapter couldn't reach target.
pub const DID_NO_CONNECT: u32 = 0x01;
/// Bus busy.
pub const DID_BUS_BUSY: u32 = 0x02;
/// Timed out waiting for response.
pub const DID_TIME_OUT: u32 = 0x03;
/// Bad target (doesn't exist).
pub const DID_BAD_TARGET: u32 = 0x04;
/// Command aborted by host.
pub const DID_ABORT: u32 = 0x05;
/// Parity error on bus.
pub const DID_PARITY: u32 = 0x06;
/// Internal host adapter error.
pub const DID_ERROR: u32 = 0x07;
/// Host adapter reset.
pub const DID_RESET: u32 = 0x08;
/// Bad interrupt (spurious).
pub const DID_BAD_INTR: u32 = 0x09;
/// Ran out of memory.
pub const DID_ALLOC_FAILURE: u32 = 0x0C;
/// Transport error.
pub const DID_TRANSPORT_DISRUPTED: u32 = 0x0E;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sam_status_distinct() {
        let statuses = [
            SAM_STAT_GOOD,
            SAM_STAT_CHECK_CONDITION,
            SAM_STAT_CONDITION_MET,
            SAM_STAT_BUSY,
            SAM_STAT_INTERMEDIATE,
            SAM_STAT_INTERMEDIATE_CONDITION_MET,
            SAM_STAT_RESERVATION_CONFLICT,
            SAM_STAT_COMMAND_TERMINATED,
            SAM_STAT_TASK_SET_FULL,
            SAM_STAT_ACA_ACTIVE,
            SAM_STAT_TASK_ABORTED,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_good_is_zero() {
        assert_eq!(SAM_STAT_GOOD, 0);
    }

    #[test]
    fn test_host_bytes_distinct() {
        let hosts = [
            DID_OK,
            DID_NO_CONNECT,
            DID_BUS_BUSY,
            DID_TIME_OUT,
            DID_BAD_TARGET,
            DID_ABORT,
            DID_PARITY,
            DID_ERROR,
            DID_RESET,
            DID_BAD_INTR,
            DID_ALLOC_FAILURE,
            DID_TRANSPORT_DISRUPTED,
        ];
        for i in 0..hosts.len() {
            for j in (i + 1)..hosts.len() {
                assert_ne!(hosts[i], hosts[j]);
            }
        }
    }

    #[test]
    fn test_did_ok_is_zero() {
        assert_eq!(DID_OK, 0);
    }
}
