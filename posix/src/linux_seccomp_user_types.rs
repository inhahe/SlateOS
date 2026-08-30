//! `<linux/seccomp.h>` — seccomp-BPF ABI.
//!
//! seccomp is the foundation of every container runtime
//! (Docker/runc, podman, systemd's `SystemCallFilter=`), browser
//! sandboxes (Chromium, Firefox), and `bwrap`/Flatpak. The
//! constants here describe the kernel↔userspace ABI: operations,
//! flags, and the BPF filter return-value encoding.

// ---------------------------------------------------------------------------
// `seccomp(2)` operations (`SECCOMP_SET_MODE_*`, etc.)
// ---------------------------------------------------------------------------

pub const SECCOMP_SET_MODE_STRICT: u32 = 0;
pub const SECCOMP_SET_MODE_FILTER: u32 = 1;
pub const SECCOMP_GET_ACTION_AVAIL: u32 = 2;
pub const SECCOMP_GET_NOTIF_SIZES: u32 = 3;

// ---------------------------------------------------------------------------
// `prctl(PR_SET_SECCOMP, mode)` legacy modes
// ---------------------------------------------------------------------------

pub const SECCOMP_MODE_DISABLED: u32 = 0;
pub const SECCOMP_MODE_STRICT: u32 = 1;
pub const SECCOMP_MODE_FILTER: u32 = 2;

// ---------------------------------------------------------------------------
// Flags for `SECCOMP_SET_MODE_FILTER`
// ---------------------------------------------------------------------------

pub const SECCOMP_FILTER_FLAG_TSYNC: u32 = 1 << 0;
pub const SECCOMP_FILTER_FLAG_LOG: u32 = 1 << 1;
pub const SECCOMP_FILTER_FLAG_SPEC_ALLOW: u32 = 1 << 2;
pub const SECCOMP_FILTER_FLAG_NEW_LISTENER: u32 = 1 << 3;
pub const SECCOMP_FILTER_FLAG_TSYNC_ESRCH: u32 = 1 << 4;
pub const SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV: u32 = 1 << 5;

/// Mask of all defined flags as of Linux 6.x.
pub const SECCOMP_FILTER_FLAG_MASK: u32 = SECCOMP_FILTER_FLAG_TSYNC
    | SECCOMP_FILTER_FLAG_LOG
    | SECCOMP_FILTER_FLAG_SPEC_ALLOW
    | SECCOMP_FILTER_FLAG_NEW_LISTENER
    | SECCOMP_FILTER_FLAG_TSYNC_ESRCH
    | SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV;

// ---------------------------------------------------------------------------
// BPF filter return value: action in the high 16 bits, data in low 16
// ---------------------------------------------------------------------------

pub const SECCOMP_RET_ACTION_FULL: u32 = 0xFFFF_0000;
pub const SECCOMP_RET_DATA: u32 = 0x0000_FFFF;

pub const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
pub const SECCOMP_RET_KILL_THREAD: u32 = 0x0000_0000;
pub const SECCOMP_RET_KILL: u32 = SECCOMP_RET_KILL_THREAD; // legacy alias
pub const SECCOMP_RET_TRAP: u32 = 0x0003_0000;
pub const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
pub const SECCOMP_RET_USER_NOTIF: u32 = 0x7FC0_0000;
pub const SECCOMP_RET_TRACE: u32 = 0x7FF0_0000;
pub const SECCOMP_RET_LOG: u32 = 0x7FFC_0000;
pub const SECCOMP_RET_ALLOW: u32 = 0x7FFF_0000;

// ---------------------------------------------------------------------------
// User-notification ioctls (`/proc/<pid>/fd/<seccomp_notif_fd>`)
// ---------------------------------------------------------------------------

pub const SECCOMP_IOC_MAGIC: u8 = b'!';
pub const SECCOMP_IOCTL_NOTIF_RECV: u32 = 0xC0502100;
pub const SECCOMP_IOCTL_NOTIF_SEND: u32 = 0xC0182101;
pub const SECCOMP_IOCTL_NOTIF_ID_VALID: u32 = 0x40082102;
pub const SECCOMP_IOCTL_NOTIF_ADDFD: u32 = 0x40182103;

