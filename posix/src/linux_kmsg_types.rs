//! `/dev/kmsg` — Kernel message device constants.
//!
//! `/dev/kmsg` provides structured access to the kernel ring
//! buffer.  Reading yields individual records with metadata;
//! writing injects messages.  These constants define the
//! record format and seek modes.

// ---------------------------------------------------------------------------
// /dev/kmsg record fields
// ---------------------------------------------------------------------------

/// Priority field: log level + facility.
pub const KMSG_PRIORITY_MASK: u32 = 0x07;
/// Facility shift (priority >> 3 = facility).
pub const KMSG_FACILITY_SHIFT: u32 = 3;
/// Facility mask (after shifting).
pub const KMSG_FACILITY_MASK: u32 = 0xFFF8;

// ---------------------------------------------------------------------------
// /dev/kmsg seek commands
// ---------------------------------------------------------------------------

/// Seek to the beginning of the log (SEEK_DATA).
pub const KMSG_SEEK_DATA: u32 = 3;
/// Seek to the end of the log (SEEK_END).
pub const KMSG_SEEK_END: u32 = 2;
/// Seek to the current read position (SEEK_SET).
pub const KMSG_SEEK_SET: u32 = 0;

// ---------------------------------------------------------------------------
// /dev/kmsg open flags
// ---------------------------------------------------------------------------

/// Open for reading (non-destructive, unlike syslog syscall).
pub const KMSG_O_RDONLY: u32 = 0;
/// Open for writing (inject messages).
pub const KMSG_O_WRONLY: u32 = 1;
/// Open for reading and writing.
pub const KMSG_O_RDWR: u32 = 2;
/// Non-blocking reads.
pub const KMSG_O_NONBLOCK: u32 = 0o4000;

// ---------------------------------------------------------------------------
// Syslog facility codes (used in priority encoding)
// ---------------------------------------------------------------------------

/// Kernel messages.
pub const LOG_KERN: u32 = 0 << 3;
/// User-level messages.
pub const LOG_USER: u32 = 1 << 3;
/// Mail system.
pub const LOG_MAIL: u32 = 2 << 3;
/// System daemons.
pub const LOG_DAEMON: u32 = 3 << 3;
/// Security/authorization messages.
pub const LOG_AUTH: u32 = 4 << 3;
/// Internal syslog messages.
pub const LOG_SYSLOG: u32 = 5 << 3;
/// Line printer subsystem.
pub const LOG_LPR: u32 = 6 << 3;
/// Network news subsystem.
pub const LOG_NEWS: u32 = 7 << 3;
/// UUCP subsystem.
pub const LOG_UUCP: u32 = 8 << 3;
/// Clock daemon (cron).
pub const LOG_CRON: u32 = 9 << 3;
/// Local use 0.
pub const LOG_LOCAL0: u32 = 16 << 3;
/// Local use 7.
pub const LOG_LOCAL7: u32 = 23 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_mask() {
        assert_eq!(KMSG_PRIORITY_MASK, 7);
    }

    #[test]
    fn test_facility_shift() {
        assert_eq!(KMSG_FACILITY_SHIFT, 3);
    }

    #[test]
    fn test_seek_commands_distinct() {
        let seeks = [KMSG_SEEK_DATA, KMSG_SEEK_END, KMSG_SEEK_SET];
        for i in 0..seeks.len() {
            for j in (i + 1)..seeks.len() {
                assert_ne!(seeks[i], seeks[j]);
            }
        }
    }

    #[test]
    fn test_seek_set_is_zero() {
        assert_eq!(KMSG_SEEK_SET, 0);
    }

    #[test]
    fn test_open_modes_distinct() {
        let modes = [KMSG_O_RDONLY, KMSG_O_WRONLY, KMSG_O_RDWR];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_log_kern_is_zero() {
        assert_eq!(LOG_KERN, 0);
    }

    #[test]
    fn test_log_user() {
        assert_eq!(LOG_USER, 8);
    }

    #[test]
    fn test_facilities_distinct() {
        let facs = [
            LOG_KERN, LOG_USER, LOG_MAIL, LOG_DAEMON,
            LOG_AUTH, LOG_SYSLOG, LOG_LPR, LOG_NEWS,
            LOG_UUCP, LOG_CRON, LOG_LOCAL0, LOG_LOCAL7,
        ];
        for i in 0..facs.len() {
            for j in (i + 1)..facs.len() {
                assert_ne!(facs[i], facs[j]);
            }
        }
    }
}
