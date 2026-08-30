//! `<linux/iopoll.h>` — I/O polling constants.
//!
//! The kernel's IO polling infrastructure allows busy-waiting on
//! I/O completion for ultra-low-latency workloads. Used by block
//! drivers (NVMe, io_uring with IOPOLL) and network drivers.

// ---------------------------------------------------------------------------
// Poll state constants
// ---------------------------------------------------------------------------

/// No poll activity.
pub const BLK_POLL_NOSLEEP: i32 = 0;
/// Adaptive polling.
pub const BLK_POLL_ADAPTIVE: i32 = 1;
/// Classic polling (busy wait).
pub const BLK_POLL_CLASSIC: i32 = 2;

// ---------------------------------------------------------------------------
// IO poll flags
// ---------------------------------------------------------------------------

/// May sleep during poll.
pub const BIO_POLL_F_MAY_SLEEP: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Sysfs poll mode values (written to /sys/block/<dev>/queue/io_poll)
// ---------------------------------------------------------------------------

/// Polling disabled.
pub const QUEUE_POLL_DISABLE: i32 = 0;
/// Polling enabled.
pub const QUEUE_POLL_ENABLE: i32 = 1;

// ---------------------------------------------------------------------------
// io_uring poll flags (IORING_POLL_*)
// ---------------------------------------------------------------------------

/// Add poll to multishot.
pub const IORING_POLL_ADD_MULTI: u32 = 1 << 0;
/// Update existing poll.
pub const IORING_POLL_UPDATE_EVENTS: u32 = 1 << 1;
/// Update user data.
pub const IORING_POLL_UPDATE_USER_DATA: u32 = 1 << 2;
/// Level triggered.
pub const IORING_POLL_LEVEL: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_poll_modes_distinct() {
        let modes = [BLK_POLL_NOSLEEP, BLK_POLL_ADAPTIVE, BLK_POLL_CLASSIC];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_queue_poll_values() {
        assert_eq!(QUEUE_POLL_DISABLE, 0);
        assert_eq!(QUEUE_POLL_ENABLE, 1);
    }

    #[test]
    fn test_ioring_poll_flags_powers_of_two() {
        let flags = [
            IORING_POLL_ADD_MULTI,
            IORING_POLL_UPDATE_EVENTS,
            IORING_POLL_UPDATE_USER_DATA,
            IORING_POLL_LEVEL,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two());
        }
    }

    #[test]
    fn test_bio_poll_flag() {
        assert!(BIO_POLL_F_MAY_SLEEP.is_power_of_two());
    }
}
