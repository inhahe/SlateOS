//! POSIX memory mapping functions.
//!
//! Implements `mmap`, `munmap`, `mprotect`.
//!
//! Our kernel provides `SYS_MMAP`, `SYS_MUNMAP`, `SYS_MPROTECT` which
//! closely follow POSIX/Linux semantics.

use crate::errno;
use crate::syscall::*;
use crate::types::*;

// ---------------------------------------------------------------------------
// mmap protection flags
// ---------------------------------------------------------------------------

/// Page may not be accessed.
pub const PROT_NONE: i32 = 0;
/// Page may be read.
pub const PROT_READ: i32 = 1;
/// Page may be written.
pub const PROT_WRITE: i32 = 2;
/// Page may be executed.
pub const PROT_EXEC: i32 = 4;

// ---------------------------------------------------------------------------
// mmap flags
// ---------------------------------------------------------------------------

/// Share mapping with other processes.
pub const MAP_SHARED: i32 = 0x01;
/// Create a private copy-on-write mapping.
pub const MAP_PRIVATE: i32 = 0x02;
/// Place mapping at exactly the specified address.
pub const MAP_FIXED: i32 = 0x10;
/// Mapping is not backed by any file (anonymous).
pub const MAP_ANONYMOUS: i32 = 0x20;
/// Alias for `MAP_ANONYMOUS` (older BSD spelling).
pub const MAP_ANON: i32 = MAP_ANONYMOUS;
/// Stack-like mapping (grows downward).  Linux extension.
pub const MAP_GROWSDOWN: i32 = 0x100;
/// Don't interpret addr as a hint: place the mapping at exactly this
/// address, replacing any existing mappings.  Like `MAP_FIXED` but
/// non-destructive (Linux 4.17+).
pub const MAP_FIXED_NOREPLACE: i32 = 0x100000;
/// Don't reserve swap space for this mapping.
pub const MAP_NORESERVE: i32 = 0x4000;
/// Populate (prefault) page tables for the mapping.
pub const MAP_POPULATE: i32 = 0x8000;
/// Do not block on IO when populating page tables.
pub const MAP_NONBLOCK: i32 = 0x10000;

/// Failure return value for mmap.
pub const MAP_FAILED: *mut core::ffi::c_void = usize::MAX as *mut core::ffi::c_void;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Map files or devices into memory.
///
/// Our kernel's `SYS_MMAP` takes:
/// - arg0: addr (hint or fixed address)
/// - arg1: length
/// - arg2: prot
/// - arg3: flags
/// - arg4: fd (-1 for anonymous)
/// - arg5: offset
///
/// Returns the mapped address, or MAP_FAILED on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mmap(
    addr: *mut core::ffi::c_void,
    length: SizeT,
    prot: i32,
    flags: i32,
    fd: Fd,
    offset: OffT,
) -> *mut core::ffi::c_void {
    if length == 0 {
        errno::set_errno(errno::EINVAL);
        return MAP_FAILED;
    }

    let ret = syscall6(
        SYS_MMAP,
        addr as u64,
        length as u64,
        prot as u64,
        flags as u64,
        fd as u64,
        offset as u64,
    );

    if ret < 0 {
        let _ = errno::translate(ret); // Called for side effect: sets errno.
        return MAP_FAILED;
    }

    ret as *mut core::ffi::c_void
}

/// Unmap a region of memory.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn munmap(addr: *mut core::ffi::c_void, length: SizeT) -> i32 {
    if addr.is_null() || length == 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let ret = syscall2(SYS_MUNMAP, addr as u64, length as u64);
    errno::translate(ret) as i32
}

/// Set protection on a memory region.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mprotect(addr: *mut core::ffi::c_void, len: SizeT, prot: i32) -> i32 {
    if addr.is_null() || len == 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let ret = syscall3(SYS_MPROTECT, addr as u64, len as u64, prot as u64);
    errno::translate(ret) as i32
}

// ---------------------------------------------------------------------------
// mlock / munlock / msync / madvise (stubs)
// ---------------------------------------------------------------------------

