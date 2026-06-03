//! `<linux/target_core_user.h>` — TCMU (target_core_user) constants.
//!
//! TCMU lets userspace daemons (tcmu-runner, libiscsi-tgt) implement
//! LIO SCSI targets by submitting commands to a kernel-shared ring.
//! The constants below cover the ring magic, operation codes, ring
//! version, and the well-known shared-memory area sizes.

// ---------------------------------------------------------------------------
// Mailbox magic / version (struct tcmu_mailbox)
// ---------------------------------------------------------------------------

/// Magic word at the start of the TCMU mailbox region.
pub const TCMU_MAILBOX_VERSION: u16 = 2;
/// `"TCMU"` little-endian (used by userspace to validate the mapped
/// region).
pub const TCMU_MAILBOX_FLAG_CAP_OOOC: u16 = 0x0001;
/// Driver supports the read-len feature for compare-and-write.
pub const TCMU_MAILBOX_FLAG_CAP_READ_LEN: u16 = 0x0002;
/// Driver supports tmr (task-management-request) entries.
pub const TCMU_MAILBOX_FLAG_CAP_TMR: u16 = 0x0004;

// ---------------------------------------------------------------------------
// Ring-entry opcodes
// ---------------------------------------------------------------------------

/// SCSI command entry (payload follows).
pub const TCMU_OP_CMD: u8 = 1;
/// Padding entry (used to align the ring head).
pub const TCMU_OP_PAD: u8 = 2;
/// Task-management request entry.
pub const TCMU_OP_TMR: u8 = 3;

// ---------------------------------------------------------------------------
// Ring sizes (typical TCMU default — runtime configurable)
// ---------------------------------------------------------------------------

/// Default command ring size (1 MiB).
pub const TCMU_DEFAULT_CMD_RING_SIZE: u32 = 1 << 20;
/// Default data area size (8 MiB).
pub const TCMU_DEFAULT_DATA_AREA_SIZE: u32 = 8 << 20;
/// Maximum allowable command ring size (16 MiB).
pub const TCMU_MAX_CMD_RING_SIZE: u32 = 16 << 20;

// ---------------------------------------------------------------------------
// netlink notification commands (tcmu_genl_cmd)
// ---------------------------------------------------------------------------

/// Add a TCMU device.
pub const TCMU_CMD_ADDED_DEVICE: u32 = 1;
/// Remove a TCMU device.
pub const TCMU_CMD_REMOVED_DEVICE: u32 = 2;
/// Reconfigure a TCMU device.
pub const TCMU_CMD_RECONFIG_DEVICE: u32 = 3;
/// Reply to one of the above (carries a status).
pub const TCMU_CMD_REPLY: u32 = 5;

// ---------------------------------------------------------------------------
// netlink attributes (tcmu_genl_attr)
// ---------------------------------------------------------------------------

/// Unspecified attribute.
pub const TCMU_ATTR_UNSPEC: u16 = 0;
/// Target device name.
pub const TCMU_ATTR_DEVICE: u16 = 1;
/// minor number.
pub const TCMU_ATTR_MINOR: u16 = 2;
/// Command status (echo of TCMU_CMD_*).
pub const TCMU_ATTR_CMD_STATUS: u16 = 3;
/// Device-id (numeric).
pub const TCMU_ATTR_DEVICE_ID: u16 = 4;
/// Supported features (bitmap).
pub const TCMU_ATTR_SUPP_KERN_CMD_REPLY: u16 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mailbox_version_and_flags_distinct() {
        // Mailbox version 2 has been stable since ~Linux 4.4.
        assert_eq!(TCMU_MAILBOX_VERSION, 2);
        let flags = [
            TCMU_MAILBOX_FLAG_CAP_OOOC,
            TCMU_MAILBOX_FLAG_CAP_READ_LEN,
            TCMU_MAILBOX_FLAG_CAP_TMR,
        ];
        for &b in &flags {
            assert!(b.is_power_of_two());
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_opcodes_distinct() {
        let o = [TCMU_OP_CMD, TCMU_OP_PAD, TCMU_OP_TMR];
        for i in 0..o.len() {
            for j in (i + 1)..o.len() {
                assert_ne!(o[i], o[j]);
            }
        }
    }

    #[test]
    fn test_ring_sizes_consistent() {
        assert!(TCMU_DEFAULT_CMD_RING_SIZE.is_power_of_two());
        assert!(TCMU_DEFAULT_DATA_AREA_SIZE.is_power_of_two());
        assert!(TCMU_MAX_CMD_RING_SIZE.is_power_of_two());
        assert!(TCMU_DEFAULT_CMD_RING_SIZE <= TCMU_MAX_CMD_RING_SIZE);
    }

    #[test]
    fn test_genl_cmds_distinct() {
        let c = [
            TCMU_CMD_ADDED_DEVICE,
            TCMU_CMD_REMOVED_DEVICE,
            TCMU_CMD_RECONFIG_DEVICE,
            TCMU_CMD_REPLY,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct_unspec_zero() {
        let a = [
            TCMU_ATTR_UNSPEC,
            TCMU_ATTR_DEVICE,
            TCMU_ATTR_MINOR,
            TCMU_ATTR_CMD_STATUS,
            TCMU_ATTR_DEVICE_ID,
            TCMU_ATTR_SUPP_KERN_CMD_REPLY,
        ];
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
        }
        assert_eq!(TCMU_ATTR_UNSPEC, 0);
    }
}