// ---------------------------------------------------------------------------
// Syscall number
// ---------------------------------------------------------------------------

pub const NR_SECCOMP: u32 = 317;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ops_dense_0_to_3() {
        assert_eq!(SECCOMP_SET_MODE_STRICT, 0);
        assert_eq!(SECCOMP_SET_MODE_FILTER, 1);
        assert_eq!(SECCOMP_GET_ACTION_AVAIL, 2);
        assert_eq!(SECCOMP_GET_NOTIF_SIZES, 3);
    }

    #[test]
    fn test_legacy_modes_dense_0_to_2() {
        assert_eq!(SECCOMP_MODE_DISABLED, 0);
        assert_eq!(SECCOMP_MODE_STRICT, 1);
        assert_eq!(SECCOMP_MODE_FILTER, 2);
    }

    #[test]
    fn test_filter_flags_low_6_bits_dense() {
        let f = [
            SECCOMP_FILTER_FLAG_TSYNC,
            SECCOMP_FILTER_FLAG_LOG,
            SECCOMP_FILTER_FLAG_SPEC_ALLOW,
            SECCOMP_FILTER_FLAG_NEW_LISTENER,
            SECCOMP_FILTER_FLAG_TSYNC_ESRCH,
            SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV,
        ];
        let mut or = 0u32;
        for (i, v) in f.iter().enumerate() {
            assert_eq!(*v, 1 << i);
            or |= v;
        }
        assert_eq!(or, 0x3F);
        assert_eq!(SECCOMP_FILTER_FLAG_MASK, 0x3F);
    }

    #[test]
    fn test_ret_action_data_split() {
        // Filter return: high 16 bits = action, low 16 bits = data.
        assert_eq!(SECCOMP_RET_ACTION_FULL, 0xFFFF_0000);
        assert_eq!(SECCOMP_RET_DATA, 0x0000_FFFF);
        // Their bit-patterns must be complementary.
        assert_eq!(SECCOMP_RET_ACTION_FULL | SECCOMP_RET_DATA, u32::MAX);
        assert_eq!(SECCOMP_RET_ACTION_FULL & SECCOMP_RET_DATA, 0);
    }

    #[test]
    fn test_action_ordering_kill_lowest_allow_highest() {
        // RET values are picked so the most restrictive action has
        // the lowest numeric value when ignoring the special
        // KILL_PROCESS top bit. RET_ALLOW must be the largest
        // "good" action so it loses on min-take.
        // Strip KILL_PROCESS's top bit to compare via numeric order.
        assert!(SECCOMP_RET_KILL_THREAD < SECCOMP_RET_TRAP);
        assert!(SECCOMP_RET_TRAP < SECCOMP_RET_ERRNO);
        assert!(SECCOMP_RET_ERRNO < SECCOMP_RET_USER_NOTIF);
        assert!(SECCOMP_RET_USER_NOTIF < SECCOMP_RET_TRACE);
        assert!(SECCOMP_RET_TRACE < SECCOMP_RET_LOG);
        assert!(SECCOMP_RET_LOG < SECCOMP_RET_ALLOW);
        // The KILL_PROCESS action has the top bit set so it wins
        // against any other when filters are AND-merged.
        assert_eq!(SECCOMP_RET_KILL_PROCESS, 0x8000_0000);
    }

    #[test]
    fn test_notif_ioctl_magic() {
        // The notification ioctls use '!' as their magic letter.
        assert_eq!(SECCOMP_IOC_MAGIC, b'!');
        assert_eq!(SECCOMP_IOC_MAGIC, 0x21);
        // The magic byte sits in bits 8..15 of the ioctl number.
        for &v in &[
            SECCOMP_IOCTL_NOTIF_RECV,
            SECCOMP_IOCTL_NOTIF_SEND,
            SECCOMP_IOCTL_NOTIF_ID_VALID,
            SECCOMP_IOCTL_NOTIF_ADDFD,
        ] {
            assert_eq!((v >> 8) & 0xFF, SECCOMP_IOC_MAGIC as u32);
        }
    }

    #[test]
    fn test_syscall_number() {
        // seccomp(2) was added at NR=317 on x86_64.
        assert_eq!(NR_SECCOMP, 317);
    }
}
