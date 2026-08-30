//! `<uapi/linux/cxl_mem.h>` — CXL memory device sysfs / cdev interface.
//!
//! Each CXL memory device appears as `/dev/cxl/memN` (char device) plus a
//! sysfs entry under `/sys/bus/cxl/devices/memN/`. The char device
//! supports ioctls for sending raw mailbox commands; sysfs exposes
//! device identity, partition sizes, and health metrics.

// ---------------------------------------------------------------------------
// Sysfs / char-device paths
// ---------------------------------------------------------------------------

pub const CXL_SYSFS_BUS: &str = "/sys/bus/cxl";
pub const CXL_SYSFS_DEVICES: &str = "/sys/bus/cxl/devices";
pub const CXL_DEV_PREFIX: &str = "/dev/cxl/mem";

// ---------------------------------------------------------------------------
// Per-device sysfs attribute filenames
// ---------------------------------------------------------------------------

pub const CXL_ATTR_FIRMWARE_VERSION: &str = "firmware_version";
pub const CXL_ATTR_PAYLOAD_MAX: &str = "payload_max";
pub const CXL_ATTR_LABEL_STORAGE_SIZE: &str = "label_storage_size";
pub const CXL_ATTR_RAM_SIZE: &str = "ram_size";
pub const CXL_ATTR_PMEM_SIZE: &str = "pmem_size";
pub const CXL_ATTR_SERIAL: &str = "serial";
pub const CXL_ATTR_NUMA_NODE: &str = "numa_node";

// ---------------------------------------------------------------------------
// ioctl numbers (CXL_MEM_QUERY_COMMANDS / CXL_MEM_SEND_COMMAND)
// ---------------------------------------------------------------------------

/// ioctl magic byte for /dev/cxl/memN.
pub const CXL_MEM_IOC_MAGIC: u8 = 0xCE;
/// CXL_MEM_QUERY_COMMANDS ioctl number (subset of magic).
pub const CXL_MEM_QUERY_COMMANDS: u8 = 1;
/// CXL_MEM_SEND_COMMAND ioctl number.
pub const CXL_MEM_SEND_COMMAND: u8 = 2;

// ---------------------------------------------------------------------------
// User-visible command IDs (kernel-defined, opaque opcodes)
// ---------------------------------------------------------------------------

pub const CXL_MEM_COMMAND_ID_INVALID: u32 = 0;
pub const CXL_MEM_COMMAND_ID_IDENTIFY: u32 = 1;
pub const CXL_MEM_COMMAND_ID_RAW: u32 = 2;
pub const CXL_MEM_COMMAND_ID_GET_SUPPORTED_LOGS: u32 = 3;
pub const CXL_MEM_COMMAND_ID_GET_FW_INFO: u32 = 4;
pub const CXL_MEM_COMMAND_ID_GET_PARTITION_INFO: u32 = 5;
pub const CXL_MEM_COMMAND_ID_GET_LSA: u32 = 6;
pub const CXL_MEM_COMMAND_ID_GET_HEALTH_INFO: u32 = 7;
pub const CXL_MEM_COMMAND_ID_GET_LOG: u32 = 8;
pub const CXL_MEM_COMMAND_ID_SET_PARTITION_INFO: u32 = 9;
pub const CXL_MEM_COMMAND_ID_SET_LSA: u32 = 10;
pub const CXL_MEM_COMMAND_ID_GET_ALERT_CONFIG: u32 = 11;
pub const CXL_MEM_COMMAND_ID_SET_ALERT_CONFIG: u32 = 12;
pub const CXL_MEM_COMMAND_ID_GET_SHUTDOWN_STATE: u32 = 13;
pub const CXL_MEM_COMMAND_ID_SET_SHUTDOWN_STATE: u32 = 14;
pub const CXL_MEM_COMMAND_ID_GET_POISON: u32 = 15;
pub const CXL_MEM_COMMAND_ID_INJECT_POISON: u32 = 16;
pub const CXL_MEM_COMMAND_ID_CLEAR_POISON: u32 = 17;
pub const CXL_MEM_COMMAND_ID_GET_SCAN_MEDIA_CAPS: u32 = 18;
pub const CXL_MEM_COMMAND_ID_SCAN_MEDIA: u32 = 19;
pub const CXL_MEM_COMMAND_ID_GET_SCAN_MEDIA: u32 = 20;
pub const CXL_MEM_COMMAND_ID_MAX: u32 = CXL_MEM_COMMAND_ID_GET_SCAN_MEDIA;

