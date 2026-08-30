//! `<linux/signalfd.h>` — Additional signalfd constants (batch 3).
//!
//! Supplementary signalfd constants covering create flags,
//! signal info fields, and signal set operations.

// ---------------------------------------------------------------------------
// Signalfd create flags
// ---------------------------------------------------------------------------

/// Set close-on-exec on the signalfd.
pub const SFD_CLOEXEC: u32 = 0o2000000;
/// Set non-blocking on the signalfd.
pub const SFD_NONBLOCK: u32 = 0o4000;

// ---------------------------------------------------------------------------
// Signalfd siginfo structure field offsets
// ---------------------------------------------------------------------------

/// Offset: signal number (ssi_signo).
pub const SFD_SIGNO_OFFSET: u32 = 0;
/// Offset: error number (ssi_errno).
pub const SFD_ERRNO_OFFSET: u32 = 4;
/// Offset: signal code (ssi_code).
pub const SFD_CODE_OFFSET: u32 = 8;
/// Offset: sending PID (ssi_pid).
pub const SFD_PID_OFFSET: u32 = 12;
/// Offset: sending UID (ssi_uid).
pub const SFD_UID_OFFSET: u32 = 16;
/// Offset: file descriptor (ssi_fd).
pub const SFD_FD_OFFSET: u32 = 20;
/// Offset: timer ID (ssi_tid).
pub const SFD_TID_OFFSET: u32 = 24;
/// Offset: band event (ssi_band).
pub const SFD_BAND_OFFSET: u32 = 28;
/// Offset: overrun count (ssi_overrun).
pub const SFD_OVERRUN_OFFSET: u32 = 32;
/// Offset: trap number (ssi_trapno).
pub const SFD_TRAPNO_OFFSET: u32 = 36;
/// Offset: exit status (ssi_status).
pub const SFD_STATUS_OFFSET: u32 = 40;
/// Offset: int value (ssi_int).
pub const SFD_INT_OFFSET: u32 = 44;
/// Offset: pointer value (ssi_ptr).
pub const SFD_PTR_OFFSET: u32 = 48;
/// Offset: user time (ssi_utime).
pub const SFD_UTIME_OFFSET: u32 = 56;
/// Offset: system time (ssi_stime).
pub const SFD_STIME_OFFSET: u32 = 64;
/// Offset: address (ssi_addr).
pub const SFD_ADDR_OFFSET: u32 = 72;

/// Size of signalfd_siginfo structure.
pub const SFD_SIGINFO_SIZE: u32 = 128;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_flags_distinct() {
        assert_ne!(SFD_CLOEXEC, SFD_NONBLOCK);
    }

    #[test]
    fn test_offsets_increasing() {
        let offsets = [
            SFD_SIGNO_OFFSET,
            SFD_ERRNO_OFFSET,
            SFD_CODE_OFFSET,
            SFD_PID_OFFSET,
            SFD_UID_OFFSET,
            SFD_FD_OFFSET,
            SFD_TID_OFFSET,
            SFD_BAND_OFFSET,
            SFD_OVERRUN_OFFSET,
            SFD_TRAPNO_OFFSET,
            SFD_STATUS_OFFSET,
            SFD_INT_OFFSET,
            SFD_PTR_OFFSET,
            SFD_UTIME_OFFSET,
            SFD_STIME_OFFSET,
            SFD_ADDR_OFFSET,
        ];
        for i in 1..offsets.len() {
            assert!(
                offsets[i] > offsets[i - 1],
                "offset {} ({}) not > offset {} ({})",
                i,
                offsets[i],
                i - 1,
                offsets[i - 1]
            );
        }
    }

    #[test]
    fn test_siginfo_size() {
        assert_eq!(SFD_SIGINFO_SIZE, 128);
    }

    #[test]
    fn test_addr_fits_in_struct() {
        // Last field (addr at offset 72) + 8 bytes < 128
        assert!(SFD_ADDR_OFFSET + 8 <= SFD_SIGINFO_SIZE);
    }

    #[test]
    fn test_signo_starts_at_zero() {
        assert_eq!(SFD_SIGNO_OFFSET, 0);
    }
}
