//! `<linux/mount.h>` — `move_mount(2)` syscall (Linux 5.2+).
//!
//! `move_mount` is half of the new file-system API. Combined with
//! `fsopen`/`fsconfig`/`fsmount` it lets a process build a mount in
//! a detached state and then attach it to the namespace atomically.
//! systemd's `nspawn`, container runtimes, and snapshot-based image
//! tools use it because it avoids the race windows in the classic
//! `mount(2)` API.

// ---------------------------------------------------------------------------
// `move_mount(2)` flags
// ---------------------------------------------------------------------------

/// Allow `from_pathname` to be empty (use only the fd).
pub const MOVE_MOUNT_F_SYMLINKS: u32 = 0x0000_0001;
/// Follow auto-mount points in the source path.
pub const MOVE_MOUNT_F_AUTOMOUNTS: u32 = 0x0000_0002;
/// Treat empty `from_pathname` as "use fd alone".
pub const MOVE_MOUNT_F_EMPTY_PATH: u32 = 0x0000_0004;
/// Symlinks resolved relative to destination path.
pub const MOVE_MOUNT_T_SYMLINKS: u32 = 0x0000_0010;
/// Follow auto-mount points in the destination path.
pub const MOVE_MOUNT_T_AUTOMOUNTS: u32 = 0x0000_0020;
/// Treat empty `to_pathname` as "use fd alone".
pub const MOVE_MOUNT_T_EMPTY_PATH: u32 = 0x0000_0040;
/// Set the mount group ID (Linux 5.14+).
pub const MOVE_MOUNT_SET_GROUP: u32 = 0x0000_0100;
/// Move to a detached mount tree (Linux 5.16+).
pub const MOVE_MOUNT_BENEATH: u32 = 0x0000_0200;

/// Mask covering all valid `move_mount` flags.
pub const MOVE_MOUNT_F_MASK: u32 =
    MOVE_MOUNT_F_SYMLINKS | MOVE_MOUNT_F_AUTOMOUNTS | MOVE_MOUNT_F_EMPTY_PATH;
pub const MOVE_MOUNT_T_MASK: u32 =
    MOVE_MOUNT_T_SYMLINKS | MOVE_MOUNT_T_AUTOMOUNTS | MOVE_MOUNT_T_EMPTY_PATH;

/// All defined flags.
pub const MOVE_MOUNT_ALL: u32 = MOVE_MOUNT_F_MASK
    | MOVE_MOUNT_T_MASK
    | MOVE_MOUNT_SET_GROUP
    | MOVE_MOUNT_BENEATH;

// ---------------------------------------------------------------------------
// Syscall numbers (x86_64, Linux 5.2+)
// ---------------------------------------------------------------------------

pub const NR_OPEN_TREE: u32 = 428;
pub const NR_MOVE_MOUNT: u32 = 429;
pub const NR_FSOPEN: u32 = 430;
pub const NR_FSCONFIG: u32 = 431;
pub const NR_FSMOUNT: u32 = 432;
pub const NR_FSPICK: u32 = 433;

// ---------------------------------------------------------------------------
// `AT_*` constants reused by `move_mount` (subset)
// ---------------------------------------------------------------------------

pub const AT_FDCWD: i32 = -100;
pub const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
pub const AT_EMPTY_PATH: u32 = 0x1000;
pub const AT_RECURSIVE: u32 = 0x8000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_and_to_flag_blocks_share_layout() {
        // Source flags occupy the low nibble (bits 0..2).
        assert_eq!(MOVE_MOUNT_F_MASK, 0x07);
        // Destination flags occupy bits 4..6 — a 4-bit shift of the source layout.
        assert_eq!(MOVE_MOUNT_T_MASK, MOVE_MOUNT_F_MASK << 4);
    }

    #[test]
    fn test_all_flags_single_bit_and_disjoint() {
        let f = [
            MOVE_MOUNT_F_SYMLINKS,
            MOVE_MOUNT_F_AUTOMOUNTS,
            MOVE_MOUNT_F_EMPTY_PATH,
            MOVE_MOUNT_T_SYMLINKS,
            MOVE_MOUNT_T_AUTOMOUNTS,
            MOVE_MOUNT_T_EMPTY_PATH,
            MOVE_MOUNT_SET_GROUP,
            MOVE_MOUNT_BENEATH,
        ];
        for v in f {
            assert!(v.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
        // ALL is the union of every defined bit.
        let or = f.iter().fold(0, |a, b| a | b);
        assert_eq!(MOVE_MOUNT_ALL, or);
    }

    #[test]
    fn test_new_fs_api_syscalls_dense_428_to_433() {
        let n = [
            NR_OPEN_TREE,
            NR_MOVE_MOUNT,
            NR_FSOPEN,
            NR_FSCONFIG,
            NR_FSMOUNT,
            NR_FSPICK,
        ];
        for (i, &v) in n.iter().enumerate() {
            assert_eq!(v, 428 + i as u32);
        }
    }

    #[test]
    fn test_at_constants() {
        assert_eq!(AT_FDCWD, -100);
        assert_eq!(AT_SYMLINK_NOFOLLOW, 0x100);
        assert_eq!(AT_EMPTY_PATH, 0x1000);
        assert_eq!(AT_RECURSIVE, 0x8000);
        // The three flag bits never collide.
        assert_eq!(AT_SYMLINK_NOFOLLOW & AT_EMPTY_PATH, 0);
        assert_eq!(AT_SYMLINK_NOFOLLOW & AT_RECURSIVE, 0);
        assert_eq!(AT_EMPTY_PATH & AT_RECURSIVE, 0);
    }
}
