//! `<linux/nbd.h>` — Additional NBD (Network Block Device) constants.
//!
//! Supplementary NBD constants covering command types,
//! flags, reply types, and connection options.

// ---------------------------------------------------------------------------
// NBD command types (NBD_CMD_*)
// ---------------------------------------------------------------------------

/// Read.
pub const NBD_CMD_READ: u32 = 0;
/// Write.
pub const NBD_CMD_WRITE: u32 = 1;
/// Disconnect.
pub const NBD_CMD_DISC: u32 = 2;
/// Flush.
pub const NBD_CMD_FLUSH: u32 = 3;
/// Trim/discard.
pub const NBD_CMD_TRIM: u32 = 4;
/// Cache hint.
pub const NBD_CMD_CACHE: u32 = 5;
/// Write zeroes.
pub const NBD_CMD_WRITE_ZEROES: u32 = 6;
/// Block status.
pub const NBD_CMD_BLOCK_STATUS: u32 = 7;
/// Resize.
pub const NBD_CMD_RESIZE: u32 = 8;

// ---------------------------------------------------------------------------
// NBD command flags
// ---------------------------------------------------------------------------

/// Force Unit Access.
pub const NBD_CMD_FLAG_FUA: u16 = 1 << 0;
/// Don't create hole (write zeroes).
pub const NBD_CMD_FLAG_NO_HOLE: u16 = 1 << 1;
/// Don't fragment.
pub const NBD_CMD_FLAG_DF: u16 = 1 << 2;
/// Request one (block status).
pub const NBD_CMD_FLAG_REQ_ONE: u16 = 1 << 3;
/// Fast zero.
pub const NBD_CMD_FLAG_FAST_ZERO: u16 = 1 << 4;
/// Payload length.
pub const NBD_CMD_FLAG_PAYLOAD_LEN: u16 = 1 << 5;

// ---------------------------------------------------------------------------
// NBD transmission flags (handshake)
// ---------------------------------------------------------------------------

/// Has flags.
pub const NBD_FLAG_HAS_FLAGS: u16 = 1 << 0;
/// Read-only export.
pub const NBD_FLAG_READ_ONLY: u16 = 1 << 1;
/// Send flush.
pub const NBD_FLAG_SEND_FLUSH: u16 = 1 << 2;
/// Send FUA.
pub const NBD_FLAG_SEND_FUA: u16 = 1 << 3;
/// Rotational media.
pub const NBD_FLAG_ROTATIONAL: u16 = 1 << 4;
/// Send trim.
pub const NBD_FLAG_SEND_TRIM: u16 = 1 << 5;
/// Send write zeroes.
pub const NBD_FLAG_SEND_WRITE_ZEROES: u16 = 1 << 6;
/// Send DF.
pub const NBD_FLAG_SEND_DF: u16 = 1 << 7;
/// Can multi-conn.
pub const NBD_FLAG_CAN_MULTI_CONN: u16 = 1 << 8;
/// Send resize.
pub const NBD_FLAG_SEND_RESIZE: u16 = 1 << 9;
/// Send cache.
pub const NBD_FLAG_SEND_CACHE: u16 = 1 << 10;
/// Send fast zero.
pub const NBD_FLAG_SEND_FAST_ZERO: u16 = 1 << 11;
/// Block status payload.
pub const NBD_FLAG_BLOCK_STATUS_PAYLOAD: u16 = 1 << 12;

// ---------------------------------------------------------------------------
// NBD reply types
// ---------------------------------------------------------------------------

/// Simple reply magic.
pub const NBD_SIMPLE_REPLY_MAGIC: u32 = 0x67446698;
/// Structured reply magic.
pub const NBD_STRUCTURED_REPLY_MAGIC: u32 = 0x668e33ef;
/// Reply type: none.
pub const NBD_REPLY_TYPE_NONE: u16 = 0;
/// Reply type: offset data.
pub const NBD_REPLY_TYPE_OFFSET_DATA: u16 = 1;
/// Reply type: offset hole.
pub const NBD_REPLY_TYPE_OFFSET_HOLE: u16 = 2;
/// Reply type: block status.
pub const NBD_REPLY_TYPE_BLOCK_STATUS: u16 = 5;

// ---------------------------------------------------------------------------
// NBD errors (over the wire)
// ---------------------------------------------------------------------------

/// Success.
pub const NBD_OK: u32 = 0;
/// Permission denied.
pub const NBD_EPERM: u32 = 1;
/// I/O error.
pub const NBD_EIO: u32 = 5;
/// Out of memory.
pub const NBD_ENOMEM: u32 = 12;
/// Invalid argument.
pub const NBD_EINVAL: u32 = 22;
/// No space.
pub const NBD_ENOSPC: u32 = 28;
/// Overflow.
pub const NBD_EOVERFLOW: u32 = 75;
/// Not supported.
pub const NBD_ENOTSUP: u32 = 95;
/// Shutdown.
pub const NBD_ESHUTDOWN: u32 = 108;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_types_distinct() {
        let cmds = [
            NBD_CMD_READ, NBD_CMD_WRITE, NBD_CMD_DISC,
            NBD_CMD_FLUSH, NBD_CMD_TRIM, NBD_CMD_CACHE,
            NBD_CMD_WRITE_ZEROES, NBD_CMD_BLOCK_STATUS,
            NBD_CMD_RESIZE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_cmd_flags_power_of_two() {
        let flags: [u16; 6] = [
            NBD_CMD_FLAG_FUA, NBD_CMD_FLAG_NO_HOLE,
            NBD_CMD_FLAG_DF, NBD_CMD_FLAG_REQ_ONE,
            NBD_CMD_FLAG_FAST_ZERO, NBD_CMD_FLAG_PAYLOAD_LEN,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:04x} not power of two", f);
        }
    }

    #[test]
    fn test_trans_flags_no_overlap() {
        let flags: [u16; 13] = [
            NBD_FLAG_HAS_FLAGS, NBD_FLAG_READ_ONLY,
            NBD_FLAG_SEND_FLUSH, NBD_FLAG_SEND_FUA,
            NBD_FLAG_ROTATIONAL, NBD_FLAG_SEND_TRIM,
            NBD_FLAG_SEND_WRITE_ZEROES, NBD_FLAG_SEND_DF,
            NBD_FLAG_CAN_MULTI_CONN, NBD_FLAG_SEND_RESIZE,
            NBD_FLAG_SEND_CACHE, NBD_FLAG_SEND_FAST_ZERO,
            NBD_FLAG_BLOCK_STATUS_PAYLOAD,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_reply_magics() {
        assert_ne!(NBD_SIMPLE_REPLY_MAGIC, NBD_STRUCTURED_REPLY_MAGIC);
    }

    #[test]
    fn test_reply_types_distinct() {
        let types: [u16; 4] = [
            NBD_REPLY_TYPE_NONE, NBD_REPLY_TYPE_OFFSET_DATA,
            NBD_REPLY_TYPE_OFFSET_HOLE, NBD_REPLY_TYPE_BLOCK_STATUS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_errors_distinct() {
        let errs = [
            NBD_OK, NBD_EPERM, NBD_EIO, NBD_ENOMEM,
            NBD_EINVAL, NBD_ENOSPC, NBD_EOVERFLOW,
            NBD_ENOTSUP, NBD_ESHUTDOWN,
        ];
        for i in 0..errs.len() {
            for j in (i + 1)..errs.len() {
                assert_ne!(errs[i], errs[j]);
            }
        }
    }
}
