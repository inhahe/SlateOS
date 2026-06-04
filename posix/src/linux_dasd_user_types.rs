//! `<uapi/asm/dasd.h>` — s390 DASD (Direct-Access Storage Device) ioctls.
//!
//! DASDs are IBM mainframe disks accessed via channel programs. The
//! Linux driver presents them as block devices with custom ioctls for
//! ECKD/FBA-specific features: reservation, format, profiling, PAV
//! (parallel access volume) aliasing, etc.

// ---------------------------------------------------------------------------
// ioctl magic
// ---------------------------------------------------------------------------

/// DASD ioctl magic byte (DASD_IOCTL_LETTER).
pub const DASD_IOCTL_LETTER: u8 = b'D';

// ---------------------------------------------------------------------------
// Common ioctl ordinals (subset)
// ---------------------------------------------------------------------------

pub const BIODASDDISABLE_NR: u8 = 0;
pub const BIODASDENABLE_NR: u8 = 1;
pub const BIODASDRSRV_NR: u8 = 2;
pub const BIODASDRLSE_NR: u8 = 3;
pub const BIODASDSLCK_NR: u8 = 4;
pub const BIODASDINFO_NR: u8 = 1;
pub const BIODASDINFO2_NR: u8 = 3;
pub const BIODASDFMT_NR: u8 = 0;
pub const BIODASDREADCMB_NR: u8 = 0x40;
pub const BIODASDRESETCMB_NR: u8 = 0x41;
pub const BIODASDPRRD_NR: u8 = 0x44;
pub const BIODASDPRRST_NR: u8 = 0x45;

// ---------------------------------------------------------------------------
// struct dasd_information / dasd_information2 sizes
// ---------------------------------------------------------------------------

pub const DASD_TYPE_LEN: usize = 4;
pub const DASD_MODEL_LEN: usize = 4;
pub const DASD_DEV_ID_LEN: usize = 4;

/// struct dasd_information layout (FBA/ECKD common info).
pub const DASD_INFORMATION_DEVNO_OFF: usize = 0;
pub const DASD_INFORMATION_REAL_DEVNO_OFF: usize = 4;
pub const DASD_INFORMATION_SCHID_OFF: usize = 8;
pub const DASD_INFORMATION_CU_TYPE_OFF: usize = 12;
pub const DASD_INFORMATION_CU_MODEL_OFF: usize = 14;
pub const DASD_INFORMATION_DEV_TYPE_OFF: usize = 16;
pub const DASD_INFORMATION_DEV_MODEL_OFF: usize = 18;
pub const DASD_INFORMATION_OPEN_COUNT_OFF: usize = 20;
pub const DASD_INFORMATION_REQ_QUEUE_LEN_OFF: usize = 24;
pub const DASD_INFORMATION_CHANQ_LEN_OFF: usize = 28;

// ---------------------------------------------------------------------------
// State flag bits (used in struct dasd_information::status)
// ---------------------------------------------------------------------------

pub const DASD_STATE_NEW: u32 = 0;
pub const DASD_STATE_KNOWN: u32 = 1;
pub const DASD_STATE_BASIC: u32 = 2;
pub const DASD_STATE_UNFMT: u32 = 3;
pub const DASD_STATE_READY: u32 = 4;
pub const DASD_STATE_ONLINE: u32 = 5;

// ---------------------------------------------------------------------------
// Format intensity flags (DASDFMT)
// ---------------------------------------------------------------------------

pub const DASD_FMT_INT_FMT_NOR0: u32 = 1;
pub const DASD_FMT_INT_FMT_R0: u32 = 2;
pub const DASD_FMT_INT_INVAL: u32 = 4;
pub const DASD_FMT_INT_COMPAT: u32 = 8;

// ---------------------------------------------------------------------------
// DASD feature flags (sysfs)
// ---------------------------------------------------------------------------

pub const DASD_FEATURE_DEFAULT: u32 = 0;
pub const DASD_FEATURE_READONLY: u32 = 1 << 0;
pub const DASD_FEATURE_USEDIAG: u32 = 1 << 1;
pub const DASD_FEATURE_INITIAL_ONLINE: u32 = 1 << 2;
pub const DASD_FEATURE_ERPLOG: u32 = 1 << 3;
pub const DASD_FEATURE_FAILFAST: u32 = 1 << 4;
pub const DASD_FEATURE_FAILONSLCK: u32 = 1 << 5;
pub const DASD_FEATURE_SAFE_OFFLINE: u32 = 1 << 6;
pub const DASD_FEATURE_PATH_VERIFICATION: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// PAV (Parallel Access Volume) limits
// ---------------------------------------------------------------------------

