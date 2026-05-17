//! `<linux/dax.h>` — Direct Access (DAX) constants.
//!
//! DAX allows applications to memory-map persistent memory (PMEM)
//! and access it directly, bypassing the page cache. This eliminates
//! double-copying (device→page cache→user) for PMEM-backed files.
//! DAX is used with NVDIMM/PMEM devices and ext4/XFS filesystems
//! with the `-o dax` mount option. The /dev/daxN.M character devices
//! allow raw DAX access to PMEM regions without a filesystem.

// ---------------------------------------------------------------------------
// DAX device types
// ---------------------------------------------------------------------------

/// Filesystem DAX (file-backed, managed by FS).
pub const DAX_TYPE_FS: u32 = 0;
/// Device DAX (character device, raw PMEM access).
pub const DAX_TYPE_DEVICE: u32 = 1;

// ---------------------------------------------------------------------------
// DAX filesystem mount flags
// ---------------------------------------------------------------------------

/// DAX disabled (use page cache normally).
pub const DAX_FS_NEVER: u32 = 0;
/// DAX always (all files use DAX, no page cache).
pub const DAX_FS_ALWAYS: u32 = 1;
/// DAX per-inode (DAX controlled by file attribute).
pub const DAX_FS_INODE: u32 = 2;

// ---------------------------------------------------------------------------
// DAX inode flags (per-file DAX control, FS_XFLAG_DAX)
// ---------------------------------------------------------------------------

/// File does not use DAX.
pub const DAX_INODE_DISABLED: u32 = 0;
/// File uses DAX for I/O.
pub const DAX_INODE_ENABLED: u32 = 1;

// ---------------------------------------------------------------------------
// DAX device IOCTLs
// ---------------------------------------------------------------------------

/// Get DAX device size.
pub const DAXCTL_GET_SIZE: u32 = 0x01;
/// Set online/offline state.
pub const DAXCTL_SET_ONLINE: u32 = 0x02;
/// Set device mode (devdax/system-ram).
pub const DAXCTL_SET_MODE: u32 = 0x03;

// ---------------------------------------------------------------------------
// DAX device modes
// ---------------------------------------------------------------------------

/// Device DAX mode (raw character device access).
pub const DAX_MODE_DEVDAX: u32 = 0;
/// System RAM mode (hot-add as regular memory).
pub const DAX_MODE_SYSTEM_RAM: u32 = 1;

// ---------------------------------------------------------------------------
// DAX memory region states (for system-ram mode)
// ---------------------------------------------------------------------------

/// Region is offline (not in use as system RAM).
pub const DAX_REGION_OFFLINE: u32 = 0;
/// Region is online (active as system RAM).
pub const DAX_REGION_ONLINE: u32 = 1;
/// Region is being onlined (transition in progress).
pub const DAX_REGION_ONLINING: u32 = 2;

// ---------------------------------------------------------------------------
// DAX alignment requirements
// ---------------------------------------------------------------------------

/// Minimum DAX alignment (2 MiB huge page).
pub const DAX_ALIGN_2M: u32 = 2 * 1024 * 1024;
/// Optional DAX alignment (1 GiB huge page).
pub const DAX_ALIGN_1G: u32 = 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        assert_ne!(DAX_TYPE_FS, DAX_TYPE_DEVICE);
    }

    #[test]
    fn test_fs_modes_distinct() {
        let modes = [DAX_FS_NEVER, DAX_FS_ALWAYS, DAX_FS_INODE];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_inode_flags_distinct() {
        assert_ne!(DAX_INODE_DISABLED, DAX_INODE_ENABLED);
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [DAXCTL_GET_SIZE, DAXCTL_SET_ONLINE, DAXCTL_SET_MODE];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_device_modes_distinct() {
        assert_ne!(DAX_MODE_DEVDAX, DAX_MODE_SYSTEM_RAM);
    }

    #[test]
    fn test_region_states_distinct() {
        let states = [
            DAX_REGION_OFFLINE, DAX_REGION_ONLINE, DAX_REGION_ONLINING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_alignment_values() {
        assert!(DAX_ALIGN_2M.is_power_of_two());
        assert!(DAX_ALIGN_1G.is_power_of_two());
        assert!(DAX_ALIGN_2M < DAX_ALIGN_1G);
    }
}
