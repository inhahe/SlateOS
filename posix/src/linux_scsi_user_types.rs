//! `<scsi/sg.h>` — SCSI generic (`/dev/sgN`) ABI.
//!
//! sg lets userspace send arbitrary SCSI CDBs to a device — used by
//! `cdrecord`, `sg_inq`, `hdparm`, `smartmontools`, and any program
//! that needs out-of-band access to a SCSI/ATA/USB-mass-storage
//! device. The ioctls below are the stable subset.

// ---------------------------------------------------------------------------
// `/dev/sgN` ioctls (`SG_*`)
// ---------------------------------------------------------------------------

pub const SG_DXFER_NONE: i32 = -1;
pub const SG_DXFER_TO_DEV: i32 = -2;
pub const SG_DXFER_FROM_DEV: i32 = -3;
pub const SG_DXFER_TO_FROM_DEV: i32 = -4;

pub const SG_IO: u32 = 0x2285;
pub const SG_GET_VERSION_NUM: u32 = 0x2282;
pub const SG_SET_TIMEOUT: u32 = 0x2201;
pub const SG_GET_TIMEOUT: u32 = 0x2202;
pub const SG_EMULATED_HOST: u32 = 0x2203;
pub const SG_GET_COMMAND_Q: u32 = 0x2270;
pub const SG_SET_COMMAND_Q: u32 = 0x2271;
pub const SG_GET_RESERVED_SIZE: u32 = 0x2272;
pub const SG_SET_RESERVED_SIZE: u32 = 0x2275;
pub const SG_GET_SCSI_ID: u32 = 0x2276;
pub const SG_SET_FORCE_LOW_DMA: u32 = 0x2279;
pub const SG_GET_LOW_DMA: u32 = 0x227A;
pub const SG_SET_FORCE_PACK_ID: u32 = 0x227B;
pub const SG_GET_PACK_ID: u32 = 0x227C;
pub const SG_GET_NUM_WAITING: u32 = 0x227D;
pub const SG_GET_SG_TABLESIZE: u32 = 0x227F;

// ---------------------------------------------------------------------------
// sg driver version + buffer sizes
// ---------------------------------------------------------------------------

/// Major driver version (`sg.c` reports 3 for v3 ABI / 4 for v4).
pub const SG_VERSION_3: u32 = 30536;
pub const SG_VERSION_4: u32 = 40045;

/// Default reserved buffer size (`SG_DEF_RESERVED_SIZE`).
pub const SG_DEF_RESERVED_SIZE: u32 = 32 * 1024;

/// Maximum sense-buffer length the kernel will copy back.
pub const SG_MAX_SENSE_LEN: usize = 96;

// ---------------------------------------------------------------------------
// SCSI status bytes (subset)
// ---------------------------------------------------------------------------

pub const SCSI_STATUS_GOOD: u8 = 0x00;
pub const SCSI_STATUS_CHECK_CONDITION: u8 = 0x02;
pub const SCSI_STATUS_CONDITION_MET: u8 = 0x04;
pub const SCSI_STATUS_BUSY: u8 = 0x08;
pub const SCSI_STATUS_RESERVATION_CONFLICT: u8 = 0x18;
pub const SCSI_STATUS_TASK_SET_FULL: u8 = 0x28;
pub const SCSI_STATUS_ACA_ACTIVE: u8 = 0x30;
pub const SCSI_STATUS_TASK_ABORTED: u8 = 0x40;

// ---------------------------------------------------------------------------
// Common SCSI opcodes (subset — used by every userspace SCSI tool)
// ---------------------------------------------------------------------------

