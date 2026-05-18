//! `<linux/mount.h>` — move_mount() syscall constants.
//!
//! move_mount() moves a mount from one location to another
//! or attaches a detached mount tree.  These constants
//! define move_mount flags.

// ---------------------------------------------------------------------------
// move_mount() flags (MOVE_MOUNT_*)
// ---------------------------------------------------------------------------

/// Source is an open_tree fd.
pub const MOVE_MOUNT_F_SYMLINKS: u32 = 0x00000001;
/// Follow symlinks on source.
pub const MOVE_MOUNT_F_AUTOMOUNTS: u32 = 0x00000002;
/// Empty path on source.
pub const MOVE_MOUNT_F_EMPTY_PATH: u32 = 0x00000004;
/// Follow symlinks on target.
pub const MOVE_MOUNT_T_SYMLINKS: u32 = 0x00000010;
/// Auto-mount on target.
pub const MOVE_MOUNT_T_AUTOMOUNTS: u32 = 0x00000020;
/// Empty path on target.
pub const MOVE_MOUNT_T_EMPTY_PATH: u32 = 0x00000040;
/// Set mount group.
pub const MOVE_MOUNT_SET_GROUP: u32 = 0x00000100;
/// Beneath (mount beneath target).
pub const MOVE_MOUNT_BENEATH: u32 = 0x00000200;

// ---------------------------------------------------------------------------
// move_mount flag masks
// ---------------------------------------------------------------------------

/// All source flags.
pub const MOVE_MOUNT__F_MASK: u32 =
    MOVE_MOUNT_F_SYMLINKS | MOVE_MOUNT_F_AUTOMOUNTS | MOVE_MOUNT_F_EMPTY_PATH;
/// All target flags.
pub const MOVE_MOUNT__T_MASK: u32 =
    MOVE_MOUNT_T_SYMLINKS | MOVE_MOUNT_T_AUTOMOUNTS | MOVE_MOUNT_T_EMPTY_PATH;
/// All flags.
pub const MOVE_MOUNT__MASK: u32 =
    MOVE_MOUNT__F_MASK | MOVE_MOUNT__T_MASK | MOVE_MOUNT_SET_GROUP | MOVE_MOUNT_BENEATH;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_distinct() {
        let flags = [
            MOVE_MOUNT_F_SYMLINKS, MOVE_MOUNT_F_AUTOMOUNTS,
            MOVE_MOUNT_F_EMPTY_PATH, MOVE_MOUNT_T_SYMLINKS,
            MOVE_MOUNT_T_AUTOMOUNTS, MOVE_MOUNT_T_EMPTY_PATH,
            MOVE_MOUNT_SET_GROUP, MOVE_MOUNT_BENEATH,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_f_mask() {
        assert_eq!(
            MOVE_MOUNT__F_MASK,
            MOVE_MOUNT_F_SYMLINKS | MOVE_MOUNT_F_AUTOMOUNTS | MOVE_MOUNT_F_EMPTY_PATH
        );
    }

    #[test]
    fn test_t_mask() {
        assert_eq!(
            MOVE_MOUNT__T_MASK,
            MOVE_MOUNT_T_SYMLINKS | MOVE_MOUNT_T_AUTOMOUNTS | MOVE_MOUNT_T_EMPTY_PATH
        );
    }

    #[test]
    fn test_f_t_no_overlap() {
        assert_eq!(MOVE_MOUNT__F_MASK & MOVE_MOUNT__T_MASK, 0);
    }

    #[test]
    fn test_all_mask_includes_all() {
        assert_ne!(MOVE_MOUNT__MASK & MOVE_MOUNT_SET_GROUP, 0);
        assert_ne!(MOVE_MOUNT__MASK & MOVE_MOUNT_BENEATH, 0);
    }

    #[test]
    fn test_symlinks_is_one() {
        assert_eq!(MOVE_MOUNT_F_SYMLINKS, 1);
    }
}
