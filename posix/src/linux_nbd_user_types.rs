//! `<linux/nbd.h>` — Network Block Device userspace API.
//!
//! nbd-client, nbd-server, qemu-nbd, and the libnbd library use the
//! ioctls and on-wire opcodes below to attach a remote block device
//! at `/dev/nbdN`. The constants split between sysfs/ioctl control
//! (set socket, set size, do-it loop) and the NBD wire protocol
//! (request/reply magics, command opcodes, flags).

// ---------------------------------------------------------------------------
// /dev/nbdN ioctls (group 0xab)
// ---------------------------------------------------------------------------

/// Set the socket fd backing this device.
pub const NBD_SET_SOCK: u32 = 0x0000_ab00;
/// Set logical block size (deprecated; superseded by SET_BLKSIZE).
pub const NBD_SET_BLKSIZE: u32 = 0x0000_ab01;
/// Set total device size in bytes.
pub const NBD_SET_SIZE: u32 = 0x0000_ab02;
/// Enter the I/O loop (blocks until disconnect).
pub const NBD_DO_IT: u32 = 0x0000_ab03;
/// Clear the socket binding.
pub const NBD_CLEAR_SOCK: u32 = 0x0000_ab04;
/// Clear the in-flight request queue.
pub const NBD_CLEAR_QUE: u32 = 0x0000_ab05;
/// Print the current state to dmesg (debug).
pub const NBD_PRINT_DEBUG: u32 = 0x0000_ab06;
/// Set total size in 512-byte sectors.
pub const NBD_SET_SIZE_BLOCKS: u32 = 0x0000_ab07;
/// Disconnect — gracefully tear down the device.
pub const NBD_DISCONNECT: u32 = 0x0000_ab08;
/// Set NBD device flags (read-only, send-flush, etc.).
pub const NBD_SET_FLAGS: u32 = 0x0000_ab0a;

// ---------------------------------------------------------------------------
// NBD device flags (NBD_SET_FLAGS arg)
// ---------------------------------------------------------------------------

/// Device has FUA support.
pub const NBD_FLAG_HAS_FLAGS: u32 = 1 << 0;
/// Device is read-only.
pub const NBD_FLAG_READ_ONLY: u32 = 1 << 1;
/// Server supports `NBD_CMD_FLUSH`.
pub const NBD_FLAG_SEND_FLUSH: u32 = 1 << 2;
/// Server supports FUA on writes.
pub const NBD_FLAG_SEND_FUA: u32 = 1 << 3;
/// Server is allowed to rotate; client may reconnect.
pub const NBD_FLAG_ROTATIONAL: u32 = 1 << 4;
/// Server supports `NBD_CMD_TRIM`.
pub const NBD_FLAG_SEND_TRIM: u32 = 1 << 5;
/// Server supports `NBD_CMD_WRITE_ZEROES`.
pub const NBD_FLAG_SEND_WRITE_ZEROES: u32 = 1 << 6;
/// Server can identify holes.
pub const NBD_FLAG_CAN_MULTI_CONN: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// Request / reply magic numbers (32-bit, big-endian on wire)
// ---------------------------------------------------------------------------

/// Magic at the head of a request.
pub const NBD_REQUEST_MAGIC: u32 = 0x2560_9513;
/// Magic at the head of a reply.
pub const NBD_REPLY_MAGIC: u32 = 0x6744_6698;
/// Initial server greeting magic ("NBDMAGIC").
pub const NBD_INIT_MAGIC: u64 = 0x4e42_444d_4147_4943;
/// "OldStyle" client magic, sent right after NBD_INIT_MAGIC.
pub const NBD_OLD_MAGIC: u64 = 0x0000_4203_6582_5523;
/// "NewStyle" client magic ("IHAVEOPT" in ASCII).
pub const NBD_NEW_MAGIC: u64 = 0x4948_4156_454f_5054;

