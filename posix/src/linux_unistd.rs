//! `<linux/unistd.h>` — Linux-specific system call wrappers.
//!
//! Re-exports Linux-specific syscall wrappers and constants that
//! extend the POSIX `<unistd.h>` interface.

// ---------------------------------------------------------------------------
// Re-exports from unistd (uid/gid operations)
// ---------------------------------------------------------------------------

pub use crate::unistd::getegid;
pub use crate::unistd::geteuid;
pub use crate::unistd::getgid;
pub use crate::unistd::getrandom;
pub use crate::unistd::getuid;
pub use crate::unistd::klogctl;
pub use crate::unistd::prctl;
pub use crate::unistd::setegid;
pub use crate::unistd::seteuid;
pub use crate::unistd::setgid;
pub use crate::unistd::setregid;
pub use crate::unistd::setreuid;
pub use crate::unistd::setuid;

// ---------------------------------------------------------------------------
// Re-exports from process
// ---------------------------------------------------------------------------

pub use crate::process::_exit;
pub use crate::process::clone3;
pub use crate::process::fork;
pub use crate::process::getpid;
pub use crate::process::getppid;
pub use crate::process::gettid;
pub use crate::process::pidfd_getfd;
pub use crate::process::pidfd_open;
pub use crate::process::pidfd_send_signal;

// ---------------------------------------------------------------------------
// Re-exports from file (I/O)
// ---------------------------------------------------------------------------

pub use crate::file::close;
pub use crate::file::read;
pub use crate::file::write;

// ---------------------------------------------------------------------------
// Re-exports from pipe
// ---------------------------------------------------------------------------

pub use crate::pipe::pipe;
pub use crate::pipe::pipe2;

// ---------------------------------------------------------------------------
// Re-exports from spawn
// ---------------------------------------------------------------------------

pub use crate::spawn::execve;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_getpid() {
        let pid = getpid();
        // On bare-metal stub, syscall returns -1 (cast to PidT).
        // On real OS, returns positive.
        let _ = pid;
    }

    #[test]
    fn test_getuid() {
        // On Windows test host, uid is 0 (stub).
        let _uid = getuid();
    }

    #[test]
    fn test_gettid() {
        let tid = gettid();
        // On stub, may return -1. On real OS, returns positive.
        let _ = tid;
    }

    #[test]
    fn test_cross_module() {
        // Verify re-exports are the same functions.
        let p1 = getpid as *const ();
        let p2 = crate::process::getpid as *const ();
        assert_eq!(p1, p2);
    }
}
