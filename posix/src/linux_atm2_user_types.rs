//! `<linux/atmdev.h>` — ATM device sysfs and AAL-layer constants.
//!
//! Continuation of `linux_atm_user_types` covering the device-side
//! ioctls, AAL-5 framing constants, and the /sys/class/atm sysfs
//! layout used by the (legacy) `atmtools` userspace.

// ---------------------------------------------------------------------------
// Device-node paths
// ---------------------------------------------------------------------------

pub const DEV_ATM: &str = "/dev/atm";
pub const SYS_CLASS_ATM: &str = "/sys/class/atm";

// ---------------------------------------------------------------------------
// AAL5 (the only AAL Linux speaks natively to userspace)
// ---------------------------------------------------------------------------

/// Maximum SDU size — 64 KiB - 1 byte (16-bit `length` field in trailer).
pub const ATM_MAX_AAL5_PDU: u32 = 65_535;
/// AAL5 trailer is 8 bytes (control/length/CRC).
pub const ATM_AAL5_TRAILER: usize = 8;
/// SDU is padded to a 48-byte boundary before the trailer.
pub const ATM_AAL5_PAD_ALIGN: usize = 48;

// ---------------------------------------------------------------------------
// AAL types (selectable via `setsockopt SO_ATMQOS`)
// ---------------------------------------------------------------------------

pub const ATM_NO_AAL: u8 = 0;
pub const ATM_AAL0: u8 = 13;
pub const ATM_AAL1: u8 = 1;
pub const ATM_AAL2: u8 = 2;
pub const ATM_AAL34: u8 = 3;
pub const ATM_AAL5: u8 = 5;

// ---------------------------------------------------------------------------
// Service categories (`atm_qos.aal*.txtp.traffic_class`)
// ---------------------------------------------------------------------------

pub const ATM_NONE: u8 = 0;
pub const ATM_UBR: u8 = 1;
pub const ATM_CBR: u8 = 2;
pub const ATM_VBR: u8 = 3;
pub const ATM_ABR: u8 = 4;
pub const ATM_ANYCLASS: u8 = 5;

// ---------------------------------------------------------------------------
// Traffic shaping defaults (cells per second)
// ---------------------------------------------------------------------------

pub const ATM_PCR_MAX: u32 = 0xFFFFFF;
pub const ATM_MCR_DEFAULT: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_and_sysfs_paths() {
        assert_eq!(DEV_ATM, "/dev/atm");
        assert_eq!(SYS_CLASS_ATM, "/sys/class/atm");
        assert!(DEV_ATM.starts_with("/dev/"));
        assert!(SYS_CLASS_ATM.starts_with("/sys/class/"));
    }

    #[test]
    fn test_aal5_constants() {
        // 65535 fits in a 16-bit length word.
        assert_eq!(ATM_MAX_AAL5_PDU, 0xFFFF);
        assert!(ATM_MAX_AAL5_PDU as usize > ATM_AAL5_TRAILER);
        assert_eq!(ATM_AAL5_TRAILER, 8);
        assert_eq!(ATM_AAL5_PAD_ALIGN, 48);
    }

    #[test]
    fn test_aal_types_distinct() {
        let a = [
            ATM_NO_AAL, ATM_AAL0, ATM_AAL1, ATM_AAL2, ATM_AAL34, ATM_AAL5,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // AAL5 is the dominant data layer.
        assert_eq!(ATM_AAL5, 5);
        // AAL0 (raw cells) was assigned a high value (13) to keep
        // legacy AAL1..5 in the low byte unambiguous.
        assert_eq!(ATM_AAL0, 13);
    }

    #[test]
    fn test_service_categories_dense_0_to_5() {
        let s = [
            ATM_NONE,
            ATM_UBR,
            ATM_CBR,
            ATM_VBR,
            ATM_ABR,
            ATM_ANYCLASS,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_pcr_max_is_24_bit() {
        // PCR is stored in 24 bits — max value 0x00FF_FFFF.
        assert_eq!(ATM_PCR_MAX, 0x00FF_FFFF);
        assert_eq!(ATM_MCR_DEFAULT, 0);
    }
}
