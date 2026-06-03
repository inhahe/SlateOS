//! `<linux/dlm_netlink.h>` — DLM generic-netlink event constants.
//!
//! Generic-netlink family, command, and attribute IDs the kernel
//! Distributed Lock Manager uses to report lock-timeout warnings to
//! userspace (consumed by `dlm_controld` for deadlock recovery).

// ---------------------------------------------------------------------------
// Family / multicast group identifiers
// ---------------------------------------------------------------------------

/// Generic-netlink family name.
pub const DLM_GENL_NAME: &str = "DLM";
/// Family-version reported in the genl header.
pub const DLM_GENL_VERSION: u32 = 1;
/// Multicast-group name for timeout events.
pub const DLM_GENL_MCAST_NAME: &str = "dlm_mcgroup";

// ---------------------------------------------------------------------------
// Generic-netlink commands
// ---------------------------------------------------------------------------

/// Unspecified.
pub const DLM_CMD_UNSPEC: u32 = 0;
/// Lock-request timed out — report the contending lock.
pub const DLM_CMD_HELLO: u32 = 1;
/// Heartbeat — userspace tells the kernel the daemon is alive.
pub const DLM_CMD_TIMEOUT: u32 = 2;

// ---------------------------------------------------------------------------
// Generic-netlink attribute types
// ---------------------------------------------------------------------------

/// Unspecified.
pub const DLM_TYPE_UNSPEC: u32 = 0;
/// Lockspace name (string).
pub const DLM_TYPE_LOCK: u32 = 1;

// ---------------------------------------------------------------------------
// dlm_lock_data fields (struct dlm_lock_data nested in DLM_TYPE_LOCK)
// ---------------------------------------------------------------------------

/// Version.
pub const DLM_LOCK_DATA_VERSION: u32 = 1;
/// Lockspace ID.
pub const DLM_LOCK_DATA_LSID: u32 = 2;
/// Owning node ID.
pub const DLM_LOCK_DATA_NODEID: u32 = 3;
/// Lock ID.
pub const DLM_LOCK_DATA_LKID: u32 = 4;
/// Owner pid.
pub const DLM_LOCK_DATA_OWNERPID: u32 = 5;
/// Resource name (binary).
pub const DLM_LOCK_DATA_RESNAME: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_name() {
        assert_eq!(DLM_GENL_NAME, "DLM");
        assert_eq!(DLM_GENL_MCAST_NAME, "dlm_mcgroup");
        assert!(DLM_GENL_VERSION >= 1);
    }

    #[test]
    fn test_cmds_distinct() {
        let cmds = [DLM_CMD_UNSPEC, DLM_CMD_HELLO, DLM_CMD_TIMEOUT];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        assert_ne!(DLM_TYPE_UNSPEC, DLM_TYPE_LOCK);
    }

    #[test]
    fn test_lock_data_fields_distinct() {
        let fields = [
            DLM_LOCK_DATA_VERSION,
            DLM_LOCK_DATA_LSID,
            DLM_LOCK_DATA_NODEID,
            DLM_LOCK_DATA_LKID,
            DLM_LOCK_DATA_OWNERPID,
            DLM_LOCK_DATA_RESNAME,
        ];
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }
}
