//! `<linux/isst_if.h>` — Intel Speed Select Technology (ISST) constants.
//!
//! Intel SST allows configuring CPU performance profiles at runtime
//! on supported server CPUs (Xeon Scalable). Features include:
//! - Speed Select Base Frequency (SST-BF): higher base frequency on a
//!   subset of cores
//! - Speed Select Core Power (SST-CP): assign cores to priority
//!   classes with different turbo budgets
//! - Speed Select Turbo Frequency (SST-TF): guaranteed higher turbo
//!   on high-priority cores
//! Controlled via MSR or MMIO mailbox through /dev/isst_interface.

// ---------------------------------------------------------------------------
// ISST IOCTL commands
// ---------------------------------------------------------------------------

/// Get ISST hardware info.
pub const ISST_IF_GET_PHY_ID: u32 = 0x01;
/// Get ISST platform info.
pub const ISST_IF_GET_PLATFORM_INFO: u32 = 0x02;
/// Send mailbox command (MSR interface).
pub const ISST_IF_MBOX_COMMAND: u32 = 0x03;
/// Send MMIO command.
pub const ISST_IF_MMIO_COMMAND: u32 = 0x04;
/// Get number of ISST instances.
pub const ISST_IF_COUNT_TPMI_INSTANCES: u32 = 0x05;

// ---------------------------------------------------------------------------
// ISST mailbox sub-commands
// ---------------------------------------------------------------------------

/// Get TDP (Thermal Design Power) level info.
pub const ISST_MBOX_GET_TDP_INFO: u32 = 0x00;
/// Set TDP level.
pub const ISST_MBOX_SET_TDP_LEVEL: u32 = 0x01;
/// Get core priority mapping.
pub const ISST_MBOX_GET_CORE_PRIORITY: u32 = 0x02;
/// Set core priority.
pub const ISST_MBOX_SET_CORE_PRIORITY: u32 = 0x03;
/// Get CLOS (Class of Service) configuration.
pub const ISST_MBOX_GET_CLOS_INFO: u32 = 0x04;
/// Set CLOS parameters.
pub const ISST_MBOX_SET_CLOS_PARAM: u32 = 0x05;
/// Get turbo frequency buckets.
pub const ISST_MBOX_GET_TURBO_FREQ: u32 = 0x06;
/// Get base frequency info.
pub const ISST_MBOX_GET_BASE_FREQ: u32 = 0x07;

// ---------------------------------------------------------------------------
// ISST feature flags
// ---------------------------------------------------------------------------

/// SST Performance Profile (PP) supported.
pub const ISST_FEAT_PP: u32 = 1 << 0;
/// SST Base Frequency (BF) supported.
pub const ISST_FEAT_BF: u32 = 1 << 1;
/// SST Turbo Frequency (TF) supported.
pub const ISST_FEAT_TF: u32 = 1 << 2;
/// SST Core Power (CP) supported.
pub const ISST_FEAT_CP: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// CLOS (Class of Service) indices
// ---------------------------------------------------------------------------

/// CLOS class 0 (default, lowest priority).
pub const ISST_CLOS_0: u32 = 0;
/// CLOS class 1.
pub const ISST_CLOS_1: u32 = 1;
/// CLOS class 2.
pub const ISST_CLOS_2: u32 = 2;
/// CLOS class 3 (highest priority).
pub const ISST_CLOS_3: u32 = 3;

/// Maximum number of CLOS classes.
pub const ISST_CLOS_MAX: u32 = 4;

// ---------------------------------------------------------------------------
// TDP level range
// ---------------------------------------------------------------------------

/// Maximum number of TDP config levels.
pub const ISST_TDP_MAX_LEVELS: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let ioctls = [
            ISST_IF_GET_PHY_ID, ISST_IF_GET_PLATFORM_INFO,
            ISST_IF_MBOX_COMMAND, ISST_IF_MMIO_COMMAND,
            ISST_IF_COUNT_TPMI_INSTANCES,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_mbox_commands_distinct() {
        let cmds = [
            ISST_MBOX_GET_TDP_INFO, ISST_MBOX_SET_TDP_LEVEL,
            ISST_MBOX_GET_CORE_PRIORITY, ISST_MBOX_SET_CORE_PRIORITY,
            ISST_MBOX_GET_CLOS_INFO, ISST_MBOX_SET_CLOS_PARAM,
            ISST_MBOX_GET_TURBO_FREQ, ISST_MBOX_GET_BASE_FREQ,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_features_no_overlap() {
        let feats = [ISST_FEAT_PP, ISST_FEAT_BF, ISST_FEAT_TF, ISST_FEAT_CP];
        for i in 0..feats.len() {
            assert!(feats[i].is_power_of_two());
            for j in (i + 1)..feats.len() {
                assert_eq!(feats[i] & feats[j], 0);
            }
        }
    }

    #[test]
    fn test_clos_ordered() {
        assert!(ISST_CLOS_0 < ISST_CLOS_1);
        assert!(ISST_CLOS_1 < ISST_CLOS_2);
        assert!(ISST_CLOS_2 < ISST_CLOS_3);
        assert!(ISST_CLOS_3 < ISST_CLOS_MAX);
    }

    #[test]
    fn test_tdp_max_levels() {
        assert_eq!(ISST_TDP_MAX_LEVELS, 8);
        assert!(ISST_TDP_MAX_LEVELS.is_power_of_two());
    }
}
