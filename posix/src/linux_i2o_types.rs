//! `<linux/i2o-dev.h>` — Legacy I2O (Intelligent Input/Output) constants.
//!
//! Constants for the legacy I2O storage / RAID controller userspace
//! interface — message classes, group IDs, and ioctl commands shared
//! between `/dev/i2o/ctl*` and userspace management tools.

// ---------------------------------------------------------------------------
// I2O message function codes (high byte of the message header)
// ---------------------------------------------------------------------------

/// Utility message class.
pub const I2O_CMD_UTIL_NOP: u32 = 0x00;
/// Abort message.
pub const I2O_CMD_UTIL_ABORT: u32 = 0x01;
/// Claim resource.
pub const I2O_CMD_UTIL_CLAIM: u32 = 0x09;
/// Release resource.
pub const I2O_CMD_UTIL_RELEASE: u32 = 0x0B;
/// Get parameters.
pub const I2O_CMD_UTIL_PARAMS_GET: u32 = 0x06;
/// Set parameters.
pub const I2O_CMD_UTIL_PARAMS_SET: u32 = 0x05;
/// Configure event.
pub const I2O_CMD_UTIL_EVT_REGISTER: u32 = 0x13;
/// Acknowledge event.
pub const I2O_CMD_UTIL_EVT_ACK: u32 = 0x14;

// ---------------------------------------------------------------------------
// Storage class commands
// ---------------------------------------------------------------------------

/// Block read.
pub const I2O_CMD_BLOCK_READ: u32 = 0x30;
/// Block write.
pub const I2O_CMD_BLOCK_WRITE: u32 = 0x31;
/// Cache flush.
pub const I2O_CMD_BLOCK_CFLUSH: u32 = 0x37;
/// Media mount.
pub const I2O_CMD_BLOCK_MEJECT: u32 = 0x43;
/// Power management.
pub const I2O_CMD_BLOCK_MLOCK: u32 = 0x49;
/// Unlock medium.
pub const I2O_CMD_BLOCK_MUNLOCK: u32 = 0x4A;

// ---------------------------------------------------------------------------
// Executive class commands
// ---------------------------------------------------------------------------

/// Executive: status.
pub const I2O_CMD_STATUS_GET: u32 = 0xA0;
/// Executive: outbound init.
pub const I2O_CMD_OUTBOUND_INIT: u32 = 0xA1;
/// Executive: hardware reset.
pub const I2O_CMD_HRT_GET: u32 = 0xA8;
/// Executive: system enable.
pub const I2O_CMD_SYS_ENABLE: u32 = 0xD1;
/// Executive: system table set.
pub const I2O_CMD_SYS_TAB_SET: u32 = 0xA3;

// ---------------------------------------------------------------------------
// Class IDs (struct i2o_class_id)
// ---------------------------------------------------------------------------

/// Executive.
pub const I2O_CLASS_EXECUTIVE: u32 = 0x000;
/// DDM (Device Driver Module).
pub const I2O_CLASS_DDM: u32 = 0x001;
/// Random block storage.
pub const I2O_CLASS_RANDOM_BLOCK_STORAGE: u32 = 0x010;
/// Sequential storage (tape).
pub const I2O_CLASS_SEQUENTIAL_STORAGE: u32 = 0x011;
/// LAN port.
pub const I2O_CLASS_LAN: u32 = 0x020;
/// WAN port.
pub const I2O_CLASS_WAN: u32 = 0x021;
/// FibreChannel.
pub const I2O_CLASS_FIBRE_CHANNEL_PORT: u32 = 0x030;
/// SCSI peripheral.
pub const I2O_CLASS_SCSI_PERIPHERAL: u32 = 0x101;
/// Bus adapter.
pub const I2O_CLASS_BUS_ADAPTER: u32 = 0x080;

// ---------------------------------------------------------------------------
// ioctl numbers passed to /dev/i2o/ctl*
// ---------------------------------------------------------------------------

/// Get number of controllers in the system.
pub const I2OGETIOPS: u32 = 0x69_00;
/// Pass-through message.
pub const I2OPASSTHRU: u32 = 0x69_0C;
/// Get the HRT (hardware resource table).
pub const I2OHRTGET: u32 = 0x69_01;
/// Get the LCT (logical configuration table).
pub const I2OLCTGET: u32 = 0x69_02;
/// Get message-frame parameters.
pub const I2OPARMSGET: u32 = 0x69_03;
/// Set message-frame parameters.
pub const I2OPARMSSET: u32 = 0x69_04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_util_cmds_distinct() {
        let cmds = [
            I2O_CMD_UTIL_NOP,
            I2O_CMD_UTIL_ABORT,
            I2O_CMD_UTIL_CLAIM,
            I2O_CMD_UTIL_RELEASE,
            I2O_CMD_UTIL_PARAMS_GET,
            I2O_CMD_UTIL_PARAMS_SET,
            I2O_CMD_UTIL_EVT_REGISTER,
            I2O_CMD_UTIL_EVT_ACK,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_block_cmds_distinct() {
        let cmds = [
            I2O_CMD_BLOCK_READ,
            I2O_CMD_BLOCK_WRITE,
            I2O_CMD_BLOCK_CFLUSH,
            I2O_CMD_BLOCK_MEJECT,
            I2O_CMD_BLOCK_MLOCK,
            I2O_CMD_BLOCK_MUNLOCK,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_exec_cmds_distinct() {
        let cmds = [
            I2O_CMD_STATUS_GET,
            I2O_CMD_OUTBOUND_INIT,
            I2O_CMD_HRT_GET,
            I2O_CMD_SYS_ENABLE,
            I2O_CMD_SYS_TAB_SET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_classes_distinct() {
        let classes = [
            I2O_CLASS_EXECUTIVE,
            I2O_CLASS_DDM,
            I2O_CLASS_RANDOM_BLOCK_STORAGE,
            I2O_CLASS_SEQUENTIAL_STORAGE,
            I2O_CLASS_LAN,
            I2O_CLASS_WAN,
            I2O_CLASS_FIBRE_CHANNEL_PORT,
            I2O_CLASS_SCSI_PERIPHERAL,
            I2O_CLASS_BUS_ADAPTER,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            I2OGETIOPS,
            I2OPASSTHRU,
            I2OHRTGET,
            I2OLCTGET,
            I2OPARMSGET,
            I2OPARMSSET,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }
}
