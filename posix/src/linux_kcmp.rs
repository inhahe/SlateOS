//! `<linux/kcmp.h>` — kernel comparison of two processes.
//!
//! Re-exports `kcmp()` and `KCMP_*` constants from `process`.

pub use crate::process::KCMP_EPOLL_TFD;
pub use crate::process::KCMP_FILE;
pub use crate::process::KCMP_FILES;
pub use crate::process::KCMP_FS;
pub use crate::process::KCMP_IO;
pub use crate::process::KCMP_SIGHAND;
pub use crate::process::KCMP_SYSVSEM;
pub use crate::process::KCMP_VM;
pub use crate::process::kcmp;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kcmp_types_sequential() {
        assert_eq!(KCMP_FILE, 0);
        assert_eq!(KCMP_VM, 1);
        assert_eq!(KCMP_FILES, 2);
        assert_eq!(KCMP_FS, 3);
        assert_eq!(KCMP_SIGHAND, 4);
        assert_eq!(KCMP_IO, 5);
        assert_eq!(KCMP_SYSVSEM, 6);
        assert_eq!(KCMP_EPOLL_TFD, 7);
    }

    #[test]
    fn test_kcmp_stub() {
        let ret = kcmp(1, 2, KCMP_FILE, 0, 0);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(KCMP_FILE, crate::process::KCMP_FILE);
        assert_eq!(KCMP_EPOLL_TFD, crate::process::KCMP_EPOLL_TFD);
    }
}
