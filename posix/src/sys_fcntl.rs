//! `<sys/fcntl.h>` — file control re-exports.
//!
//! Re-exports open flags, file control constants, and `fcntl()`
//! from the `fcntl` module.  Some programs include `<sys/fcntl.h>`
//! instead of `<fcntl.h>`.

// ---------------------------------------------------------------------------
// Open flags
// ---------------------------------------------------------------------------

pub use crate::fcntl::O_RDONLY;
pub use crate::fcntl::O_WRONLY;
pub use crate::fcntl::O_RDWR;
pub use crate::fcntl::O_CREAT;
pub use crate::fcntl::O_EXCL;
pub use crate::fcntl::O_TRUNC;
pub use crate::fcntl::O_APPEND;
pub use crate::fcntl::O_NONBLOCK;
pub use crate::fcntl::O_CLOEXEC;
pub use crate::fcntl::O_DIRECTORY;
pub use crate::fcntl::O_NOFOLLOW;

// ---------------------------------------------------------------------------
// fcntl commands
// ---------------------------------------------------------------------------

pub use crate::fcntl_ops::F_DUPFD;
pub use crate::fcntl_ops::F_GETFD;
pub use crate::fcntl_ops::F_SETFD;
pub use crate::fcntl_ops::F_GETFL;
pub use crate::fcntl_ops::F_SETFL;
pub use crate::fdtable::FD_CLOEXEC;

// ---------------------------------------------------------------------------
// File mode constants
// ---------------------------------------------------------------------------

pub use crate::fcntl::S_IFMT;
pub use crate::fcntl::S_IFREG;
pub use crate::fcntl::S_IFDIR;
pub use crate::fcntl::S_IFLNK;
pub use crate::fcntl::S_IRUSR;
pub use crate::fcntl::S_IWUSR;
pub use crate::fcntl::S_IXUSR;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

pub use crate::fcntl_ops::fcntl;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_flags() {
        assert_eq!(O_RDONLY, 0);
        assert_eq!(O_WRONLY, 1);
        assert_eq!(O_RDWR, 2);
    }

    #[test]
    fn test_fcntl_commands() {
        assert_ne!(F_DUPFD, F_GETFD);
        assert_ne!(F_GETFD, F_SETFD);
        assert_ne!(F_GETFL, F_SETFL);
    }

    #[test]
    fn test_fd_cloexec() {
        assert_eq!(FD_CLOEXEC, 1);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(O_CREAT, crate::fcntl::O_CREAT);
        assert_eq!(O_APPEND, crate::fcntl::O_APPEND);
        assert_eq!(F_DUPFD, crate::fcntl_ops::F_DUPFD);
    }
}
