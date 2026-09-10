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

/// The **C library's** `struct ipc_perm`, which is not the kernel's
/// [`Ipc64Perm`] below it.
///
/// # Two structures, one name, and they are both here on purpose
///
/// `ipc64_perm` is what the `msgctl`/`shmctl` *syscall* moves; `ipc_perm` is
/// what a C program declares and reads. They are the same size (48) and differ
/// inside: the kernel's carries `seq` as an `unsigned short` at 24 with
/// padding either side, the C library's an `int` at 24. Keeping them adjacent
/// is deliberate — the alternative is one type serving both roles, which is
/// how `sigaction` came to have the kernel's field order under a comment
/// claiming it had glibc's (`design-decisions.md` §1010).
///
/// Offsets, measured against musl with `zig cc --target=x86_64-linux-musl`:
/// key 0, uid 4, gid 8, cuid 12, cgid 16, mode 20, seq 24, size 48. The two
/// trailing `long`s are musl's `__pad1`/`__pad2` and are not to be read.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IpcPerm {
    /// `__ipc_perm_key` — the key the segment or queue was created with.
    pub __ipc_perm_key: i32,
    /// Owner UID.
    pub uid: u32,
    /// Owner GID.
    pub gid: u32,
    /// Creator UID.
    pub cuid: u32,
    /// Creator GID.
    pub cgid: u32,
    /// Permission bits. `unsigned int` here, not `unsigned short`: `mode_t`.
    pub mode: u32,
    /// `__ipc_perm_seq` — the slot's reuse counter.
    pub __ipc_perm_seq: i32,
    /// musl's `__pad1`. Public only so that callers in other modules can use
    /// struct-update syntax; the leading underscores are musl's own way of
    /// saying it is not to be read.
    pub __pad1: i64,
    /// musl's `__pad2`. See [`IpcPerm::__pad1`].
    pub __pad2: i64,
}

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
