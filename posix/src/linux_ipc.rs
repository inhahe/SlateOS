//! `<linux/ipc.h>` — System V IPC constants.
//!
//! Re-exports IPC_* constants from the `sysv_msg` module and adds
//! Linux-specific IPC extensions.

pub use crate::sysv_msg::IPC_CREAT;
pub use crate::sysv_msg::IPC_EXCL;
pub use crate::sysv_msg::IPC_NOWAIT;
pub use crate::sysv_msg::IPC_PRIVATE;
pub use crate::sysv_msg::IPC_RMID;
pub use crate::sysv_msg::IPC_SET;
pub use crate::sysv_msg::IPC_STAT;

// ---------------------------------------------------------------------------
// Linux-specific IPC constants
// ---------------------------------------------------------------------------

/// Get IPC info.
pub const IPC_INFO: i32 = 3;
/// IPC old (compatibility).
pub const IPC_OLD: i32 = 0;
/// IPC 64-bit mode.
pub const IPC_64: i32 = 0x100;

// ---------------------------------------------------------------------------
// IpcPerm struct
// ---------------------------------------------------------------------------

/// IPC permission structure (matching `struct ipc64_perm`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ipc64Perm {
    /// Key.
    pub key: i32,
    /// Owner UID.
    pub uid: u32,
    /// Owner GID.
    pub gid: u32,
    /// Creator UID.
    pub cuid: u32,
    /// Creator GID.
    pub cgid: u32,
    /// Permissions.
    pub mode: u32,
    /// Padding.
    _pad1: u8,
    /// Sequence number.
    pub seq: u16,
    /// Padding.
    _pad2: u8,
    /// Reserved.
    _reserved1: u64,
    /// Reserved.
    _reserved2: u64,
}

impl Ipc64Perm {
    /// Create a zeroed `Ipc64Perm`.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipc_constants() {
        assert_ne!(IPC_CREAT, IPC_EXCL);
        assert_ne!(IPC_RMID, IPC_SET);
        assert_ne!(IPC_SET, IPC_STAT);
    }

    #[test]
    fn test_linux_ipc_constants() {
        assert_eq!(IPC_INFO, 3);
        assert_eq!(IPC_64, 0x100);
    }

    #[test]
    fn test_ipc64_perm_size() {
        assert!(core::mem::size_of::<Ipc64Perm>() >= 36);
    }

    #[test]
    fn test_ipc64_perm_zeroed() {
        let perm = Ipc64Perm::zeroed();
        assert_eq!(perm.key, 0);
        assert_eq!(perm.uid, 0);
        assert_eq!(perm.mode, 0);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(IPC_CREAT, crate::sysv_msg::IPC_CREAT);
        assert_eq!(IPC_PRIVATE, crate::sysv_msg::IPC_PRIVATE);
    }
}