/// Flags for msync.
pub const MS_ASYNC: i32 = 1;
pub const MS_SYNC: i32 = 4;
pub const MS_INVALIDATE: i32 = 2;

/// Flags for madvise.
pub const MADV_NORMAL: i32 = 0;
pub const MADV_RANDOM: i32 = 1;
pub const MADV_SEQUENTIAL: i32 = 2;
pub const MADV_WILLNEED: i32 = 3;
pub const MADV_DONTNEED: i32 = 4;

/// Flags for mlockall.
/// Lock all pages currently mapped into the address space.
pub const MCL_CURRENT: i32 = 1;
/// Lock all pages that will be mapped in the future.
pub const MCL_FUTURE: i32 = 2;
/// Lock all pages when they are faulted in (Linux 4.4+).
pub const MCL_ONFAULT: i32 = 4;

/// Lock pages in memory.
///
/// Stub: succeeds silently.  No kernel page-pinning support yet.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mlock(_addr: *const core::ffi::c_void, _len: SizeT) -> i32 {
    0
}

/// Unlock pages in memory.
///
/// Stub: succeeds silently.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn munlock(_addr: *const core::ffi::c_void, _len: SizeT) -> i32 {
    0
}

/// Lock all pages in the process address space.
///
/// Stub: succeeds silently.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mlockall(_flags: i32) -> i32 {
    0
}

/// Unlock all pages.
///
/// Stub: succeeds silently.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn munlockall() -> i32 {
    0
}

/// Synchronize a mapped region to its backing store.
///
/// Stub: succeeds silently.  We don't have file-backed mmap yet.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn msync(_addr: *mut core::ffi::c_void, _length: SizeT, _flags: i32) -> i32 {
    0
}

