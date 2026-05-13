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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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

/// Lock pages in memory.
///
/// Stub: succeeds silently.  No kernel page-pinning support yet.
#[unsafe(no_mangle)]
pub extern "C" fn mlock(_addr: *const core::ffi::c_void, _len: SizeT) -> i32 {
    0
}

/// Unlock pages in memory.
///
/// Stub: succeeds silently.
#[unsafe(no_mangle)]
pub extern "C" fn munlock(_addr: *const core::ffi::c_void, _len: SizeT) -> i32 {
    0
}

/// Lock all pages in the process address space.
///
/// Stub: succeeds silently.
#[unsafe(no_mangle)]
pub extern "C" fn mlockall(_flags: i32) -> i32 {
    0
}

/// Unlock all pages.
///
/// Stub: succeeds silently.
#[unsafe(no_mangle)]
pub extern "C" fn munlockall() -> i32 {
    0
}

/// Synchronize a mapped region to its backing store.
///
/// Stub: succeeds silently.  We don't have file-backed mmap yet.
#[unsafe(no_mangle)]
pub extern "C" fn msync(_addr: *mut core::ffi::c_void, _length: SizeT, _flags: i32) -> i32 {
    0
}

/// Give advice about use of memory.
///
/// Stub: succeeds silently.
#[unsafe(no_mangle)]
pub extern "C" fn madvise(_addr: *mut core::ffi::c_void, _length: SizeT, _advice: i32) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// POSIX shared memory objects — stubs
// ---------------------------------------------------------------------------

/// Open a POSIX shared memory object.
///
/// Stub: returns -1 with ENOSYS.  Shared memory between processes
/// requires kernel support for named memory regions.
#[unsafe(no_mangle)]
pub extern "C" fn shm_open(_name: *const u8, _oflag: i32, _mode: ModeT) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Remove a POSIX shared memory object.
///
/// Stub: returns -1 with ENOSYS.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn mremap(
    _old_address: *mut core::ffi::c_void,
    _old_size: SizeT,
    _new_size: SizeT,
    _flags: i32,
) -> *mut core::ffi::c_void {
    errno::set_errno(errno::ENOSYS);
    MAP_FAILED
}
