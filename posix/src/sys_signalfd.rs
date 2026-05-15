//! `<sys/signalfd.h>` — signal notification file descriptor.
//!
//! Re-exports from the `epoll` module.

pub use crate::epoll::signalfd;
pub use crate::epoll::signalfd4;

// ---------------------------------------------------------------------------
// Signalfd flags
// ---------------------------------------------------------------------------

/// Close-on-exec flag for signalfd.
pub const SFD_CLOEXEC: i32 = 0o2_000_000;

/// Non-blocking flag for signalfd.
pub const SFD_NONBLOCK: i32 = 0o4000;

// ---------------------------------------------------------------------------
// Signalfd info structure
// ---------------------------------------------------------------------------

/// Information read from a signalfd file descriptor.
///
/// Each 128-byte read from a signalfd returns one of these.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SignalfdSiginfo {
    /// Signal number.
    pub ssi_signo: u32,
    /// Error number (unused for most signals).
    pub ssi_errno: i32,
    /// Signal code.
    pub ssi_code: i32,
    /// Sending process PID.
    pub ssi_pid: u32,
    /// Sending process UID.
    pub ssi_uid: u32,
    /// File descriptor (SIGIO).
    pub ssi_fd: i32,
    /// Kernel timer ID (POSIX timer).
    pub ssi_tid: u32,
    /// Band event (SIGPOLL).
    pub ssi_band: u32,
    /// POSIX timer overrun count.
    pub ssi_overrun: u32,
    /// Trap number.
    pub ssi_trapno: u32,
    /// Signal status/exit code.
    pub ssi_status: i32,
    /// Integer sent via sigqueue.
    pub ssi_int: i32,
    /// Pointer sent via sigqueue.
    pub ssi_ptr: u64,
    /// User CPU time consumed.
    pub ssi_utime: u64,
    /// System CPU time consumed.
    pub ssi_stime: u64,
    /// Signal-specific address.
    pub ssi_addr: u64,
    /// Lower 16 bits of ssi_addr.
    pub ssi_addr_lsb: u16,
    /// Padding to 128 bytes.
    _pad: [u8; 46],
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sfd_flags() {
        assert_ne!(SFD_CLOEXEC, 0);
        assert_ne!(SFD_NONBLOCK, 0);
        assert_ne!(SFD_CLOEXEC, SFD_NONBLOCK);
    }

    #[test]
    fn test_signalfd_stub() {
        let fd = signalfd(-1, core::ptr::null(), 0);
        assert_eq!(fd, -1);
    }

    #[test]
    fn test_signalfd4_stub() {
        let fd = signalfd4(-1, core::ptr::null(), 0);
        assert_eq!(fd, -1);
    }

    #[test]
    fn test_signalfd_siginfo_size() {
        assert_eq!(core::mem::size_of::<SignalfdSiginfo>(), 128);
    }
}