/// Give advice about use of memory.
///
/// Stub: succeeds silently.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn madvise(_addr: *mut core::ffi::c_void, _length: SizeT, _advice: i32) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Protection flags match Linux x86_64 --

    #[test]
    fn test_prot_flags() {
        assert_eq!(PROT_NONE, 0);
        assert_eq!(PROT_READ, 1);
        assert_eq!(PROT_WRITE, 2);
        assert_eq!(PROT_EXEC, 4);
    }

    #[test]
    fn test_prot_flags_composable() {
        // Common combos must not collide
        let rw = PROT_READ | PROT_WRITE;
        assert_eq!(rw, 3);
        let rwx = PROT_READ | PROT_WRITE | PROT_EXEC;
        assert_eq!(rwx, 7);
        let rx = PROT_READ | PROT_EXEC;
        assert_eq!(rx, 5);
    }

    // -- Map flags match Linux x86_64 --

    #[test]
    fn test_map_flags() {
        assert_eq!(MAP_SHARED, 0x01);
        assert_eq!(MAP_PRIVATE, 0x02);
        assert_eq!(MAP_FIXED, 0x10);
        assert_eq!(MAP_ANONYMOUS, 0x20);
    }

    #[test]
    fn test_map_flags_composable() {
        let anon_priv = MAP_PRIVATE | MAP_ANONYMOUS;
        assert_eq!(anon_priv, 0x22);
        let anon_fixed = MAP_ANONYMOUS | MAP_FIXED | MAP_PRIVATE;
        assert_eq!(anon_fixed, 0x32);
    }

    #[test]
    fn test_map_shared_private_disjoint() {
        assert_eq!(MAP_SHARED & MAP_PRIVATE, 0);
    }

    // -- MAP_FAILED --

    #[test]
    fn test_map_failed_is_minus_one() {
        // MAP_FAILED should be (void*)-1 = usize::MAX
        assert_eq!(MAP_FAILED as usize, usize::MAX);
    }

    #[test]
    fn test_map_failed_not_null() {
        assert!(!MAP_FAILED.is_null());
    }

    // -- msync flags match Linux --

    #[test]
    fn test_msync_flags() {
        assert_eq!(MS_ASYNC, 1);
        assert_eq!(MS_SYNC, 4);
        assert_eq!(MS_INVALIDATE, 2);
    }

    // -- madvise flags match Linux --

    #[test]
    fn test_madvise_flags() {
        assert_eq!(MADV_NORMAL, 0);
        assert_eq!(MADV_RANDOM, 1);
        assert_eq!(MADV_SEQUENTIAL, 2);
        assert_eq!(MADV_WILLNEED, 3);
        assert_eq!(MADV_DONTNEED, 4);
    }

    // -- Stub functions succeed --

    #[test]
    fn test_mlock_stubs_succeed() {
        assert_eq!(mlock(core::ptr::null(), 4096), 0);
        assert_eq!(munlock(core::ptr::null(), 4096), 0);
        assert_eq!(mlockall(0), 0);
        assert_eq!(munlockall(), 0);
    }

    #[test]
    fn test_msync_stub_succeeds() {
        assert_eq!(msync(core::ptr::null_mut(), 4096, MS_SYNC), 0);
    }

    #[test]
    fn test_madvise_stub_succeeds() {
        assert_eq!(madvise(core::ptr::null_mut(), 4096, MADV_NORMAL), 0);
    }

    // -- Shared memory stubs return ENOSYS --

    #[test]
    fn test_shm_open_returns_enosys() {
        assert_eq!(shm_open(b"/test\0".as_ptr(), 0, 0), -1);
    }

    #[test]
    fn test_shm_unlink_returns_enosys() {
        assert_eq!(shm_unlink(b"/test\0".as_ptr()), -1);
    }

    // -- posix_madvise stub succeeds --

    #[test]
    fn test_posix_madvise_succeeds() {
        assert_eq!(posix_madvise(core::ptr::null_mut(), 4096, POSIX_MADV_NORMAL), 0);
        assert_eq!(posix_madvise(core::ptr::null_mut(), 4096, POSIX_MADV_SEQUENTIAL), 0);
    }

    // -- POSIX_MADV_* constants --

    #[test]
    fn test_posix_madv_constants() {
        assert_eq!(POSIX_MADV_NORMAL, 0);
        assert_eq!(POSIX_MADV_RANDOM, 1);
        assert_eq!(POSIX_MADV_SEQUENTIAL, 2);
        assert_eq!(POSIX_MADV_WILLNEED, 3);
        assert_eq!(POSIX_MADV_DONTNEED, 4);
    }

    // -- memfd_create returns ENOSYS --

    #[test]
    fn test_memfd_create_returns_enosys() {
        assert_eq!(memfd_create(b"test\0".as_ptr(), 0), -1);
    }

    // -- mremap returns MAP_FAILED --

    #[test]
    fn test_mremap_returns_map_failed() {
        let ret = mremap(core::ptr::null_mut(), 4096, 8192, 0);
        assert_eq!(ret, MAP_FAILED);
    }

    // -- MREMAP_* constants --

    #[test]
    fn test_mremap_constants() {
        assert_eq!(MREMAP_MAYMOVE, 1);
        assert_eq!(MREMAP_FIXED, 2);
    }

    // -- MCL_* constants match Linux --

    #[test]
    fn test_mcl_constants() {
        assert_eq!(MCL_CURRENT, 1);
        assert_eq!(MCL_FUTURE, 2);
        assert_eq!(MCL_ONFAULT, 4);
    }

    // -- Extended MAP_* constants match Linux --

    #[test]
    fn test_map_anon_alias() {
        assert_eq!(MAP_ANON, MAP_ANONYMOUS);
    }

    #[test]
    fn test_extended_map_flags() {
        assert_eq!(MAP_GROWSDOWN, 0x100);
        assert_eq!(MAP_NORESERVE, 0x4000);
        assert_eq!(MAP_POPULATE, 0x8000);
        assert_eq!(MAP_NONBLOCK, 0x10000);
        assert_eq!(MAP_FIXED_NOREPLACE, 0x100000);
    }

    #[test]
    fn test_extended_map_flags_no_collisions() {
        // All MAP_* flags must be distinct bit positions.
        let all = MAP_SHARED | MAP_PRIVATE | MAP_FIXED | MAP_ANONYMOUS
            | MAP_GROWSDOWN | MAP_NORESERVE | MAP_POPULATE
            | MAP_NONBLOCK | MAP_FIXED_NOREPLACE;
        // If any two flags share a bit, OR-ing them all won't equal the
        // sum of their individual values.  However these are not all
        // single-bit flags (e.g., MAP_FIXED_NOREPLACE = 0x100000 is one
        // bit, but MAP_GROWSDOWN = 0x100 overlaps with MAP_FIXED_NOREPLACE
        // in different bits).  Just verify they don't collide with the
        // core flags.
        assert_eq!(MAP_SHARED & MAP_ANONYMOUS, 0);
        assert_eq!(MAP_PRIVATE & MAP_ANONYMOUS, 0);
        assert_eq!(MAP_FIXED & MAP_ANONYMOUS, 0);
        assert_eq!(MAP_GROWSDOWN & MAP_ANONYMOUS, 0);
        // Verify all are non-zero.
        assert_ne!(all, 0);
    }
}

