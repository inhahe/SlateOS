//! `<linux/nbd.h>` — Network Block Device ioctls.
//!
//! NBD allows a block device to be backed by a remote server
//! connected over a network socket. The kernel NBD driver exposes
//! `/dev/nbdN` devices controlled via these ioctls.

// ---------------------------------------------------------------------------
// NBD ioctl commands
// ---------------------------------------------------------------------------

/// Set socket for NBD device.
pub const NBD_SET_SOCK: u64 = 0xAB00;
/// Set block size.
pub const NBD_SET_BLKSIZE: u64 = 0xAB01;
/// Set device size.
pub const NBD_SET_SIZE: u64 = 0xAB02;
/// Start NBD device.
pub const NBD_DO_IT: u64 = 0xAB03;
/// Clear socket.
pub const NBD_CLEAR_SOCK: u64 = 0xAB04;
/// Clear queue.
pub const NBD_CLEAR_QUE: u64 = 0xAB05;
/// Print debug info.
pub const NBD_PRINT_DEBUG: u64 = 0xAB06;
/// Set device size (in blocks).
pub const NBD_SET_SIZE_BLOCKS: u64 = 0xAB07;
/// Disconnect.
pub const NBD_DISCONNECT: u64 = 0xAB08;
/// Set device timeout.
pub const NBD_SET_TIMEOUT: u64 = 0xAB09;
/// Set flags.
pub const NBD_SET_FLAGS: u64 = 0xAB0A;

// ---------------------------------------------------------------------------
// NBD flags (server capabilities)
// ---------------------------------------------------------------------------

/// Server has read-only flag support.
pub const NBD_FLAG_HAS_FLAGS: u32 = 1 << 0;
/// Export is read-only.
pub const NBD_FLAG_READ_ONLY: u32 = 1 << 1;
/// Server supports FLUSH command.
pub const NBD_FLAG_SEND_FLUSH: u32 = 1 << 2;
/// Server supports FUA (Force Unit Access).
pub const NBD_FLAG_SEND_FUA: u32 = 1 << 3;
/// Use rotational media hints.
pub const NBD_FLAG_ROTATIONAL: u32 = 1 << 4;
/// Server supports TRIM/DISCARD.
pub const NBD_FLAG_SEND_TRIM: u32 = 1 << 5;
/// Server supports WRITE_ZEROES.
pub const NBD_FLAG_SEND_WRITE_ZEROES: u32 = 1 << 6;
/// Multiple connections allowed.
pub const NBD_FLAG_CAN_MULTI_CONN: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// NBD command types
// ---------------------------------------------------------------------------

/// Read request.
pub const NBD_CMD_READ: u32 = 0;
/// Write request.
pub const NBD_CMD_WRITE: u32 = 1;
/// Disconnect.
pub const NBD_CMD_DISC: u32 = 2;
/// Flush.
pub const NBD_CMD_FLUSH: u32 = 3;
/// Trim/discard.
pub const NBD_CMD_TRIM: u32 = 4;
/// Write zeroes.
pub const NBD_CMD_WRITE_ZEROES: u32 = 6;

// ---------------------------------------------------------------------------
// NBD magic
// ---------------------------------------------------------------------------

/// NBD request magic.
pub const NBD_REQUEST_MAGIC: u32 = 0x25609513;
/// NBD reply magic.
pub const NBD_REPLY_MAGIC: u32 = 0x67446698;

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
    fn test_flags_are_powers_of_two() {
        let flags = [
            NBD_FLAG_HAS_FLAGS, NBD_FLAG_READ_ONLY,
            NBD_FLAG_SEND_FLUSH, NBD_FLAG_SEND_FUA,
            NBD_FLAG_ROTATIONAL, NBD_FLAG_SEND_TRIM,
            NBD_FLAG_SEND_WRITE_ZEROES, NBD_FLAG_CAN_MULTI_CONN,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f:#x} not a power of 2");
        }
    }

    #[test]
    fn test_commands_distinct() {
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
    fn test_magic_values() {
        assert_ne!(NBD_REQUEST_MAGIC, NBD_REPLY_MAGIC);
        assert_eq!(NBD_REQUEST_MAGIC, 0x25609513);
    }
}
