//! `<linux/nbd.h>` — Network Block Device (NBD) constants.
//!
//! NBD exports a block device backed by a remote server over TCP.
//! The client kernel module (/dev/nbdN) forwards block I/O requests
//! over the network to an nbd-server. Used for remote storage, live
//! migration of disk images, distributed storage systems, and backup
//! solutions.

// ---------------------------------------------------------------------------
// NBD ioctl commands
// ---------------------------------------------------------------------------

/// Set the socket for NBD communication.
pub const NBD_SET_SOCK: u32 = 0xAB00;
/// Set block device size (bytes).
pub const NBD_SET_BLKSIZE: u32 = 0xAB01;
/// Set total device size (blocks).
pub const NBD_SET_SIZE: u32 = 0xAB02;
/// Start the NBD device (begin serving requests).
pub const NBD_DO_IT: u32 = 0xAB03;
/// Clear the socket (disconnect).
pub const NBD_CLEAR_SOCK: u32 = 0xAB04;
/// Clear the request queue.
pub const NBD_CLEAR_QUE: u32 = 0xAB05;
/// Print debug info.
pub const NBD_PRINT_DEBUG: u32 = 0xAB06;
/// Set total size in bytes (64-bit).
pub const NBD_SET_SIZE_BLOCKS: u32 = 0xAB07;
/// Soft disconnect (flush then disconnect).
pub const NBD_DISCONNECT: u32 = 0xAB08;
/// Set request timeout (seconds).
pub const NBD_SET_TIMEOUT: u32 = 0xAB09;
/// Set flags (read-only, etc.).
pub const NBD_SET_FLAGS: u32 = 0xAB0A;

// ---------------------------------------------------------------------------
// NBD command types (in request header)
// ---------------------------------------------------------------------------

/// Read request.
pub const NBD_CMD_READ: u32 = 0;
/// Write request.
pub const NBD_CMD_WRITE: u32 = 1;
/// Disconnect (graceful shutdown).
pub const NBD_CMD_DISC: u32 = 2;
/// Flush volatile write cache.
pub const NBD_CMD_FLUSH: u32 = 3;
/// Trim (discard / UNMAP).
pub const NBD_CMD_TRIM: u32 = 4;
/// Write zeroes.
pub const NBD_CMD_WRITE_ZEROES: u32 = 6;

// ---------------------------------------------------------------------------
// NBD flags (server capabilities / device flags)
// ---------------------------------------------------------------------------

/// Server has flush support.
pub const NBD_FLAG_HAS_FLAGS: u32 = 1 << 0;
/// Export is read-only.
pub const NBD_FLAG_READ_ONLY: u32 = 1 << 1;
/// Server supports flush.
pub const NBD_FLAG_SEND_FLUSH: u32 = 1 << 2;
/// Server supports FUA (Force Unit Access).
pub const NBD_FLAG_SEND_FUA: u32 = 1 << 3;
/// Rotational storage (not SSD).
pub const NBD_FLAG_ROTATIONAL: u32 = 1 << 4;
/// Server supports trim.
pub const NBD_FLAG_SEND_TRIM: u32 = 1 << 5;
/// Server supports write zeroes.
pub const NBD_FLAG_SEND_WRITE_ZEROES: u32 = 1 << 6;
/// Multiple connections allowed.
pub const NBD_FLAG_CAN_MULTI_CONN: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// NBD magic numbers (protocol)
// ---------------------------------------------------------------------------

/// NBD request magic.
pub const NBD_REQUEST_MAGIC: u32 = 0x2560_1953;
/// NBD reply magic.
pub const NBD_REPLY_MAGIC: u32 = 0x6744_6698;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            NBD_SET_SOCK, NBD_SET_BLKSIZE, NBD_SET_SIZE,
            NBD_DO_IT, NBD_CLEAR_SOCK, NBD_CLEAR_QUE,
            NBD_PRINT_DEBUG, NBD_SET_SIZE_BLOCKS,
            NBD_DISCONNECT, NBD_SET_TIMEOUT, NBD_SET_FLAGS,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_cmd_types_distinct() {
        let cmds = [
            NBD_CMD_READ, NBD_CMD_WRITE, NBD_CMD_DISC,
            NBD_CMD_FLUSH, NBD_CMD_TRIM, NBD_CMD_WRITE_ZEROES,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            NBD_FLAG_HAS_FLAGS, NBD_FLAG_READ_ONLY,
            NBD_FLAG_SEND_FLUSH, NBD_FLAG_SEND_FUA,
            NBD_FLAG_ROTATIONAL, NBD_FLAG_SEND_TRIM,
            NBD_FLAG_SEND_WRITE_ZEROES, NBD_FLAG_CAN_MULTI_CONN,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_magic_numbers() {
        assert_ne!(NBD_REQUEST_MAGIC, NBD_REPLY_MAGIC);
        assert_ne!(NBD_REQUEST_MAGIC, 0);
        assert_ne!(NBD_REPLY_MAGIC, 0);
    }
}
