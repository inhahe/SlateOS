//! `<linux/signalfd.h>` — signal file descriptor (kernel view).
//!
//! Re-exports from `sys_signalfd` (the POSIX-facing header).

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use crate::epoll::signalfd;
pub use crate::epoll::signalfd4;
pub use crate::sys_signalfd::SFD_CLOEXEC;
pub use crate::sys_signalfd::SFD_NONBLOCK;
pub use crate::sys_signalfd::SignalfdSiginfo;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sfd_flags() {
        assert_ne!(SFD_CLOEXEC, SFD_NONBLOCK);
        assert_ne!(SFD_CLOEXEC, 0);
        assert_ne!(SFD_NONBLOCK, 0);
    }

    #[test]
    fn test_siginfo_size() {
        assert_eq!(core::mem::size_of::<SignalfdSiginfo>(), 128);
    }

    #[test]
    fn test_signalfd_stub() {
        let fd = signalfd(-1, core::ptr::null(), 0);
        assert_eq!(fd, -1);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(SFD_CLOEXEC, crate::sys_signalfd::SFD_CLOEXEC);
        assert_eq!(SFD_NONBLOCK, crate::sys_signalfd::SFD_NONBLOCK);
    }
}
