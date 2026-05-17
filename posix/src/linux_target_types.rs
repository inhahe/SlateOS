//! `<target/target_core_base.h>` — SCSI target subsystem constants.
//!
//! The Linux target subsystem (LIO/TCM) implements SCSI target mode,
//! allowing Linux to serve storage to remote initiators via iSCSI,
//! Fibre Channel, SRP, or USB gadget. It presents block devices,
//! files, or RAM as SCSI LUNs.

// ---------------------------------------------------------------------------
// Transport protocol types
// ---------------------------------------------------------------------------

/// iSCSI transport.
pub const TARGET_TRANSPORT_ISCSI: u8 = 0;
/// Fibre Channel transport.
pub const TARGET_TRANSPORT_FC: u8 = 1;
/// SRP (SCSI RDMA Protocol).
pub const TARGET_TRANSPORT_SRP: u8 = 2;
/// Loopback transport.
pub const TARGET_TRANSPORT_LOOP: u8 = 3;
/// USB gadget transport.
pub const TARGET_TRANSPORT_USB: u8 = 4;
/// XCOPY transport.
pub const TARGET_TRANSPORT_XCOPY: u8 = 5;

// ---------------------------------------------------------------------------
// Backend types (how storage is provided)
// ---------------------------------------------------------------------------

/// Block device backend (IBLOCK).
pub const TARGET_BACKEND_IBLOCK: u8 = 0;
/// File backend (FILEIO).
pub const TARGET_BACKEND_FILEIO: u8 = 1;
/// RAM disk backend (RD_MCP).
pub const TARGET_BACKEND_RAMDISK: u8 = 2;
/// Pass-through SCSI (pSCSI).
pub const TARGET_BACKEND_PSCSI: u8 = 3;
/// User-space backend (TCMU).
pub const TARGET_BACKEND_USER: u8 = 4;

// ---------------------------------------------------------------------------
// Task management functions
// ---------------------------------------------------------------------------

/// Abort task.
pub const TMR_ABORT_TASK: u8 = 1;
/// Abort task set.
pub const TMR_ABORT_TASK_SET: u8 = 2;
/// Clear ACA.
pub const TMR_CLEAR_ACA: u8 = 3;
/// Clear task set.
pub const TMR_CLEAR_TASK_SET: u8 = 4;
/// Logical Unit Reset.
pub const TMR_LUN_RESET: u8 = 5;
/// Target warm reset.
pub const TMR_TARGET_WARM_RESET: u8 = 6;
/// Target cold reset.
pub const TMR_TARGET_COLD_RESET: u8 = 7;

// ---------------------------------------------------------------------------
// LUN states
// ---------------------------------------------------------------------------

/// LUN is active.
pub const LUN_STATE_ACTIVE: u8 = 0;
/// LUN is offline.
pub const LUN_STATE_OFFLINE: u8 = 1;
/// LUN is transitioning.
pub const LUN_STATE_TRANSITION: u8 = 2;

// ---------------------------------------------------------------------------
// ALUA (Asymmetric Logical Unit Access) states
// ---------------------------------------------------------------------------

/// Active/Optimized.
pub const ALUA_ACCESS_ACTIVE_OPT: u8 = 0x00;
/// Active/Non-Optimized.
pub const ALUA_ACCESS_ACTIVE_NON_OPT: u8 = 0x01;
/// Standby.
pub const ALUA_ACCESS_STANDBY: u8 = 0x02;
/// Unavailable.
pub const ALUA_ACCESS_UNAVAILABLE: u8 = 0x03;
/// Transitioning.
pub const ALUA_ACCESS_TRANSITION: u8 = 0x0F;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transports_distinct() {
        let types = [
            TARGET_TRANSPORT_ISCSI, TARGET_TRANSPORT_FC,
            TARGET_TRANSPORT_SRP, TARGET_TRANSPORT_LOOP,
            TARGET_TRANSPORT_USB, TARGET_TRANSPORT_XCOPY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_backends_distinct() {
        let backends = [
            TARGET_BACKEND_IBLOCK, TARGET_BACKEND_FILEIO,
            TARGET_BACKEND_RAMDISK, TARGET_BACKEND_PSCSI,
            TARGET_BACKEND_USER,
        ];
        for i in 0..backends.len() {
            for j in (i + 1)..backends.len() {
                assert_ne!(backends[i], backends[j]);
            }
        }
    }

    #[test]
    fn test_tmr_functions_distinct() {
        let funcs = [
            TMR_ABORT_TASK, TMR_ABORT_TASK_SET, TMR_CLEAR_ACA,
            TMR_CLEAR_TASK_SET, TMR_LUN_RESET,
            TMR_TARGET_WARM_RESET, TMR_TARGET_COLD_RESET,
        ];
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                assert_ne!(funcs[i], funcs[j]);
            }
        }
    }

    #[test]
    fn test_alua_states_distinct() {
        let states = [
            ALUA_ACCESS_ACTIVE_OPT, ALUA_ACCESS_ACTIVE_NON_OPT,
            ALUA_ACCESS_STANDBY, ALUA_ACCESS_UNAVAILABLE,
            ALUA_ACCESS_TRANSITION,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
