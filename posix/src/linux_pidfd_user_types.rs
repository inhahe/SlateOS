//! `<sys/pidfd.h>` / `<linux/pidfd.h>` — pid file-descriptor ABI.
//!
//! pidfds (Linux 5.3+) let userspace hold a reliable, race-free
//! handle on a process. systemd uses them for service supervision,
//! container runtimes for cgroup-less child tracking, and CRIU for
//! checkpoint capture. Signals, FD-passing, and reaping all go
//! through the syscalls below instead of the kill/wait pid races.

// ---------------------------------------------------------------------------
// `pidfd_open` and `pidfd_send_signal` flags
// ---------------------------------------------------------------------------

/// Returned pidfd should be non-blocking on `poll(2)`.
pub const PIDFD_NONBLOCK: u32 = 0o4000;
/// Wait for any child of the same thread-group leader.
pub const PIDFD_THREAD: u32 = 0x0000_0010;

// ---------------------------------------------------------------------------
// `pidfd_getfd` reserved flags
// ---------------------------------------------------------------------------
//
// `pidfd_getfd` currently rejects all flag bits — kept here as a 0
// constant so callers can be explicit about passing "no flags".

pub const PIDFD_GETFD_NO_FLAGS: u32 = 0;

// ---------------------------------------------------------------------------
// `waitid` idtype values relevant to pidfd
// ---------------------------------------------------------------------------

pub const P_PID: u32 = 1;
pub const P_PGID: u32 = 2;
pub const P_ALL: u32 = 0;
pub const P_PIDFD: u32 = 3;

// ---------------------------------------------------------------------------
// Syscall numbers (x86_64)
// ---------------------------------------------------------------------------

pub const NR_PIDFD_SEND_SIGNAL: u32 = 424;
pub const NR_PIDFD_OPEN: u32 = 434;
pub const NR_PIDFD_GETFD: u32 = 438;
pub const NR_CLONE3: u32 = 435;

// ---------------------------------------------------------------------------
// `clone3` `clone_args.flags` — pidfd-relevant subset
// ---------------------------------------------------------------------------

pub const CLONE_PIDFD: u64 = 0x0000_1000;
pub const CLONE_PARENT_SETTID: u64 = 0x0010_0000;
pub const CLONE_CHILD_SETTID: u64 = 0x0100_0000;
pub const CLONE_CHILD_CLEARTID: u64 = 0x0020_0000;

// ---------------------------------------------------------------------------
// `pidfd_info` ioctl (kernel ≥ 6.13)
// ---------------------------------------------------------------------------

pub const PIDFS_IOCTL_MAGIC: u8 = 0xFF;
pub const PIDFD_GET_INFO_NR: u8 = 0x10;
pub const PIDFD_GET_CGROUPID_NR: u8 = 0x11;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nonblock_matches_o_nonblock() {
        // PIDFD_NONBLOCK is just O_NONBLOCK from <fcntl.h>.
        assert_eq!(PIDFD_NONBLOCK, 0o4000);
    }

    #[test]
    fn test_pidfd_thread_distinct_from_nonblock() {
        // The clone3 flag PIDFD_THREAD is unrelated to O_NONBLOCK.
        assert_eq!(PIDFD_THREAD, 0x10);
        assert_ne!(PIDFD_THREAD, PIDFD_NONBLOCK);
    }

    #[test]
    fn test_getfd_flags_zero() {
        // pidfd_getfd currently rejects every flag bit — must be 0.
        assert_eq!(PIDFD_GETFD_NO_FLAGS, 0);
    }

    #[test]
    fn test_waitid_idtypes() {
        // The P_PIDFD idtype was added alongside pidfds.
        assert_eq!(P_ALL, 0);
        assert_eq!(P_PID, 1);
        assert_eq!(P_PGID, 2);
        assert_eq!(P_PIDFD, 3);
    }

    #[test]
    fn test_syscall_numbers_in_range_for_x86_64() {
        // All four were added in 5.x; they all live in the 4xx range.
        let n = [NR_PIDFD_SEND_SIGNAL, NR_PIDFD_OPEN, NR_PIDFD_GETFD, NR_CLONE3];
        for &v in n.iter() {
            assert!(v >= 424 && v <= 438);
        }
        // Anchors.
        assert_eq!(NR_PIDFD_SEND_SIGNAL, 424);
        assert_eq!(NR_CLONE3, 435);
        assert_eq!(NR_PIDFD_OPEN, 434);
        assert_eq!(NR_PIDFD_GETFD, 438);
    }

    #[test]
    fn test_clone_flags_single_bit() {
        let c = [
            CLONE_PIDFD,
            CLONE_PARENT_SETTID,
            CLONE_CHILD_SETTID,
            CLONE_CHILD_CLEARTID,
        ];
        for v in c {
            assert!(v.is_power_of_two());
        }
        // CLONE_PIDFD specifically lives at bit 12.
        assert_eq!(CLONE_PIDFD, 1 << 12);
    }

    #[test]
    fn test_pidfs_ioctl_magic_and_nrs() {
        // pidfs uses magic 0xFF — distinct from any common ABI byte.
        assert_eq!(PIDFS_IOCTL_MAGIC, 0xFF);
        // The two pidfs ioctls are consecutive numbers in the 0x10 range.
        assert_eq!(PIDFD_GET_INFO_NR, 0x10);
        assert_eq!(PIDFD_GET_CGROUPID_NR, 0x11);
        assert_eq!(PIDFD_GET_CGROUPID_NR, PIDFD_GET_INFO_NR + 1);
    }
}
