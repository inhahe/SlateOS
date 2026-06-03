//! `<sys/signalfd.h>` — `signalfd(2)` userspace constants and siginfo layout.
//!
//! signalfd lets a process consume signals via a file descriptor
//! instead of an async handler. systemd, tini, runc, and any
//! event-loop-based daemon (libuv, libevent, mio) use the
//! constants below to create the fd and parse the
//! `signalfd_siginfo` records read from it.

// ---------------------------------------------------------------------------
// signalfd(2) flag bits
// ---------------------------------------------------------------------------

/// Set close-on-exec on the new fd.
pub const SFD_CLOEXEC: u32 = 0x0008_0000;
/// Set non-blocking on the new fd.
pub const SFD_NONBLOCK: u32 = 0x0000_0800;

// ---------------------------------------------------------------------------
// Sentinel for "create a new fd"
// ---------------------------------------------------------------------------

/// Passed as the `fd` argument to create a new signalfd.
pub const SFD_NEW_FD: i32 = -1;

// ---------------------------------------------------------------------------
// Size of struct signalfd_siginfo (stable since 2.6.22)
// ---------------------------------------------------------------------------

/// Size of one `signalfd_siginfo` record in bytes.
pub const SIGNALFD_SIGINFO_SIZE: u32 = 128;

// ---------------------------------------------------------------------------
// si_code values relevant to signalfd consumers
// ---------------------------------------------------------------------------

/// Sent by kill(2).
pub const SI_USER: i32 = 0;
/// Sent by the kernel.
pub const SI_KERNEL: i32 = 0x80;
/// Sent by sigqueue(3).
pub const SI_QUEUE: i32 = -1;
/// Sent by a POSIX timer expiration.
pub const SI_TIMER: i32 = -2;
/// Sent by a POSIX mq_notify(3).
pub const SI_MESGQ: i32 = -3;
/// Sent by an async I/O completion.
pub const SI_ASYNCIO: i32 = -4;
/// Sent by tkill(2)/tgkill(2).
pub const SI_TKILL: i32 = -6;

// ---------------------------------------------------------------------------
// SIGCHLD-specific si_code values
// ---------------------------------------------------------------------------

/// Child exited normally.
pub const CLD_EXITED: i32 = 1;
/// Child was killed by a signal.
pub const CLD_KILLED: i32 = 2;
/// Child was killed by a signal and dumped core.
pub const CLD_DUMPED: i32 = 3;
/// Child was traced.
pub const CLD_TRAPPED: i32 = 4;
/// Child was stopped (SIGSTOP/SIGTSTP).
pub const CLD_STOPPED: i32 = 5;
/// Child was continued.
pub const CLD_CONTINUED: i32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_bits_distinct_pow2() {
        // Match O_CLOEXEC / O_NONBLOCK so signalfd4(2) flags can be
        // OR'd directly with open(2) flags in flag-translation code.
        assert!(SFD_CLOEXEC.is_power_of_two());
        assert!(SFD_NONBLOCK.is_power_of_two());
        assert_ne!(SFD_CLOEXEC, SFD_NONBLOCK);
    }

    #[test]
    fn test_new_fd_sentinel() {
        // -1 is the documented "create new" sentinel for signalfd(2).
        assert_eq!(SFD_NEW_FD, -1);
    }

    #[test]
    fn test_siginfo_size_known() {
        // signalfd_siginfo is exactly 128 bytes (the kernel pads any
        // future additions inside the existing 128-byte layout).
        assert_eq!(SIGNALFD_SIGINFO_SIZE, 128);
    }

    #[test]
    fn test_si_codes_distinct() {
        let c = [
            SI_USER,
            SI_KERNEL,
            SI_QUEUE,
            SI_TIMER,
            SI_MESGQ,
            SI_ASYNCIO,
            SI_TKILL,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
        // SI_USER==0 so userspace can use a zeroed siginfo to mean
        // "sent by kill(2) from user space".
        assert_eq!(SI_USER, 0);
    }

    #[test]
    fn test_cld_codes_dense_and_above_zero() {
        let c = [
            CLD_EXITED,
            CLD_KILLED,
            CLD_DUMPED,
            CLD_TRAPPED,
            CLD_STOPPED,
            CLD_CONTINUED,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }
}
