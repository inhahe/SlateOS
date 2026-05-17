//! `<linux/android/binder.h>` — Android Binder IPC constants.
//!
//! Binder is Android's primary IPC mechanism. Processes communicate
//! via transactions through the /dev/binder driver. A transaction
//! sends a data buffer and optional file descriptor array from one
//! process to another, with the kernel managing reference counting
//! and death notifications. Used by all Android system services.

// ---------------------------------------------------------------------------
// Binder ioctl commands
// ---------------------------------------------------------------------------

/// Write command(s) to the driver.
pub const BINDER_WRITE_READ: u32 = 0xC030_6201;
/// Set maximum number of binder threads.
pub const BINDER_SET_MAX_THREADS: u32 = 0x4004_6205;
/// Set the context manager (servicemanager).
pub const BINDER_SET_CONTEXT_MGR: u32 = 0x4004_6207;
/// Register for thread notifications.
pub const BINDER_THREAD_EXIT: u32 = 0x4004_6208;
/// Get binder version.
pub const BINDER_VERSION: u32 = 0xC004_6209;
/// Get extended binder info (node stats).
pub const BINDER_GET_NODE_INFO_FOR_REF: u32 = 0xC010_620A;
/// Set context manager with security context.
pub const BINDER_SET_CONTEXT_MGR_EXT: u32 = 0x4010_620D;
/// Enable one-way spam detection.
pub const BINDER_ENABLE_ONEWAY_SPAM_DETECTION: u32 = 0x4004_620F;

// ---------------------------------------------------------------------------
// Binder driver return protocol (BR_* commands)
// ---------------------------------------------------------------------------

/// No-op / wakeup.
pub const BR_NOOP: u32 = 0;
/// Transaction from another process.
pub const BR_TRANSACTION: u32 = 1;
/// Reply to a transaction.
pub const BR_REPLY: u32 = 2;
/// Acquire result (unused).
pub const BR_ACQUIRE_RESULT: u32 = 3;
/// Remote binder has died.
pub const BR_DEAD_REPLY: u32 = 4;
/// Transaction complete notification.
pub const BR_TRANSACTION_COMPLETE: u32 = 5;
/// Increment local weak reference.
pub const BR_INCREFS: u32 = 6;
/// Acquire local strong reference.
pub const BR_ACQUIRE: u32 = 7;
/// Release local strong reference.
pub const BR_RELEASE: u32 = 8;
/// Decrement local weak reference.
pub const BR_DECREFS: u32 = 9;
/// Attempt acquire (unused).
pub const BR_ATTEMPT_ACQUIRE: u32 = 10;
/// Death notification of a watched binder.
pub const BR_DEAD_BINDER: u32 = 11;
/// Clear death notification done.
pub const BR_CLEAR_DEATH_NOTIFICATION_DONE: u32 = 12;
/// Failed reply (target died during transaction).
pub const BR_FAILED_REPLY: u32 = 13;
/// Spawn a new looper thread.
pub const BR_SPAWN_LOOPER: u32 = 14;

// ---------------------------------------------------------------------------
// Binder driver write protocol (BC_* commands)
// ---------------------------------------------------------------------------

/// Send a transaction.
pub const BC_TRANSACTION: u32 = 0;
/// Send a reply.
pub const BC_REPLY: u32 = 1;
/// Notify free buffer.
pub const BC_FREE_BUFFER: u32 = 2;
/// Increment weak reference.
pub const BC_INCREFS: u32 = 3;
/// Acquire strong reference.
pub const BC_ACQUIRE: u32 = 4;
/// Release strong reference.
pub const BC_RELEASE: u32 = 5;
/// Decrement weak reference.
pub const BC_DECREFS: u32 = 6;
/// Increment weak ref done.
pub const BC_INCREFS_DONE: u32 = 7;
/// Acquire done.
pub const BC_ACQUIRE_DONE: u32 = 8;
/// Register looper thread.
pub const BC_REGISTER_LOOPER: u32 = 11;
/// Enter looper.
pub const BC_ENTER_LOOPER: u32 = 12;
/// Exit looper.
pub const BC_EXIT_LOOPER: u32 = 13;
/// Request death notification.
pub const BC_REQUEST_DEATH_NOTIFICATION: u32 = 14;
/// Clear death notification.
pub const BC_CLEAR_DEATH_NOTIFICATION: u32 = 15;
/// Dead binder done.
pub const BC_DEAD_BINDER_DONE: u32 = 16;

// ---------------------------------------------------------------------------
// Transaction flags
// ---------------------------------------------------------------------------

/// One-way transaction (no reply expected).
pub const TF_ONE_WAY: u32 = 0x01;
/// Transaction contains a root object.
pub const TF_ROOT_OBJECT: u32 = 0x04;
/// Status code in data (transaction failed remotely).
pub const TF_STATUS_CODE: u32 = 0x08;
/// Accept file descriptors.
pub const TF_ACCEPT_FDS: u32 = 0x10;
/// Clear caller's calling identity.
pub const TF_CLEAR_BUF: u32 = 0x20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            BINDER_WRITE_READ, BINDER_SET_MAX_THREADS,
            BINDER_SET_CONTEXT_MGR, BINDER_THREAD_EXIT,
            BINDER_VERSION, BINDER_GET_NODE_INFO_FOR_REF,
            BINDER_SET_CONTEXT_MGR_EXT,
            BINDER_ENABLE_ONEWAY_SPAM_DETECTION,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_br_commands_distinct() {
        let cmds = [
            BR_NOOP, BR_TRANSACTION, BR_REPLY,
            BR_DEAD_REPLY, BR_TRANSACTION_COMPLETE,
            BR_INCREFS, BR_ACQUIRE, BR_RELEASE, BR_DECREFS,
            BR_DEAD_BINDER, BR_CLEAR_DEATH_NOTIFICATION_DONE,
            BR_FAILED_REPLY, BR_SPAWN_LOOPER,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_bc_commands_distinct() {
        let cmds = [
            BC_TRANSACTION, BC_REPLY, BC_FREE_BUFFER,
            BC_INCREFS, BC_ACQUIRE, BC_RELEASE, BC_DECREFS,
            BC_INCREFS_DONE, BC_ACQUIRE_DONE,
            BC_REGISTER_LOOPER, BC_ENTER_LOOPER, BC_EXIT_LOOPER,
            BC_REQUEST_DEATH_NOTIFICATION,
            BC_CLEAR_DEATH_NOTIFICATION, BC_DEAD_BINDER_DONE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_transaction_flags_distinct() {
        let flags = [
            TF_ONE_WAY, TF_ROOT_OBJECT, TF_STATUS_CODE,
            TF_ACCEPT_FDS, TF_CLEAR_BUF,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
