//! `<linux/ublk_cmd.h>` — UBLK (Userspace Block Device) constants.
//!
//! UBLK constants covering command opcodes, IO flags,
//! device parameters, and feature flags.

// ---------------------------------------------------------------------------
// UBLK command opcodes
// ---------------------------------------------------------------------------

/// Get queue affinity.
pub const UBLK_CMD_GET_QUEUE_AFFINITY: u32 = 0x01;
/// Get device info.
pub const UBLK_CMD_GET_DEV_INFO: u32 = 0x02;
/// Add device.
pub const UBLK_CMD_ADD_DEV: u32 = 0x04;
/// Delete device.
pub const UBLK_CMD_DEL_DEV: u32 = 0x05;
/// Start device.
pub const UBLK_CMD_START_DEV: u32 = 0x06;
/// Stop device.
pub const UBLK_CMD_STOP_DEV: u32 = 0x07;
/// Set device parameters.
pub const UBLK_CMD_SET_PARAMS: u32 = 0x08;
/// Get device parameters.
pub const UBLK_CMD_GET_PARAMS: u32 = 0x09;
/// Start user recovery.
pub const UBLK_CMD_START_USER_RECOVERY: u32 = 0x10;
/// End user recovery.
pub const UBLK_CMD_END_USER_RECOVERY: u32 = 0x11;
/// Get device info (v2).
pub const UBLK_CMD_GET_DEV_INFO2: u32 = 0x12;

// ---------------------------------------------------------------------------
// UBLK IO opcodes
// ---------------------------------------------------------------------------

/// Fetch request.
pub const UBLK_IO_FETCH_REQ: u32 = 0x20;
/// Commit and fetch.
pub const UBLK_IO_COMMIT_AND_FETCH_REQ: u32 = 0x21;
/// Need get data.
pub const UBLK_IO_NEED_GET_DATA: u32 = 0x22;

// ---------------------------------------------------------------------------
// UBLK IO operation types
// ---------------------------------------------------------------------------

/// Read operation.
pub const UBLK_IO_OP_READ: u32 = 0;
/// Write operation.
pub const UBLK_IO_OP_WRITE: u32 = 1;
/// Flush operation.
pub const UBLK_IO_OP_FLUSH: u32 = 2;
/// Discard operation.
pub const UBLK_IO_OP_DISCARD: u32 = 3;
/// Write same operation.
pub const UBLK_IO_OP_WRITE_SAME: u32 = 4;
/// Write zeroes operation.
pub const UBLK_IO_OP_WRITE_ZEROES: u32 = 5;
/// Zone append operation.
pub const UBLK_IO_OP_ZONE_APPEND: u32 = 6;

// ---------------------------------------------------------------------------
// UBLK feature flags
// ---------------------------------------------------------------------------

/// Support zero-copy.
pub const UBLK_F_SUPPORT_ZERO_COPY: u64 = 1 << 0;
/// Unprivileged device.
pub const UBLK_F_UNPRIVILEGED_DEV: u64 = 1 << 1;
/// User recovery.
pub const UBLK_F_USER_RECOVERY: u64 = 1 << 2;
/// User recovery reissue.
pub const UBLK_F_USER_RECOVERY_REISSUE: u64 = 1 << 3;
/// User copy.
pub const UBLK_F_USER_COPY: u64 = 1 << 4;
/// Zoned device.
pub const UBLK_F_ZONED: u64 = 1 << 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_opcodes_distinct() {
        let cmds = [
            UBLK_CMD_GET_QUEUE_AFFINITY, UBLK_CMD_GET_DEV_INFO,
            UBLK_CMD_ADD_DEV, UBLK_CMD_DEL_DEV,
            UBLK_CMD_START_DEV, UBLK_CMD_STOP_DEV,
            UBLK_CMD_SET_PARAMS, UBLK_CMD_GET_PARAMS,
            UBLK_CMD_START_USER_RECOVERY, UBLK_CMD_END_USER_RECOVERY,
            UBLK_CMD_GET_DEV_INFO2,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_io_opcodes_distinct() {
        let ios = [
            UBLK_IO_FETCH_REQ, UBLK_IO_COMMIT_AND_FETCH_REQ,
            UBLK_IO_NEED_GET_DATA,
        ];
        for i in 0..ios.len() {
            for j in (i + 1)..ios.len() {
                assert_ne!(ios[i], ios[j]);
            }
        }
    }

    #[test]
    fn test_op_types_distinct() {
        let ops = [
            UBLK_IO_OP_READ, UBLK_IO_OP_WRITE, UBLK_IO_OP_FLUSH,
            UBLK_IO_OP_DISCARD, UBLK_IO_OP_WRITE_SAME,
            UBLK_IO_OP_WRITE_ZEROES, UBLK_IO_OP_ZONE_APPEND,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_feature_flags_power_of_two() {
        let flags = [
            UBLK_F_SUPPORT_ZERO_COPY, UBLK_F_UNPRIVILEGED_DEV,
            UBLK_F_USER_RECOVERY, UBLK_F_USER_RECOVERY_REISSUE,
            UBLK_F_USER_COPY, UBLK_F_ZONED,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_feature_flags_no_overlap() {
        let flags = [
            UBLK_F_SUPPORT_ZERO_COPY, UBLK_F_UNPRIVILEGED_DEV,
            UBLK_F_USER_RECOVERY, UBLK_F_USER_RECOVERY_REISSUE,
            UBLK_F_USER_COPY, UBLK_F_ZONED,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
