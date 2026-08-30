//! `<linux/io_uring/cmd.h>` — io_uring command passthrough constants.
//!
//! io_uring command passthrough allows issuing device-specific
//! commands (e.g., NVMe passthrough) directly through the
//! io_uring submission queue, bypassing traditional ioctl paths
//! for improved performance.

// ---------------------------------------------------------------------------
// io_uring opcodes related to commands
// ---------------------------------------------------------------------------

/// Passthrough command opcode.
pub const IORING_OP_URING_CMD: u8 = 46;

// ---------------------------------------------------------------------------
// Command flags (in sqe.uring_cmd_flags)
// ---------------------------------------------------------------------------

/// Fixed file (use registered fd).
pub const IORING_URING_CMD_FIXED: u32 = 1 << 0;
/// Polled completion (busy-wait).
pub const IORING_URING_CMD_POLLED: u32 = 1 << 31;

// ---------------------------------------------------------------------------
// Command sizes
// ---------------------------------------------------------------------------

/// Size of the SQE command data area (bytes).
pub const IORING_CMD_DATA_SIZE: usize = 80;

/// Size of the pdu (protocol data unit) inline area.
pub const IORING_CMD_PDU_SIZE: usize = 32;

// ---------------------------------------------------------------------------
// NVMe passthrough command types
// ---------------------------------------------------------------------------

/// NVMe admin command.
pub const NVME_URING_CMD_ADMIN: u32 = 0;
/// NVMe I/O command.
pub const NVME_URING_CMD_IO: u32 = 1;
/// NVMe admin command (vectored).
pub const NVME_URING_CMD_ADMIN_VEC: u32 = 2;
/// NVMe I/O command (vectored).
pub const NVME_URING_CMD_IO_VEC: u32 = 3;

// ---------------------------------------------------------------------------
// Socket uring cmd types
// ---------------------------------------------------------------------------

/// Socket send zero-copy.
pub const SOCKET_URING_OP_SIOCINQ: u32 = 0;
/// Socket receive.
pub const SOCKET_URING_OP_SIOCOUTQ: u32 = 1;
/// Socket getsockopt.
pub const SOCKET_URING_OP_GETSOCKOPT: u32 = 2;
/// Socket setsockopt.
pub const SOCKET_URING_OP_SETSOCKOPT: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_value() {
        assert_eq!(IORING_OP_URING_CMD, 46);
    }

    #[test]
    fn test_cmd_flags_no_overlap() {
        assert_eq!(IORING_URING_CMD_FIXED & IORING_URING_CMD_POLLED, 0);
    }

    #[test]
    fn test_cmd_flags_powers_of_two() {
        assert!(IORING_URING_CMD_FIXED.is_power_of_two());
        assert!(IORING_URING_CMD_POLLED.is_power_of_two());
    }

    #[test]
    fn test_data_sizes() {
        assert!(IORING_CMD_DATA_SIZE > 0);
        assert!(IORING_CMD_PDU_SIZE > 0);
        assert!(IORING_CMD_PDU_SIZE <= IORING_CMD_DATA_SIZE);
    }

    #[test]
    fn test_nvme_cmd_types_distinct() {
        let types = [
            NVME_URING_CMD_ADMIN,
            NVME_URING_CMD_IO,
            NVME_URING_CMD_ADMIN_VEC,
            NVME_URING_CMD_IO_VEC,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_socket_ops_distinct() {
        let ops = [
            SOCKET_URING_OP_SIOCINQ,
            SOCKET_URING_OP_SIOCOUTQ,
            SOCKET_URING_OP_GETSOCKOPT,
            SOCKET_URING_OP_SETSOCKOPT,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }
}
