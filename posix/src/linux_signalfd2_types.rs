//! `<sys/signalfd.h>` — Signalfd flag and info constants.
//!
//! Signalfd allows receiving signals via a file descriptor
//! instead of signal handlers. Each read returns a `signalfd_siginfo`
//! structure describing the signal.

// ---------------------------------------------------------------------------
// signalfd flags
// ---------------------------------------------------------------------------

/// Set close-on-exec flag.
pub const SFD_CLOEXEC: u32 = 0o2000000;
/// Set non-blocking I/O.
pub const SFD_NONBLOCK: u32 = 0o4000;

// ---------------------------------------------------------------------------
// signalfd_siginfo field sizes
// ---------------------------------------------------------------------------

/// Size of signalfd_siginfo structure (128 bytes).
pub const SFD_SIGINFO_SIZE: u32 = 128;

// ---------------------------------------------------------------------------
// signalfd_siginfo field offsets (for manual parsing)
// ---------------------------------------------------------------------------

/// Offset of ssi_signo (signal number, u32).
pub const SFD_SIGINFO_OFF_SIGNO: u32 = 0;
/// Offset of ssi_errno (errno value, i32).
pub const SFD_SIGINFO_OFF_ERRNO: u32 = 4;
/// Offset of ssi_code (signal code, i32).
pub const SFD_SIGINFO_OFF_CODE: u32 = 8;
/// Offset of ssi_pid (sender PID, u32).
pub const SFD_SIGINFO_OFF_PID: u32 = 12;
/// Offset of ssi_uid (sender UID, u32).
pub const SFD_SIGINFO_OFF_UID: u32 = 16;
/// Offset of ssi_fd (file descriptor, i32, for SIGIO).
pub const SFD_SIGINFO_OFF_FD: u32 = 20;
/// Offset of ssi_tid (timer ID, u32).
pub const SFD_SIGINFO_OFF_TID: u32 = 24;
/// Offset of ssi_band (band event, u32, for SIGPOLL).
pub const SFD_SIGINFO_OFF_BAND: u32 = 28;
/// Offset of ssi_overrun (timer overrun count, u32).
pub const SFD_SIGINFO_OFF_OVERRUN: u32 = 32;
/// Offset of ssi_status (exit status, i32, for SIGCHLD).
pub const SFD_SIGINFO_OFF_STATUS: u32 = 48;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_distinct() {
        assert_ne!(SFD_CLOEXEC, SFD_NONBLOCK);
    }

    #[test]
    fn test_cloexec() {
        assert_eq!(SFD_CLOEXEC, 0o2000000);
    }

    #[test]
    fn test_nonblock() {
        assert_eq!(SFD_NONBLOCK, 0o4000);
    }

    #[test]
    fn test_siginfo_size() {
        assert_eq!(SFD_SIGINFO_SIZE, 128);
    }

    #[test]
    fn test_offsets_ascending() {
        let offsets = [
            SFD_SIGINFO_OFF_SIGNO, SFD_SIGINFO_OFF_ERRNO,
            SFD_SIGINFO_OFF_CODE, SFD_SIGINFO_OFF_PID,
            SFD_SIGINFO_OFF_UID, SFD_SIGINFO_OFF_FD,
            SFD_SIGINFO_OFF_TID, SFD_SIGINFO_OFF_BAND,
            SFD_SIGINFO_OFF_OVERRUN,
        ];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_offsets_within_struct() {
        assert!(SFD_SIGINFO_OFF_STATUS < SFD_SIGINFO_SIZE);
    }
}
