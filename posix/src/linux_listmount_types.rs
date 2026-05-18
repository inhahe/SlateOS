//! `<linux/mount.h>` — listmount() syscall constants.
//!
//! listmount() enumerates child mounts of a given mount point.
//! These constants define request flags, special mount IDs,
//! and iteration parameters.

// ---------------------------------------------------------------------------
// listmount() flags
// ---------------------------------------------------------------------------

/// List in reverse order.
pub const LISTMOUNT_REVERSE: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Special mount IDs for listmount()
// ---------------------------------------------------------------------------

/// Start from first child.
pub const LISTMOUNT_FROM_FIRST: u64 = 0;
/// Current namespace root.
pub const LISTMOUNT_ROOT: u64 = 0xFFFFFFFFFFFFFFFF;

// ---------------------------------------------------------------------------
// MNT_ID_REQ flags (for statmount/listmount mnt_id_req)
// ---------------------------------------------------------------------------

/// Request by unique mount ID.
pub const MNT_ID_REQ_SIZE_VER0: u32 = 24;

// ---------------------------------------------------------------------------
// Mount namespace request flags
// ---------------------------------------------------------------------------

/// Get mount ns id.
pub const NS_GET_MNTNS_ID: u32 = 0xB705;
/// Get parent namespace.
pub const NS_GET_PARENT: u32 = 0xB702;
/// Get namespace type.
pub const NS_GET_NSTYPE: u32 = 0xB703;
/// Get owner UID.
pub const NS_GET_OWNER_UID: u32 = 0xB704;

// ---------------------------------------------------------------------------
// Maximum results
// ---------------------------------------------------------------------------

/// Default buffer size for results.
pub const LISTMOUNT_BUF_DEFAULT: u32 = 256;
/// Maximum mounts per call.
pub const LISTMOUNT_MAX_MOUNTS: u32 = 65536;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_first_is_zero() {
        assert_eq!(LISTMOUNT_FROM_FIRST, 0);
    }

    #[test]
    fn test_root_is_max() {
        assert_eq!(LISTMOUNT_ROOT, u64::MAX);
    }

    #[test]
    fn test_ns_cmds_distinct() {
        let cmds = [NS_GET_MNTNS_ID, NS_GET_PARENT, NS_GET_NSTYPE, NS_GET_OWNER_UID];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_reverse_flag() {
        assert_eq!(LISTMOUNT_REVERSE, 1);
    }

    #[test]
    fn test_max_mounts() {
        assert_eq!(LISTMOUNT_MAX_MOUNTS, 65536);
    }

    #[test]
    fn test_buf_default() {
        assert_eq!(LISTMOUNT_BUF_DEFAULT, 256);
    }
}
