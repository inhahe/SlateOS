//! `<mqueue.h>` — POSIX message queue ABI.
//!
//! POSIX mqueues are a IPC mechanism with priorities and select-able
//! notifications. Real-time and embedded software favors them (vs.
//! SysV IPC or pipes) because of the priority ordering and the fact
//! that the mqueue fd integrates with `poll`/`epoll`. The kernel
//! exposes the queues via `mqueuefs`, conventionally mounted on
//! `/dev/mqueue`.

// ---------------------------------------------------------------------------
// Open flags reused from `<fcntl.h>`
// ---------------------------------------------------------------------------

pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;
pub const O_CREAT: u32 = 0o100;
pub const O_EXCL: u32 = 0o200;
pub const O_NONBLOCK: u32 = 0o4000;
pub const O_CLOEXEC: u32 = 0o2_000_000;

// ---------------------------------------------------------------------------
// Notification mechanism (`struct sigevent.sigev_notify`)
// ---------------------------------------------------------------------------

pub const SIGEV_SIGNAL: u32 = 0;
pub const SIGEV_NONE: u32 = 1;
pub const SIGEV_THREAD: u32 = 2;
pub const SIGEV_THREAD_ID: u32 = 4;

// ---------------------------------------------------------------------------
// Sysctl-tunable kernel defaults
// ---------------------------------------------------------------------------

/// `/proc/sys/fs/mqueue/msg_default` default (10 messages per queue).
pub const MQUEUE_MSG_DEFAULT: u32 = 10;
/// `/proc/sys/fs/mqueue/msgsize_default` default (8 KiB).
pub const MQUEUE_MSGSIZE_DEFAULT: u32 = 8 * 1024;
/// `/proc/sys/fs/mqueue/msg_max` default upper bound.
pub const MQUEUE_MSG_MAX: u32 = 10;
/// `/proc/sys/fs/mqueue/msgsize_max` default upper bound (8 KiB).
pub const MQUEUE_MSGSIZE_MAX: u32 = 8 * 1024;
/// `/proc/sys/fs/mqueue/queues_max` default — total queues per user.
pub const MQUEUE_QUEUES_MAX: u32 = 256;

// ---------------------------------------------------------------------------
// Message priority limits
// ---------------------------------------------------------------------------

/// `MQ_PRIO_MAX` from POSIX.
pub const MQ_PRIO_MAX: u32 = 32_768;

// ---------------------------------------------------------------------------
// Syscall numbers (x86_64)
// ---------------------------------------------------------------------------

pub const NR_MQ_OPEN: u32 = 240;
pub const NR_MQ_UNLINK: u32 = 241;
pub const NR_MQ_TIMEDSEND: u32 = 242;
pub const NR_MQ_TIMEDRECEIVE: u32 = 243;
pub const NR_MQ_NOTIFY: u32 = 244;
pub const NR_MQ_GETSETATTR: u32 = 245;

// ---------------------------------------------------------------------------
// Conventional mount point
// ---------------------------------------------------------------------------

pub const MQUEUE_MOUNT_POINT: &str = "/dev/mqueue";
pub const MQUEUEFS_NAME: &str = "mqueue";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_modes_dense_0_to_2() {
        assert_eq!(O_RDONLY, 0);
        assert_eq!(O_WRONLY, 1);
        assert_eq!(O_RDWR, 2);
    }

    #[test]
    fn test_sigev_notify_values() {
        // SIGEV_SIGNAL/NONE/THREAD dense at 0..2, THREAD_ID separate.
        assert_eq!(SIGEV_SIGNAL, 0);
        assert_eq!(SIGEV_NONE, 1);
        assert_eq!(SIGEV_THREAD, 2);
        assert_eq!(SIGEV_THREAD_ID, 4);
        // The four values are distinct.
        let s = [SIGEV_SIGNAL, SIGEV_NONE, SIGEV_THREAD, SIGEV_THREAD_ID];
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
        }
    }

    #[test]
    fn test_kernel_defaults_consistent() {
        // The "default" and "max" kernel sysctls agree by default — admin
        // raises max before raising default.
        assert_eq!(MQUEUE_MSG_DEFAULT, MQUEUE_MSG_MAX);
        assert_eq!(MQUEUE_MSGSIZE_DEFAULT, MQUEUE_MSGSIZE_MAX);
        assert_eq!(MQUEUE_MSGSIZE_MAX, 8 * 1024);
    }

    #[test]
    fn test_mq_prio_max() {
        // POSIX requires MQ_PRIO_MAX >= 32; Linux uses 32768.
        assert_eq!(MQ_PRIO_MAX, 32_768);
        assert!(MQ_PRIO_MAX.is_power_of_two());
    }

    #[test]
    fn test_syscalls_dense_240_to_245() {
        let n = [
            NR_MQ_OPEN,
            NR_MQ_UNLINK,
            NR_MQ_TIMEDSEND,
            NR_MQ_TIMEDRECEIVE,
            NR_MQ_NOTIFY,
            NR_MQ_GETSETATTR,
        ];
        for (i, &v) in n.iter().enumerate() {
            assert_eq!(v, 240 + i as u32);
        }
    }

    #[test]
    fn test_mount_point_strings() {
        assert_eq!(MQUEUE_MOUNT_POINT, "/dev/mqueue");
        assert_eq!(MQUEUEFS_NAME, "mqueue");
    }
}
