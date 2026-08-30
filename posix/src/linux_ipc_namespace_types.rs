//! `<linux/ipc_namespace.h>` — IPC namespace constants.
//!
//! IPC namespaces isolate System V IPC objects (semaphores, message
//! queues, shared memory) and POSIX message queues. Each IPC
//! namespace has its own set of IPC identifiers and keys, so
//! processes in different namespaces cannot see or interfere with
//! each other's IPC resources. Containers use IPC namespaces to
//! prevent IPC-based information leaks between containers.

// ---------------------------------------------------------------------------
// IPC namespace clone flag
// ---------------------------------------------------------------------------

/// Clone flag for creating a new IPC namespace.
pub const CLONE_NEWIPC: u32 = 0x0800_0000;

// ---------------------------------------------------------------------------
// IPC namespace resource limits (defaults)
// ---------------------------------------------------------------------------

/// Default maximum number of System V message queues per namespace.
pub const IPCNS_MSGMNI_DEFAULT: u32 = 32000;
/// Default maximum message size in bytes (8 KiB).
pub const IPCNS_MSGMAX_DEFAULT: u32 = 8192;
/// Default maximum total bytes in a message queue (16 KiB).
pub const IPCNS_MSGMNB_DEFAULT: u32 = 16384;
/// Default maximum number of System V semaphore sets.
pub const IPCNS_SEMMNI_DEFAULT: u32 = 32000;
/// Default maximum number of semaphores per set.
pub const IPCNS_SEMMSL_DEFAULT: u32 = 32000;
/// Default maximum total semaphore operations per semop call.
pub const IPCNS_SEMOPM_DEFAULT: u32 = 500;
/// Default maximum number of shared memory segments.
pub const IPCNS_SHMMNI_DEFAULT: u32 = 4096;

// ---------------------------------------------------------------------------
// IPC namespace states
// ---------------------------------------------------------------------------

/// Namespace is active.
pub const IPCNS_STATE_ACTIVE: u32 = 0;
/// Namespace is being destroyed (cleaning up IPC objects).
pub const IPCNS_STATE_DYING: u32 = 1;

// ---------------------------------------------------------------------------
// IPC key special values
// ---------------------------------------------------------------------------

/// Private key (create new unique IPC object).
pub const IPC_PRIVATE: u32 = 0;

// ---------------------------------------------------------------------------
// IPC command flags (shared by semctl, msgctl, shmctl)
// ---------------------------------------------------------------------------

/// Create IPC object if it doesn't exist.
pub const IPC_CREAT: u32 = 0o001000;
/// Fail if IPC object already exists (with IPC_CREAT).
pub const IPC_EXCL: u32 = 0o002000;
/// Don't wait (return error instead of blocking).
pub const IPC_NOWAIT: u32 = 0o004000;

// ---------------------------------------------------------------------------
// IPC ctl commands
// ---------------------------------------------------------------------------

/// Remove an IPC object.
pub const IPC_RMID: u32 = 0;
/// Set IPC object attributes.
pub const IPC_SET: u32 = 1;
/// Get IPC object status.
pub const IPC_STAT: u32 = 2;
/// Get IPC info (system-wide limits).
pub const IPC_INFO: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_flag() {
        assert!(CLONE_NEWIPC.is_power_of_two());
    }

    #[test]
    fn test_defaults_positive() {
        assert!(IPCNS_MSGMNI_DEFAULT > 0);
        assert!(IPCNS_MSGMAX_DEFAULT > 0);
        assert!(IPCNS_MSGMNB_DEFAULT > 0);
        assert!(IPCNS_SEMMNI_DEFAULT > 0);
        assert!(IPCNS_SEMMSL_DEFAULT > 0);
        assert!(IPCNS_SEMOPM_DEFAULT > 0);
        assert!(IPCNS_SHMMNI_DEFAULT > 0);
    }

    #[test]
    fn test_ipc_flags_no_overlap() {
        let flags = [IPC_CREAT, IPC_EXCL, IPC_NOWAIT];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ctl_commands_distinct() {
        let cmds = [IPC_RMID, IPC_SET, IPC_STAT, IPC_INFO];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        assert_ne!(IPCNS_STATE_ACTIVE, IPCNS_STATE_DYING);
    }
}
