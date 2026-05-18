//! `<linux/lustre/lustre_user.h>` — Lustre distributed filesystem constants.
//!
//! Lustre is a high-performance parallel distributed filesystem.
//! These constants define stripe parameters, layout types,
//! HSM states, and changelog record types.

// ---------------------------------------------------------------------------
// Magic numbers
// ---------------------------------------------------------------------------

/// Lustre super magic.
pub const LUSTRE_SUPER_MAGIC: u32 = 0x0BD00BD0;

// ---------------------------------------------------------------------------
// Stripe / layout parameters
// ---------------------------------------------------------------------------

/// Default stripe size (1 MiB).
pub const LOV_DEFAULT_STRIPE_SIZE: u32 = 1048576;
/// Default stripe count.
pub const LOV_DEFAULT_STRIPE_COUNT: u32 = 1;
/// All OSTs (use all).
pub const LOV_ALL_STRIPES: u32 = 0xFFFF;
/// Max stripe count.
pub const LOV_MAX_STRIPE_COUNT: u32 = 2000;

// ---------------------------------------------------------------------------
// Layout types (LOV_PATTERN_*)
// ---------------------------------------------------------------------------

/// RAID0 stripe.
pub const LOV_PATTERN_RAID0: u32 = 0x001;
/// RAID1 mirror.
pub const LOV_PATTERN_RAID1: u32 = 0x002;
/// Composite/PFL layout.
pub const LOV_PATTERN_MDT: u32 = 0x100;
/// Overstriping.
pub const LOV_PATTERN_OVERSTRIPING: u32 = 0x200;
/// Foreign layout.
pub const LOV_PATTERN_FOREIGN: u32 = 0xFFFFFFFF;

// ---------------------------------------------------------------------------
// HSM (Hierarchical Storage Management) states
// ---------------------------------------------------------------------------

/// No HSM state.
pub const HS_NONE: u32 = 0x00000000;
/// File exists in HSM.
pub const HS_EXISTS: u32 = 0x00000001;
/// File is dirty.
pub const HS_DIRTY: u32 = 0x00000002;
/// File is released.
pub const HS_RELEASED: u32 = 0x00000004;
/// File is archived.
pub const HS_ARCHIVED: u32 = 0x00000008;
/// No release.
pub const HS_NORELEASE: u32 = 0x00000010;
/// No archive.
pub const HS_NOARCHIVE: u32 = 0x00000020;
/// Lost.
pub const HS_LOST: u32 = 0x00000040;

// ---------------------------------------------------------------------------
// HSM actions
// ---------------------------------------------------------------------------

/// No action.
pub const HUA_NONE: u32 = 1;
/// Archive.
pub const HUA_ARCHIVE: u32 = 10;
/// Restore.
pub const HUA_RESTORE: u32 = 11;
/// Release.
pub const HUA_RELEASE: u32 = 12;
/// Remove.
pub const HUA_REMOVE: u32 = 13;
/// Cancel.
pub const HUA_CANCEL: u32 = 14;

// ---------------------------------------------------------------------------
// Changelog record types
// ---------------------------------------------------------------------------

/// Mark record.
pub const CL_MARK: u32 = 0;
/// Create.
pub const CL_CREATE: u32 = 1;
/// Mkdir.
pub const CL_MKDIR: u32 = 2;
/// Hard link.
pub const CL_HARDLINK: u32 = 3;
/// Soft link.
pub const CL_SOFTLINK: u32 = 4;
/// Mknod.
pub const CL_MKNOD: u32 = 5;
/// Unlink.
pub const CL_UNLINK: u32 = 6;
/// Rmdir.
pub const CL_RMDIR: u32 = 7;
/// Rename (from).
pub const CL_RENAME: u32 = 8;
/// Extended attributes.
pub const CL_EXT: u32 = 9;
/// Open.
pub const CL_OPEN: u32 = 10;
/// Close.
pub const CL_CLOSE: u32 = 11;
/// Layout change.
pub const CL_LAYOUT: u32 = 12;
/// Truncate.
pub const CL_TRUNC: u32 = 13;
/// Set attr.
pub const CL_SETATTR: u32 = 14;
/// Set xattr.
pub const CL_SETXATTR: u32 = 15;
/// HSM.
pub const CL_HSM: u32 = 16;
/// Migrate.
pub const CL_MIGRATE: u32 = 17;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_super_magic() {
        assert_eq!(LUSTRE_SUPER_MAGIC, 0x0BD00BD0);
    }

    #[test]
    fn test_default_stripe() {
        assert_eq!(LOV_DEFAULT_STRIPE_SIZE, 1024 * 1024);
        assert!(LOV_DEFAULT_STRIPE_SIZE.is_power_of_two());
    }

    #[test]
    fn test_pattern_types_distinct() {
        let patterns = [
            LOV_PATTERN_RAID0, LOV_PATTERN_RAID1,
            LOV_PATTERN_MDT, LOV_PATTERN_OVERSTRIPING,
            LOV_PATTERN_FOREIGN,
        ];
        for i in 0..patterns.len() {
            for j in (i + 1)..patterns.len() {
                assert_ne!(patterns[i], patterns[j]);
            }
        }
    }

    #[test]
    fn test_hsm_states_power_of_two() {
        let states = [
            HS_EXISTS, HS_DIRTY, HS_RELEASED, HS_ARCHIVED,
            HS_NORELEASE, HS_NOARCHIVE, HS_LOST,
        ];
        for s in &states {
            assert!(s.is_power_of_two(), "0x{:08x} not power of two", s);
        }
    }

    #[test]
    fn test_hsm_actions_distinct() {
        let actions = [
            HUA_NONE, HUA_ARCHIVE, HUA_RESTORE,
            HUA_RELEASE, HUA_REMOVE, HUA_CANCEL,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_changelog_types_sequential() {
        assert_eq!(CL_MARK, 0);
        assert_eq!(CL_CREATE, 1);
        assert_eq!(CL_MIGRATE, 17);
    }

    #[test]
    fn test_changelog_types_distinct() {
        let types = [
            CL_MARK, CL_CREATE, CL_MKDIR, CL_HARDLINK,
            CL_SOFTLINK, CL_MKNOD, CL_UNLINK, CL_RMDIR,
            CL_RENAME, CL_EXT, CL_OPEN, CL_CLOSE,
            CL_LAYOUT, CL_TRUNC, CL_SETATTR, CL_SETXATTR,
            CL_HSM, CL_MIGRATE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_all_stripes() {
        assert_eq!(LOV_ALL_STRIPES, 0xFFFF);
    }
}
