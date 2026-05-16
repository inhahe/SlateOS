//! `<linux/target_core_base.h>` — SCSI target core constants.
//!
//! The LIO (Linux-IO) target subsystem provides a SCSI target
//! framework for building iSCSI targets, FC targets, SRP targets,
//! and vhost-scsi backends. This module defines transport types,
//! SAM task attributes, and backend types.

// ---------------------------------------------------------------------------
// Transport protocol IDs
// ---------------------------------------------------------------------------

/// Fibre Channel.
pub const SCSI_PROTOCOL_FCP: u32 = 0;
/// SPI (parallel SCSI).
pub const SCSI_PROTOCOL_SPI: u32 = 1;
/// SSA.
pub const SCSI_PROTOCOL_SSA: u32 = 2;
/// IEEE 1394.
pub const SCSI_PROTOCOL_SBP: u32 = 3;
/// SRP (SCSI RDMA).
pub const SCSI_PROTOCOL_SRP: u32 = 4;
/// iSCSI.
pub const SCSI_PROTOCOL_ISCSI: u32 = 5;
/// SAS.
pub const SCSI_PROTOCOL_SAS: u32 = 6;
/// ADT.
pub const SCSI_PROTOCOL_ADT: u32 = 7;
/// ATA.
pub const SCSI_PROTOCOL_ATA: u32 = 8;
/// USB Attached SCSI.
pub const SCSI_PROTOCOL_UAS: u32 = 9;

// ---------------------------------------------------------------------------
// SAM task attributes
// ---------------------------------------------------------------------------

/// Simple task attribute.
pub const TCM_SIMPLE_TAG: u32 = 0;
/// Head of queue.
pub const TCM_HEAD_TAG: u32 = 1;
/// Ordered task.
pub const TCM_ORDERED_TAG: u32 = 2;
/// ACA task.
pub const TCM_ACA_TAG: u32 = 3;

// ---------------------------------------------------------------------------
// Backend types
// ---------------------------------------------------------------------------

/// Block I/O backend.
pub const TRANSPORT_PLUGIN_PHBA_IBLOCK: u32 = 0;
/// File I/O backend.
pub const TRANSPORT_PLUGIN_PHBA_FILEIO: u32 = 1;
/// RAM disk backend.
pub const TRANSPORT_PLUGIN_PHBA_RAMDISK: u32 = 2;
/// User-space backend (TCMU).
pub const TRANSPORT_PLUGIN_PHBA_USER: u32 = 3;
/// Passthrough to physical device.
pub const TRANSPORT_PLUGIN_PHBA_PSCSI: u32 = 4;

// ---------------------------------------------------------------------------
// Target port group states (ALUA)
// ---------------------------------------------------------------------------

/// Active/Optimized.
pub const ALUA_ACCESS_STATE_ACTIVE_OPTIMIZED: u8 = 0x00;
/// Active/Non-Optimized.
pub const ALUA_ACCESS_STATE_ACTIVE_NON_OPTIMIZED: u8 = 0x01;
/// Standby.
pub const ALUA_ACCESS_STATE_STANDBY: u8 = 0x02;
/// Unavailable.
pub const ALUA_ACCESS_STATE_UNAVAILABLE: u8 = 0x03;
/// LBA dependent.
pub const ALUA_ACCESS_STATE_LBA_DEPENDENT: u8 = 0x04;
/// Offline.
pub const ALUA_ACCESS_STATE_OFFLINE: u8 = 0x0E;
/// Transitioning.
pub const ALUA_ACCESS_STATE_TRANSITION: u8 = 0x0F;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            SCSI_PROTOCOL_FCP, SCSI_PROTOCOL_SPI, SCSI_PROTOCOL_SSA,
            SCSI_PROTOCOL_SBP, SCSI_PROTOCOL_SRP, SCSI_PROTOCOL_ISCSI,
            SCSI_PROTOCOL_SAS, SCSI_PROTOCOL_ADT, SCSI_PROTOCOL_ATA,
            SCSI_PROTOCOL_UAS,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_task_attrs_distinct() {
        let attrs = [TCM_SIMPLE_TAG, TCM_HEAD_TAG, TCM_ORDERED_TAG, TCM_ACA_TAG];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_backends_distinct() {
        let backends = [
            TRANSPORT_PLUGIN_PHBA_IBLOCK, TRANSPORT_PLUGIN_PHBA_FILEIO,
            TRANSPORT_PLUGIN_PHBA_RAMDISK, TRANSPORT_PLUGIN_PHBA_USER,
            TRANSPORT_PLUGIN_PHBA_PSCSI,
        ];
        for i in 0..backends.len() {
            for j in (i + 1)..backends.len() {
                assert_ne!(backends[i], backends[j]);
            }
        }
    }

    #[test]
    fn test_alua_states_distinct() {
        let states = [
            ALUA_ACCESS_STATE_ACTIVE_OPTIMIZED,
            ALUA_ACCESS_STATE_ACTIVE_NON_OPTIMIZED,
            ALUA_ACCESS_STATE_STANDBY,
            ALUA_ACCESS_STATE_UNAVAILABLE,
            ALUA_ACCESS_STATE_LBA_DEPENDENT,
            ALUA_ACCESS_STATE_OFFLINE,
            ALUA_ACCESS_STATE_TRANSITION,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
