//! `<linux/mqueue.h>` — POSIX message queue constants.
//!
//! POSIX message queues provide a message-passing IPC mechanism
//! with priority ordering. Unlike System V message queues, POSIX
//! mqueues appear in a virtual filesystem (/dev/mqueue), support
//! notification via signals or threads when messages arrive, and
//! have cleaner semantics. Messages are received in priority order
//! (highest priority first). Each queue has configurable maximum
//! message count and maximum message size.

// ---------------------------------------------------------------------------
// POSIX message queue default limits
// ---------------------------------------------------------------------------

/// Default maximum number of messages in a queue.
pub const MQ_MAXMSG_DEFAULT: u32 = 10;
/// Default maximum message size in bytes.
pub const MQ_MSGSIZE_DEFAULT: u32 = 8192;
/// Hard upper limit for maximum messages (sysctl limit).
pub const MQ_MAXMSG_HARD: u32 = 65536;
/// Hard upper limit for message size (sysctl limit).
pub const MQ_MSGSIZE_HARD: u32 = 16_777_216;
/// Default maximum number of queues system-wide.
pub const MQ_QUEUES_MAX_DEFAULT: u32 = 256;

// ---------------------------------------------------------------------------
// Message queue open flags (in addition to O_RDONLY/O_WRONLY/O_RDWR)
// ---------------------------------------------------------------------------

/// Create queue if it doesn't exist.
pub const MQ_OFLAG_CREAT: u32 = 0o100;
/// Fail if queue exists (with O_CREAT).
pub const MQ_OFLAG_EXCL: u32 = 0o200;
/// Non-blocking mode.
pub const MQ_OFLAG_NONBLOCK: u32 = 0o4000;
/// Close-on-exec.
pub const MQ_OFLAG_CLOEXEC: u32 = 0o2000000;

// ---------------------------------------------------------------------------
// Message queue notification types (for mq_notify)
// ---------------------------------------------------------------------------

/// No notification.
pub const MQ_NOTIFY_NONE: u32 = 0;
/// Notify via signal.
pub const MQ_NOTIFY_SIGNAL: u32 = 1;
/// Notify via thread creation.
pub const MQ_NOTIFY_THREAD: u32 = 2;

// ---------------------------------------------------------------------------
// Message priority limits
// ---------------------------------------------------------------------------

/// Minimum message priority.
pub const MQ_PRIO_MIN: u32 = 0;
/// Maximum message priority (POSIX says at least 31).
pub const MQ_PRIO_MAX: u32 = 32768;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_limits() {
        assert!(MQ_MAXMSG_DEFAULT > 0);
        assert!(MQ_MSGSIZE_DEFAULT > 0);
        assert!(MQ_MAXMSG_HARD >= MQ_MAXMSG_DEFAULT);
        assert!(MQ_MSGSIZE_HARD >= MQ_MSGSIZE_DEFAULT);
        assert!(MQ_QUEUES_MAX_DEFAULT > 0);
    }

    #[test]
    fn test_notification_types_distinct() {
        let types = [MQ_NOTIFY_NONE, MQ_NOTIFY_SIGNAL, MQ_NOTIFY_THREAD];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_priority_range() {
        assert!(MQ_PRIO_MAX > MQ_PRIO_MIN);
    }

    #[test]
    fn test_open_flags_distinct() {
        let flags = [
            MQ_OFLAG_CREAT,
            MQ_OFLAG_EXCL,
            MQ_OFLAG_NONBLOCK,
            MQ_OFLAG_CLOEXEC,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
