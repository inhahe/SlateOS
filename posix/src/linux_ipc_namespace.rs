//! Linux IPC namespace constants.
//!
//! IPC namespaces isolate System V IPC objects (message queues,
//! semaphores, shared memory) and POSIX message queues. Each
//! namespace has independent IPC ID spaces and limits.

// ---------------------------------------------------------------------------
// Clone flags
// ---------------------------------------------------------------------------

/// Create new IPC namespace.
pub const CLONE_NEWIPC: u64 = 0x08000000;

// ---------------------------------------------------------------------------
// /proc interface
// ---------------------------------------------------------------------------

/// IPC namespace proc link.
pub const PROC_NS_IPC: &str = "ns/ipc";

// ---------------------------------------------------------------------------
// Default IPC limits (per-namespace, from kernel defaults)
// ---------------------------------------------------------------------------

/// Default maximum shared memory segment size (SHMMAX, bytes).
pub const IPC_NS_SHMMAX_DEFAULT: u64 = 32 * 1024 * 1024;
/// Default maximum total shared memory (SHMALL, pages).
pub const IPC_NS_SHMALL_DEFAULT: u64 = 2 * 1024 * 1024;
/// Default maximum number of shared memory segments (SHMMNI).
pub const IPC_NS_SHMMNI_DEFAULT: u32 = 4096;
/// Default maximum number of semaphore sets (SEMMNI).
pub const IPC_NS_SEMMNI_DEFAULT: u32 = 32000;
/// Default maximum semaphores per set (SEMMSL).
pub const IPC_NS_SEMMSL_DEFAULT: u32 = 32000;
/// Default maximum number of message queues (MSGMNI).
pub const IPC_NS_MSGMNI_DEFAULT: u32 = 32000;
/// Default maximum message size (MSGMAX, bytes).
pub const IPC_NS_MSGMAX_DEFAULT: u32 = 8192;
/// Default maximum message queue size (MSGMNB, bytes).
pub const IPC_NS_MSGMNB_DEFAULT: u32 = 16384;

// ---------------------------------------------------------------------------
// Sysctl paths
// ---------------------------------------------------------------------------

/// Shared memory max size sysctl.
pub const SYSCTL_SHMMAX: &str = "kernel.shmmax";
/// Total shared memory sysctl.
pub const SYSCTL_SHMALL: &str = "kernel.shmall";
/// Shared memory segment count sysctl.
pub const SYSCTL_SHMMNI: &str = "kernel.shmmni";
/// Message queue count sysctl.
pub const SYSCTL_MSGMNI: &str = "kernel.msgmni";
/// Message max size sysctl.
pub const SYSCTL_MSGMAX: &str = "kernel.msgmax";
/// Semaphore set count sysctl.
pub const SYSCTL_SEM: &str = "kernel.sem";

// ---------------------------------------------------------------------------
// POSIX message queue limits
// ---------------------------------------------------------------------------

/// Default maximum POSIX message queues per namespace.
pub const POSIX_MQ_QUEUES_MAX: u32 = 256;
/// Default maximum message size for POSIX mqueues.
pub const POSIX_MQ_MSGSIZE_MAX: u32 = 8192;
/// Default maximum messages per POSIX queue.
pub const POSIX_MQ_MSG_MAX: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_newipc() {
        assert_eq!(CLONE_NEWIPC, 0x08000000);
        assert!((CLONE_NEWIPC as u64).is_power_of_two());
    }

    #[test]
    fn test_clone_flags_distinct() {
        let other_ns_flags: &[u64] = &[
            0x10000000, // CLONE_NEWUSER
            0x20000000, // CLONE_NEWPID
            0x00020000, // CLONE_NEWNS
            0x40000000, // CLONE_NEWNET
        ];
        for flag in other_ns_flags {
            assert_ne!(CLONE_NEWIPC, *flag);
        }
    }

    #[test]
    fn test_proc_path() {
        assert_eq!(PROC_NS_IPC, "ns/ipc");
    }

    #[test]
    fn test_shm_defaults() {
        assert_eq!(IPC_NS_SHMMAX_DEFAULT, 32 * 1024 * 1024);
        assert!(IPC_NS_SHMMNI_DEFAULT > 0);
    }

    #[test]
    fn test_msg_defaults() {
        assert!(IPC_NS_MSGMAX_DEFAULT > 0);
        assert!(IPC_NS_MSGMNB_DEFAULT >= IPC_NS_MSGMAX_DEFAULT);
        assert!(IPC_NS_MSGMNI_DEFAULT > 0);
    }

    #[test]
    fn test_sysctl_paths_distinct() {
        let paths = [
            SYSCTL_SHMMAX,
            SYSCTL_SHMALL,
            SYSCTL_SHMMNI,
            SYSCTL_MSGMNI,
            SYSCTL_MSGMAX,
            SYSCTL_SEM,
        ];
        for i in 0..paths.len() {
            for j in (i + 1)..paths.len() {
                assert_ne!(paths[i], paths[j]);
            }
        }
    }

    #[test]
    fn test_posix_mq_limits() {
        assert!(POSIX_MQ_QUEUES_MAX > 0);
        assert!(POSIX_MQ_MSGSIZE_MAX > 0);
        assert!(POSIX_MQ_MSG_MAX > 0);
    }
}
