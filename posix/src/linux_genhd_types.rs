//! `<linux/genhd.h>` — generic hard disk partition and disk flag constants.
//!
//! The genhd layer manages block device geometry, partition tables,
//! and disk-level attributes. It bridges the block I/O layer and
//! the partition scanning code, providing structures that describe
//! disks and their partitions to the rest of the kernel.

// ---------------------------------------------------------------------------
// Disk flags (GENHD_FL_*)
// ---------------------------------------------------------------------------

/// Disk is removable (USB stick, CD-ROM).
pub const GENHD_FL_REMOVABLE: u32 = 1 << 0;
/// Disk media is present (for removable drives).
pub const GENHD_FL_MEDIA_CHANGE_NOTIFY: u32 = 1 << 1;
/// Disk is read-only (CD-ROM, write-protected).
pub const GENHD_FL_CD: u32 = 1 << 2;
/// Disk supports discard (TRIM/UNMAP).
pub const GENHD_FL_SUPPRESS_PARTITION_INFO: u32 = 1 << 3;
/// Disk is an extended partition.
pub const GENHD_FL_EXT_DEVT: u32 = 1 << 4;
/// Disk was added by the kernel (not user).
pub const GENHD_FL_NATIVE_CAPACITY: u32 = 1 << 5;
/// Block events are disabled on this disk.
pub const GENHD_FL_BLOCK_EVENTS_ON_EXCL_WRITE: u32 = 1 << 6;
/// Disk doesn't have a partition table.
pub const GENHD_FL_NO_PART: u32 = 1 << 7;
/// Disk is hidden from user enumeration.
pub const GENHD_FL_HIDDEN: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// Partition flags
// ---------------------------------------------------------------------------

/// Partition is bootable.
pub const ADDPART_FLAG_NONE: u32 = 0;
/// Partition is a raid member.
pub const ADDPART_FLAG_RAID: u32 = 1;
/// Partition is a whole-disk entry.
pub const ADDPART_FLAG_WHOLEDISK: u32 = 2;

// ---------------------------------------------------------------------------
// Disk event types
// ---------------------------------------------------------------------------

/// Media was changed (removable device).
pub const DISK_EVENT_MEDIA_CHANGE: u32 = 1 << 0;
/// Media eject request.
pub const DISK_EVENT_EJECT_REQUEST: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Partition table types
// ---------------------------------------------------------------------------

/// MBR (DOS) partition table.
pub const DISK_PTYPE_MBR: u32 = 1;
/// GPT (GUID Partition Table).
pub const DISK_PTYPE_GPT: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disk_flags_no_overlap() {
        let flags = [
            GENHD_FL_REMOVABLE, GENHD_FL_MEDIA_CHANGE_NOTIFY,
            GENHD_FL_CD, GENHD_FL_SUPPRESS_PARTITION_INFO,
            GENHD_FL_EXT_DEVT, GENHD_FL_NATIVE_CAPACITY,
            GENHD_FL_BLOCK_EVENTS_ON_EXCL_WRITE,
            GENHD_FL_NO_PART, GENHD_FL_HIDDEN,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_disk_events_no_overlap() {
        assert!(DISK_EVENT_MEDIA_CHANGE.is_power_of_two());
        assert!(DISK_EVENT_EJECT_REQUEST.is_power_of_two());
        assert_eq!(DISK_EVENT_MEDIA_CHANGE & DISK_EVENT_EJECT_REQUEST, 0);
    }

    #[test]
    fn test_partition_flags_distinct() {
        assert_ne!(ADDPART_FLAG_NONE, ADDPART_FLAG_RAID);
        assert_ne!(ADDPART_FLAG_RAID, ADDPART_FLAG_WHOLEDISK);
    }

    #[test]
    fn test_partition_types_distinct() {
        assert_ne!(DISK_PTYPE_MBR, DISK_PTYPE_GPT);
        assert!(DISK_PTYPE_MBR > 0);
        assert!(DISK_PTYPE_GPT > 0);
    }
}
