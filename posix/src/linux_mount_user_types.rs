//! `<sys/mount.h>` — classic `mount(2)` flags.
//!
//! The legacy `mount` syscall flag space, used by `util-linux`'s
//! `mount(8)`, by `systemd` for fstab entries, and as the propagation
//! API for namespace setup. The new `fsopen`/`fsmount`/`move_mount`
//! API has a separate (and richer) flag space, but the `MS_*` flags
//! below remain the lingua franca of /etc/fstab.

// ---------------------------------------------------------------------------
// Per-superblock mount option flags
// ---------------------------------------------------------------------------

pub const MS_RDONLY: u64 = 1 << 0;
pub const MS_NOSUID: u64 = 1 << 1;
pub const MS_NODEV: u64 = 1 << 2;
pub const MS_NOEXEC: u64 = 1 << 3;
pub const MS_SYNCHRONOUS: u64 = 1 << 4;
pub const MS_REMOUNT: u64 = 1 << 5;
pub const MS_MANDLOCK: u64 = 1 << 6;
pub const MS_DIRSYNC: u64 = 1 << 7;
pub const MS_NOSYMFOLLOW: u64 = 1 << 8;
pub const MS_NOATIME: u64 = 1 << 10;
pub const MS_NODIRATIME: u64 = 1 << 11;
pub const MS_BIND: u64 = 1 << 12;
pub const MS_MOVE: u64 = 1 << 13;
pub const MS_REC: u64 = 1 << 14;
pub const MS_SILENT: u64 = 1 << 15;
pub const MS_POSIXACL: u64 = 1 << 16;
pub const MS_UNBINDABLE: u64 = 1 << 17;
pub const MS_PRIVATE: u64 = 1 << 18;
pub const MS_SLAVE: u64 = 1 << 19;
pub const MS_SHARED: u64 = 1 << 20;
pub const MS_RELATIME: u64 = 1 << 21;
pub const MS_KERNMOUNT: u64 = 1 << 22;
pub const MS_I_VERSION: u64 = 1 << 23;
pub const MS_STRICTATIME: u64 = 1 << 24;
pub const MS_LAZYTIME: u64 = 1 << 25;

/// Magic constant; combined with another value indicates "no change".
pub const MS_MGC_VAL: u64 = 0xC0ED_0000;
/// Mask for the magic field.
pub const MS_MGC_MSK: u64 = 0xFFFF_0000;

// ---------------------------------------------------------------------------
// `umount2(2)` flags
// ---------------------------------------------------------------------------

/// Force unmount even if busy.
pub const MNT_FORCE: u32 = 1 << 0;
/// Detach immediately, finish cleanup in the background.
pub const MNT_DETACH: u32 = 1 << 1;
/// Mark a mount as expired (not actually unmounted unless idle).
pub const MNT_EXPIRE: u32 = 1 << 2;
/// Don't follow symlinks for the unmount path.
pub const UMOUNT_NOFOLLOW: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Syscall numbers (x86_64)
// ---------------------------------------------------------------------------

pub const NR_MOUNT: u32 = 165;
pub const NR_UMOUNT2: u32 = 166;
pub const NR_PIVOT_ROOT: u32 = 155;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_flags_low_nibble() {
        // Classic security flags are the low four bits.
        assert_eq!(MS_RDONLY | MS_NOSUID | MS_NODEV | MS_NOEXEC, 0xF);
        for f in [MS_RDONLY, MS_NOSUID, MS_NODEV, MS_NOEXEC] {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_propagation_flags_distinct() {
        let p = [MS_UNBINDABLE, MS_PRIVATE, MS_SLAVE, MS_SHARED];
        for v in p {
            assert!(v.is_power_of_two());
        }
        // All four propagation bits sit in bits 17..20.
        assert_eq!(
            (MS_UNBINDABLE | MS_PRIVATE | MS_SLAVE | MS_SHARED) >> 17,
            0xF
        );
    }

    #[test]
    fn test_all_ms_flags_distinct_single_bit() {
        let f = [
            MS_RDONLY,
            MS_NOSUID,
            MS_NODEV,
            MS_NOEXEC,
            MS_SYNCHRONOUS,
            MS_REMOUNT,
            MS_MANDLOCK,
            MS_DIRSYNC,
            MS_NOSYMFOLLOW,
            MS_NOATIME,
            MS_NODIRATIME,
            MS_BIND,
            MS_MOVE,
            MS_REC,
            MS_SILENT,
            MS_POSIXACL,
            MS_UNBINDABLE,
            MS_PRIVATE,
            MS_SLAVE,
            MS_SHARED,
            MS_RELATIME,
            MS_KERNMOUNT,
            MS_I_VERSION,
            MS_STRICTATIME,
            MS_LAZYTIME,
        ];
        for v in f {
            assert!(v.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_mgc_field() {
        // Legacy magic field lives in the top 16 bits.
        assert_eq!(MS_MGC_VAL & MS_MGC_MSK, MS_MGC_VAL);
        assert_eq!(MS_MGC_VAL >> 16, 0xC0ED);
    }

    #[test]
    fn test_umount_flags_dense() {
        let u = [MNT_FORCE, MNT_DETACH, MNT_EXPIRE, UMOUNT_NOFOLLOW];
        for v in u {
            assert!(v.is_power_of_two());
        }
        // Four dense bits.
        assert_eq!(MNT_FORCE | MNT_DETACH | MNT_EXPIRE | UMOUNT_NOFOLLOW, 0xF);
    }

    #[test]
    fn test_syscall_numbers() {
        assert_eq!(NR_MOUNT, 165);
        assert_eq!(NR_UMOUNT2, 166);
        assert!(NR_PIVOT_ROOT < NR_MOUNT);
    }
}
