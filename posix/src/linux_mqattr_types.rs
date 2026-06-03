//! `<mqueue.h>` — POSIX message queue attribute constants.
//!
//! POSIX message queues (`mq_open`, `mq_send`, `mq_receive`,
//! `mq_notify`) provide priority-ordered inter-process messaging.
//! These constants define attribute limits, open flags, and
//! notification types.

// ---------------------------------------------------------------------------
// mq_open() flags (oflag parameter)
// ---------------------------------------------------------------------------

/// Open for reading only.
pub const MQ_O_RDONLY: u32 = 0;
/// Open for writing only.
pub const MQ_O_WRONLY: u32 = 1;
/// Open for reading and writing.
pub const MQ_O_RDWR: u32 = 2;
/// Create queue if it does not exist.
pub const MQ_O_CREAT: u32 = 0o100;
/// Fail if queue exists (with O_CREAT).
pub const MQ_O_EXCL: u32 = 0o200;
/// Non-blocking mode.
pub const MQ_O_NONBLOCK: u32 = 0o4000;

// ---------------------------------------------------------------------------
// mq_attr limits (Linux defaults)
// ---------------------------------------------------------------------------

/// Default maximum number of messages on a queue.
pub const MQ_MAXMSG_DEFAULT: u32 = 10;
/// Default maximum message size (bytes).
pub const MQ_MSGSIZE_DEFAULT: u32 = 8192;
/// System-wide maximum for mq_maxmsg.
pub const MQ_MAXMSG_LIMIT: u32 = 65536;
/// System-wide maximum for mq_msgsize.
pub const MQ_MSGSIZE_LIMIT: u32 = 16777216; // 16 MiB
/// Maximum message priority.
pub const MQ_PRIO_MAX: u32 = 32768;

// ---------------------------------------------------------------------------
// mq_attr field offsets (struct mq_attr, Linux x86_64)
// ---------------------------------------------------------------------------

/// Offset of mq_flags in struct mq_attr.
pub const MQ_ATTR_OFF_FLAGS: u32 = 0;
/// Offset of mq_maxmsg in struct mq_attr.
pub const MQ_ATTR_OFF_MAXMSG: u32 = 8;
/// Offset of mq_msgsize in struct mq_attr.
pub const MQ_ATTR_OFF_MSGSIZE: u32 = 16;
/// Offset of mq_curmsgs in struct mq_attr.
pub const MQ_ATTR_OFF_CURMSGS: u32 = 24;
/// Size of struct mq_attr (bytes).
pub const MQ_ATTR_SIZE: u32 = 64;

// ---------------------------------------------------------------------------
// mq_notify() notification types
// ---------------------------------------------------------------------------

/// No notification.
pub const MQ_NOTIFY_NONE: u32 = 0;
/// Notify via signal.
pub const MQ_NOTIFY_SIGNAL: u32 = 1;
/// Notify via thread creation.
pub const MQ_NOTIFY_THREAD: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_modes_distinct() {
        let modes = [MQ_O_RDONLY, MQ_O_WRONLY, MQ_O_RDWR];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_rdonly_is_zero() {
        assert_eq!(MQ_O_RDONLY, 0);
    }

    #[test]
    fn test_open_flags_no_overlap_with_modes() {
        let flags = [MQ_O_CREAT, MQ_O_EXCL, MQ_O_NONBLOCK];
        for f in flags {
            assert_eq!(f & 0x3, 0); // no overlap with RDONLY/WRONLY/RDWR
        }
    }

    #[test]
    fn test_defaults() {
        assert_eq!(MQ_MAXMSG_DEFAULT, 10);
        assert_eq!(MQ_MSGSIZE_DEFAULT, 8192);
    }

    #[test]
    fn test_limits_greater_than_defaults() {
        assert!(MQ_MAXMSG_LIMIT > MQ_MAXMSG_DEFAULT);
        assert!(MQ_MSGSIZE_LIMIT > MQ_MSGSIZE_DEFAULT);
    }

    #[test]
    fn test_prio_max() {
        assert_eq!(MQ_PRIO_MAX, 32768);
    }

    #[test]
    fn test_attr_offsets_ascending() {
        let offsets = [
            MQ_ATTR_OFF_FLAGS,
            MQ_ATTR_OFF_MAXMSG,
            MQ_ATTR_OFF_MSGSIZE,
            MQ_ATTR_OFF_CURMSGS,
        ];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_attr_offsets_within_struct() {
        assert!(MQ_ATTR_OFF_CURMSGS < MQ_ATTR_SIZE);
    }

    #[test]
    fn test_notify_types_distinct() {
        let types = [MQ_NOTIFY_NONE, MQ_NOTIFY_SIGNAL, MQ_NOTIFY_THREAD];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_notify_none_is_zero() {
        assert_eq!(MQ_NOTIFY_NONE, 0);
    }
}
