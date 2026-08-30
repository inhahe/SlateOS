//! `<linux/cn_proc.h>` — detailed process connector event structures.
//!
//! Extends `linux_connector` with the actual event payload structures
//! for fork, exec, exit, uid/gid change events. These are delivered
//! as payloads in connector messages to subscribed listeners.

pub use crate::linux_connector::CN_IDX_PROC;
pub use crate::linux_connector::CN_VAL_PROC;
pub use crate::linux_connector::CnMsg;
pub use crate::linux_connector::PROC_CN_MCAST_IGNORE;
pub use crate::linux_connector::PROC_CN_MCAST_LISTEN;
pub use crate::linux_connector::PROC_EVENT_COMM;
pub use crate::linux_connector::PROC_EVENT_COREDUMP;
pub use crate::linux_connector::PROC_EVENT_EXEC;
pub use crate::linux_connector::PROC_EVENT_EXIT;
pub use crate::linux_connector::PROC_EVENT_FORK;
pub use crate::linux_connector::PROC_EVENT_GID;
pub use crate::linux_connector::PROC_EVENT_NONE;
pub use crate::linux_connector::PROC_EVENT_PTRACE;
pub use crate::linux_connector::PROC_EVENT_SID;
pub use crate::linux_connector::PROC_EVENT_UID;

// ---------------------------------------------------------------------------
// Process event header
// ---------------------------------------------------------------------------

/// Process event header (12 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProcEventHeader {
    /// Event type (PROC_EVENT_*).
    pub what: u32,
    /// CPU number.
    pub cpu: u32,
    /// Timestamp (nanoseconds).
    pub timestamp_ns: u64,
}

// Note: The actual ProcEventHeader layout in Linux is just what + cpu +
// timestamp_ns. The event-specific data follows as a union, but since we
// can't safely represent C unions in Rust without knowing which variant
// is active, we provide the header only — callers can parse the remaining
// bytes based on the `what` field.

impl ProcEventHeader {
    /// Create a zeroed event header.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Fork event data
// ---------------------------------------------------------------------------

/// Fork event payload (16 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ForkProcEvent {
    /// Parent PID (thread group leader).
    pub parent_pid: i32,
    /// Parent TGID.
    pub parent_tgid: i32,
    /// Child PID.
    pub child_pid: i32,
    /// Child TGID.
    pub child_tgid: i32,
}

// ---------------------------------------------------------------------------
// Exec event data
// ---------------------------------------------------------------------------

/// Exec event payload (8 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExecProcEvent {
    /// Process PID.
    pub process_pid: i32,
    /// Process TGID.
    pub process_tgid: i32,
}

// ---------------------------------------------------------------------------
// Exit event data
// ---------------------------------------------------------------------------

/// Exit event payload (16 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExitProcEvent {
    /// Process PID.
    pub process_pid: i32,
    /// Process TGID.
    pub process_tgid: i32,
    /// Exit code.
    pub exit_code: u32,
    /// Exit signal.
    pub exit_signal: u32,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_size() {
        assert_eq!(core::mem::size_of::<ProcEventHeader>(), 16);
    }

    #[test]
    fn test_fork_event_size() {
        assert_eq!(core::mem::size_of::<ForkProcEvent>(), 16);
    }

    #[test]
    fn test_exec_event_size() {
        assert_eq!(core::mem::size_of::<ExecProcEvent>(), 8);
    }

    #[test]
    fn test_exit_event_size() {
        assert_eq!(core::mem::size_of::<ExitProcEvent>(), 16);
    }

    #[test]
    fn test_event_reexports() {
        assert_eq!(PROC_EVENT_FORK, 0x00000001);
        assert_eq!(PROC_EVENT_EXEC, 0x00000002);
        assert_eq!(PROC_EVENT_EXIT, 0x80000000);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(CN_IDX_PROC, crate::linux_connector::CN_IDX_PROC);
        assert_eq!(
            PROC_CN_MCAST_LISTEN,
            crate::linux_connector::PROC_CN_MCAST_LISTEN
        );
    }
}
