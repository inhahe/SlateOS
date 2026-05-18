//! `<linux/mqueue.h>` — Additional POSIX message queue constants.
//!
//! Supplementary mqueue constants covering priorities,
//! limits, notify methods, and ioctl commands.

// ---------------------------------------------------------------------------
// Mqueue limits
// ---------------------------------------------------------------------------

/// Default max messages.
pub const MQ_MAXMSG_DEFAULT: u32 = 10;
/// Default max message size.
pub const MQ_MSGSIZE_DEFAULT: u32 = 8192;
/// Hard limit max messages.
pub const MQ_MAXMSG_LIMIT: u32 = 65536;
/// Hard limit max message size.
pub const MQ_MSGSIZE_LIMIT: u32 = 16_777_216;
/// Min message size.
pub const MQ_MSGSIZE_MIN: u32 = 1;
/// Max priority.
pub const MQ_PRIO_MAX: u32 = 32768;

// ---------------------------------------------------------------------------
// Mqueue open flags
// ---------------------------------------------------------------------------

/// Read only.
pub const O_RDONLY_MQ: u32 = 0x0000;
/// Write only.
pub const O_WRONLY_MQ: u32 = 0x0001;
/// Read/write.
pub const O_RDWR_MQ: u32 = 0x0002;
/// Non-blocking.
pub const O_NONBLOCK_MQ: u32 = 0x0800;
/// Create.
pub const O_CREAT_MQ: u32 = 0x0040;
/// Exclusive create.
pub const O_EXCL_MQ: u32 = 0x0080;

// ---------------------------------------------------------------------------
// Mqueue notification methods
// ---------------------------------------------------------------------------

/// No notification.
pub const SIGEV_NONE: u32 = 0;
/// Signal notification.
pub const SIGEV_SIGNAL: u32 = 1;
/// Thread notification.
pub const SIGEV_THREAD: u32 = 2;
/// Thread ID notification (Linux extension).
pub const SIGEV_THREAD_ID: u32 = 4;

// ---------------------------------------------------------------------------
// Mqueue sysfs paths (string lengths)
// ---------------------------------------------------------------------------

/// Maximum length of mqueue name.
pub const MQ_NAME_MAX: u32 = 255;
/// Mount point path component.
pub const MQ_MOUNT_POINT_LEN: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_limits() {
        assert!(MQ_MAXMSG_DEFAULT <= MQ_MAXMSG_LIMIT);
        assert!(MQ_MSGSIZE_DEFAULT <= MQ_MSGSIZE_LIMIT);
        assert!(MQ_MSGSIZE_MIN <= MQ_MSGSIZE_DEFAULT);
    }

    #[test]
    fn test_prio_max() {
        assert_eq!(MQ_PRIO_MAX, 32768);
    }

    #[test]
    fn test_open_flags_distinct() {
        let flags = [
            O_RDONLY_MQ, O_WRONLY_MQ, O_RDWR_MQ,
            O_NONBLOCK_MQ, O_CREAT_MQ, O_EXCL_MQ,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_sigev_distinct() {
        let methods = [SIGEV_NONE, SIGEV_SIGNAL, SIGEV_THREAD, SIGEV_THREAD_ID];
        for i in 0..methods.len() {
            for j in (i + 1)..methods.len() {
                assert_ne!(methods[i], methods[j]);
            }
        }
    }

    #[test]
    fn test_name_max() {
        assert_eq!(MQ_NAME_MAX, 255);
    }
}