pub const SCSI_TEST_UNIT_READY: u8 = 0x00;
pub const SCSI_REQUEST_SENSE: u8 = 0x03;
pub const SCSI_INQUIRY: u8 = 0x12;
pub const SCSI_MODE_SENSE_6: u8 = 0x1A;
pub const SCSI_START_STOP_UNIT: u8 = 0x1B;
pub const SCSI_READ_CAPACITY_10: u8 = 0x25;
pub const SCSI_READ_10: u8 = 0x28;
pub const SCSI_WRITE_10: u8 = 0x2A;
pub const SCSI_SYNCHRONIZE_CACHE_10: u8 = 0x35;
pub const SCSI_REPORT_LUNS: u8 = 0xA0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dxfer_negative_distinct() {
        let d = [SG_DXFER_NONE, SG_DXFER_TO_DEV, SG_DXFER_FROM_DEV, SG_DXFER_TO_FROM_DEV];
        for a in 0..d.len() {
            for b in (a + 1)..d.len() {
                assert_ne!(d[a], d[b]);
            }
            // All four direction sentinels are negative.
            assert!(d[a] < 0);
        }
    }

    #[test]
    fn test_sg_ioctls_in_0x22xx_range() {
        let i = [
            SG_IO,
            SG_GET_VERSION_NUM,
            SG_SET_TIMEOUT,
            SG_GET_TIMEOUT,
            SG_EMULATED_HOST,
            SG_GET_COMMAND_Q,
            SG_SET_COMMAND_Q,
            SG_GET_RESERVED_SIZE,
            SG_SET_RESERVED_SIZE,
            SG_GET_SCSI_ID,
            SG_GET_PACK_ID,
            SG_GET_NUM_WAITING,
            SG_GET_SG_TABLESIZE,
        ];
        for &v in i.iter() {
            // All sg ioctls live in 0x2200..=0x22FF.
            assert_eq!(v & 0xFF00, 0x2200);
        }
    }

    #[test]
    fn test_version_v3_lower_than_v4() {
        // Encoded as MMmmm (Major * 10000 + minor * 100 + patch).
        assert!(SG_VERSION_3 < SG_VERSION_4);
        assert!(SG_VERSION_3 >= 30000);
        assert!(SG_VERSION_4 >= 40000);
    }

    #[test]
    fn test_reserved_size_default_and_sense_cap() {
        assert_eq!(SG_DEF_RESERVED_SIZE, 32 * 1024);
        assert!(SG_DEF_RESERVED_SIZE.is_power_of_two());
        assert_eq!(SG_MAX_SENSE_LEN, 96);
    }

    #[test]
    fn test_scsi_status_distinct_and_check_condition_is_2() {
        let s = [
            SCSI_STATUS_GOOD,
            SCSI_STATUS_CHECK_CONDITION,
            SCSI_STATUS_CONDITION_MET,
            SCSI_STATUS_BUSY,
            SCSI_STATUS_RESERVATION_CONFLICT,
            SCSI_STATUS_TASK_SET_FULL,
            SCSI_STATUS_ACA_ACTIVE,
            SCSI_STATUS_TASK_ABORTED,
        ];
        for a in 0..s.len() {
            for b in (a + 1)..s.len() {
                assert_ne!(s[a], s[b]);
            }
        }
        // CHECK CONDITION is the famous 0x02 — the trigger for REQUEST SENSE.
        assert_eq!(SCSI_STATUS_CHECK_CONDITION, 0x02);
        assert_eq!(SCSI_STATUS_GOOD, 0x00);
    }

    #[test]
    fn test_opcode_well_known_values() {
        // These five opcodes appear in every SCSI tutorial.
        assert_eq!(SCSI_TEST_UNIT_READY, 0x00);
        assert_eq!(SCSI_INQUIRY, 0x12);
        assert_eq!(SCSI_READ_10, 0x28);
        assert_eq!(SCSI_WRITE_10, 0x2A);
        assert_eq!(SCSI_REPORT_LUNS, 0xA0);
        // READ(10) and WRITE(10) are two opcodes apart.
        assert_eq!(SCSI_WRITE_10 - SCSI_READ_10, 2);
    }
}
