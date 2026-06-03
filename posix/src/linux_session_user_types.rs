//! Session / process-group syscalls — `setsid`, `getsid`, `setpgid`,
//! `getpgid`, `getpgrp`, controlling-terminal management.
//!
//! Every login shell, `tmux`, `screen`, and job-control implementation
//! relies on the session/process-group layering described here. The
//! syscall numbers and the `TIOCSCTTY`/`TIOCNOTTY` ioctls are the
//! stable surface.

// ---------------------------------------------------------------------------
// Controlling-tty ioctls
// ---------------------------------------------------------------------------

/// Make the open file's tty the controlling terminal.
pub const TIOCSCTTY: u32 = 0x540E;
/// Disconnect from the controlling terminal.
pub const TIOCNOTTY: u32 = 0x5422;
/// Get the foreground process group of the controlling tty.
pub const TIOCGPGRP: u32 = 0x540F;
/// Set the foreground process group of the controlling tty.
pub const TIOCSPGRP: u32 = 0x5410;
/// Get the session id of the session leader on the tty.
pub const TIOCGSID: u32 = 0x5429;

// ---------------------------------------------------------------------------
// Syscall numbers (x86_64)
// ---------------------------------------------------------------------------

pub const NR_GETPID: u32 = 39;
pub const NR_GETPPID: u32 = 110;
pub const NR_GETTID: u32 = 186;

pub const NR_GETPGRP: u32 = 111;
pub const NR_GETPGID: u32 = 121;
pub const NR_SETPGID: u32 = 109;
pub const NR_GETSID: u32 = 124;
pub const NR_SETSID: u32 = 112;

pub const NR_GETUID: u32 = 102;
pub const NR_GETEUID: u32 = 107;
pub const NR_GETGID: u32 = 104;
pub const NR_GETEGID: u32 = 108;
pub const NR_GETGROUPS: u32 = 115;
pub const NR_SETGROUPS: u32 = 116;

// ---------------------------------------------------------------------------
// Sentinel pids/pgids
// ---------------------------------------------------------------------------

/// `getsid(0)` / `setpgid(0, 0)` etc. mean "the calling process".
pub const SELF_PID_SENTINEL: u32 = 0;

/// Reserved PID for the swapper / idle task (kernel-internal, but
/// userspace queries treat `pid == 0` specially).
pub const PID_KERNEL_IDLE: u32 = 0;

/// PID 1 is always init / the session manager.
pub const PID_INIT: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controlling_tty_ioctls_distinct() {
        let i = [TIOCSCTTY, TIOCNOTTY, TIOCGPGRP, TIOCSPGRP, TIOCGSID];
        for a in 0..i.len() {
            for b in (a + 1)..i.len() {
                assert_ne!(i[a], i[b]);
            }
        }
        // The pgrp pair are adjacent.
        assert_eq!(TIOCSPGRP, TIOCGPGRP + 1);
    }

    #[test]
    fn test_pid_syscalls_well_known() {
        // getpid is #39 on x86_64, gettid is #186.
        assert_eq!(NR_GETPID, 39);
        assert_eq!(NR_GETTID, 186);
        assert_eq!(NR_GETPPID, 110);
    }

    #[test]
    fn test_pgrp_session_block_dense() {
        // setpgid/getpgrp/setsid/getsid sit in a 109..124 block.
        let n = [NR_SETPGID, NR_GETPGRP, NR_SETSID, NR_GETPGID, NR_GETSID];
        for v in n {
            assert!((109..=124).contains(&v));
        }
    }

    #[test]
    fn test_uid_gid_syscalls_dense() {
        // The "credentials" syscall numbers cluster around 102..108.
        let n = [
            NR_GETUID, NR_GETEUID, NR_GETGID, NR_GETEGID, NR_GETGROUPS, NR_SETGROUPS,
        ];
        for v in n {
            assert!((102..=116).contains(&v));
        }
    }

    #[test]
    fn test_pid_sentinels() {
        // pid 0 is reserved for both "self" and the kernel idle task.
        assert_eq!(SELF_PID_SENTINEL, 0);
        assert_eq!(PID_KERNEL_IDLE, 0);
        // init is pid 1.
        assert_eq!(PID_INIT, 1);
    }
}
