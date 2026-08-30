//! `<linux/cgroupstats.h>` — taskstats-derived cgroup statistics.
//!
//! cgroupstats are delivered through a TASKSTATS-family netlink
//! request that returns per-cgroup aggregate task counts. This
//! module covers the netlink attribute IDs and the
//! `struct cgroupstats` field layout.

// ---------------------------------------------------------------------------
// Generic-netlink family name
// ---------------------------------------------------------------------------

/// Generic-netlink family the kernel registers for taskstats/cgroupstats.
pub const CGROUPSTATS_FAMILY_NAME: &str = "TASKSTATS";

// ---------------------------------------------------------------------------
// Netlink command IDs (`enum`)
// ---------------------------------------------------------------------------

pub const CGROUPSTATS_CMD_UNSPEC: u32 = 0;
pub const CGROUPSTATS_CMD_GET: u32 = 1;
pub const CGROUPSTATS_CMD_NEW: u32 = 2;

// ---------------------------------------------------------------------------
// Netlink attribute IDs (`enum`)
// ---------------------------------------------------------------------------

pub const CGROUPSTATS_CMD_ATTR_UNSPEC: u32 = 0;
pub const CGROUPSTATS_CMD_ATTR_FD: u32 = 1;

pub const CGROUPSTATS_TYPE_UNSPEC: u32 = 0;
pub const CGROUPSTATS_TYPE_CGROUP_STATS: u32 = 1;

// ---------------------------------------------------------------------------
// `struct cgroupstats` field offsets (six packed u64 fields)
// ---------------------------------------------------------------------------

pub const CGROUPSTATS_OFF_NR_SLEEPING: usize = 0;
pub const CGROUPSTATS_OFF_NR_RUNNING: usize = 8;
pub const CGROUPSTATS_OFF_NR_STOPPED: usize = 16;
pub const CGROUPSTATS_OFF_NR_UNINTERRUPTIBLE: usize = 24;
pub const CGROUPSTATS_OFF_NR_IO_WAIT: usize = 32;
pub const CGROUPSTATS_SIZE: usize = 40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_name_is_taskstats() {
        assert_eq!(CGROUPSTATS_FAMILY_NAME, "TASKSTATS");
        // Generic-netlink family names are uppercase.
        for c in CGROUPSTATS_FAMILY_NAME.chars() {
            assert!(c.is_ascii_uppercase());
        }
    }

    #[test]
    fn test_commands_dense_0_to_2() {
        let c = [
            CGROUPSTATS_CMD_UNSPEC,
            CGROUPSTATS_CMD_GET,
            CGROUPSTATS_CMD_NEW,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_attr_ids_dense() {
        assert_eq!(CGROUPSTATS_CMD_ATTR_UNSPEC, 0);
        assert_eq!(CGROUPSTATS_CMD_ATTR_FD, 1);
        assert_eq!(CGROUPSTATS_TYPE_UNSPEC, 0);
        assert_eq!(CGROUPSTATS_TYPE_CGROUP_STATS, 1);
    }

    #[test]
    fn test_struct_layout_five_packed_u64s() {
        let o = [
            CGROUPSTATS_OFF_NR_SLEEPING,
            CGROUPSTATS_OFF_NR_RUNNING,
            CGROUPSTATS_OFF_NR_STOPPED,
            CGROUPSTATS_OFF_NR_UNINTERRUPTIBLE,
            CGROUPSTATS_OFF_NR_IO_WAIT,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v, i * 8);
        }
        assert_eq!(CGROUPSTATS_SIZE, 5 * 8);
    }

    #[test]
    fn test_running_after_sleeping() {
        // Sleeping is typically the largest count; running comes second.
        assert!(CGROUPSTATS_OFF_NR_RUNNING > CGROUPSTATS_OFF_NR_SLEEPING);
        // IO wait is the last field.
        assert_eq!(CGROUPSTATS_OFF_NR_IO_WAIT, CGROUPSTATS_SIZE - 8);
    }

    #[test]
    fn test_packed_struct_size_is_40() {
        // 40 bytes = 5 u64s. Matches the kernel struct.
        assert_eq!(CGROUPSTATS_SIZE, 40);
    }
}
