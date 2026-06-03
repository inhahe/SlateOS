//! `<linux/nbd.h>` — Additional NBD (Network Block Device) constants.
//!
//! Supplementary NBD constants covering command flags,
//! transmission flags, and info types.

// ---------------------------------------------------------------------------
// NBD command flags (NBD_CMD_FLAG_*)
// ---------------------------------------------------------------------------

/// Force Unit Access — bypass volatile write cache.
pub const NBD_CMD_FLAG_FUA: u16 = 1 << 0;
/// No hole — don't punch holes for TRIM.
pub const NBD_CMD_FLAG_NO_HOLE: u16 = 1 << 1;
/// Don't fragment — single extent reply.
pub const NBD_CMD_FLAG_DF: u16 = 1 << 2;
/// Request one — limit structured replies to one.
pub const NBD_CMD_FLAG_REQ_ONE: u16 = 1 << 3;
/// Fast zero — fail fast if WRITE_ZEROES would be slow.
pub const NBD_CMD_FLAG_FAST_ZERO: u16 = 1 << 4;
/// Payload length — payload follows header.
pub const NBD_CMD_FLAG_PAYLOAD_LEN: u16 = 1 << 5;

// ---------------------------------------------------------------------------
// NBD command types (NBD_CMD_*)
// ---------------------------------------------------------------------------

/// Read request.
pub const NBD_CMD_READ: u16 = 0;
/// Write request.
pub const NBD_CMD_WRITE: u16 = 1;
/// Disconnect.
pub const NBD_CMD_DISC: u16 = 2;
/// Flush.
pub const NBD_CMD_FLUSH: u16 = 3;
/// Trim (discard).
pub const NBD_CMD_TRIM: u16 = 4;
/// Cache hint.
pub const NBD_CMD_CACHE: u16 = 5;
/// Write zeroes.
pub const NBD_CMD_WRITE_ZEROES: u16 = 6;
/// Block status query.
pub const NBD_CMD_BLOCK_STATUS: u16 = 7;
/// Resize notification.
pub const NBD_CMD_RESIZE: u16 = 8;

// ---------------------------------------------------------------------------
// NBD handshake flags (NBD_FLAG_*)
// ---------------------------------------------------------------------------

/// Server supports flags.
pub const NBD_FLAG_HAS_FLAGS: u16 = 1 << 0;
/// Export is read-only.
pub const NBD_FLAG_READ_ONLY: u16 = 1 << 1;
/// Server supports flush.
pub const NBD_FLAG_SEND_FLUSH: u16 = 1 << 2;
/// Server supports FUA.
pub const NBD_FLAG_SEND_FUA: u16 = 1 << 3;
/// Export is rotational medium.
pub const NBD_FLAG_ROTATIONAL: u16 = 1 << 4;
/// Server supports trim.
pub const NBD_FLAG_SEND_TRIM: u16 = 1 << 5;
/// Server supports write zeroes.
pub const NBD_FLAG_SEND_WRITE_ZEROES: u16 = 1 << 6;
/// Server supports DF flag.
pub const NBD_FLAG_SEND_DF: u16 = 1 << 7;
/// Server supports multiple connections.
pub const NBD_FLAG_CAN_MULTI_CONN: u16 = 1 << 8;
/// Server supports resize.
pub const NBD_FLAG_SEND_RESIZE: u16 = 1 << 9;
/// Server supports cache command.
pub const NBD_FLAG_SEND_CACHE: u16 = 1 << 10;
/// Server supports fast zero.
pub const NBD_FLAG_SEND_FAST_ZERO: u16 = 1 << 11;
/// Server supports block status.
pub const NBD_FLAG_SEND_BLOCK_STATUS: u16 = 1 << 12;

// ---------------------------------------------------------------------------
// NBD info types
// ---------------------------------------------------------------------------

/// Export info.
pub const NBD_INFO_EXPORT: u16 = 0;
/// Name info.
pub const NBD_INFO_NAME: u16 = 1;
/// Description info.
pub const NBD_INFO_DESCRIPTION: u16 = 2;
/// Block size info.
pub const NBD_INFO_BLOCK_SIZE: u16 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_flags_power_of_two() {
        assert!(NBD_CMD_FLAG_FUA.is_power_of_two());
        assert!(NBD_CMD_FLAG_NO_HOLE.is_power_of_two());
        assert!(NBD_CMD_FLAG_DF.is_power_of_two());
        assert!(NBD_CMD_FLAG_REQ_ONE.is_power_of_two());
        assert!(NBD_CMD_FLAG_FAST_ZERO.is_power_of_two());
        assert!(NBD_CMD_FLAG_PAYLOAD_LEN.is_power_of_two());
    }

    #[test]
    fn test_cmd_flags_no_overlap() {
        let flags = [
            NBD_CMD_FLAG_FUA,
            NBD_CMD_FLAG_NO_HOLE,
            NBD_CMD_FLAG_DF,
            NBD_CMD_FLAG_REQ_ONE,
            NBD_CMD_FLAG_FAST_ZERO,
            NBD_CMD_FLAG_PAYLOAD_LEN,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            NBD_CMD_READ,
            NBD_CMD_WRITE,
            NBD_CMD_DISC,
            NBD_CMD_FLUSH,
            NBD_CMD_TRIM,
            NBD_CMD_CACHE,
            NBD_CMD_WRITE_ZEROES,
            NBD_CMD_BLOCK_STATUS,
            NBD_CMD_RESIZE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_handshake_flags_power_of_two() {
        let flags = [
            NBD_FLAG_HAS_FLAGS,
            NBD_FLAG_READ_ONLY,
            NBD_FLAG_SEND_FLUSH,
            NBD_FLAG_SEND_FUA,
            NBD_FLAG_ROTATIONAL,
            NBD_FLAG_SEND_TRIM,
            NBD_FLAG_SEND_WRITE_ZEROES,
            NBD_FLAG_SEND_DF,
            NBD_FLAG_CAN_MULTI_CONN,
            NBD_FLAG_SEND_RESIZE,
            NBD_FLAG_SEND_CACHE,
            NBD_FLAG_SEND_FAST_ZERO,
            NBD_FLAG_SEND_BLOCK_STATUS,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_handshake_flags_no_overlap() {
        let flags = [
            NBD_FLAG_HAS_FLAGS,
            NBD_FLAG_READ_ONLY,
            NBD_FLAG_SEND_FLUSH,
            NBD_FLAG_SEND_FUA,
            NBD_FLAG_ROTATIONAL,
            NBD_FLAG_SEND_TRIM,
            NBD_FLAG_SEND_WRITE_ZEROES,
            NBD_FLAG_SEND_DF,
            NBD_FLAG_CAN_MULTI_CONN,
            NBD_FLAG_SEND_RESIZE,
            NBD_FLAG_SEND_CACHE,
            NBD_FLAG_SEND_FAST_ZERO,
            NBD_FLAG_SEND_BLOCK_STATUS,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_info_types_distinct() {
        let infos = [
            NBD_INFO_EXPORT,
            NBD_INFO_NAME,
            NBD_INFO_DESCRIPTION,
            NBD_INFO_BLOCK_SIZE,
        ];
        for i in 0..infos.len() {
            for j in (i + 1)..infos.len() {
                assert_ne!(infos[i], infos[j]);
            }
        }
    }
}
