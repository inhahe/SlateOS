//! `<sys/syscall.h>` — syscall number definitions.
//!
//! Re-exports native syscall numbers from the `syscall` module.
//! Programs that include `<sys/syscall.h>` find the `SYS_*` constants
//! and `syscall()` wrapper here.

// Re-export all syscall numbers.
pub use crate::syscall::*;

// ---------------------------------------------------------------------------
// Linux x86_64 syscall number aliases
// ---------------------------------------------------------------------------
//
// These are convenience aliases mapping Linux syscall names to our
// native syscall numbers where equivalent functionality exists.

/// `__NR_read` equivalent.
pub const SYS_READ: u64 = 0;

/// `__NR_write` equivalent.
pub const SYS_WRITE: u64 = 1;

/// `__NR_close` equivalent.
pub const SYS_CLOSE: u64 = 3;

/// `__NR_brk` equivalent.
pub const SYS_BRK: u64 = 12;

/// `__NR_rt_sigaction` equivalent.
pub const SYS_RT_SIGACTION: u64 = 13;

/// `__NR_ioctl` equivalent.
pub const SYS_IOCTL: u64 = 16;

/// `__NR_pipe` equivalent.
pub const SYS_PIPE: u64 = 22;

/// `__NR_getpid` equivalent.
pub const SYS_GETPID: u64 = 39;

/// `__NR_fork` equivalent.
pub const SYS_FORK: u64 = 57;

/// `__NR_execve` equivalent.
pub const SYS_EXECVE: u64 = 59;

/// `__NR_exit` equivalent.
pub const SYS_EXIT_LINUX: u64 = 60;

/// `__NR_gettimeofday` equivalent.
pub const SYS_GETTIMEOFDAY: u64 = 96;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_linux_aliases_nonzero() {
        // Some may be zero (SYS_READ), but they should be defined.
        assert_eq!(SYS_READ, 0);
        assert_ne!(SYS_WRITE, SYS_READ);
        assert_ne!(SYS_GETPID, SYS_READ);
    }

    #[test]
    fn test_native_syscalls_exist() {
        // Verify some native syscall numbers are accessible.
        assert!(SYS_EXIT > 0);
        assert!(SYS_MMAP > 0);
    }

    #[test]
    fn test_linux_aliases_distinct() {
        let vals = [
            SYS_READ,
            SYS_WRITE,
            SYS_CLOSE,
            SYS_BRK,
            SYS_RT_SIGACTION,
            SYS_IOCTL,
            SYS_PIPE,
            SYS_GETPID,
            SYS_FORK,
            SYS_EXECVE,
            SYS_EXIT_LINUX,
            SYS_GETTIMEOFDAY,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j], "SYS_ constants must be distinct");
            }
        }
    }
}