// ---------------------------------------------------------------------------
// Command opcodes (struct nbd_request.type)
// ---------------------------------------------------------------------------

/// Read.
pub const NBD_CMD_READ: u16 = 0;
/// Write.
pub const NBD_CMD_WRITE: u16 = 1;
/// Disconnect.
pub const NBD_CMD_DISC: u16 = 2;
/// Flush.
pub const NBD_CMD_FLUSH: u16 = 3;
/// Trim (discard).
pub const NBD_CMD_TRIM: u16 = 4;
/// Write zeroes.
pub const NBD_CMD_WRITE_ZEROES: u16 = 6;
/// Block status query.
pub const NBD_CMD_BLOCK_STATUS: u16 = 7;
/// Resize device.
pub const NBD_CMD_RESIZE: u16 = 8;

/// Command flags (high byte of request.type): force-unit-access.
pub const NBD_CMD_FLAG_FUA: u16 = 1 << 0;
/// Block-status flag: don't fragment ranges.
pub const NBD_CMD_FLAG_NO_HOLE: u16 = 1 << 1;
/// Block-status flag: request more-info reply.
pub const NBD_CMD_FLAG_DF: u16 = 1 << 2;
/// Block-status flag: req-one (only first extent).
pub const NBD_CMD_FLAG_REQ_ONE: u16 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct_and_in_group() {
        let ops = [
            NBD_SET_SOCK,
            NBD_SET_BLKSIZE,
            NBD_SET_SIZE,
            NBD_DO_IT,
            NBD_CLEAR_SOCK,
            NBD_CLEAR_QUE,
            NBD_PRINT_DEBUG,
            NBD_SET_SIZE_BLOCKS,
            NBD_DISCONNECT,
            NBD_SET_FLAGS,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // High 24 bits identify the NBD ioctl group (0xab).
            assert_eq!(ops[i] >> 8, 0xab);
        }
    }

    #[test]
    fn test_device_flags_distinct_pow2() {
        let f = [
            NBD_FLAG_HAS_FLAGS,
            NBD_FLAG_READ_ONLY,
            NBD_FLAG_SEND_FLUSH,
            NBD_FLAG_SEND_FUA,
            NBD_FLAG_ROTATIONAL,
            NBD_FLAG_SEND_TRIM,
            NBD_FLAG_SEND_WRITE_ZEROES,
            NBD_FLAG_CAN_MULTI_CONN,
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
    fn test_wire_magics_known_values() {
        // These are baked into clients/servers worldwide — must not
        // change.
        assert_eq!(NBD_REQUEST_MAGIC, 0x2560_9513);
        assert_eq!(NBD_REPLY_MAGIC, 0x6744_6698);
        // "NBDMAGIC" and "IHAVEOPT" in ASCII.
        assert_eq!(NBD_INIT_MAGIC, u64::from_be_bytes(*b"NBDMAGIC"));
        assert_eq!(NBD_NEW_MAGIC, u64::from_be_bytes(*b"IHAVEOPT"));
    }

    #[test]
    fn test_cmds_distinct_and_known() {
        let c = [
            NBD_CMD_READ,
            NBD_CMD_WRITE,
            NBD_CMD_DISC,
            NBD_CMD_FLUSH,
            NBD_CMD_TRIM,
            NBD_CMD_WRITE_ZEROES,
            NBD_CMD_BLOCK_STATUS,
            NBD_CMD_RESIZE,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
        // READ=0 / WRITE=1 are baked into every client implementation.
        assert_eq!(NBD_CMD_READ, 0);
        assert_eq!(NBD_CMD_WRITE, 1);
    }

    #[test]
    fn test_cmd_flag_bits_distinct_pow2() {
        let f = [
            NBD_CMD_FLAG_FUA,
            NBD_CMD_FLAG_NO_HOLE,
            NBD_CMD_FLAG_DF,
            NBD_CMD_FLAG_REQ_ONE,
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
}
