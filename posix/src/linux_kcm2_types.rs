//! `<linux/kcm.h>` — Additional KCM (Kernel Connection Multiplexor) constants.
//!
//! Supplementary KCM constants covering netlink commands,
//! attribute types, and socket options.

// ---------------------------------------------------------------------------
// KCM netlink commands
// ---------------------------------------------------------------------------

/// Unspec.
pub const KCM_CMD_UNSPEC: u32 = 0;
/// Attach socket.
pub const KCM_CMD_ATTACH: u32 = 1;
/// Detach socket.
pub const KCM_CMD_DETACH: u32 = 2;
/// Clone KCM socket.
pub const KCM_CMD_CLONE: u32 = 3;

// ---------------------------------------------------------------------------
// KCM netlink attributes
// ---------------------------------------------------------------------------

/// Unspec attribute.
pub const KCM_ATTR_UNSPEC: u32 = 0;
/// KCM file descriptor.
pub const KCM_ATTR_KCMFD: u32 = 1;
/// Connected socket file descriptor.
pub const KCM_ATTR_CSOCKFD: u32 = 2;
/// Socket index.
pub const KCM_ATTR_SOCK_INDEX: u32 = 3;
/// Socket BPF program.
pub const KCM_ATTR_SOCK_BPF_PROG: u32 = 4;

// ---------------------------------------------------------------------------
// KCM socket options (SOL_KCM level)
// ---------------------------------------------------------------------------

/// Receive disable.
pub const KCM_RECV_DISABLE: u32 = 1;

// ---------------------------------------------------------------------------
// KCM status flags
// ---------------------------------------------------------------------------

/// Socket is connected.
pub const KCM_STATUS_CONNECTED: u32 = 0;
/// Socket has TX in progress.
pub const KCM_STATUS_TX_DATA: u32 = 1;
/// Socket waiting for buffer.
pub const KCM_STATUS_WAITING: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            KCM_CMD_UNSPEC, KCM_CMD_ATTACH,
            KCM_CMD_DETACH, KCM_CMD_CLONE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            KCM_ATTR_UNSPEC, KCM_ATTR_KCMFD,
            KCM_ATTR_CSOCKFD, KCM_ATTR_SOCK_INDEX,
            KCM_ATTR_SOCK_BPF_PROG,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_status_flags_distinct() {
        let flags = [
            KCM_STATUS_CONNECTED, KCM_STATUS_TX_DATA,
            KCM_STATUS_WAITING,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
