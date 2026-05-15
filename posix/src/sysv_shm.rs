//! System V shared memory — `<sys/shm.h>`.
//!
//! Stubs for `shmget`, `shmat`, `shmdt`, `shmctl`.
//!
//! Our OS does not implement System V IPC.  These stubs return
//! ENOSYS and satisfy link-time references.  Programs should use
//! POSIX shared memory (`shm_open`, `mmap`) instead.

use crate::errno;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Create if key doesn't exist.
pub const IPC_CREAT: i32 = 0o1000;
/// Fail if key exists.
pub const IPC_EXCL: i32 = 0o2000;

/// Remove identifier.
pub const IPC_RMID: i32 = 0;
/// Set options.
pub const IPC_SET: i32 = 1;
/// Get options.
pub const IPC_STAT: i32 = 2;

/// Private key.
pub const IPC_PRIVATE: i32 = 0;

/// Attach read-only.
pub const SHM_RDONLY: i32 = 0o10000;
/// Round attach address down to `SHMLBA`.
pub const SHM_RND: i32 = 0o20000;
/// Take-over region on attach (remove on last detach).
pub const SHM_REMAP: i32 = 0o40000;
/// Executable mapping.
pub const SHM_EXEC: i32 = 0o100000;

/// Lock pages in memory.
pub const SHM_LOCK: i32 = 11;
/// Unlock pages.
pub const SHM_UNLOCK: i32 = 12;

/// Segment low boundary address multiple (page size).
pub const SHMLBA: usize = 16384; // 16 KiB pages

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// `struct shmid_ds` — shared memory segment data structure.
#[repr(C)]
pub struct ShmidDs {
    /// Owner's UID.
    pub shm_perm_uid: u32,
    /// Owner's GID.
    pub shm_perm_gid: u32,
    /// Creator's UID.
    pub shm_perm_cuid: u32,
    /// Creator's GID.
    pub shm_perm_cgid: u32,
    /// Permissions mode.
    pub shm_perm_mode: u16,
    /// Padding.
    pub _pad: u16,
    /// Segment size in bytes.
    pub shm_segsz: usize,
    /// PID of last shmat/shmdt.
    pub shm_lpid: i32,
    /// PID of creator.
    pub shm_cpid: i32,
    /// Number of current attaches.
    pub shm_nattch: usize,
    /// Last attach time.
    pub shm_atime: i64,
    /// Last detach time.
    pub shm_dtime: i64,
    /// Last change time.
    pub shm_ctime: i64,
}

// ---------------------------------------------------------------------------
// shmget
// ---------------------------------------------------------------------------

/// `shmget` — get a shared memory identifier.
///
/// Stub: always fails with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn shmget(_key: i32, _size: usize, _shmflg: i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// shmat
// ---------------------------------------------------------------------------

/// `shmat` — attach shared memory segment.
///
/// Stub: always fails with ENOSYS and returns `(void *)-1`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn shmat(
    _shmid: i32,
    _shmaddr: *const u8,
    _shmflg: i32,
) -> *mut u8 {
    errno::set_errno(errno::ENOSYS);
    usize::MAX as *mut u8 // (void *)-1
}

// ---------------------------------------------------------------------------
// shmdt
// ---------------------------------------------------------------------------

/// `shmdt` — detach shared memory segment.
///
/// Stub: always fails with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn shmdt(_shmaddr: *const u8) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// shmctl
// ---------------------------------------------------------------------------

/// `shmctl` — shared memory control operations.
///
/// Stub: always fails with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn shmctl(
    _shmid: i32,
    _cmd: i32,
    _buf: *mut ShmidDs,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_ipc_constants() {
        assert_eq!(IPC_CREAT, 0o1000);
        assert_eq!(IPC_EXCL, 0o2000);
    }

    #[test]
    fn test_shm_flags() {
        assert_ne!(SHM_RDONLY, 0);
        assert_ne!(SHM_RND, 0);
        assert_ne!(SHM_REMAP, 0);
        assert_ne!(SHM_EXEC, 0);
    }

    #[test]
    fn test_shm_lock_unlock() {
        assert_eq!(SHM_LOCK, 11);
        assert_eq!(SHM_UNLOCK, 12);
    }

    #[test]
    fn test_shmlba() {
        // Must match our 16 KiB page size.
        assert_eq!(SHMLBA, 16384);
    }

    // -----------------------------------------------------------------------
    // shmget
    // -----------------------------------------------------------------------

    #[test]
    fn test_shmget_enosys() {
        crate::errno::set_errno(0);
        let ret = shmget(0xABCD, 4096, IPC_CREAT | 0o666);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_shmget_private() {
        let ret = shmget(IPC_PRIVATE, 8192, IPC_CREAT | 0o600);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_shmget_zero_size() {
        let ret = shmget(1234, 0, 0);
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // shmat
    // -----------------------------------------------------------------------

    #[test]
    fn test_shmat_enosys() {
        crate::errno::set_errno(0);
        let ret = shmat(0, core::ptr::null(), 0);
        assert_eq!(ret, usize::MAX as *mut u8);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_shmat_rdonly() {
        let ret = shmat(0, core::ptr::null(), SHM_RDONLY);
        assert_eq!(ret, usize::MAX as *mut u8);
    }

    #[test]
    fn test_shmat_with_address() {
        let ret = shmat(0, 0x1000 as *const u8, SHM_RND);
        assert_eq!(ret, usize::MAX as *mut u8);
    }

    // -----------------------------------------------------------------------
    // shmdt
    // -----------------------------------------------------------------------

    #[test]
    fn test_shmdt_enosys() {
        crate::errno::set_errno(0);
        let ret = shmdt(0x1000 as *const u8);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_shmdt_null() {
        let ret = shmdt(core::ptr::null());
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // shmctl
    // -----------------------------------------------------------------------

    #[test]
    fn test_shmctl_stat() {
        crate::errno::set_errno(0);
        let ret = shmctl(0, IPC_STAT, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_shmctl_rmid() {
        let ret = shmctl(0, IPC_RMID, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_shmctl_lock() {
        let ret = shmctl(0, SHM_LOCK, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_shmctl_unlock() {
        let ret = shmctl(0, SHM_UNLOCK, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // Types
    // -----------------------------------------------------------------------

    #[test]
    fn test_shmid_ds_layout() {
        let ds = ShmidDs {
            shm_perm_uid: 1000,
            shm_perm_gid: 1000,
            shm_perm_cuid: 0,
            shm_perm_cgid: 0,
            shm_perm_mode: 0o666,
            _pad: 0,
            shm_segsz: 65536,
            shm_lpid: 42,
            shm_cpid: 1,
            shm_nattch: 2,
            shm_atime: 1000,
            shm_dtime: 0,
            shm_ctime: 500,
        };
        assert_eq!(ds.shm_segsz, 65536);
        assert_eq!(ds.shm_nattch, 2);
        assert_eq!(ds.shm_perm_uid, 1000);
    }

    // -----------------------------------------------------------------------
    // Workflow
    // -----------------------------------------------------------------------

    #[test]
    fn test_full_workflow() {
        // Typical: create → attach → use → detach → remove.
        let shmid = shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600);
        assert_eq!(shmid, -1);

        let ptr = shmat(shmid, core::ptr::null(), 0);
        assert_eq!(ptr, usize::MAX as *mut u8);

        let dt = shmdt(ptr);
        assert_eq!(dt, -1);

        let ctl = shmctl(shmid, IPC_RMID, core::ptr::null_mut());
        assert_eq!(ctl, -1);
    }
}
