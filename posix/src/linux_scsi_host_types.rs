//! `<scsi/scsi_host.h>` — SCSI host adapter state and flag constants.
//!
//! SCSI host adapters (HBAs) manage the physical connection between
//! the system bus (PCI) and the SCSI transport (SAS, FC, iSCSI).
//! The host template describes the adapter's capabilities, and the
//! host state tracks its lifecycle from detection through removal.

// ---------------------------------------------------------------------------
// Host adapter states (scsi_host_state)
// ---------------------------------------------------------------------------

/// Host was created but not yet scanned.
pub const SHOST_CREATED: u32 = 1;
/// Host is running (normal operation).
pub const SHOST_RUNNING: u32 = 2;
/// Host cancel in progress (removing devices).
pub const SHOST_CANCEL: u32 = 3;
/// Host is being deleted.
pub const SHOST_DEL: u32 = 4;
/// Host error recovery in progress.
pub const SHOST_RECOVERY: u32 = 5;
/// Host cancel + recovery.
pub const SHOST_CANCEL_RECOVERY: u32 = 6;
/// Host recovery from transport error.
pub const SHOST_DEL_RECOVERY: u32 = 7;

// ---------------------------------------------------------------------------
// Host template flags
// ---------------------------------------------------------------------------

/// Host supports SCSI EH (error handling).
pub const SHOST_EH_SCHEDULED: u32 = 1 << 0;
/// Host uses block layer MQ.
pub const SHOST_USE_BLK_MQ: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Error handling actions (scsi_eh_action)
// ---------------------------------------------------------------------------

/// Abort the failing command.
pub const SCSI_EH_ABORT_CMD: u32 = 0;
/// Reset the device (LUN reset).
pub const SCSI_EH_DEV_RESET: u32 = 1;
/// Reset the target (all LUNs on that target).
pub const SCSI_EH_TARGET_RESET: u32 = 2;
/// Reset the bus (all targets on that bus).
pub const SCSI_EH_BUS_RESET: u32 = 3;
/// Reset the host adapter.
pub const SCSI_EH_HOST_RESET: u32 = 4;

// ---------------------------------------------------------------------------
// Queue limits
// ---------------------------------------------------------------------------

/// Default max commands per LUN.
pub const SCSI_DEFAULT_CMD_PER_LUN: u32 = 1;
/// Default host queue depth.
pub const SCSI_DEFAULT_HOST_BLOCKED: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_states_distinct() {
        let states = [
            SHOST_CREATED,
            SHOST_RUNNING,
            SHOST_CANCEL,
            SHOST_DEL,
            SHOST_RECOVERY,
            SHOST_CANCEL_RECOVERY,
            SHOST_DEL_RECOVERY,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_eh_actions_ordered() {
        // Escalation order: abort → device reset → target → bus → host
        assert!(SCSI_EH_ABORT_CMD < SCSI_EH_DEV_RESET);
        assert!(SCSI_EH_DEV_RESET < SCSI_EH_TARGET_RESET);
        assert!(SCSI_EH_TARGET_RESET < SCSI_EH_BUS_RESET);
        assert!(SCSI_EH_BUS_RESET < SCSI_EH_HOST_RESET);
    }

    #[test]
    fn test_defaults() {
        assert!(SCSI_DEFAULT_CMD_PER_LUN > 0);
        assert!(SCSI_DEFAULT_HOST_BLOCKED > 0);
    }
}