/// Maximum number of aliases per DASD base device.
pub const DASD_MAX_PAV_ALIASES: u32 = 256;
/// Maximum number of paths per device.
pub const DASD_MAX_PATHS: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_letter_is_D() {
        assert_eq!(DASD_IOCTL_LETTER, b'D');
        assert_eq!(DASD_IOCTL_LETTER, 0x44);
    }

    #[test]
    fn test_id_field_lengths_are_4() {
        assert_eq!(DASD_TYPE_LEN, 4);
        assert_eq!(DASD_MODEL_LEN, 4);
        assert_eq!(DASD_DEV_ID_LEN, 4);
    }

    #[test]
    fn test_state_progression_dense_0_to_5() {
        let s = [
            DASD_STATE_NEW,
            DASD_STATE_KNOWN,
            DASD_STATE_BASIC,
            DASD_STATE_UNFMT,
            DASD_STATE_READY,
            DASD_STATE_ONLINE,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_state_ordering_invariants() {
        assert!(DASD_STATE_NEW < DASD_STATE_KNOWN);
        assert!(DASD_STATE_KNOWN < DASD_STATE_READY);
        assert!(DASD_STATE_READY < DASD_STATE_ONLINE);
    }

    #[test]
    fn test_format_flags_powers_of_two() {
        for f in [
            DASD_FMT_INT_FMT_NOR0,
            DASD_FMT_INT_FMT_R0,
            DASD_FMT_INT_INVAL,
            DASD_FMT_INT_COMPAT,
        ] {
            assert!(f.is_power_of_two());
        }
        // All four are disjoint.
        assert_eq!(
            DASD_FMT_INT_FMT_NOR0
                | DASD_FMT_INT_FMT_R0
                | DASD_FMT_INT_INVAL
                | DASD_FMT_INT_COMPAT,
            0x0F
        );
    }

    #[test]
    fn test_feature_flags_single_bit_distinct() {
        let f = [
            DASD_FEATURE_READONLY,
            DASD_FEATURE_USEDIAG,
            DASD_FEATURE_INITIAL_ONLINE,
            DASD_FEATURE_ERPLOG,
            DASD_FEATURE_FAILFAST,
            DASD_FEATURE_FAILONSLCK,
            DASD_FEATURE_SAFE_OFFLINE,
            DASD_FEATURE_PATH_VERIFICATION,
        ];
        let mut or_all = 0u32;
        for &v in &f {
            assert!(v.is_power_of_two());
            or_all |= v;
        }
        assert_eq!(or_all, 0xFF);
        assert_eq!(DASD_FEATURE_DEFAULT, 0);
    }

    #[test]
    fn test_info_struct_offsets_strictly_increasing() {
        let off = [
            DASD_INFORMATION_DEVNO_OFF,
            DASD_INFORMATION_REAL_DEVNO_OFF,
            DASD_INFORMATION_SCHID_OFF,
            DASD_INFORMATION_CU_TYPE_OFF,
            DASD_INFORMATION_CU_MODEL_OFF,
            DASD_INFORMATION_DEV_TYPE_OFF,
            DASD_INFORMATION_DEV_MODEL_OFF,
            DASD_INFORMATION_OPEN_COUNT_OFF,
            DASD_INFORMATION_REQ_QUEUE_LEN_OFF,
            DASD_INFORMATION_CHANQ_LEN_OFF,
        ];
        for w in off.windows(2) {
            assert!(w[1] > w[0]);
        }
    }

    #[test]
    fn test_pav_alias_limit() {
        assert_eq!(DASD_MAX_PAV_ALIASES, 256);
        assert!(DASD_MAX_PAV_ALIASES.is_power_of_two());
        assert_eq!(DASD_MAX_PATHS, 8);
    }

    #[test]
    fn test_reserve_release_lock_ordinals_dense() {
        // RSRV→RLSE→SLCK are 2,3,4.
        assert_eq!(BIODASDRSRV_NR + 1, BIODASDRLSE_NR);
        assert_eq!(BIODASDRLSE_NR + 1, BIODASDSLCK_NR);
    }
}
