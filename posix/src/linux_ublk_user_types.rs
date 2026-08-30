//! `<linux/ublk_cmd.h>` — ublk userspace block-device feature bits.
//!
//! ublk (5.16+) lets a userspace daemon implement a block device
//! via an io_uring command-queue control channel. ublksrv (the
//! reference userspace daemon) and qemu-storage-daemon publish the
//! feature flags / command opcodes below.

// ---------------------------------------------------------------------------
// Major number prefix and limits
// ---------------------------------------------------------------------------

/// Default I/O command-buffer size per queue (256 KiB).
pub const UBLK_IO_BUF_SIZE: u32 = 1 << 18;
/// Maximum number of queues per device.
pub const UBLK_MAX_NR_QUEUES: u32 = 32;
/// Maximum queue depth.
pub const UBLK_MAX_QUEUE_DEPTH: u32 = 4096;

// ---------------------------------------------------------------------------
// ublk_ctrl_cmd opcodes (group 'u', issued via io_uring_cmd)
// ---------------------------------------------------------------------------

/// Get device info.
pub const UBLK_CMD_GET_DEV_INFO: u32 = 0x02;
/// Add a new ublk device.
pub const UBLK_CMD_ADD_DEV: u32 = 0x04;
/// Delete a ublk device.
pub const UBLK_CMD_DEL_DEV: u32 = 0x05;
/// Start a ublk device (kick the kernel block-layer registration).
pub const UBLK_CMD_START_DEV: u32 = 0x06;
/// Stop a ublk device.
pub const UBLK_CMD_STOP_DEV: u32 = 0x07;
/// Set device parameters (block size, max-sectors, etc.).
pub const UBLK_CMD_SET_PARAMS: u32 = 0x08;
/// Get current device parameters.
pub const UBLK_CMD_GET_PARAMS: u32 = 0x09;
/// Start re-covering a crashed userspace daemon's device.
pub const UBLK_CMD_START_USER_RECOVERY: u32 = 0x10;
/// Finish recovery.
pub const UBLK_CMD_END_USER_RECOVERY: u32 = 0x11;

// ---------------------------------------------------------------------------
// ublk_io_cmd opcodes (per-queue)
// ---------------------------------------------------------------------------

/// Fetch the next pending I/O command.
pub const UBLK_IO_FETCH_REQ: u32 = 0x20;
/// Commit completed I/O and fetch the next one.
pub const UBLK_IO_COMMIT_AND_FETCH_REQ: u32 = 0x21;
/// Mark a command as needing re-fetch (kernel keeps slot reserved).
pub const UBLK_IO_NEED_GET_DATA: u32 = 0x22;

// ---------------------------------------------------------------------------
// Feature flags (struct ublk_params.features and ublksrv_ctrl_dev_info.flags)
// ---------------------------------------------------------------------------

/// Use zero-copy via io_uring fixed buffers.
pub const UBLK_F_SUPPORT_ZERO_COPY: u64 = 1 << 0;
/// Device may be unprivileged (no CAP_SYS_ADMIN).
pub const UBLK_F_URING_CMD_COMP_IN_TASK: u64 = 1 << 1;
/// Tell kernel the daemon can do write-zeros NEED_GET_DATA.
pub const UBLK_F_NEED_GET_DATA: u64 = 1 << 2;
/// Allow user recovery (re-attach a crashed daemon).
pub const UBLK_F_USER_RECOVERY: u64 = 1 << 3;
/// User recovery — re-issue in-flight requests.
pub const UBLK_F_USER_RECOVERY_REISSUE: u64 = 1 << 4;
/// Unprivileged-dev opt-in.
pub const UBLK_F_UNPRIVILEGED_DEV: u64 = 1 << 5;
/// Use the cmd-completion-in-task feature.
pub const UBLK_F_CMD_IOCTL_ENCODE: u64 = 1 << 6;
/// Allow the userspace daemon to map per-IO buffer.
pub const UBLK_F_USER_COPY: u64 = 1 << 7;
/// Zoned-block-device target support.
pub const UBLK_F_ZONED: u64 = 1 << 8;

// ---------------------------------------------------------------------------
// I/O operation types (struct ublksrv_io_desc.op_flags low 8 bits)
// ---------------------------------------------------------------------------

/// Read.
pub const UBLK_IO_OP_READ: u8 = 0;
/// Write.
pub const UBLK_IO_OP_WRITE: u8 = 1;
/// Flush.
pub const UBLK_IO_OP_FLUSH: u8 = 2;
/// Discard.
pub const UBLK_IO_OP_DISCARD: u8 = 3;
/// Write zeroes.
pub const UBLK_IO_OP_WRITE_ZEROES: u8 = 4;
/// Write-same.
pub const UBLK_IO_OP_WRITE_SAME: u8 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_buf_and_depth_pow2() {
        // I/O buffer size and queue depth must be powers of two so
        // the kernel can mask instead of mod for ring indices.
        assert!(UBLK_IO_BUF_SIZE.is_power_of_two());
        assert!(UBLK_MAX_NR_QUEUES.is_power_of_two());
        assert!(UBLK_MAX_QUEUE_DEPTH.is_power_of_two());
        assert_eq!(UBLK_IO_BUF_SIZE, 256 * 1024);
    }

    #[test]
    fn test_ctrl_cmds_distinct() {
        let c = [
            UBLK_CMD_GET_DEV_INFO,
            UBLK_CMD_ADD_DEV,
            UBLK_CMD_DEL_DEV,
            UBLK_CMD_START_DEV,
            UBLK_CMD_STOP_DEV,
            UBLK_CMD_SET_PARAMS,
            UBLK_CMD_GET_PARAMS,
            UBLK_CMD_START_USER_RECOVERY,
            UBLK_CMD_END_USER_RECOVERY,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
    }

    #[test]
    fn test_io_cmds_distinct_and_above_ctrl() {
        let io = [
            UBLK_IO_FETCH_REQ,
            UBLK_IO_COMMIT_AND_FETCH_REQ,
            UBLK_IO_NEED_GET_DATA,
        ];
        for i in 0..io.len() {
            for j in (i + 1)..io.len() {
                assert_ne!(io[i], io[j]);
            }
            // I/O cmds sit in the 0x20.. range to leave room for
            // future ctrl cmds in 0x00..0x1f.
            assert!(io[i] >= 0x20);
        }
    }

    #[test]
    fn test_feature_flags_distinct_pow2() {
        let f = [
            UBLK_F_SUPPORT_ZERO_COPY,
            UBLK_F_URING_CMD_COMP_IN_TASK,
            UBLK_F_NEED_GET_DATA,
            UBLK_F_USER_RECOVERY,
            UBLK_F_USER_RECOVERY_REISSUE,
            UBLK_F_UNPRIVILEGED_DEV,
            UBLK_F_CMD_IOCTL_ENCODE,
            UBLK_F_USER_COPY,
            UBLK_F_ZONED,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_io_ops_dense() {
        let ops = [
            UBLK_IO_OP_READ,
            UBLK_IO_OP_WRITE,
            UBLK_IO_OP_FLUSH,
            UBLK_IO_OP_DISCARD,
            UBLK_IO_OP_WRITE_ZEROES,
            UBLK_IO_OP_WRITE_SAME,
        ];
        for (i, &v) in ops.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }
}
