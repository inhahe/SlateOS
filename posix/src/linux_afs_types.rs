//! `<linux/afs.h>` — AFS (Andrew File System) constants.
//!
//! AFS is a distributed filesystem protocol.
//! These constants define volume types, file types,
//! access rights, and callback types.

// ---------------------------------------------------------------------------
// Magic
// ---------------------------------------------------------------------------

/// AFS super magic.
pub const AFS_SUPER_MAGIC: u32 = 0x5346414F;

// ---------------------------------------------------------------------------
// Volume types
// ---------------------------------------------------------------------------

/// Read-write volume.
pub const AFSVL_RWVOL: u32 = 0;
/// Read-only volume.
pub const AFSVL_ROVOL: u32 = 1;
/// Backup volume.
pub const AFSVL_BACKVOL: u32 = 2;

// ---------------------------------------------------------------------------
// File types
// ---------------------------------------------------------------------------

/// Regular file.
pub const AFS_FTYPE_FILE: u32 = 1;
/// Directory.
pub const AFS_FTYPE_DIR: u32 = 2;
/// Symlink.
pub const AFS_FTYPE_SYMLINK: u32 = 3;

// ---------------------------------------------------------------------------
// Access rights (ACL)
// ---------------------------------------------------------------------------

/// Read.
pub const AFS_ACE_READ: u32 = 0x01;
/// Write.
pub const AFS_ACE_WRITE: u32 = 0x02;
/// Insert.
pub const AFS_ACE_INSERT: u32 = 0x04;
/// Lookup.
pub const AFS_ACE_LOOKUP: u32 = 0x08;
/// Delete.
pub const AFS_ACE_DELETE: u32 = 0x10;
/// Lock.
pub const AFS_ACE_LOCK: u32 = 0x20;
/// Administer.
pub const AFS_ACE_ADMINISTER: u32 = 0x40;

// ---------------------------------------------------------------------------
// Callback types
// ---------------------------------------------------------------------------

/// Exclusive callback.
pub const AFS_CB_EXCLUSIVE: u32 = 1;
/// Shared callback.
pub const AFS_CB_SHARED: u32 = 2;
/// Dropped callback.
pub const AFS_CB_DROPPED: u32 = 3;

// ---------------------------------------------------------------------------
// Lock types
// ---------------------------------------------------------------------------

/// No lock.
pub const AFS_LOCK_NONE: u32 = 0;
/// Read lock.
pub const AFS_LOCK_READ: u32 = 1;
/// Write lock.
pub const AFS_LOCK_WRITE: u32 = 2;

// ---------------------------------------------------------------------------
// Server/cell parameters
// ---------------------------------------------------------------------------

/// Max cell name length.
pub const AFS_MAXCELLNAME: u32 = 256;
/// Max volume name length.
pub const AFS_MAXVOLNAME: u32 = 64;
/// Max server addresses.
pub const AFS_MAXSERVERS: u32 = 8;
/// Max file name length.
pub const AFS_MAXNAMELEN: u32 = 256;

// ---------------------------------------------------------------------------
// FID components
// ---------------------------------------------------------------------------

/// Root vnode number.
pub const AFS_ROOT_VNODE: u32 = 1;
/// Root uniquifier.
pub const AFS_ROOT_UNIQUE: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_super_magic() {
        assert_eq!(AFS_SUPER_MAGIC, 0x5346414F);
    }

    #[test]
    fn test_volume_types_sequential() {
        assert_eq!(AFSVL_RWVOL, 0);
        assert_eq!(AFSVL_ROVOL, 1);
        assert_eq!(AFSVL_BACKVOL, 2);
    }

    #[test]
    fn test_file_types_sequential() {
        assert_eq!(AFS_FTYPE_FILE, 1);
        assert_eq!(AFS_FTYPE_DIR, 2);
        assert_eq!(AFS_FTYPE_SYMLINK, 3);
    }

    #[test]
    fn test_ace_rights_power_of_two() {
        let rights = [
            AFS_ACE_READ, AFS_ACE_WRITE, AFS_ACE_INSERT,
            AFS_ACE_LOOKUP, AFS_ACE_DELETE, AFS_ACE_LOCK,
            AFS_ACE_ADMINISTER,
        ];
        for r in &rights {
            assert!(r.is_power_of_two(), "0x{:02x} not power of two", r);
        }
    }

    #[test]
    fn test_ace_rights_distinct() {
        let rights = [
            AFS_ACE_READ, AFS_ACE_WRITE, AFS_ACE_INSERT,
            AFS_ACE_LOOKUP, AFS_ACE_DELETE, AFS_ACE_LOCK,
            AFS_ACE_ADMINISTER,
        ];
        for i in 0..rights.len() {
            for j in (i + 1)..rights.len() {
                assert_ne!(rights[i], rights[j]);
            }
        }
    }

    #[test]
    fn test_callback_types() {
        assert_eq!(AFS_CB_EXCLUSIVE, 1);
        assert_eq!(AFS_CB_SHARED, 2);
        assert_eq!(AFS_CB_DROPPED, 3);
    }

    #[test]
    fn test_lock_types() {
        assert_eq!(AFS_LOCK_NONE, 0);
        assert_eq!(AFS_LOCK_READ, 1);
        assert_eq!(AFS_LOCK_WRITE, 2);
    }

    #[test]
    fn test_name_limits() {
        assert_eq!(AFS_MAXCELLNAME, 256);
        assert_eq!(AFS_MAXVOLNAME, 64);
    }

    #[test]
    fn test_root_fid() {
        assert_eq!(AFS_ROOT_VNODE, 1);
        assert_eq!(AFS_ROOT_UNIQUE, 1);
    }
}