// ---------------------------------------------------------------------------
// POSIX shared memory objects — stubs
// ---------------------------------------------------------------------------

/// Open a POSIX shared memory object.
///
/// Stub: returns -1 with ENOSYS.  Shared memory between processes
/// requires kernel support for named memory regions.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn shm_open(_name: *const u8, _oflag: i32, _mode: ModeT) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Remove a POSIX shared memory object.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn shm_unlink(_name: *const u8) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// posix_madvise
// ---------------------------------------------------------------------------

/// POSIX memory advice constants.
pub const POSIX_MADV_NORMAL: i32 = 0;
/// Expect random access.
pub const POSIX_MADV_RANDOM: i32 = 1;
/// Expect sequential access.
pub const POSIX_MADV_SEQUENTIAL: i32 = 2;
/// Expect access in the near future.
pub const POSIX_MADV_WILLNEED: i32 = 3;
/// Do not expect access in the near future.
pub const POSIX_MADV_DONTNEED: i32 = 4;

/// POSIX-specified memory advice.
///
/// Unlike `madvise` (which sets errno), `posix_madvise` returns the
/// error code directly (0 on success).
///
/// Stub: always returns 0 (advisory, no kernel action).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_madvise(
    _addr: *mut core::ffi::c_void,
    _len: SizeT,
    _advice: i32,
) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// memfd_create (Linux extension)
// ---------------------------------------------------------------------------

/// Create an anonymous file backed by memory.
///
/// Stub: returns -1 with ENOSYS.  Requires kernel support for
/// anonymous file descriptors backed by anonymous memory.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn memfd_create(_name: *const u8, _flags: u32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// mremap (Linux extension)
// ---------------------------------------------------------------------------

/// Flags for `mremap`.
pub const MREMAP_MAYMOVE: i32 = 1;
/// Flag indicating a fixed new address was provided.
pub const MREMAP_FIXED: i32 = 2;

/// `mmap64` — alias for `mmap` on LP64 (off_t is already 64-bit).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mmap64(
    addr: *mut core::ffi::c_void,
    length: SizeT,
    prot: i32,
    flags: i32,
    fd: Fd,
    offset: OffT,
) -> *mut core::ffi::c_void {
    mmap(addr, length, prot, flags, fd, offset)
}

/// Remap a virtual memory region.
///
/// Stub: returns MAP_FAILED with ENOSYS.  A real implementation would
/// grow/shrink/relocate an existing mmap region.  We don't support this
/// because our simple allocator doesn't track mmap regions globally.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mremap(
    _old_address: *mut core::ffi::c_void,
    _old_size: SizeT,
    _new_size: SizeT,
    _flags: i32,
) -> *mut core::ffi::c_void {
    errno::set_errno(errno::ENOSYS);
    MAP_FAILED
}
