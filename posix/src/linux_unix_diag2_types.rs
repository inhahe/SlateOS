//! `<linux/unix_diag.h>` — Additional Unix socket diagnostics constants.
//!
//! Supplementary Unix socket diagnostics constants covering
//! attribute types, show flags, and state values.

// ---------------------------------------------------------------------------
// Unix diagnostics attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const UNIX_DIAG_NONE: u32 = 0;
/// Name.
pub const UNIX_DIAG_NAME: u32 = 1;
/// VFS info.
pub const UNIX_DIAG_VFS: u32 = 2;
/// Peer.
pub const UNIX_DIAG_PEER: u32 = 3;
/// Icons.
pub const UNIX_DIAG_ICONS: u32 = 4;
/// Receive queue length.
pub const UNIX_DIAG_RQLEN: u32 = 5;
/// Memory info.
pub const UNIX_DIAG_MEMINFO: u32 = 6;
/// Shutdown state.
pub const UNIX_DIAG_SHUTDOWN: u32 = 7;
/// UID.
pub const UNIX_DIAG_UID: u32 = 8;

// ---------------------------------------------------------------------------
// Unix diagnostics show flags
// ---------------------------------------------------------------------------

/// Show name.
pub const UDIAG_SHOW_NAME: u32 = 1 << 0;
/// Show VFS.
pub const UDIAG_SHOW_VFS: u32 = 1 << 1;
/// Show peer.
pub const UDIAG_SHOW_PEER: u32 = 1 << 2;
/// Show icons.
pub const UDIAG_SHOW_ICONS: u32 = 1 << 3;
/// Show receive queue.
pub const UDIAG_SHOW_RQLEN: u32 = 1 << 4;
/// Show memory info.
pub const UDIAG_SHOW_MEMINFO: u32 = 1 << 5;
/// Show UID.
pub const UDIAG_SHOW_UID: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            UNIX_DIAG_NONE,
            UNIX_DIAG_NAME,
            UNIX_DIAG_VFS,
            UNIX_DIAG_PEER,
            UNIX_DIAG_ICONS,
            UNIX_DIAG_RQLEN,
            UNIX_DIAG_MEMINFO,
            UNIX_DIAG_SHUTDOWN,
            UNIX_DIAG_UID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_show_flags_no_overlap() {
        let flags = [
            UDIAG_SHOW_NAME,
            UDIAG_SHOW_VFS,
            UDIAG_SHOW_PEER,
            UDIAG_SHOW_ICONS,
            UDIAG_SHOW_RQLEN,
            UDIAG_SHOW_MEMINFO,
            UDIAG_SHOW_UID,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