// ---------------------------------------------------------------------------
// Flags on struct cxl_send_command
// ---------------------------------------------------------------------------

pub const CXL_MEM_COMMAND_FLAG_NONE: u32 = 0;
/// Raw flag — forward arbitrary opcode without kernel validation.
pub const CXL_MEM_COMMAND_FLAG_RAW: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Sysfs values for memdev state
// ---------------------------------------------------------------------------

pub const CXL_STATE_ACTIVE: &str = "active";
pub const CXL_STATE_OFFLINE: &str = "offline";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_paths_under_sys_bus_cxl() {
        assert!(CXL_SYSFS_DEVICES.starts_with(CXL_SYSFS_BUS));
        assert_eq!(CXL_SYSFS_BUS, "/sys/bus/cxl");
    }

    #[test]
    fn test_dev_path_prefix() {
        assert_eq!(CXL_DEV_PREFIX, "/dev/cxl/mem");
        assert!(CXL_DEV_PREFIX.starts_with("/dev/"));
    }

    #[test]
    fn test_size_attrs_have_size_suffix() {
        for a in [
            CXL_ATTR_RAM_SIZE,
            CXL_ATTR_PMEM_SIZE,
            CXL_ATTR_LABEL_STORAGE_SIZE,
            CXL_ATTR_PAYLOAD_MAX,
        ] {
            // All size-bearing attrs include the word "size" or "max".
            assert!(
                a.contains("size") || a.contains("max"),
                "attr {a:?} lacks size/max"
            );
        }
    }

    #[test]
    fn test_ioctl_magic_is_ce() {
        assert_eq!(CXL_MEM_IOC_MAGIC, 0xCE);
    }

    #[test]
    fn test_ioctl_subops_distinct() {
        assert_ne!(CXL_MEM_QUERY_COMMANDS, CXL_MEM_SEND_COMMAND);
        assert_eq!(CXL_MEM_QUERY_COMMANDS, 1);
        assert_eq!(CXL_MEM_SEND_COMMAND, 2);
    }

    #[test]
    fn test_command_ids_dense_0_to_max() {
        let c = [
            CXL_MEM_COMMAND_ID_INVALID,
            CXL_MEM_COMMAND_ID_IDENTIFY,
            CXL_MEM_COMMAND_ID_RAW,
            CXL_MEM_COMMAND_ID_GET_SUPPORTED_LOGS,
            CXL_MEM_COMMAND_ID_GET_FW_INFO,
            CXL_MEM_COMMAND_ID_GET_PARTITION_INFO,
            CXL_MEM_COMMAND_ID_GET_LSA,
            CXL_MEM_COMMAND_ID_GET_HEALTH_INFO,
            CXL_MEM_COMMAND_ID_GET_LOG,
            CXL_MEM_COMMAND_ID_SET_PARTITION_INFO,
            CXL_MEM_COMMAND_ID_SET_LSA,
            CXL_MEM_COMMAND_ID_GET_ALERT_CONFIG,
            CXL_MEM_COMMAND_ID_SET_ALERT_CONFIG,
            CXL_MEM_COMMAND_ID_GET_SHUTDOWN_STATE,
            CXL_MEM_COMMAND_ID_SET_SHUTDOWN_STATE,
            CXL_MEM_COMMAND_ID_GET_POISON,
            CXL_MEM_COMMAND_ID_INJECT_POISON,
            CXL_MEM_COMMAND_ID_CLEAR_POISON,
            CXL_MEM_COMMAND_ID_GET_SCAN_MEDIA_CAPS,
            CXL_MEM_COMMAND_ID_SCAN_MEDIA,
            CXL_MEM_COMMAND_ID_GET_SCAN_MEDIA,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(CXL_MEM_COMMAND_ID_MAX, CXL_MEM_COMMAND_ID_GET_SCAN_MEDIA);
    }

    #[test]
    fn test_raw_flag_single_bit() {
        assert!(CXL_MEM_COMMAND_FLAG_RAW.is_power_of_two());
        assert_eq!(CXL_MEM_COMMAND_FLAG_NONE, 0);
    }

    #[test]
    fn test_state_values_distinct() {
        assert_ne!(CXL_STATE_ACTIVE, CXL_STATE_OFFLINE);
    }
}
