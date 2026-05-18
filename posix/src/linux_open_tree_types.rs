//! `<linux/mount.h>` — open_tree() syscall constants.
//!
//! open_tree() opens or clones a mount subtree, returning
//! a file descriptor.  These constants define open_tree
//! flags and related parameters.

// ---------------------------------------------------------------------------
// open_tree() flags (OPEN_TREE_*)
// ---------------------------------------------------------------------------

/// Clone the mount subtree.
pub const OPEN_TREE_CLONE: u32 = 1;
/// Close-on-exec.
pub const OPEN_TREE_CLOEXEC: u32 = 0x00080000;

// ---------------------------------------------------------------------------
// open_tree() AT_* flags
// ---------------------------------------------------------------------------

/// Empty path.
pub const AT_EMPTY_PATH_OT: u32 = 0x1000;
/// No automount.
pub const AT_NO_AUTOMOUNT_OT: u32 = 0x800;
/// Symlink no follow.
pub const AT_SYMLINK_NOFOLLOW_OT: u32 = 0x100;
/// Recursive.
pub const AT_RECURSIVE_OT: u32 = 0x8000;

// ---------------------------------------------------------------------------
// open_tree combined flags mask
// ---------------------------------------------------------------------------

/// All valid open_tree flags.
pub const OPEN_TREE_FLAGS: u32 = OPEN_TREE_CLONE | OPEN_TREE_CLOEXEC;

// ---------------------------------------------------------------------------
// Clone mount flag combinations
// ---------------------------------------------------------------------------

/// Clone with recursive option.
pub const OPEN_TREE_CLONE_RECURSIVE: u32 = OPEN_TREE_CLONE | AT_RECURSIVE_OT;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_is_one() {
        assert_eq!(OPEN_TREE_CLONE, 1);
    }

    #[test]
    fn test_cloexec_value() {
        assert_eq!(OPEN_TREE_CLOEXEC, 0x00080000);
    }

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(OPEN_TREE_CLONE & OPEN_TREE_CLOEXEC, 0);
    }

    #[test]
    fn test_at_flags_distinct() {
        let flags = [
            AT_EMPTY_PATH_OT, AT_NO_AUTOMOUNT_OT,
            AT_SYMLINK_NOFOLLOW_OT, AT_RECURSIVE_OT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_flags_mask() {
        assert_eq!(OPEN_TREE_FLAGS, OPEN_TREE_CLONE | OPEN_TREE_CLOEXEC);
    }

    #[test]
    fn test_clone_recursive() {
        assert_eq!(OPEN_TREE_CLONE_RECURSIVE, OPEN_TREE_CLONE | AT_RECURSIVE_OT);
        assert_ne!(OPEN_TREE_CLONE_RECURSIVE & OPEN_TREE_CLONE, 0);
        assert_ne!(OPEN_TREE_CLONE_RECURSIVE & AT_RECURSIVE_OT, 0);
    }
}
