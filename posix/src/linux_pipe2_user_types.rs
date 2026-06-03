//! `pipe2(2)` flags and pipe sizing ABI.
//!
//! `pipe2` extends the classic `pipe(2)` with a flags argument so
//! callers don't need a follow-up `fcntl(F_SETFD, FD_CLOEXEC)` race.
//! The fcntl ops here tune the kernel pipe-buffer size — used by
//! shells, log aggregators, and anything that wants to avoid blocking
//! on bursty stdout.

// ---------------------------------------------------------------------------
// `pipe2` flag bits (must subset `<fcntl.h>` O_* values)
// ---------------------------------------------------------------------------

pub const O_CLOEXEC: u32 = 0o2_000_000;
pub const O_NONBLOCK: u32 = 0o4_000;
pub const O_DIRECT: u32 = 0o40_000;
pub const O_NOTIFICATION_PIPE: u32 = O_EXCL;

const O_EXCL: u32 = 0o200;

/// Mask of every flag bit `pipe2` accepts. Anything outside returns
/// `EINVAL`.
pub const PIPE2_VALID_FLAGS: u32 =
    O_CLOEXEC | O_NONBLOCK | O_DIRECT | O_NOTIFICATION_PIPE;

// ---------------------------------------------------------------------------
// Pipe-buffer sizing ABI (`fcntl(F_*PIPE_SZ)` + sysctl)
// ---------------------------------------------------------------------------

pub const F_SETPIPE_SZ: u32 = 1031;
pub const F_GETPIPE_SZ: u32 = 1032;

/// Default capacity of a freshly-created pipe (since 2.6.35).
pub const PIPE_DEF_BUFFERS: u32 = 16;
/// Stock 4 KiB page-sized buffer × `PIPE_DEF_BUFFERS` = 64 KiB.
pub const PIPE_DEF_SIZE_BYTES: u32 = PIPE_DEF_BUFFERS * 4096;

pub const SYSCTL_PIPE_MAX_SIZE: &str = "/proc/sys/fs/pipe-max-size";
pub const SYSCTL_PIPE_USER_PAGES_HARD: &str =
    "/proc/sys/fs/pipe-user-pages-hard";
pub const SYSCTL_PIPE_USER_PAGES_SOFT: &str =
    "/proc/sys/fs/pipe-user-pages-soft";

// ---------------------------------------------------------------------------
// Syscalls
// ---------------------------------------------------------------------------

pub const NR_PIPE: u32 = 22;
pub const NR_PIPE2: u32 = 293;

// ---------------------------------------------------------------------------
// `splice` / `tee` / `vmsplice` related limits
// ---------------------------------------------------------------------------

/// `PIPE_BUF` — guaranteed-atomic write size into a pipe (POSIX).
pub const PIPE_BUF: u32 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_values_match_fcntl() {
        assert_eq!(O_CLOEXEC, 0o2000000);
        assert_eq!(O_NONBLOCK, 0o4000);
        assert_eq!(O_DIRECT, 0o40000);
        // O_NOTIFICATION_PIPE rides O_EXCL's bit (it's accepted only by
        // pipe2, not by open(2)).
        assert_eq!(O_NOTIFICATION_PIPE, 0o200);
    }

    #[test]
    fn test_valid_flag_mask_is_or_of_all() {
        let expected = O_CLOEXEC | O_NONBLOCK | O_DIRECT | O_NOTIFICATION_PIPE;
        assert_eq!(PIPE2_VALID_FLAGS, expected);
    }

    #[test]
    fn test_fcntl_pipe_sz_pair_consecutive() {
        // SETPIPE_SZ / GETPIPE_SZ pair are consecutive (1031, 1032).
        assert_eq!(F_SETPIPE_SZ, 1031);
        assert_eq!(F_GETPIPE_SZ, F_SETPIPE_SZ + 1);
    }

    #[test]
    fn test_default_pipe_buffer_count_and_bytes() {
        // 16 × 4 KiB = 64 KiB default.
        assert_eq!(PIPE_DEF_BUFFERS, 16);
        assert_eq!(PIPE_DEF_SIZE_BYTES, 64 * 1024);
    }

    #[test]
    fn test_sysctl_paths_under_fs() {
        let p = [
            SYSCTL_PIPE_MAX_SIZE,
            SYSCTL_PIPE_USER_PAGES_HARD,
            SYSCTL_PIPE_USER_PAGES_SOFT,
        ];
        for path in p {
            assert!(path.starts_with("/proc/sys/fs/"));
        }
    }

    #[test]
    fn test_syscall_numbers_x86_64() {
        // Old pipe is syscall 22; pipe2 was added later as 293.
        assert_eq!(NR_PIPE, 22);
        assert_eq!(NR_PIPE2, 293);
    }

    #[test]
    fn test_pipe_buf_atomic_write_size() {
        // POSIX requires `PIPE_BUF >= _POSIX_PIPE_BUF` (512). Linux ships 4096.
        assert_eq!(PIPE_BUF, 4096);
        assert!(PIPE_BUF >= 512);
    }
}
