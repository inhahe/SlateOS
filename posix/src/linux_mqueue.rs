//! `<linux/mqueue.h>` — POSIX message queue constants (kernel view).
//!
//! Re-exports from `mqueue` module and adds Linux-specific constants
//! for the mq_notify mechanism and default queue limits.

// ---------------------------------------------------------------------------
// Re-exports from mqueue module
// ---------------------------------------------------------------------------

pub use crate::mqueue::MqAttr;
pub use crate::mqueue::mq_open;
pub use crate::mqueue::mq_close;
pub use crate::mqueue::mq_unlink;
pub use crate::mqueue::mq_send;
pub use crate::mqueue::mq_receive;
pub use crate::mqueue::mq_getattr;
pub use crate::mqueue::mq_setattr;

// ---------------------------------------------------------------------------
// Default limits (from kernel defaults)
// ---------------------------------------------------------------------------

/// Default max messages per queue.
pub const MQ_MAXMSG_DEFAULT: i32 = 10;

/// Default max message size (bytes).
pub const MQ_MSGSIZE_DEFAULT: i32 = 8192;

/// System-wide max queues per user (RLIMIT_MSGQUEUE limit).
pub const MQ_MAXQUEUES: i32 = 256;

// ---------------------------------------------------------------------------
// Notification types (for mq_notify via sigev_notify)
// ---------------------------------------------------------------------------

/// No notification.
pub const SIGEV_NONE_: i32 = 0;
/// Signal notification.
pub const SIGEV_SIGNAL_: i32 = 1;
/// Thread notification.
pub const SIGEV_THREAD_: i32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mq_attr_size() {
        assert_eq!(core::mem::size_of::<MqAttr>(), 64);
    }

    #[test]
    fn test_defaults() {
        assert_eq!(MQ_MAXMSG_DEFAULT, 10);
        assert_eq!(MQ_MSGSIZE_DEFAULT, 8192);
        assert!(MQ_MAXQUEUES > 0);
    }

    #[test]
    fn test_sigev_types() {
        assert_eq!(SIGEV_NONE_, 0);
        assert_eq!(SIGEV_SIGNAL_, 1);
        assert_eq!(SIGEV_THREAD_, 2);
    }

    #[test]
    fn test_cross_module() {
        let _ = core::mem::size_of::<crate::mqueue::MqAttr>();
    }
}
